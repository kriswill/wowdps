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
mod paint;
mod top_bar;
mod w;

#[cfg(test)]
mod shots;

#[cfg(test)]
mod tests;

use gpui_kit::prelude::*;
use gpui_kit::{
    App, Bounds, Context, Entity, FocusHandle, Subscription, TestSupportExt as _, TitlebarOptions,
    Window, WindowBounds, WindowOptions, div, px, relative, size,
};
use wowdps_gui_logic::config::Config;
use wowdps_gui_logic::fight_head::{Seen, owner_among};
use wowdps_gui_logic::keys::Zoom;
use wowdps_gui_logic::theme::{Chrome, class_accent};
use wowdps_model::{Action, Row, Screen, View};
use wowdps_proto::DaemonClient;

use crate::keys::{self, Do, Gesture, Go, ZoomTo};
use crate::session::{Linked, Session};
use crate::talents::{Player, TalentEvent, TalentViewer};
use crate::theme;
use top_bar::Pick;
use w::{Fit, REGULAR, W};

/// The zoom's step and its ends (`window.rs` in iced: 0.1, 0.5..=3.0).
const ZOOM_STEP: f32 = 0.1;
const ZOOM_MIN: f32 = 0.5;
const ZOOM_MAX: f32 = 3.0;
/// The docked rail's width and the rule after it (`.body{grid-template-
/// columns:236px …}`).
const RAIL_W: f32 = 236.0;

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
    _subscriptions: Vec<Subscription>,
}

impl Gui {
    pub fn new(
        session: Entity<Session>,
        cfg: Config,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let changes = cx.observe(&session, |this, _, cx| this.on_session(cx));
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
            _subscriptions: vec![changes],
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
    pub(crate) fn picked(&self, cx: &App) -> Option<Pick> {
        let state = self.session.read(cx).state();
        let rows = state.rows();
        let me = self
            .owner_of(&rows)
            .and_then(|i| rows.get(i))
            .or_else(|| self.seen.of(state).and_then(Seen::owner))?;
        Some(Pick {
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
            Gesture::Filter | Gesture::Pin => {}
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

    /// The stage: the pull on show (3.2 builds its header, ribbon, tabs and
    /// tables; until then the step-1.2 meter stands in).
    fn stage(&self, w: &W, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .id("stage")
            .flex_1()
            .min_w_0()
            .h_full()
            .flex()
            .flex_col()
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .p(w.z(12.))
                    .child(crate::meter::meter(&self.session, cx)),
            )
            .children(self.footer(w, cx))
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
            Place::Fights => self.stage(&w, cx).into_any_element(),
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
