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

mod chrome;
mod deaths;
mod fight_head;
mod paint;
mod ribbon;
mod table;
mod tabs;
mod top_bar;
mod w;

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
use wowdps_gui_logic::table::{Col, filtered_indexed, meter_set};
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
    vec![KeyBinding::new(
        "escape",
        FilterDone,
        Some("Filter > Input"),
    )]
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
use top_bar::CharPick;
use w::{Fit, REGULAR, W};

/// The zoom's step and its ends (`window.rs` in iced: 0.1, 0.5..=3.0).
const ZOOM_STEP: f32 = 0.1;
const ZOOM_MIN: f32 = 0.5;
const ZOOM_MAX: f32 = 3.0;
/// The docked rail's width and the rule after it (`.body{grid-template-
/// columns:236px …}`).
const RAIL_W: f32 = 236.0;
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
        app_id: Some("wowdps-gui-new".to_string()),
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
    pub(crate) meter_scroll: ScrollHandle,
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
            meter_scroll: ScrollHandle::new(),
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
        let rows = state.rows();
        let owner = self.owner_of(&rows).and_then(|i| rows.get(i)).cloned();
        let mut seen = std::mem::take(&mut self.seen);
        seen.observe(state, owner.as_ref());
        self.seen = seen;
        if to_newest {
            self.session
                .update(cx, |s, cx| s.act(|st| st.pin_live(), cx));
        }
        if let Some(me) = owner {
            self.learn_class(&me, cx);
        }
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
        let state = self.session.read(cx).state();
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
            let state = self.session.read(cx).state();
            (state.rows(), state.view)
        };
        if let Some(i) = self.owner_in(&rows, view) {
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

    /// The header's ‹ › (and `[` `]`): the older or newer pull.
    pub(crate) fn step_pull(&mut self, older: bool, _window: &mut Window, cx: &mut Context<Self>) {
        self.place = Place::Fights;
        self.act(|s| s.apply(fight_head::step_action(older)), cx);
    }

    /// The header's rail button (step 3.4 draws the drawer).
    pub(crate) fn open_rail(&mut self, cx: &mut Context<Self>) {
        self.cards.rail = true;
        cx.notify();
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
        theme::apply(self.cfg.theme(), Some(class_accent(Some(class))), cx);
        if self.cfg.character_class() != Some(class) {
            Config::store_character_class(Some(class.name().to_string()));
        }
    }

    /// Who the picker names: the character played last — until the rail
    /// and Home read the store's pages (steps 3.4 and 3.5), the owner of
    /// the pull on the stage, as the owner marks find them.
    pub(crate) fn picked(&self, cx: &App) -> Option<CharPick> {
        let state = self.session.read(cx).state();
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

    fn act(
        &self,
        f: impl FnOnce(&mut wowdps_proto::ClientState) -> Vec<wowdps_proto::ClientMsg>,
        cx: &mut Context<Self>,
    ) {
        self.session.update(cx, |s, cx| s.act(f, cx));
    }

    /// A key of the shared keymap. Esc on the meter with nothing to back
    /// out of goes Home, as the iced window's chain ends; `q` quits.
    fn on_do(&mut self, action: Action, window: &mut Window, cx: &mut Context<Self>) {
        match action {
            Action::Quit => cx.quit(),
            Action::Back if self.session.read(cx).state().drill.is_none() => {
                self.goto_home(window, cx);
            }
            _ => {
                if self.place == Place::Home && !matches!(action, Action::SetView(_)) {
                    return;
                }
                self.place = Place::Fights;
                self.act(|s| s.apply(action), cx);
            }
        }
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
            Gesture::Jump => self.jump(window, cx),
            Gesture::Sheet => self.toggle_sheet(cx),
            Gesture::Earlier => {
                self.cards.rail = true;
                cx.notify();
            }
            Gesture::Filter => self.focus_filter(window, cx),
            Gesture::Pin => {}
        }
    }

    pub(crate) fn goto_home(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        self.place = Place::Home;
        cx.notify();
    }

    pub(crate) fn goto_fights(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        self.place = Place::Fights;
        cx.notify();
    }

    /// The command palette (step 3.6 draws it).
    pub(crate) fn jump(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        self.cards.palette = !self.cards.palette;
        cx.notify();
    }

    /// `m`, the live pill: the log's live pull on the stage.
    pub(crate) fn go_live(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        self.place = Place::Fights;
        self.cards.rail = false;
        self.act(
            |s| {
                s.uninspect();
                s.pin_live()
            },
            cx,
        );
    }

    pub(crate) fn toggle_options(&mut self, cx: &mut Context<Self>) {
        self.cards.options = !self.cards.options;
        cx.notify();
    }

    pub(crate) fn toggle_sheet(&mut self, cx: &mut Context<Self>) {
        self.cards.sheet = !self.cards.sheet;
        cx.notify();
    }

    pub(crate) fn toggle_picker(&mut self, cx: &mut Context<Self>) {
        self.cards.picker = !self.cards.picker;
        cx.notify();
    }

    /// `t`: the talent viewer on the selected row's player (or on nobody),
    /// asking the daemon for the build they wore in this pull.
    pub(crate) fn open_talents(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let state = self.session.read(cx).state();
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
        // A pull of the log: the daemon answers with the build the player
        // wore in it, which wins over a stored paste.
        let viewer = cx.new(|cx| {
            let mut viewer = TalentViewer::open(player.clone(), window, cx);
            if let Some(p) = &player {
                viewer.ask_loadout(&session, segment, p.guid.clone(), cx);
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

    /// The docked rail's seat (step 3.4 draws the rail): the panel and its
    /// rule, so the stage stands where it will.
    fn rail_seat(&self, w: &W) -> impl IntoElement {
        div()
            .id("rail")
            .test_support()
            .flex_none()
            .h_full()
            .flex()
            .child(
                div()
                    .w(w.z(RAIL_W))
                    .h_full()
                    .bg(w.c(|t| t.surface))
                    .pt(w.z(14.))
                    .px(w.z(16.))
                    .child(w.text("Pulls", w.size.place, w.c(|t| t.ink), w::SEMIBOLD)),
            )
            .child(chrome::vrule(w))
    }

    /// Home's seat (step 3.5 builds Home).
    fn home(&self, w: &W) -> impl IntoElement {
        div()
            .id("home")
            .flex_1()
            .min_w_0()
            .p(w.z(24.))
            .flex()
            .flex_col()
            .gap(w.z(8.))
            .child(w.title_text("You, this week", w.size.encounter_narrow, w.c(|t| t.ink)))
            .child(chrome::quiet(
                w,
                "Your week, from the history store.",
                w.size.body,
            ))
    }

    /// The stage (`.stage`): the fight header, the ribbon, the view tabs with
    /// the row filter at their end, and under them master and detail — the
    /// meter (or, on Deaths, the deaths in order) and beside it the
    /// inspector's seat (step 3.3 draws the inspector), which at 820 px and
    /// under is pushed over the whole stage while the keys are in it.
    fn stage(&self, w: &W, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let mut stage = div()
            .id("stage")
            .flex_1()
            .min_w_0()
            .h_full()
            .flex()
            .flex_col();
        let pushed = w.narrow() && self.session.read(cx).state().inspecting();
        if pushed {
            stage = stage.child(self.inspector_seat(w, None));
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
                Some(width) => body
                    .child(chrome::vrule(w))
                    .child(self.inspector_seat(w, Some(width))),
                None => body,
            });
        }
        stage.children(self.footer(w, cx))
    }

    /// The inspector's seat (step 3.3 draws the inspector): the panel the
    /// grid gives it — 520 px wide, 410 in a tile, the whole stage pushed.
    fn inspector_seat(&self, w: &W, width: Option<f32>) -> impl IntoElement {
        div()
            .id("inspector")
            .test_support()
            .h_full()
            .when_some(width, |d, width| d.w(w.z(width)).flex_none())
            .when(width.is_none(), |d| d.flex_1())
            .bg(w.c(|t| t.surface))
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
            // under it.
            .when(self.talents.is_none(), |d| d.key_context(keys::METER))
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
            .font_family(w.ui)
            .text_size(w.z(w.size.frame))
            .line_height(relative(1.3));
        // The talent viewer holds the whole window while it is open.
        if let Some(viewer) = &self.talents {
            return root.child(div().size_full().p(w.z(10.)).child(viewer.clone()));
        }
        let docked = w.fit() == Fit::Wide;
        let content = match self.place {
            Place::Home => self.home(&w).into_any_element(),
            Place::Fights => self.stage(&w, window, cx).into_any_element(),
        };
        root.child(top_bar::bar(self, &w, window, cx)).child(
            div()
                .id("body")
                .flex_1()
                .min_h_0()
                .flex()
                .when(docked, |d| d.child(self.rail_seat(&w)))
                .child(content),
        )
    }
}
