//! The regular-window frontend.
//!
//! The runtime shape mirrors `wowdps-tui`: a 100 ms tick drains the daemon
//! client's inbox (stale snapshots were already coalesced away) and feeds the
//! shared [`ClientState`]; this module only translates iced events into
//! `Action`s and draws.

use std::time::{Duration, Instant};

use iced::{Subscription, Task, Theme, keyboard, time, window};

use wowdps_model::Action;
use wowdps_proto::{ClientKind, ClientState, DaemonClient, DaemonMsg, Reconnect};

use crate::config::Config;
use crate::history;
use crate::home;
use crate::keys;
use crate::talents;
use crate::theme;
use crate::view;

/// Redraw/drain cadence. Live durations tick at this rate.
pub(crate) const TICK: Duration = Duration::from_millis(100);

/// Least time between two `GetStatus` asks off the store-changed path.
const STATUS_REFRESH: Duration = Duration::from_secs(5);

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
    /// R12/v12: the comparison marker label under the cursor, if any.
    pub(crate) compare_hover: Option<String>,
    /// The graph curve value under the cursor, for the legend's readout.
    pub(crate) graph_probe: Option<usize>,
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
    /// R21: the Taken drill shows its stack matrix instead of the panes.
    pub(crate) stacks_open: bool,
    /// The History screen when open — window-local like Home and the
    /// talent viewer; it sits above Home in the stack.
    pub(crate) history: Option<history::History>,
    /// The owner's guid as Home last resolved it — held after Home closes,
    /// so History can put the owner's own number beside each pull.
    pub(crate) owner_guid: Option<String>,
    /// Every character the store has shown the window you play, from any
    /// Home or History answer — so a History scoped to one dungeon still
    /// offers the characters the unscoped list knew.
    pub(crate) known_characters: Vec<home::CharLine>,
    /// The character picker's menu is up (over Home's title or the tab
    /// strip). Window-local like the sheet; Esc or a press away closes it.
    pub(crate) picker_open: bool,
    /// The menu row the pointer is over — drawn, never sent anywhere.
    pub(crate) picker_hover: Option<usize>,
    /// What the fight header knows of the watched fight from views other
    /// than the one on screen: the owner's presence in it.
    pub(crate) seen: crate::fight_head::Seen,
}

/// Where a window-side `Up`/`Down` lands when the drawn order is not the
/// state machine's: on a meter row, or on a row of the drill's spell pane.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Step {
    Meter(usize),
    Spell(usize),
}

/// One positional step through `order` from the row `sel` — the next row
/// down the screen, whatever its rank. A selection that is not drawn
/// (hidden by the filter) lands on the first drawn row.
fn step_in(order: &[usize], sel: usize, action: Action) -> Option<usize> {
    let first = *order.first()?;
    let last = *order.last()?;
    Some(match order.iter().position(|&i| i == sel) {
        Some(p) if action == Action::Down => order.get(p + 1).copied().unwrap_or(last),
        Some(p) => order.get(p.wrapping_sub(1)).copied().unwrap_or(first),
        None => first,
    })
}

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

/// The owner's row among `rows` (our side's, never an enemy's), by the
/// most certain thing that names it, over every row before a less certain
/// one is asked: the locked character's `guid`, then one of `names` whole
/// ("Name-Realm", case aside), then a bare name by its name half — and a
/// bare name only when it names ONE row, since a namesake from another
/// realm who out-ranks the owner would otherwise wear their tag, their
/// chip and their chrome.
pub(crate) fn owner_among(
    rows: &[wowdps_model::Row],
    guid: Option<&str>,
    names: &[String],
) -> Option<usize> {
    let ours = || rows.iter().enumerate().filter(|(_, r)| !r.enemy);
    if let Some(guid) = guid
        && let Some((i, _)) = ours().find(|(_, r)| r.key == guid)
    {
        return Some(i);
    }
    if let Some((i, _)) = ours().find(|(_, r)| {
        names
            .iter()
            .any(|n| r.label.to_lowercase() == n.to_lowercase())
    }) {
        return Some(i);
    }
    let mut bare = ours().filter(|(_, r)| {
        names
            .iter()
            .any(|n| crate::fight_head::is_named(&r.label, n))
    });
    match (bare.next(), bare.next()) {
        (Some((i, _)), None) => Some(i),
        _ => None,
    }
}

impl Gui {
    fn new(mut client: DaemonClient, cfg: Config) -> Self {
        let state = ClientState::new();
        client.send(&state.initial_request());
        // Home renders three different empty screens (off / cold / degraded)
        // and only `Status` tells them apart, so ask once at startup rather
        // than when Home opens: the answer is tiny and always wanted.
        client.send(&wowdps_proto::ClientMsg::GetStatus { req_id: 0 });
        let season = home::Season::from_config(&cfg);
        let locked = cfg.character.clone();
        // The first frame's chrome, from the config alone: gold, or the
        // class remembered for the locked character.
        let owner_class = cfg.character_class().map(|c| (c, None));
        let accent = chrome_accent(cfg.chrome(), owner_class);
        Self {
            state,
            compare_hover: None,
            graph_probe: None,
            client,
            last_snapshot_at: None,
            cfg,
            options_open: false,
            talents: None,
            pending_loadout: None,
            next_req_id: 1,
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
            stacks_open: false,
            history: None,
            // The remembered pick, so a launch is locked to the last
            // selected character before Home ever answers.
            owner_guid: locked,
            known_characters: Vec::new(),
            picker_open: false,
            picker_hover: None,
            seen: crate::fight_head::Seen::default(),
        }
    }

    /// Which surface is showing — what the `?` sheet keys its "here" column
    /// on. Window-local screens sit over the state machine's, so they win.
    pub(crate) fn surface(&self) -> keys::Surface {
        use wowdps_model::Screen;
        if self.talents.is_some() {
            keys::Surface::Talents
        } else if self.history.is_some() {
            keys::Surface::History
        } else if self.home.is_some() {
            keys::Surface::Home
        } else {
            match self.state.screen {
                Screen::List => keys::Surface::List,
                Screen::Compare => keys::Surface::Compare,
                Screen::Meter if self.state.drill_spell().is_some() => keys::Surface::Ability,
                Screen::Meter if self.state.drill.is_some() => keys::Surface::Drill,
                Screen::Meter => keys::Surface::Meter,
            }
        }
    }

    /// Whose window this is, once known — the name the accent was resolved
    /// from ("Name-Realm", as a row label spells it).
    pub(crate) fn owner_name(&self) -> Option<&str> {
        self.accent_owner.as_deref()
    }

    /// The owner's row among `rows` — the chart on screen, which the caller
    /// already holds — by what the config calls them ([`owner_among`]): the
    /// locked character's guid (`character`), then a `history_characters`
    /// name or the name the accent resolved. `None` on the Enemies view,
    /// whose rows are the enemies. (The daemon will flag the row itself one
    /// day, `Row.mine`; until then the names decide.)
    pub(crate) fn owner_in(&self, rows: &[wowdps_model::Row]) -> Option<usize> {
        if self.state.view == wowdps_model::View::EnemyTaken {
            return None;
        }
        let mut names = self.cfg.history_characters();
        names.extend(self.owner_name().map(str::to_string));
        owner_among(rows, self.owner_guid.as_deref(), &names)
    }

    /// [`Gui::owner_in`] over the chart as it stands.
    #[cfg(test)]
    pub(crate) fn owner_row(&self) -> Option<usize> {
        self.owner_in(&self.state.rows())
    }

    /// The meter's sort as the meter draws it: the chosen column while the
    /// view's table has it, and the daemon's order on a view that does not
    /// — Healing's overheal share is no order for Damage, nor a rate for
    /// Interrupts, and a sort by a figure off screen would jumble the
    /// ranks with no heading to say why. The choice is kept, so a view
    /// that has the column again sorts by it again.
    pub(crate) fn meter_sort(&self) -> Option<(crate::table::Col, bool)> {
        self.sort
            .filter(|(c, _)| crate::table::meter_set(self.state.view, false).contains(c))
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

    /// Is the row filter actually on screen? Only the meter draws it, and
    /// only when nothing window-local covers the meter. Focusing a field
    /// that is not in the widget tree would swallow every key with nothing
    /// to type into — a window that looks keyboard-dead — so `/` and the
    /// swallow branch both ask this first.
    pub(crate) fn filter_visible(&self) -> bool {
        self.talents.is_none()
            && self.home.is_none()
            && !self.shortcuts_open
            && self.state.screen == wowdps_model::Screen::Meter
            // The drill's panes are abilities and targets, not players: the
            // filter has nothing to narrow there, so it is not drawn there.
            && self.state.drill.is_none()
    }

    /// Where `Up`/`Down` land while a filter narrows the meter: the next
    /// row that is actually DRAWN, or `None` when the question does not
    /// apply (another action, another screen, a drill, no filter) and the
    /// state machine's own clamped step is right.
    fn filtered_step(&self, action: Action) -> Option<Step> {
        if !matches!(action, Action::Up | Action::Down)
            || self.state.screen != wowdps_model::Screen::Meter
        {
            return None;
        }
        // A sorted by-spell pane: the step is positional in the drawn
        // order, and lands on the pane's own selection.
        if let Some(d) = self.state.drill.as_ref() {
            if d.spell.is_some() || d.pane != wowdps_model::Pane::Spell || self.drill_sort.is_none()
            {
                return None;
            }
            let (by_spell, _) = self.state.breakdown();
            let order: Vec<usize> =
                crate::table::sorted(by_spell.into_iter().enumerate().collect(), self.drill_sort)
                    .into_iter()
                    .map(|(i, _)| i)
                    .collect();
            return step_in(&order, d.spell_sel, action).map(Step::Spell);
        }
        let sort = self.meter_sort();
        if self.filter.trim().is_empty() && sort.is_none() {
            return None;
        }
        // The DRAWN order: filtered, then sorted. Under a sort the step is
        // positional — the next row down the screen, whatever its rank.
        let visible: Vec<usize> = crate::view::ordered(self.state.rows(), &self.filter, sort)
            .into_iter()
            .map(|(i, _)| i)
            .collect();
        let sel = self.state.row_sel;
        if sort.is_some() {
            return step_in(&visible, sel, action).map(Step::Meter);
        }
        let (first, last) = (*visible.first()?, *visible.last()?);
        Some(Step::Meter(match action {
            // From a hidden row (the filter was typed after the selection
            // moved) the step lands on the nearest visible one either way.
            Action::Down => visible.iter().copied().find(|&i| i > sel).unwrap_or(last),
            _ => visible
                .iter()
                .copied()
                .rev()
                .find(|&i| i < sel)
                .unwrap_or(first),
        }))
    }

    /// v28: on a Deaths drill, ← and → step the death windows. The index
    /// to ask for, or `None` when the key means something else here.
    fn death_step(&self, key: &keyboard::Key) -> Option<u32> {
        if self.state.view != wowdps_model::View::Deaths || self.state.drill.is_none() {
            return None;
        }
        let (deaths, shown) = self.state.deaths();
        if deaths.is_empty() {
            return None;
        }
        let last = deaths.len() as u32 - 1;
        let at = shown.unwrap_or(last);
        match key {
            keyboard::Key::Named(keyboard::key::Named::ArrowLeft) => Some(at.saturating_sub(1)),
            keyboard::Key::Named(keyboard::key::Named::ArrowRight) => Some((at + 1).min(last)),
            _ => None,
        }
    }

    /// The History screen's own keys, while it is up: Esc walks one level
    /// up (drill → stored fight → list → closed), Enter opens, j/k move,
    /// `p` pins, and the view keys switch a stored fight's view. `true`
    /// when the key was History's — the meter keymap is not consulted.
    fn history_key(
        &mut self,
        key: &keyboard::Key,
        modifiers: keyboard::Modifiers,
        requests: &mut Vec<wowdps_proto::ClientMsg>,
    ) -> bool {
        let Some(h) = self.history.as_mut() else {
            return false;
        };
        if *key == keyboard::Key::Named(keyboard::key::Named::Escape) {
            if !h.back() {
                self.history = None;
            }
            return true;
        }
        if *key == keyboard::Key::Character("p".into()) {
            if h.stored.is_none()
                && let Some(c) = h.cards.get(h.sel)
            {
                let req_id = self.next_req_id;
                self.next_req_id = self.next_req_id.wrapping_add(1);
                requests.push(wowdps_proto::ClientMsg::PinFight {
                    req_id,
                    fight_id: c.id.clone(),
                    pinned: !c.pinned,
                });
            }
            return true;
        }
        let Some(action) = keys::action_for(key, modifiers) else {
            return true;
        };
        let req_id = self.next_req_id;
        self.next_req_id = self.next_req_id.wrapping_add(1);
        match (action, h.stored.as_mut()) {
            (Action::Quit, _) => self.state.quit = true,
            (Action::SetView(v), Some(s)) => {
                if s.set_view(v)
                    && let Some(msg) = h.refetch(req_id)
                {
                    requests.push(msg);
                }
            }
            (Action::Open, Some(s)) => {
                if s.drill_selected()
                    && let Some(msg) = h.refetch(req_id)
                {
                    requests.push(msg);
                }
            }
            (Action::Open, None) => {
                if let Some(id) = h.selected_id().map(str::to_string) {
                    requests.push(h.open(id, req_id));
                }
            }
            (Action::Up, Some(s)) => s.sel = s.sel.saturating_sub(1),
            (Action::Down, Some(s)) => {
                let len = s.rows().len();
                if len > 0 {
                    s.sel = (s.sel + 1).min(len - 1);
                }
            }
            (Action::Up, None) => h.sel = h.sel.saturating_sub(1),
            (Action::Down, None) if !h.cards.is_empty() => {
                h.sel = (h.sel + 1).min(h.cards.len() - 1);
            }
            _ => {}
        }
        true
    }

    /// Open History on a scope and ask for its first page.
    fn open_history(&mut self, scope: history::Scope, requests: &mut Vec<wowdps_proto::ClientMsg>) {
        self.home = None;
        self.talents = None;
        let mut h = history::History::new(scope);
        // History opens on the locked character and is the ONE screen that
        // can widen to everyone — its "everyone" chip, which never moves the
        // lock itself.
        h.character = self.owner_guid.clone();
        h.configured = self.cfg.history_characters();
        h.characters = self.known_characters.clone();
        let req_id = self.next_req_id();
        if let Some(msg) = h.next_request(req_id) {
            requests.push(msg);
        }
        self.history = Some(h);
    }

    fn next_req_id(&mut self) -> u32 {
        let id = self.next_req_id;
        self.next_req_id = self.next_req_id.wrapping_add(1);
        id
    }

    /// Open Home and ask for its first slice of cards.
    fn open_home(&mut self, requests: &mut Vec<wowdps_proto::ClientMsg>) {
        let mut ui = home::Home::new();
        // A pick outlives the screen: reopening Home lands on the same
        // character, and so does the next launch (it is in the config).
        ui.character = self.cfg.character.clone();
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
        if let Some(class) = self.home_panels.me.class {
            self.accent_owner = Some(self.home_panels.me.name.clone());
            // Home's "me" IS the locked character when there is a lock:
            // Home opens on the lock and derives its panels from the lock's
            // pulls alone.
            self.learn_owner_class(Some(class), self.home_panels.me.spec, None);
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
        let rows = self.state.rows();
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
    /// it if the chrome is the class's, and — when `who` is the locked
    /// character, or nothing is locked — remember it beside the lock so the
    /// next launch's first frame wears it too. A configured alt that turned
    /// up on the meter is worn for the session and never remembered as the
    /// lock's class. `None` for `who` is the lock itself (a pick).
    fn learn_owner_class(
        &mut self,
        class: Option<wowdps_model::Class>,
        spec: Option<wowdps_model::Spec>,
        who: Option<&str>,
    ) {
        self.owner_class = class.map(|c| (c, spec));
        self.accent = chrome_accent(self.cfg.chrome(), self.owner_class);
        let is_lock = match (self.cfg.character.as_deref(), who) {
            (None, _) | (_, None) => true,
            (Some(lock), Some(who)) => lock == who,
        };
        let name = class.map(|c| c.name().to_string());
        if is_lock && self.cfg.character_class != name {
            self.cfg.character_class = name.clone();
            Config::store_character_class(name);
        }
    }

    /// Merge characters the store named into the window's memory of them.
    /// A configured name with no guid yet is not a character the store can
    /// be asked about, so it waits until a card resolves it.
    fn remember_characters(&mut self, seen: Vec<home::CharLine>) {
        for c in seen.into_iter().filter(|c| !c.guid.is_empty()) {
            if let Some(have) = self.known_characters.iter_mut().find(|h| h.guid == c.guid) {
                *have = c;
            } else {
                self.known_characters.push(c);
            }
        }
        self.known_characters
            .sort_by(|a, b| b.fights.cmp(&a.fights).then(a.name.cmp(&b.name)));
    }
    /// Re-derive the panels from whatever Home holds now.
    fn rederive_home(&mut self) {
        if let Some(ui) = self.home.as_ref() {
            let owner = ui.owner().map(str::to_string);
            if owner.is_some() {
                self.owner_guid = owner.clone();
            }
            self.home_panels = home::derive(
                &ui.cards,
                owner.as_deref(),
                &self.season,
                &self.cfg.history_characters(),
            );
            // The store just named the owner: adopt their accent now rather
            // than at the next drain, so opening Home tints the window.
            self.resolve_accent();
            let seen = self.home_panels.characters.clone();
            self.remember_characters(seen);
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
        gui
    }

    pub(crate) fn set_last_snapshot_at(&mut self, at: Option<Instant>) {
        self.last_snapshot_at = at;
    }

    /// Name the owner directly, as resolve_accent would from the store.
    pub(crate) fn adopt_owner_for_test(&mut self, name: &str) {
        self.accent_owner = Some(name.to_string());
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
/// clock. Reconnects (and re-declares the cursor) if the daemon went away.
/// v19: `Loadout` replies are one-shots the window consumes itself (the
/// shared state machine treats them as no-ops), so they come back to the
/// caller instead of going through `on_msg`.
pub(crate) fn drain_client(
    state: &mut ClientState,
    client: &mut DaemonClient,
    last_snapshot_at: &mut Option<Instant>,
) -> Vec<DaemonMsg> {
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
    intercepted
}

#[derive(Debug, Clone)]
pub(crate) enum Message {
    /// Drain the daemon client and let live durations advance.
    Tick,
    Key(keyboard::Event),
    /// A segment-list row was clicked: select and open it.
    ListRow(usize),
    /// A meter row was clicked: select it and drill in.
    MeterRow(usize),
    /// R12: a meter row's class icon was clicked — pick that player for the
    /// comparison (or unpick them).
    CompareRow(usize),
    /// R12: right-click — drop the picked pair (or a lone half-pick) and
    /// return to the meter. Pointer parity with `Esc`.
    ClearCompare,
    /// R12/v12: a drag on a comparison graph selected a time window (ms from
    /// segment start) — or a right-click asked for the whole fight back.
    CompareRange(Option<(u32, u32)>),
    /// R12/v12: the cursor entered (or left) a marker icon on a comparison
    /// graph; both graphs highlight every use of that item.
    CompareHover(Option<String>),
    /// v14: a drag on the drilldown's graph selected a zoom window (or a
    /// right-click asked for the whole fight back). Client-side only — the
    /// drill timeline is always whole, so nothing round-trips.
    DrillRange(Option<(u32, u32)>),
    /// The BUCKET under the cursor on any graph — one instant, echoed to
    /// every graph sharing the ctl so a comparison marks the same moment on
    /// both curves; the legend words each side's value there. None when the
    /// pointer leaves.
    GraphProbe(Option<usize>),
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
    /// `~` or the Home tab: open the window-local Home screen, or close it
    /// and fall back to whatever `app.screen` already was.
    ToggleHome,
    /// Home's list scrolled. Near its end this asks for the next slice —
    /// paging is transport, and the reader never sees a pager.
    HomeScrolled(home::ScrollAt),
    /// A view tab was clicked: the pointer twin of d/h/i/c/x/K/T.
    PickView(wowdps_model::View),
    /// Leave Home for the live meter (`m`, or the Live tab).
    GotoLive,
    /// The fights tab: close whatever is open and show the segment list.
    GotoList,
    /// A meter column heading was clicked: cycle its sort desc → asc → off.
    SortBy(crate::table::Col),
    /// A by-spell pane heading was clicked: the same cycle for the drill.
    SortSpellsBy(crate::table::Col),
    /// v28: a death chip was clicked — ask for that window's recap.
    PickDeath(u32),
    /// R21: the Taken drill's section chips — the panes, or the stack matrix.
    ShowStacks(bool),
    /// Open History on a scope (the tab, `H`, a Home panel row).
    HistoryOpen(history::Scope),
    /// The fight header's step buttons: the pointer twins of `]` and `[`.
    NewerPull,
    OlderPull,
    /// The fight header's "you" chip: select the owner's row, and bring it
    /// into view.
    SelectOwner,
    /// A History list row was clicked: select and open that stored fight.
    HistoryRow(usize),
    /// History: scope the list to one character guid (None = everyone).
    HistoryCharacter(Option<String>),
    /// History's list scrolled; near its end this pages, like Home.
    HistoryScrolled(home::ScrollAt),
    /// A stored fight's meter row was clicked: drill into that player.
    StoredRow(usize),
    /// Home's recent panel: open one stored fight straight away.
    OpenStored(String),
    /// `?`: show or hide the shortcut sheet.
    ToggleShortcuts,
    /// The filter field's text changed.
    Filter(String),
    /// Home: focus one section — the whole of a list the overview can only
    /// show the head of. `Season` is the overview itself.
    HomeSection(home::Section),
    /// Home: scope the screen to this character guid (None = the newest
    /// card's owner).
    HomeCharacter(Option<String>),
    /// Open or close the character picker's menu.
    TogglePicker,
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
}

/// Which row the pointer is over. Panes are told apart because the drill
/// draws two lists side by side and both answer the mouse.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RowHover {
    Meter(usize),
    Drill(wowdps_model::Pane, usize),
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

fn update(state: &mut Gui, message: Message) -> Task<Message> {
    let mut requests = Vec::new();
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
            let intercepted = drain_client(
                &mut state.state,
                &mut state.client,
                &mut state.last_snapshot_at,
            );
            // v19: the answered loadout lands in the open talent viewer. A
            // `None` loadout leaves whatever the viewer opened with (stored
            // simc paste or the empty tree) — the silent fallback.
            let mut home_changed = false;
            let mut history_changed = false;
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
                        if let Some(h) = state.history.as_mut() {
                            h.absorb(req_id, &answer);
                            history_changed = true;
                            let seen = h.characters.clone();
                            state.remember_characters(seen);
                        }
                    }
                    DaemonMsg::Fight { req_id, fight } => {
                        if let Some(h) = state.history.as_mut() {
                            h.absorb_fight(req_id, fight);
                        }
                    }
                    // The store wrote a fight: the list the reader is looking
                    // at is now one pull out of date. No debounce needed —
                    // this arrives once per closed fight, not on a timer.
                    DaemonMsg::HistoryChanged { .. } => {
                        if let Some(ui) = state.home.as_mut() {
                            ui.reset();
                            home_changed = true;
                            store_changed = true;
                        }
                        if let Some(h) = state.history.as_mut() {
                            h.reset();
                            history_changed = true;
                        }
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
            if history_changed {
                let req_id = state.next_req_id();
                if let Some(msg) = state.history.as_mut().and_then(|h| h.next_request(req_id)) {
                    requests.push(msg);
                }
            }
            if home_changed {
                state.rederive_home();
                // Keep the list filling itself: one request in flight, and
                // only while Home is the screen the reader is looking at.
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
            let rows = state.state.rows();
            let owner = state.owner_in(&rows).and_then(|i| rows.get(i));
            state.seen.observe(&state.state, owner);
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
                ..
            } = event
            {
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
                    if modified_key == keyboard::Key::Named(keyboard::key::Named::Escape) {
                        state.talents = None;
                        // A parked reply must not land in a viewer opened
                        // later for someone else.
                        state.pending_loadout = None;
                    } else if modified_key == keyboard::Key::Named(keyboard::key::Named::Tab) {
                        ui.on_msg(talents::Msg::ToggleTab);
                    }
                } else if state.picker_open {
                    // The menu is modal the way the sheet is: any key closes
                    // it and does nothing else.
                    state.picker_open = false;
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
                        state.history = None;
                        state.open_home(&mut requests);
                    }
                } else if modified_key == keyboard::Key::Character("?".into()) {
                    state.shortcuts_open = true;
                } else if modified_key == keyboard::Key::Character("/".into())
                    && state.filter_visible()
                {
                    state.filter_focused = true;
                    return iced::widget::operation::focus(crate::nav::filter_id());
                } else if modified_key == keyboard::Key::Character("m".into()) {
                    // Back to the live meter from anywhere, through the
                    // accessor that already exists rather than a new Action.
                    state.home = None;
                    requests.extend(state.state.pin_live());
                } else if state.home.is_some()
                    && modified_key == keyboard::Key::Named(keyboard::key::Named::Escape)
                {
                    // Esc walks one level up, and a focused section is a
                    // level: it returns to the overview before Home itself
                    // closes. Home sits ABOVE the state machine's screens,
                    // so neither step may reach `Action::Back`.
                    match state.home.as_mut() {
                        Some(ui) if ui.section != home::Section::Season => {
                            ui.section = home::Section::Season;
                        }
                        _ => state.home = None,
                    }
                } else if modified_key == keyboard::Key::Character("H".into()) {
                    // History from anywhere; pressed on History, it closes.
                    if state.history.is_some() {
                        state.history = None;
                    } else {
                        state.open_history(history::Scope::All, &mut requests);
                    }
                } else if state.history_key(&modified_key, modifiers, &mut requests) {
                    // Consumed by the History screen.
                } else if state.state.screen == wowdps_model::Screen::List
                    && modified_key == keyboard::Key::Named(keyboard::key::Named::Escape)
                {
                    // The view map: Esc from the fight list lands on Home,
                    // the front door — `Action::Back` has nowhere to go from
                    // the list, so the key would otherwise be dead here.
                    state.open_home(&mut requests);
                } else if let Some(step) = state.death_step(&modified_key) {
                    // ← → step the death windows on a Deaths drill, where
                    // the segment keys would otherwise leave the drill.
                    requests.extend(state.state.select_death(Some(step)));
                } else if modified_key == keyboard::Key::Character("t".into())
                    && !modifiers.control()
                {
                    // Open on the selected meter row's player when there is
                    // one: a stored simc paste (or the spec's empty tree)
                    // shows instantly, and the daemon is asked for the
                    // logged COMBATANT_INFO build, which wins when it lands.
                    let row = state.state.rows().get(state.state.row_sel).cloned();
                    let player = row
                        .as_ref()
                        .map(|r| (r.label.clone(), r.spec.map(|s| s.id())));
                    state.talents = Some(talents::TalentsUi::open(player));
                    // Any older request now answers a viewer that no longer
                    // exists; only the request made HERE may adopt.
                    state.pending_loadout = None;
                    if let Some(r) = row {
                        let req_id = state.next_req_id;
                        state.next_req_id = state.next_req_id.wrapping_add(1);
                        state.pending_loadout = Some(req_id);
                        requests.push(wowdps_proto::ClientMsg::GetLoadout {
                            req_id,
                            segment: state.state.watched_segment(),
                            guid: r.key,
                        });
                    }
                } else if let Some(action) = keys::action_for(&modified_key, modifiers) {
                    // A filtered list is what the reader can SEE, so j/k
                    // must walk it: stepping through hidden rows would park
                    // the highlight on nothing and drill into a stranger.
                    match state.filtered_step(action) {
                        Some(Step::Meter(row)) => state.state.row_sel = row,
                        Some(Step::Spell(row)) => {
                            if let Some(d) = state.state.drill.as_mut() {
                                d.spell_sel = row;
                            }
                        }
                        None => requests.extend(state.state.apply(action)),
                    }
                    // The selection a step moved stays in sight: past the
                    // fold the list follows it, or Enter would drill into a
                    // row the reader cannot see.
                    if matches!(action, Action::Up | Action::Down) {
                        follow = Some(state.keep_row_in_sight(state.state.row_sel));
                    }
                }
            }
        }
        Message::ListRow(row) => {
            state.state.set_list_selection(row);
            requests.extend(state.state.apply(Action::Open));
        }
        Message::MeterRow(row) => {
            state.state.row_sel = row;
            requests.extend(state.state.apply(Action::Open));
        }
        // R12: pick by class icon. Selecting the row first keeps the keyboard
        // and the pointer on the same player.
        Message::CompareRow(row) => {
            state.state.row_sel = row;
            requests.extend(state.state.apply(Action::PickCompare));
        }
        Message::ClearCompare => {
            requests.extend(state.state.clear_compare());
        }
        Message::CompareRange(range) => {
            requests.extend(state.state.set_compare_range(range));
        }
        Message::CompareHover(label) => state.compare_hover = label,
        Message::DrillRange(range) => requests.extend(state.state.set_drill_range(range)),
        Message::GraphProbe(v) => state.graph_probe = v,
        // v16: select the clicked spell row, then Open descends into it.
        Message::SpellRow(i) => {
            if let Some(d) = state.state.drill.as_mut() {
                d.spell_sel = i;
                d.pane = wowdps_model::Pane::Spell;
            }
            requests.extend(state.state.apply(Action::Open));
        }
        Message::AttackerRow(i) => {
            if let Some(d) = state.state.drill.as_mut() {
                d.target_sel = i;
                d.pane = wowdps_model::Pane::Target;
            }
            requests.extend(state.state.apply(Action::Open));
        }
        Message::CompareSpell((key, label)) => {
            requests.extend(state.state.drill_compare_spell(&key, &label));
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
        Message::ToggleHome => {
            if state.home.is_some() {
                state.home = None;
            } else {
                state.history = None;
                state.open_home(&mut requests);
            }
        }
        Message::HomeScrolled(viewport) => {
            // The gesture fires many times a second; `next_request` is what
            // makes that safe — one request in flight, and none at all once
            // the cache holds everything the store matched.
            if home::wants_more(viewport.content_h, viewport.view_h, viewport.offset_y) {
                let req_id = state.next_req_id();
                if let Some(ui) = state.home.as_mut() {
                    ui.scrolled_to_end();
                    if let Some(msg) = ui.next_request(req_id, &state.season) {
                        requests.push(msg);
                    }
                }
            }
        }
        Message::PickView(view) => {
            // On a stored fight the tab switches ITS view; anywhere else
            // the tab is the meter's and History steps aside.
            let req_id = state.next_req_id();
            if let Some(h) = state.history.as_mut()
                && let Some(s) = h.stored.as_mut()
            {
                if s.set_view(view)
                    && let Some(msg) = h.refetch(req_id)
                {
                    requests.push(msg);
                }
            } else {
                state.home = None;
                state.history = None;
                requests.extend(state.state.apply(Action::SetView(view)));
            }
        }
        Message::GotoLive => {
            state.home = None;
            state.history = None;
            requests.extend(state.state.pin_live());
        }
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
        Message::HistoryOpen(scope) => state.open_history(scope, &mut requests),
        Message::NewerPull => requests.extend(state.state.apply(Action::NewerSegment)),
        Message::OlderPull => requests.extend(state.state.apply(Action::OlderSegment)),
        Message::SelectOwner => {
            let rows = state.state.rows();
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
                state.state.row_sel = owner;
                // Into view, the least that shows it whole: nothing when it
                // already is.
                follow = Some(state.keep_row_in_sight(owner));
            }
        }
        Message::HistoryCharacter(guid) => {
            state.picker_open = false;
            let req_id = state.next_req_id();
            if let Some(h) = state.history.as_mut() {
                h.set_character(guid);
                if let Some(msg) = h.next_request(req_id) {
                    requests.push(msg);
                }
            }
        }
        Message::HistoryRow(i) => {
            let req_id = state.next_req_id();
            if let Some(h) = state.history.as_mut() {
                h.sel = i;
                if let Some(id) = h.selected_id().map(str::to_string) {
                    requests.push(h.open(id, req_id));
                }
            }
        }
        Message::HistoryScrolled(at) => {
            if home::wants_more(at.content_h, at.view_h, at.offset_y) {
                let req_id = state.next_req_id();
                if let Some(h) = state.history.as_mut() {
                    h.scrolled_to_end();
                    if let Some(msg) = h.next_request(req_id) {
                        requests.push(msg);
                    }
                }
            }
        }
        Message::StoredRow(i) => {
            let req_id = state.next_req_id();
            if let Some(h) = state.history.as_mut()
                && let Some(s) = h.stored.as_mut()
            {
                s.sel = i;
                if s.drill_selected()
                    && let Some(msg) = h.refetch(req_id)
                {
                    requests.push(msg);
                }
            }
        }
        Message::OpenStored(id) => {
            state.open_history(history::Scope::All, &mut requests);
            let req_id = state.next_req_id();
            if let Some(h) = state.history.as_mut() {
                requests.push(h.open(id, req_id));
            }
        }
        Message::PickDeath(i) => {
            requests.extend(state.state.select_death(Some(i)));
        }
        Message::SortSpellsBy(col) => {
            state.drill_sort = match state.drill_sort {
                Some((c, true)) if c == col => Some((col, false)),
                Some((c, false)) if c == col => None,
                _ => Some((col, true)),
            };
        }
        Message::GotoList => {
            state.home = None;
            state.history = None;
            state.talents = None;
            // Back walks ability → drill → meter → list; bounded, since the
            // list itself answers Back with nothing.
            for _ in 0..4 {
                if state.state.screen == wowdps_model::Screen::List {
                    break;
                }
                requests.extend(state.state.apply(Action::Back));
            }
        }
        Message::HomeSection(section) => {
            if let Some(ui) = state.home.as_mut() {
                ui.section = section;
            }
        }
        Message::TogglePicker => {
            state.picker_open = !state.picker_open;
            state.picker_hover = None;
        }
        Message::PickerHover(at) => state.picker_hover = at,
        Message::HomeCharacter(guid) => {
            state.picker_open = false;
            if let Some(ui) = state.home.as_mut() {
                ui.character = guid.clone();
            }
            // The pick is the WINDOW's lock, not Home's: it names whose
            // chrome this is and who History opens on, and it is remembered
            // in the config so the next launch is already theirs. `None` is
            // back to the newest card's owner, which `rederive_home`
            // resolves again.
            state.cfg.character = guid.clone();
            state.cfg.save();
            state.owner_guid = guid.clone();
            state.accent_owner = None;
            // The pick's class, as far as the window already knows it: the
            // remembered one belonged to the previous lock.
            let picked = state
                .known_characters
                .iter()
                .find(|c| Some(c.guid.as_str()) == guid.as_deref())
                .map(|c| (c.name.clone(), c.class, c.spec));
            state.learn_owner_class(
                picked.as_ref().and_then(|p| p.1),
                picked.as_ref().and_then(|p| p.2),
                None,
            );
            state.rederive_home();
            // Picked from the tab strip with no Home open: Home's panels
            // cannot name the owner, so the character list the window
            // remembers does. And an open History follows the lock.
            if state.accent_owner.is_none()
                && let Some((name, _, _)) = picked
            {
                state.accent_owner = Some(name);
            }
            let req_id = state.next_req_id();
            if let Some(h) = state.history.as_mut() {
                h.set_character(guid);
                if let Some(msg) = h.next_request(req_id) {
                    requests.push(msg);
                }
            }
        }
        Message::ToggleShortcuts => state.shortcuts_open = !state.shortcuts_open,
        Message::Filter(text) => state.filter = text,
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
    }
    for req in requests {
        state.client.send(&req);
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

fn subscription(state: &Gui) -> Subscription<Message> {
    let mut subs = vec![
        time::every(TICK).map(|_| Message::Tick),
        keyboard::listen().map(Message::Key),
    ];
    if state.filter_focused && state.filter_visible() {
        subs.push(iced::event::listen_with(captured_escape));
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
        let mut mock = MockDaemon::fixture();
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
        let (mut state, mut mock) = wipe();
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

    /// A Heroic raid kill of `players` — what the committed fixtures cannot
    /// hold (a handful of players at most): one segment in the list, the
    /// meter on it, the rows in the daemon's order, every spec in turn so
    /// tanks and healers are among them. Row `i` is "Raider{i}-Realm-US"
    /// with guid "Player-1-{i}", and the amounts step down from 100 M.
    pub(crate) fn raid(players: usize) -> ClientState {
        use wowdps_model::{
            Encounter, ListRow, Row, SegmentId, SegmentInfo, SegmentKind, Spec, View,
        };
        use wowdps_proto::{ListEntry, SegmentRef};
        let secs = 422.04;
        let rows: Vec<Row> = (0..players)
            .map(|i| {
                let spec = Spec::ALL[i % Spec::ALL.len()];
                let amount = 100_000_000_u64.saturating_sub(i as u64 * 2_000_000);
                Row {
                    key: format!("Player-1-{i}"),
                    label: format!("Raider{i}-Realm-US"),
                    amount,
                    extra: 50_000,
                    count: 1_000,
                    crits: 300,
                    per_sec: amount as f64 / secs,
                    pct: 100.0 / players as f64,
                    class: Some(spec.class()),
                    spec: Some(spec),
                    ..Row::default()
                }
            })
            .collect();
        let encounter = Some(Encounter {
            id: 3492,
            difficulty: 15,
            group_size: 25,
        });
        let info = SegmentInfo {
            kind: SegmentKind::Encounter,
            name: "The Coiled Altar".to_string(),
            start_ms: 1_000,
            duration_ms: 422_040,
            success: Some(true),
            live: false,
            instance: Some(0),
            pars_ms: None,
            arena: false,
            encounter,
        };
        let mut state = ClientState::new();
        let _ = state.on_msg(DaemonMsg::SegmentList {
            seq: 1,
            entries: vec![ListEntry {
                id: SegmentId(1),
                row: ListRow {
                    kind: SegmentKind::Encounter,
                    name: info.name.clone(),
                    start_ms: info.start_ms,
                    success: info.success,
                    duration_ms: info.duration_ms,
                    live: false,
                    instance: Some(0),
                    pars_ms: None,
                    arena: false,
                    encounter,
                },
            }],
            source: Some("raid.txt".to_string()),
            active: true,
            log_id: None,
        });
        let _ = state.on_msg(DaemonMsg::Snapshot {
            seq: 2,
            segment: SegmentRef::Live,
            id: Some(SegmentId(1)),
            view: View::Damage,
            info,
            total_rows: rows.len() as u32,
            rows,
            breakdown: None,
            segment_count: 1,
            source: Some("raid.txt".to_string()),
            status: None,
        });
        assert_eq!(state.screen, wowdps_model::Screen::Meter);
        state
    }

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
    /// at which the window lays out every column — under `theme::NARROW`
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

    #[test]
    fn ticks_drain_the_daemon_into_the_state() {
        let b = Bridge::new(MockDaemon::fixture());
        assert_eq!(b.gui.state.screen, Screen::List);
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
        b.send(Message::ListRow(0));
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
        b.send(Message::ListRow(0));
        b.send(Message::CompareRow(0));
        assert_eq!(b.gui.state.compare_picks().len(), 1);
        assert_eq!(b.gui.state.screen, Screen::Meter);
        b.send(Message::CompareRow(1));
        assert_eq!(b.gui.state.screen, Screen::Compare);
        let (a, bb) = b.gui.state.compare_sides().expect("both sides answered");
        let spell = a.spells.first().cloned().expect("the side has spells");
        assert_ne!(a.guid, bb.guid);

        b.send(Message::CompareHover(Some("Potion".to_string())));
        assert_eq!(b.gui.compare_hover.as_deref(), Some("Potion"));
        b.send(Message::GraphProbe(Some(12)));
        assert_eq!(b.gui.graph_probe, Some(12));

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
        b.send(named(Named::Enter));
        assert_eq!(b.gui.state.screen, Screen::Meter);
        b.send(chr("j"));
        assert_eq!(b.gui.state.row_sel, 1);
        b.send(chr("h"));
        assert_eq!(b.gui.state.view, View::Healing);
        b.send(named(Named::Escape));
        assert_eq!(b.gui.state.screen, Screen::List);
        // Unknown keys are ignored.
        b.send(chr("z"));
        assert_eq!(b.gui.state.screen, Screen::List);
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
        b.send(named(Named::Enter));
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
        b.send(named(Named::Enter));
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
        // A key step on the meter asks for the scroll; a drilled meter has
        // no list to keep anything in.
        drop(ui);
        gui.state.row_sel = 20;
        assert!(update(&mut gui, chr("j")).units() > 0);
        assert_eq!(gui.state.row_sel, 21);
        gui.state.drill = Some(wowdps_model::Drill {
            key: String::new(),
            label: String::new(),
            pane: Pane::Spell,
            spell_sel: 0,
            target_sel: 0,
            spell: None,
        });
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
        assert!(!b.gui.home_panels.recent.is_empty());
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

    /// The user requirement: the list grows by scrolling, never by a button.
    #[test]
    fn scrolling_to_the_bottom_asks_for_more_without_a_button() {
        let mut b = home_bridge();
        b.send(chr("~"));
        let ui = b.gui.home.as_mut().unwrap();
        // Pretend the burst already ran out, as it would after 1 000 cards.
        ui.pages = crate::home::MAX_PAGES;
        ui.total = Some(u32::MAX);
        let _ = b.requests();
        let _ = update(
            &mut b.gui,
            Message::HomeScrolled(crate::home::ScrollAt {
                content_h: 2000.0,
                view_h: 400.0,
                offset_y: 1600.0,
            }),
        );
        let asked = b.requests();
        assert!(
            asked
                .iter()
                .any(|r| matches!(r, ClientMsg::GetHistory { .. })),
            "the scroll gesture is the only affordance, {asked:?}"
        );
    }

    /// The same gesture fires many times a second: it must not become many
    /// requests, or it would flood the queue the daemon's read quota exists
    /// to protect.
    #[test]
    fn a_second_scroll_while_a_query_is_out_sends_nothing() {
        let mut b = home_bridge();
        b.send(chr("~"));
        let ui = b.gui.home.as_mut().unwrap();
        ui.pages = 0;
        ui.total = Some(u32::MAX);
        ui.pending = None;
        let _ = b.requests();
        let _ = update(
            &mut b.gui,
            Message::HomeScrolled(crate::home::ScrollAt {
                content_h: 2000.0,
                view_h: 400.0,
                offset_y: 1600.0,
            }),
        );
        assert_eq!(b.requests().len(), 1, "the first scroll asks once");
        let _ = update(
            &mut b.gui,
            Message::HomeScrolled(crate::home::ScrollAt {
                content_h: 2000.0,
                view_h: 400.0,
                offset_y: 1600.0,
            }),
        );
        assert!(
            b.requests().is_empty(),
            "a second scroll with one in flight asks nothing"
        );
    }

    #[test]
    fn a_short_list_never_asks_for_more() {
        // `relative_offset` would divide by zero here; `wants_more` must not.
        assert!(!crate::home::wants_more(100.0, 400.0, 0.0));
        assert!(
            !crate::home::wants_more(4000.0, 400.0, 0.0),
            "the top of a long list is not the end"
        );
        assert!(crate::home::wants_more(4000.0, 400.0, 3500.0));
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
        b.send(named(Named::Enter));
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

        // Nor inside a drilldown, whose panes are abilities and targets.
        b.send(named(Named::Enter));
        b.send(Message::MeterRow(0));
        assert!(b.gui.state.drill.is_some());
        b.send(chr("/"));
        assert!(!b.gui.filter_focused);
    }

    /// iced owns focus: a click elsewhere unfocuses the field without ever
    /// telling us, so the tick's answer is what the swallow flag follows.
    #[test]
    fn the_fields_own_focus_wins_over_the_flag() {
        let mut b = home_bridge();
        b.send(named(Named::Enter));
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
        b.send(named(Named::Enter));
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
        b.send(named(Named::Enter));
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
        b.send(named(Named::Enter));
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
        b.send(named(Named::Enter));
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

    #[test]
    fn esc_walks_one_level_up_through_the_new_layers() {
        let mut b = home_bridge();
        b.send(named(Named::Enter));
        assert_eq!(b.gui.state.screen, Screen::Meter);
        b.send(chr("~"));
        b.send(chr("?"));
        b.send(named(Named::Escape));
        assert!(!b.gui.shortcuts_open, "the sheet goes first");
        assert!(b.gui.home.is_some(), "Home is still up");
        b.send(named(Named::Escape));
        assert!(b.gui.home.is_none(), "then Home");
        b.send(named(Named::Escape));
        assert_eq!(b.gui.state.screen, Screen::List, "then Action::Back");
    }

    /// The chrome says whose window this is, not what the cursor is on.
    /// Rows resort on every 10 Hz snapshot, so an accent taken from the
    /// selection re-tinted the whole window whenever rank 1 changed class.
    #[test]
    fn the_accent_ignores_the_selection_and_the_sort() {
        let mut b = Bridge::with_config(MockDaemon::fixture().with_history(), class_chrome());
        b.send(named(Named::Enter));
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
        b.send(named(Named::Enter));
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

    /// The other identity: the owner Home derives from the store's cards.
    #[test]
    fn home_naming_the_owner_tints_the_window() {
        let mut b = Bridge::with_config(MockDaemon::fixture().with_history(), class_chrome());
        b.send(named(Named::Enter));
        assert_eq!(view::accent_for_test(&b.gui), theme::NEUTRAL);
        b.gui.home_panels.me.name = "Mírelle-Nebula-US".to_string();
        b.gui.home_panels.me.class = Some(wowdps_model::Class::Priest);
        b.send(Message::Tick);
        assert_eq!(
            view::accent_for_test(&b.gui),
            theme::accent(Some(wowdps_model::Class::Priest), None)
        );
        // And the class is remembered beside the lock for the next launch.
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
        let mut b = Bridge::with_config(
            MockDaemon::fixture().with_history(),
            Config {
                extra,
                ..test_config()
            },
        );
        b.send(named(Named::Enter));
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
    /// a zoom) survives. And only the LOCK's class is remembered — a
    /// configured alt who turns up on the meter is worn for the session,
    /// never written down as the locked character's.
    #[test]
    fn learning_the_class_keeps_the_overlays_placement_and_the_locks_class() {
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
        // No lock: the resolved owner's class is remembered.
        let mut b = Bridge::with_config(
            MockDaemon::fixture().with_history(),
            Config {
                extra: owner("Thraxx-Nebula-US"),
                ..class_chrome()
            },
        );
        // The overlay is dragged after the window read the config.
        let mut disk = Config::load();
        disk.offset = 4242;
        disk.character_class = None;
        disk.save();
        b.send(named(Named::Enter));
        assert_eq!(b.gui.owner_name(), Some("Thraxx-Nebula-US"));
        let now = Config::load();
        assert_eq!(now.offset, 4242, "the overlay's drag survives");
        assert!(now.character_class.is_some(), "the class is remembered");

        // Locked to someone else: Thraxx is worn, and not written down.
        let mut disk = Config::load();
        disk.character_class = Some("Priest".to_string());
        disk.save();
        let mut b = Bridge::with_config(
            MockDaemon::fixture().with_history(),
            Config {
                extra: owner("Thraxx-Nebula-US"),
                character: Some("Player-0000-LOCKED".to_string()),
                character_class: Some("Priest".to_string()),
                ..class_chrome()
            },
        );
        b.send(named(Named::Enter));
        assert_eq!(b.gui.owner_name(), Some("Thraxx-Nebula-US"));
        assert_ne!(
            view::accent_for_test(&b.gui),
            theme::accent(Some(wowdps_model::Class::Priest), None),
            "the session wears the owner on the meter"
        );
        assert_eq!(b.gui.cfg.character_class.as_deref(), Some("Priest"));
        assert_eq!(
            Config::load().character_class.as_deref(),
            Some("Priest"),
            "the lock's class is not overwritten by an alt's"
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
        b.send(named(Named::Enter));
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
        // The selection starts on row 0 of the daemon's order; a step down
        // lands on whatever is drawn under it now.
        let sel = b.gui.state.drill.as_ref().unwrap().spell_sel;
        let pos = order.iter().position(|&i| i == sel).unwrap();
        b.send(chr("j"));
        assert_eq!(
            b.gui.state.drill.as_ref().unwrap().spell_sel,
            order[(pos + 1).min(order.len() - 1)]
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

    /// v28: on a Deaths drill ← → ask for another death window through
    /// the state machine; on any other drill they still step segments.
    #[test]
    fn arrows_step_death_windows_on_a_deaths_drill_only() {
        let mut b = home_bridge();
        b.send(named(Named::Enter));
        b.send(chr("K"));
        b.send(named(Named::Enter));
        assert!(b.gui.state.drill.is_some());
        assert_eq!(b.gui.state.view, View::Deaths);
        let (deaths, shown) = b.gui.state.deaths();
        assert!(
            !deaths.is_empty(),
            "the fixture's top death row has a window"
        );
        let last = deaths.len() as u32 - 1;
        assert_eq!(
            shown,
            Some(last),
            "the daemon describes the last death by default"
        );
        let before = b.gui.state.segment_index();
        b.send(named(Named::ArrowLeft));
        let asked = last.saturating_sub(1);
        assert_eq!(
            b.gui.state.death_request(),
            Some(asked),
            "← asks for the previous window (or the same one when there is one)"
        );
        assert_eq!(
            b.gui.state.segment_index(),
            before,
            "and never moves the segment"
        );
        b.send(Message::PickDeath(last));
        assert_eq!(b.gui.state.death_request(), Some(last));
        // Back to Damage: the arrows are segment keys again.
        b.send(chr("d"));
        assert_eq!(b.gui.state.death_request(), None);
        b.send(named(Named::ArrowLeft));
        assert_ne!(
            b.gui.state.segment_index(),
            before,
            "← is OlderSegment here"
        );
    }

    /// The History screen: `H` opens it anywhere, Enter opens a stored
    /// fight the mock answers, Esc walks back out one level at a time, and
    /// `p` asks the daemon to pin the selected pull.
    #[test]
    fn history_opens_a_stored_fight_and_walks_back_out() {
        use wowdps_proto::ClientMsg;
        let mut b = Bridge::new(MockDaemon::fixture().with_history());
        b.send(chr("H"));
        let h = b.gui.history.as_ref().expect("H opens History");
        assert_eq!(h.scope, history::Scope::All);
        assert!(!h.cards.is_empty(), "the first page landed");
        assert_eq!(b.gui.surface(), keys::Surface::History);
        b.send(named(Named::Enter));
        let s = b.gui.history.as_ref().unwrap().stored.as_ref();
        let s = s.expect("Enter opens the selected pull");
        assert!(
            matches!(s.fight, Some(Some(_))),
            "the mock answered GetFight"
        );
        assert!(!s.rows().is_empty());
        // A view key switches the stored fight's view and refetches it.
        b.send(chr("h"));
        let s = b.gui.history.as_ref().unwrap().stored.as_ref().unwrap();
        assert_eq!(s.view, View::Healing);
        assert!(matches!(s.fight, Some(Some(_))));
        // Enter drills; Esc backs out drill → fight → list → closed.
        b.send(named(Named::Enter));
        assert!(
            b.gui
                .history
                .as_ref()
                .unwrap()
                .stored
                .as_ref()
                .unwrap()
                .drill
                .is_some()
        );
        b.send(named(Named::Escape));
        assert!(
            b.gui
                .history
                .as_ref()
                .unwrap()
                .stored
                .as_ref()
                .unwrap()
                .drill
                .is_none()
        );
        b.send(named(Named::Escape));
        assert!(b.gui.history.as_ref().unwrap().stored.is_none());
        // p pins: the request carries the selected card's id.
        let id = b
            .gui
            .history
            .as_ref()
            .unwrap()
            .selected_id()
            .unwrap()
            .to_string();
        let _ = update(&mut b.gui, chr("p"));
        let sent = b.requests();
        assert!(
            sent.iter().any(|m| matches!(m, ClientMsg::PinFight { fight_id, pinned: true, .. } if *fight_id == id)),
            "{sent:?}"
        );
        // Reading the socket took the request from the mock: serve it.
        for req in sent {
            for reply in b.mock.handle(req) {
                b.push(&reply);
            }
        }
        b.settle();
        assert!(
            b.gui
                .history
                .as_ref()
                .unwrap()
                .cards
                .iter()
                .any(|c| c.id == id && c.pinned)
        );
        b.send(named(Named::Escape));
        assert!(b.gui.history.is_none(), "the last Esc closes History");
        // Home's recent panel opens a fight straight away.
        b.send(Message::OpenStored(id.clone()));
        let h = b.gui.history.as_ref().unwrap();
        assert_eq!(h.stored.as_ref().unwrap().fight_id, id);
        assert!(matches!(h.stored.as_ref().unwrap().fight, Some(Some(_))));
        // The fights tab and ~ both step past History.
        b.send(Message::GotoList);
        assert!(b.gui.history.is_none());
    }

    /// History scopes to a character: the chips remember everyone the
    /// unscoped list saw, and a scope re-asks the store with that guid.
    #[test]
    fn history_scopes_to_a_character_and_remembers_the_others() {
        use wowdps_proto::{ClientMsg, HistoryQuery};
        // The fixture cards name no owner, so the config names one of the
        // players — the same join the real config makes.
        let mock = MockDaemon::fixture().with_history();
        let me = mock.history().cards()[0].players[0].name.clone();
        let mut cfg = testkit::test_config();
        cfg.extra.insert(
            "history_characters".to_string(),
            toml::Value::Array(vec![toml::Value::String(me.clone())]),
        );
        let mut b = Bridge::with_config(mock, cfg);
        b.send(chr("H"));
        let h = b.gui.history.as_ref().unwrap();
        assert!(
            h.characters.iter().any(|c| c.name == me),
            "a configured name resolves to a chip"
        );
        let guid = h.characters[0].guid.clone();
        let _ = update(&mut b.gui, Message::HistoryCharacter(Some(guid.clone())));
        let sent = b.requests();
        assert!(
            sent.iter().any(|m| matches!(
                m,
                ClientMsg::GetHistory { query: HistoryQuery::Fights { guid: Some(g), .. }, .. } if *g == guid
            )),
            "{sent:?}"
        );
        for req in sent {
            for reply in b.mock.handle(req) {
                b.push(&reply);
            }
        }
        b.settle();
        let h = b.gui.history.as_ref().unwrap();
        assert_eq!(h.character.as_deref(), Some(guid.as_str()));
        assert!(!h.characters.is_empty(), "the chips survive the scope");
        assert!(
            h.cards
                .iter()
                .all(|c| c.players.iter().any(|p| p.guid == guid))
        );
        b.send(Message::HistoryCharacter(None));
        assert!(b.gui.history.as_ref().unwrap().character.is_none());
    }
    #[test]
    fn m_from_home_pins_live() {
        let mut b = home_bridge();
        b.send(chr("~"));
        b.send(chr("m"));
        assert!(b.gui.home.is_none());
        assert!(b.gui.state.following_live());
    }

    /// The view map: the fight list's Esc lands on Home, the front door —
    /// `Action::Back` has nowhere to go from the list.
    #[test]
    fn esc_from_the_list_opens_home() {
        let mut b = home_bridge();
        assert_eq!(b.gui.state.screen, Screen::List);
        b.send(named(Named::Escape));
        assert!(b.gui.home.is_some());
        b.send(named(Named::Escape));
        assert!(b.gui.home.is_none(), "and Esc from Home closes it again");
        assert_eq!(b.gui.state.screen, Screen::List);
    }

    /// The fights tab shows the list from any depth: Home, a drill, an
    /// ability — never the live meter it used to pin.
    #[test]
    fn the_fights_tab_walks_back_to_the_list() {
        let mut b = home_bridge();
        b.send(named(Named::Enter));
        b.send(named(Named::Enter));
        assert!(b.gui.state.drill.is_some());
        b.send(chr("~"));
        b.send(Message::GotoList);
        assert!(b.gui.home.is_none());
        assert!(b.gui.state.drill.is_none());
        assert_eq!(b.gui.state.screen, Screen::List);
    }

    /// `m` is not Home's alone: it pins the live meter from any surface.
    #[test]
    fn m_pins_live_from_the_meter_too() {
        let mut b = home_bridge();
        b.send(named(Named::Enter));
        assert_eq!(b.gui.state.screen, Screen::Meter);
        b.send(chr("m"));
        assert!(b.gui.state.following_live());
    }

    /// The sheet is keyed on the surface: the meter's view keys are "here"
    /// on the meter and "elsewhere" on the fight list.
    #[test]
    fn the_sheet_knows_which_surface_it_is_on() {
        let mut b = home_bridge();
        assert_eq!(b.gui.surface(), keys::Surface::List);
        b.send(named(Named::Enter));
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

    /// A chip is a control, so it must DO something: focus its section, show
    /// that one whole, and hide the others.
    #[test]
    fn each_chip_focuses_its_own_section() {
        let mut b = home_bridge();
        b.send(chr("~"));
        let panels = b.gui.home_panels.clone();
        let offered = crate::home::sections(&panels);
        assert!(
            offered.len() > 2,
            "the fixture must offer real sections, saw {offered:?}"
        );
        for section in offered.iter().copied() {
            b.send(Message::HomeSection(section));
            assert_eq!(b.gui.home.as_ref().unwrap().section, section);
            let mut ui = simulator(view::view(&b.gui));
            if section == crate::home::Section::Season {
                // The overview: every panel that has content.
                for other in offered.iter().copied() {
                    if let Some(m) = marker(other, &panels) {
                        assert!(
                            ui.find(m.as_str()).is_ok(),
                            "{other:?} missing from the overview"
                        );
                    }
                }
                continue;
            }
            let Some(mine) = marker(section, &panels) else {
                continue;
            };
            assert!(ui.find(mine.as_str()).is_ok(), "{section:?} did not render");
            for other in offered.iter().copied() {
                if other == section || other == crate::home::Section::Season {
                    continue;
                }
                if let Some(m) = marker(other, &panels) {
                    assert!(
                        ui.find(m.as_str()).is_err(),
                        "{other:?} still on screen while {section:?} is focused"
                    );
                }
            }
        }
    }

    /// A marker that appears ONLY inside that section's panel. The chip row
    /// repeats every section's word, so the panel headings cannot be the
    /// probe — "me" and "raid" are on screen as chips whatever is focused.
    fn marker(s: crate::home::Section, panels: &crate::home::Panels) -> Option<String> {
        match s {
            crate::home::Section::Keys => Some("mythic+".to_string()),
            crate::home::Section::Raid => {
                let down = panels
                    .raid
                    .bosses
                    .iter()
                    .filter(|b| b.best_kill_ms.is_some())
                    .count();
                Some(format!("{down} down · {} seen", panels.raid.bosses.len()))
            }
            crate::home::Section::Me => Some(if panels.me.name.is_empty() {
                "no owner identified — set history_characters in the config".to_string()
            } else {
                "deaths / pull".to_string()
            }),
            crate::home::Section::Recent => panels
                .recent
                .first()
                .filter(|r| !r.tag.is_empty())
                .map(|r| crate::nav::sentence(&r.tag)),
            crate::home::Section::Season => None,
        }
    }

    #[test]
    fn the_active_chip_returns_to_the_overview() {
        let mut b = home_bridge();
        b.send(chr("~"));
        b.send(Message::HomeSection(crate::home::Section::Recent));
        assert_eq!(
            b.gui.home.as_ref().unwrap().section,
            crate::home::Section::Recent
        );
        // Pressing the chip that is already active is the way back — a chip
        // must never be a one-way door.
        let panels = b.gui.home_panels.clone();
        let offered = crate::home::sections(&panels);
        let mut ui = simulator(view::view(&b.gui));
        let idx = offered
            .iter()
            .position(|s| *s == crate::home::Section::Recent)
            .unwrap();
        assert_eq!(offered[idx], crate::home::Section::Recent);
        ui.click("recent").unwrap();
        let msgs: Vec<Message> = ui.into_messages().collect();
        assert!(
            msgs.iter()
                .any(|m| matches!(m, Message::HomeSection(crate::home::Section::Season))),
            "the active chip must lead back to the overview, got {msgs:?}"
        );
    }

    #[test]
    fn esc_leaves_a_focused_section_before_it_leaves_home() {
        let mut b = home_bridge();
        b.send(named(Named::Enter));
        b.send(chr("~"));
        b.send(Message::HomeSection(crate::home::Section::Recent));
        b.send(named(Named::Escape));
        assert!(
            b.gui.home.is_some(),
            "Esc closed Home instead of the section"
        );
        assert_eq!(
            b.gui.home.as_ref().unwrap().section,
            crate::home::Section::Season
        );
        b.send(named(Named::Escape));
        assert!(b.gui.home.is_none(), "and then Home");
    }

    /// §9 holds inside a focused section too: the long list grows by
    /// scrolling, with one request in flight and no pager.
    #[test]
    fn a_focused_list_still_appends_on_scroll() {
        let mut b = home_bridge();
        b.send(chr("~"));
        b.send(Message::HomeSection(crate::home::Section::Recent));
        let ui = b.gui.home.as_mut().unwrap();
        ui.pages = crate::home::MAX_PAGES;
        ui.total = Some(u32::MAX);
        let _ = b.requests();
        let scroll = Message::HomeScrolled(crate::home::ScrollAt {
            content_h: 2000.0,
            view_h: 400.0,
            offset_y: 1600.0,
        });
        let _ = update(&mut b.gui, scroll.clone());
        assert_eq!(b.requests().len(), 1, "the scroll asked once");
        let _ = update(&mut b.gui, scroll);
        assert!(
            b.requests().is_empty(),
            "and not again while one is in flight"
        );
        // Still no pager anywhere on the focused screen.
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
            let mut ui = simulator(view::view(&b.gui));
            // The attackers wear the meter's captions, not a drill pane's.
            assert!(ui.find("Per sec").is_ok());
            assert!(ui.find("By ability").is_err());
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
        let mut ui = simulator(view::view(&b.gui));
        assert!(ui.find("Abilities").is_ok());
        assert!(ui.find("Targets").is_err());
    }

    /// The header's step buttons are `]` and `[` for the pointer: from the
    /// newest pull there is only older, and a step there and back again
    /// lands where it started.
    #[test]
    fn the_header_steps_walk_the_pulls() {
        let mut b = Bridge::new(MockDaemon::fixture());
        b.send(chr("m"));
        let newest = b.gui.state.segment_index();
        let head = crate::fight_head::Head::of(&b.gui, true);
        assert!(
            !head.newer && head.older,
            "the newest pull steps older only"
        );
        b.send(Message::OlderPull);
        assert_eq!(b.gui.state.segment_index() + 1, newest);
        let head = crate::fight_head::Head::of(&b.gui, true);
        assert!(head.newer, "and back");
        b.send(Message::NewerPull);
        assert_eq!(b.gui.state.segment_index(), newest);
    }

    /// The owner is whoever the config names — the locked guid, or a
    /// `history_characters` name whole or by its name half — and the "you"
    /// chip selects their row.
    #[test]
    fn the_you_chip_selects_the_owner_s_row() {
        let (state, _mock) = testkit::kill();
        let rows = state.rows();
        let (mut gui, _peer) = testkit::gui_over(state);
        assert_eq!(gui.owner_row(), None, "nobody configured, nobody owns it");
        let me = rows.len() - 1;
        let name = rows[me].label.split('-').next().unwrap().to_uppercase();
        gui.cfg.extra.insert(
            "history_characters".to_string(),
            toml::Value::String(format!("Somebody-Else-US, {name}")),
        );
        assert_eq!(gui.owner_row(), Some(me), "a bare name, any case");
        // The chrome's accent is found by the same matcher: a bare name
        // resolves the owner the chip already shows.
        gui.resolve_accent();
        assert_eq!(gui.owner_name(), Some(rows[me].label.as_str()));
        gui.accent_owner = None;
        gui.cfg.extra.clear();
        gui.adopt_owner_for_test(&rows[me].label);
        assert_eq!(gui.owner_row(), Some(me), "the name Home resolved");
        let (state, _mock) = testkit::kill();
        let (mut gui, _peer) = testkit::gui_over(state);
        gui.owner_guid = Some(rows[me].key.clone());
        assert_eq!(gui.owner_row(), Some(me), "the lock's guid");
        gui.state.row_sel = 0;
        let _ = update(&mut gui, Message::SelectOwner);
        assert_eq!(gui.state.row_sel, me);
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

    #[test]
    fn the_picker_menu_opens_picks_and_closes() {
        let mut b = home_bridge();
        b.send(chr("~"));
        b.send(Message::TogglePicker);
        assert!(b.gui.picker_open);
        // A pick locks and closes the menu in one gesture.
        let guid = b
            .gui
            .home
            .as_ref()
            .and_then(|h| h.cards.first())
            .and_then(|c| c.players.first())
            .map(|p| p.guid.clone());
        b.send(Message::HomeCharacter(guid.clone()));
        assert!(!b.gui.picker_open);
        assert_eq!(b.gui.owner_guid, guid);
        // Esc closes it and does nothing else: Home stays open.
        b.send(Message::TogglePicker);
        b.send(named(Named::Escape));
        assert!(!b.gui.picker_open);
        assert!(b.gui.home.is_some());
        // The characters panel is gone: the picker is the only chooser.
        let mut ui = simulator(view::view(&b.gui));
        assert!(ui.find("show the newest character").is_err());
    }

    #[test]
    fn the_character_chips_lock_the_window() {
        let mut b = home_bridge();
        b.send(chr("~"));
        // The mock store resolves no owner, so the pick is any player a
        // stored card lists — the lock does not care who named them.
        let guid = b
            .gui
            .home
            .as_ref()
            .and_then(|h| h.cards.first())
            .and_then(|c| c.players.first())
            .map(|p| p.guid.clone());
        assert!(
            guid.is_some(),
            "the fixture store has a stored fight with players"
        );
        b.send(Message::HomeCharacter(guid.clone()));
        assert_eq!(b.gui.home.as_ref().unwrap().character, guid);
        // The pick is the window's, not Home's: it is the owner History
        // opens on, it is remembered in the config, and the chrome is theirs.
        assert_eq!(b.gui.owner_guid, guid);
        assert_eq!(b.gui.cfg.character, guid);
        assert_eq!(
            b.gui.owner_name().map(str::to_string),
            Some(b.gui.home_panels.me.name.clone()),
            "the accent follows the pick"
        );
        b.send(chr("H"));
        let h = b.gui.history.as_ref().expect("H opens History");
        assert_eq!(h.character, guid, "History opens scoped to the lock");
        // The way out of the lock is the picker's menu, on this screen only.
        b.send(Message::TogglePicker);
        {
            let mut ui = simulator(view::view(&b.gui));
            assert!(ui.find("Everyone").is_ok(), "and offers the way out of it");
        }
        b.send(Message::TogglePicker);
        // Widening History never moves the lock.
        b.send(Message::HistoryCharacter(None));
        assert_eq!(b.gui.history.as_ref().unwrap().character, None);
        assert_eq!(b.gui.owner_guid, guid);
        // Reopening Home lands on the same character.
        b.send(chr("~"));
        assert_eq!(b.gui.home.as_ref().unwrap().character, guid);
        b.send(Message::HomeCharacter(None));
        assert_eq!(b.gui.home.as_ref().unwrap().character, None);
        assert_eq!(b.gui.cfg.character, None);
    }
}
