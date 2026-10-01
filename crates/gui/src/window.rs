//! The regular-window frontend.
//!
//! The runtime shape mirrors `wowdps-tui`: a 100 ms tick drains the daemon
//! client's inbox (stale snapshots were already coalesced away) and feeds the
//! shared [`ClientState`]; this module only translates iced events into
//! `Action`s and draws.

use std::time::{Duration, Instant};

use iced::{Subscription, Task, Theme, keyboard, time, window};

use wowdps_model::{Action, ListRow, Screen, SegmentId, View};
use wowdps_proto::{
    ClientKind, ClientMsg, ClientState, DaemonClient, DaemonMsg, HistoryAnswer, Reconnect,
};

use crate::config::Config;
use crate::history;
use crate::home;
use crate::keys;
use crate::palette;
use crate::rail::{self, Pull};
use crate::talents;
use crate::theme;
use crate::view;

/// Redraw/drain cadence. Live durations tick at this rate.
pub(crate) const TICK: Duration = Duration::from_millis(100);

/// Least time between two `GetStatus` asks off the store-changed path.
const STATUS_REFRESH: Duration = Duration::from_secs(5);

/// How long a toast stays up (the prototype's `toast()`: 2.6 s).
const TOAST_FOR: Duration = Duration::from_millis(2_600);

/// What `p` says on a pull the store holds no card of (yet).
pub(crate) const NO_CARD: &str =
    "No stored card for this pull yet: the store writes one when it ends";
/// What the store's answer to `p` says.
pub(crate) const PINNED: &str = "Pinned: retention keeps this pull";
pub(crate) const UNPINNED: &str = "Unpinned: retention may remove this pull";
/// What a stored pull says of what the store keeps no answer for: a
/// comparison, and an ability's own curve (the enemies' view says
/// `view::NOT_STORED`).
pub(crate) const NO_STORED_PAIR: &str = "The history store keeps no comparison";
pub(crate) const NO_STORED_ABILITY: &str = "The history store keeps no ability's own curve";

const ZOOM_STEP: f32 = 0.1;
const ZOOM_RANGE: std::ops::RangeInclusive<f32> = 0.5..=3.0;

/// The window's iced settings — its fonts, default font and text size. One
/// function so the design-shot harness (`window::shots`) renders with
/// exactly what the running window does. The fonts are the window's alone
/// (`theme::FONTS`): the overlay's settings load none.
pub(crate) fn settings() -> iced::Settings {
    iced::Settings {
        fonts: theme::FONTS
            .into_iter()
            .map(std::borrow::Cow::Borrowed)
            .collect(),
        default_font: theme::UI,
        ..iced::Settings::default()
    }
}

pub fn run(cfg: Config) -> Result<(), String> {
    // Connect before iced takes over, so a missing daemon is a clean CLI
    // error, not a blank window. The factory is `Fn` but runs once; the
    // fallback reconnect covers the theoretical second call.
    let first = std::sync::Mutex::new(Some(connect()?));
    iced::application(
        move || {
            let handoff = first.lock().ok().and_then(|mut slot| slot.take());
            let client = handoff.unwrap_or_else(|| reconnect_forever(ClientKind::Window));
            Gui::new(client, cfg.clone())
        },
        update,
        view::view,
    )
    .settings(settings())
    .title(title)
    .subscription(subscription)
    .theme(theme)
    .style(style)
    .scale_factor(|state| state.cfg.zoom)
    .window(window::Settings {
        size: iced::Size::new(460.0, 640.0),
        min_size: Some(iced::Size::new(320.0, 240.0)),
        // The compositor must see the surface as alpha-capable, or the
        // translucent background composites against black instead of the
        // desktop (see `style`).
        transparent: true,
        ..window::Settings::default()
    })
    .run()
    .map_err(|e| e.to_string())
}

/// Connect, spawning the daemon if none is running. There is no embedded
/// fallback: no daemon, no meter. The kind matters: `Overlay` is how the
/// daemon's supervisor recognizes the session it manages (`SetVisible`,
/// never-spawn-a-second-overlay, failure clearing).
pub(crate) fn connect_as(kind: ClientKind) -> Result<DaemonClient, String> {
    DaemonClient::connect(&crate::daemon_bin(), None, kind)
        .map_err(|e| format!("cannot reach the wowdps daemon: {e}"))
}

pub(crate) fn connect() -> Result<DaemonClient, String> {
    connect_as(ClientKind::Window)
}

/// Fallback for the state factory's theoretical second call: it must yield a
/// client, and a rendering client without one has nothing to show. Retrying
/// beats aborting — the overlay in particular is supervised, and a crash
/// mid-raid is visible. Unreachable on the normal path (the first connection
/// is handed off).
pub(crate) fn reconnect_forever(kind: ClientKind) -> DaemonClient {
    loop {
        match connect_as(kind) {
            Ok(c) => return c,
            Err(e) => {
                eprintln!("wowdps-gui: {e}; retrying");
                std::thread::sleep(Duration::from_millis(500));
            }
        }
    }
}

pub(crate) struct Gui {
    pub(crate) state: ClientState,
    client: DaemonClient,
    /// When the last snapshot arrived, wall-clock. WoW buffers its log
    /// writes (sometimes for a long while), so the meter shows how far
    /// behind the file is instead of silently looking frozen.
    last_snapshot_at: Option<Instant>,
    pub(crate) cfg: Config,
    /// The ⚙ options panel is open.
    pub(crate) options_open: bool,
    /// The talent viewer, when open — a window-local screen over the
    /// shared `ClientState` machine, which never learns about it.
    pub(crate) talents: Option<talents::TalentsUi>,
    /// v19: the in-flight `GetLoadout`'s req_id, matched against `Loadout`
    /// replies. A reply for anything else (a closed viewer, a superseded
    /// open) is dropped.
    pending_loadout: Option<u32>,
    next_req_id: u32,
    /// The Home dashboard when open — window-local like `talents`, so the
    /// shared state machine never learns it exists.
    pub(crate) home: Option<home::Home>,
    /// Derived once per answer, not once per frame.
    pub(crate) home_panels: home::Panels,
    /// The season window Home scopes itself to, read from the config once.
    pub(crate) season: home::Season,
    /// When the window last asked for `Status`, so a burst of stored fights
    /// does not become a burst of one-shots.
    last_status_at: Option<Instant>,
    /// Whether Home has been offered at startup yet. The offer needs the
    /// first snapshot (to know whether a pull is live), and must happen once.
    home_considered: bool,
    /// Liveness at the previous drain, so a pull STARTING can dismiss Home
    /// without a live fight holding it shut forever.
    was_live: bool,
    /// From the daemon's `Status`: why the history store is off, when it is.
    pub(crate) history_disabled: Option<String>,
    /// From `Status`: requests the store dropped. Home says so rather than
    /// presenting a partial answer as the whole story.
    pub(crate) history_dropped: u32,
    /// The chrome accent: the game's gold, or — `chrome = "class"` — the
    /// OWNER's class colour, resolved once and then held. It answers "whose
    /// window is this", so it must not move when the meter resorts, the
    /// view changes or the selection does. Gold needs nobody, and the class
    /// is remembered in the config, so either is right on the first frame.
    pub(crate) accent: theme::Accent,
    /// Who the owner was resolved to — `Some` means stop looking. Until
    /// then a class chrome wears the remembered class, else
    /// [`theme::NEUTRAL`]: borrowing whichever row is selected would make
    /// the window's color a property of the cursor.
    accent_owner: Option<String>,
    /// The owner's class: from the config at launch, then from whoever
    /// resolves. What a class chrome is drawn in.
    owner_class: Option<(wowdps_model::Class, Option<wowdps_model::Spec>)>,
    /// The `?` sheet is up.
    pub(crate) shortcuts_open: bool,
    /// The meter row filter's text, applied client-side at render time.
    pub(crate) filter: String,
    /// The row the pointer is over, if any — drawn, never sent anywhere.
    pub(crate) row_hover: Option<RowHover>,
    /// R12: the by-spell key the pointer is over in a comparison table, so
    /// the other side's table can light the same ability.
    pub(crate) spell_hover: Option<String>,
    /// The filter field has focus: while true the meter keymap is swallowed
    /// so the field is typable — the same trick the talent viewer uses, and
    /// the reason typing "q" into it does not quit the app.
    pub(crate) filter_focused: bool,
    /// The meter's sort: a column and whether it is descending. `None` is
    /// the daemon's own order (by amount, teams grouped).
    pub(crate) sort: Option<(crate::table::Col, bool)>,
    /// The drill's by-spell pane sort, the same way.
    pub(crate) drill_sort: Option<(crate::table::Col, bool)>,
    /// R26: the ability tree's open folds, by `inspector::tree` fold key
    /// (a group's, or a row's parts') — shut until opened, kept across
    /// players and pulls so a group opened once stays open.
    pub(crate) tree_open: std::collections::HashSet<String>,
    /// R26: the tree line the keys rest on when it is not a plain row (a
    /// group, a part), with the drilled player it belongs to; a row's own
    /// line is the drill's `spell_sel`, as before the tree.
    pub(crate) tree_cursor: Option<(String, crate::inspector::tree::Node)>,
    /// R26 (step 2): the drill graph stacks by ability (or, with an ability
    /// open, by target) — on until the reader asks for the total alone.
    pub(crate) stack_graph: bool,
    /// R26 (step 2): which hue each stacked curve wears, seated once per
    /// curve so a live re-sort never repaints it.
    pub(crate) stack_slots: crate::inspector::stack::Slots,
    /// R21: the Taken drill shows its stack matrix instead of the panes.
    pub(crate) stacks_open: bool,
    /// A stored pull on the stage, when the reader picked one from the
    /// rail (or Home): the pull's own `ClientState`, fed from the history
    /// store. `None` is the tailed log's pull, `state`.
    pub(crate) stored: Option<history::Stored>,
    /// The store's pages the rail's earlier nights are made from.
    pub(crate) earlier: history::Earlier,
    /// The rail's drawer is open (at 1180 px and under, where the rail is
    /// not beside the stage).
    pub(crate) rail_open: bool,
    /// The rail leaves trash out (`.toggle` "Hide trash").
    pub(crate) hide_trash: bool,
    /// The drawer's keyboard highlight: the row j, k and the arrows walk
    /// and Enter opens, where the drawer has the keys.
    pub(crate) rail_cursor: Option<Pull>,
    /// A `GetFight` a stored pull the reader left still has out: the next
    /// stored pull waits for its answer before it asks, so the window has
    /// one read in the daemon's queue however fast the rail is walked.
    stray_fight: Option<u32>,
    /// The command palette, while it is up (Ctrl K, the jump box): what is
    /// typed into it and its selection. Window-local like the sheet; while
    /// it is up every key is its own.
    pub(crate) palette: Option<palette::Palette>,
    /// The night the rail calls "Tonight", when a test pins it — the test
    /// seam over the clock ([`Gui::tonight`]); a running window has none.
    #[cfg(test)]
    pub(crate) tonight: Option<i64>,
    /// The view the reader was on in the log when they stepped onto a
    /// stored pull that lacks it (the enemies'): a stored pull shows
    /// Damage in its place, and a step back onto the log's pulls restores
    /// it — unless the reader chose another view since.
    log_view: Option<View>,
    /// Whose window this is: the character the store's newest card was
    /// played on, as Home's answers (else the rail's pages) name them — held
    /// after Home closes, so the top bar's picker shows them and the "you"
    /// on a meter the daemon marks nobody on falls back to them. Never a
    /// scope: Home's chips change what Home shows, not who the reader is.
    pub(crate) owner_guid: Option<String>,
    /// Every character the store has shown the window you play, from any
    /// Home answer or rail page — what the top bar's picker offers.
    pub(crate) known_characters: Vec<home::CharLine>,
    /// The character picker's menu is up (over Home's title or the top
    /// bar). Window-local like the sheet; Esc or a press away closes it.
    pub(crate) picker_open: bool,
    /// The menu row the pointer is over — drawn, never sent anywhere.
    pub(crate) picker_hover: Option<usize>,
    /// What the fight header knows of the watched fight from views other
    /// than the one on screen: the owner's presence in it.
    pub(crate) seen: crate::fight_head::Seen,
    /// Everyone the window has seen on a meter, by guid — who cast the
    /// external on the inspector's lanes, in their class colour.
    pub(crate) roster: crate::inspector::Roster,
    /// The inspector's body as it last stood for an answered player,
    /// standing in (dimmed) while the next one's breakdown is on its way.
    pub(crate) insp_held: Option<crate::inspector::Held>,
    /// R25: a death just opened (its player's key and window) whose recap
    /// has yet to land: when it does, the inspector brings its killing blow
    /// into sight — the recap can run past the fold, and the blow ends it.
    reveal_death: Option<(String, u32)>,
    /// A player the palette selected whose row the chart in hand lacked (a
    /// count view gave way to Damage, whose rows are on their way): the
    /// snapshot that brings their row scrolls the meter to it, as the
    /// palette promised.
    pub(crate) reveal_player: Option<String>,
    /// A passing word over the stage (`.toast`) and when it was said: `v`
    /// confirms a pin — the meter's "A" is small, and a narrow window's
    /// button that says so may be off screen. Gone after [`TOAST_FOR`],
    /// or as soon as the pair it asked for forms.
    pub(crate) toast: Option<(String, Instant)>,
    /// The window's logical width at zoom 1, from its own open and resize
    /// events — `None` until the first arrives. What the keys ask before
    /// doing something only the drawn layout would show (a narrow window
    /// pushes the inspector; a wide one has it beside the meter).
    pub(crate) window_w: Option<f32>,
}

/// Where a window-side `Up`/`Down` lands when the drawn order is not the
/// state machine's: on a meter row, or on a row of the drill's spell pane.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Step {
    Meter(usize),
    Spell(usize),
    /// R25: a death of the Deaths table, by its place in the raid's deaths.
    Death(usize),
    /// R26: a line of the inspector's ability tree.
    Tree(crate::inspector::tree::Node),
    /// Nowhere drawn to land (the filter hides every row): the key does
    /// nothing.
    Stay,
}

use wowdps_gui_logic::table::step_in;

/// Scrolls the scrollable `id` the least that brings `[top, bottom]` of its
/// content whole into sight, and leaves it be when that already is: what
/// keeps a stepped selection on screen without moving a list the reader
/// can already see it in.
pub(crate) struct RevealSpan {
    pub id: iced::widget::Id,
    pub top: f32,
    pub bottom: f32,
}

impl RevealSpan {
    /// Where the list should stand, from where it stands (`offset`, its
    /// content's scroll) in a viewport `height` tall: `None` to stay.
    pub(crate) fn offset(&self, offset: f32, height: f32) -> Option<f32> {
        let to = crate::reveal::nearest(offset, height, self.top, self.bottom);
        (to != offset).then_some(to)
    }
}

impl iced::advanced::widget::Operation for RevealSpan {
    fn traverse(&mut self, operate: &mut dyn FnMut(&mut dyn iced::advanced::widget::Operation)) {
        operate(self);
    }

    fn scrollable(
        &mut self,
        id: Option<&iced::widget::Id>,
        bounds: iced::Rectangle,
        _content: iced::Rectangle,
        translation: iced::Vector,
        state: &mut dyn iced::advanced::widget::operation::Scrollable,
    ) {
        if id != Some(&self.id) {
            return;
        }
        if let Some(y) = self.offset(translation.y, bounds.height) {
            state.scroll_to(iced::widget::operation::AbsoluteOffset {
                x: None,
                y: Some(y),
            });
        }
    }
}

/// Finds where the widget `row` stands in the content of the scrollable
/// `scroll`, then hands that span to a [`RevealSpan`] — for a row whose
/// place in its list's content only the layout knows (the inspector's list
/// stands under sections of every height). Nothing when either is absent.
pub(crate) struct FindRow {
    pub scroll: iced::widget::Id,
    pub row: iced::widget::Id,
    /// The scrollable's content bounds, once visited.
    pub content: Option<iced::Rectangle>,
    /// The row's bounds, once visited.
    pub found: Option<iced::Rectangle>,
}

impl FindRow {
    /// The reveal the two bounds make: the row's span in the content's own
    /// coordinates.
    pub(crate) fn reveal(&self) -> Option<RevealSpan> {
        let (content, row) = (self.content?, self.found?);
        Some(RevealSpan {
            id: self.scroll.clone(),
            top: row.y - content.y,
            bottom: row.y + row.height - content.y,
        })
    }
}

impl iced::advanced::widget::Operation for FindRow {
    fn traverse(&mut self, operate: &mut dyn FnMut(&mut dyn iced::advanced::widget::Operation)) {
        operate(self);
    }

    fn container(&mut self, id: Option<&iced::widget::Id>, bounds: iced::Rectangle) {
        if id == Some(&self.row) {
            self.found = Some(bounds);
        }
    }

    fn scrollable(
        &mut self,
        id: Option<&iced::widget::Id>,
        _bounds: iced::Rectangle,
        content: iced::Rectangle,
        _translation: iced::Vector,
        _state: &mut dyn iced::advanced::widget::operation::Scrollable,
    ) {
        if id == Some(&self.scroll) {
            self.content = Some(content);
        }
    }

    fn finish(&self) -> iced::advanced::widget::operation::Outcome<()> {
        match self.reveal() {
            Some(span) => iced::advanced::widget::operation::Outcome::Chain(Box::new(span)),
            None => iced::advanced::widget::operation::Outcome::None,
        }
    }
}

pub(crate) use wowdps_gui_logic::fight_head::owner_among;

impl Gui {
    fn new(mut client: DaemonClient, cfg: Config) -> Self {
        let mut state = ClientState::new();
        // Master and detail: the inspector beside the meter follows the
        // selection. The window's opt-in; the TUI never makes it. On the
        // list screen a launch starts on, it asks for nothing yet.
        let _ = state.set_follow(true);
        client.send(&state.initial_request());
        // Home renders three different empty screens (off / cold / degraded)
        // and only `Status` tells them apart, so ask once at startup rather
        // than when Home opens: the answer is tiny and always wanted.
        client.send(&wowdps_proto::ClientMsg::GetStatus { req_id: 0 });
        // The rail lists the store's nights under tonight's from the first
        // frame: its newest page, asked for once, now.
        let mut earlier = history::Earlier::default();
        earlier.want_newest();
        if let Some(msg) = earlier.next_request(1, Instant::now()) {
            client.send(&msg);
        }
        let season = home::Season::from_config(&cfg);
        // The first frame's chrome, from the config alone: gold, or the
        // class remembered for the character played last.
        let owner_class = cfg.character_class().map(|c| (c, None));
        let accent = chrome_accent(cfg.chrome(), owner_class);
        Self {
            state,
            client,
            last_snapshot_at: None,
            cfg,
            options_open: false,
            talents: None,
            pending_loadout: None,
            next_req_id: 2,
            home: None,
            home_panels: home::Panels::default(),
            season,
            history_disabled: None,
            history_dropped: 0,
            last_status_at: None,
            home_considered: false,
            was_live: false,
            accent,
            accent_owner: None,
            owner_class,
            shortcuts_open: false,
            filter: String::new(),
            filter_focused: false,
            row_hover: None,
            spell_hover: None,
            sort: None,
            drill_sort: None,
            tree_open: std::collections::HashSet::new(),
            tree_cursor: None,
            stack_graph: true,
            stack_slots: crate::inspector::stack::Slots::default(),
            stacks_open: false,
            stored: None,
            earlier,
            rail_open: false,
            hide_trash: false,
            rail_cursor: None,
            stray_fight: None,
            palette: None,
            #[cfg(test)]
            tonight: None,
            log_view: None,
            // Nobody until the store says: the config's `character` is Home's
            // scope, and names no one as the reader.
            owner_guid: None,
            known_characters: Vec::new(),
            picker_open: false,
            picker_hover: None,
            seen: crate::fight_head::Seen::default(),
            roster: crate::inspector::Roster::default(),
            insp_held: None,
            reveal_death: None,
            reveal_player: None,
            toast: None,
            window_w: None,
        }
    }

    /// Which surface is showing — what the `?` sheet keys its "here" column
    /// on. Window-local screens sit over the state machine's, so they win.
    pub(crate) fn surface(&self) -> keys::Surface {
        let app = self.fight();
        if self.talents.is_some() {
            keys::Surface::Talents
        } else if self.drawer_open() {
            // The drawer is over whatever it was opened on, and has the
            // keys: its own list's.
            keys::Surface::Rail
        } else if self.home.is_some() {
            keys::Surface::Home
        } else {
            match app.screen {
                Screen::Compare => keys::Surface::Compare,
                _ if app.drill_spell().is_some() => keys::Surface::Ability,
                // The inspector is beside the meter; it is the surface the
                // keys work on once Enter gave them to it.
                _ if app.inspecting() => keys::Surface::Drill,
                // The window draws no fight list: a stage with no pull on
                // it yet is the meter, waiting.
                Screen::Meter | Screen::List => keys::Surface::Meter,
            }
        }
    }

    /// The pull on the stage: a stored pull's own state while one is open,
    /// else the tailed log's. What every renderer of a pull reads, so one
    /// set of them draws both.
    pub(crate) fn fight(&self) -> &ClientState {
        self.stored.as_ref().map_or(&self.state, |s| &s.state)
    }

    fn fight_mut(&mut self) -> &mut ClientState {
        match self.stored.as_mut() {
            Some(s) => &mut s.state,
            None => &mut self.state,
        }
    }

    /// Run `f` on the stage's pull and send what it asks for where it is
    /// answered: the tailed log's `Watch` to the daemon as it is, a stored
    /// pull's as the `GetFight` that answers it.
    fn on_fight(&mut self, f: impl FnOnce(&mut ClientState) -> Vec<ClientMsg>) -> Vec<ClientMsg> {
        match self.stored.as_mut() {
            Some(s) => {
                let sent = f(&mut s.state);
                s.route(sent, &mut self.next_req_id)
            }
            None => f(&mut self.state),
        }
    }

    /// The pull rail as the window stands: tonight's log, and the store's
    /// pages under it.
    pub(crate) fn rail(&self) -> rail::Rail {
        let app = &self.state;
        rail::Rail::build(&rail::Sources {
            entries: app.entries(),
            log_id: app.log_id(),
            watched: self.watched_row(),
            cards: &self.earlier.cards,
            owner: self
                .owner_class
                .map(|(class, _)| (self.owner_name().map(str::to_string), class)),
            tonight: self.tonight(),
        })
    }

    /// The log segment the tailed log's state watches, as its snapshot has
    /// it now — the list's row with the snapshot's verdict, clock and
    /// liveness, which move before the list does.
    fn watched_row(&self) -> Option<(SegmentId, ListRow)> {
        let app = &self.state;
        if app.screen == Screen::List || app.segment_name().is_none() {
            return None;
        }
        let e = app.entries().get(app.segment_index())?;
        Some((
            e.id,
            ListRow {
                success: app.segment_success(),
                duration_ms: app.duration_ms(),
                live: app.is_live(),
                ..e.row.clone()
            },
        ))
    }

    /// What the rail draws beside its model: the pull on the stage (none
    /// while Home is), the trash toggle, and whether more of the store can
    /// be asked for.
    pub(crate) fn rail_shown(&self) -> rail::Shown {
        rail::Shown {
            at: self.home.is_none().then(|| self.current_pull()).flatten(),
            // The keys' highlight, where the drawer has them.
            cursor: self
                .drawer_open()
                .then(|| self.rail_cursor.clone())
                .flatten(),
            hide_trash: self.hide_trash,
            more: if self.earlier.asking() {
                rail::More::Asking
            } else if self.earlier.more() {
                rail::More::Offer
            } else {
                rail::More::None
            },
            accent: self.accent,
            hide_realms: self.cfg.hide_realms,
        }
    }

    /// The night "Tonight" is: the clock's, in the timezone the store's
    /// newest card was logged in (UTC with none) — or the one a test pinned.
    pub(crate) fn tonight(&self) -> i64 {
        #[cfg(test)]
        if let Some(night) = self.tonight {
            return night;
        }
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_millis() as i64);
        rail::tonight(now, self.earlier.cards.first().and_then(|c| c.tz_min))
    }

    /// The pull the stage stands on: the stored one, else the log segment
    /// the tailed log's state watches — none before it watches any.
    pub(crate) fn current_pull(&self) -> Option<Pull> {
        if let Some(s) = &self.stored {
            return Some(Pull::Stored(s.fight_id.clone()));
        }
        let app = &self.state;
        if app.screen == Screen::List {
            return None;
        }
        app.entries()
            .get(app.segment_index())
            .map(|e| Pull::Log(e.id))
    }

    /// Is there a newer pull up the rail, and an older one down it — or
    /// more of the store to ask for past its end? The header's steps.
    pub(crate) fn pull_steps(&self) -> (bool, bool) {
        let rail = self.rail();
        let at = self.current_pull();
        let newer = at.is_some() && rail.step(at.as_ref(), false, self.hide_trash).is_some();
        let older = rail.step(at.as_ref(), true, self.hide_trash).is_some()
            || (self.earlier.answered && self.earlier.more());
        (newer, older)
    }

    /// Is the rail beside the stage, at the window's width? Unknown counts
    /// as a drawer: the launch size is narrow.
    fn rail_docked(&self) -> bool {
        self.window_w
            .is_some_and(|w| rail::docked(w / self.cfg.zoom.max(f32::EPSILON)))
    }

    /// `[` (older) or `]`: the next pull along the rail, stored nights
    /// included. Past the store's last card in hand, `[` asks for the next
    /// page, and the next press goes on into it. From Home, where the rail
    /// lights no row, either starts at the rail's top: a step from the
    /// hidden stage's pull would land somewhere relative to a row the
    /// reader cannot see.
    fn step_pull(&mut self, older: bool, requests: &mut Vec<ClientMsg>) {
        let at = self.home.is_none().then(|| self.current_pull()).flatten();
        match self.rail().step(at.as_ref(), older, self.hide_trash) {
            Some(pull) => self.go_pull(pull, requests),
            None if older => {
                self.earlier.want_older();
                self.ask_earlier(requests);
            }
            None => {}
        }
    }

    /// Put `pull` on the stage — a log segment through the tailed log's
    /// state, a stored pull through one of its own — and leave Home for it.
    /// The player being inspected, and the view, go with the reader: the
    /// next pull is most likely theirs too. The drawer is the caller's: a
    /// pick closes it, a step of the keys it has leaves it open on the row
    /// it stepped to.
    fn go_pull(&mut self, pull: Pull, requests: &mut Vec<ClientMsg>) {
        self.home = None;
        // A stored fight that is one of the tailed log's own pulls (Home's
        // recent list names tonight's too) opens as the log's: the rail
        // lists it there, and the log answers what the store cannot.
        let pull = match pull {
            Pull::Stored(id) => self.log_segment_of(&id).map_or(Pull::Stored(id), Pull::Log),
            log => log,
        };
        self.rail_cursor = Some(pull.clone());
        if self.current_pull().as_ref() == Some(&pull) {
            return;
        }
        let (view, drill) = {
            let f = self.fight();
            (f.view, f.drill.clone())
        };
        self.stacks_open = false;
        match pull {
            Pull::Log(id) => {
                // Back from the store onto the log: the view the log was on,
                // where a stored pull only stood in for it.
                let view = match self.leave_stored() {
                    true => self.log_view.take().unwrap_or(view),
                    false => view,
                };
                self.log_view = None;
                let app = &mut self.state;
                let Some(pos) = app.entries().iter().position(|e| e.id == id) else {
                    return;
                };
                let here = app.screen != Screen::List
                    && app.entries().get(app.segment_index()).map(|e| e.id) == Some(id);
                if here {
                    // Back from a stored pull onto the one the log's state
                    // still watches: only the view has to follow.
                    if app.view != view {
                        requests.extend(app.apply(Action::SetView(view)));
                    }
                    return;
                }
                app.view = view;
                if drill.is_some() {
                    app.drill = drill;
                }
                requests.extend(app.goto_list_pos(pos));
            }
            Pull::Stored(id) => {
                // Off the log onto a view the store does not keep: the stored
                // pull shows Damage, and the log's view waits for the step back.
                if self.stored.is_none() && !view.is_stored() {
                    self.log_view = Some(view);
                }
                let card = self.earlier.card(&id).cloned();
                // The read the pull left behind is still in the daemon's
                // queue: this one waits for its answer.
                self.leave_stored();
                let inherit = self.stray_fight.take();
                let (stored, sent) =
                    history::Stored::open(id, card, view, drill, inherit, &mut self.next_req_id);
                self.stored = Some(stored);
                requests.extend(sent);
            }
        }
    }

    /// Take the stored pull off the stage — `true` when there was one —
    /// keeping the read it still has out, for the next stored pull to wait
    /// on.
    fn leave_stored(&mut self) -> bool {
        let Some(s) = self.stored.take() else {
            return false;
        };
        if let Some(id) = s.in_flight() {
            self.stray_fight = Some(id);
        }
        true
    }

    /// The stored card of the pull on the stage: a stored pull's own, or —
    /// a pull of the log — the card the store wrote for it, paired as the
    /// rail pairs them, when the rail's pages hold it.
    pub(crate) fn stage_card(&self) -> Option<&wowdps_proto::history::FightCard> {
        if let Some(s) = &self.stored {
            return s.card.as_ref().or_else(|| self.earlier.card(&s.fight_id));
        }
        let app = &self.state;
        if app.screen == Screen::List {
            return None;
        }
        let log = app.log_id()?;
        let e = app.entries().get(app.segment_index())?;
        let sigma = e.row.kind == wowdps_model::SegmentKind::Overall;
        let id = wowdps_proto::history::fight_id(log, e.row.start_ms, sigma);
        self.earlier.card(&id)
    }

    /// The stored card of the pull on the stage, by its id, and whether it
    /// is pinned ([`Gui::stage_card`]). What `p` pins; what the header's
    /// star says.
    pub(crate) fn pin_target(&self) -> Option<(String, bool)> {
        if let Some(s) = &self.stored {
            let card = self.stage_card()?;
            return Some((s.fight_id.clone(), card.pinned));
        }
        self.stage_card().map(|c| (c.id.clone(), c.pinned))
    }

    /// `p`: pin the pull on the stage — or let it go — so retention keeps
    /// it. A pull the store holds no card of says so rather than nothing.
    fn pin(&mut self, requests: &mut Vec<ClientMsg>) {
        match self.pin_target() {
            Some((fight_id, pinned)) => {
                let req_id = self.next_req_id();
                requests.push(ClientMsg::PinFight {
                    req_id,
                    fight_id,
                    pinned: !pinned,
                });
            }
            None => self.say(NO_CARD),
        }
    }

    /// A passing word over the stage (`.toast`).
    fn say(&mut self, words: &str) {
        self.toast = Some((words.to_string(), Instant::now()));
    }

    /// The daemon came back on a new connection (it restarted — a rebuild
    /// bounces the dev unit): what was in flight on the old one will never
    /// be answered. The log's `Watch` the client re-declares itself; the
    /// rail's page, the stored pull's `GetFight` and Home's page are asked
    /// for again here, or each would wait forever on its one request.
    fn reconnected(&mut self, requests: &mut Vec<ClientMsg>) {
        self.earlier.lost();
        self.ask_earlier(requests);
        // The old connection's reads will never answer: nothing waits on
        // one a left pull had out.
        self.stray_fight = None;
        if let Some(s) = self.stored.as_mut() {
            requests.extend(s.lost(&mut self.next_req_id));
        }
        self.pending_loadout = None;
        if self.home.is_some() {
            let req_id = self.next_req_id();
            if let Some(ui) = self.home.as_mut() {
                ui.reset();
                if let Some(msg) = ui.next_request(req_id, &self.season) {
                    requests.push(msg);
                }
            }
            self.home_panels = home::Panels::default();
        }
    }

    /// The pull rail is a drawer, and open over the stage.
    fn drawer_open(&self) -> bool {
        self.rail_open && !self.rail_docked()
    }

    /// The tailed log's segment a stored fight id names, when the fight is
    /// one of the log's own: its id is the log's id and the row's start (a
    /// Σ's with its mark), as the rail pairs them.
    fn log_segment_of(&self, fight_id: &str) -> Option<SegmentId> {
        let log = self.state.log_id()?;
        self.state
            .entries()
            .iter()
            .find(|e| {
                let sigma = e.row.kind == wowdps_model::SegmentKind::Overall;
                wowdps_proto::history::fight_id(log, e.row.start_ms, sigma) == fight_id
            })
            .map(|e| e.id)
    }

    /// `m` and the live pill: the log's newest pull, on the meter — a
    /// stored pull, Home and the drawer step aside, and a narrow window's
    /// pushed inspector gives the keys back.
    fn go_live(&mut self, requests: &mut Vec<ClientMsg>) {
        self.home = None;
        self.rail_open = false;
        self.leave_stored();
        self.log_view = None;
        self.state.uninspect();
        requests.extend(self.state.pin_live());
    }

    /// Send the rail's next page request, when it wants one and none is out.
    fn ask_earlier(&mut self, requests: &mut Vec<ClientMsg>) {
        if let Some(msg) = self.earlier.next_request(self.next_req_id, Instant::now()) {
            self.next_req_id = self.next_req_id.wrapping_add(1);
            requests.push(msg);
        }
    }

    /// Open the drawer (where the rail is one) on the pull on the stage,
    /// its keys on that row.
    fn open_rail(&mut self) {
        if !self.rail_docked() {
            self.rail_open = true;
        }
        self.rail_cursor = self.current_pull();
    }

    /// `H`: the rail open at the earlier nights — the drawer where the rail
    /// is one, scrolled to the first night that is not tonight, or to the
    /// pull on the stage when it is on one of them. The store's first page
    /// is asked for if none has landed (and none is on its way), and the
    /// next one if all in hand is tonight's.
    fn open_earlier(&mut self, requests: &mut Vec<ClientMsg>) -> Task<Message> {
        self.open_rail();
        if !self.earlier.answered {
            if !self.earlier.asking() {
                self.earlier.want_newest();
            }
        } else if self.rail().earlier().is_none() {
            self.earlier.want_older();
        }
        self.ask_earlier(requests);
        // The pull on the stage is itself on an earlier night: the rail
        // opens on its row, lit, rather than on a heading above it.
        let rail = self.rail();
        let on_earlier = self
            .current_pull()
            .and_then(|p| rail.night_of_pull(&p))
            .zip(rail.earlier())
            .is_some_and(|(at, first)| at >= first);
        if on_earlier {
            return self.open_on_pull();
        }
        iced::advanced::widget::operate::<()>(rail::ToEarlier::default()).discard()
    }

    /// A task that scrolls the rail the least that shows the pull on the
    /// stage, with room under it — after a step moved it.
    fn keep_pull_in_sight(&self) -> Task<Message> {
        iced::advanced::widget::operate::<()>(rail::Reveal::near(rail::current_id())).discard()
    }

    /// The same for the row the drawer's keys are on.
    fn keep_cursor_in_sight(&self) -> Task<Message> {
        iced::advanced::widget::operate::<()>(rail::Reveal::near(rail::cursor_id())).discard()
    }

    /// A task that stands the rail on the pull on the stage as the drawer
    /// opens on it: where it is when the row is in sight, else with its
    /// night's heading at the top, else the row in the middle.
    fn open_on_pull(&self) -> Task<Message> {
        iced::advanced::widget::operate::<()>(rail::Reveal::open()).discard()
    }

    /// Whose window this is, once known — the name the accent was resolved
    /// from ("Name-Realm", as a row label spells it).
    pub(crate) fn owner_name(&self) -> Option<&str> {
        self.accent_owner.as_deref()
    }

    /// The owner's row among `rows` — the chart on screen, which the caller
    /// already holds — as [`Gui::owner_of`] finds it. `None` on the Enemies
    /// view, whose rows are the enemies.
    pub(crate) fn owner_in(&self, rows: &[wowdps_model::Row]) -> Option<usize> {
        if self.fight().view == View::EnemyTaken {
            return None;
        }
        self.owner_of(rows)
    }

    /// The owner's row among `rows` whatever the view — an enemy's
    /// attackers or a drill's targets as much as a meter's players: the
    /// row that wears the "you" tag in a list. v35: the row the daemon
    /// marked the reader's own (`Row.mine`, from its owner resolution — the
    /// addon's own characters, the configured ones, the store's owner — so
    /// every character of the account is "you", a stored pull's included,
    /// without the window matching names). A daemon that marks nobody — its
    /// history store off, or not yet published — leaves the window its own
    /// hints: the character played last (its guid, as Home's answers name
    /// it), then the configured `history_characters` and the name the
    /// accent resolved, so the top bar and the meter never disagree about
    /// who the reader is.
    pub(crate) fn owner_of(&self, rows: &[wowdps_model::Row]) -> Option<usize> {
        if let Some(i) = rows.iter().position(|r| r.mine && !r.enemy) {
            return Some(i);
        }
        let mut names = self.cfg.history_characters();
        names.extend(self.owner_name().map(str::to_string));
        owner_among(rows, self.owner_guid.as_deref(), &names)
    }

    /// How wide the window is by the inspector's breakpoints, once its
    /// width is known — at the zoom the stage is drawn at, as the layout's
    /// own `responsive` measures it.
    pub(crate) fn fit(&self) -> Option<crate::inspector::Fit> {
        self.window_w
            .map(|w| crate::inspector::Fit::of(w / self.cfg.zoom.max(f32::EPSILON)))
    }

    /// Is the inspector pushed over the meter — a narrow window's, with the
    /// keys in it? Unknown width counts as narrow, the launch size.
    fn pushed(&self) -> bool {
        self.fight().inspecting()
            && self
                .fit()
                .is_none_or(|f| f == crate::inspector::Fit::Narrow)
    }

    /// A task that scrolls the inspector the least that brings the row the
    /// keys are on whole into sight — the list sits under the head, the
    /// numbers, the graph and its lanes, so a few j presses walk it past
    /// the fold. Nothing when no row is keyed.
    fn keep_keyed_in_sight(&self) -> Task<Message> {
        iced::advanced::widget::operate::<()>(FindRow {
            scroll: crate::inspector::scroll_id(),
            row: crate::inspector::keyed_row_id(),
            content: None,
            found: None,
        })
        .discard()
    }

    /// [`Gui::owner_in`] over the chart as it stands.
    #[cfg(test)]
    pub(crate) fn owner_row(&self) -> Option<usize> {
        self.owner_in(&self.fight().rows())
    }

    /// The meter's sort as the meter draws it: the chosen column while the
    /// view's table has it, and the daemon's order on a view that does not
    /// — Healing's overheal share is no order for Damage, nor a rate for
    /// Interrupts, and a sort by a figure off screen would jumble the
    /// ranks with no heading to say why. The choice is kept, so a view
    /// that has the column again sorts by it again.
    pub(crate) fn meter_sort(&self) -> Option<(crate::table::Col, bool)> {
        self.sort
            .filter(|(c, _)| crate::table::meter_set(self.fight().view, false).contains(c))
    }

    /// A task that scrolls the meter's list the least that brings the
    /// daemon row `row` whole into sight — nothing when it already is, and
    /// nothing at all when the meter's list is not what is on screen.
    fn keep_row_in_sight(&self, row: usize) -> Task<Message> {
        match view::meter_row_extent(self, row) {
            Some((top, bottom)) => iced::advanced::widget::operate::<()>(RevealSpan {
                id: view::meter_list_id(),
                top,
                bottom,
            })
            .discard(),
            None => Task::none(),
        }
    }

    /// R25: a task that scrolls the Deaths table the least that brings the
    /// selected death whole into sight — nothing when the table is not on
    /// screen or the death is already in it.
    fn keep_death_in_sight(&self) -> Task<Message> {
        let covered = self.talents.is_some() || self.home.is_some();
        let Some(table) = crate::deaths::Table::of(self).filter(|_| !covered) else {
            return Task::none();
        };
        let app = self.fight();
        let at = app
            .raid()
            .and_then(|raid| crate::deaths::selected(app, raid))
            .and_then(|i| table.extent(i));
        match at {
            Some((top, bottom)) => iced::advanced::widget::operate::<()>(RevealSpan {
                id: view::meter_list_id(),
                top,
                bottom,
            })
            .discard(),
            None => Task::none(),
        }
    }

    /// The hovered meter row, when the pointer is on the meter's list.
    pub(crate) fn hover_meter(&self) -> Option<usize> {
        match self.row_hover {
            Some(RowHover::Meter(i)) => Some(i),
            _ => None,
        }
    }

    /// The hovered row of one drill pane. The panes are drawn side by side,
    /// so the pointer is in at most one of them.
    pub(crate) fn hover_in(&self, pane: wowdps_model::Pane) -> Option<usize> {
        match self.row_hover {
            Some(RowHover::Drill(p, i)) if p == pane => Some(i),
            _ => None,
        }
    }

    /// Is the row filter actually on screen? Only a pull's stage draws it,
    /// and only when nothing window-local covers the stage. Focusing a
    /// field that is not in the widget tree would swallow every key with
    /// nothing to type into — a window that looks keyboard-dead — so `/`
    /// and the swallow branch both ask this first.
    pub(crate) fn filter_visible(&self) -> bool {
        // The meter stands beside the inspector — and under a comparison —
        // so its filter is drawn with it, on any pull, waiting for its
        // first rows or not. A narrow window's pushed inspector hides both,
        // and `/` gives the keys back to the meter before it focuses the
        // field.
        self.talents.is_none()
            && self.home.is_none()
            && !self.shortcuts_open
            && self.palette.is_none()
            && !self.drawer_open()
            && !self.stored.as_ref().is_some_and(|s| s.missing)
    }

    /// Where `Up`/`Down` land while a filter narrows the meter: the next
    /// row that is actually DRAWN, or `None` when the question does not
    /// apply (another action, another screen, a drill, no filter) and the
    /// state machine's own clamped step is right.
    fn filtered_step(&self, action: Action) -> Option<Step> {
        if !matches!(action, Action::Up | Action::Down) {
            return None;
        }
        let app = self.fight();
        // The keys walk the meter's rows until Enter hands them to the
        // inspector's list; a comparison's keys move its second half, a
        // meter row.
        let in_list = match app.screen {
            Screen::Meter => app.inspecting(),
            Screen::Compare => false,
            Screen::List => return None,
        };
        // R26: a Damage or Healing ability list is a tree — the step walks
        // its drawn lines, groups and parts included.
        if in_list && let Some(lines) = self.tree_lines() {
            let at = self.tree_keyed(&lines);
            return crate::inspector::tree::step(&lines, at, action == Action::Down)
                .and_then(|p| lines.get(p))
                .map(|l| Step::Tree(l.node.clone()));
        }
        // A sorted by-spell pane: the step is positional in the drawn
        // order, and lands on the pane's own selection.
        if in_list && let Some(d) = app.drill.as_ref() {
            if d.spell.is_some() || d.pane != wowdps_model::Pane::Spell || self.drill_sort.is_none()
            {
                return None;
            }
            let (by_spell, _) = app.breakdown();
            let order: Vec<usize> =
                crate::table::sorted(by_spell.into_iter().enumerate().collect(), self.drill_sort)
                    .into_iter()
                    .map(|(i, _)| i)
                    .collect();
            return step_in(&order, d.spell_sel, action).map(Step::Spell);
        }
        // R25: the Deaths table walks the deaths in the order they happened
        // — each step is the next death's recap, a player who died twice
        // stepped onto twice.
        if !in_list
            && app.screen == Screen::Meter
            && app.view == View::Deaths
            && let Some(raid) = app.raid()
        {
            let order = crate::deaths::drawn(raid, &self.filter);
            let sel = crate::deaths::selected(app, raid).unwrap_or(usize::MAX);
            // A filter that hides every death leaves nowhere to land: the
            // key is swallowed, never handed to the meter's hidden count
            // rows, whose drill would move the recap to a player the table
            // says matches nothing.
            return Some(step_in(&order, sel, action).map_or(Step::Stay, Step::Death));
        }
        wowdps_gui_logic::table::meter_step(
            app.rows(),
            &self.filter,
            self.meter_sort(),
            app.row_sel,
            action,
        )
        .map(Step::Meter)
    }

    /// v28: on a Deaths drill, ← and → step the death windows — while the
    /// keys are in the inspector (Enter put them there; its death chips are
    /// what they step), and there alone: on the meter they walk the rail,
    /// whoever is selected, so the selection's death count never changes
    /// what a key does. `Some(Some(i))` asks for window `i`; `Some(None)`
    /// is the recap's key with nowhere to step (one death), swallowed
    /// rather than leave the fight; `None` is a key that means something
    /// else here.
    fn death_step(&self, key: &keyboard::Key) -> Option<Option<u32>> {
        use keyboard::key::Named;
        let app = self.fight();
        let arrow = matches!(
            key,
            keyboard::Key::Named(Named::ArrowLeft | Named::ArrowRight)
        );
        if !arrow || app.view != View::Deaths || app.drill.is_none() || !app.inspecting() {
            return None;
        }
        let (deaths, shown) = app.deaths();
        let Some(last) = (deaths.len() as u32).checked_sub(1).filter(|l| *l > 0) else {
            return Some(None);
        };
        let at = shown.unwrap_or(last);
        Some(Some(match key {
            keyboard::Key::Named(Named::ArrowLeft) => at.saturating_sub(1),
            _ => (at + 1).min(last),
        }))
    }

    /// R26: open a fold of the ability tree, or shut it.
    fn fold(&mut self, key: &str) {
        if !self.tree_open.remove(key) {
            self.tree_open.insert(key.to_string());
        }
    }

    /// R26: the ability list's tree lines, when the drill on the stage has
    /// one to draw — Damage or Healing, the Abilities tab up, no ability
    /// open. The inspector draws these; the keys walk them.
    pub(crate) fn tree_lines(&self) -> Option<Vec<crate::inspector::tree::Line>> {
        let app = self.fight();
        let d = app.drill.as_ref()?;
        if d.spell.is_some()
            || d.pane != wowdps_model::Pane::Spell
            || !matches!(app.view, View::Damage | View::Healing)
        {
            return None;
        }
        let (by_spell, _) = app.breakdown();
        Some(crate::inspector::tree::lines(
            &by_spell,
            &app.drill_tree(),
            &self.tree_open,
            self.drill_sort,
        ))
    }

    /// R26: which of `lines` the keys rest on — the window's cursor when it
    /// is this player's and drawn, else the drill's selected row (or the
    /// shut group holding it).
    pub(crate) fn tree_keyed(&self, lines: &[crate::inspector::tree::Line]) -> Option<usize> {
        let d = self.fight().drill.as_ref()?;
        let cursor = self
            .tree_cursor
            .as_ref()
            .filter(|(key, _)| *key == d.key)
            .map(|(_, node)| node);
        crate::inspector::tree::keyed(lines, cursor, d.spell_sel)
    }

    /// R26: rest the keys on `node`. A row's or a part's line also selects
    /// the row, so Enter opens its ability through the state machine; a
    /// row's line needs no cursor of the window's.
    fn tree_rest(&mut self, node: crate::inspector::tree::Node) {
        use crate::inspector::tree::Node;
        let Some(key) = self.fight().drill.as_ref().map(|d| d.key.clone()) else {
            return;
        };
        let row = match node {
            Node::Row(i) | Node::Part(i, _) => Some(i),
            Node::Group(_) => None,
        };
        if let (Some(i), Some(d)) = (row, self.fight_mut().drill.as_mut()) {
            d.spell_sel = i;
        }
        self.tree_cursor = match node {
            Node::Row(_) => None,
            other => Some((key, other)),
        };
    }

    /// R26: the keyed tree line, while the keys are in the ability list.
    fn tree_keyed_line(&self) -> Option<crate::inspector::tree::Line> {
        if !self.fight().inspecting() {
            return None;
        }
        let lines = self.tree_lines()?;
        let at = self.tree_keyed(&lines)?;
        lines.get(at).cloned()
    }

    /// R26: Enter on the keyed tree line. A group's folds (it has no
    /// ability to open) and is answered here — `true`; a part's selects
    /// its row first, so the state machine's Open opens that row.
    fn tree_enter(&mut self) -> bool {
        let Some(line) = self.tree_keyed_line() else {
            return false;
        };
        match (line.opens, line.fold_key) {
            (None, Some(key)) => {
                self.fold(&key);
                true
            }
            (Some(i), _) => {
                if let Some(d) = self.fight_mut().drill.as_mut() {
                    d.spell_sel = i;
                }
                false
            }
            (None, None) => false,
        }
    }

    /// R26: ← → while the keys are in a Damage or Healing ability list are
    /// the tree's, as they are a recap's deaths on the Deaths drill: →
    /// opens the keyed line's fold, ← shuts it — or, on a line inside one,
    /// goes to the line that holds it. `true` when the key was the tree's;
    /// one with nothing to do is swallowed rather than leave the fight.
    fn tree_arrow(&mut self, key: &keyboard::Key) -> bool {
        use keyboard::key::Named;
        let right = match key {
            keyboard::Key::Named(Named::ArrowRight) => true,
            keyboard::Key::Named(Named::ArrowLeft) => false,
            _ => return false,
        };
        if !self.fight().inspecting() {
            return false;
        }
        let Some(lines) = self.tree_lines() else {
            return false;
        };
        let Some(at) = self.tree_keyed(&lines) else {
            return true;
        };
        let Some(line) = lines.get(at) else {
            return true;
        };
        match (right, line.fold, &line.fold_key) {
            (true, Some(false), Some(k)) | (false, Some(true), Some(k)) => {
                let k = k.clone();
                self.fold(&k);
            }
            (false, _, _) => {
                if let Some(up) = crate::inspector::tree::parent(&lines, at)
                    .and_then(|p| lines.get(p))
                    .map(|l| l.node.clone())
                {
                    self.tree_rest(up);
                }
            }
            _ => {}
        }
        true
    }

    /// What a stored pull cannot do, asked of it, and the word that says so:
    /// the store keeps no comparison, no enemies' view and no ability's own
    /// curve, so `v`, the enemies' view and Enter inside the inspector
    /// (which opens an ability) are answered with a toast there rather than
    /// ask for what it cannot answer.
    fn stored_refusal(&self, action: Action) -> Option<&'static str> {
        let s = self.stored.as_ref()?;
        match action {
            Action::PickCompare => Some(NO_STORED_PAIR),
            Action::SetView(v) if !v.is_stored() => Some(view::NOT_STORED),
            // R26: Enter on a group folds it, stored or not.
            Action::Open
                if s.state.inspecting()
                    && !self.tree_keyed_line().is_some_and(|l| l.opens.is_none()) =>
            {
                Some(NO_STORED_ABILITY)
            }
            _ => None,
        }
    }

    fn stored_refuses(&self, action: Action) -> bool {
        self.stored_refusal(action).is_some()
    }

    /// The keys the `?` sheet dims: what the pull on the stage cannot answer
    /// on its surface — a stored pull keeps no comparison, no enemies' view
    /// and no ability's own curve — `p` where the store holds no card of
    /// it to pin, and Enter on the Deaths table beside the inspector.
    pub(crate) fn inert_keys(&self) -> Vec<&'static str> {
        let mut keys = Vec::new();
        if self.home.is_some() || self.talents.is_some() {
            return keys;
        }
        if self.stored.is_some() {
            keys.extend(["E", "v"]);
            if self.fight().inspecting() {
                keys.push("enter");
            }
        }
        if self.pin_target().is_none() {
            keys.push("p");
        }
        // R25: beside the inspector the Deaths table keeps the keys and
        // Enter hands the keyless recap nothing (`stage_key`).
        let beside = self
            .fit()
            .is_some_and(|f| f != crate::inspector::Fit::Narrow);
        if beside && self.deaths_table_keys() && !keys.contains(&"enter") {
            keys.push("enter");
        }
        keys
    }

    fn next_req_id(&mut self) -> u32 {
        let id = self.next_req_id;
        self.next_req_id = self.next_req_id.wrapping_add(1);
        id
    }

    /// Open the talent viewer on the selected meter row's player when there
    /// is one — `t`, and the inspector's "Talents and gear". A stored simc
    /// paste (or the spec's empty tree) shows at once; the logged
    /// COMBATANT_INFO build wins when it is there — asked of the daemon for
    /// a pull of the log, and already in hand for a stored pull whose
    /// answer carried it. `on_row` false opens it on nobody (from Home,
    /// whose meter is not the reader's).
    fn open_talents(&mut self, on_row: bool, requests: &mut Vec<ClientMsg>) {
        let app = self.fight();
        let row = on_row
            .then(|| app.rows().get(app.row_sel).cloned())
            .flatten();
        let player = row
            .as_ref()
            .map(|r| (r.label.clone(), r.spec.map(|s| s.id())));
        self.talents = Some(talents::TalentsUi::open(player));
        // Any older request now answers a viewer that no longer exists;
        // only the request made HERE may adopt.
        self.pending_loadout = None;
        let Some(r) = row else {
            return;
        };
        if let Some(stored) = &self.stored {
            if let (Some(ui), Some(l)) = (self.talents.as_mut(), stored.loadout_of(&r.key)) {
                ui.adopt_logged(l);
            }
            return;
        }
        let req_id = self.next_req_id();
        self.pending_loadout = Some(req_id);
        requests.push(ClientMsg::GetLoadout {
            req_id,
            segment: self.state.watched_segment(),
            guid: r.key,
        });
    }

    /// Esc on a fight's stage, one level at a time: a filter's text, the
    /// inspector's ability, the keys the inspector holds (a narrow
    /// window's pushed inspector), the comparison (its ability, then the
    /// pair, then a lone pin) — and, with nothing left to back out of,
    /// Home, the front door, where the chain ends. (The rail's drawer,
    /// over all of it, went before any of these.) `false` when the key is
    /// not the stage's to answer.
    fn stage_escape(&mut self, requests: &mut Vec<ClientMsg>) -> bool {
        let app = self.fight();
        if !matches!(app.screen, Screen::Meter | Screen::Compare) {
            return false;
        }
        let comparing = app.screen == Screen::Compare || !app.compare_picks().is_empty();
        let ability = app.drill_spell().is_some();
        // The filter's text goes first wherever the field shows — beside
        // the inspector, keys in it or not. Only a narrow window's pushed
        // inspector covers the field, and there its keys come back first:
        // Esc must never clear text the reader cannot see.
        if !self.filter.is_empty() && self.filter_visible() && !self.pushed() {
            self.filter.clear();
        } else if ability || self.keys_shown() {
            requests.extend(self.on_fight(|s| s.apply(Action::Back)));
        } else if comparing {
            // Keys held by a pair beside the meter lit nothing: they go
            // with it, not as an Esc of their own that seemed to do nothing.
            self.fight_mut().uninspect();
            requests.extend(self.on_fight(ClientState::clear_compare));
        } else {
            self.open_home(requests);
        }
        true
    }

    /// Do the keys the inspector holds show? On the meter a keyed row is
    /// lit in its list (and a narrow window's inspector is pushed); a pair
    /// has no keyed row, so only its push shows them.
    fn keys_shown(&self) -> bool {
        let app = self.fight();
        app.inspecting() && (app.screen == Screen::Meter || self.pushed())
    }

    /// The keys the stage answers by the window's width and the
    /// inspector's tabs, before the state machine hears them. `true` when
    /// the key was answered here. Enter on a pair beside the meter would
    /// hand the keys to lists with no keyed row — nothing to see, and an Esc
    /// later spent taking them back — so it does nothing there; Tab and `g`
    /// in a narrow window push the inspector they change (it is not on
    /// screen until they do); and Tab on a Taken drill with an R21 ledger
    /// walks its three tabs, Hit by → Attackers → Stacks.
    fn stage_key(&mut self, action: Action) -> bool {
        use crate::inspector::Fit;
        let narrow = self.fit() == Some(Fit::Narrow);
        let wide = self.fit().is_some() && !narrow;
        match (self.fight().screen, action) {
            (Screen::Compare, Action::Open) if wide => true,
            // R25: on the Deaths table the keys stay with the table — j/k
            // walk the deaths and the recap beside it follows; the recap
            // has no row to key, so Enter hands it nothing (a narrow
            // window's pushes it over the table, its one way to show it).
            (Screen::Meter, Action::Open) if self.deaths_table_keys() => {
                if narrow {
                    self.fight_mut().inspect();
                }
                true
            }
            // R26: Enter on a group of the ability tree folds it — a group
            // has no ability to open.
            (Screen::Meter, Action::Open) if self.tree_enter() => true,
            (Screen::Meter, Action::SwapPane | Action::ToggleGraph) => {
                if narrow && !self.fight().inspecting() {
                    self.fight_mut().inspect();
                }
                action == Action::SwapPane && self.stacks_tab()
            }
            _ => false,
        }
    }

    /// R25: the Deaths table holds the keys — it is on the stage (the
    /// Deaths view with a raid timeline) and the inspector has not been
    /// given them.
    fn deaths_table_keys(&self) -> bool {
        let app = self.fight();
        app.view == View::Deaths && app.raid().is_some() && !app.inspecting()
    }

    /// R25: beside the inspector, the Deaths table OWNS the keys — the
    /// recap has no row to key, so keys handed to the inspector on another
    /// view (Enter on the Damage meter, then a skull or `K` onto Deaths)
    /// come back to the table, whose j/k walk the deaths; left with the
    /// inspector they would move nothing visible, Enter would open nothing,
    /// and the table's selection would sit at the hover's weight. A narrow
    /// window's pushed recap keeps them: that is its one way to show it.
    fn deaths_table_takes_the_keys(&mut self) {
        let beside = self
            .fit()
            .is_some_and(|f| f != crate::inspector::Fit::Narrow);
        let app = self.fight();
        let held = app.screen == Screen::Meter
            && app.view == View::Deaths
            && app.raid().is_some()
            && app.inspecting();
        if beside && held {
            self.fight_mut().uninspect();
        }
    }

    /// Tab on a Taken drill whose player has an R21 ledger: the third tab
    /// joins the walk. `true` when this Tab was the walk's.
    fn stacks_tab(&mut self) -> bool {
        let app = self.fight();
        let ledger = app.view == View::Taken
            && app.drill_spell().is_none()
            && app
                .drill_stacks()
                .is_some_and(|(s, c, b)| !crate::taken::matrices(s, c, b).is_empty());
        let stacks_open = self.stacks_open;
        let Some(d) = self.fight_mut().drill.as_mut().filter(|_| ledger) else {
            return false;
        };
        if stacks_open {
            d.pane = wowdps_model::Pane::Spell;
            self.stacks_open = false;
            true
        } else if d.pane == wowdps_model::Pane::Target {
            self.stacks_open = true;
            true
        } else {
            false
        }
    }

    /// Open Home and ask for its first slice of cards.
    fn open_home(&mut self, requests: &mut Vec<wowdps_proto::ClientMsg>) {
        let mut ui = home::Home::new();
        // A scope outlives the screen: reopening Home lands on the same
        // character, and so does the next launch (it is in the config).
        ui.scope = self.cfg.character.clone();
        ui.disabled_reason = self.history_disabled.clone();
        ui.dropped = self.history_dropped;
        let req_id = self.next_req_id();
        if let Some(msg) = ui.next_request(req_id, &self.season) {
            requests.push(msg);
        }
        self.home_panels = home::Panels::default();
        self.home = Some(ui);
        // The store's state is what tells a disabled store from a cold one
        // from one that lost writes, and the daemon never broadcasts it —
        // a value read once at launch would be stale by the first pull.
        requests.push(self.ask_status());
    }

    /// Scope Home to `guid` (`None`: every character of yours) — a chip, a
    /// pick of the picker's menu, a palette item — opening it so scoped
    /// when it is not up. The scope is Home's, and the one it opens on
    /// next time — the config's `character` says no more than that. It
    /// locks nothing: whose window this is, its chrome and the "you" on a
    /// meter follow the character played, whoever Home is showing.
    fn scope_home(&mut self, guid: Option<String>, requests: &mut Vec<ClientMsg>) {
        // The one key, over what is on disk: the window's launch-time copy
        // would put back an overlay drag or zoom saved since.
        if self.cfg.character != guid {
            self.cfg.character = guid.clone();
            Config::store_character(guid.clone());
        }
        if self.home.is_none() {
            self.open_home(requests);
        }
        if let Some(ui) = self.home.as_mut() {
            ui.scope = guid;
        }
        self.rederive_home();
    }

    /// A `GetStatus` one-shot. Its req_id is not tracked: `Status` carries
    /// the whole answer, any reply is as good as the newest, and the window
    /// has exactly one asker.
    fn ask_status(&mut self) -> wowdps_proto::ClientMsg {
        wowdps_proto::ClientMsg::GetStatus {
            req_id: self.next_req_id(),
        }
    }

    /// Resolve the owner, once. Two identities can name "me": the one Home
    /// derives from the store's cards, and `history_characters` from the
    /// config matched against the players on the meter — the same union
    /// Home's characters panel uses, so a window opened without Home still
    /// knows its owner. Neither available yet means no owner, never a
    /// borrowed row. The owner's class is what a class chrome wears, and is
    /// remembered for the next launch; a gold chrome does not move.
    fn resolve_accent(&mut self) {
        if self.accent_owner.is_some() {
            return;
        }
        if let Some(owner) = self.home_panels.owner.clone()
            && let Some(class) = owner.class
        {
            // Home's owner is whose the store's newest card is — the
            // character played last, whatever Home is scoped to.
            self.accent_owner = Some(owner.name.clone());
            self.learn_owner_class(Some(class), owner.spec, Some(&owner.guid));
            return;
        }
        let names = self.cfg.history_characters();
        if names.is_empty() {
            return;
        }
        // A row label is "Name-Realm"; the config lists it whole or by its
        // name half — the one matcher the owner's row is found by too
        // (`owner_among`, whole names before a bare one, and a bare one
        // only when it names one row), so the chip and the chrome never
        // disagree, and a namesake never lends the chrome their class.
        let rows = self.fight().rows();
        if let Some(row) = owner_among(&rows, None, &names)
            .and_then(|i| rows.get(i))
            .filter(|r| r.class.is_some())
            .cloned()
        {
            self.accent_owner = Some(row.label);
            self.learn_owner_class(row.class, row.spec, Some(&row.key));
        }
    }

    /// The owner's class is known (or known to be unknown): hold it, wear
    /// it if the chrome is the class's, and — when `who` is whose window
    /// this is (the character played last, as Home's answers or the rail's
    /// pages name them), or nobody is named yet — remember it so the next
    /// launch's first frame wears it too. Home's scope has no say: it is
    /// what Home shows, not who the reader is. A configured alt who turned
    /// up on the meter while the store names another character as played
    /// last is worn for the session and never remembered. `None` for `who`
    /// is the owner itself.
    fn learn_owner_class(
        &mut self,
        class: Option<wowdps_model::Class>,
        spec: Option<wowdps_model::Spec>,
        who: Option<&str>,
    ) {
        self.owner_class = class.map(|c| (c, spec));
        self.accent = chrome_accent(self.cfg.chrome(), self.owner_class);
        let played_last =
            self.owner_guid
                .as_deref()
                .or(self.home_panels.owner.as_ref().map(|o| o.guid.as_str()));
        let is_owner = match (who, played_last) {
            (None, _) | (_, None) => true,
            (Some(who), Some(last)) => who == last,
        };
        let name = class.map(|c| c.name().to_string());
        if is_owner && self.cfg.character_class != name {
            self.cfg.character_class = name.clone();
            Config::store_character_class(name);
        }
    }

    /// Merge characters the store named into the window's memory of them
    /// (gui-logic's `home::remember`).
    fn remember_characters(&mut self, seen: Vec<home::CharLine>) {
        home::remember(&mut self.known_characters, seen);
    }
    /// Re-derive the panels from whatever Home holds now.
    fn rederive_home(&mut self) {
        if let Some(ui) = self.home.as_ref() {
            // The tailed log's visits name a raid night still going, whose
            // Σ card the store has yet to write.
            let log = rail::log_instances(self.state.entries(), self.state.log_id());
            self.home_panels = home::derive(
                &ui.cards,
                ui.scope.as_deref(),
                &self.season,
                &self.cfg.history_characters(),
                &log,
            );
            // Whose window it is follows the character played last, never
            // the scope: a chip changes what Home shows, not who "you" are.
            if let Some(owner) = &self.home_panels.owner {
                self.owner_guid = Some(owner.guid.clone());
            }
            // The store just named the owner: adopt their accent now rather
            // than at the next drain, so opening Home tints the window.
            self.resolve_accent();
            let seen = self.home_panels.characters.clone();
            self.remember_characters(seen);
        }
    }

    /// The command palette's items for what is typed into it, as its card
    /// lists them: the rail's pulls, the players of the pull on the stage
    /// (none without one), the views and the screens. Nothing while it is
    /// shut.
    pub(crate) fn palette_items(&self) -> Vec<palette::Item> {
        let Some(p) = &self.palette else {
            return Vec::new();
        };
        let players = match self.current_pull() {
            Some(_) => self.seen.players(self.fight()),
            None => Vec::new(),
        };
        palette::listed(
            palette::items(
                &self.rail(),
                &players,
                &self.known_characters,
                self.cfg.hide_realms,
            ),
            &p.query,
        )
    }

    /// Ctrl K, or the jump box: the palette, empty, its field focused —
    /// over whatever card was up, which it replaces.
    fn open_palette(&mut self) -> Task<Message> {
        self.palette = Some(palette::Palette::default());
        self.shortcuts_open = false;
        self.picker_open = false;
        self.options_open = false;
        self.filter_focused = false;
        iced::widget::operation::focus(palette::input_id())
    }

    /// A key while the palette is up: every key is its own. The field takes
    /// what it types and captures Enter and Esc for itself (both come back
    /// as their own messages); what reaches here is the rest — the arrows
    /// and Ctrl N / Ctrl P move the selection, Ctrl K closes it, and a
    /// letter or Backspace the field let through (a press on the list took
    /// its focus) is typed into it all the same, the focus handed back.
    fn palette_key(
        &mut self,
        key: &keyboard::Key,
        modifiers: keyboard::Modifiers,
        typed: Option<&str>,
        requests: &mut Vec<ClientMsg>,
    ) -> Option<Task<Message>> {
        use keyboard::key::Named;
        let ctrl = modifiers.control();
        let chord = |c: &str| ctrl && matches!(key, keyboard::Key::Character(k) if k.as_str() == c);
        match key {
            keyboard::Key::Named(Named::Escape) => {
                self.palette = None;
                None
            }
            _ if is_jump_key(key, modifiers) => {
                self.palette = None;
                None
            }
            keyboard::Key::Named(Named::ArrowDown) => self.palette_step(true),
            keyboard::Key::Named(Named::ArrowUp) => self.palette_step(false),
            _ if chord("n") => self.palette_step(true),
            _ if chord("p") => self.palette_step(false),
            keyboard::Key::Named(Named::Enter) => self.palette_submit(requests),
            keyboard::Key::Named(Named::Backspace) => {
                if let Some(p) = self.palette.as_mut() {
                    let mut query = p.query.clone();
                    query.pop();
                    p.typed(query);
                }
                Some(iced::widget::operation::focus(palette::input_id()))
            }
            keyboard::Key::Character(c) if !ctrl && !modifiers.alt() => {
                // What the key typed; its character where the event says
                // nothing of its text.
                let t = typed.unwrap_or(c.as_str());
                if let Some(p) = self.palette.as_mut() {
                    let query = format!("{}{t}", p.query);
                    p.typed(query);
                }
                Some(iced::widget::operation::focus(palette::input_id()))
            }
            _ => None,
        }
    }

    /// The palette's selection one step down or up, kept in sight.
    fn palette_step(&mut self, down: bool) -> Option<Task<Message>> {
        let n = self.palette_items().len();
        self.palette.as_mut()?.step(down, n);
        Some(
            iced::advanced::widget::operate::<()>(FindRow {
                scroll: palette::list_id(),
                row: palette::selected_id(),
                content: None,
                found: None,
            })
            .discard(),
        )
    }

    /// Enter: run the palette's selection.
    fn palette_submit(&mut self, requests: &mut Vec<ClientMsg>) -> Option<Task<Message>> {
        let items = self.palette_items();
        let p = self.palette.as_mut()?;
        p.clamp(items.len());
        let run = items.get(p.sel)?.run.clone();
        self.run_palette(run, requests)
    }

    /// Run a palette item: the palette closes, and the window goes where
    /// the item says — as the key or the press it stands for would take it.
    fn run_palette(
        &mut self,
        run: palette::Run,
        requests: &mut Vec<ClientMsg>,
    ) -> Option<Task<Message>> {
        self.palette = None;
        match run {
            palette::Run::Pull(pull) => {
                self.rail_open = false;
                self.go_pull(pull, requests);
                None
            }
            // The player on the pull's meter, on a chart they have a row on
            // — the view a count or the enemies were on gives way to Damage
            // — the filter cleared so their row is drawn, and in sight.
            palette::Run::Player { key, label } => {
                self.home = None;
                self.rail_open = false;
                self.filter.clear();
                if !crate::fight_head::player_chart(self.fight().view) {
                    self.log_view = None;
                    requests.extend(self.on_fight(|s| s.apply(Action::SetView(View::Damage))));
                }
                requests.extend(self.on_fight(|s| s.select_player(&key, &label)));
                // Their row, when the chart in hand has it; else the view's
                // rows are on their way, and the snapshot that brings them
                // scrolls to it ([`Gui::reveal_player`]).
                match self.fight().rows().iter().position(|r| r.key == key) {
                    Some(row) => {
                        self.reveal_player = None;
                        Some(self.keep_row_in_sight(row))
                    }
                    None => {
                        self.reveal_player = Some(key);
                        None
                    }
                }
            }
            palette::Run::View(view) => {
                self.home = None;
                self.rail_open = false;
                match self.stored_refusal(Action::SetView(view)) {
                    Some(words) => self.say(words),
                    None => {
                        self.log_view = None;
                        requests.extend(self.on_fight(|s| s.apply(Action::SetView(view))));
                    }
                }
                None
            }
            palette::Run::Home => {
                if self.home.is_none() {
                    self.open_home(requests);
                }
                None
            }
            palette::Run::Live => {
                self.go_live(requests);
                None
            }
            palette::Run::Earlier => Some(self.open_earlier(requests)),
            palette::Run::HomeScope(guid) => {
                self.scope_home(guid, requests);
                None
            }
            palette::Run::Sheet => {
                self.shortcuts_open = true;
                None
            }
            palette::Run::Talents => {
                let on_row = self.home.is_none();
                self.open_talents(on_row, requests);
                None
            }
        }
    }

    /// Seconds since data last arrived, once it stops looking live.
    pub(crate) fn stale_secs(&self) -> Option<u64> {
        stale_secs(self.last_snapshot_at)
    }
}

#[cfg(test)]
impl Gui {
    /// A window over an already-driven `ClientState` and any client (tests
    /// hand in a socketpair whose peer plays daemon, or stays silent).
    pub(crate) fn for_test(client: DaemonClient, state: ClientState, cfg: Config) -> Self {
        // Every test window may save its config (a zoom, a pick, a learned
        // class): never over the real one.
        testkit::isolate_config();
        let mut gui = Self::new(client, cfg);
        gui.state = state;
        // The window follows the selection, as a running one does; a state
        // driven without the opt-in gets it here, its drill (or pair) the
        // selection's.
        let _ = gui.state.set_follow(true);
        gui.roster.observe(&gui.state.rows());
        gui
    }

    pub(crate) fn set_last_snapshot_at(&mut self, at: Option<Instant>) {
        self.last_snapshot_at = at;
    }

    pub(crate) fn pending_loadout(&self) -> Option<u32> {
        self.pending_loadout
    }
}

/// What the chrome wears: the game's gold whoever owns the window, or the
/// owner's class — [`theme::NEUTRAL`] while a class chrome knows no class.
fn chrome_accent(
    chrome: theme::Chrome,
    owner_class: Option<(wowdps_model::Class, Option<wowdps_model::Spec>)>,
) -> theme::Accent {
    match (chrome, owner_class) {
        (theme::Chrome::Gold, _) => theme::GOLD_ACCENT,
        (theme::Chrome::Class, Some((class, spec))) => theme::accent(Some(class), spec),
        (theme::Chrome::Class, None) => theme::NEUTRAL,
    }
}

/// Shared with the overlay: 5s of silence is when "live" starts needing an
/// asterisk, thanks to the game's buffered log writes.
pub(crate) fn stale_secs(last_at: Option<Instant>) -> Option<u64> {
    let secs = last_at?.elapsed().as_secs();
    (secs >= 5).then_some(secs)
}

/// Drain the daemon client into the state; snapshots refresh the staleness
/// clock. Reconnects (and re-declares the cursor) if the daemon went away,
/// and says so: the one-shots in flight on the old connection are the
/// caller's to ask again. v19: `Loadout` replies are one-shots the window
/// consumes itself (the shared state machine treats them as no-ops), so
/// they come back to the caller instead of going through `on_msg`.
pub(crate) fn drain_client(
    state: &mut ClientState,
    client: &mut DaemonClient,
    last_snapshot_at: &mut Option<Instant>,
) -> (Vec<DaemonMsg>, bool) {
    let mut reconnected = false;
    let mut intercepted = Vec::new();
    for msg in client.poll() {
        if matches!(
            msg,
            DaemonMsg::Snapshot { .. } | DaemonMsg::SegmentList { .. }
        ) {
            *last_snapshot_at = Some(Instant::now());
        }
        // One-shots the shared state machine treats as no-ops: the window
        // consumes them itself rather than letting them fall through.
        if matches!(
            msg,
            DaemonMsg::Loadout { .. }
                | DaemonMsg::History { .. }
                | DaemonMsg::HistoryChanged { .. }
                | DaemonMsg::Status { .. }
                | DaemonMsg::Fight { .. }
        ) {
            intercepted.push(msg);
            continue;
        }
        for req in state.on_msg(msg) {
            client.send(&req);
        }
    }
    // Never `reconnect_if_dead` here: this runs on the UI thread every tick,
    // and its bounded wait for a spawned daemon is 3 s of not answering the
    // compositor. `try_reconnect` spawns at most once per backoff and
    // returns at once; the status line says which of its states we are in.
    if client.is_dead() {
        match client.try_reconnect() {
            Reconnect::Connected => {
                state.status = None;
                client.send(&state.initial_request());
                reconnected = true;
            }
            Reconnect::Spawned => {
                eprintln!("wowdps-gui: daemon gone; spawned one");
                state.status = Some("daemon gone — starting one…".to_string());
            }
            Reconnect::Waiting => {
                state.status = Some("daemon gone — reconnecting…".to_string());
            }
            Reconnect::Failed(e) => {
                eprintln!("wowdps-gui: daemon gone; {e}");
                state.status = Some(format!("daemon gone — {e}"));
            }
        }
    }
    (intercepted, reconnected)
}

#[derive(Debug, Clone)]
pub(crate) enum Message {
    /// Drain the daemon client and let live durations advance.
    Tick,
    Key(keyboard::Event),
    /// A meter row was clicked: select it; the inspector follows.
    MeterRow(usize),
    /// A meter row was clicked in a narrow window: select it and push the
    /// inspector over the meter.
    PushRow(usize),
    /// R12: a meter row's class icon was clicked — pin that player for the
    /// comparison, pair them with the pin, or unpin them.
    CompareRow(usize),
    /// The inspector's Compare button: `v` on the selection.
    PinCompare,
    /// The pushed inspector's back button: the keys, and the view, go back
    /// to the meter.
    Uninspect,
    /// The ability strip's back button: close the ability.
    CloseAbility,
    /// An inspector tab: abilities (or what hit them) or targets.
    InspectorTab(wowdps_model::Pane),
    /// The inspector's graph mode button: `g`.
    ToggleGraph,
    /// The inspector's "Talents and gear": `t` on the selection.
    OpenTalents,
    /// R12: right-click — drop the picked pair (or a lone half-pick) and
    /// return to the meter. Pointer parity with `Esc`.
    ClearCompare,
    /// R12/v12: a drag on a comparison graph selected a time window (ms from
    /// segment start) — or a right-click asked for the whole fight back.
    CompareRange(Option<(u32, u32)>),
    /// v14: a drag on the drilldown's graph selected a zoom window (or a
    /// right-click asked for the whole fight back). Client-side only — the
    /// drill timeline is always whole, so nothing round-trips.
    DrillRange(Option<(u32, u32)>),
    /// v16: a by-spell drill row was clicked — descend into that ability.
    SpellRow(usize),
    /// R24: an attacker row of the enemy drill was clicked — descend into
    /// that attacker's abilities on the enemy.
    AttackerRow(usize),
    /// v18: a comparison spell row was clicked — drill BOTH sides into that
    /// ability (by-spell key, label).
    CompareSpell((String, String)),
    /// The header's ⚙ was clicked: open/close the options panel.
    ToggleOptions,
    /// The pointer left the options panel: dismiss it.
    CloseOptions,
    /// Options panel: number meter rows by sort position.
    SetShowRanks(bool),
    /// Options panel: strip "-Realm" from player names on the meter.
    SetHideRealms(bool),
    /// Options panel: the chrome's colour — the game's gold, or yours.
    SetChrome(theme::Chrome),
    /// The talent viewer's own messages (`t` opens it; `talents.rs`).
    Talents(talents::Msg),
    /// Swallow clicks on the options panel's body so they don't fall
    /// through to the meter rows underneath.
    Noop,
    /// The top bar's Home place: Home, whatever is up. (`~` opens it or
    /// closes it, back to the pull on the stage.)
    GotoHome,
    /// The top bar's Fights place: the pull on the stage, Home aside.
    GotoFights,
    /// A pull on the rail was pressed: put it on the stage.
    Pull(Pull),
    /// The rail's "Hide trash" toggle.
    HideTrash,
    /// The rail's "Show older nights": the store's next page.
    OlderNights,
    /// The fight header's list button: open the rail's drawer.
    OpenRail,
    /// The drawer's scrim was pressed: close it.
    CloseRail,
    /// A view tab was clicked: the pointer twin of d/h/i/c/x/K/T.
    PickView(wowdps_model::View),
    /// The live pill: the log's newest pull, as `m`.
    GotoLive,
    /// A meter column heading was clicked: cycle its sort desc → asc → off.
    SortBy(crate::table::Col),
    /// A by-spell pane heading was clicked: the same cycle for the drill.
    SortSpellsBy(crate::table::Col),
    /// R26: a fold of the ability tree was pressed — a group's line, or a
    /// row's caret: open it, or shut it.
    TreeFold(String),
    /// R26 (step 2): the drill graph's "By ability" / "Total" button.
    ToggleStack,
    /// v28: a death chip was clicked — ask for that window's recap.
    PickDeath(u32),
    /// R25 (v35): a skull on the ribbon, or a row of the Deaths table, was
    /// pressed — the Deaths view on that death's recap (pushed over the
    /// meter in a narrow window).
    OpenDeath(crate::deaths::Pick),
    /// R21: the Taken drill's section chips — the panes, or the stack matrix.
    ShowStacks(bool),
    /// The fight header's step buttons: the pointer twins of `]` and `[`.
    NewerPull,
    OlderPull,
    /// The fight header's "you" chip: select the owner's row, and bring it
    /// into view.
    SelectOwner,
    /// A Home panel row: open that stored pull on the stage.
    OpenStored(String),
    /// `?`: show or hide the shortcut sheet.
    ToggleShortcuts,
    /// The jump box, or its glyph: the command palette, as Ctrl K.
    Jump,
    /// The palette's field changed.
    PaletteQuery(String),
    /// Enter in the palette's field, which captured it: run the selection.
    PaletteSubmit,
    /// A palette item was pressed: run it.
    PaletteRun(palette::Run),
    /// A press outside the palette's card, or Esc heard although its field
    /// captured it: close it.
    PaletteClose,
    /// The release of a press on the palette's field: its focus, back.
    PaletteFocus,
    /// The filter field's text changed.
    Filter(String),
    /// Home: scope it to this character's guid (`None`: every character of
    /// yours) — a chip, a pick of the picker's menu or the palette — and
    /// remember it as the scope Home opens on. It locks nothing. Away from
    /// Home, it opens Home so scoped.
    HomeCharacter(Option<String>),
    /// Open or close the character picker's menu.
    TogglePicker,
    /// The picker menu's follow item: a statement of what the window does,
    /// not a switch — the menu closes and the window says it.
    PickerFollow,
    /// The pointer entered (or left) a row of the picker's menu.
    PickerHover(Option<usize>),
    /// `/`, or a click on the field: focus it and start swallowing the
    /// meter keymap, so typing in it cannot quit the app or switch views.
    FocusFilter,
    /// The tick's answer to "does the filter field actually have focus?" —
    /// iced's own truth, which our gestures alone cannot know.
    FilterFocus(bool),
    /// Enter in the filter field: keep the text, give the keys back.
    FilterDone,
    /// Esc in the filter field, heard although the field captured it: give
    /// up the text and the keys.
    FilterEscape,
    /// The pointer entered (or left) a row of the meter or of a drill pane.
    HoverRow(Option<RowHover>),
    /// R12: the pointer entered (or left) a comparison spell-table row, by
    /// by-spell key. Both tables light that ability.
    CompareSpellHover(Option<String>),
    /// The window opened or was resized: its logical width at the zoom it
    /// was measured at.
    WindowWidth(f32),
}

/// Which row the pointer is over. Panes are told apart because the drill
/// draws two lists side by side and both answer the mouse.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RowHover {
    Meter(usize),
    Drill(wowdps_model::Pane, usize),
    /// R25: a row of the Deaths table, by its place in the raid's deaths.
    Death(usize),
}

/// `~` on a US layout arrives as `Character("~")`; on layouts where it is a
/// dead key the shifted backtick is what shows up instead, so both open Home.
fn is_home_key(key: &keyboard::Key, modifiers: keyboard::Modifiers) -> bool {
    match key {
        keyboard::Key::Character(c) if c.as_str() == "~" => true,
        keyboard::Key::Character(c) if c.as_str() == "`" => modifiers.shift(),
        _ => false,
    }
}

/// The window's theme: the tokens as iced's palette (`theme::window_theme`).
fn theme(_state: &Gui) -> Theme {
    theme::window_theme()
}

/// The translucent look the overlay panel has, for the whole window: the
/// tokens' ground at `window_alpha` (the surface is created `transparent:
/// true`, so the remainder shows the desktop through), parchment ink on it.
fn style(state: &Gui, theme: &Theme) -> iced::theme::Style {
    let palette = theme.palette();
    iced::theme::Style {
        background_color: iced::Color {
            a: state.cfg.window_alpha.clamp(0.0, 1.0),
            ..palette.background
        },
        text_color: palette.text,
    }
}

fn title(state: &Gui) -> String {
    match state.state.source.as_deref() {
        Some(name) => format!("wowdps — {name}"),
        None => "wowdps".to_string(),
    }
}

/// Ctrl K, the jump box's key: the command palette. Window-local, so not
/// `action_for`'s — where a control chord is nothing but Ctrl C.
fn is_jump_key(key: &keyboard::Key, modifiers: keyboard::Modifiers) -> bool {
    modifiers.control()
        && matches!(key, keyboard::Key::Character(c) if c.as_str().eq_ignore_ascii_case("k"))
}

fn update(state: &mut Gui, message: Message) -> Task<Message> {
    let mut requests = Vec::new();
    // Who was pinned before this message, so a pin it makes can say so.
    let pin_before = state
        .fight()
        .compare_picks()
        .first()
        .map(|(k, _)| k.clone());
    // The pull on the stage before this message, so a step that moves it
    // brings its row on the rail into sight.
    let pull_before = state.current_pull();
    // Set by `Tick`: ask the field itself whether it has focus. iced owns
    // that truth (a click focuses it, a click elsewhere unfocuses it) and
    // gives no callback for either, so the flag that swallows the keymap is
    // re-synced from the widget every tick rather than only from our own
    // gestures — otherwise a click away leaves the window keyboard-dead.
    let mut poll_focus = false;
    // A widget operation this message asks for beside the usual ones: the
    // meter's list following its selection.
    let mut follow: Option<Task<Message>> = None;
    match message {
        Message::Tick => {
            // The log's list before the drain: a visit that began or was
            // named since is one Home's places may be named by.
            let list_before = state.state.entries().len();
            let (intercepted, reconnected) = drain_client(
                &mut state.state,
                &mut state.client,
                &mut state.last_snapshot_at,
            );
            if reconnected {
                state.reconnected(&mut requests);
            }
            // v19: the answered loadout lands in the open talent viewer. A
            // `None` loadout leaves whatever the viewer opened with (stored
            // simc paste or the empty tree) — the silent fallback.
            let mut home_changed = state.state.entries().len() != list_before;
            let mut rail_changed = false;
            let mut store_changed = false;
            for msg in intercepted {
                match msg {
                    DaemonMsg::Loadout {
                        req_id, loadout, ..
                    } if state.pending_loadout == Some(req_id) => {
                        state.pending_loadout = None;
                        if let (Some(ui), Some(l)) = (state.talents.as_mut(), loadout) {
                            ui.adopt_logged(&l);
                        }
                    }
                    DaemonMsg::History { req_id, answer } => {
                        if let Some(ui) = state.home.as_mut() {
                            ui.absorb(req_id, &answer);
                            home_changed = true;
                        }
                        rail_changed |= state.earlier.absorb(req_id, &answer);
                        // The store's word on a pin (`p`): every card in hand
                        // says so, and the reader is told.
                        if let HistoryAnswer::Pinned { fight_id, pinned } = &answer {
                            if let Some(s) = state.stored.as_mut() {
                                s.pinned(fight_id, *pinned);
                            }
                            if let Some(c) = state
                                .home
                                .as_mut()
                                .and_then(|ui| ui.cards.iter_mut().find(|c| c.id == *fight_id))
                            {
                                c.pinned = *pinned;
                            }
                            state.say(if *pinned { PINNED } else { UNPINNED });
                        }
                    }
                    // The store's answer for the stored pull on the stage: its
                    // snapshot, and whatever the pull's state asks next.
                    DaemonMsg::Fight { req_id, fight } => {
                        if let Some(s) = state.stored.as_mut() {
                            requests.extend(s.absorb(req_id, fight, &mut state.next_req_id));
                        }
                        // The read a pull the reader left had out is back:
                        // nothing waits on it now.
                        if state.stray_fight == Some(req_id) {
                            state.stray_fight = None;
                        }
                    }
                    // The store wrote a fight: the lists the reader is looking
                    // at are now one pull out of date. No debounce needed —
                    // this arrives once per closed fight, not on a timer.
                    DaemonMsg::HistoryChanged { .. } => {
                        if let Some(ui) = state.home.as_mut() {
                            ui.reset();
                            home_changed = true;
                            store_changed = true;
                        }
                        // The rail's newest few cards, merged over what it
                        // holds: the reader's older pages stay, and a
                        // wipe-heavy night costs a small read per pull, not
                        // a whole page.
                        state.earlier.want_fresh();
                    }
                    DaemonMsg::Status { history, .. } => {
                        state.history_disabled = (!history.enabled).then(|| {
                            history
                                .error
                                .clone()
                                .unwrap_or_else(|| "no reason given".to_string())
                        });
                        state.history_dropped = history.dropped;
                        if let Some(ui) = state.home.as_mut() {
                            ui.disabled_reason = state.history_disabled.clone();
                            ui.dropped = state.history_dropped;
                        }
                    }
                    _ => {}
                }
            }
            // The store wrote something while Home is up: its enabled/dropped
            // state may have moved too. Debounced, because a wipe-heavy night
            // closes fights faster than anyone reads a banner.
            if store_changed
                && state
                    .last_status_at
                    .is_none_or(|at| at.elapsed() >= STATUS_REFRESH)
            {
                state.last_status_at = Some(Instant::now());
                let ask = state.ask_status();
                requests.push(ask);
            }
            // The rail's pages name the characters you played, as Home's
            // answers do: the top bar's picker offers them without a visit
            // to Home.
            if rail_changed {
                let cards: Vec<&wowdps_proto::history::FightCard> =
                    state.earlier.cards.iter().collect();
                let seen = home::character_lines(&cards, &[]);
                state.remember_characters(seen);
                // Nobody resolved yet: whoever played the rail's newest
                // stored pull is who the picker shows until Home says.
                if state.owner_guid.is_none() {
                    state.owner_guid = state
                        .earlier
                        .cards
                        .iter()
                        .filter(|c| c.owner.is_some())
                        .max_by_key(|c| c.start_utc_ms)
                        .and_then(|c| c.owner.clone());
                }
            }
            // One request in flight, whatever asked for it — and a second
            // asking whose pause is over goes out.
            state.ask_earlier(&mut requests);
            if let Some(s) = state.stored.as_mut() {
                requests.extend(s.tick(Instant::now(), &mut state.next_req_id));
            }
            if home_changed {
                state.rederive_home();
                // Keep reading until the week is in hand: one request in
                // flight, and only while Home is the screen on show.
                let req_id = state.next_req_id();
                if let Some(msg) = state
                    .home
                    .as_mut()
                    .and_then(|ui| ui.next_request(req_id, &state.season))
                {
                    requests.push(msg);
                }
            }
            // Cheap while unresolved, a no-op forever after: the accent must
            // not be recomputed per snapshot.
            state.resolve_accent();
            // What this view says that the header will want on another:
            // whether the owner is in the fight.
            let rows = state.fight().rows();
            let owner = state.owner_in(&rows).and_then(|i| rows.get(i));
            let fight = state.stored.as_ref().map_or(&state.state, |s| &s.state);
            state.seen.observe(fight, owner);
            // Who is who, for the inspector's lanes: every player a meter
            // names (never the enemies'), and a comparison's two.
            if state.fight().view != View::EnemyTaken {
                state.roster.observe(&rows);
            }
            if let Some((a, b)) = state.fight().compare_sides() {
                let pair = [a.total.clone(), b.total.clone()];
                state.roster.observe(&pair);
            }
            // Home is a front door: a pull STARTING replaces it with the
            // meter, but a fight that was already live when Home was opened
            // deliberately does not (the reader asked for Home).
            let live = state.state.is_live();
            if live && !state.was_live {
                state.home = None;
            }
            state.was_live = live;
            // Offer Home once, after the first snapshot: opening it earlier
            // would flash the dashboard over a pull already in progress.
            if !state.home_considered && state.last_snapshot_at.is_some() {
                state.home_considered = true;
                if state.cfg.home_on_start && !live {
                    state.open_home(&mut requests);
                }
            }
            poll_focus = true;
        }
        Message::Key(event) => {
            if let keyboard::Event::KeyPressed {
                modified_key,
                modifiers,
                text: typed,
                ..
            } = event
            {
                let escape = modified_key == keyboard::Key::Named(keyboard::key::Named::Escape);
                if let Some(zoom) = keys::zoom_for(&modified_key, modifiers) {
                    state.cfg.zoom = match zoom {
                        keys::Zoom::In => state.cfg.zoom + ZOOM_STEP,
                        keys::Zoom::Out => state.cfg.zoom - ZOOM_STEP,
                        keys::Zoom::Reset => Config::default().zoom,
                    }
                    .clamp(*ZOOM_RANGE.start(), *ZOOM_RANGE.end());
                    state.cfg.save();
                } else if let Some(ui) = state.talents.as_mut() {
                    // The viewer swallows the meter keymap: its text input
                    // must be typable without "q" quitting or "d" switching
                    // views. Esc closes it; Tab flips talents/inventory.
                    if escape {
                        state.talents = None;
                        // A parked reply must not land in a viewer opened
                        // later for someone else.
                        state.pending_loadout = None;
                    } else if modified_key == keyboard::Key::Named(keyboard::key::Named::Tab) {
                        ui.on_msg(talents::Msg::ToggleTab);
                    }
                } else if state.palette.is_some() {
                    // The palette has every key: a name typed into it must
                    // not quit at its `q` or pin at its `p`.
                    follow = state.palette_key(
                        &modified_key,
                        modifiers,
                        typed.as_deref(),
                        &mut requests,
                    );
                } else if is_jump_key(&modified_key, modifiers) {
                    // From anywhere — the filter's field, the sheet, a menu
                    // — the palette replaces what was up.
                    follow = Some(state.open_palette());
                } else if state.picker_open {
                    // The menu is modal the way the sheet is: any key closes
                    // it and does nothing else.
                    state.picker_open = false;
                } else if state.options_open {
                    // So is the ⚙ card: Esc (any key) closes it, and nothing
                    // typed while it is up reaches the meter under it.
                    state.options_open = false;
                } else if state.shortcuts_open {
                    // The sheet is a modal over everything: any key dismisses
                    // it and does nothing else, so a key pressed to close it
                    // never also switches a view.
                    state.shortcuts_open = false;
                } else if state.filter_focused && state.filter_visible() {
                    // The keymap is a global subscription, so while the field
                    // has focus every key must be left to it — otherwise
                    // typing "q" quits the app mid-word. Esc gives up and
                    // clears; Enter keeps the text and gives the keys back.
                    match &modified_key {
                        keyboard::Key::Named(keyboard::key::Named::Escape) => {
                            state.filter.clear();
                            state.filter_focused = false;
                        }
                        keyboard::Key::Named(keyboard::key::Named::Enter) => {
                            state.filter_focused = false;
                        }
                        _ => {}
                    }
                } else if is_home_key(&modified_key, modifiers) {
                    if state.home.is_some() {
                        state.home = None;
                    } else {
                        state.open_home(&mut requests);
                    }
                } else if modified_key == keyboard::Key::Character("?".into()) {
                    state.shortcuts_open = true;
                } else if modified_key == keyboard::Key::Character("/".into())
                    && state.filter_visible()
                {
                    // The filter is the meter's: a narrow window's pushed
                    // inspector steps aside for it, and the keys go back to
                    // the rows it narrows.
                    state.fight_mut().uninspect();
                    state.filter_focused = true;
                    return iced::widget::operation::focus(crate::nav::filter_id());
                } else if modified_key == keyboard::Key::Character("m".into()) {
                    // Back to the live meter from anywhere, through the
                    // accessor that already exists rather than a new Action
                    // — and to the METER: a narrow window's pushed inspector
                    // gives the keys back, or on the live pull already
                    // (where `pin_live` has nothing to do) `m` would change
                    // nothing the reader can see.
                    state.go_live(&mut requests);
                } else if modified_key == keyboard::Key::Character("H".into()) {
                    // The rail, at the nights before tonight's.
                    follow = Some(state.open_earlier(&mut requests));
                } else if escape && state.drawer_open() {
                    // The drawer is over whatever it was opened on — the
                    // stage or Home — and goes first.
                    state.rail_open = false;
                } else if state.drawer_open() {
                    // The drawer is over the stage, which the scrim dims: the
                    // keys that walk the rail or leave it pass (`[` `]`, and
                    // m ~ H ? above), and the stage's own — j, k, Enter,
                    // Tab, the views, v, g, t, p — would change what the
                    // reader cannot see while choosing a pull, so they do
                    // nothing until it closes.
                    match keys::action_for(&modified_key, modifiers) {
                        // The stage steps along the rail under the drawer,
                        // which stays open on the row it stepped to.
                        Some(Action::OlderSegment) => state.step_pull(true, &mut requests),
                        Some(Action::NewerSegment) => state.step_pull(false, &mut requests),
                        // The drawer's own highlight: j, k and the arrows
                        // walk the rows it draws, Enter opens the one it is
                        // on and closes the drawer.
                        Some(step @ (Action::Down | Action::Up)) => {
                            let from = state.rail_cursor.clone().or_else(|| state.current_pull());
                            if let Some(to) = state.rail().step(
                                from.as_ref(),
                                step == Action::Down,
                                state.hide_trash,
                            ) {
                                state.rail_cursor = Some(to);
                                follow = Some(state.keep_cursor_in_sight());
                            }
                        }
                        Some(Action::Open) => {
                            state.rail_open = false;
                            if let Some(pull) = state.rail_cursor.clone() {
                                state.go_pull(pull, &mut requests);
                            }
                        }
                        Some(Action::Quit) => state.state.quit = true,
                        _ => {}
                    }
                } else if state.home.is_some() && escape {
                    // Home is where Esc's chain ENDS — the front door every
                    // Esc leads to — so Esc there leaves it standing rather
                    // than toggle it shut (`~`, `m`, a view key and the
                    // places leave it). It sits ABOVE the stage, so no step
                    // reaches `Action::Back`.
                } else if state.home.is_some() {
                    // Home stands over the stage, and its keys are its own:
                    // a view key or a pull step leaves it for the pull it
                    // names, `q` quits, `t` opens the talent viewer on
                    // nobody — and j, k, Enter, v, g never reach the meter
                    // hidden under it.
                    match keys::action_for(&modified_key, modifiers) {
                        Some(action @ Action::SetView(_)) if !state.stored_refuses(action) => {
                            state.home = None;
                            state.log_view = None;
                            requests.extend(state.on_fight(|s| s.apply(action)));
                        }
                        Some(Action::OlderSegment) => state.step_pull(true, &mut requests),
                        Some(Action::NewerSegment) => state.step_pull(false, &mut requests),
                        Some(Action::Quit) => state.state.quit = true,
                        _ if modified_key == keyboard::Key::Character("t".into())
                            && !modifiers.control() =>
                        {
                            state.open_talents(false, &mut requests);
                        }
                        _ => {}
                    }
                } else if state.fight().screen == Screen::List && escape {
                    // No pull on the stage yet (an empty log): Esc lands on
                    // Home, the front door, where the chain ends.
                    state.open_home(&mut requests);
                } else if escape && state.stage_escape(&mut requests) {
                    // The stage's chain: filter, ability, inspector,
                    // comparison, Home.
                } else if state.tree_arrow(&modified_key) {
                    // R26: ← → fold the ability tree the keys are in.
                } else if let Some(step) = state.death_step(&modified_key) {
                    // ← → step the death windows of the recap the keys are
                    // in, where the pull keys would otherwise leave it.
                    if let Some(i) = step {
                        requests.extend(state.on_fight(|s| s.select_death(Some(i))));
                    }
                } else if modified_key == keyboard::Key::Character("t".into())
                    && !modifiers.control()
                {
                    state.open_talents(true, &mut requests);
                } else if modified_key == keyboard::Key::Character("p".into())
                    && !modifiers.control()
                {
                    // Pin the pull on the stage, or let it go: the store
                    // keeps a pinned fight whatever its retention says.
                    state.pin(&mut requests);
                } else if let Some(action) = keys::action_for(&modified_key, modifiers) {
                    // R25: a step walked the Deaths table, whose list keeps
                    // its own place in sight.
                    let mut stepped_death = false;
                    match action {
                        // The pull keys walk the rail — tonight's log, then
                        // the stored nights — not the log's segment order.
                        Action::OlderSegment => state.step_pull(true, &mut requests),
                        Action::NewerSegment => state.step_pull(false, &mut requests),
                        // The WINDOW quits, whichever pull is on the stage:
                        // a stored pull's own state is not what the window
                        // reads its quit from.
                        Action::Quit => state.state.quit = true,
                        // What a stored pull keeps no answer for says so.
                        _ if state.stored_refuses(action) => {
                            if let Some(words) = state.stored_refusal(action) {
                                state.say(words);
                            }
                        }
                        _ if state.stage_key(action) => {}
                        // A filtered list is what the reader can SEE, so
                        // j/k must walk it: stepping through hidden rows
                        // would park the highlight on nothing and inspect a
                        // stranger.
                        _ => match state.filtered_step(action) {
                            Some(Step::Meter(row)) => {
                                requests.extend(state.on_fight(|s| s.select_row(row)));
                            }
                            Some(Step::Spell(row)) => {
                                if let Some(d) = state.fight_mut().drill.as_mut() {
                                    d.spell_sel = row;
                                }
                            }
                            Some(Step::Tree(node)) => state.tree_rest(node),
                            Some(Step::Death(i)) => {
                                let pick =
                                    state.fight().raid().and_then(|r| r.deaths.get(i)).map(|d| {
                                        crate::deaths::Pick {
                                            key: d.guid.clone(),
                                            label: d.name.clone(),
                                            index: d.index,
                                        }
                                    });
                                if let Some(p) = pick {
                                    requests.extend(
                                        state.on_fight(|s| s.open_death(&p.key, &p.label, p.index)),
                                    );
                                    stepped_death = true;
                                    state.reveal_death = Some((p.key.clone(), p.index));
                                }
                            }
                            Some(Step::Stay) => {}
                            None => {
                                // A view the reader chose is theirs: no step
                                // back to the log puts another in its place.
                                if matches!(action, Action::SetView(_)) {
                                    state.log_view = None;
                                }
                                requests.extend(state.on_fight(|s| s.apply(action)));
                            }
                        },
                    }
                    // The selection a step moved stays in sight: past the
                    // fold the list follows it, or Enter would drill into a
                    // row the reader cannot see — the meter's, or the
                    // inspector's once the keys are there.
                    let moves = matches!(
                        action,
                        Action::Up | Action::Down | Action::Open | Action::SwapPane
                    );
                    if moves && state.fight().inspecting() {
                        follow = Some(state.keep_keyed_in_sight());
                    } else if stepped_death {
                        follow = Some(state.keep_death_in_sight());
                    } else if matches!(action, Action::Up | Action::Down) {
                        follow = Some(state.keep_row_in_sight(state.fight().row_sel));
                    }
                }
            }
        }
        // A click selects, and the inspector follows the selection.
        Message::MeterRow(row) => requests.extend(state.on_fight(|s| s.select_row(row))),
        // A narrow window's click: select, and push the inspector over the
        // meter to show it.
        Message::PushRow(row) => {
            requests.extend(state.on_fight(|s| s.select_row(row)));
            state.fight_mut().inspect();
        }
        // R12: the class icon is the pin. On a player with nothing pinned it
        // pins them (the keys' `v` on their row); on another player while one
        // is pinned it makes them the second half, as moving onto them
        // would; on the pinned player it stops comparing. A stored pull
        // keeps no comparison to make.
        Message::CompareRow(_) if state.stored.is_some() => {}
        Message::CompareRow(row) => {
            let key = state.state.rows().get(row).map(|r| r.key.clone());
            let pin = state.state.compare_picks().first().map(|(k, _)| k.clone());
            match (pin, key) {
                (Some(pin), Some(key)) if pin == key => {
                    requests.extend(state.state.apply(Action::PickCompare));
                }
                (Some(_), Some(_)) => requests.extend(state.state.select_row(row)),
                (None, Some(_)) => {
                    requests.extend(state.state.select_row(row));
                    requests.extend(state.state.apply(Action::PickCompare));
                }
                (_, None) => {}
            }
        }
        Message::PinCompare if state.stored.is_some() => {}
        Message::PinCompare => requests.extend(state.state.apply(Action::PickCompare)),
        Message::Uninspect => state.fight_mut().uninspect(),
        // The ability strip's back: close the ability (the inspector's or
        // the pair's), one level, as Esc does.
        Message::CloseAbility => {
            let app = state.fight();
            if app.drill_spell().is_some() || app.compare_spell().is_some() {
                requests.extend(state.on_fight(|s| s.apply(Action::Back)));
            }
        }
        // A list's tab: that list, in the place of R21's matrix too.
        Message::InspectorTab(pane) => {
            if let Some(d) = state.fight_mut().drill.as_mut() {
                d.pane = pane;
            }
            state.stacks_open = false;
        }
        Message::ToggleGraph => state.fight_mut().toggle_graph(),
        Message::OpenTalents => state.open_talents(true, &mut requests),
        Message::ClearCompare => {
            requests.extend(state.on_fight(ClientState::clear_compare));
        }
        Message::CompareRange(range) => {
            requests.extend(state.on_fight(|s| s.set_compare_range(range)));
        }
        Message::DrillRange(range) => {
            requests.extend(state.on_fight(|s| s.set_drill_range(range)));
        }
        // v16: select the clicked ability, then Open descends into it — the
        // keys go with it into the inspector, where Esc backs out. A stored
        // pull keeps no ability's own curve: the row is selected, no more.
        Message::SpellRow(i) => {
            state.tree_cursor = None;
            if let Some(d) = state.fight_mut().drill.as_mut() {
                d.spell_sel = i;
                d.pane = wowdps_model::Pane::Spell;
            }
            state.fight_mut().inspect();
            if state.stored.is_none() {
                requests.extend(state.state.apply(Action::Open));
            }
        }
        Message::AttackerRow(i) => {
            if let Some(d) = state.fight_mut().drill.as_mut() {
                d.target_sel = i;
                d.pane = wowdps_model::Pane::Target;
            }
            state.fight_mut().inspect();
            requests.extend(state.on_fight(|s| s.apply(Action::Open)));
        }
        Message::CompareSpell((key, label)) => {
            requests.extend(state.on_fight(|s| s.drill_compare_spell(&key, &label)));
        }
        Message::ToggleOptions => state.options_open = !state.options_open,
        Message::CloseOptions => state.options_open = false,
        Message::SetShowRanks(on) => {
            state.cfg.show_ranks = on;
            state.cfg.save();
        }
        Message::SetHideRealms(on) => {
            state.cfg.hide_realms = on;
            state.cfg.save();
        }
        Message::SetChrome(chrome) => {
            state.cfg.chrome = chrome.name().to_string();
            state.cfg.save();
            state.accent = chrome_accent(chrome, state.owner_class);
        }
        Message::Talents(msg) => match msg {
            talents::Msg::Close => {
                state.talents = None;
                state.pending_loadout = None;
            }
            // The clipboard read is a Task; its contents come back as
            // another Talents message.
            talents::Msg::PasteClipboard if state.talents.is_some() => {
                return iced::clipboard::read()
                    .map(|c| Message::Talents(talents::Msg::Clipboard(c)));
            }
            // Encode the current (possibly edited) build to the clipboard.
            talents::Msg::CopyString => {
                if let Some(s) = state
                    .talents
                    .as_ref()
                    .and_then(talents::TalentsUi::encode_current)
                {
                    return iced::clipboard::write(s);
                }
            }
            msg => {
                if let Some(ui) = state.talents.as_mut() {
                    ui.on_msg(msg);
                }
            }
        },
        Message::Noop => {}
        Message::GotoHome => {
            if state.home.is_none() {
                state.open_home(&mut requests);
            }
        }
        Message::GotoFights => state.home = None,
        // A pick closes the drawer it was made in.
        Message::Pull(pull) => {
            state.rail_open = false;
            state.go_pull(pull, &mut requests);
        }
        Message::HideTrash => state.hide_trash = !state.hide_trash,
        Message::OlderNights => {
            state.earlier.want_older();
            state.ask_earlier(&mut requests);
        }
        // The drawer opens on the pull on the stage, its keys on its row:
        // the list is built afresh each time it opens, at its top, and
        // stands where the row is in sight with room under it — its
        // night's heading at the top when both fit, else the row centred.
        Message::OpenRail => {
            state.open_rail();
            state.rail_open = true;
            follow = Some(state.open_on_pull());
        }
        Message::CloseRail => state.rail_open = false,
        // A view a stored pull lacks is a disabled tab, which sends
        // nothing; this holds the line for any other way here.
        Message::PickView(view) if state.stored_refuses(Action::SetView(view)) => {}
        Message::PickView(view) => {
            state.home = None;
            state.log_view = None;
            requests.extend(state.on_fight(|s| s.apply(Action::SetView(view))));
        }
        Message::GotoLive => state.go_live(&mut requests),
        Message::SortBy(col) => {
            // The cycle starts from what is drawn: a choice another view's
            // column made is no sort here, so its heading starts afresh.
            state.sort = match state.meter_sort() {
                Some((c, true)) if c == col => Some((col, false)),
                Some((c, false)) if c == col => None,
                _ => Some((col, true)),
            };
        }
        Message::ShowStacks(on) => state.stacks_open = on,
        Message::NewerPull => state.step_pull(false, &mut requests),
        Message::OlderPull => state.step_pull(true, &mut requests),
        Message::SelectOwner => {
            let rows = state.fight().rows();
            if let Some(owner) = state.owner_in(&rows) {
                // The selection is always on a drawn row: a filter that
                // hides the owner gives way to the press that asked for
                // them, rather than leave the highlight — and Enter's
                // drill — on a row the reader cannot see.
                let hidden = !view::ordered(rows, &state.filter, state.meter_sort())
                    .iter()
                    .any(|(i, _)| *i == owner);
                if hidden {
                    state.filter.clear();
                }
                // The inspector follows them there.
                requests.extend(state.on_fight(|s| s.select_row(owner)));
                // Into view, the least that shows it whole: nothing when it
                // already is.
                follow = Some(state.keep_row_in_sight(owner));
            }
        }
        Message::OpenStored(id) => {
            // Home's card, lent to the rail so it can place the pull.
            if let Some(card) = state
                .home
                .as_ref()
                .and_then(|ui| ui.cards.iter().find(|c| c.id == id))
                .cloned()
            {
                state.earlier.adopt(card);
            }
            state.go_pull(Pull::Stored(id), &mut requests);
        }
        Message::PickDeath(i) => {
            requests.extend(state.on_fight(|s| s.select_death(Some(i))));
        }
        Message::OpenDeath(pick) => {
            // The Deaths view is the reader's choice now, as a tab's is.
            state.log_view = None;
            let narrow = state
                .fit()
                .is_none_or(|f| f == crate::inspector::Fit::Narrow);
            requests.extend(state.on_fight(|s| s.open_death(&pick.key, &pick.label, pick.index)));
            state.reveal_death = Some((pick.key.clone(), pick.index));
            // A narrow window has no inspector beside the meter: the recap
            // is pushed over it, as a press on a meter row pushes a drill.
            // Beside it the table keeps the keys, whoever had them before.
            if narrow {
                state.fight_mut().inspect();
            } else {
                state.fight_mut().uninspect();
            }
            follow = Some(state.keep_death_in_sight());
        }
        Message::SortSpellsBy(col) => {
            state.drill_sort = match state.drill_sort {
                Some((c, true)) if c == col => Some((col, false)),
                Some((c, false)) if c == col => None,
                _ => Some((col, true)),
            };
        }
        Message::TreeFold(key) => state.fold(&key),
        Message::ToggleStack => state.stack_graph = !state.stack_graph,
        Message::TogglePicker => {
            state.picker_open = !state.picker_open;
            state.picker_hover = None;
        }
        Message::PickerHover(at) => state.picker_hover = at,
        Message::PickerFollow => {
            state.picker_open = false;
            state.say(crate::nav::FOLLOW_NOTE);
        }
        Message::HomeCharacter(guid) => {
            state.picker_open = false;
            state.scope_home(guid, &mut requests);
        }
        Message::ToggleShortcuts => state.shortcuts_open = !state.shortcuts_open,
        Message::Jump => follow = Some(state.open_palette()),
        Message::PaletteQuery(query) => {
            if let Some(p) = state.palette.as_mut() {
                p.typed(query);
            }
        }
        Message::PaletteSubmit => follow = state.palette_submit(&mut requests),
        Message::PaletteRun(run) => follow = state.run_palette(run, &mut requests),
        Message::PaletteClose => state.palette = None,
        Message::PaletteFocus => {
            if state.palette.is_some() {
                follow = Some(iced::widget::operation::focus(palette::input_id()));
            }
        }
        Message::Filter(text) => {
            state.filter = text;
            // The inspector is the selection's: a filter that hides the
            // selected row moves the selection to the first row it draws,
            // so the detail is always of a row in the list (the
            // prototype's `ensureSel`).
            let app = state.fight();
            let drawn = crate::view::ordered(app.rows(), &state.filter, state.meter_sort());
            if let Some((first, _)) = drawn.first()
                && !drawn.iter().any(|(i, _)| *i == app.row_sel)
            {
                let first = *first;
                requests.extend(state.on_fight(|s| s.select_row(first)));
            }
        }
        Message::FocusFilter => {
            if state.filter_visible() {
                state.filter_focused = true;
                return iced::widget::operation::focus(crate::nav::filter_id());
            }
        }
        // iced's own answer wins over anything we inferred from a gesture.
        Message::FilterFocus(on) => state.filter_focused = on,
        // Enter in the field: the text stays and the keys come back — and
        // iced's focus goes with the flag, or the next tick's poll would
        // find the field still focused and swallow the keymap again.
        Message::FilterDone => {
            state.filter_focused = false;
            follow = Some(unfocus());
        }
        // Esc in the field, which has already let go of iced's focus (and
        // captured the key, so the keymap never heard it): the filter gives
        // up its text too.
        Message::FilterEscape => {
            if state.filter_focused {
                state.filter.clear();
                state.filter_focused = false;
            }
        }
        Message::HoverRow(at) => state.row_hover = at,
        Message::CompareSpellHover(key) => state.spell_hover = key,
        // Held at zoom 1: the stage is drawn at the zoom of the moment. A
        // window grown past the drawer's width puts the rail beside the
        // stage, and the drawer has nothing to be open over.
        Message::WindowWidth(w) => {
            state.window_w = Some(w * state.cfg.zoom);
            if state.rail_docked() {
                state.rail_open = false;
            }
        }
    }
    // The window never shows the fight list: with no pull on the stage —
    // a launch, a rotated log — and the stage in sight, the log's newest
    // takes it, as the list's Enter used to.
    if state.home.is_none()
        && state.stored.is_none()
        && state.state.screen == Screen::List
        && !state.state.entries().is_empty()
    {
        requests.extend(state.state.pin_live());
    }
    for req in requests {
        state.client.send(&req);
    }
    // A pin this message made says so for a moment (the prototype's
    // toast); the pair it asks for, or the pin's end, takes the word back.
    let pin = state.fight().compare_picks().first().cloned();
    let screen = state.fight().screen;
    match pin {
        Some((key, label))
            if screen == Screen::Meter && pin_before.as_deref() != Some(key.as_str()) =>
        {
            let name = if state.cfg.hide_realms {
                crate::view::display_name(&label).to_string()
            } else {
                label
            };
            state.toast = Some((
                format!("Pinned {name}. Move to another player to compare."),
                Instant::now(),
            ));
        }
        Some(_) if screen != Screen::Compare => {}
        // The pair formed, or the pin ended: its word goes with it. A word
        // about anything else (a pin of the pull, a refusal) keeps its time.
        Some(_) => state.toast = None,
        None if pin_before.is_some() => state.toast = None,
        None => {}
    }
    if state
        .toast
        .as_ref()
        .is_some_and(|(_, at)| at.elapsed() >= TOAST_FOR)
    {
        state.toast = None;
    }
    // Hold the inspector's body once per answered player, view, list and
    // mode: the next move drops its breakdown, and this stands in, dimmed,
    // until that player's lands.
    if state.fight().drill_breakdown().is_some()
        && !state
            .insp_held
            .as_ref()
            .is_some_and(|h| h.current(state.fight()))
    {
        state.insp_held = crate::inspector::Held::of(state);
    }
    // R26: the stacked graph's curves take their hues — a curve seated
    // once keeps its hue while it stays in the stack.
    if let Some((context, keys)) = crate::inspector::stack_keys(state.fight()) {
        state.stack_slots.observe(&context, &keys);
    }
    // R25: an opened death's recap has landed — its killing blow, which
    // ends the list, is brought into sight (the least scroll that shows it;
    // none when it already is).
    if let Some((key, index)) = state.reveal_death.clone() {
        let app = state.fight();
        let landed = app.view == View::Deaths
            && app.drill.as_ref().is_some_and(|d| d.key == key)
            && app.deaths().1 == Some(index)
            && !app.breakdown().0.is_empty();
        let gone = app.view != View::Deaths || app.drill.as_ref().is_none_or(|d| d.key != key);
        if landed || gone {
            state.reveal_death = None;
        }
        if landed {
            let reveal = iced::advanced::widget::operate::<()>(FindRow {
                scroll: crate::inspector::scroll_id(),
                row: crate::inspector::recap_kill_id(),
                content: None,
                found: None,
            })
            .discard();
            follow = Some(match follow {
                Some(task) => Task::batch([task, reveal]),
                None => reveal,
            });
        }
    }
    // A player the palette selected before their view's rows landed: the
    // snapshot that brings their row scrolls the meter to it. Anything
    // that takes the drill off them, or a chart answered without them,
    // ends the wait.
    if let Some(key) = state.reveal_player.clone() {
        let app = state.fight();
        let theirs = state.home.is_none() && app.drill.as_ref().is_some_and(|d| d.key == key);
        let answered = crate::fight_head::player_chart(app.view) && app.view_answered();
        let at = app.rows().iter().position(|r| r.key == key);
        if !theirs || answered {
            state.reveal_player = None;
        }
        if let Some(row) = at.filter(|_| theirs && answered) {
            let reveal = state.keep_row_in_sight(row);
            follow = Some(match follow {
                Some(task) => Task::batch([task, reveal]),
                None => reveal,
            });
        }
    }
    // The palette's list may have changed under it (a live pull, the
    // players refilled, a rail page): its selection stays on a line it
    // lists.
    if state.palette.is_some() {
        let n = state.palette_items().len();
        if let Some(p) = state.palette.as_mut() {
            p.clamp(n);
        }
    }
    // R25: whatever route reached the Deaths table — a skull, a view key, a
    // pull stepped onto, a window widened — it holds the keys beside the
    // inspector.
    state.deaths_table_takes_the_keys();
    // A step that moved the pull brings its row on the rail into sight.
    if state.current_pull() != pull_before && state.current_pull().is_some() {
        let reveal = state.keep_pull_in_sight();
        follow = Some(match follow {
            Some(task) => Task::batch([task, reveal]),
            None => reveal,
        });
    }

    let task = if state.state.quit {
        iced::exit()
    } else if poll_focus {
        // Answers nothing when the field is not on screen — which is why
        // `filter_focused` is only ever SET while the meter draws it.
        iced::widget::operation::is_focused(crate::nav::filter_id()).map(Message::FilterFocus)
    } else {
        Task::none()
    };
    match follow {
        Some(follow) => Task::batch([follow, task]),
        None => task,
    }
}

/// Drop iced's focus from whatever holds it — the filter field, when the
/// window gives its keys back.
fn unfocus() -> Task<Message> {
    iced::advanced::widget::operate::<()>(iced::advanced::widget::operation::focusable::unfocus())
        .discard()
}

/// The keys a focused field takes for itself, as the window must still
/// hear them: iced's text input captures Escape (it lets go of its focus),
/// and `keyboard::listen` hears only what no widget captured — so the
/// window's "Esc gives up and clears" would never run. Listened for only
/// while the filter holds the keys.
fn captured_escape(
    event: iced::Event,
    status: iced::event::Status,
    _window: window::Id,
) -> Option<Message> {
    match (event, status) {
        (
            iced::Event::Keyboard(keyboard::Event::KeyPressed {
                key: keyboard::Key::Named(keyboard::key::Named::Escape),
                ..
            }),
            iced::event::Status::Captured,
        ) => Some(Message::FilterEscape),
        _ => None,
    }
}

/// The same for the command palette's field: Esc there closes the palette,
/// though the field took the key for itself.
fn palette_escape(
    event: iced::Event,
    status: iced::event::Status,
    window: window::Id,
) -> Option<Message> {
    captured_escape(event, status, window).map(|_| Message::PaletteClose)
}

/// The window's width as it opens and each time it is resized, in the
/// logical pixels of the zoom it was measured at.
fn window_width(
    event: iced::Event,
    _status: iced::event::Status,
    _window: window::Id,
) -> Option<Message> {
    match event {
        iced::Event::Window(window::Event::Opened { size, .. } | window::Event::Resized(size)) => {
            Some(Message::WindowWidth(size.width))
        }
        _ => None,
    }
}

fn subscription(state: &Gui) -> Subscription<Message> {
    let mut subs = vec![
        time::every(TICK).map(|_| Message::Tick),
        keyboard::listen().map(Message::Key),
        iced::event::listen_with(window_width),
    ];
    if state.filter_focused && state.filter_visible() {
        subs.push(iced::event::listen_with(captured_escape));
    }
    if state.palette.is_some() {
        subs.push(iced::event::listen_with(palette_escape));
    }
    Subscription::batch(subs)
}

/// Test scaffolding shared by the window's render tests (`view.rs`,
/// `compare.rs`, `gauge.rs`): a `DaemonClient` over a socketpair, a bridge
/// that lets the daemon's in-process mock answer that socket, and headless
/// rendering through `iced_test` (tiny-skia, no display).
#[cfg(test)]
pub(crate) mod testkit {
    use std::io::Write;
    use std::os::unix::net::UnixStream;
    use std::sync::Once;

    use iced::Element;
    use wowdps_daemon::mock::{MockDaemon, pump};
    use wowdps_model::Action;
    use wowdps_proto::{
        ClientKind, ClientMsg, ClientState, DaemonClient, DaemonMsg, PROTO_VERSION,
    };

    use super::{Gui, Message, update};
    use crate::config::Config;

    /// A client whose handshake a thread on the peer end answered; the peer
    /// comes back so a test can play daemon (or drop it to play a crash).
    pub(crate) fn fake_client() -> (DaemonClient, UnixStream) {
        fake_client_as(ClientKind::Window)
    }

    /// `fake_client` for another kind of session — the overlay's guard
    /// connects as the overlay does.
    pub(crate) fn fake_client_as(kind: ClientKind) -> (DaemonClient, UnixStream) {
        let (ours, theirs) = UnixStream::pair().unwrap();
        let mut peer = theirs.try_clone().unwrap();
        let ack = std::thread::spawn(move || {
            let (tag, body) = wowdps_proto::wire::read_frame(&mut peer).unwrap();
            let hello = ClientMsg::decode(tag, &body).unwrap();
            assert!(matches!(hello, ClientMsg::Hello { .. }), "{hello:?}");
            peer.write_all(
                &DaemonMsg::HelloAck {
                    proto: PROTO_VERSION,
                    version: "test".to_string(),
                }
                .encode(),
            )
            .unwrap();
        });
        let client = DaemonClient::over(ours, kind).unwrap();
        ack.join().unwrap();
        (client, theirs)
    }

    /// The config a test window starts with: the shipping defaults, minus
    /// the Home-on-start offer. Home is a screen a test opens deliberately;
    /// having it appear under every meter test would make them all about
    /// Home. `home_tests_config` is the other half.
    pub(crate) fn test_config() -> Config {
        Config {
            home_on_start: false,
            ..Config::default()
        }
    }

    /// A window over a pre-driven state. The socket peer is kept alive but
    /// silent, so the client neither answers nor looks dead.
    pub(crate) fn gui_over(state: ClientState) -> (Gui, UnixStream) {
        let (client, peer) = fake_client();
        (Gui::for_test(client, state, test_config()), peer)
    }

    /// Keep every config write these tests trigger out of the real
    /// `~/.config`: a process-wide scratch `XDG_CONFIG_HOME`, set once.
    /// One scratch root for the whole test binary: the overlay's and
    /// Hyprland's tests set it through the same `Once`
    /// (`hypr::fake::test_env`), so which test runs first can never decide
    /// where a config lands.
    pub(crate) fn isolate_config() {
        let _ = crate::hypr::fake::test_env();
    }

    // ---- ClientState builders over the mock daemon ---------------------

    pub(crate) fn apply(state: &mut ClientState, mock: &mut MockDaemon, action: Action) {
        let reqs = state.apply(action);
        pump(state, mock, reqs);
    }

    /// Indexed startup over the whole fixture: the list screen.
    pub(crate) fn indexed() -> (ClientState, MockDaemon) {
        indexed_on(MockDaemon::fixture())
    }

    /// The same over a mock the caller set up — one that knows the
    /// account's characters (`with_characters`), say.
    pub(crate) fn indexed_on(mut mock: MockDaemon) -> (ClientState, MockDaemon) {
        let mut state = ClientState::new();
        let first = state.initial_request();
        pump(&mut state, &mut mock, vec![first]);
        (state, mock)
    }

    /// The meter on the newest segment: the final wipe.
    pub(crate) fn wipe() -> (ClientState, MockDaemon) {
        let (mut state, mut mock) = indexed();
        apply(&mut state, &mut mock, Action::Open);
        (state, mock)
    }

    /// Mid-fight arrival: the live meter.
    pub(crate) fn live() -> (ClientState, MockDaemon) {
        let mut mock = MockDaemon::fixture_live();
        let mut state = ClientState::new();
        let first = state.initial_request();
        pump(&mut state, &mut mock, vec![first]);
        (state, mock)
    }

    /// The boss kill — the fixture's richest segment.
    pub(crate) fn kill() -> (ClientState, MockDaemon) {
        kill_on(MockDaemon::fixture())
    }

    /// The boss kill over a mock the caller set up.
    pub(crate) fn kill_on(mock: MockDaemon) -> (ClientState, MockDaemon) {
        let (mut state, mut mock) = indexed_on(mock);
        apply(&mut state, &mut mock, Action::Open);
        apply(&mut state, &mut mock, Action::OlderSegment);
        apply(&mut state, &mut mock, Action::OlderSegment);
        assert_eq!(state.segment_name().as_deref(), Some("The Ashen Warden"));
        (state, mock)
    }

    /// The kill's top row drilled open (by spell / by target + timeline).
    pub(crate) fn drilled() -> (ClientState, MockDaemon) {
        let (mut state, mut mock) = kill();
        apply(&mut state, &mut mock, Action::Open);
        assert!(state.drill.is_some());
        (state, mock)
    }

    /// R26: the `tree.txt` fixture's boss kill drilled into its Warlock,
    /// whose abilities group — Wither with its proc, two pets under their
    /// summons, two trinkets standing alone.
    pub(crate) fn tree_drilled() -> (ClientState, MockDaemon) {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../core/fixtures/tree.txt");
        let mut mock = MockDaemon::fixture_at(std::path::Path::new(path));
        let mut state = ClientState::new();
        let first = state.initial_request();
        pump(&mut state, &mut mock, vec![first]);
        apply(&mut state, &mut mock, Action::Open);
        for _ in 0..4 {
            if state.segment_name().as_deref() == Some("Tree Test Boss") {
                break;
            }
            apply(&mut state, &mut mock, Action::OlderSegment);
        }
        assert_eq!(state.segment_name().as_deref(), Some("Tree Test Boss"));
        state.row_sel = state
            .rows()
            .iter()
            .position(|r| r.label.starts_with("Vexxa"))
            .expect("the Warlock has a row");
        apply(&mut state, &mut mock, Action::Open);
        assert!(!state.drill_tree().groups.is_empty(), "the tree arrived");
        (state, mock)
    }

    /// R17: the `taken.txt` fixture's boss kill in the Taken view, over the
    /// same synchronous daemon — its tank tops the rows with every
    /// mitigation kind the record can carry.
    pub(crate) fn taken_kill() -> (ClientState, MockDaemon) {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../core/fixtures/taken.txt");
        let mut mock = MockDaemon::fixture_at(std::path::Path::new(path));
        let mut state = ClientState::new();
        let first = state.initial_request();
        pump(&mut state, &mut mock, vec![first]);
        // Newest is the trash tail; the encounter sits one older.
        apply(&mut state, &mut mock, Action::Open);
        apply(&mut state, &mut mock, Action::OlderSegment);
        assert_eq!(state.segment_name().as_deref(), Some("Taken Test Boss"));
        apply(
            &mut state,
            &mut mock,
            Action::SetView(wowdps_model::View::Taken),
        );
        assert_eq!(
            state.rows().first().map(|r| r.label.as_str()),
            Some("Durgan-Nebula-US"),
            "the tank took the most"
        );
        (state, mock)
    }

    /// One level deeper: the drilled player's top ability.
    pub(crate) fn spell_drilled() -> (ClientState, MockDaemon) {
        let (mut state, mut mock) = drilled();
        apply(&mut state, &mut mock, Action::Open);
        assert!(state.drill_spell().is_some());
        (state, mock)
    }

    // A Heroic raid of any size, with or without its raid timeline: the
    // synthetic kill both GUIs measure their chrome over (gui-logic's).
    pub(crate) use wowdps_gui_logic::raid::{
        raid, raid_deaths_bare, raid_timeline, raided, raided_deaths, raided_unmarked,
    };

    /// The kill's top two players compared.
    pub(crate) fn compared() -> (ClientState, MockDaemon) {
        let (mut state, mut mock) = kill();
        apply(&mut state, &mut mock, Action::PickCompare);
        apply(&mut state, &mut mock, Action::Down);
        apply(&mut state, &mut mock, Action::PickCompare);
        assert_eq!(state.screen, wowdps_model::Screen::Compare);
        assert!(state.compare_sides().is_some(), "the mock answers inline");
        (state, mock)
    }

    // ---- the socket bridge -----------------------------------------------

    /// A window whose socket the mock daemon answers: requests the window
    /// writes are read off the peer, handled, and the replies written back,
    /// so `update`/`drain_client` run over the real client plumbing.
    pub(crate) struct Bridge {
        pub gui: Gui,
        pub mock: MockDaemon,
        peer: UnixStream,
    }

    impl Bridge {
        pub(crate) fn new(mock: MockDaemon) -> Self {
            Self::with_config(mock, test_config())
        }

        /// A bridge whose window starts with a specific config — what the
        /// Home tests use to turn the startup offer back on.
        pub(crate) fn with_config(mock: MockDaemon, cfg: Config) -> Self {
            let (client, peer) = fake_client();
            peer.set_read_timeout(Some(std::time::Duration::from_millis(10)))
                .unwrap();
            let gui = Gui::for_test(client, ClientState::new(), cfg);
            let mut b = Self { gui, mock, peer };
            b.settle();
            b
        }

        /// Every request the window has written so far.
        pub(crate) fn requests(&mut self) -> Vec<ClientMsg> {
            let mut out = Vec::new();
            while let Ok((tag, body)) = wowdps_proto::wire::read_frame(&mut self.peer) {
                out.push(ClientMsg::decode(tag, &body).unwrap());
            }
            out
        }

        /// Push one daemon message at the window (its reader thread picks
        /// it up; the next `settle` drains it).
        pub(crate) fn push(&mut self, msg: &DaemonMsg) {
            self.peer.write_all(&msg.encode()).unwrap();
        }

        /// Serve requests and tick until the exchange has been quiet for a
        /// while — the client's reader thread delivers asynchronously.
        pub(crate) fn settle(&mut self) {
            let mut quiet = 0;
            while quiet < 8 {
                let reqs = self.requests();
                if reqs.is_empty() {
                    quiet += 1;
                } else {
                    quiet = 0;
                    let mut replies = Vec::new();
                    for req in reqs {
                        replies.extend(self.mock.handle(req));
                    }
                    for reply in replies {
                        self.push(&reply);
                    }
                }
                std::thread::sleep(std::time::Duration::from_millis(5));
                let _ = update(&mut self.gui, Message::Tick);
            }
        }

        pub(crate) fn send(&mut self, msg: Message) {
            let _ = update(&mut self.gui, msg);
            self.settle();
        }

        /// Open the log's segment at `pos` (the list's order, oldest first)
        /// the way a reader does: a press on its row on the rail.
        pub(crate) fn open(&mut self, pos: usize) {
            let id = self.gui.state.entries()[pos].id;
            self.send(Message::Pull(crate::rail::Pull::Log(id)));
        }

        /// Tell the window its log is another than the one the mock's store
        /// filed its cards under: none of them is the log's own then, so
        /// each stays a stored pull — under the log's on the rail, and on
        /// the stage when opened.
        pub(crate) fn foreign_store(&mut self) {
            let entries = self.gui.state.entries().to_vec();
            let source = self.gui.state.source.clone();
            self.push(&DaemonMsg::SegmentList {
                seq: 99,
                entries,
                source,
                active: false,
                log_id: Some(0xdead),
            });
            self.settle();
        }
    }

    // ---- headless rendering ----------------------------------------------

    /// iced_test tries wgpu first; a bare runner has no adapter and the dev
    /// box would spin a GPU device per test. Pin the software renderer.
    fn force_tiny_skia() {
        static ONCE: Once = Once::new();
        ONCE.call_once(|| {
            // SAFETY: set once, before the first simulator is built.
            unsafe { std::env::set_var("ICED_TEST_BACKEND", "tiny-skia") };
        });
    }

    pub(crate) fn simulator<'a, M: 'a>(el: Element<'a, M>) -> iced_test::Simulator<'a, M> {
        force_tiny_skia();
        iced_test::Simulator::with_size(
            iced::Settings::default(),
            iced::Size::new(640.0, 480.0),
            el,
        )
    }

    /// [`simulator`] at the prototype's wide frame (1440 × 900): the width
    /// at which the window lays out every column — under `theme::NARROW_WINDOW`
    /// the meter keeps two and a drill pane what fits.
    pub(crate) fn wide<'a, M: 'a>(el: Element<'a, M>) -> iced_test::Simulator<'a, M> {
        force_tiny_skia();
        iced_test::Simulator::with_size(
            iced::Settings::default(),
            iced::Size::new(1440.0, 900.0),
            el,
        )
    }

    /// What the running app's `Font::DEFAULT` becomes. iced_test swaps
    /// `DEFAULT` for its bundled Fira Sans, a font no running window has:
    /// the real renderer asks cosmic-text for the generic sans-serif family,
    /// which cosmic-text 0.15 spells "Open Sans" and, where that is not
    /// installed, falls back from (Noto Sans, then DejaVu Sans). Naming it
    /// outright walks the same lookup, so a picture's text is the app's.
    const RUNTIME_SANS: iced::Font = iced::Font::with_name("Open Sans");

    /// A simulator that renders as the running app does: the app's own
    /// `settings` (fonts loaded, default font and size), at `size` logical
    /// pixels. What the design shots and the overlay's guard draw through;
    /// `simulator` stays as it is so no existing test changes under it.
    pub(crate) fn simulator_as<'a, M: 'a>(
        mut settings: iced::Settings,
        size: iced::Size,
        el: Element<'a, M>,
    ) -> iced_test::Simulator<'a, M> {
        force_tiny_skia();
        if settings.default_font == iced::Font::DEFAULT {
            settings.default_font = RUNTIME_SANS;
        }
        iced_test::Simulator::with_size(settings, size, el)
    }

    /// Lay out and draw an element through the software renderer, so every
    /// style closure and canvas program in it actually runs.
    pub(crate) fn render<'a, M: 'a>(el: Element<'a, M>) -> iced_test::simulator::Snapshot {
        let mut ui = simulator(el);
        ui.snapshot(&iced::Theme::TokyoNight).unwrap()
    }

    /// What a test can ask of a drawn picture: its physical RGBA pixels at
    /// the snapshots' scale of 2, where `iced_test`'s `Snapshot` keeps
    /// them private — so a test can assert what was DRAWN (a gold
    /// underline, no yellow), not only which constant was chosen.
    pub(crate) struct Pixels {
        rgba: Vec<u8>,
        pub w: u32,
        pub h: u32,
    }

    impl Pixels {
        /// How many pixels in `[x0, x1) × [y0, y1)` (physical) are `c`,
        /// each channel within `tol`.
        pub(crate) fn count_in(
            &self,
            (x0, y0, x1, y1): (u32, u32, u32, u32),
            c: iced::Color,
            tol: u8,
        ) -> usize {
            let want = c.into_rgba8();
            let mut n = 0;
            for y in y0..y1.min(self.h) {
                for x in x0..x1.min(self.w) {
                    let i = ((y * self.w + x) * 4) as usize;
                    let px = &self.rgba[i..i + 3];
                    if px.iter().zip(want).all(|(a, b)| a.abs_diff(b) <= tol) {
                        n += 1;
                    }
                }
            }
            n
        }

        /// [`Self::count_in`] over the whole picture.
        pub(crate) fn count(&self, c: iced::Color, tol: u8) -> usize {
            self.count_in((0, 0, self.w, self.h), c, tol)
        }
    }

    /// Lay out and draw `el` at `size` the way `Simulator::snapshot` does,
    /// keeping the pixels.
    pub(crate) fn pixels<'a, M: 'a>(
        el: Element<'a, M>,
        size: iced::Size,
        theme: &iced::Theme,
    ) -> Pixels {
        pixels_at(el, size, theme, None)
    }

    /// [`pixels`] with the pointer moved to `at` first, so what answers
    /// the pointer — a hover wash, a tooltip — is drawn as it would be.
    pub(crate) fn pixels_at<'a, M: 'a>(
        el: Element<'a, M>,
        size: iced::Size,
        theme: &iced::Theme,
        at: Option<iced::Point>,
    ) -> Pixels {
        use iced::theme::Base;
        use iced_test::core::renderer::{Headless, Style};
        use iced_test::runtime::{UserInterface, user_interface};
        let mut renderer = renderer();
        let mut ui =
            UserInterface::build(el, size, user_interface::Cache::default(), &mut renderer);
        let cursor = at.map_or(iced::mouse::Cursor::Unavailable, |p| {
            iced::mouse::Cursor::Available(p)
        });
        let mut messages = Vec::new();
        let mut events = Vec::new();
        if let Some(position) = at {
            events.push(iced::Event::Mouse(iced::mouse::Event::CursorMoved {
                position,
            }));
        }
        events.push(iced::Event::Window(iced::window::Event::RedrawRequested(
            std::time::Instant::now(),
        )));
        let _ = ui.update(
            &events,
            cursor,
            &mut renderer,
            &mut iced_test::core::clipboard::Null,
            &mut messages,
        );
        let base = theme.base();
        ui.draw(
            &mut renderer,
            theme,
            &Style {
                text_color: base.text_color,
            },
            cursor,
        );
        let (w, h) = (
            (size.width * 2.0).round() as u32,
            (size.height * 2.0).round() as u32,
        );
        let rgba = renderer.screenshot(iced::Size::new(w, h), 2.0, base.background_color);
        Pixels { rgba, w, h }
    }

    /// A software renderer for calling canvas `Program::draw` directly.
    pub(crate) fn renderer() -> iced::Renderer {
        use iced_test::core::renderer::Headless;
        force_tiny_skia();
        iced_test::futures::futures::executor::block_on(iced::Renderer::new(
            iced::Font::DEFAULT,
            iced::Pixels(14.0),
            Some("tiny-skia"),
        ))
        .unwrap()
    }

    /// A key press as the window sees it.
    pub(crate) fn key(k: iced::keyboard::Key, modifiers: iced::keyboard::Modifiers) -> Message {
        Message::Key(iced::keyboard::Event::KeyPressed {
            key: k.clone(),
            modified_key: k,
            physical_key: iced::keyboard::key::Physical::Unidentified(
                iced::keyboard::key::NativeCode::Unidentified,
            ),
            location: iced::keyboard::Location::Standard,
            modifiers,
            repeat: false,
            text: None,
        })
    }

    pub(crate) fn chr(c: &str) -> Message {
        key(
            iced::keyboard::Key::Character(c.into()),
            iced::keyboard::Modifiers::default(),
        )
    }

    pub(crate) fn named(n: iced::keyboard::key::Named) -> Message {
        key(
            iced::keyboard::Key::Named(n),
            iced::keyboard::Modifiers::default(),
        )
    }
}

/// Design shots: the window's screens rendered to PNG for review against
/// the redesign's prototype (`crates/gui/SHOTS.md`).
#[cfg(test)]
mod shots;

#[cfg(test)]
mod tests {
    use super::testkit::{Bridge, chr, fake_client, gui_over, isolate_config, key, named};
    use super::*;
    use iced::keyboard::key::Named;
    use iced::keyboard::{Key, Modifiers};
    use wowdps_daemon::mock::MockDaemon;
    use wowdps_model::{Pane, Screen, View};
    use wowdps_proto::{ClientMsg, Cursor};

    #[test]
    fn staleness_starts_at_five_seconds() {
        assert_eq!(stale_secs(None), None);
        assert_eq!(stale_secs(Some(Instant::now())), None);
        let old = Instant::now() - Duration::from_secs(7);
        assert_eq!(stale_secs(Some(old)), Some(7));
        let (mut gui, _peer) = gui_over(ClientState::new());
        assert_eq!(gui.stale_secs(), None);
        gui.set_last_snapshot_at(Some(old));
        assert_eq!(gui.stale_secs(), Some(7));
    }

    #[test]
    fn title_names_the_source_and_style_uses_the_window_alpha() {
        let (mut gui, _peer) = gui_over(ClientState::new());
        assert_eq!(title(&gui), "wowdps");
        gui.state.source = Some("WoWCombatLog-1.txt".to_string());
        assert_eq!(title(&gui), "wowdps — WoWCombatLog-1.txt");
        gui.cfg.window_alpha = 0.5;
        let t = theme(&gui);
        let s = style(&gui, &t);
        assert_eq!(s.background_color.a, 0.5);
        assert_eq!(s.text_color, t.palette().text);
        gui.cfg.window_alpha = 7.0;
        assert_eq!(style(&gui, &t).background_color.a, 1.0);
        // Just building the subscription: it batches the tick and the keys.
        let _ = subscription(&gui);
    }

    #[test]
    fn a_new_window_declares_the_list_cursor() {
        let (client, mut peer) = fake_client();
        let _gui = Gui::for_test(client, ClientState::new(), Config::default());
        let (tag, body) = wowdps_proto::wire::read_frame(&mut peer).unwrap();
        assert_eq!(
            ClientMsg::decode(tag, &body).unwrap(),
            ClientMsg::Watch(Cursor::List)
        );
    }

    /// The daemon's answers reach the state, and — the window drawing no
    /// fight list — the log's newest pull takes the stage as they land.
    #[test]
    fn ticks_drain_the_daemon_into_the_state() {
        let b = Bridge::new(MockDaemon::fixture());
        assert_eq!(b.gui.state.screen, Screen::Meter);
        assert!(b.gui.state.following_live(), "the newest pull");
        assert!(
            b.gui.state.segment_count() >= 3,
            "the fixture's list arrived"
        );
        assert!(b.gui.state.source.is_some());
        assert_eq!(b.gui.stale_secs(), None, "data just arrived");
    }

    #[test]
    fn arriving_mid_fight_lands_on_the_live_meter() {
        // The list's `active` verdict makes the state answer with a new
        // Watch, which the drain forwards to the daemon.
        let b = Bridge::new(MockDaemon::fixture_live());
        assert_eq!(b.gui.state.screen, Screen::Meter);
        assert!(b.gui.state.is_live());
        assert!(!b.gui.state.rows().is_empty());
    }

    #[test]
    fn pointer_rows_open_meter_drill_and_ability() {
        let mut b = Bridge::new(MockDaemon::fixture());
        b.open(0);
        assert_eq!(b.gui.state.screen, Screen::Meter);
        assert_eq!(b.gui.state.segment_index(), 0);
        assert!(!b.gui.state.rows().is_empty());

        b.send(Message::MeterRow(1));
        assert_eq!(b.gui.state.row_sel, 1);
        let drill = b.gui.state.drill.clone().unwrap();
        assert_eq!(drill.label, b.gui.state.rows()[1].label);

        b.send(Message::SpellRow(0));
        let drill = b.gui.state.drill.clone().unwrap();
        assert_eq!(drill.pane, Pane::Spell);
        assert_eq!(drill.spell_sel, 0);
        assert!(drill.spell.is_some(), "Open descended into the ability");

        b.send(Message::DrillRange(Some((1_000, 5_000))));
        assert_eq!(b.gui.state.drill_range(), Some((1_000, 5_000)));
        b.send(Message::DrillRange(None));
        assert_eq!(b.gui.state.drill_range(), None);
    }

    #[test]
    fn class_icons_pick_the_comparison_and_right_click_clears_it() {
        let mut b = Bridge::new(MockDaemon::fixture());
        b.open(0);
        b.send(Message::CompareRow(0));
        assert_eq!(b.gui.state.compare_picks().len(), 1);
        assert_eq!(b.gui.state.screen, Screen::Meter);
        b.send(Message::CompareRow(1));
        assert_eq!(b.gui.state.screen, Screen::Compare);
        let (a, bb) = b.gui.state.compare_sides().expect("both sides answered");
        let spell = a.spells.first().cloned().expect("the side has spells");
        assert_ne!(a.guid, bb.guid);

        b.send(Message::CompareRange(Some((0, 10_000))));
        assert_eq!(b.gui.state.compare_shown_range(), Some((0, 10_000)));
        b.send(Message::CompareRange(None));
        assert_eq!(b.gui.state.compare_shown_range(), None);

        b.send(Message::CompareSpell((
            spell.key.clone(),
            spell.label.clone(),
        )));
        assert_eq!(
            b.gui.state.compare_spell().map(|(k, _)| k.as_str()),
            Some(spell.key.as_str())
        );

        // Right-click backs out one level: the ability first, then the pair.
        b.send(Message::ClearCompare);
        assert_eq!(b.gui.state.screen, Screen::Compare);
        assert!(b.gui.state.compare_spell().is_none());
        b.send(Message::ClearCompare);
        assert_eq!(b.gui.state.screen, Screen::Meter);
        assert!(b.gui.state.compare_picks().is_empty());
    }

    #[test]
    fn keys_reach_the_shared_keymap() {
        let mut b = Bridge::new(MockDaemon::fixture());
        assert_eq!(b.gui.state.screen, Screen::Meter);
        b.send(chr("j"));
        assert_eq!(b.gui.state.row_sel, 1);
        b.send(chr("h"));
        assert_eq!(b.gui.state.view, View::Healing);
        // Esc from the meter with nothing to back out of: the front door.
        b.send(named(Named::Escape));
        assert!(b.gui.home.is_some());
        assert_eq!(b.gui.state.screen, Screen::Meter, "Home is over it");
        // Unknown keys are ignored.
        b.send(chr("z"));
        assert!(b.gui.home.is_some());
        b.send(chr("q"));
        assert!(b.gui.state.quit);
    }

    #[test]
    fn zoom_chords_step_and_clamp() {
        isolate_config();
        let (mut gui, _peer) = gui_over(ClientState::new());
        let base = gui.cfg.zoom;
        let _ = update(&mut gui, key(Key::Character("=".into()), Modifiers::CTRL));
        assert!((gui.cfg.zoom - (base + ZOOM_STEP)).abs() < 1e-6);
        let _ = update(&mut gui, key(Key::Character("0".into()), Modifiers::CTRL));
        assert_eq!(gui.cfg.zoom, Config::default().zoom);
        for _ in 0..40 {
            let _ = update(&mut gui, key(Key::Character("-".into()), Modifiers::CTRL));
        }
        assert_eq!(gui.cfg.zoom, *ZOOM_RANGE.start());
        for _ in 0..40 {
            let _ = update(&mut gui, key(Key::Character("+".into()), Modifiers::CTRL));
        }
        assert_eq!(gui.cfg.zoom, *ZOOM_RANGE.end());
    }

    #[test]
    fn options_panel_toggles_and_saves_ranks() {
        isolate_config();
        let (mut gui, _peer) = gui_over(ClientState::new());
        assert!(!gui.options_open);
        let _ = update(&mut gui, Message::ToggleOptions);
        assert!(gui.options_open);
        let _ = update(&mut gui, Message::Noop);
        assert!(gui.options_open);
        let _ = update(&mut gui, Message::CloseOptions);
        assert!(!gui.options_open);
        let _ = update(&mut gui, Message::SetShowRanks(false));
        assert!(!gui.cfg.show_ranks);
        let _ = update(&mut gui, Message::SetShowRanks(true));
        assert!(gui.cfg.show_ranks);
    }

    #[test]
    fn t_opens_the_talent_viewer_on_the_selected_player_and_asks_for_the_loadout() {
        let mut b = Bridge::new(MockDaemon::fixture());
        let top = b.gui.state.rows()[0].clone();
        b.send(chr("t"));
        let ui = b.gui.talents.as_ref().expect("viewer open");
        assert_eq!(ui.player.as_deref(), Some(top.label.as_str()));
        // The mock answered the GetLoadout inline; the reply was consumed.
        assert_eq!(b.gui.pending_loadout(), None);

        // The meter keymap is swallowed while the viewer is up.
        b.send(chr("q"));
        assert!(!b.gui.state.quit);
        b.send(named(Named::Tab));
        assert!(b.gui.talents.is_some());
        b.send(named(Named::Escape));
        assert!(b.gui.talents.is_none());
        assert_eq!(
            b.gui.state.screen,
            Screen::Meter,
            "Esc closed the viewer, not the meter"
        );
    }

    #[test]
    fn t_without_a_row_opens_an_empty_viewer() {
        let (mut gui, _peer) = gui_over(ClientState::new());
        let _ = update(&mut gui, chr("t"));
        assert!(gui.talents.is_some());
        assert_eq!(gui.pending_loadout(), None, "nothing to ask about");
        let _ = update(&mut gui, Message::Talents(talents::Msg::Close));
        assert!(gui.talents.is_none());
        // Ctrl-t is not the viewer.
        let _ = update(&mut gui, key(Key::Character("t".into()), Modifiers::CTRL));
        assert!(gui.talents.is_none());
    }

    #[test]
    fn talent_messages_route_to_the_open_viewer() {
        let (mut gui, _peer) = gui_over(ClientState::new());
        // Nobody to route to: all no-ops.
        let _ = update(&mut gui, Message::Talents(talents::Msg::PasteClipboard));
        let _ = update(&mut gui, Message::Talents(talents::Msg::CopyString));
        let _ = update(&mut gui, Message::Talents(talents::Msg::ToggleTab));
        assert!(gui.talents.is_none());

        let _ = update(&mut gui, chr("t"));
        let _ = update(
            &mut gui,
            Message::Talents(talents::Msg::Input("abc".to_string())),
        );
        assert_eq!(gui.talents.as_ref().unwrap().input, "abc");
        let _ = update(&mut gui, Message::Talents(talents::Msg::PasteClipboard));
        let _ = update(&mut gui, Message::Talents(talents::Msg::CopyString));
        let _ = update(&mut gui, Message::Talents(talents::Msg::ToggleTab));
        assert!(gui.talents.is_some());
    }

    #[test]
    fn a_stale_loadout_reply_is_dropped() {
        let mut b = Bridge::new(MockDaemon::fixture());
        let guid = b.gui.state.rows()[0].key.clone();
        b.push(&DaemonMsg::Loadout {
            req_id: 999,
            guid,
            loadout: Some(wowdps_model::Loadout::default()),
        });
        b.settle();
        assert!(b.gui.talents.is_none(), "no viewer, nothing adopted");
        assert_eq!(b.gui.pending_loadout(), None);
    }

    /// The owner is the most certain match over every row, not the first
    /// row to match anything: the lock's guid, then a whole "Name-Realm",
    /// then a bare name — and a bare name only when it names one row, so a
    /// namesake from another realm who out-ranks the owner never wears
    /// their tag.
    #[test]
    fn the_owner_is_the_most_certain_match_not_the_first() {
        let row = |key: &str, label: &str| wowdps_model::Row {
            key: key.to_string(),
            label: label.to_string(),
            ..wowdps_model::Row::default()
        };
        // The namesake out-ranks the owner, as the daemon orders them.
        let rows = vec![
            row("Player-2-99", "Tranqlock-Stormrage-US"),
            row("Player-1-16", "Tranqlock-Proudmoore-US"),
            row("Player-1-17", "Bea-Proudmoore-US"),
        ];
        let names = |ns: &[&str]| ns.iter().map(|n| n.to_string()).collect::<Vec<_>>();
        assert_eq!(
            owner_among(&rows, Some("Player-1-16"), &names(&["Tranqlock"])),
            Some(1),
            "the guid lock wins over any name"
        );
        assert_eq!(
            owner_among(
                &rows,
                None,
                &names(&["Tranqlock", "Tranqlock-Proudmoore-US"])
            ),
            Some(1),
            "a whole name wins over a bare one"
        );
        assert_eq!(
            owner_among(&rows, None, &names(&["Tranqlock"])),
            None,
            "a bare name that names two rows names neither"
        );
        assert_eq!(
            owner_among(&rows, None, &names(&["bea"])),
            Some(2),
            "a bare name that names one row, case aside"
        );
        assert_eq!(
            owner_among(&rows, Some("Player-0-0"), &names(&[])),
            None,
            "a lock not in the fight"
        );
        let mut enemy = rows.clone();
        enemy[2].enemy = true;
        assert_eq!(owner_among(&enemy, None, &names(&["Bea"])), None, "R13");
    }

    /// A step past the fold brings the selection into sight, and the list
    /// moves the least that does: the extent the window computes for a
    /// row is where the layout draws it, a row past the list's foot scrolls
    /// just far enough to show it whole, and one in sight moves nothing.
    #[test]
    fn a_step_past_the_fold_keeps_the_selection_in_sight() {
        let (mut gui, _peer) = gui_over(testkit::raid(25));
        gui.cfg.hide_realms = true;
        let size = iced::Size::new(1440.0, 900.0);
        let mut ui = testkit::simulator_as(settings(), size, view::view(&gui));
        let list = ui.find(view::meter_list_id()).unwrap().bounds();
        for i in [0usize, 7, 18] {
            let (top, bottom) = view::meter_row_extent(&gui, i).expect("drawn");
            let name = ui.find(format!("Raider{i}").as_str()).unwrap().bounds();
            assert!(
                name.y >= list.y + top && name.y + name.height <= list.y + bottom,
                "row {i}: {name:?} in {top}..{bottom} under {list:?}"
            );
        }
        let span = |row: usize| {
            let (top, bottom) = view::meter_row_extent(&gui, row).unwrap();
            RevealSpan {
                id: view::meter_list_id(),
                top,
                bottom,
            }
        };
        assert_eq!(span(3).offset(0.0, list.height), None, "in sight: stay");
        let (_, bottom) = view::meter_row_extent(&gui, 22).unwrap();
        assert_eq!(
            span(22).offset(0.0, list.height),
            Some(bottom - list.height),
            "past the foot: just far enough"
        );
        assert_eq!(
            span(0).offset(200.0, list.height),
            Some(0.0),
            "above: to it"
        );
        // A key step on the meter asks for the scroll — the inspector beside
        // it following — and a meter under Home has no list to keep
        // anything in.
        drop(ui);
        gui.state.select_row(20);
        assert!(update(&mut gui, chr("j")).units() > 0);
        assert_eq!(gui.state.row_sel, 21);
        assert!(view::meter_row_extent(&gui, 21).is_some());
        gui.home = Some(home::Home::new());
        assert_eq!(view::meter_row_extent(&gui, 21), None);
    }

    #[test]
    fn a_vanished_daemon_is_reported_in_the_footer() {
        let (client, peer) = fake_client();
        let mut gui = Gui::for_test(client, ClientState::new(), Config::default());
        drop(peer);
        // The reader thread notices the hangup; give it a moment.
        for _ in 0..50 {
            let _ = update(&mut gui, Message::Tick);
            if gui.state.status.is_some() {
                break;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        assert_eq!(
            gui.state.status.as_deref(),
            Some("daemon gone — no daemon binary to respawn"),
            "no daemon binary to respawn, so the notice says so and sticks"
        );
    }

    /// A window over the fixture at the wide frame, on the log's pull with
    /// the most deaths — its raid timeline in hand.
    fn on_the_deadliest() -> Bridge {
        let mut b = Bridge::new(MockDaemon::fixture());
        b.send(Message::WindowWidth(1440.0));
        let mut best = (0, 0);
        for pos in 0..b.gui.state.entries().len() {
            b.open(pos);
            let n = b.gui.fight().raid().map_or(0, |r| r.deaths.len());
            if n > best.1 {
                best = (pos, n);
            }
        }
        assert!(best.1 >= 2, "a pull of the fixture holds two deaths");
        b.open(best.0);
        b
    }

    /// R25 (v35): a skull — or a row of the Deaths table — opens that
    /// death: the Deaths view, drilled into the player at that window, its
    /// skull lit on the ribbon; and j/k then walk the deaths in the order
    /// they happened, each step the next one's recap.
    #[test]
    fn a_death_opens_its_recap_and_j_k_walk_the_deaths_in_order() {
        let mut b = on_the_deadliest();
        let raid = b.gui.fight().raid().cloned().expect("a raid timeline");
        let pick = |i: usize| {
            let d = &raid.deaths[i];
            crate::deaths::Pick {
                key: d.guid.clone(),
                label: d.name.clone(),
                index: d.index,
            }
        };
        let on = |b: &Bridge| {
            let app = b.gui.fight();
            (
                app.view,
                app.drill.as_ref().map(|d| d.key.clone()),
                app.deaths().1,
            )
        };
        b.send(Message::OpenDeath(pick(1)));
        let second = &raid.deaths[1];
        assert_eq!(
            on(&b),
            (View::Deaths, Some(second.guid.clone()), Some(second.index))
        );
        assert!(!b.gui.fight().inspecting(), "wide: the inspector is beside");
        let lit = crate::ribbon::Ribbon::of(&b.gui).map(|r| {
            r.skulls
                .iter()
                .enumerate()
                .filter(|(_, s)| s.on)
                .map(|(i, _)| i)
                .collect::<Vec<_>>()
        });
        assert_eq!(lit, Some(vec![1]));
        b.send(chr("k"));
        let first = &raid.deaths[0];
        assert_eq!(
            on(&b),
            (View::Deaths, Some(first.guid.clone()), Some(first.index))
        );
        b.send(chr("j"));
        b.send(chr("j"));
        let at = raid.deaths.len().min(3) - 1;
        assert_eq!(on(&b).1, Some(raid.deaths[at].guid.clone()));
        // The recap names each event's time before the death.
        let (events, _) = b.gui.fight().breakdown();
        assert!(!events.is_empty() && events.iter().all(|e| e.offset_ms.is_some_and(|o| o <= 0)));
    }

    /// Keys handed to the inspector on another view come back to the
    /// Deaths table beside it: Enter on the Damage meter, then a skull —
    /// j opens the next death; and `K` onto the view does the same. The
    /// `?` sheet dims Enter there, where it hands the recap nothing.
    #[test]
    fn the_deaths_table_takes_back_keys_the_inspector_held() {
        let mut b = on_the_deadliest();
        let raid = b.gui.fight().raid().cloned().expect("a raid timeline");
        assert!(raid.deaths.len() >= 2);
        let pick = |i: usize| {
            let d = &raid.deaths[i];
            crate::deaths::Pick {
                key: d.guid.clone(),
                label: d.name.clone(),
                index: d.index,
            }
        };
        b.send(Message::PickView(View::Damage));
        b.send(named(iced::keyboard::key::Named::Enter));
        assert!(b.gui.fight().inspecting(), "the keys in the inspector");
        b.send(Message::OpenDeath(pick(0)));
        assert!(!b.gui.fight().inspecting(), "back with the table");
        b.send(chr("j"));
        let on = b.gui.fight().drill.as_ref().map(|d| d.key.clone());
        assert_eq!(on, Some(raid.deaths[1].guid.clone()), "j opened the next");
        assert!(b.gui.inert_keys().contains(&"enter"), "Enter is dimmed");
        // The view key: Enter on Damage, then K.
        b.send(Message::PickView(View::Damage));
        b.send(named(iced::keyboard::key::Named::Enter));
        assert!(b.gui.fight().inspecting());
        b.send(Message::PickView(View::Deaths));
        assert!(!b.gui.fight().inspecting(), "the table holds them");
        // Narrow, the pushed recap keeps them, and Enter works there.
        b.send(Message::WindowWidth(460.0));
        b.send(Message::OpenDeath(pick(0)));
        assert!(b.gui.fight().inspecting());
        assert!(!b.gui.inert_keys().contains(&"enter"));
    }

    /// A filter that hides every death swallows j/k: they never step the
    /// hidden count rows under the table, whose drill would move the recap
    /// to a player the table says matches nothing.
    #[test]
    fn j_k_on_a_filter_that_hides_every_death_do_nothing() {
        let mut b = on_the_deadliest();
        b.send(Message::PickView(View::Deaths));
        let before = b.gui.fight().drill.as_ref().map(|d| d.key.clone());
        let sel = b.gui.fight().row_sel;
        b.send(Message::Filter("zzz".to_string()));
        b.send(chr("j"));
        b.send(chr("j"));
        assert_eq!(b.gui.fight().drill.as_ref().map(|d| d.key.clone()), before);
        assert_eq!(b.gui.fight().row_sel, sel);
    }

    /// Narrow, a death's press pushes its recap over the meter, as a
    /// press on a meter row pushes a drill.
    #[test]
    fn a_narrow_window_pushes_the_recap_it_opens() {
        let mut b = on_the_deadliest();
        b.send(Message::WindowWidth(460.0));
        let d = b
            .gui
            .fight()
            .raid()
            .map(|r| r.deaths[0].clone())
            .expect("a death");
        b.send(Message::OpenDeath(crate::deaths::Pick {
            key: d.guid.clone(),
            label: d.name.clone(),
            index: d.index,
        }));
        assert!(b.gui.fight().inspecting());
        assert_eq!(b.gui.fight().view, View::Deaths);
    }

    /// Beside the Deaths table Enter hands the recap nothing — it has no
    /// row to key, and the table's selection must not go quiet on a keyless
    /// panel — while a narrow window's Enter pushes the recap over it.
    #[test]
    fn enter_on_the_deaths_table_keeps_the_keys_there() {
        let mut b = on_the_deadliest();
        b.send(Message::PickView(View::Deaths));
        assert!(crate::deaths::Table::of(&b.gui).is_some());
        b.send(named(iced::keyboard::key::Named::Enter));
        assert!(!b.gui.fight().inspecting(), "the table keeps the keys");
        b.send(Message::WindowWidth(460.0));
        b.send(named(iced::keyboard::key::Named::Enter));
        assert!(b.gui.fight().inspecting(), "narrow: the recap is pushed");
    }

    /// An opened death waits for its recap, then brings the killing blow's
    /// row — which ends the list — into sight: the row wears the id the
    /// window's reveal finds, and the wait is over once it has landed.
    #[test]
    fn an_opened_death_reveals_its_killing_blow() {
        use iced_test::runtime::user_interface::{Cache, UserInterface};
        let mut b = on_the_deadliest();
        let d = b
            .gui
            .fight()
            .raid()
            .and_then(|r| r.deaths.iter().find(|d| !d.blow.is_empty()).cloned())
            .expect("a death to a blow");
        b.send(Message::OpenDeath(crate::deaths::Pick {
            key: d.guid.clone(),
            label: d.name.clone(),
            index: d.index,
        }));
        let _ = update(&mut b.gui, Message::Tick);
        assert_eq!(b.gui.reveal_death, None, "the recap landed: revealed");
        let mut renderer = testkit::renderer();
        let mut ui = UserInterface::build(
            view::view(&b.gui),
            iced::Size::new(1440.0, 420.0),
            Cache::default(),
            &mut renderer,
        );
        let mut op = FindRow {
            scroll: crate::inspector::scroll_id(),
            row: crate::inspector::recap_kill_id(),
            content: None,
            found: None,
        };
        ui.operate(&renderer, &mut op);
        assert!(op.reveal().is_some(), "the killing blow's row is found");
    }
}

#[cfg(test)]
mod home_tests {
    use super::testkit::{Bridge, chr, named, simulator, test_config};
    use super::*;
    use iced::keyboard::key::Named;
    use wowdps_daemon::mock::MockDaemon;
    use wowdps_model::{Pane, Screen, View};
    use wowdps_proto::{ClientMsg, HistoryAnswer, HistoryQuery};

    fn home_bridge() -> Bridge {
        Bridge::new(MockDaemon::fixture().with_history())
    }

    /// The fixture's store with its owner named — `history_characters`, as
    /// a user's config names them — so Thraxx's pulls are "yours".
    fn owned_bridge() -> Bridge {
        Bridge::new(
            MockDaemon::fixture()
                .with_characters(&["Thraxx-Nebula-US".to_string()])
                .with_history(),
        )
    }

    /// A window whose config asks for the class chrome: the owner's colour,
    /// where the default is the game's gold.
    fn class_chrome() -> Config {
        Config {
            chrome: "class".to_string(),
            ..test_config()
        }
    }

    #[test]
    fn tilde_opens_and_closes_home() {
        let mut b = home_bridge();
        let screen = b.gui.state.screen;
        b.send(chr("~"));
        assert!(b.gui.home.is_some());
        assert_eq!(b.gui.state.screen, screen, "the state machine is untouched");
        b.send(chr("~"));
        assert!(b.gui.home.is_none());
        // The layout-stable alternative for keyboards where ~ is a dead key.
        b.send(super::testkit::key(
            iced::keyboard::Key::Character("`".into()),
            iced::keyboard::Modifiers::SHIFT,
        ));
        assert!(b.gui.home.is_some());
    }

    #[test]
    fn home_asks_the_daemon_for_fights_and_fills_itself() {
        let mut b = home_bridge();
        let _ = b.requests();
        let _ = update(&mut b.gui, chr("~"));
        let asked = b.requests();
        assert!(
            asked.iter().any(|r| matches!(
                r,
                ClientMsg::GetHistory {
                    query: HistoryQuery::Fights { .. },
                    ..
                }
            )),
            "{asked:?}"
        );
        // `requests` consumed the frames, so play daemon for them by hand.
        for req in asked {
            for reply in b.mock.handle(req) {
                b.push(&reply);
            }
        }
        b.settle();
        assert!(!b.gui.home.as_ref().unwrap().cards.is_empty());
        // The mock's store names no owner: nothing of the week is "yours",
        // and Home says why rather than showing an empty week.
        assert!(b.gui.home_panels.unowned);
    }

    #[test]
    fn a_stale_history_reply_is_dropped() {
        let mut b = home_bridge();
        b.send(chr("~"));
        let before = b.gui.home.as_ref().unwrap().cards.len();
        b.push(&DaemonMsg::History {
            req_id: 4242,
            answer: HistoryAnswer::Fights {
                cards: vec![wowdps_proto::history::FightCard::default()],
                total: 1,
            },
        });
        b.settle();
        assert_eq!(b.gui.home.as_ref().unwrap().cards.len(), before);
    }

    /// Home reads the store until the week is in hand, one request in
    /// flight at a time, and then stops asking: a store that fits one page
    /// is whole after its first answer, and nothing more goes out.
    #[test]
    fn home_reads_the_week_and_stops() {
        let mut b = home_bridge();
        b.send(chr("~"));
        let ui = b.gui.home.as_ref().unwrap();
        assert!(ui.answered && ui.complete(), "the fixture fits one page");
        assert_eq!(ui.pages, 1);
        let _ = b.requests();
        for _ in 0..3 {
            let _ = update(&mut b.gui, Message::Tick);
        }
        assert!(
            !b.requests().iter().any(|r| matches!(
                r,
                ClientMsg::GetHistory {
                    query: HistoryQuery::Fights { .. },
                    ..
                }
            )),
            "a whole store is asked for once"
        );
    }

    #[test]
    fn home_opens_on_start_and_closes_when_a_pull_starts() {
        let mut b = Bridge::with_config(
            MockDaemon::fixture().with_history(),
            Config {
                home_on_start: true,
                ..test_config()
            },
        );
        assert!(
            b.gui.home.is_some(),
            "nothing live, so Home is the front door"
        );
        // Mid-pull at launch: the dashboard must not flash over it.
        let live = Bridge::with_config(
            MockDaemon::fixture_live(),
            Config {
                home_on_start: true,
                ..test_config()
            },
        );
        assert!(live.gui.state.is_live());
        assert!(live.gui.home.is_none(), "no dashboard over a live pull");

        // And a pull STARTING while Home is up puts the meter back.
        assert!(b.gui.home.is_some());
        for reply in b.mock.feed(vec![
            "7/27/2026 20:20:00.000-4  ENCOUNTER_START,3130,\"The Ashen Warden\",15,3,2769"
                .to_string(),
        ]) {
            b.push(&reply);
        }
        b.settle();
        assert!(b.gui.state.is_live());
        assert!(b.gui.home.is_none(), "a pull replaces the dashboard");
    }

    #[test]
    fn question_mark_toggles_the_sheet_and_any_key_dismisses_it() {
        let mut b = home_bridge();
        b.send(chr("?"));
        assert!(b.gui.shortcuts_open);
        b.send(chr("T"));
        assert!(!b.gui.shortcuts_open);
        assert_ne!(
            b.gui.state.view,
            View::Taken,
            "the dismissing key does nothing else"
        );
        b.send(Message::ToggleShortcuts);
        assert!(b.gui.shortcuts_open);
        b.send(Message::ToggleShortcuts);
        assert!(!b.gui.shortcuts_open);
    }

    #[test]
    fn the_meter_keymap_is_swallowed_while_the_filter_has_focus() {
        let mut b = home_bridge();
        b.send(chr("/"));
        assert!(b.gui.filter_focused);
        b.send(chr("q"));
        assert!(!b.gui.state.quit, "typing q into the filter must not quit");
        b.send(named(Named::Escape));
        assert!(!b.gui.filter_focused);
        b.send(chr("q"));
        assert!(b.gui.state.quit);
    }

    /// The keymap is swallowed only while a field is really there to type
    /// into: `/` on a screen that draws no filter box would otherwise focus
    /// nothing and leave the window looking keyboard-dead.
    #[test]
    fn slash_is_ignored_where_no_filter_box_is_drawn() {
        let mut b = home_bridge();
        b.send(chr("~"));
        assert!(b.gui.home.is_some());
        b.send(chr("/"));
        assert!(!b.gui.filter_focused, "Home draws no filter box");
        b.send(chr("~"));

        // The meter stands beside the inspector, filter and all: with the
        // keys in the inspector, `/` hands them back to the rows it narrows
        // (a narrow window's pushed inspector steps aside for it).
        b.send(named(Named::Enter));
        assert!(b.gui.state.inspecting());
        b.send(chr("/"));
        assert!(b.gui.filter_focused);
        assert!(!b.gui.state.inspecting(), "the keys are the meter's again");
    }

    /// iced owns focus: a click elsewhere unfocuses the field without ever
    /// telling us, so the tick's answer is what the swallow flag follows.
    #[test]
    fn the_fields_own_focus_wins_over_the_flag() {
        let mut b = home_bridge();
        b.send(chr("/"));
        assert!(b.gui.filter_focused);
        b.send(Message::FilterFocus(false));
        assert!(
            !b.gui.filter_focused,
            "the field lost focus, so does the flag"
        );
        b.send(chr("q"));
        assert!(b.gui.state.quit, "the keymap is the meter's again");
    }

    /// j/k walk what is DRAWN: stepping onto a hidden row would park the
    /// highlight on nothing and drill into a player nobody can see.
    #[test]
    fn the_selection_steps_over_the_rows_a_filter_hides() {
        let mut b = home_bridge();
        let rows = b.gui.state.rows();
        assert!(rows.len() > 2, "the fixture has a chart to filter");
        // Everything but the last row.
        let last = rows.len() - 1;
        b.send(Message::Filter(rows[last].label.clone()));
        b.gui.state.row_sel = 0;
        b.send(chr("j"));
        assert_eq!(
            b.gui.state.row_sel, last,
            "down from a hidden row lands on the only visible one"
        );
        b.send(chr("j"));
        assert_eq!(b.gui.state.row_sel, last, "and stays there");
        b.send(chr("k"));
        assert_eq!(b.gui.state.row_sel, last, "nothing visible above it");
        // With the filter gone the state machine's own step is back.
        b.send(Message::Filter(String::new()));
        b.gui.state.row_sel = 0;
        b.send(chr("j"));
        assert_eq!(b.gui.state.row_sel, 1);
    }

    /// A drill is read with the mouse, so its panes answer the pointer: the
    /// hover is per pane (they are drawn side by side) and lands on inert
    /// rows too — the mark says "this is the line you are reading".
    #[test]
    fn the_pointer_marks_one_row_of_one_pane() {
        let mut b = home_bridge();
        assert_eq!(b.gui.hover_meter(), None);
        b.send(Message::HoverRow(Some(RowHover::Meter(2))));
        assert_eq!(b.gui.hover_meter(), Some(2));
        assert_eq!(b.gui.hover_in(Pane::Spell), None, "not a drill row");

        b.send(Message::HoverRow(Some(RowHover::Drill(Pane::Target, 1))));
        assert_eq!(b.gui.hover_in(Pane::Target), Some(1));
        assert_eq!(
            b.gui.hover_in(Pane::Spell),
            None,
            "the other pane stays unlit"
        );
        assert_eq!(b.gui.hover_meter(), None);
        b.send(Message::HoverRow(None));
        assert_eq!(b.gui.hover_in(Pane::Target), None);

        // R12: the comparison's echo is a spell KEY, so both tables can
        // light the same ability wherever it sits in each.
        b.send(Message::CompareSpellHover(Some("Melee".to_string())));
        assert_eq!(b.gui.spell_hover.as_deref(), Some("Melee"));
        b.send(Message::CompareSpellHover(None));
        assert_eq!(b.gui.spell_hover, None);
    }

    #[test]
    fn enter_leaves_the_filter_text_alone() {
        let mut b = home_bridge();
        b.send(chr("/"));
        b.send(Message::Filter("durgan".to_string()));
        b.send(named(Named::Enter));
        assert!(!b.gui.filter_focused);
        assert_eq!(b.gui.filter, "durgan");
        // Esc only reaches the filter while the filter has focus; after
        // Enter it is the meter's Esc again.
        b.send(chr("/"));
        b.send(named(Named::Escape));
        assert!(b.gui.filter.is_empty(), "Esc gives up on the filter");
    }

    /// The field's own keys, as iced delivers them — not fed to `update`
    /// as the tests above feed them. A focused text input captures Escape
    /// (and lets go of its focus), so `keyboard::listen` never hears it:
    /// the window listens for it captured, and gives up the text and the
    /// keys. Enter is the field's submit: the text stays, and iced's focus
    /// goes with the flag, or the next tick would find the field focused
    /// and swallow the keymap again.
    #[test]
    fn the_filter_s_esc_and_enter_reach_the_window_through_iced() {
        let mut b = home_bridge();
        let size = iced::Size::new(1440.0, 900.0);
        let focus = |b: &mut Bridge| {
            b.send(chr("/"));
            b.send(Message::Filter("dur".to_string()));
            assert!(b.gui.filter_focused);
        };
        // Enter, typed into the focused field.
        focus(&mut b);
        let mut ui = super::testkit::simulator_as(settings(), size, view::view(&b.gui));
        ui.click(crate::nav::filter_id())
            .expect("the field is drawn");
        let _ = ui.tap_key(iced::keyboard::Key::Named(Named::Enter));
        let sent: Vec<Message> = ui.into_messages().collect();
        assert!(
            sent.iter().any(|m| matches!(m, Message::FilterDone)),
            "{sent:?}"
        );
        for m in sent {
            let _ = update(&mut b.gui, m);
        }
        assert!(!b.gui.filter_focused, "the keys come back");
        assert_eq!(b.gui.filter, "dur", "the text stays");
        assert!(
            update(&mut b.gui, Message::FilterDone).units() > 0,
            "iced's focus is dropped with the flag"
        );
        // Escape: the field takes it for itself…
        focus(&mut b);
        let mut ui = super::testkit::simulator_as(settings(), size, view::view(&b.gui));
        ui.click(crate::nav::filter_id())
            .expect("the field is drawn");
        let status = ui.tap_key(iced::keyboard::Key::Named(Named::Escape));
        assert_eq!(status, iced::event::Status::Captured, "the field's own");
        drop(ui);
        // …and the window hears it captured, only captured.
        let esc = iced_test::simulator::press_key(iced::keyboard::Key::Named(Named::Escape), None);
        let window = iced::window::Id::unique();
        assert!(matches!(
            captured_escape(esc.clone(), iced::event::Status::Captured, window),
            Some(Message::FilterEscape)
        ));
        assert!(captured_escape(esc, iced::event::Status::Ignored, window).is_none());
        let other =
            iced_test::simulator::press_key(iced::keyboard::Key::Character("q".into()), None);
        assert!(captured_escape(other, iced::event::Status::Captured, window).is_none());
        b.send(Message::FilterEscape);
        assert!(!b.gui.filter_focused);
        assert!(b.gui.filter.is_empty(), "Esc gives up the text");
        // Heard with no field focused, it touches nothing.
        b.send(Message::Filter("kept".to_string()));
        b.send(Message::FilterEscape);
        assert_eq!(b.gui.filter, "kept");
    }

    /// Each view's table has its own columns now, so a sort chosen on one
    /// is no order on a view without that column: Healing's overheal share
    /// sorts nothing on Damage (the daemon's order, no heading marked),
    /// and comes back when Healing does.
    #[test]
    fn a_sort_by_another_view_s_column_is_no_sort_here() {
        use crate::table::{Col, meter_set, sort_of};
        let mut b = home_bridge();
        b.send(Message::PickView(View::Healing));
        b.send(Message::SortBy(Col::Overheal));
        assert_eq!(b.gui.meter_sort(), Some((Col::Overheal, true)));
        b.send(Message::PickView(View::Damage));
        assert_eq!(b.gui.meter_sort(), None, "Damage has no overheal column");
        let drawn: Vec<usize> =
            view::ordered(b.gui.state.rows(), &b.gui.filter, b.gui.meter_sort())
                .into_iter()
                .map(|(i, _)| i)
                .collect();
        assert!(
            drawn.iter().enumerate().all(|(at, i)| at == *i),
            "the daemon's order: {drawn:?}"
        );
        for &c in meter_set(View::Damage, false) {
            assert_eq!(sort_of(c, b.gui.meter_sort()), None, "{c:?} is unmarked");
        }
        // A heading pressed here starts its own cycle from the top.
        b.send(Message::SortBy(Col::CritFine));
        assert_eq!(b.gui.meter_sort(), Some((Col::CritFine, true)));
        // A rate is no order for a count.
        b.send(Message::SortBy(Col::Rate));
        b.send(Message::PickView(View::Interrupts));
        assert_eq!(b.gui.meter_sort(), None);
        // What a view shares keeps its sort across them.
        b.send(Message::PickView(View::Healing));
        assert_eq!(b.gui.meter_sort(), Some((Col::Rate, true)));
    }

    /// Home stands over the meter, and the meter's keys stop at it: j, k,
    /// Enter, v and g reach nothing under it, `t` opens the talent viewer
    /// on nobody (no loadout asked for the hidden meter's row), and a view
    /// key leaves Home for the fight it names.
    #[test]
    fn keys_on_home_never_reach_the_hidden_meter() {
        let mut b = home_bridge();
        let (sel, mode) = (b.gui.state.row_sel, b.gui.state.graph_mode());
        b.send(chr("~"));
        for k in ["j", "j", "v", "g"] {
            b.send(chr(k));
        }
        b.send(named(Named::Enter));
        assert!(b.gui.home.is_some());
        assert_eq!(b.gui.state.row_sel, sel, "j never moved the meter");
        assert!(b.gui.state.compare_picks().is_empty(), "v pinned nobody");
        assert_eq!(b.gui.state.graph_mode(), mode, "g changed nothing");
        assert!(!b.gui.state.inspecting(), "Enter inspected nothing");
        b.send(chr("t"));
        assert!(b.gui.talents.is_some());
        assert_eq!(b.gui.pending_loadout(), None, "nobody's loadout");
        b.send(named(Named::Escape));
        assert!(b.gui.talents.is_none());
        b.send(chr("h"));
        assert!(b.gui.home.is_none(), "a view key leaves Home");
        assert_eq!(b.gui.state.view, View::Healing);
    }

    /// Esc on the window, one level at a time: menus, the filter, the
    /// inspector's ability, the keys it holds, the comparison — and then
    /// Home, the front door, where the chain ends: Esc there leaves it
    /// standing. A narrow window's pushed inspector covers the filter, so
    /// its keys come back before the filter's text goes; beside the meter a
    /// pair has no keyed row, so Enter gives it no keys and Esc spends no
    /// press taking back what never showed.
    #[test]
    fn esc_walks_one_level_up_through_the_new_layers() {
        let mut b = home_bridge();
        b.send(Message::WindowWidth(460.0));
        assert_eq!(b.gui.state.screen, Screen::Meter);
        b.send(Message::ToggleOptions);
        b.send(chr("h"));
        assert!(
            !b.gui.options_open,
            "the ⚙ card is modal: any key closes it"
        );
        assert_eq!(b.gui.state.view, View::Damage, "and does nothing else");
        b.send(chr("?"));
        b.send(named(Named::Escape));
        assert!(!b.gui.shortcuts_open, "the sheet goes first");
        b.send(chr("v"));
        b.send(chr("j"));
        assert_eq!(b.gui.state.screen, Screen::Compare, "pinned, then paired");
        b.send(Message::Filter("zz".to_string()));
        b.send(named(Named::Enter));
        assert!(b.gui.state.inspecting(), "narrow: the pair pushed");
        b.send(named(Named::Escape));
        assert!(!b.gui.state.inspecting(), "the pushed inspector's keys");
        assert_eq!(b.gui.filter, "zz", "the field was under it: kept");
        b.send(named(Named::Escape));
        assert!(b.gui.filter.is_empty(), "the filter's text");
        assert_eq!(b.gui.state.screen, Screen::Compare);
        b.send(named(Named::Escape));
        assert_eq!(b.gui.state.screen, Screen::Meter, "the comparison");
        assert!(b.gui.state.compare_picks().is_empty());
        assert!(b.gui.home.is_none());

        // Wide: the pair stands beside the meter.
        b.send(Message::WindowWidth(1440.0));
        b.send(chr("v"));
        b.send(chr("j"));
        assert_eq!(b.gui.state.screen, Screen::Compare);
        b.send(named(Named::Enter));
        assert!(!b.gui.state.inspecting(), "no keyed row to give keys to");
        b.send(Message::Filter("zz".to_string()));
        b.send(named(Named::Escape));
        assert!(b.gui.filter.is_empty(), "the filter first, in sight");
        b.send(named(Named::Escape));
        assert_eq!(b.gui.state.screen, Screen::Meter, "then the pair");
        assert!(b.gui.state.compare_picks().is_empty());

        b.send(named(Named::Enter));
        b.send(Message::SpellRow(0));
        assert!(b.gui.state.drill_spell().is_some());
        b.send(named(Named::Escape));
        assert!(b.gui.state.drill_spell().is_none(), "the ability");
        assert!(b.gui.state.inspecting(), "…inside the inspector");
        b.send(named(Named::Escape));
        assert!(!b.gui.state.inspecting());
        assert!(b.gui.home.is_none());
        b.send(named(Named::Escape));
        assert!(b.gui.home.is_some(), "then the front door");
        assert_eq!(b.gui.state.screen, Screen::Meter, "over the meter");
        b.send(named(Named::Escape));
        assert!(b.gui.home.is_some(), "where the chain ends");
    }

    /// Beside the meter the filter stays in sight while the keys are in
    /// the inspector's list: Esc clears its text first, as the chain says
    /// (menus → filter → narrow inspector), and only then takes the keys
    /// back.
    #[test]
    fn a_wide_esc_clears_the_filter_before_the_inspector_s_keys() {
        let mut b = home_bridge();
        b.send(Message::WindowWidth(1440.0));
        assert_eq!(b.gui.state.screen, Screen::Meter);
        b.send(named(Named::Enter));
        assert!(b.gui.state.inspecting(), "the keys in the list");
        b.send(Message::Filter("zz".to_string()));
        assert!(b.gui.state.inspecting(), "a typed filter keeps them there");
        b.send(named(Named::Escape));
        assert!(b.gui.filter.is_empty(), "the filter's text, in sight");
        assert!(b.gui.state.inspecting(), "the keys stay");
        b.send(named(Named::Escape));
        assert!(!b.gui.state.inspecting(), "then the keys");
    }

    /// `m` and the Live tab end on the live METER: a narrow window's pushed
    /// inspector gives the keys back, even on the live pull already, where
    /// there is no pull to switch to.
    #[test]
    fn m_and_the_live_tab_end_on_the_meter() {
        let (mut state, mut mock) = super::testkit::kill();
        let reqs = state.set_follow(true);
        wowdps_daemon::mock::pump(&mut state, &mut mock, reqs);
        let (mut gui, _peer) = super::testkit::gui_over(state);
        let _ = update(&mut gui, Message::WindowWidth(460.0));
        for gesture in [chr("m"), Message::GotoLive] {
            gui.state.inspect();
            assert!(gui.state.inspecting(), "pushed");
            let _ = update(&mut gui, gesture);
            assert!(!gui.state.inspecting(), "back on the meter");
            assert_eq!(gui.state.screen, Screen::Meter);
        }
    }

    /// The chrome says whose window this is, not what the cursor is on.
    /// Rows resort on every 10 Hz snapshot, so an accent taken from the
    /// selection re-tinted the whole window whenever rank 1 changed class.
    #[test]
    fn the_accent_ignores_the_selection_and_the_sort() {
        let mut b = Bridge::with_config(MockDaemon::fixture().with_history(), class_chrome());
        let rows = b.gui.state.rows();
        let classes: Vec<_> = rows.iter().filter_map(|r| r.class).collect();
        assert!(
            classes.windows(2).any(|w| w[0] != w[1]),
            "the fixture needs two classes for this to mean anything"
        );
        // No owner is known here, so the chrome is neutral — NOT row 0's.
        assert_eq!(view::accent_for_test(&b.gui), theme::NEUTRAL);
        assert_ne!(
            theme::accent(rows[0].class, rows[0].spec),
            theme::NEUTRAL,
            "row 0 does have a class of its own"
        );

        // Walk the selection across classes: the chrome does not move.
        for (i, row) in rows.iter().enumerate() {
            b.send(Message::MeterRow(i));
            assert_eq!(
                view::accent_for_test(&b.gui),
                theme::NEUTRAL,
                "selecting row {i} ({:?}) re-tinted the window",
                row.class
            );
        }
        // Nor does changing the view, which re-sorts the rows entirely.
        b.send(Message::PickView(View::Healing));
        assert_eq!(view::accent_for_test(&b.gui), theme::NEUTRAL);
    }

    #[test]
    fn the_accent_follows_the_owner_once_one_is_known() {
        // `history_characters` is how the window knows which player on the
        // meter is its owner when the store cannot say (one log cannot tell
        // the logger from a guildmate).
        let mut extra = toml::Table::new();
        extra.insert(
            "history_characters".to_string(),
            toml::Value::Array(vec![toml::Value::String("Thraxx-Nebula-US".to_string())]),
        );
        let mut b = Bridge::with_config(
            MockDaemon::fixture().with_history(),
            Config {
                extra,
                ..class_chrome()
            },
        );
        let me = b
            .gui
            .state
            .rows()
            .into_iter()
            .find(|r| r.label == "Thraxx-Nebula-US")
            .expect("the fixture has Thraxx");
        let owner_accent = theme::accent(me.class, me.spec);
        assert_ne!(owner_accent, theme::NEUTRAL);
        assert_eq!(view::accent_for_test(&b.gui), owner_accent);

        // Having resolved, it holds: a resort, a view change and a new
        // selection all leave it where it is.
        b.send(Message::PickView(View::Healing));
        b.send(Message::MeterRow(0));
        assert_eq!(view::accent_for_test(&b.gui), owner_accent);
        for _ in 0..5 {
            let _ = update(&mut b.gui, Message::Tick);
        }
        assert_eq!(
            view::accent_for_test(&b.gui),
            owner_accent,
            "the accent is not re-derived per snapshot"
        );
    }

    /// The other identity: the owner Home derives from the store's cards —
    /// remembered for the next launch whatever Home is scoped to, since the
    /// scope says what Home shows, not whose window it is.
    #[test]
    fn home_naming_the_owner_tints_the_window() {
        let scoped_elsewhere = Config {
            character: Some("Player-1-ALT".to_string()),
            ..class_chrome()
        };
        let mut b = Bridge::with_config(MockDaemon::fixture().with_history(), scoped_elsewhere);
        assert_eq!(view::accent_for_test(&b.gui), theme::NEUTRAL);
        b.gui.home_panels.owner = Some(crate::home::Char {
            guid: "Player-1-MIRELLE".to_string(),
            name: "Mírelle-Nebula-US".to_string(),
            class: Some(wowdps_model::Class::Priest),
            spec: None,
        });
        b.send(Message::Tick);
        assert_eq!(
            view::accent_for_test(&b.gui),
            theme::accent(Some(wowdps_model::Class::Priest), None)
        );
        // And the class is remembered for the next launch, the scope on
        // another character notwithstanding.
        assert_eq!(b.gui.cfg.character_class.as_deref(), Some("Priest"));
    }

    /// Gold needs nobody: a fresh window wears it before its first tick —
    /// before a snapshot, before Home, before any owner — and an owner
    /// resolving later moves nothing, though their class is still learned
    /// and remembered for a class chrome.
    #[test]
    fn the_gold_chrome_is_there_on_the_first_frame() {
        let (client, _peer) = super::testkit::fake_client();
        let gui = Gui::for_test(client, ClientState::new(), test_config());
        assert_eq!(gui.cfg.chrome(), theme::Chrome::Gold, "gold is the default");
        assert!(gui.home.is_none() && gui.owner_name().is_none());
        assert_eq!(view::accent_for_test(&gui), theme::GOLD_ACCENT);
        // The first frame DRAWS in it: the active place's underline, on
        // the top bar's bottom edge, is gold.
        let px = super::testkit::pixels(
            view::view(&gui),
            iced::Size::new(640.0, 480.0),
            &theme(&gui),
        );
        let bar = theme::pitch::TOP_BAR as u32 * 2;
        let gold = px.count_in((0, bar - 4, px.w, bar), theme::GOLD, 2);
        assert!(gold > 60, "only {gold} gold pixels on the bar's edge");
        // And the bar itself is the panel's surface, ruled off by a line.
        assert!(px.count_in((0, 0, px.w, bar - 4), theme::SURFACE, 1) > 1000);

        let mut extra = toml::Table::new();
        extra.insert(
            "history_characters".to_string(),
            toml::Value::Array(vec![toml::Value::String("Thraxx-Nebula-US".to_string())]),
        );
        let b = Bridge::with_config(
            MockDaemon::fixture().with_history(),
            Config {
                extra,
                ..test_config()
            },
        );
        assert_eq!(b.gui.owner_name(), Some("Thraxx-Nebula-US"), "resolved");
        assert_eq!(view::accent_for_test(&b.gui), theme::GOLD_ACCENT, "unmoved");
        let thraxx = b
            .gui
            .state
            .rows()
            .into_iter()
            .find(|r| r.label == "Thraxx-Nebula-US")
            .and_then(|r| r.class)
            .expect("the fixture knows Thraxx's class");
        assert_eq!(
            b.gui.cfg.character_class.as_deref(),
            Some(thraxx.name()),
            "remembered for a class chrome"
        );
    }

    /// Learning the owner's class writes ONE key, into the file as it is
    /// now: whatever the overlay saved since the window launched (a drag,
    /// a zoom) survives. And only the OWNER's class is remembered — whose
    /// window it is, whatever Home is scoped to — while a configured alt
    /// who turns up on the meter when the store names another character
    /// as played last is worn for the session, never written down.
    #[test]
    fn learning_the_class_keeps_the_overlays_placement_and_the_owners_class() {
        // A file of this test's own: every window test saves configs.
        let dir = std::env::temp_dir().join(format!("wowdps-learn-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        Config::use_path_on_this_thread(Some(dir.join("config.toml")));
        let owner = |label: &str| {
            let mut extra = toml::Table::new();
            extra.insert(
                "history_characters".to_string(),
                toml::Value::Array(vec![toml::Value::String(label.to_string())]),
            );
            extra
        };
        // The resolved owner's class is remembered. Home is up at launch,
        // so no pull is on the stage to resolve anyone from yet.
        let mut b = Bridge::with_config(
            MockDaemon::fixture().with_history(),
            Config {
                extra: owner("Thraxx-Nebula-US"),
                home_on_start: true,
                ..class_chrome()
            },
        );
        assert!(b.gui.home.is_some() && b.gui.owner_name().is_none());
        // The overlay is dragged after the window read the config.
        let mut disk = Config::load();
        disk.offset = 4242;
        disk.character_class = None;
        disk.save();
        b.send(Message::GotoFights);
        assert_eq!(b.gui.owner_name(), Some("Thraxx-Nebula-US"));
        let now = Config::load();
        assert_eq!(now.offset, 4242, "the overlay's drag survives");
        let thraxx = now
            .character_class
            .clone()
            .expect("the class is remembered");

        // Home scoped to someone else: the scope is Home's alone, and the
        // owner's class is remembered all the same.
        let mut disk = Config::load();
        disk.character_class = Some("Priest".to_string());
        disk.save();
        let mut b = Bridge::with_config(
            MockDaemon::fixture().with_history(),
            Config {
                extra: owner("Thraxx-Nebula-US"),
                character: Some("Player-0000-SCOPE".to_string()),
                character_class: Some("Priest".to_string()),
                ..class_chrome()
            },
        );
        assert_eq!(b.gui.owner_name(), Some("Thraxx-Nebula-US"));
        assert_eq!(b.gui.cfg.character_class.as_deref(), Some(thraxx.as_str()));
        assert_eq!(
            Config::load().character_class.as_deref(),
            Some(thraxx.as_str()),
            "the scope does not keep the owner's class from being remembered"
        );

        // An alt on the meter while the store names another character as
        // played last: worn for the session, never written down.
        b.gui.owner_guid = Some("Player-0000-LAST".to_string());
        b.gui
            .learn_owner_class(Some(wowdps_model::Class::Priest), None, Some("Player-ALT"));
        assert_eq!(
            view::accent_for_test(&b.gui),
            theme::accent(Some(wowdps_model::Class::Priest), None),
            "the session wears the alt"
        );
        assert_eq!(b.gui.cfg.character_class.as_deref(), Some(thraxx.as_str()));
        assert_eq!(
            Config::load().character_class.as_deref(),
            Some(thraxx.as_str()),
            "the owner's class is not overwritten by an alt's"
        );
        Config::use_path_on_this_thread(None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A class chrome is right on the first frame too: the class the config
    /// remembers for the locked character, before anyone resolves — and,
    /// with nothing remembered, neutral rather than a borrowed row.
    #[test]
    fn a_class_chrome_wears_the_remembered_class_at_launch() {
        let (client, _peer) = super::testkit::fake_client();
        let cfg = Config {
            character_class: Some("Priest".to_string()),
            ..class_chrome()
        };
        let gui = Gui::for_test(client, ClientState::new(), cfg);
        assert_eq!(
            view::accent_for_test(&gui),
            theme::accent(Some(wowdps_model::Class::Priest), None)
        );
        let (client, _peer) = super::testkit::fake_client();
        let gui = Gui::for_test(client, ClientState::new(), class_chrome());
        assert_eq!(view::accent_for_test(&gui), theme::NEUTRAL);
    }

    /// The gear and the `?` on the top bar are where a reader presses them,
    /// on every screen, and each opens what it says: the options card (the
    /// one way into the chrome setting) and the shortcut sheet.
    #[test]
    fn the_top_bar_s_gear_and_help_open_their_cards() {
        let (client, _peer) = super::testkit::fake_client();
        let gui = Gui::for_test(client, ClientState::new(), test_config());
        for (id, want) in [
            (crate::nav::gear_id(), "the gear"),
            (crate::nav::help_id(), "the help button"),
        ] {
            let mut ui = simulator(view::view(&gui));
            ui.click(id)
                .unwrap_or_else(|e| panic!("{want} is on the bar: {e}"));
            let sent: Vec<Message> = ui.into_messages().collect();
            let opened = match want {
                "the gear" => sent.iter().any(|m| matches!(m, Message::ToggleOptions)),
                _ => sent.iter().any(|m| matches!(m, Message::ToggleShortcuts)),
            };
            assert!(opened, "{want} sent {sent:?}");
        }
    }

    /// The window's own settings load its own faces and make Barlow its
    /// default: what the window draws with is what the tests measure.
    #[test]
    fn the_window_s_settings_load_its_fonts_and_default_to_barlow() {
        let s = settings();
        assert_eq!(s.default_font, theme::UI);
        for face in theme::FONTS {
            assert!(
                s.fonts.iter().any(|f| f.as_ref() == face),
                "a window face is not loaded"
            );
        }
        assert_eq!(s.fonts.len(), theme::FONTS.len());
    }

    /// The options card offers the chrome as two chips; a pick saves it and
    /// re-dresses the window at once.
    #[test]
    fn the_options_card_switches_the_chrome() {
        let (client, _peer) = super::testkit::fake_client();
        let cfg = Config {
            character_class: Some("Death Knight".to_string()),
            ..test_config()
        };
        let mut gui = Gui::for_test(client, ClientState::new(), cfg);
        let _ = update(&mut gui, Message::ToggleOptions);
        {
            let mut ui = simulator(view::view(&gui));
            assert!(ui.find("Game gold").is_ok());
            ui.click("Your class").unwrap();
            assert!(
                ui.into_messages()
                    .any(|m| matches!(m, Message::SetChrome(theme::Chrome::Class)))
            );
        }
        let _ = update(&mut gui, Message::SetChrome(theme::Chrome::Class));
        assert_eq!(gui.cfg.chrome, "class");
        assert_eq!(
            view::accent_for_test(&gui),
            theme::accent(Some(wowdps_model::Class::DeathKnight), None)
        );
        let _ = update(&mut gui, Message::SetChrome(theme::Chrome::Gold));
        assert_eq!(gui.cfg.chrome(), theme::Chrome::Gold);
        assert_eq!(view::accent_for_test(&gui), theme::GOLD_ACCENT);
    }

    #[test]
    fn view_tabs_switch_the_view_like_the_keys() {
        let mut b = home_bridge();
        b.send(Message::PickView(View::Taken));
        assert_eq!(b.gui.state.view, View::Taken);
        assert!(b.gui.home.is_none());
    }

    /// The by-spell pane is the throughput table: it sorts like the meter,
    /// and j/k walk the DRAWN order, landing on the pane's own selection.
    #[test]
    fn the_spell_pane_sorts_and_the_keys_walk_the_drawn_order() {
        let mut b = home_bridge();
        b.send(named(Named::Enter));
        let (by_spell, _) = b.gui.state.breakdown();
        assert!(by_spell.len() > 2, "the fixture drill has spells to sort");
        b.send(Message::SortSpellsBy(crate::table::Col::Crit));
        assert_eq!(b.gui.drill_sort, Some((crate::table::Col::Crit, true)));
        let order: Vec<usize> =
            crate::table::sorted(by_spell.into_iter().enumerate().collect(), b.gui.drill_sort)
                .into_iter()
                .map(|(i, _)| i)
                .collect();
        // R26: the pane is the ability tree, sorted the same way at each
        // level — the hunter's pet sums under one line. The keys start on
        // row 0 of the daemon's order; a step down lands on whatever LINE
        // is drawn under it now, a group's included.
        let lines = b.gui.tree_lines().unwrap();
        let tops: Vec<usize> = lines
            .iter()
            .filter_map(|l| match l.node {
                crate::inspector::tree::Node::Row(i) => Some(i),
                _ => None,
            })
            .collect();
        assert!(
            order.iter().filter(|i| tops.contains(i)).eq(tops.iter()),
            "the rows the tree draws keep the sort's order: {tops:?} {order:?}"
        );
        let at = b.gui.tree_keyed(&lines).unwrap();
        assert_eq!(lines[at].node, crate::inspector::tree::Node::Row(0));
        b.send(chr("j"));
        let after = b.gui.tree_lines().unwrap();
        assert_eq!(
            b.gui.tree_keyed(&after),
            Some((at + 1).min(lines.len() - 1))
        );
        // The cycle: desc → asc → the daemon's order.
        b.send(Message::SortSpellsBy(crate::table::Col::Crit));
        assert_eq!(b.gui.drill_sort, Some((crate::table::Col::Crit, false)));
        b.send(Message::SortSpellsBy(crate::table::Col::Crit));
        assert_eq!(b.gui.drill_sort, None);
        // The meter's own sort cycles the same way and is a separate state.
        b.send(Message::SortBy(crate::table::Col::Rate));
        assert_eq!(b.gui.sort, Some((crate::table::Col::Rate, true)));
        assert_eq!(b.gui.drill_sort, None);
    }

    /// R26: the hunter's abilities are a tree — Sharptooth's two under the
    /// pet's line, shut. The keys walk its lines; → opens the line they are
    /// on, ← shuts it or climbs to the line holding it; Enter on a group
    /// folds it and opens no ability; a group's press folds it too; and the
    /// pull stays put through all of it.
    #[test]
    fn the_ability_tree_folds_by_key_and_by_press() {
        use crate::inspector::tree::{Node, group_fold};
        let mut b = home_bridge();
        b.send(named(Named::Enter));
        assert!(b.gui.state.inspecting());
        let pull = b.gui.state.segment_index();
        let names = |b: &Bridge| -> Vec<(u8, String)> {
            b.gui
                .tree_lines()
                .unwrap()
                .into_iter()
                .map(|l| (l.depth, l.name))
                .collect()
        };
        let keyed = |b: &Bridge| {
            let lines = b.gui.tree_lines().unwrap();
            b.gui
                .tree_keyed(&lines)
                .map(|at| lines[at].node.clone())
                .unwrap()
        };
        assert_eq!(
            names(&b),
            [
                (0, "Aimed Shot".to_string()),
                (0, "Sharptooth".to_string()),
                (0, "Serpent Sting".to_string()),
            ]
        );
        b.send(chr("j"));
        let group = Node::Group("pet:Sharptooth".to_string());
        assert_eq!(keyed(&b), group, "the keys on the pet's line");
        assert_eq!(b.gui.state.drill.as_ref().unwrap().spell_sel, 0);
        b.send(named(Named::ArrowRight));
        assert!(b.gui.tree_open.contains(&group_fold("pet:Sharptooth")));
        assert_eq!(
            names(&b),
            [
                (0, "Aimed Shot".to_string()),
                (0, "Sharptooth".to_string()),
                (1, "Bite".to_string()),
                (1, "Melee".to_string()),
                (0, "Serpent Sting".to_string()),
            ],
            "the pet's name leaves its abilities: the group says whose"
        );
        b.send(chr("j"));
        assert_eq!(keyed(&b), Node::Row(1), "Bite");
        assert_eq!(b.gui.state.drill.as_ref().unwrap().spell_sel, 1);
        b.send(named(Named::ArrowLeft));
        assert_eq!(keyed(&b), group, "← climbs to the line holding it");
        b.send(named(Named::ArrowLeft));
        assert!(b.gui.tree_open.is_empty(), "…and shuts it");
        assert_eq!(b.gui.state.segment_index(), pull, "no pull step");
        // Enter on the group folds it, and opens no ability.
        b.send(named(Named::Enter));
        assert!(b.gui.tree_open.contains(&group_fold("pet:Sharptooth")));
        assert!(b.gui.state.drill_spell().is_none());
        // A press on its line folds it back.
        b.send(Message::TreeFold(group_fold("pet:Sharptooth")));
        assert!(b.gui.tree_open.is_empty());
        // Enter on a row still opens its ability.
        b.send(chr("j"));
        assert_eq!(keyed(&b), Node::Row(3), "Serpent Sting");
        b.send(named(Named::Enter));
        assert_eq!(
            b.gui.state.drill_spell().map(|(k, _)| k.as_str()),
            Some("Serpent Sting")
        );
    }

    /// v28: beside the Deaths meter the inspector is the selection's
    /// recap; with the keys in it (Enter) ← → step its death windows, and
    /// a chip asks for one directly. On the meter — whoever is selected,
    /// however many times they died — and on any other view the arrows
    /// step pulls, so a player's death count never changes what a key
    /// does; in the recap with one window they are swallowed rather than
    /// leave the fight. Both counts are made to happen: two windows on a
    /// raid the test writes, one on the fixture's.
    #[test]
    fn arrows_step_death_windows_on_a_deaths_drill_only() {
        use wowdps_model::{SegmentInfo, SegmentKind};
        use wowdps_proto::{Breakdown, DaemonMsg, DeathWindow, SegmentRef};
        // Two windows: ← asks for the one before, and the pull stays.
        let mut state = super::testkit::raid(3);
        let rows = state.rows();
        state.view = View::Deaths;
        let _ = state.set_follow(true);
        let snap = DaemonMsg::Snapshot {
            seq: 3,
            segment: SegmentRef::Live,
            id: None,
            view: View::Deaths,
            info: SegmentInfo {
                kind: SegmentKind::Encounter,
                name: "The Coiled Altar".to_string(),
                start_ms: 1_000,
                duration_ms: 60_000,
                success: Some(true),
                live: false,
                instance: Some(0),
                pars_ms: None,
                arena: false,
                encounter: None,
            },
            total_rows: 3,
            rows,
            breakdown: Some(Breakdown {
                deaths: vec![
                    DeathWindow {
                        index: 0,
                        at_ms: 20_000,
                    },
                    DeathWindow {
                        index: 1,
                        at_ms: 50_000,
                    },
                ],
                death_index: Some(1),
                ..Breakdown::default()
            }),
            segment_count: 1,
            source: Some("raid.txt".to_string()),
            status: None,
            raid: None,
        };
        // The first snapshot names the drill (and drops the breakdown it
        // carried for no one); the second is that drill's.
        let _ = state.on_msg(snap.clone());
        let _ = state.on_msg(snap);
        assert_eq!(state.deaths().0.len(), 2, "two windows to step");
        let (mut gui, _peer) = super::testkit::gui_over(state);
        // The keys on the meter: the arrows are the pulls', two deaths or
        // not.
        let _ = update(&mut gui, named(Named::ArrowLeft));
        assert_eq!(gui.state.death_request(), None, "no window asked for");
        // In the recap they step its windows, and the pull stays.
        gui.state.inspect();
        let before = gui.state.segment_index();
        let _ = update(&mut gui, named(Named::ArrowLeft));
        assert_eq!(gui.state.death_request(), Some(0), "the window before");
        assert_eq!(gui.state.segment_index(), before, "no pull step");

        // One window on the fixture: nothing to step, so ← steps the pull.
        let mut b = home_bridge();
        b.send(chr("K"));
        assert!(b.gui.state.drill.is_some(), "the recap follows the row");
        let (deaths, shown) = b.gui.state.deaths();
        assert_eq!(
            deaths.len(),
            1,
            "the fixture's top death row has one window"
        );
        assert_eq!(
            shown,
            Some(0),
            "the daemon describes the last death by default"
        );
        b.send(Message::PickDeath(0));
        assert_eq!(b.gui.state.death_request(), Some(0), "a chip asks");
        // In the recap, one window: nowhere to step, and the fight stays.
        b.gui.state.inspect();
        let before = b.gui.state.segment_index();
        b.send(named(Named::ArrowLeft));
        assert_eq!(b.gui.state.segment_index(), before, "swallowed");
        assert_eq!(b.gui.state.death_request(), Some(0));
        // On the meter the arrows are the pulls'.
        b.gui.state.uninspect();
        b.send(named(Named::ArrowLeft));
        assert_ne!(
            b.gui.state.segment_index(),
            before,
            "the keys on the meter: ← steps the pull"
        );
        // On Damage the arrows are segment keys, whatever the recap had.
        b.send(chr("d"));
        assert_eq!(b.gui.state.death_request(), None);
        let before = b.gui.state.segment_index();
        b.send(named(if before > 0 {
            Named::ArrowLeft
        } else {
            Named::ArrowRight
        }));
        assert_ne!(
            b.gui.state.segment_index(),
            before,
            "← → step the pull here"
        );
    }

    /// The inspector is the selection's, and the selection a DRAWN row: a
    /// filter that hides the selected player moves the selection — and the
    /// inspector with it — to the first row it draws (`ensureSel`).
    #[test]
    fn a_filter_that_hides_the_selection_moves_it() {
        let mut b = home_bridge();
        let rows = b.gui.state.rows();
        assert!(rows.len() > 1, "two players to choose between");
        b.send(Message::MeterRow(1));
        let first = crate::view::realmless(&rows[0].label);
        b.send(Message::Filter(first.clone()));
        assert_eq!(b.gui.state.row_sel, 0, "the one row {first} draws");
        assert_eq!(
            b.gui.state.drill.as_ref().map(|d| d.key.as_str()),
            Some(rows[0].key.as_str()),
            "the inspector followed"
        );
        // A filter that keeps the selection drawn leaves it be.
        b.send(Message::Filter(String::new()));
        b.send(Message::MeterRow(1));
        b.send(Message::Filter(crate::view::realmless(&rows[1].label)));
        assert_eq!(b.gui.state.row_sel, 1);
    }

    /// A narrow window's meter has no inspector on screen: Tab and `g`
    /// push it, as Enter does, rather than change what nobody can see. A
    /// wide one has it beside the meter, and they change it in place.
    #[test]
    fn narrow_tab_and_g_push_the_inspector_they_change() {
        let mut b = home_bridge();
        b.send(Message::WindowWidth(460.0));
        b.send(named(Named::Tab));
        assert!(b.gui.state.inspecting(), "Tab pushed it");
        assert_eq!(
            b.gui.state.drill.as_ref().map(|d| d.pane),
            Some(Pane::Target)
        );
        b.send(named(Named::Escape));
        assert!(!b.gui.state.inspecting());
        b.send(chr("g"));
        assert!(b.gui.state.inspecting(), "g pushed it");
        b.send(named(Named::Escape));
        b.send(Message::WindowWidth(1440.0));
        b.send(named(Named::Tab));
        assert!(!b.gui.state.inspecting(), "beside the meter: in place");
    }

    /// Once the keys are in the inspector, a step in its list asks for the
    /// scroll that keeps the keyed row in sight — as a step on the meter
    /// does for the meter's row.
    #[test]
    fn a_step_in_the_inspector_keeps_its_row_in_sight() {
        let mut b = home_bridge();
        b.send(named(Named::Enter));
        assert!(b.gui.state.inspecting());
        assert!(
            update(&mut b.gui, chr("j")).units() > 0,
            "a scroll asked for"
        );
    }

    /// A move of the selection drops the breakdown in hand; until the new
    /// player's lands, the last one's graph and lists stand in, dimmed,
    /// so the column keeps its height — and go as soon as it does.
    #[test]
    fn the_last_body_stands_in_while_the_next_is_on_its_way() {
        let (mut state, mut mock) = super::testkit::kill();
        let reqs = state.set_follow(true);
        wowdps_daemon::mock::pump(&mut state, &mut mock, reqs);
        let (mut gui, _peer) = super::testkit::gui_over(state);
        let _ = update(&mut gui, Message::Tick);
        assert!(gui.state.drill_breakdown().is_some(), "the first player's");
        assert!(!crate::inspector::Insp::of(&gui).stale());
        // The peer never answers: the next player's breakdown stays away.
        let _ = update(&mut gui, chr("j"));
        assert!(gui.state.drill_breakdown().is_none(), "on its way");
        let insp = crate::inspector::Insp::of(&gui);
        assert!(insp.stale(), "the last body stands in");
        let _ = super::testkit::render(insp.view(400.0, crate::inspector::Fit::Tile, false));
    }

    /// A stored pull opens on the stage and is drawn by the renderers that
    /// draw the log's: the fight header over the meter, the inspector
    /// beside it on the selection, the views refetched from the store. What
    /// the store keeps no answer for — a comparison, the enemies — is
    /// refused rather than asked for; `m` leaves it for the live pull.
    #[test]
    fn a_stored_pull_is_drawn_by_the_meter_s_own_renderers() {
        let mut b = Bridge::new(MockDaemon::fixture().with_history());
        let card = b
            .mock
            .history()
            .cards()
            .iter()
            .find(|c| c.name == "The Ashen Warden" && c.success == Some(true))
            .cloned()
            .expect("the fixture's kill is stored");
        // The card is the log's own pull: it opens as the log's.
        b.send(Message::OpenStored(card.id.clone()));
        assert!(b.gui.stored.is_none(), "the log answers it");
        assert_eq!(
            b.gui.fight().segment_name().as_deref(),
            Some("The Ashen Warden")
        );
        assert!(matches!(b.gui.current_pull(), Some(Pull::Log(_))));
        // Another log's card opens from the store.
        b.foreign_store();
        b.send(Message::OpenStored(card.id.clone()));
        let s = b.gui.stored.as_ref().expect("the pull is on the stage");
        assert_eq!(s.fight_id, card.id);
        assert!(!s.missing, "the mock answered GetFight");
        let fight = b.gui.fight();
        assert_eq!(fight.segment_name().as_deref(), Some("The Ashen Warden"));
        assert!(!fight.rows().is_empty());
        assert!(fight.drill.is_some(), "the inspector follows the selection");
        assert!(fight.drill_breakdown().is_some(), "the store keeps details");
        assert_eq!(
            b.gui.current_pull(),
            Some(Pull::Stored(card.id.clone())),
            "the rail's pull on the stage"
        );
        {
            let mut ui = super::testkit::wide(view::view(&b.gui));
            // The header: the title, the card's outcome, the stat line.
            assert!(ui.find("Kill").is_ok());
            assert!(ui.find("Raid dps").is_ok());
            // The inspector beside the meter, on the selection.
            assert!(ui.find("Talents and gear").is_ok());
            // The meter's own table.
            assert!(ui.find("Per sec").is_ok());
            assert!(ui.find(view::meter_list_id()).is_ok());
            // Compare keeps its place, inert: no comparison to ask for.
            ui.click("Compare").expect("the action row keeps its shape");
            let sent: Vec<Message> = ui.into_messages().collect();
            assert!(
                !sent.iter().any(|m| matches!(m, Message::PinCompare)),
                "{sent:?}"
            );
        }
        // A view key refetches the pull on that view.
        b.send(chr("h"));
        assert_eq!(b.gui.fight().view, View::Healing);
        assert!(b.gui.fight().view_answered());
        // What the store cannot answer is swallowed.
        b.send(chr("E"));
        assert_eq!(b.gui.fight().view, View::Healing, "no enemies stored");
        b.send(chr("v"));
        assert!(b.gui.fight().compare_picks().is_empty(), "no pair stored");
        b.send(named(Named::Enter));
        assert!(b.gui.fight().inspecting(), "Enter hands the keys over");
        b.send(named(Named::Enter));
        assert!(
            b.gui.fight().drill_spell().is_none(),
            "no ability's own curve in the store"
        );
        // Esc: the keys, then Home — the stored pull stays under it.
        b.send(named(Named::Escape));
        assert!(!b.gui.fight().inspecting());
        b.send(named(Named::Escape));
        assert!(b.gui.home.is_some());
        assert!(b.gui.stored.is_some());
        // m: the live pull, the stored one set down.
        b.send(chr("m"));
        assert!(b.gui.home.is_none() && b.gui.stored.is_none());
        assert!(b.gui.state.following_live());
    }

    /// A view a stored pull lacks stays on the strip, disabled, saying why
    /// under the pointer: the Enemies tab sends nothing there, where on the
    /// log's pull it is a view like the rest.
    #[test]
    fn a_stored_pull_s_enemy_tab_is_there_and_disabled() {
        let mut b = Bridge::new(MockDaemon::fixture().with_history());
        let size = iced::Size::new(1440.0, 900.0);
        let tab_at = |b: &Bridge| {
            let mut ui = super::testkit::simulator_as(settings(), size, view::view(&b.gui));
            ui.find("Enemies").expect("the tab is drawn").bounds()
        };
        let press = |b: &Bridge, at: iced::Rectangle| {
            let mut ui = super::testkit::simulator_as(settings(), size, view::view(&b.gui));
            ui.point_at(at.center());
            let _ = ui.simulate(iced_test::simulator::click());
            ui.into_messages().collect::<Vec<Message>>()
        };
        let live = tab_at(&b);
        assert!(
            press(&b, live)
                .iter()
                .any(|m| matches!(m, Message::PickView(View::EnemyTaken))),
            "the log's pull has its enemies"
        );
        let id = b.mock.history().cards()[0].id.clone();
        b.foreign_store();
        b.send(Message::OpenStored(id));
        assert!(b.gui.stored.is_some());
        let stored = tab_at(&b);
        assert!(press(&b, stored).is_empty(), "a disabled tab is silent");
        // Its tip floats under it on the pointer, framed in the floating
        // edge (the inspector's surface is under it, so its frame is what
        // tells it apart).
        let band = (
            ((stored.x - 160.0) * 2.0) as u32,
            ((stored.y + stored.height + 2.0) * 2.0) as u32,
            ((stored.x + 260.0) * 2.0) as u32,
            ((stored.y + stored.height + 40.0) * 2.0) as u32,
        );
        let tipped = |at: Option<iced::Point>| {
            super::testkit::pixels_at(view::view(&b.gui), size, &theme(&b.gui), at).count_in(
                band,
                theme::EDGE,
                2,
            )
        };
        assert!(
            tipped(Some(stored.center())) > tipped(None) + 100,
            "{:?} is said under the pointer",
            view::NOT_STORED
        );
    }

    /// Esc with no pull on the stage — a log with nothing in it yet — lands
    /// on Home, the front door; Esc there leaves it standing.
    #[test]
    fn esc_on_an_empty_stage_opens_home_where_the_chain_ends() {
        let (mut gui, _peer) = super::testkit::gui_over(ClientState::new());
        assert_eq!(gui.state.screen, Screen::List, "nothing to put on it");
        let _ = update(&mut gui, named(Named::Escape));
        assert!(gui.home.is_some());
        let _ = update(&mut gui, named(Named::Escape));
        assert!(gui.home.is_some(), "Esc backs out, it never toggles");
    }

    /// The top bar's places: Home opens Home and stays there; Fights sets
    /// Home aside for the pull on the stage, as it stood — its drill and
    /// all.
    #[test]
    fn the_places_go_home_and_back_to_the_pull() {
        let mut b = home_bridge();
        b.send(named(Named::Enter));
        let drill = b.gui.state.drill.clone();
        assert!(drill.is_some());
        b.send(Message::GotoHome);
        assert!(b.gui.home.is_some());
        b.send(Message::GotoHome);
        assert!(b.gui.home.is_some(), "a place, not a toggle");
        b.send(Message::GotoFights);
        assert!(b.gui.home.is_none());
        assert_eq!(b.gui.state.drill, drill, "the pull as it stood");
        assert_eq!(b.gui.state.screen, Screen::Meter);
    }

    #[test]
    fn m_from_home_pins_live() {
        let mut b = home_bridge();
        b.send(chr("~"));
        b.send(chr("m"));
        assert!(b.gui.home.is_none());
        assert!(b.gui.state.following_live());
    }

    /// `m` is not Home's alone: it pins the live meter from any surface.
    #[test]
    fn m_pins_live_from_the_meter_too() {
        let mut b = home_bridge();
        assert_eq!(b.gui.state.screen, Screen::Meter);
        b.send(chr("m"));
        assert!(b.gui.state.following_live());
    }

    /// The sheet is keyed on the surface: the meter's keys, the drill's
    /// once Enter hands them over, Home's and the viewer's.
    #[test]
    fn the_sheet_knows_which_surface_it_is_on() {
        let mut b = home_bridge();
        assert_eq!(b.gui.surface(), keys::Surface::Meter);
        b.send(named(Named::Enter));
        assert_eq!(b.gui.surface(), keys::Surface::Drill);
        b.send(chr("~"));
        assert_eq!(b.gui.surface(), keys::Surface::Home);
        b.send(chr("t"));
        assert_eq!(
            b.gui.surface(),
            keys::Surface::Talents,
            "the viewer opens from Home too, and wins over it"
        );
    }

    /// The degraded/disabled banner is only honest if it is current: the
    /// daemon never broadcasts `Status`, so opening Home has to ask.
    #[test]
    fn opening_home_asks_the_daemon_how_the_store_is() {
        let mut b = home_bridge();
        let _ = b.requests();
        let _ = update(&mut b.gui, chr("~"));
        assert!(
            b.requests()
                .iter()
                .any(|r| matches!(r, ClientMsg::GetStatus { .. })),
            "Home opened without asking for the store's state"
        );
    }

    #[test]
    fn the_store_state_reaches_an_open_home() {
        let mut b = home_bridge();
        b.send(chr("~"));
        b.push(&DaemonMsg::Status {
            req_id: 7,
            game_running: false,
            source: None,
            clients: 1,
            linger: false,
            overlay: wowdps_proto::OverlayState::Absent,
            history: wowdps_proto::msg::HistoryStatus {
                enabled: false,
                error: Some("history_enabled = false".to_string()),
                dropped: 2,
                ..Default::default()
            },
        });
        b.settle();
        let ui = b.gui.home.as_ref().unwrap();
        assert_eq!(
            ui.disabled_reason.as_deref(),
            Some("history_enabled = false")
        );
        assert_eq!(ui.dropped, 2);
        // Re-opening asks again, and the daemon's live answer wins over the
        // stale one — which is the whole point of asking on open.
        b.send(chr("~"));
        b.send(chr("~"));
        let ui = b.gui.home.as_ref().unwrap();
        assert_eq!(ui.disabled_reason, None, "the store is actually up");
        assert_eq!(ui.dropped, 0);
    }

    #[test]
    fn a_stored_fight_refills_the_list() {
        let mut b = home_bridge();
        b.send(chr("~"));
        let before = b.gui.home.as_ref().unwrap().cards.len();
        assert!(before > 0);
        b.push(&DaemonMsg::HistoryChanged {
            fight_id: "whatever".to_string(),
        });
        b.settle();
        assert_eq!(
            b.gui.home.as_ref().unwrap().cards.len(),
            before,
            "the list is rebuilt, not doubled"
        );
    }

    /// Home is where Esc's chain ends: Esc there leaves it standing, and a
    /// scope survives it.
    #[test]
    fn esc_on_home_stops_there() {
        let mut b = owned_bridge();
        b.send(chr("~"));
        let owner = b.gui.home_panels.owner.clone().map(|o| o.guid);
        b.send(Message::HomeCharacter(owner.clone()));
        b.send(named(Named::Escape));
        assert!(b.gui.home.is_some(), "Home itself is where Esc ends");
        assert_eq!(b.gui.home.as_ref().unwrap().scope, owner);
        // Home is a page of the week, not an endless list: no pager on it.
        let mut ui = simulator(view::view(&b.gui));
        for pager in ["next", "prev", "load more", "page"] {
            assert!(ui.find(pager).is_err(), "{pager} is a pager control");
        }
    }

    /// R24: the enemy drill is attackers first — drawn as meter rows, class
    /// and spec on them — then one attacker's abilities on that enemy, the
    /// same Enter that walks player → ability elsewhere.
    #[test]
    fn the_enemy_drill_lists_attackers_then_their_abilities() {
        let mut b = Bridge::new(MockDaemon::fixture());
        b.send(chr("m"));
        b.send(chr("E"));
        assert_eq!(b.gui.state.view, wowdps_model::View::EnemyTaken);
        assert!(
            !b.gui.state.rows().is_empty(),
            "the fixture's enemies took damage"
        );
        b.send(named(Named::Enter));
        assert!(b.gui.state.drill.is_some(), "Enter drills the top enemy");
        let (by_spell, by_attacker) = b.gui.state.breakdown();
        assert!(by_spell.is_empty(), "one list at the enemy level");
        // v33: a zoom window on the graph scopes the attackers to it — the
        // window rides the watch, the daemon answers the windowed list and
        // echoes the window.
        let whole: u64 = by_attacker.iter().map(|r| r.amount).sum();
        b.requests();
        let reqs = b.gui.state.set_drill_range(Some((0, 1_000)));
        assert!(
            matches!(
                reqs.first(),
                Some(wowdps_proto::ClientMsg::Watch(
                    wowdps_proto::Cursor::Segment {
                        range: Some((0, 1_000)),
                        ..
                    }
                ))
            ),
            "the window rides the watch: {reqs:?}"
        );
        // The direct call above already holds the window, so the message must
        // see a CHANGE to re-watch: clear it first.
        b.gui.state.set_drill_range(None);
        b.send(Message::DrillRange(Some((0, 1_000))));
        assert_eq!(
            b.gui.state.drill_breakdown().and_then(|bd| bd.range),
            Some((0, 1_000)),
            "the daemon echoed the window"
        );
        let (_, windowed) = b.gui.state.breakdown();
        let part: u64 = windowed.iter().map(|r| r.amount).sum();
        assert!(
            part < whole,
            "one second of the fight is less than all of it: {part} vs {whole}"
        );
        b.send(Message::DrillRange(None));
        let (_, again) = b.gui.state.breakdown();
        assert_eq!(
            again.iter().map(|r| r.amount).sum::<u64>(),
            whole,
            "zoomed out is the whole again"
        );
        assert!(
            by_attacker.iter().any(|r| r.class.is_some()),
            "attacker rows carry their class: {by_attacker:?}"
        );
        {
            let mut ui = super::testkit::wide(view::view(&b.gui));
            // The attackers are the inspector's one list, beside the meter
            // of enemies — no tabs, no drill panes.
            assert!(ui.find("Attacker").is_ok());
            assert!(ui.find("Per sec").is_ok(), "the meter, beside");
            assert!(ui.find("Hit by").is_err());
            assert!(ui.find("By attacker").is_err());
        }
        b.send(named(Named::Enter));
        assert!(
            b.gui.state.drill_spell().is_some(),
            "Enter descends into the top attacker"
        );
        assert!(
            !b.gui.state.spell_target_rows().is_empty(),
            "their abilities on the enemy"
        );
        let mut ui = super::testkit::wide(view::view(&b.gui));
        assert!(ui.find("Ability").is_ok(), "their abilities on it");
        assert!(ui.find("Targets").is_err());
    }

    /// The header's step buttons are `]` and `[` for the pointer, and walk
    /// the rail: from its top there is only older, and a step there and
    /// back again lands where it started.
    #[test]
    fn the_header_steps_walk_the_pulls() {
        let mut b = Bridge::new(MockDaemon::fixture());
        let lines: Vec<Pull> = b.gui.rail().lines().map(|l| l.pull.clone()).collect();
        assert!(lines.len() >= 2, "the fixture has pulls to walk");
        b.send(Message::Pull(lines[0].clone()));
        assert_eq!(b.gui.current_pull().as_ref(), Some(&lines[0]));
        let head = crate::fight_head::Head::of(&b.gui, true);
        assert!(!head.newer && head.older, "the rail's top steps older only");
        b.send(Message::OlderPull);
        assert_eq!(b.gui.current_pull().as_ref(), Some(&lines[1]));
        let head = crate::fight_head::Head::of(&b.gui, true);
        assert!(head.newer, "and back");
        b.send(Message::NewerPull);
        assert_eq!(b.gui.current_pull().as_ref(), Some(&lines[0]));
    }

    /// v35: the owner is the row the daemon marks `mine` — else, while it
    /// marks nobody, the character the window is locked to (its guid) and
    /// the configured names; and the "you" chip selects their row.
    #[test]
    fn the_you_chip_selects_the_owner_s_row() {
        let (state, _mock) = testkit::kill();
        let rows = state.rows();
        let (mut gui, _peer) = testkit::gui_over(state);
        assert_eq!(gui.owner_row(), None, "nobody marked, nobody locked");
        let me = rows.len() - 1;
        let name = rows[me].label.split('-').next().unwrap().to_uppercase();
        gui.cfg.extra.insert(
            "history_characters".to_string(),
            toml::Value::String(format!("Somebody-Else-US, {name}")),
        );
        // A daemon that marks nobody (its store off, not yet published):
        // the window's own hint, the configured names, stands in — so the
        // top bar and the meter agree on who the reader is.
        assert_eq!(gui.owner_row(), Some(me), "the configured name, unmarked");
        // The chrome's accent still resolves from the config's names.
        gui.resolve_accent();
        assert_eq!(gui.owner_name(), Some(rows[me].label.as_str()));
        gui.cfg.extra.clear();
        // The name the accent resolved from is a hint of its own.
        assert_eq!(gui.owner_row(), Some(me), "the accent's name, unmarked");
        gui.accent_owner = None;
        assert_eq!(gui.owner_row(), None, "no hint left");
        // The same name told to the daemon: it marks the row, bare name,
        // any case.
        let (state, _mock) =
            testkit::kill_on(MockDaemon::fixture().with_characters(std::slice::from_ref(&name)));
        let marked = state.rows();
        let (gui, _peer) = testkit::gui_over(state);
        let at = marked.iter().position(|r| r.label == rows[me].label);
        assert!(at.is_some_and(|i| marked[i].mine), "{marked:?}");
        assert_eq!(gui.owner_row(), at, "the row the daemon marked");
        let (state, _mock) = testkit::kill();
        let (mut gui, _peer) = testkit::gui_over(state);
        gui.owner_guid = Some(rows[me].key.clone());
        assert_eq!(gui.owner_row(), Some(me), "the lock's guid");
        gui.state.row_sel = 0;
        let _ = update(&mut gui, Message::SelectOwner);
        assert_eq!(gui.state.row_sel, me);
        assert_eq!(
            gui.state.drill.as_ref().map(|d| d.key.as_str()),
            Some(rows[me].key.as_str()),
            "the inspector follows the chip's press"
        );
        // A filter that hides the owner gives way to the press: the
        // selection never sits on a row the reader cannot see.
        gui.state.row_sel = 0;
        gui.filter = "zzz-nobody".to_string();
        let _ = update(&mut gui, Message::SelectOwner);
        assert_eq!(gui.state.row_sel, me);
        assert!(gui.filter.is_empty(), "the filter cleared");
        // One that keeps them is kept.
        let keeps: String = rows[me].label.chars().take(3).collect();
        gui.filter = keeps.clone();
        let _ = update(&mut gui, Message::SelectOwner);
        assert_eq!(gui.filter, keeps);
        gui.filter.clear();
        // The chip is drawn over the table — the first place the owner's
        // name is found — and pressing it is the same message.
        gui.cfg.hide_realms = true;
        let label = view::display_name(&rows[me].label).to_string();
        let mut ui = testkit::wide(view::view(&gui));
        ui.click(label.as_str()).expect("the chip names the owner");
        let sent: Vec<Message> = ui.into_messages().collect();
        assert!(
            matches!(sent.as_slice(), [Message::SelectOwner]),
            "{sent:?}"
        );
    }

    /// The picker is the top bar's on every screen, Home's too, and its
    /// menu scopes Home: a pick opens Home (when it is not up) on that
    /// character and closes the menu; Esc closes it and does nothing else.
    #[test]
    fn the_picker_menu_scopes_home() {
        let mut b = owned_bridge();
        b.send(chr("~"));
        let owner = b
            .gui
            .home_panels
            .owner
            .clone()
            .expect("the store names the owner");
        b.send(Message::GotoFights);
        assert!(b.gui.home.is_none());
        b.send(Message::TogglePicker);
        assert!(b.gui.picker_open);
        b.send(Message::HomeCharacter(Some(owner.guid.clone())));
        assert!(!b.gui.picker_open);
        assert_eq!(
            b.gui.home.as_ref().and_then(|h| h.scope.clone()),
            Some(owner.guid.clone()),
            "Home, scoped to the pick"
        );
        // The bar's picker stands over Home too.
        {
            let mut ui = simulator(view::view(&b.gui));
            assert!(
                ui.find(view::display_name(&owner.name)).is_ok()
                    || ui.find(owner.name.as_str()).is_ok(),
                "the picker names who played last"
            );
        }
        b.send(Message::TogglePicker);
        b.send(named(Named::Escape));
        assert!(!b.gui.picker_open);
        assert!(b.gui.home.is_some());
    }

    /// The scope chips are Home's scope, remembered as the scope Home opens
    /// on — and they lock nothing: whose window it is, and with it the
    /// chrome and the "you" on a meter, stays with the character played
    /// last.
    #[test]
    fn the_scope_chips_scope_home_and_lock_nothing() {
        let mut b = owned_bridge();
        b.send(chr("~"));
        let owner = b
            .gui
            .home_panels
            .owner
            .clone()
            .expect("the store names the owner");
        assert_eq!(b.gui.owner_guid.as_deref(), Some(owner.guid.as_str()));
        assert!(b.gui.home_panels.night.is_some(), "the owner's night");
        {
            let mut ui = simulator(view::view(&b.gui));
            assert!(ui.find("You, this week").is_ok());
            assert!(ui.find("All characters").is_ok(), "the first chip");
            // A chip is a control: pressing the owner's scopes Home.
            let chip = if b.gui.cfg.hide_realms {
                view::display_name(&owner.name).to_string()
            } else {
                owner.name.clone()
            };
            ui.click(chip.as_str()).expect("the owner's chip");
            let sent: Vec<Message> = ui.into_messages().collect();
            assert!(
                sent.iter().any(|m| matches!(
                    m,
                    Message::HomeCharacter(Some(g)) if *g == owner.guid
                )),
                "{sent:?}"
            );
        }
        // Another player of the store's cards: scoped all the same, and
        // nothing of theirs is "you".
        let other = b
            .gui
            .home
            .as_ref()
            .and_then(|h| h.cards.first())
            .and_then(|c| c.players.iter().find(|p| p.guid != owner.guid))
            .map(|p| p.guid.clone())
            .expect("a second player");
        b.send(Message::HomeCharacter(Some(other.clone())));
        assert_eq!(b.gui.home.as_ref().unwrap().scope, Some(other.clone()));
        assert_eq!(b.gui.cfg.character, Some(other.clone()), "remembered");
        assert_eq!(
            b.gui.owner_guid.as_deref(),
            Some(owner.guid.as_str()),
            "a chip locks nothing"
        );
        assert_eq!(
            b.gui.home_panels.owner.as_ref().map(|o| o.guid.as_str()),
            Some(owner.guid.as_str())
        );
        assert!(
            b.gui
                .home_panels
                .night
                .as_ref()
                .is_some_and(|n| n.who.guid == other),
            "the night is the scope's"
        );
        // Reopening Home lands on the same scope.
        b.send(chr("~"));
        assert!(b.gui.home.is_none());
        b.send(chr("~"));
        assert_eq!(b.gui.home.as_ref().unwrap().scope, Some(other));
        // And back to everyone.
        b.send(Message::HomeCharacter(None));
        assert_eq!(b.gui.home.as_ref().unwrap().scope, None);
        assert_eq!(b.gui.cfg.character, None);
        assert_eq!(
            b.gui
                .home_panels
                .night
                .as_ref()
                .map(|n| n.who.guid.as_str()),
            Some(owner.guid.as_str())
        );
    }
}

/// The pull rail and the top bar over it: what the window asks the store
/// for, where the rail stands at each width, and the keys that walk it.
#[cfg(test)]
mod rail_tests {
    use super::testkit::{
        Bridge, chr, fake_client, gui_over, key, named, simulator_as, test_config,
    };
    use super::*;
    use iced::keyboard::key::Named;
    use iced::keyboard::{Key, Modifiers};
    use wowdps_daemon::mock::MockDaemon;
    use wowdps_proto::{ClientMsg, Cursor, HistoryQuery};

    /// The window as it draws itself `w` wide, in its own fonts.
    fn ui_at(gui: &Gui, w: f32) -> iced_test::Simulator<'_, Message> {
        simulator_as(settings(), iced::Size::new(w, 880.0), view::view(gui))
    }

    /// At launch the rail asks for the store's newest page — every
    /// character's, at most a page the store will serve (`FIGHTS_CAP`) —
    /// and asks for no second while that one is out.
    #[test]
    fn the_rail_asks_the_store_for_its_newest_page_at_launch() {
        let (client, mut peer) = fake_client();
        let mut gui = Gui::for_test(client, ClientState::new(), test_config());
        peer.set_read_timeout(Some(Duration::from_millis(50)))
            .unwrap();
        let mut read = || {
            let mut sent = Vec::new();
            while let Ok((tag, body)) = wowdps_proto::wire::read_frame(&mut peer) {
                sent.push(ClientMsg::decode(tag, &body).unwrap());
            }
            sent
        };
        let sent = read();
        assert!(matches!(sent.first(), Some(ClientMsg::Watch(Cursor::List))));
        let pages: Vec<(Option<String>, u32, Option<String>)> = sent
            .iter()
            .filter_map(|m| match m {
                ClientMsg::GetHistory {
                    query:
                        HistoryQuery::Fights {
                            after_id,
                            limit,
                            guid,
                            ..
                        },
                    ..
                } => Some((after_id.clone(), *limit, guid.clone())),
                _ => None,
            })
            .collect();
        assert_eq!(pages.len(), 1, "{sent:?}");
        let (after, limit, guid) = &pages[0];
        assert_eq!(*after, None, "the newest page");
        assert_eq!(*guid, None, "every character's");
        assert!(*limit as usize <= wowdps_daemon::history::FIGHTS_CAP);
        // "Show older nights" while it is out asks nothing more.
        let _ = update(&mut gui, Message::OlderNights);
        assert!(
            !read()
                .iter()
                .any(|m| matches!(m, ClientMsg::GetHistory { .. })),
            "one request in flight"
        );
    }

    /// At 1180 px and under the rail is a drawer: shut, with the fight
    /// header's list button to open it; open, over a scrim that closes it,
    /// as Esc does (before anything under it) and as a pick does — while a
    /// press on the rail itself, where no row is (its head, a night's
    /// heading, a visit's line, the empty foot of a short list), is the
    /// rail's and never the scrim's. Wider, it stands at the window's left,
    /// and a widening shuts a drawer left open.
    #[test]
    fn the_rail_is_a_drawer_at_1180_and_under() {
        assert!(
            !rail::docked(1180.0) && rail::docked(1181.0),
            "the breakpoint"
        );
        let (mut gui, _peer) = gui_over(testkit::raid(25));
        gui.tonight = Some(rail::night_of(1_000));
        for w in [960.0, 1180.0] {
            let _ = update(&mut gui, Message::WindowWidth(w));
            let mut ui = ui_at(&gui, w);
            assert!(ui.find("Pulls").is_err(), "at {w}: a drawer, shut");
            ui.click(crate::fight_head::rail_button_id())
                .expect("the header's list button");
            let sent: Vec<Message> = ui.into_messages().collect();
            assert!(
                sent.iter().any(|m| matches!(m, Message::OpenRail)),
                "{sent:?}"
            );
        }
        let _ = update(&mut gui, Message::WindowWidth(960.0));
        let _ = update(&mut gui, Message::OpenRail);
        {
            let mut ui = ui_at(&gui, 960.0);
            assert!(ui.find("Pulls").is_ok(), "open");
            let rail = ui.find("Pulls").unwrap().bounds();
            assert!(rail.x < rail::DRAWER_W, "at the left");
            ui.click(rail::scrim_id()).expect("the scrim");
            let sent: Vec<Message> = ui.into_messages().collect();
            assert!(matches!(sent.as_slice(), [Message::CloseRail]), "{sent:?}");
        }
        {
            // The rail is opaque to the scrim under it.
            let r = gui.rail();
            let heading = r.nights[0].label.clone();
            let visit = r.nights[0].visits[0].title.clone();
            let mut ui = ui_at(&gui, 960.0);
            for words in ["Pulls", heading.as_str(), visit.as_str()] {
                ui.click(words).unwrap_or_else(|e| panic!("{words}: {e:?}"));
            }
            ui.point_at(iced::Point::new(rail::DRAWER_W / 2.0, 860.0));
            let _ = ui.simulate(iced_test::simulator::click());
            let sent: Vec<Message> = ui.into_messages().collect();
            assert!(
                !sent.iter().any(|m| matches!(m, Message::CloseRail)),
                "a press on the rail closed it: {sent:?}"
            );
        }
        let _ = update(&mut gui, Message::CloseRail);
        assert!(!gui.rail_open);
        // Esc shuts it first, and nothing under it moves.
        let _ = update(&mut gui, Message::OpenRail);
        let _ = update(&mut gui, named(Named::Escape));
        assert!(!gui.rail_open);
        assert!(gui.home.is_none(), "the drawer alone");
        // A pick shuts it too.
        let _ = update(&mut gui, Message::OpenRail);
        let top = gui.rail().lines().next().map(|l| l.pull.clone()).unwrap();
        let _ = update(&mut gui, Message::Pull(top));
        assert!(!gui.rail_open);
        // Above 1180 it is beside the stage: no button, no scrim.
        let _ = update(&mut gui, Message::OpenRail);
        let _ = update(&mut gui, Message::WindowWidth(1181.0));
        assert!(!gui.rail_open, "the widening shut it");
        let mut ui = ui_at(&gui, 1181.0);
        let rail = ui.find("Pulls").expect("docked").bounds();
        assert!(rail.x < rail::RAIL_W);
        assert!(ui.find(crate::fight_head::rail_button_id()).is_err());
        assert!(ui.find(rail::scrim_id()).is_err());
        // Esc with the rail docked is the stage's: on to Home.
        drop(ui);
        let _ = update(&mut gui, named(Named::Escape));
        assert!(gui.home.is_some());
    }

    /// While the drawer is open over the stage, the keys that would change
    /// what its scrim hides do nothing — a view key, Tab, `v`, `/`, `t`,
    /// `p` — j and k walk the drawer's own rows, not the meter's, and the
    /// ones that walk the rail or leave it still do; once it closes the
    /// stage has its keys back.
    #[test]
    fn the_stage_under_the_drawer_keeps_still() {
        let (mut gui, _peer) = gui_over(testkit::raid(25));
        let _ = update(&mut gui, Message::WindowWidth(960.0));
        let _ = update(&mut gui, Message::OpenRail);
        let before = (gui.state.row_sel, gui.state.view);
        for k in ["j", "k", "d", "h", "v", "/", "t", "p"] {
            let _ = update(&mut gui, chr(k));
        }
        let _ = update(&mut gui, named(Named::Tab));
        assert_eq!((gui.state.row_sel, gui.state.view), before, "nothing moved");
        assert!(!gui.state.inspecting(), "Tab stayed with the drawer");
        assert!(
            !gui.filter_focused && !gui.filter_visible(),
            "no field under the scrim"
        );
        assert!(gui.talents.is_none());
        assert!(gui.state.compare_picks().is_empty());
        assert!(gui.rail_open);
        assert_eq!(gui.surface(), keys::Surface::Rail, "the sheet's surface");
        // `?` still shows the sheet; Esc (twice: the sheet, then the
        // drawer) gives the stage its keys back.
        let _ = update(&mut gui, chr("?"));
        assert!(gui.shortcuts_open);
        let _ = update(&mut gui, named(Named::Escape));
        let _ = update(&mut gui, named(Named::Escape));
        assert!(!gui.rail_open);
        let _ = update(&mut gui, chr("j"));
        assert_eq!(gui.state.row_sel, before.0 + 1);
    }

    /// The drawer is usable from the keys alone: it opens with its
    /// highlight on the pull on the stage; j, k and the arrows walk the
    /// rows it draws (over hidden trash), each kept in sight, and Enter
    /// opens the one it is on and closes the drawer; `[` and `]` step the
    /// stage along the rail with the drawer left open on the row they
    /// landed on, its highlight with them.
    #[test]
    fn the_drawer_has_its_own_keys() {
        let (mut gui, _peer) = gui_over(testkit::raid(25));
        gui.tonight = Some(rail::night_of(1_000));
        for night in 1..=3_i64 {
            gui.earlier.adopt(wowdps_proto::history::FightCard {
                id: format!("before-{night}"),
                kind: wowdps_proto::history::FightKind::Encounter,
                name: "The Lost Explorers".to_string(),
                start_local_ms: 1_000 - night * 86_400_000,
                start_utc_ms: 1_000 - night * 86_400_000,
                duration_ms: 454_000,
                success: Some(false),
                ..Default::default()
            });
        }
        let _ = update(&mut gui, Message::WindowWidth(960.0));
        let order: Vec<Pull> = gui.rail().lines().map(|l| l.pull.clone()).collect();
        assert_eq!(order.len(), 4, "tonight's pull and three stored");
        let _ = update(&mut gui, Message::OpenRail);
        assert_eq!(gui.rail_shown().cursor.as_ref(), Some(&order[0]));
        {
            let mut ui = ui_at(&gui, 960.0);
            assert!(ui.find(rail::cursor_id()).is_ok(), "the highlight is drawn");
        }
        // j and ↓ walk down, k and ↑ back; the stage stays where it is.
        let _ = update(&mut gui, chr("j"));
        assert_eq!(gui.rail_cursor.as_ref(), Some(&order[1]));
        assert!(
            update(&mut gui, named(Named::ArrowDown)).units() > 0,
            "kept in sight"
        );
        assert_eq!(gui.rail_cursor.as_ref(), Some(&order[2]));
        let _ = update(&mut gui, chr("k"));
        assert_eq!(gui.rail_cursor.as_ref(), Some(&order[1]));
        let _ = update(&mut gui, named(Named::ArrowUp));
        let _ = update(&mut gui, named(Named::ArrowUp));
        assert_eq!(gui.rail_cursor.as_ref(), Some(&order[0]), "the top holds");
        assert_eq!(gui.current_pull().as_ref(), Some(&order[0]));
        // `[` steps the stage; the drawer stays open on the row it reached.
        let _ = update(&mut gui, chr("["));
        assert_eq!(gui.current_pull().as_ref(), Some(&order[1]));
        assert!(gui.rail_open, "open across the step");
        assert_eq!(gui.rail_cursor.as_ref(), Some(&order[1]));
        let _ = update(&mut gui, chr("]"));
        assert_eq!(gui.current_pull().as_ref(), Some(&order[0]));
        assert!(gui.rail_open);
        // Enter opens the highlighted pull and closes the drawer.
        let _ = update(&mut gui, chr("j"));
        let _ = update(&mut gui, chr("j"));
        let _ = update(&mut gui, named(Named::Enter));
        assert!(!gui.rail_open);
        assert_eq!(gui.current_pull().as_ref(), Some(&order[2]));
        // The sheet lists the drawer's keys as the ones that work here.
        let here: Vec<&str> = keys::BINDINGS
            .iter()
            .filter(|b| b.applies(keys::Surface::Rail))
            .map(|b| b.keys)
            .collect();
        for k in ["j k", "[ ]", "← →", "enter", "esc", "m", "H", "~", "?"] {
            assert!(here.contains(&k), "{k} on the drawer: {here:?}");
        }
        for k in ["d", "tab", "v", "t", "p", "/"] {
            assert!(!here.contains(&k), "{k} is not the drawer's");
        }
    }

    /// `[` walks down the rail from the log's pulls into the stored nights
    /// — the stage fetching the stored pull and drawing it, on the view the
    /// reader was on — and `]` walks back up onto the log.
    #[test]
    fn the_pull_keys_walk_the_rail_into_the_stored_nights() {
        let mut b = Bridge::new(MockDaemon::fixture().with_history());
        // The store's cards as another log's: the rail lists them under the
        // log's pulls.
        b.foreign_store();
        let order: Vec<Pull> = b.gui.rail().lines().map(|l| l.pull.clone()).collect();
        let at = order
            .windows(3)
            .position(|w| matches!(w, [Pull::Log(_), Pull::Stored(_), Pull::Stored(_)]))
            .expect("the log's pulls, then two of the store's");
        b.send(Message::Pull(order[at].clone()));
        b.send(chr("h"));
        assert_eq!(b.gui.current_pull().as_ref(), Some(&order[at]));
        b.send(chr("["));
        assert_eq!(b.gui.current_pull().as_ref(), Some(&order[at + 1]));
        let s = b.gui.stored.as_ref().expect("a stored pull on the stage");
        assert!(!s.missing, "the store answered");
        assert!(b.gui.fight().segment_name().is_some(), "drawn");
        assert_eq!(b.gui.fight().view, View::Healing, "the reader's view");
        // ← is `[` too, on down the store's pulls.
        b.send(named(Named::ArrowLeft));
        assert_eq!(b.gui.current_pull().as_ref(), Some(&order[at + 2]));
        assert!(b.gui.stored.as_ref().is_some_and(|s| !s.missing));
        b.send(chr("]"));
        assert_eq!(b.gui.current_pull().as_ref(), Some(&order[at + 1]));
        b.send(chr("]"));
        assert_eq!(b.gui.current_pull().as_ref(), Some(&order[at]));
        assert!(b.gui.stored.is_none(), "the log's pull, from the log");
        assert_eq!(b.gui.fight().view, View::Healing);
        // From the rail's top, `]` goes nowhere.
        b.send(Message::Pull(order[0].clone()));
        b.send(chr("]"));
        assert_eq!(b.gui.current_pull().as_ref(), Some(&order[0]));
    }

    /// A stored pull is missing no view the log was on: stepping off the
    /// log's Enemies onto a stored pull shows its Damage, says why a view
    /// key for the enemies does nothing there, and the step back onto the
    /// log is on Enemies again — unless the reader chose a view since.
    #[test]
    fn a_stored_pull_keeps_the_log_s_view_for_the_step_back() {
        let mut b = Bridge::new(MockDaemon::fixture().with_history());
        b.foreign_store();
        let order: Vec<Pull> = b.gui.rail().lines().map(|l| l.pull.clone()).collect();
        let at = order
            .windows(2)
            .position(|w| matches!(w, [Pull::Log(_), Pull::Stored(_)]))
            .expect("the log's pulls, then the store's");
        b.send(Message::Pull(order[at].clone()));
        b.send(Message::PickView(View::EnemyTaken));
        assert_eq!(b.gui.fight().view, View::EnemyTaken);
        b.send(chr("["));
        assert!(b.gui.stored.is_some());
        assert_eq!(b.gui.fight().view, View::Damage, "the store has no enemies");
        // What the store keeps no answer for says so.
        b.send(chr("E"));
        assert_eq!(b.gui.fight().view, View::Damage);
        assert_eq!(
            b.gui.toast.as_ref().map(|(w, _)| w.as_str()),
            Some(view::NOT_STORED)
        );
        b.send(chr("v"));
        assert_eq!(
            b.gui.toast.as_ref().map(|(w, _)| w.as_str()),
            Some(NO_STORED_PAIR)
        );
        {
            // The sheet dims them there.
            let inert = b.gui.inert_keys();
            assert!(inert.contains(&"E") && inert.contains(&"v"), "{inert:?}");
        }
        b.send(chr("]"));
        assert!(b.gui.stored.is_none());
        assert_eq!(b.gui.fight().view, View::EnemyTaken, "the log's view, back");
        // A view chosen on the stored pull is the reader's: it goes back.
        b.send(chr("["));
        b.send(chr("h"));
        b.send(chr("]"));
        assert_eq!(b.gui.fight().view, View::Healing);
    }

    /// `H` opens the rail at the nights before tonight's: the drawer where
    /// the rail is one, scrolled so the first earlier night's heading
    /// stands at the top of the rail's viewport; the store is asked for its
    /// first page when none has landed.
    #[test]
    fn h_opens_the_rail_at_the_earlier_nights() {
        use iced::advanced::widget::Operation;
        use iced::advanced::widget::operation::Outcome;
        use iced_test::runtime::{UserInterface, user_interface};
        let (mut gui, _peer) = gui_over(testkit::raid(25));
        // The raid's pull starts a second into the epoch; tonight is its
        // night, and a dozen stored nights before it are the earlier ones —
        // more than the drawer shows, so there is somewhere to scroll.
        gui.tonight = Some(rail::night_of(1_000));
        for night in 1..=12_i64 {
            for pull in 0..3 {
                gui.earlier.adopt(wowdps_proto::history::FightCard {
                    id: format!("before-{night}-{pull}"),
                    kind: wowdps_proto::history::FightKind::Encounter,
                    name: "The Lost Explorers".to_string(),
                    start_local_ms: 1_000 - night * 86_400_000 + pull * 600_000,
                    start_utc_ms: 1_000 - night * 86_400_000 + pull * 600_000,
                    duration_ms: 454_000,
                    success: Some(false),
                    ..Default::default()
                });
            }
        }
        assert_eq!(gui.rail().earlier(), Some(1));
        let _ = update(&mut gui, chr("H"));
        assert!(gui.rail_open, "the drawer: the width is not known");
        // The operation `H` asks for, run as the runtime runs it: over the
        // window as drawn, each chained step in turn.
        let size = iced::Size::new(960.0, 880.0);
        let mut renderer = testkit::renderer();
        let mut ui = UserInterface::build(
            view::view(&gui),
            size,
            user_interface::Cache::default(),
            &mut renderer,
        );
        let mut op: Box<dyn Operation> = Box::new(rail::ToEarlier::default());
        loop {
            ui.operate(&renderer, op.as_mut());
            match op.finish() {
                Outcome::Chain(next) => op = next,
                _ => break,
            }
        }
        /// Where the rail's list stands, and where the heading is in it.
        #[derive(Default)]
        struct Stand {
            viewport: Option<(iced::Rectangle, iced::Rectangle, iced::Vector)>,
            heading: Option<iced::Rectangle>,
        }
        impl Operation for Stand {
            fn traverse(&mut self, operate: &mut dyn FnMut(&mut dyn Operation)) {
                operate(self);
            }
            fn container(&mut self, id: Option<&iced::widget::Id>, bounds: iced::Rectangle) {
                if id == Some(&rail::earlier_id()) {
                    self.heading = Some(bounds);
                }
            }
            fn scrollable(
                &mut self,
                id: Option<&iced::widget::Id>,
                bounds: iced::Rectangle,
                content: iced::Rectangle,
                translation: iced::Vector,
                _state: &mut dyn iced::advanced::widget::operation::Scrollable,
            ) {
                if id == Some(&rail::scroll_id()) {
                    self.viewport = Some((bounds, content, translation));
                }
            }
        }
        let mut stand = Stand::default();
        ui.operate(&renderer, &mut stand);
        let (bounds, content, scrolled) = stand.viewport.expect("the rail's list");
        let heading = stand.heading.expect("the earlier nights' heading");
        assert!(content.height > bounds.height, "a list to scroll");
        assert!(scrolled.y > 0.0, "it scrolled");
        assert!(
            (heading.y - scrolled.y - bounds.y).abs() < 1.0,
            "the heading at the viewport's top: {heading:?} {scrolled:?} {bounds:?}"
        );
        drop(ui);
        // Beside the stage there is no drawer to open.
        let _ = update(&mut gui, Message::CloseRail);
        let _ = update(&mut gui, Message::WindowWidth(1440.0));
        assert!(update(&mut gui, chr("H")).units() > 0);
        assert!(!gui.rail_open);
    }

    /// `p` pins the pull on the stage — a stored pull's card, or the card
    /// the store wrote for a pull of the log — or lets it go; the store's
    /// answer lands on the card, the rail's row and the header wear the
    /// star, and a pull the store holds no card of says so.
    #[test]
    fn p_pins_the_pull_on_the_stage() {
        let mut b = Bridge::new(MockDaemon::fixture().with_history());
        let kill = b
            .mock
            .history()
            .cards()
            .iter()
            .find(|c| c.name == "The Ashen Warden" && c.success == Some(true))
            .cloned()
            .expect("the fixture's kill is stored");
        assert!(!kill.pinned);
        // The log's own pull: its card, paired as the rail pairs it.
        b.send(Message::OpenStored(kill.id.clone()));
        assert!(b.gui.stored.is_none(), "the log's pull");
        assert_eq!(b.gui.pin_target(), Some((kill.id.clone(), false)));
        let _ = update(&mut b.gui, chr("p"));
        let sent = b.requests();
        assert!(
            sent.iter().any(|m| matches!(m, ClientMsg::PinFight { fight_id, pinned: true, .. } if *fight_id == kill.id)),
            "{sent:?}"
        );
        for req in sent {
            for reply in b.mock.handle(req) {
                b.push(&reply);
            }
        }
        b.settle();
        assert_eq!(b.gui.pin_target(), Some((kill.id.clone(), true)));
        let at = b.gui.current_pull().unwrap();
        assert!(
            b.gui.rail().line(&at).is_some_and(|l| l.pinned),
            "the row's star"
        );
        assert_eq!(b.gui.toast.as_ref().map(|(w, _)| w.as_str()), Some(PINNED));
        {
            let mut ui = ui_at(&b.gui, 1440.0);
            let stars = ui.find(rail::PIN).is_ok();
            assert!(stars, "the star is drawn");
            assert!(crate::fight_head::Head::of(&b.gui, false).pinned);
        }
        // And back: `p` lets it go.
        b.send(chr("p"));
        assert_eq!(b.gui.pin_target(), Some((kill.id.clone(), false)));
        // Another log's cards: the stage's stored pull pins its own card,
        // and a pull of the log the store holds no card of says so.
        b.foreign_store();
        b.send(Message::OpenStored(kill.id.clone()));
        assert!(b.gui.stored.is_some());
        b.send(chr("p"));
        assert!(
            b.gui
                .stored
                .as_ref()
                .and_then(|s| s.card.as_ref())
                .is_some_and(|c| c.pinned),
            "the stored pull's card"
        );
        b.send(chr("m"));
        assert!(b.gui.stored.is_none());
        assert_eq!(b.gui.pin_target(), None, "no card pairs with this log");
        let _ = update(&mut b.gui, chr("p"));
        assert!(
            !b.requests()
                .iter()
                .any(|m| matches!(m, ClientMsg::PinFight { .. }))
        );
        assert_eq!(b.gui.toast.as_ref().map(|(w, _)| w.as_str()), Some(NO_CARD));
        assert!(b.gui.inert_keys().contains(&"p"));
    }

    /// The daemon restarts (a rebuild bounces the dev unit): the requests in
    /// flight on the old connection will never be answered. The rail's page
    /// and the stored pull's `GetFight` are asked for again on the new one,
    /// where each would otherwise wait on its one request for good.
    #[test]
    fn a_reconnect_asks_again_for_what_the_old_connection_lost() {
        let mut b = Bridge::new(MockDaemon::fixture().with_history());
        b.foreign_store();
        let id = b.mock.history().cards()[0].id.clone();
        b.send(Message::OpenStored(id.clone()));
        assert!(b.gui.stored.as_ref().is_some_and(|s| !s.missing));
        // A page and a fight go out, and the connection dies under them.
        let mut out = Vec::new();
        b.gui.earlier.want_newest();
        b.gui.ask_earlier(&mut out);
        assert!(
            out.iter()
                .any(|m| matches!(m, ClientMsg::GetHistory { .. })),
            "{out:?}"
        );
        let _ = update(&mut b.gui, chr("j"));
        assert!(
            b.requests()
                .iter()
                .any(|m| matches!(m, ClientMsg::GetFight { .. })),
            "the stored pull asked"
        );
        // Unanswered, they hold their places: nothing more goes out.
        assert!(b.gui.earlier.asking());
        let mut quiet = Vec::new();
        b.gui.earlier.want_newest();
        b.gui.ask_earlier(&mut quiet);
        assert!(quiet.is_empty(), "one in flight, and it never answers");
        // The new connection asks again for both.
        let mut again = Vec::new();
        b.gui.reconnected(&mut again);
        assert!(
            again.iter().any(|m| matches!(
                m,
                ClientMsg::GetHistory {
                    query: HistoryQuery::Fights { after_id: None, .. },
                    ..
                }
            )),
            "{again:?}"
        );
        let drill = b.gui.fight().drill.as_ref().map(|d| d.key.clone());
        assert!(
            again.iter().any(|m| matches!(
                m,
                ClientMsg::GetFight { fight_id, drill: d, .. } if *fight_id == id && *d == drill
            )),
            "{again:?}"
        );
    }

    /// The talent viewer opens on a stored pull's row from what the store's
    /// answer carried — no `GetLoadout` to the daemon, whose log has no
    /// such pull — and adopts the logged build when the answer had one.
    #[test]
    fn t_on_a_stored_pull_opens_the_viewer_from_the_store_s_answer() {
        let mut b = Bridge::new(MockDaemon::fixture().with_history());
        b.foreign_store();
        let kill = b
            .mock
            .history()
            .cards()
            .iter()
            .find(|c| c.name == "The Ashen Warden" && c.success == Some(true))
            .cloned()
            .expect("the fixture's kill is stored");
        b.send(Message::OpenStored(kill.id.clone()));
        let key = b
            .gui
            .fight()
            .drill
            .as_ref()
            .map(|d| d.key.clone())
            .expect("the selection's drill");
        let _ = update(&mut b.gui, chr("t"));
        assert!(
            !b.requests()
                .iter()
                .any(|m| matches!(m, ClientMsg::GetLoadout { .. })),
            "the store answered already"
        );
        let ui = b.gui.talents.as_ref().expect("the viewer is open");
        assert_eq!(b.gui.pending_loadout(), None);
        if b.gui
            .stored
            .as_ref()
            .and_then(|s| s.loadout_of(&key))
            .is_some()
        {
            // Adopted: the logged build, or — on a machine without the
            // talent dataset — the viewer saying it has none to lay it on.
            assert!(ui.logged || ui.error.is_some());
        }
    }

    /// The top bar: the wordmark, the places (Fights lit on a pull), the
    /// jump box with its key — which opens the `?` sheet until the palette
    /// is here, as Ctrl K does — the live pill, the gear and help; and at
    /// 820 px and under, the box a glyph and the wordmark gone.
    #[test]
    fn the_top_bar_carries_the_places_the_jump_box_and_the_live_pill() {
        let (state, _mock) = testkit::live();
        let live = state.segment_name().expect("a live pull");
        let (mut gui, _peer) = gui_over(state);
        {
            let mut ui = ui_at(&gui, 1440.0);
            for words in [
                "wowdps",
                "Home",
                "Fights",
                crate::top_bar::JUMP_WORDS,
                "Ctrl K",
            ] {
                assert!(ui.find(words).is_ok(), "{words}");
            }
            assert!(ui.find(format!("Live, {live}").as_str()).is_ok());
            assert!(ui.find("History").is_err(), "no History place");
            ui.click(crate::top_bar::jump_id()).unwrap();
            ui.click(crate::top_bar::live_id()).unwrap();
            ui.click("Home").unwrap();
            let sent: Vec<Message> = ui.into_messages().collect();
            assert!(
                matches!(
                    sent.as_slice(),
                    [Message::Jump, Message::GotoLive, Message::GotoHome]
                ),
                "{sent:?}"
            );
        }
        {
            let mut ui = ui_at(&gui, 460.0);
            assert!(ui.find("wowdps").is_err(), "no wordmark narrow");
            assert!(ui.find(crate::top_bar::JUMP_WORDS).is_err());
            assert!(ui.find(crate::top_bar::jump_id()).is_ok(), "the glyph");
            assert!(ui.find(format!("Live, {live}").as_str()).is_err());
        }
        let _ = update(&mut gui, key(Key::Character("k".into()), Modifiers::CTRL));
        assert!(gui.palette.is_some(), "Ctrl K");
    }

    /// The jump box stands centred in the room between the places and the
    /// live pill (`.jump{margin-inline:auto}`), as wide as the prototype's
    /// (382 px edge to edge: its 360 px of content, its padding and its
    /// border) — the places are as wide as they are, not a stretch that
    /// takes half the room.
    #[test]
    fn the_jump_box_stands_centred_between_the_places_and_the_pill() {
        let (state, _mock) = testkit::live();
        let (gui, _peer) = gui_over(state);
        for w in [1440.0, 1180.0, 960.0] {
            let mut ui = ui_at(&gui, w);
            let fights = ui.find("Fights").unwrap().bounds();
            let pill = ui.find(crate::top_bar::live_id()).unwrap().bounds();
            let jump = ui.find(crate::top_bar::jump_id()).unwrap().bounds();
            assert!(
                (jump.width - 382.0).abs() < 0.5,
                "at {w}: the box whole, {jump:?}"
            );
            // The places end their tab's padding after "Fights"; the room is
            // what lies between that and the pill, the bar's gaps aside.
            let room = (fights.x + fights.width + 11.0, pill.x);
            let left = jump.x - room.0;
            let right = room.1 - (jump.x + jump.width);
            assert!(
                (left - right).abs() < 2.0,
                "at {w}: {left} to its left, {right} to its right"
            );
        }
    }

    /// Squeezed between the places and what follows, the jump box gives
    /// way in its placeholder, never its key: at every width the box is
    /// drawn, "Ctrl K" keeps the width it has at 1440 and stays inside the
    /// frame (a squeezed cap drew the words through its own frame and past
    /// the box's).
    #[test]
    fn the_jump_box_keeps_its_key_whole_when_squeezed() {
        let (gui, _peer) = crowded_bar();
        let (key, words) = (crate::top_bar::JUMP_KEY, crate::top_bar::JUMP_WORDS);
        let (whole, said) = {
            let mut ui = ui_at(&gui, 1440.0);
            let cap = ui.find(key).unwrap().bounds().width;
            (cap, ui.find(words).unwrap().bounds().width)
        };
        let mut squeezed = false;
        let mut w = theme::NARROW_WINDOW + 1.0;
        while w <= 1440.0 {
            let mut ui = ui_at(&gui, w);
            let frame = ui.find(crate::top_bar::jump_id()).unwrap().bounds();
            let cap = ui.find(key).unwrap().bounds();
            squeezed |= ui.find(words).unwrap().bounds().width < said - 0.5;
            assert!(
                (cap.width - whole).abs() < 0.5,
                "at {w}: {cap:?}, whole {whole}"
            );
            assert!(
                cap.x + cap.width <= frame.x + frame.width,
                "at {w}: {cap:?} in {frame:?}"
            );
            w += 40.0;
        }
        assert!(squeezed, "some width cuts the placeholder short");
        // And nothing is pushed off the bar's end.
        let mut ui = ui_at(&gui, theme::NARROW_WINDOW + 1.0);
        let help = ui.find(nav_help()).unwrap().bounds();
        assert!(help.x + help.width <= theme::NARROW_WINDOW + 1.0);
    }

    fn nav_help() -> iced::widget::Id {
        crate::nav::help_id()
    }

    /// The bar at its most crowded: a long name on the live pill and a long
    /// one on the picker, realms shown — what caps both names.
    fn crowded_bar() -> (Gui, std::os::unix::net::UnixStream) {
        let (mut state, _mock) = testkit::live();
        let mut entries = state.entries().to_vec();
        if let Some(last) = entries.last_mut() {
            last.row.name = LONG_PULL.to_string();
            last.row.kind = wowdps_model::SegmentKind::Encounter;
        }
        let _ = state.on_msg(DaemonMsg::SegmentList {
            seq: 900,
            entries,
            source: state.source.clone(),
            active: true,
            log_id: None,
        });
        let _ = state.pin_live();
        let (mut gui, peer) = gui_over(state);
        gui.cfg.hide_realms = false;
        gui.known_characters = vec![home::CharLine {
            guid: "Player-1-1".to_string(),
            name: "Tranqlockhasalongname-Proudmoore-US".to_string(),
            class: Some(wowdps_model::Class::Warlock),
            fights: 3,
            ..Default::default()
        }];
        (gui, peer)
    }

    const LONG_PULL: &str = "Priory of the Sacred Flame, the long way round +14";

    /// A long name on the live pill ends in "…" before it squeezes the jump
    /// box, the clock after it whole; its tip says what it goes to.
    #[test]
    fn the_live_pill_caps_its_name_and_keeps_its_clock() {
        let (gui, _peer) = crowded_bar();
        let pill = crate::top_bar::Bar::of(&gui).pill.expect("a pill");
        let mut ui = ui_at(&gui, 1440.0);
        let face = ui.find(crate::top_bar::live_id()).unwrap().bounds();
        assert!(face.width < 320.0, "capped: {face:?}");
        let clock = wowdps_model::fmt::duration(pill.ms);
        assert!(ui.find(clock.as_str()).is_ok(), "the clock, whole");
        assert_eq!(crate::top_bar::LIVE_TIP, "Go to the live pull (m)");
    }

    /// `q` quits the WINDOW whichever pull is on the stage: a stored pull's
    /// own state is not where the window reads its quit from.
    #[test]
    fn q_quits_from_a_stored_pull() {
        let mut b = Bridge::new(MockDaemon::fixture().with_history());
        b.foreign_store();
        let id = b.mock.history().cards()[0].id.clone();
        b.send(Message::OpenStored(id));
        assert!(b.gui.stored.is_some(), "a stored pull on the stage");
        let _ = update(&mut b.gui, chr("q"));
        assert!(b.gui.state.quit, "q");
        b.gui.state.quit = false;
        let _ = update(&mut b.gui, key(Key::Character("c".into()), Modifiers::CTRL));
        assert!(b.gui.state.quit, "Ctrl C");
    }

    /// The jump box and Ctrl K open the command palette, and what the
    /// reader types there is a search: typing a name neither quits at its
    /// `q` nor pins at its `p` nor switches a view — a key the field let
    /// through is typed into it all the same. Esc and Ctrl K again close
    /// it; the `?` sheet is still closed by any key.
    #[test]
    fn typing_into_the_palette_runs_no_keys() {
        let mut b = Bridge::new(MockDaemon::fixture().with_history());
        b.open(b.gui.state.entries().len() - 2);
        let view = b.gui.fight().view;
        {
            let mut ui = ui_at(&b.gui, 1440.0);
            ui.click(crate::top_bar::jump_id()).unwrap();
            let sent: Vec<Message> = ui.into_messages().collect();
            assert!(matches!(sent.as_slice(), [Message::Jump]), "{sent:?}");
        }
        let _ = update(&mut b.gui, Message::Jump);
        assert!(b.gui.palette.is_some() && !b.gui.shortcuts_open);
        let _ = b.requests();
        for c in "Tranqlock pdh".chars() {
            let _ = update(&mut b.gui, chr(&c.to_string()));
        }
        assert_eq!(
            b.gui.palette.as_ref().map(|p| p.query.as_str()),
            Some("Tranqlock pdh"),
            "typed into the search"
        );
        assert!(!b.gui.state.quit, "no quit at the q");
        assert!(
            !b.requests()
                .iter()
                .any(|m| matches!(m, ClientMsg::PinFight { .. })),
            "nothing pinned"
        );
        assert_eq!(b.gui.fight().view, view, "no view switched");
        assert!(b.gui.talents.is_none(), "no talents at the t");
        let _ = update(&mut b.gui, named(Named::Backspace));
        assert_eq!(
            b.gui.palette.as_ref().map(|p| p.query.as_str()),
            Some("Tranqlock pd")
        );
        let _ = update(&mut b.gui, named(Named::Escape));
        assert!(b.gui.palette.is_none());
        // Ctrl K opens it the same way — over the sheet, which it replaces
        // — and closes it.
        let ctrl_k = || key(Key::Character("k".into()), Modifiers::CTRL);
        let _ = update(&mut b.gui, chr("?"));
        assert!(b.gui.shortcuts_open);
        let _ = update(&mut b.gui, ctrl_k());
        assert!(b.gui.palette.is_some() && !b.gui.shortcuts_open);
        let _ = update(&mut b.gui, chr("q"));
        assert!(!b.gui.state.quit);
        let _ = update(&mut b.gui, ctrl_k());
        assert!(b.gui.palette.is_none());
        // Esc heard although the focused field captured it closes it too:
        // the listener turns a CAPTURED Escape, and only that, into the
        // close…
        let esc = iced_test::simulator::press_key(Key::Named(Named::Escape), None);
        let window = iced::window::Id::unique();
        assert!(matches!(
            palette_escape(esc.clone(), iced::event::Status::Captured, window),
            Some(Message::PaletteClose)
        ));
        assert!(
            palette_escape(esc, iced::event::Status::Ignored, window).is_none(),
            "an Escape nobody took reaches the keymap, which closes it"
        );
        let other = iced_test::simulator::press_key(Key::Character("q".into()), None);
        assert!(palette_escape(other, iced::event::Status::Captured, window).is_none());
        // …which closes it.
        let _ = update(&mut b.gui, Message::Jump);
        let _ = update(&mut b.gui, Message::PaletteClose);
        assert!(b.gui.palette.is_none());
        // The `?` sheet is still closed by any key.
        let _ = update(&mut b.gui, chr("?"));
        assert!(b.gui.shortcuts_open);
        let _ = update(&mut b.gui, chr("x"));
        assert!(!b.gui.shortcuts_open);
    }

    /// The window keeps one `GetFight` in the daemon's queue however fast
    /// the rail is walked: three `[` over stored pulls with no answer send
    /// one, and the last pull's goes out when that one answers.
    #[test]
    fn walking_stored_pulls_keeps_one_read_in_flight() {
        let (mut gui, mut peer) = gui_over(testkit::raid(25));
        peer.set_read_timeout(Some(Duration::from_millis(30)))
            .unwrap();
        let mut theirs = peer.try_clone().unwrap();
        gui.tonight = Some(rail::night_of(1_000));
        for night in 1..=3_i64 {
            gui.earlier.adopt(wowdps_proto::history::FightCard {
                id: format!("before-{night}"),
                kind: wowdps_proto::history::FightKind::Encounter,
                name: "The Lost Explorers".to_string(),
                start_local_ms: 1_000 - night * 86_400_000,
                start_utc_ms: 1_000 - night * 86_400_000,
                duration_ms: 454_000,
                success: Some(false),
                ..Default::default()
            });
        }
        let mut read = || {
            let mut sent = Vec::new();
            while let Ok((tag, body)) = wowdps_proto::wire::read_frame(&mut peer) {
                sent.push(ClientMsg::decode(tag, &body).unwrap());
            }
            sent
        };
        let _ = read();
        for _ in 0..3 {
            let _ = update(&mut gui, chr("["));
        }
        assert_eq!(
            gui.current_pull(),
            Some(Pull::Stored("before-3".to_string()))
        );
        let fights = |sent: &[ClientMsg]| -> Vec<(u32, String)> {
            sent.iter()
                .filter_map(|m| match m {
                    ClientMsg::GetFight {
                        req_id, fight_id, ..
                    } => Some((*req_id, fight_id.clone())),
                    _ => None,
                })
                .collect()
        };
        let sent = fights(&read());
        assert_eq!(sent.len(), 1, "one read out: {sent:?}");
        assert_eq!(sent[0].1, "before-1");
        // The first answers — empty, for a pull the reader has left: the
        // last one's read goes out now, and only it.
        let answer = DaemonMsg::Fight {
            req_id: sent[0].0,
            fight: None,
        };
        let mut next = Vec::new();
        {
            use std::io::Write as _;
            theirs.write_all(&answer.encode()).unwrap();
        }
        for _ in 0..20 {
            std::thread::sleep(Duration::from_millis(10));
            let _ = update(&mut gui, Message::Tick);
            next.extend(fights(&read()));
            if !next.is_empty() {
                break;
            }
        }
        assert_eq!(
            next.iter().map(|(_, f)| f.as_str()).collect::<Vec<_>>(),
            ["before-3"],
            "the pull on the stage asks"
        );
        assert!(
            gui.stored.as_ref().is_some_and(|s| !s.missing),
            "another pull's empty answer is not this one's"
        );
    }

    /// A fight the store wrote re-asks the rail's newest few cards, merged
    /// over what is in hand — not a whole page for every closed pull.
    #[test]
    fn a_store_write_asks_for_the_newest_few_cards() {
        let mut b = Bridge::new(MockDaemon::fixture().with_history());
        assert!(b.gui.earlier.answered);
        let _ = b.requests();
        b.push(&DaemonMsg::HistoryChanged {
            fight_id: "x".to_string(),
        });
        let mut asked = Vec::new();
        for _ in 0..20 {
            std::thread::sleep(Duration::from_millis(10));
            let _ = update(&mut b.gui, Message::Tick);
            asked.extend(b.requests());
            if !asked.is_empty() {
                break;
            }
        }
        assert!(
            asked.iter().any(|m| matches!(
                m,
                ClientMsg::GetHistory {
                    query: HistoryQuery::Fights {
                        limit: history::FRESH,
                        after_id: None,
                        ..
                    },
                    ..
                }
            )),
            "{asked:?}"
        );
    }

    /// A stored pull of an earlier night says which night in the header's
    /// meta — the one locator left once the drawer is shut — and gives way
    /// in a narrow window; and its "you" is whichever of the owner's
    /// characters the card says played it, before the window's lock.
    #[test]
    fn a_stored_pull_names_its_night_and_whose_it_was() {
        let mut b = Bridge::new(MockDaemon::fixture().with_history());
        b.foreign_store();
        let kill = b
            .mock
            .history()
            .cards()
            .iter()
            .find(|c| c.name == "The Ashen Warden" && c.success == Some(true))
            .cloned()
            .expect("the fixture's kill is stored");
        let day = rail::night_of(kill.start_local_ms);
        b.gui.tonight = Some(day + 3);
        b.send(Message::OpenStored(kill.id.clone()));
        assert!(b.gui.stored.as_ref().is_some_and(|s| !s.missing));
        let night = rail::night_short(day, day + 3);
        assert_eq!(
            crate::fight_head::Head::of(&b.gui, false).night.as_deref(),
            Some(night.as_str())
        );
        assert!(ui_at(&b.gui, 1440.0).find(night.as_str()).is_ok());
        assert!(
            ui_at(&b.gui, 460.0).find(night.as_str()).is_err(),
            "narrow, it gives way"
        );
        // Tonight's own stored pull names no night.
        b.gui.tonight = Some(day);
        assert_eq!(crate::fight_head::Head::of(&b.gui, false).night, None);
        // Whose: the row the store marked (v35), over the window's lock.
        let rows = b.gui.fight().rows();
        assert!(rows.len() >= 2, "the kill has players");
        b.gui.owner_guid = Some(rows[0].key.clone());
        assert_eq!(b.gui.owner_row(), Some(0), "nothing marked: the lock");
        let mut marked = rows.clone();
        marked[1].mine = true;
        assert_eq!(b.gui.owner_of(&marked), Some(1), "the store's mark");
        // Back on the log, the lock decides again.
        b.send(chr("m"));
        let rows = b.gui.fight().rows();
        let locked = rows
            .iter()
            .position(|r| Some(&r.key) == b.gui.owner_guid.as_ref());
        assert_eq!(b.gui.owner_row(), locked);
    }

    /// A pull of the log whose card the rail's pages hold wears the card's
    /// best health on its badge, as its row on the rail does.
    #[test]
    fn a_pull_of_the_log_wears_its_card_s_best_health() {
        let mut b = Bridge::new(MockDaemon::fixture().with_history());
        let wipe = b
            .gui
            .state
            .entries()
            .iter()
            .rposition(|e| {
                e.row.kind == wowdps_model::SegmentKind::Encounter && e.row.success == Some(false)
            })
            .expect("the fixture has a wipe");
        b.open(wipe);
        let (id, _) = b.gui.pin_target().expect("the store wrote a card for it");
        if let Some(c) = b.gui.earlier.cards.iter_mut().find(|c| c.id == id) {
            c.best_pct = Some(42);
        }
        let head = crate::fight_head::Head::of(&b.gui, false);
        assert_eq!(head.badge.map(|b| b.word), Some("Wipe at 42%".to_string()));
    }

    /// On Home the rail lights no row, so `[` and `]` start from its top —
    /// the newest pull — never from a step off the hidden stage's pull.
    #[test]
    fn the_pull_keys_on_home_start_at_the_rail_s_top() {
        let mut b = Bridge::new(MockDaemon::fixture().with_history());
        b.foreign_store();
        let order: Vec<Pull> = b.gui.rail().lines().map(|l| l.pull.clone()).collect();
        assert!(order.len() >= 3, "{order:?}");
        for step in ["[", "]"] {
            b.send(Message::Pull(order[2].clone()));
            b.send(Message::GotoHome);
            assert!(b.gui.rail_shown().at.is_none(), "no row lit under Home");
            b.send(chr(step));
            assert!(b.gui.home.is_none(), "{step} leaves Home");
            assert_eq!(b.gui.current_pull().as_ref(), Some(&order[0]), "{step}");
        }
    }
}
