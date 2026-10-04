//! The window (plan phase 3; the iced window's `window.rs` and `view.rs`):
//! a top bar over a body — the pull rail beside (or, 1180 px and under,
//! over) either Home or the STAGE, a pull's fight header, ribbon, view
//! tabs, meter and the inspector beside it. `Gui` is the root entity: it
//! holds the tailed log's `Session` and everything window-local (the place
//! on show, the filter, the sort, the pointer, the open cards), and every
//! piece renders from it.
//!
//! Built in the redesign's order (`docs/plan-gui-new.md` phase 3): 3.1 the
//! Look — the frame, the top bar, the breakpoints, the zoom and the chrome
//! — and 3.2 the stage's header, ribbon, tabs and tables. The rail (3.4),
//! Home (3.5), the inspector (3.3) and the cards over everything (3.6) have
//! their seats here and are drawn by their own steps.

mod cards;
mod chrome;
mod deaths;
pub(crate) mod field;
mod fight_head;
mod history;
mod home;
mod inspector;
mod instruments;
mod paint;
mod palette;
mod rail;
mod ribbon;
mod table;
mod tabs;
mod top_bar;
pub(crate) mod w;

#[cfg(test)]
mod shots;

#[cfg(test)]
mod tests;

use std::cell::Cell;
use std::rc::Rc;

use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::prelude::*;
use gpui_kit::{
    App, Bounds, Context, Entity, FocusHandle, KeyBinding, Pixels, ScrollHandle, Subscription,
    TestSupportExt as _, TitlebarOptions, Window, WindowBounds, WindowOptions, div, px, relative,
    size,
};
use wowdps_gui_logic::config::Config;
use wowdps_gui_logic::deaths::{Pick, any_mine, owner_rows};
use wowdps_gui_logic::fight_head::{Seen, owner_among};
use wowdps_gui_logic::keys::Zoom;
use wowdps_gui_logic::table::{Col, filtered_indexed, meter_set, meter_step, step_in};
use wowdps_gui_logic::theme::{Chrome, class_accent};
use wowdps_model::{Action, RaidTimeline, Row, Screen, View};
use wowdps_proto::DaemonClient;

/// Esc or Enter in the row filter: done filtering, the keys back on the
/// meter (Esc clears the text first).
#[derive(Clone, Debug, PartialEq, gpui_kit::Action)]
#[action(namespace = wowdps, no_json)]
pub struct FilterDone;

/// The filter's own keys: Esc in it is the window's, not the field's.
pub fn bindings() -> Vec<KeyBinding> {
    let mut keys = vec![KeyBinding::new(
        "escape",
        FilterDone,
        Some("Filter > Input"),
    )];
    keys.extend(palette::bindings());
    keys
}

/// A line the stage's list must bring into sight on its next layout,
/// scrolling the least that shows it whole: a meter row by its key, a
/// death by its player and window. Set by a key step, the "you" chip and
/// an opened death, never by a click on a row already in sight.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Reveal {
    Row(String),
    Death(String, u32),
}

/// Where `Up`/`Down` land on the stage when the state machine's own step
/// is not what is drawn (the iced window's `Step`, its meter half).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Step {
    /// A meter row, by the daemon's index.
    Meter(usize),
    /// A death of the raid timeline, by its place in it.
    Death(usize),
    /// Nowhere drawn to land: the key does nothing.
    Stay,
}

/// Where the pointer is on the stage's lists: drawn, never sent anywhere.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RowHover {
    /// A meter row, by the daemon's index.
    Meter(usize),
    /// A death of the Deaths table, by its place in the raid timeline.
    Death(usize),
}

use crate::keys::{self, Do, Gesture, Go, ZoomTo};
use crate::session::{Linked, Session};
use crate::talents::{Player, TalentEvent, TalentViewer};
use crate::theme;
use inspector::view::Seat;
use top_bar::CharPick;
use w::{Fit, REGULAR, W};

/// The zoom's step and its ends (`window.rs` in iced: 0.1, 0.5..=3.0).
const ZOOM_STEP: f32 = 0.1;
const ZOOM_MIN: f32 = 0.5;
const ZOOM_MAX: f32 = 3.0;
/// The inspector beside the meter: the most of the prototype's `minmax`,
/// which the grid always gives it — 520 wide, 410 in a tile.
const INSPECTOR_WIDE: f32 = 520.0;
const INSPECTOR_TILE: f32 = 410.0;

pub fn open(client: DaemonClient, cx: &mut App) -> Result<(), String> {
    // A narrow launch, as the iced window opens (460 × 640, at least
    // 320 × 240).
    let bounds = Bounds::centered(None, size(px(460.), px(640.)), cx);
    let options = WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(bounds)),
        window_min_size: Some(size(px(320.), px(240.))),
        titlebar: Some(TitlebarOptions {
            title: Some("wowdps".into()),
            ..Default::default()
        }),
        app_id: Some("wowdps-gui".to_string()),
        ..Default::default()
    };
    gpui_kit::open_window(options, cx, |window, cx| {
        let session = cx.new(|cx| Session::running(client, cx));
        let gui = cx.new(|cx| Gui::new(session, Config::load(), window, cx));
        let focus = gui.read(cx).focus.clone();
        window.focus(&focus, cx);
        gui
    })
    .map(|_| ())
    .map_err(|e| format!("cannot open the window: {e}"))
}

/// The place on show: the top bar's two.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Place {
    Home,
    Fights,
}

/// The cards over everything (step 3.6 draws them): which one is open.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Cards {
    pub options: bool,
    pub sheet: bool,
    pub palette: bool,
    pub picker: bool,
    /// The rail's drawer, at 1180 px and under (step 3.4 draws it).
    pub rail: bool,
}

pub struct Gui {
    pub(crate) session: Entity<Session>,
    focus: FocusHandle,
    pub(crate) cfg: Config,
    pub(crate) place: Place,
    pub(crate) cards: Cards,
    /// What the window has seen of the owner on the fight's other views.
    pub(crate) seen: Seen,
    /// The class the chrome was learned from, once it was.
    learned: bool,
    /// The talent viewer, while it is open: it holds the whole window.
    pub(crate) talents: Option<Entity<TalentViewer>>,
    /// The window's width at zoom 1, as the last frame laid it out.
    pub(crate) width: f32,
    /// The row filter's field and its text (what the meter narrows by).
    pub(crate) filter: Entity<InputState>,
    pub(crate) filter_text: String,
    /// The meter's sort: a column and descending; `None` the daemon's
    /// order.
    pub(crate) sort: Option<(Col, bool)>,
    pub(crate) row_hover: Option<RowHover>,
    /// The pointer over the ribbon, and the ribbon's bounds at its last
    /// paint.
    pub(crate) ribbon_hover: Option<ribbon::Hover>,
    pub(crate) ribbon_bounds: Rc<Cell<Bounds<Pixels>>>,
    /// The view tabs' strip, and what its active tab was last revealed
    /// for (the view and the window's width).
    pub(crate) tab_scroll: ScrollHandle,
    pub(crate) tab_revealed: Cell<Option<(View, i64)>>,
    /// The meter's (or the Deaths table's) list.
    pub(crate) meter_scroll: crate::scrollbar::Scroll,
    /// The inspector's own state, and this frame's inspector.
    pub(crate) insp: inspector::model::InspState,
    /// The command palette, while it is up.
    pub(crate) pal: Option<palette::PalState>,
    insp_frame: Option<inspector::model::Insp>,
    /// What that list brings into sight on its next layout.
    pub(crate) reveal: Cell<Option<Reveal>>,
    /// What the cards keep: the toast up, the pin it spoke for, the sheet's
    /// scroll (step 3.6).
    pub(crate) cards_ui: cards::CardsUi,
    /// The rail's and Home's reads of the history store (steps 3.4, 3.5).
    pub(crate) hist: history::Hist,
    _subscriptions: Vec<Subscription>,
}

impl Gui {
    pub fn new(
        session: Entity<Session>,
        cfg: Config,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let changes = cx.observe(&session, |this, _, cx| this.on_session(cx));
        let filter = cx.new(|cx| InputState::new(window, cx).placeholder("Filter players"));
        let typed = cx.subscribe_in(&filter, window, |this, state, event, window, cx| {
            match event {
                InputEvent::Change => {
                    let text = state.read(cx).value().to_string();
                    this.set_filter(text, cx);
                }
                // Enter keeps the text and gives the keys back.
                InputEvent::PressEnter { .. } => {
                    window.focus(&this.focus, cx);
                    cx.notify();
                }
                InputEvent::Focus | InputEvent::Blur => cx.notify(),
            }
        });
        // Master and detail: the selection is the drill (the TUI never opts
        // in), and the window opens on the log's newest pull.
        session.update(cx, |s, cx| {
            s.act(
                |state| {
                    let mut reqs = state.set_follow(true);
                    reqs.extend(state.pin_live());
                    reqs
                },
                cx,
            )
        });
        let hist = history::Hist::new(&cfg, &session, cx);
        Self {
            session,
            focus: cx.focus_handle(),
            cfg,
            place: Place::Fights,
            cards: Cards::default(),
            seen: Seen::default(),
            learned: false,
            talents: None,
            width: 0.0,
            filter,
            filter_text: String::new(),
            sort: None,
            row_hover: None,
            ribbon_hover: None,
            ribbon_bounds: Rc::new(Cell::new(Bounds::default())),
            tab_scroll: ScrollHandle::new(),
            tab_revealed: Cell::new(None),
            meter_scroll: crate::scrollbar::Scroll::new(),
            insp: inspector::model::InspState::new(),
            pal: None,
            insp_frame: None,
            reveal: Cell::new(None),
            cards_ui: cards::CardsUi::default(),
            hist,
            _subscriptions: vec![changes, typed],
        }
    }

    #[cfg(test)]
    pub fn session(&self) -> &Entity<Session> {
        &self.session
    }

    /// The keys' target: the meter's key context lives on the root.
    #[cfg(test)]
    pub fn focus(&self) -> &FocusHandle {
        &self.focus
    }

    /// Whatever the session said: keep what other views told of the owner,
    /// learn the chrome, and land on a pull when the log's list arrives.
    fn on_session(&mut self, cx: &mut Context<Self>) {
        let state = self.session.read(cx).state();
        // The window never shows the list screen: with no pull on the stage
        // the log's newest takes it.
        let to_newest = state.screen == Screen::List && state.segment_count() > 0;
        // The pull the stage draws: a stored one's own state, else the log's.
        let state = history::fight_of(&self.hist, &self.session, cx);
        let rows = state.rows();
        let owner = self.owner_of(&rows).and_then(|i| rows.get(i)).cloned();
        let mut seen = std::mem::take(&mut self.seen);
        seen.observe(state, owner.as_ref());
        self.seen = seen;
        self.inspector_learns(cx);
        if to_newest {
            self.session
                .update(cx, |s, cx| s.act(|st| st.pin_live(), cx));
        }
        if let Some(me) = owner {
            self.learn_class(&me, cx);
        }
        self.pin_watch(cx);
        cx.notify();
    }

    /// The owner's row among `rows` — whose window it is: the row the
    /// daemon marked `mine`, else the configured `history_characters`.
    /// The owner's row on the chart on screen — none on the Enemies view,
    /// whose rows are the enemies.
    pub(crate) fn owner_in(&self, rows: &[Row], view: View) -> Option<usize> {
        if view == View::EnemyTaken {
            return None;
        }
        self.owner_of(rows)
    }

    /// The reader's own guid among a raid timeline's deaths when the daemon
    /// marked none of them: found as the meter's "you" is, so the skull,
    /// the table, the header and the meter agree.
    pub(crate) fn death_owner(&self, raid: &RaidTimeline) -> Option<String> {
        if any_mine(raid) {
            return None;
        }
        let rows = owner_rows(raid);
        self.owner_of(&rows)
            .and_then(|i| rows.get(i))
            .map(|r| r.key.clone())
    }

    /// The meter's sort, when the view's table has its column: another
    /// view's choice is the daemon's order here.
    pub(crate) fn meter_sort(&self, view: View, narrow: bool) -> Option<(Col, bool)> {
        self.sort
            .filter(|(c, _)| meter_set(view, narrow).contains(c))
    }

    /// A heading's press: descending, then ascending, then the daemon's
    /// order again; another column starts descending.
    pub(crate) fn sort_by(&mut self, col: Col, cx: &mut Context<Self>) {
        self.sort = match self.sort {
            Some((c, true)) if c == col => Some((col, false)),
            Some((c, false)) if c == col => None,
            _ => Some((col, true)),
        };
        cx.notify();
    }

    /// The filter's text changed: what is drawn narrows, and a selection
    /// the filter hides moves to the first row drawn.
    fn set_filter(&mut self, text: String, cx: &mut Context<Self>) {
        self.filter_text = text;
        let state = self.fight(cx);
        let drawn = filtered_indexed(state.rows(), &self.filter_text);
        let sel = state.row_sel;
        if !drawn.is_empty()
            && !drawn.iter().any(|(i, _)| *i == sel)
            && let Some((first, _)) = drawn.first()
        {
            let first = *first;
            self.act(|s| s.select_row(first), cx);
        }
        cx.notify();
    }

    /// `/`: the keys to the row filter.
    fn focus_filter(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.place = Place::Fights;
        self.act(
            |s| {
                s.uninspect();
                Vec::new()
            },
            cx,
        );
        self.filter.update(cx, |f, cx| f.focus(window, cx));
        cx.notify();
    }

    /// Esc in the filter: its text cleared and the keys back on the meter.
    pub(crate) fn filter_done(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.clear_filter(window, cx);
        window.focus(&self.focus, cx);
    }

    /// The filter's clear mark.
    pub(crate) fn clear_filter(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.filter.update(cx, |f, cx| f.set_value("", window, cx));
        self.set_filter(String::new(), cx);
    }

    /// A view tab's press, as its key: the view switches, Home stands
    /// aside.
    pub(crate) fn pick_view(&mut self, view: View, cx: &mut Context<Self>) {
        self.place = Place::Fights;
        self.act(|s| s.apply(Action::SetView(view)), cx);
    }

    /// A meter row's press: the row is the selection and the inspector
    /// follows it; in a narrow window, where the inspector is not beside
    /// the meter, the press pushes it too.
    pub(crate) fn press_row(&mut self, i: usize, window: &mut Window, cx: &mut Context<Self>) {
        let narrow = w::Fit::of(self.width) == Fit::Narrow;
        self.act(
            |s| {
                let reqs = s.select_row(i);
                if narrow {
                    s.inspect();
                }
                reqs
            },
            cx,
        );
        window.focus(&self.focus, cx);
    }

    /// A class disc's press: the player picked for a comparison (`v`).
    pub(crate) fn pick_compare(&mut self, i: usize, cx: &mut Context<Self>) {
        if self.refuse(Action::PickCompare, cx) {
            return;
        }
        self.act(
            |s| {
                let mut reqs = s.select_row(i);
                reqs.extend(s.apply(Action::PickCompare));
                reqs
            },
            cx,
        );
    }

    /// The "you" chip's press: the owner's row selected.
    pub(crate) fn select_owner(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let (rows, view) = {
            let state = self.fight(cx);
            (state.rows(), state.view)
        };
        if let Some(i) = self.owner_in(&rows, view) {
            // Into view, the least that shows them whole.
            let key = rows.get(i).map(|r| r.key.clone());
            self.reveal.set(key.map(Reveal::Row));
            // A filter that hides the owner gives way to them: the field
            // too, which keeps its own text.
            let hidden = !filtered_indexed(rows, &self.filter_text)
                .iter()
                .any(|(j, _)| *j == i);
            if hidden {
                self.filter.update(cx, |f, cx| f.set_value("", window, cx));
                self.filter_text.clear();
            }
            self.act(|s| s.select_row(i), cx);
        }
    }

    /// The header's ‹ › (and `[` `]`): the older or newer pull along the
    /// rail, stored nights included — past the last card in hand, `[` asks
    /// for the next page.
    pub(crate) fn step_pull(&mut self, older: bool, _window: &mut Window, cx: &mut Context<Self>) {
        self.rail_step(older, cx);
    }

    /// The header's rail button: the drawer, on the pull on the stage.
    pub(crate) fn open_rail(&mut self, cx: &mut Context<Self>) {
        self.open_drawer(cx);
    }

    /// A death's press — a skull on the ribbon, a line of the Deaths table:
    /// the Deaths view drilled into that death window, pushed in a narrow
    /// window.
    pub(crate) fn open_death(&mut self, pick: Pick, window: &mut Window, cx: &mut Context<Self>) {
        let narrow = Fit::of(self.width) == Fit::Narrow;
        self.place = Place::Fights;
        self.act(
            |s| {
                let reqs = s.open_death(&pick.key, &pick.label, pick.index);
                if narrow {
                    s.inspect();
                }
                reqs
            },
            cx,
        );
        self.reveal.set(Some(Reveal::Death(pick.key, pick.index)));
        window.focus(&self.focus, cx);
    }

    pub(crate) fn owner_of(&self, rows: &[Row]) -> Option<usize> {
        rows.iter()
            .position(|r| r.mine && !r.enemy)
            .or_else(|| owner_among(rows, None, &self.cfg.history_characters()))
    }

    /// A class chrome wears the owner's class, learned once and held — a
    /// selection never re-colours the window — and remembered for the next
    /// launch's first frame.
    fn learn_class(&mut self, me: &Row, cx: &mut Context<Self>) {
        if self.learned || self.cfg.chrome() != Chrome::Class {
            return;
        }
        let Some(class) = me.class else {
            return;
        };
        self.learned = true;
        let def = self.theme_def(cx);
        theme::apply(&def, Some(class_accent(class)), cx);
        if self.cfg.character_class() != Some(class) {
            Config::store_character_class(Some(class.name().to_string()));
        }
    }

    /// Who the picker names: the character picked from its menu or Home's
    /// chips (the config's `character`, Home's scope) once the window knows
    /// them, else the character played last. (iced's picker names only the
    /// latter, and read as stuck on a pick.)
    pub(crate) fn picked(&self, cx: &App) -> Option<CharPick> {
        let pick = self
            .cfg
            .character
            .as_deref()
            .and_then(|guid| self.hist.known.iter().find(|c| c.guid == guid));
        match pick {
            Some(c) => Some(CharPick {
                guid: c.guid.clone(),
                name: c.name.clone(),
                class: c.class,
                spec: c.spec,
            }),
            None => self.played(cx),
        }
    }

    /// Whose window it is, whatever is picked: the character played last,
    /// as the store's newest card names them (Home's answers, else the
    /// rail's pages) — before the store has said, the owner of the pull on
    /// the stage, as the owner marks find them.
    pub(crate) fn played(&self, cx: &App) -> Option<CharPick> {
        if let Some(c) = self.hist.owner() {
            return Some(CharPick {
                guid: c.guid,
                name: c.name,
                class: c.class,
                spec: c.spec,
            });
        }
        let state = self.fight(cx);
        let rows = state.rows();
        let me = self
            .owner_of(&rows)
            .and_then(|i| rows.get(i))
            .or_else(|| self.seen.of(state).and_then(Seen::owner))?;
        Some(CharPick {
            guid: me.key.clone(),
            name: me.label.clone(),
            class: me.class,
            spec: me.spec,
        })
    }

    /// Act on the pull on the stage: a stored pull's own state (whose asks
    /// become the `GetFight` that answers them), else the log's.
    fn act(
        &mut self,
        f: impl FnOnce(&mut wowdps_proto::ClientState) -> Vec<wowdps_proto::ClientMsg>,
        cx: &mut Context<Self>,
    ) {
        self.act_fight(f, cx);
        self.pin_watch(cx);
    }

    /// A key of the shared keymap. Esc on the meter with nothing to back
    /// out of goes Home, as the iced window's chain ends; `q` quits.
    fn on_do(&mut self, action: Action, window: &mut Window, cx: &mut Context<Self>) {
        // Esc walks one level up, in the iced window's order (step 3.6).
        if action == Action::Back && self.escape(window, cx) {
            return;
        }
        // The rail's drawer, while it is open, has the keys first; `[` `]`
        // walk the rail anywhere (step 3.4).
        if self.rail_key(action, cx) {
            return;
        }
        match action {
            Action::Quit => cx.quit(),
            _ => {
                if self.place == Place::Home && !matches!(action, Action::SetView(_)) {
                    return;
                }
                // What a stored pull keeps no answer for says so.
                if self.refuse(action, cx) {
                    return;
                }
                self.place = Place::Fights;
                if self.inspector_key(action, cx) {
                    self.reveal_insp(cx);
                    return;
                }
                match self.step(action, cx) {
                    Some(Step::Meter(row)) => self.act(|s| s.select_row(row), cx),
                    Some(Step::Death(i)) => {
                        let pick = self
                            .fight(cx)
                            .raid()
                            .and_then(|r| r.deaths.get(i))
                            .map(Pick::of);
                        if let Some(p) = pick {
                            self.act(|s| s.open_death(&p.key, &p.label, p.index), cx);
                            self.reveal.set(Some(Reveal::Death(p.key, p.index)));
                        }
                        return;
                    }
                    Some(Step::Stay) => return,
                    None => self.act(|s| s.apply(action), cx),
                }
                // A step past the fold brings the list with it.
                let state = self.fight(cx);
                if matches!(action, Action::Up | Action::Down | Action::Open) && state.inspecting()
                {
                    // The inspector's, once the keys are there.
                    self.reveal_insp(cx);
                } else if matches!(action, Action::Up | Action::Down) {
                    let key = state.rows().get(state.row_sel).map(|r| r.key.clone());
                    self.reveal.set(key.map(Reveal::Row));
                }
            }
        }
    }

    /// Where `Up`/`Down` land when what is drawn is not the daemon's list
    /// in its order: the Deaths table walks the deaths in the order they
    /// happened, and a filtered or sorted meter its drawn rows. `None` when
    /// the state machine's own step is right.
    fn step(&self, action: Action, cx: &App) -> Option<Step> {
        if !matches!(action, Action::Up | Action::Down) {
            return None;
        }
        let app = self.fight(cx);
        // The inspector's keys are its own (step 3.3).
        if app.screen == Screen::List || (app.screen == Screen::Meter && app.inspecting()) {
            return None;
        }
        if app.screen == Screen::Meter
            && app.view == View::Deaths
            && let Some(raid) = app.raid()
        {
            let order = wowdps_gui_logic::deaths::drawn(raid, &self.filter_text);
            let sel = wowdps_gui_logic::deaths::selected(app, raid).unwrap_or(usize::MAX);
            // A filter that hides every death leaves nowhere to land.
            return Some(step_in(&order, sel, action).map_or(Step::Stay, Step::Death));
        }
        let narrow = Fit::of(self.width) == Fit::Narrow;
        meter_step(
            app.rows(),
            &self.filter_text,
            self.meter_sort(app.view, narrow),
            app.row_sel,
            action,
        )
        .map(Step::Meter)
    }

    fn on_zoom(&mut self, zoom: Zoom, cx: &mut Context<Self>) {
        let z = match zoom {
            Zoom::In => self.cfg.zoom + ZOOM_STEP,
            Zoom::Out => self.cfg.zoom - ZOOM_STEP,
            Zoom::Reset => Config::default().zoom,
        }
        .clamp(ZOOM_MIN, ZOOM_MAX);
        self.cfg.zoom = z;
        // One key, over the file as it is now: an overlay drag saved since
        // launch survives (the iced window's `save` did not, D2).
        Config::store(|c| c.zoom = z);
        cx.notify();
    }

    fn on_go(&mut self, gesture: Gesture, window: &mut Window, cx: &mut Context<Self>) {
        match gesture {
            Gesture::Talents => self.open_talents(window, cx),
            Gesture::Live => self.go_live(window, cx),
            Gesture::Home => {
                if self.place == Place::Home {
                    self.goto_fights(window, cx);
                } else {
                    self.goto_home(window, cx);
                }
            }
            Gesture::Jump => {
                // From a menu too: the palette replaces what was up.
                self.close_menus(cx);
                self.jump(window, cx);
            }
            Gesture::Sheet => self.toggle_menu(cards::Menu::Sheet, window, cx),
            Gesture::Earlier => self.open_earlier(cx),
            Gesture::Filter => self.focus_filter(window, cx),
            Gesture::Pin => self.pin(cx),
            Gesture::Wide => {
                if self.place == Place::Fights {
                    self.toggle_wide(cx);
                }
            }
        }
    }

    pub(crate) fn goto_home(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        self.open_home(cx);
    }

    pub(crate) fn goto_fights(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        self.leave_home();
        cx.notify();
    }

    /// Ctrl K, the jump box: the command palette, or shut it.
    pub(crate) fn jump(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.toggle_palette(window, cx);
    }

    /// `m`, the live pill: the log's live pull on the stage.
    pub(crate) fn go_live(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        self.leave_home();
        self.cards.rail = false;
        // Off a stored pull: the view the log was on before it.
        let view = self.leave_stored();
        self.act(
            |s| {
                s.uninspect();
                if let Some(v) = view {
                    s.view = v;
                }
                s.pin_live()
            },
            cx,
        );
    }

    /// `t`: the talent viewer on the selected row's player (or on nobody),
    /// asking the daemon for the build they wore in this pull.
    pub(crate) fn open_talents(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let state = self.fight(cx);
        let row = (state.screen != Screen::List && state.view != View::EnemyTaken)
            .then(|| state.rows().into_iter().nth(state.row_sel))
            .flatten()
            .filter(|r| !r.enemy);
        let segment = state.watched_segment();
        let player = row.map(|r| Player {
            name: r.label.clone(),
            spec_id: r.spec.map(|s| s.id()),
            guid: r.key.clone(),
        });
        let session = self.session.clone();
        // A stored pull's answer carried its players' builds already.
        let stored = self
            .hist
            .store
            .stored
            .as_ref()
            .map(|s| player.as_ref().and_then(|p| s.loadout_of(&p.guid)).cloned());
        // A pull of the log: the daemon answers with the build the player
        // wore in it, which wins over a stored paste.
        let viewer = cx.new(|cx| {
            let mut viewer = TalentViewer::open(player.clone(), window, cx);
            match (&stored, &player) {
                (Some(Some(loadout)), _) => viewer.adopt_logged(loadout, cx),
                (Some(None), _) | (None, None) => {}
                (None, Some(p)) => viewer.ask_loadout(&session, segment, p.guid.clone(), cx),
            }
            viewer
        });
        let closed = cx.subscribe_in(&viewer, window, |this, _, event, window, cx| match event {
            TalentEvent::Close => this.close_talents(window, cx),
        });
        self._subscriptions.push(closed);
        let focus = viewer.read(cx).focus_handle().clone();
        window.focus(&focus, cx);
        self.talents = Some(viewer);
        cx.notify();
    }

    /// The viewer closes: the keys come back to the meter.
    fn close_talents(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.talents = None;
        window.focus(&self.focus, cx);
        cx.notify();
    }

    /// The docked rail's seat: the rail and the rule after it.
    fn rail_seat(&mut self, w: &W, cx: &mut Context<Self>) -> gpui_kit::Div {
        div()
            .flex_none()
            .h_full()
            .flex()
            .child(rail::panel(self, w, rail::RAIL_W, false, cx))
            .child(chrome::vrule(w))
    }

    /// Home's seat: the whole body beside the rail.
    fn home(&mut self, w: &W, cx: &mut Context<Self>) -> gpui_kit::AnyElement {
        home::view(self, w, cx).into_any_element()
    }

    /// The stage (`.stage`): the fight header, the ribbon, the view tabs with
    /// the row filter at their end, and under them master and detail — the
    /// meter (or, on Deaths, the deaths in order) and beside it the
    /// inspector's seat (step 3.3 draws the inspector), which at 820 px and
    /// under is pushed over the whole stage while the keys are in it.
    fn stage(&self, w: &W, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let mut stage = div()
            .id("stage")
            .relative()
            .flex_1()
            .min_w_0()
            .h_full()
            .flex()
            .flex_col();
        let pushed = w.narrow() && self.fight(cx).inspecting();
        if pushed {
            stage = stage.child(self.inspector_seat(w, Seat::Pushed, window, cx));
        } else {
            let head = fight_head::Head::of(self, w, cx);
            stage = stage.child(fight_head::view(head, w, cx));
            if let Some(r) = ribbon::Ribbon::of(self, cx) {
                stage = stage.child(ribbon::view(
                    r,
                    self.ribbon_hover,
                    Rc::clone(&self.ribbon_bounds),
                    w,
                    cx,
                ));
            }
            stage = stage.child(tabs::view(self, w, window, cx));
            // Widened (the inspector's corner button, `f`): the whole width
            // under the tabs is the inspector's, the meter set aside — its
            // keys still walk the players.
            if self.widened(w) {
                return stage
                    .child(
                        div()
                            .id("stage-body")
                            .flex_1()
                            .min_h_0()
                            .w_full()
                            .flex()
                            .child(self.inspector_seat(w, Seat::Wide, window, cx)),
                    )
                    .children(self.footer(w, cx))
                    .children(cards::toast(self, w, window, cx));
            }
            let table = match deaths::Table::of(self, cx) {
                Some(t) => deaths::view(t, self, w, cx).into_any_element(),
                None => {
                    let m = table::Meter::of(self, w, cx);
                    table::view(m, self, w, window, cx).into_any_element()
                }
            };
            let beside = match w.fit() {
                Fit::Narrow => None,
                Fit::Tile => Some(INSPECTOR_TILE),
                Fit::Wide => Some(INSPECTOR_WIDE),
            };
            let body = div()
                .id("stage-body")
                .flex_1()
                .min_h_0()
                .w_full()
                .flex()
                .child(div().flex_1().min_w_0().h_full().child(table));
            stage = stage.child(match beside {
                Some(width) => body.child(chrome::vrule(w)).child(self.inspector_seat(
                    w,
                    Seat::Beside(width),
                    window,
                    cx,
                )),
                None => body,
            });
        }
        // A passing word over the stage's foot (step 3.6).
        stage
            .children(self.footer(w, cx))
            .children(cards::toast(self, w, window, cx))
    }

    /// The inspector's seat: the panel the grid gives it — 520 px wide,
    /// 410 in a tile, the whole stage pushed, the whole width under the
    /// tabs widened — and the inspector in it.
    fn inspector_seat(
        &self,
        w: &W,
        seat: Seat,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let width = match seat {
            Seat::Beside(width) => Some(width),
            Seat::Pushed | Seat::Wide => None,
        };
        let body = self
            .insp_frame
            .as_ref()
            .map(|insp| inspector::view::view(insp, self, w, seat, window, cx));
        div()
            .id("inspector")
            .test_support()
            .h_full()
            .when_some(width, |d, width| d.w(w.z(width)).flex_none())
            .when(width.is_none(), |d| d.flex_1().min_w_0())
            .bg(w.c(|t| t.surface))
            .children(body)
    }

    /// The footer: the daemon's status line when it has something to say,
    /// and nothing otherwise — not an empty line.
    fn footer(&self, w: &W, cx: &App) -> Option<impl IntoElement> {
        let session = self.session.read(cx);
        let status = match session.linked() {
            Linked::Down(why) => Some(format!("daemon gone — {why}")),
            Linked::Up => session.state().status.clone(),
        };
        let status = status.filter(|s| !s.trim().is_empty())?;
        Some(
            div()
                .id("footer")
                .flex_none()
                .px(w.z(10.))
                .pb(w.z(6.))
                .pt(w.z(6.))
                .child(w.text(status, w.size.small, w.c(|t| t.bad), REGULAR)),
        )
    }
}

impl Render for Gui {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let zoom = self.cfg.zoom;
        self.width = f32::from(window.viewport_size().width) / zoom;
        let w = W::new(zoom, self.width, cx);
        let root = div()
            .id("window")
            .track_focus(&self.focus)
            // The meter's keys live here — except while the talent viewer holds
            // the window: its keys are its own, and none of the meter's fire
            // under it — and while a menu is up, whose key is any key: it
            // closes the menu and does nothing else (step 3.6).
            .when(self.talents.is_none() && !self.modal(), |d| {
                d.key_context(keys::METER)
            })
            .when(self.talents.is_none() && self.modal(), |d| {
                d.key_context(keys::MODAL)
                    .on_key_down(cx.listener(Self::menu_key))
            })
            .on_action(
                cx.listener(|this, Do(action): &Do, window, cx| this.on_do(*action, window, cx)),
            )
            .on_action(cx.listener(|this, ZoomTo(zoom): &ZoomTo, _, cx| this.on_zoom(*zoom, cx)))
            .on_action(
                cx.listener(|this, Go(gesture): &Go, window, cx| this.on_go(*gesture, window, cx)),
            )
            .size_full()
            .flex()
            .flex_col()
            .bg(w.c(|t| t.ground))
            .text_color(w.c(|t| t.ink))
            .font_family(w.ui.clone())
            .text_size(w.z(w.size.frame))
            .line_height(relative(1.3));
        // The talent viewer holds the whole window while it is open.
        if let Some(viewer) = &self.talents {
            return root.child(div().size_full().p(w.z(10.)).child(viewer.clone()));
        }
        // The inspector, built once a frame; the last player's body taken to
        // stand in while the next one's breakdown is on its way.
        self.insp_frame = (self.place == Place::Fights).then(|| self.insp(&w, cx));
        if let Some(insp) = &self.insp_frame {
            let wide = insp.wide;
            let app = self.fight(cx);
            if app.drill_breakdown().is_some()
                && self
                    .insp
                    .held
                    .as_ref()
                    .is_none_or(|h| !h.current(app, wide))
                && let Some(held) = inspector::model::Held::of(insp, app)
            {
                self.insp.held = Some(held);
            }
        }
        let docked = w.fit() == Fit::Wide;
        let content = match self.place {
            Place::Home => self.home(&w, cx),
            Place::Fights => self.stage(&w, window, cx).into_any_element(),
        };
        let seat = if docked {
            Some(self.rail_seat(&w, cx))
        } else {
            None
        };
        // At 1180 px and under the rail is a drawer over a scrim.
        let drawer = if self.cards.rail && !docked {
            Some(rail::drawer(self, &w, cx))
        } else {
            None
        };
        // The menu up, over everything (step 3.6).
        let menu = if self.cards.sheet {
            Some(cards::sheet(self, &w, window, cx))
        } else if self.cards.options {
            Some(cards::options(self, &w, window, cx))
        } else if self.cards.picker {
            Some(cards::menu(self, &w, window, cx))
        } else {
            None
        };
        // The palette goes over everything, the menus and the top bar included.
        let palette = palette::view(self, &w, cx);
        root.relative()
            .child(top_bar::bar(self, &w, window, cx))
            .child(
                div()
                    .id("body")
                    .relative()
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .children(seat)
                    .child(content)
                    .children(drawer),
            )
            .children(menu)
            .children(palette)
    }
}
