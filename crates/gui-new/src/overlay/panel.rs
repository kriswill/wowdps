//! The overlay's view (plan step 2.1): the collapsed tab, and the panel —
//! header, meter rows, footer — with its two cards, the options card and
//! the view menu. Drawn as the iced overlay draws them
//! (`overlay-anatomy.md` §3), from the overlay palette and at its zoom.
//!
//! The overlay has no keyboard: every gesture is the pointer's. Its state
//! lives in its `Session`'s `ClientState` (what is watched, the selection,
//! the drill) and in this entity (what the pointer is over, which card is
//! open, whether the panel is expanded).

use std::collections::HashSet;
use std::rc::Rc;
use std::time::{Duration, Instant};

use gpui_kit::prelude::*;
use gpui_kit::{
    AnyElement, App, Context, Div, ElementId, Entity, MouseButton, ScrollWheelEvent, SharedString,
    Subscription, TestSupportExt as _, Window, div, px, relative,
};
use wowdps_gui_logic::config::Config;
use wowdps_gui_logic::labels::{self, Tone};

use wowdps_gui_logic::table::enemy_split;
use wowdps_gui_logic::timeline;
use wowdps_model::fmt::{duration, view_name};
use wowdps_model::{Action, Screen, SegmentId, View};
use wowdps_proto::{ClientMsg, ClientState};

use super::ov::Ov;
use super::rows::{self, class_icon, enemy_icon, meter_row, rank_cell, team_divider};
use super::{drill, graph, instance};
use crate::ease::bar_frac;
use crate::session::{Session, SessionEvent};

/// One revolution of the staleness radar's hand.
const RADAR_PERIOD: Duration = Duration::from_millis(2500);
/// How often the radar redraws while it shows: smooth enough to read as a
/// sweep, and every frame of it redraws the whole panel.
const RADAR_FRAME: Duration = Duration::from_millis(100);
/// Silence after which a live meter shows its radar.
const STALE_AFTER: Duration = Duration::from_secs(5);

pub struct Overlay {
    session: Entity<Session>,
    pub(crate) cfg: Config,
    pub(crate) expanded: bool,
    /// The list line the pointer is over (an index into whichever list is
    /// drawn).
    pub(crate) row_hover: Option<usize>,
    pub(crate) view_menu: bool,
    pub(crate) view_hover: Option<View>,
    /// The pointer is on the footer's view name: a right press there opens
    /// the menu rather than backing out of a drill.
    over_view_name: bool,
    pub(crate) options_open: bool,
    radar_from: Instant,
    /// The radar's next frame, while it shows: a redraw every
    /// `RADAR_FRAME`, a third of the iced overlay's 30 fps.
    radar_tick: Option<gpui_kit::Task<()>>,
    /// The Σ split: the visit's overall rows under the watched fight's.
    pub(crate) split: bool,
    /// The split's own connection — a `Window` kind, so the daemon's
    /// `SetVisible` never reaches it — watching the visit's Σ for its top
    /// rows. Made on first want and kept.
    aux: Option<Entity<Session>>,
    aux_maker: AuxMaker,
    /// What the split's connection was last pointed at.
    aux_watch: Option<(SegmentId, View)>,
    /// Wheel notches over the strip not yet a whole step (touchpads).
    strip_acc: f32,
    /// R26: the drill groups opened, by fold key (session-only).
    pub(crate) tree_open: HashSet<String>,
    /// The marker every graph lights, by label, and the instant every
    /// graph marks: what the graphs last said, echoed back to them all.
    pub(crate) graph_hover: Option<String>,
    pub(crate) graph_probe: Option<usize>,
    /// R12: the ability the pointer is on in either comparison list, by
    /// key — lit in both, since the two are sorted apart.
    pub(crate) spell_hover: Option<String>,
    /// The content's offset along its edge, as shown (a drag moves it
    /// before the config learns it).
    pub(crate) offset: f32,
    /// The grip held, while it is.
    grip: Option<surface::Grip>,
    /// The daemon's wish (`SetVisible`) and the game's workspace being on
    /// screen: the overlay shows only when both say so.
    pub(crate) daemon_visible: bool,
    pub(crate) game_visible: bool,
    /// The input region last set, and this frame's placement.
    region: Option<Option<super::strip::Rect>>,
    placed: Option<surface::Placed>,
    /// Hyprland's socket directory, when it is there to ask.
    hypr: Option<std::path::PathBuf>,
    _game: Option<gpui_kit::Task<()>>,
    /// The body's list and the comparison's two tables, scrolled: their
    /// scrollbars read and move them.
    body_scroll: gpui_kit::ScrollHandle,
    table_scroll: [gpui_kit::ScrollHandle; 2],
    /// The debug aids still to run (`autos.rs`), and when the overlay
    /// started, for their traces.
    autos: autos::Autos,
    started: Instant,
    /// The surface size last asked of the compositor.
    sized: Option<(u32, u32)>,
    /// Each graph's own state: the drill's, or a comparison's two.
    graphs: [graph::Shared; 2],
    _session: Vec<Subscription>,
}

/// What a press on a drill line does.
type Press = Rc<dyn Fn(&mut Overlay, &mut Context<Overlay>)>;

/// How the Σ split's connection is made: a real daemon client in the app,
/// the daemon's mock in a test.
pub type AuxMaker = Rc<dyn Fn(&mut App) -> Result<Entity<Session>, String>>;

/// The rows the Σ split asks for.
pub const AUX_TOP_N: u32 = 8;

impl Overlay {
    pub fn new(
        session: Entity<Session>,
        cfg: Config,
        aux_maker: AuxMaker,
        cx: &mut Context<Self>,
    ) -> Self {
        let subscriptions = vec![
            cx.observe(&session, |this, _, cx| {
                this.open_newest(cx);
                this.run_autos(cx);
                this.sync_aux(cx);
                cx.notify();
            }),
            cx.subscribe(&session, |this, _, event: &SessionEvent, cx| {
                this.on_session_event(event, cx);
            }),
        ];
        let cfg_offset = cfg.offset.max(0) as f32;
        let mut overlay = Self {
            session,
            split: cfg.overlay_split,
            cfg,
            expanded: std::env::var_os("WOWDPS_OVERLAY_START_EXPANDED").is_some(),
            row_hover: None,
            view_menu: false,
            view_hover: None,
            over_view_name: false,
            options_open: false,
            radar_from: Instant::now(),
            radar_tick: None,
            aux: None,
            aux_maker,
            aux_watch: None,
            strip_acc: 0.0,
            tree_open: HashSet::new(),
            graph_hover: None,
            graph_probe: None,
            spell_hover: None,
            sized: None,
            autos: if cfg!(test) {
                autos::Autos::default()
            } else {
                autos::Autos::from_env()
            },
            started: Instant::now(),
            body_scroll: gpui_kit::ScrollHandle::new(),
            table_scroll: Default::default(),
            offset: cfg_offset,
            grip: None,
            daemon_visible: true,
            game_visible: true,
            region: None,
            placed: None,
            // Tests never ask the desktop they run on.
            hypr: if cfg!(test) {
                None
            } else {
                wowdps_gui_logic::hypr::socket_dir()
            },
            _game: None,
            graphs: Default::default(),
            _session: subscriptions,
        };
        overlay.open_newest(cx);
        overlay.follow_game(cx);
        if !cfg!(test) {
            overlay.auto_toggle(cx);
        }
        overlay
    }

    /// What the split's connection should watch: the watched block's Σ in
    /// the current view — none when split is off or the panel shut, the
    /// block has no Σ, or the Σ itself is watched (nothing to repeat).
    fn aux_want(&self, cx: &App) -> Option<(SegmentId, View)> {
        if !(self.split && self.expanded) {
            return None;
        }
        let state = self.state(cx);
        let entries = state.entries();
        let blocks = timeline::blocks(entries);
        let pos = timeline::watched_pos(state)?;
        let block = blocks.get(timeline::block_of(&blocks, pos)?)?;
        let overall = block.overall.filter(|_| block.is_instance())?;
        (overall != pos).then(|| entries.get(overall).map(|e| (e.id, state.view)))?
    }

    /// Point the split's connection at what is wanted (making it the first
    /// time), or back at the list when nothing is.
    pub(crate) fn sync_aux(&mut self, cx: &mut Context<Self>) {
        let Some((id, view)) = self.aux_want(cx) else {
            if self.aux_watch.take().is_some()
                && let Some(aux) = &self.aux
            {
                aux.update(cx, |s, cx| {
                    s.act(
                        |st| {
                            if st.screen == Screen::List {
                                Vec::new()
                            } else {
                                st.apply(Action::Back)
                            }
                        },
                        cx,
                    )
                });
            }
            return;
        };
        if self.aux.is_none() {
            match (self.aux_maker)(cx) {
                Ok(aux) => {
                    let observed = cx.observe(&aux, |this, _, cx| {
                        this.sync_aux(cx);
                        cx.notify();
                    });
                    self._session.push(observed);
                    self.aux = Some(aux);
                }
                Err(e) => {
                    eprintln!("wowdps-gui-new: split view unavailable: {e}");
                    self.split = false;
                    return;
                }
            }
        }
        if self.aux_watch == Some((id, view)) {
            return;
        }
        let Some(aux) = self.aux.clone() else {
            return;
        };
        // The split's own list must know the Σ before it can be watched.
        let pointed = aux.update(cx, |s, cx| {
            let Some(pos) = s.state().entries().iter().position(|e| e.id == id) else {
                return false;
            };
            s.act(
                |st| {
                    let mut reqs = st.goto_list_pos(pos);
                    if st.view != view {
                        reqs.extend(st.apply(Action::SetView(view)));
                    }
                    reqs
                },
                cx,
            );
            true
        });
        if pointed {
            self.aux_watch = Some((id, view));
        }
    }

    /// The Σ split's rows and clock, when its connection holds what is
    /// wanted.
    fn aux_rows(&self, cx: &App) -> Option<(Vec<wowdps_model::Row>, i64)> {
        let want = self.aux_want(cx)?;
        if self.aux_watch != Some(want) {
            return None;
        }
        let aux = self.aux.as_ref()?.read(cx).state();
        let rows = aux.rows();
        (aux.screen == Screen::Meter && aux.view == want.1 && !rows.is_empty())
            .then(|| (rows, aux.duration_ms()))
    }

    /// The Σ split's clock for the header, when its connection holds the
    /// watched block's Σ.
    fn aux_ms(&self, overall: usize, cx: &App) -> Option<i64> {
        let (id, _) = self.aux_watch?;
        let held = self.state(cx).entries().get(overall)?.id == id;
        let aux = self.aux.as_ref()?.read(cx).state();
        (held && aux.screen == Screen::Meter).then(|| aux.duration_ms())
    }

    /// The watched visit, when it wears the instance frame: its block and
    /// the watched position.
    fn instance(&self, cx: &App) -> Option<(timeline::Block, Option<usize>)> {
        let state = self.state(cx);
        let pos = timeline::watched_pos(state);
        let mut blocks = timeline::blocks(state.entries());
        let at = timeline::block_of(&blocks, pos?)?;
        let block = blocks.swap_remove(at);
        block.is_instance().then_some((block, pos))
    }

    /// Show or hide the visit's Σ under the fight, remembered by itself.
    fn toggle_split(&mut self, cx: &mut Context<Self>) {
        self.split = !self.split;
        self.cfg.overlay_split = self.split;
        let on = self.split;
        Config::store(|c| c.overlay_split = on);
        self.sync_aux(cx);
        cx.notify();
    }

    /// The wheel over the strip: whole notches scrub the visit's members
    /// (up older, down newer), fractions carrying over. Each step starts
    /// from where the last landed, so a fast spin stays in order.
    fn strip_scroll(&mut self, notches: f32, cx: &mut Context<Self>) {
        self.strip_acc += notches;
        let whole = self.strip_acc.trunc();
        self.strip_acc -= whole;
        let mut steps = whole as i32;
        self.act(
            |s| {
                let mut reqs = Vec::new();
                while steps != 0 {
                    let delta: isize = if steps > 0 { -1 } else { 1 };
                    let target = timeline::watched_pos(s).and_then(|p| {
                        let blocks = timeline::blocks(s.entries());
                        let at = timeline::block_of(&blocks, p)?;
                        timeline::scrub(blocks.get(at)?, p, delta)
                    });
                    let Some(p) = target else { break };
                    reqs.extend(s.goto_list_pos(p));
                    steps -= steps.signum();
                }
                reqs
            },
            cx,
        );
    }

    #[cfg(test)]
    pub fn session(&self) -> &Entity<Session> {
        &self.session
    }

    fn state<'a>(&self, cx: &'a App) -> &'a ClientState {
        self.session.read(cx).state()
    }

    /// Change the session's state and send what it asks for.
    fn act(&self, f: impl FnOnce(&mut ClientState) -> Vec<ClientMsg>, cx: &mut Context<Self>) {
        self.session.update(cx, |session, cx| session.act(f, cx));
    }

    /// The overlay has no list screen: once segments exist, the newest
    /// opens.
    fn open_newest(&mut self, cx: &mut Context<Self>) {
        let state = self.state(cx);
        if state.screen == Screen::List && state.segment_count() > 0 {
            self.act(
                |s| {
                    s.set_list_selection(usize::MAX);
                    s.apply(Action::Open)
                },
                cx,
            );
        }
    }

    fn on_session_event(&mut self, event: &SessionEvent, cx: &mut Context<Self>) {
        match event {
            // A new pull always brings the meter home to Live: scrubbing
            // history is between-pulls inspection, and the overlay is a live
            // meter first. The one deliberate parking spot that stays put is
            // the live visit's Σ overall, a live meter of its own.
            SessionEvent::SegmentOpened => {
                let state = self.state(cx);
                if state.following_live() {
                    return;
                }
                let parked = timeline::watched_pos(state)
                    .and_then(|p| state.entries().get(p))
                    .is_some_and(|e| {
                        e.row.kind == wowdps_model::SegmentKind::Overall && e.row.live
                    });
                if !parked {
                    self.act(|s| s.pin_live(), cx);
                }
            }
            SessionEvent::SetVisible(v) => self.set_daemon_visible(*v, cx),
        }
    }

    fn set_view(&mut self, view: View, cx: &mut Context<Self>) {
        self.act(|s| s.apply(Action::SetView(view)), cx);
    }

    fn nav_block(&mut self, delta: isize, cx: &mut Context<Self>) {
        let Some((anchor, live_last)) = timeline::block_step(self.state(cx), delta) else {
            return;
        };
        self.act(
            |s| {
                if live_last {
                    s.pin_live()
                } else {
                    s.goto_list_pos(anchor)
                }
            },
            cx,
        );
    }

    /// Expand or collapse, and size the surface for it.
    fn toggle(&mut self, cx: &mut Context<Self>) {
        self.expanded = !self.expanded;
        autos::trace(self.started, format_args!("expanded={}", self.expanded));
        self.sync_aux(cx);
        cx.notify();
    }

    fn zoom(&mut self, event: &ScrollWheelEvent, cx: &mut Context<Self>) {
        let old = self.cfg.zoom;
        let new = (old + 0.05 * notches(event)).clamp(0.6, 2.5);
        if (new - old).abs() < 0.001 {
            return;
        }
        let ratio = new / old;
        self.cfg.zoom = new;
        self.cfg.width = ((self.cfg.width as f32 * ratio).round() as u32).max(160);
        self.cfg.height = ((self.cfg.height as f32 * ratio).round() as u32).max(180);
        let (zoom, width, height) = (self.cfg.zoom, self.cfg.width, self.cfg.height);
        Config::store(|c| {
            c.zoom = zoom;
            c.width = width;
            c.height = height;
        });
        cx.notify();
    }
}

impl Render for Overlay {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // The surface follows the state: a comparison grows it to
        // COMPARE_MIN, its end gives the room back — however the state got
        // there (a badge, a right press, the daemon's answer). On the edge
        // strip, that size is the content's; the strip spans the edge.
        let listener = self.grip_listener(cx).into_any_element();
        let Some(placed) = self.place(window, cx) else {
            // Hidden: nothing drawn, nothing taking input.
            return div().size_full().child(listener).into_any_element();
        };
        // The radar sweeps while it shows: one redraw per `RADAR_FRAME`,
        // and none once the data is fresh again or the panel is folded.
        if self.expanded && self.radar_shown(cx) && self.radar_tick.is_none() {
            self.radar_tick = Some(cx.spawn(async move |this, cx| {
                cx.background_executor().timer(RADAR_FRAME).await;
                let _ = this.update(cx, |o, cx| {
                    o.radar_tick = None;
                    cx.notify();
                });
            }));
        }
        let ov = Ov::new(self.cfg.zoom, cx);
        let content = if self.expanded {
            self.panel(&ov, window, cx).into_any_element()
        } else {
            self.tab(&ov, cx).into_any_element()
        };
        let r = placed.rect;
        div()
            .size_full()
            .relative()
            .child(
                div()
                    .absolute()
                    .left(px(r.x))
                    .top(px(r.y))
                    .w(px(r.w))
                    .h(px(r.h))
                    .child(content),
            )
            .child(listener)
            .into_any_element()
    }
}

// ---- the tab ----------------------------------------------------------------

impl Overlay {
    /// The collapsed tab: a dot (yellow while live) and "dps", down a side
    /// edge or across a top or bottom one. A click expands it.
    fn tab(&self, ov: &Ov, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let live = self.state(cx).is_live();
        let dot = ov.words(
            "●",
            10.,
            if live {
                ov.c(|t| t.yellow)
            } else {
                ov.c(|t| t.dim)
            },
        );
        let letter = |s: &'static str| ov.words(s, 11., ov.c(|t| t.text));
        let label = if self.cfg.edge.is_vertical() {
            div()
                .flex()
                .flex_col()
                .items_center()
                .gap(px(2.))
                .child(dot)
                .child(letter("d"))
                .child(letter("p"))
                .child(letter("s"))
        } else {
            div()
                .flex()
                .items_center()
                .gap(px(4.))
                .child(dot)
                .child(letter("dps"))
        };
        div()
            .id("tab")
            .test_support()
            .size_full()
            .flex()
            .items_center()
            .justify_center()
            .line_height(relative(1.3))
            .bg(ov.c(|t| t.panel.alpha(0.85)))
            .border_1()
            .border_color(ov.c(|t| t.edge))
            .rounded(px(6.))
            .child(label)
            // The grip: a press here is a click or a drag, decided when it
            // is let go (`surface.rs`).
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, e: &gpui_kit::MouseDownEvent, _, _| this.grip_down(e.position)),
            )
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| this.grip_up(cx)),
            )
            .on_scroll_wheel(cx.listener(|this, e: &ScrollWheelEvent, _, cx| this.zoom(e, cx)))
    }
}

// ---- the panel --------------------------------------------------------------

impl Overlay {
    fn panel(
        &mut self,
        ov: &Ov,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let instance = self.instance(cx);
        let frame = div()
            .size_full()
            .flex()
            .flex_col()
            .gap(px(4.))
            .p(px(6.))
            .bg(ov.c(|t| t.panel.alpha(0.92)))
            .border_1()
            .border_color(ov.c(|t| t.edge))
            .rounded(px(6.))
            .child(self.header(ov, instance.as_ref(), cx))
            .when_some(instance.as_ref(), |d, (block, pos)| {
                d.child(self.strip(ov, block, *pos, cx))
                    .child(self.chip(ov, block, *pos, cx))
            })
            .child(self.body(ov, window, cx))
            .child(self.footer(ov, instance.is_some(), cx));
        let card = if self.options_open {
            Some(self.options_card(ov, cx))
        } else if self.view_menu {
            Some(self.view_menu_card(ov, cx))
        } else {
            None
        };
        div()
            .id("overlay")
            .size_full()
            .relative()
            .font_family(ov.sans)
            .line_height(relative(1.3))
            .child(frame)
            .children(card)
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(|this, _, _, cx| {
                    // A right press that no control took backs out of a
                    // drill, one level (never onto the list).
                    let drilled = this.state(cx).drill.is_some();
                    if drilled && !this.over_view_name && !this.view_menu {
                        this.act(|s| s.apply(Action::Back), cx);
                    }
                }),
            )
    }

    /// The header: the visit (or the fight) by name, its badge and its
    /// clock. It is the panel's grip: a click collapses the panel, the wheel
    /// zooms.
    fn header(
        &self,
        ov: &Ov,
        instance: Option<&(timeline::Block, Option<usize>)>,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let state = self.state(cx);
        let entries = state.entries();
        let (name, (tag, tone), clock) = match instance
            .and_then(|(b, _)| b.overall.and_then(|o| entries.get(o).map(|e| (b, o, e))))
        {
            Some((block, overall, entry)) => {
                let aux_ms = self.aux_ms(overall, cx);
                let clock = timeline::instance_clock(state, block, overall, aux_ms);
                (
                    entry.row.name.clone(),
                    labels::overall_tag(&entry.row, clock),
                    clock,
                )
            }
            None => {
                let (word, tone) = labels::header_tag(state);
                (
                    state
                        .segment_name()
                        .unwrap_or_else(|| "waiting for combat…".to_string()),
                    (word.to_string(), tone),
                    state.duration_ms(),
                )
            }
        };
        div()
            .id("header")
            .test_support()
            .w_full()
            .flex()
            .items_center()
            .gap(px(6.))
            .py(px(4.))
            .px(px(8.))
            .child(ov.words(name, 13., ov.c(|t| t.text)))
            .child(ov.words(tag, 10., tone_color(ov, tone)))
            .child(div().flex_1())
            .child(ov.words(duration(clock), 12., ov.c(|t| t.text)))
            // The grip: a press here is a click or a drag, decided when it
            // is let go (`surface.rs`).
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, e: &gpui_kit::MouseDownEvent, _, _| this.grip_down(e.position)),
            )
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| this.grip_up(cx)),
            )
            .on_scroll_wheel(cx.listener(|this, e: &ScrollWheelEvent, _, cx| this.zoom(e, cx)))
    }

    /// The strip over the visit (`instance::strip`): a press goes to its
    /// element's pull, the wheel scrubs the visit's members — the fan
    /// compresses space, the wheel gives it back.
    fn strip(
        &self,
        ov: &Ov,
        block: &timeline::Block,
        pos: Option<usize>,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let entries = self.state(cx).entries();
        // Progression nights: wipe runs between kills collapse into ×N
        // pills; the chip's scrubbers still step every hidden attempt.
        let items = timeline::collapse(timeline::items(block, entries), entries, pos);
        // The panel's padding (6 + 6) and the strip's own (8 + 8).
        let budget = (self.cfg.width as f32 - 28.0).max(40.0);
        let fan = instance::strip(ov, &items, pos, budget, |i, el, goto| {
            div()
                .id(("strip", i))
                .test_support()
                .child(el)
                .when_some(goto, |d, p| {
                    d.cursor_pointer().on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, _, _, cx| this.act(|s| s.goto_list_pos(p), cx)),
                    )
                })
                .into_any_element()
        });
        div()
            .id("strip")
            .test_support()
            .px(px(8.))
            .child(fan)
            .on_scroll_wheel(cx.listener(|this, e: &ScrollWheelEvent, _, cx| {
                this.strip_scroll(notches(e), cx);
                cx.stop_propagation();
            }))
    }

    /// The chip under the strip: ‹ › scrubbing the visit's Σ and members,
    /// the watched one's name and outcome, and its own clock (the header
    /// keeps the visit's).
    fn chip(
        &self,
        ov: &Ov,
        block: &timeline::Block,
        pos: Option<usize>,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let state = self.state(cx);
        let prev = pos.and_then(|p| timeline::scrub(block, p, -1));
        let next = pos.and_then(|p| timeline::scrub(block, p, 1));
        let (ink, dim, yellow) = (ov.c(|t| t.ink), ov.c(|t| t.dim), ov.c(|t| t.yellow));
        // Padded hit boxes, not bare glyphs, so a mid-fight press lands.
        let mini = |id: &'static str, glyph: &'static str, target: Option<usize>| {
            div()
                .id(id)
                .test_support()
                .py(ov.z(3.))
                .px(ov.z(8.))
                .child(ov.words(glyph, 13., if target.is_some() { ink } else { dim }))
                .when_some(target, |d, p| {
                    d.cursor_pointer().on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, _, _, cx| this.act(|s| s.goto_list_pos(p), cx)),
                    )
                })
        };
        let (name, color) = match pos.and_then(|p| state.entries().get(p)).map(|e| &e.row) {
            Some(r) if r.kind == wowdps_model::SegmentKind::Overall => {
                ("Σ overall".to_string(), yellow)
            }
            Some(r) if r.kind == wowdps_model::SegmentKind::Encounter => (r.name.clone(), ink),
            Some(r) if !r.name.is_empty() => (r.name.clone(), dim),
            Some(_) => ("trash".to_string(), dim),
            None => (String::new(), dim),
        };
        // The outcome alone: while live the header and the footer say so,
        // and a LIVE badge here reads as part of the name.
        let (tag, tone) = if state.is_live() {
            ("", Tone::None)
        } else {
            labels::header_tag(state)
        };
        div()
            .flex()
            .items_center()
            .gap(ov.z(6.))
            .px(px(8.))
            .child(mini("chip-prev", "‹", prev))
            .child(mini("chip-next", "›", next))
            .child(div().w(ov.z(4.)).flex_none())
            .child(ov.words(name, 11., color))
            .child(ov.words(tag, 9., tone_color(ov, tone)))
            .child(div().flex_1())
            .child(ov.nums(duration(state.duration_ms()), 11., dim))
    }

    /// The body: the meter's rows, or a drill's with its graph under them
    /// (the comparison arrives with step 2.5), the list scrolling with room
    /// on the right for the scrollbar. A right press here clears a
    /// comparison or a lone pick; with neither it falls through to the
    /// panel, which backs out of a drill — one level per press.
    fn body(&self, ov: &Ov, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let comparing = self.state(cx).screen == Screen::Compare;
        let content = if comparing {
            // R12: the comparison replaces the rows outright — at panel
            // width there is no room for both, and the meter is a right
            // press away.
            self.compare(ov, cx)
        } else {
            let list = if self.state(cx).drill.is_some() {
                self.drill(ov, cx)
            } else {
                self.meter(ov, window, cx)
                    .children(self.split_rows(ov, window, cx))
            };
            div()
                .flex_1()
                .min_h_0()
                .flex()
                .flex_col()
                .gap(px(4.))
                .child(
                    // The list scrolls in its own lane, its scrollbar beside
                    // it in the lane the list leaves on the right.
                    div()
                        .relative()
                        .flex_1()
                        .min_h_0()
                        .child(
                            div()
                                .id("body")
                                .test_support()
                                .size_full()
                                .overflow_y_scroll()
                                .track_scroll(&self.body_scroll)
                                .pr(px(10.))
                                .child(list),
                        )
                        .child(crate::scrollbar::bar(ov.scrollbar(), &self.body_scroll)),
                )
                .children(self.drill_graph(ov, cx))
                .into_any_element()
        };
        div()
            .id("body-area")
            .test_support()
            .flex_1()
            .min_h_0()
            .flex()
            .flex_col()
            .child(content)
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(|this, _, _, cx| {
                    let state = this.state(cx);
                    if state.screen == Screen::Compare || !state.compare_picks().is_empty() {
                        this.act(|s| s.clear_compare(), cx);
                        cx.stop_propagation();
                    }
                }),
            )
    }

    /// The drilled player's lines (`drill.rs`): an ability drill's crumb,
    /// stat cards and targets, or the player's caption and abilities — R26's
    /// tree on Damage and Healing, a recap on Deaths, the attackers of an
    /// enemy — and R17's mitigation line under a Taken drill.
    fn drill(&self, ov: &Ov, cx: &mut Context<Self>) -> Div {
        let state = self.state(cx);
        let list = div().flex().flex_col().gap(px(2.));
        let Some(d) = state.drill.as_ref() else {
            return list;
        };
        let view = state.view;
        let dim = ov.c(|t| t.dim);
        if let Some((_, spell)) = state.drill_spell() {
            let row = state.drill_spell_row();
            let list = list.child(drill::crumb(ov, &d.label, spell, row.as_ref()));
            let list = match &row {
                Some(r) => list.child(drill::stats(ov, r, view)),
                None => list.child(ov.words("no data yet", 12., dim)),
            };
            let targets = state.spell_target_rows();
            return if targets.is_empty() {
                list
            } else {
                list.children(drill::targets(ov, &targets, view))
            };
        }
        let who = d.label.split('-').next().unwrap_or(&d.label).to_string();
        let mut list = list.child(drill::caption(ov, &who, view));
        let enemy = view == View::EnemyTaken;
        let recap = view == View::Deaths;
        let counts = wowdps_gui_logic::drill::counts(view);
        let (by_spell, by_target) = state.breakdown();
        let listed = if enemy { by_target } else { by_spell };
        if listed.is_empty() {
            list = list.child(ov.words("no data yet", 12., dim));
        }
        // R26: a Damage or Healing drill rolls its abilities up as the
        // window's inspector does — a pet's under its summon, a trinket's
        // under the item, a proc under its driver — groups shut until a
        // press opens one; a press on an ability still drills into it. The
        // overlay never opens a row's parts, and a drill with no groups
        // stays the flat list.
        let tree = state.drill_tree();
        let grouped = (!enemy && !recap && !counts && !tree.groups.is_empty())
            .then(|| wowdps_gui_logic::tree::lines(&listed, &tree, &self.tree_open, None));
        let hover_wash = ov.c(|t| t.hover);
        let wrap = |at: usize, el: Div, cx: &Context<Self>| {
            div()
                .id(("drill-line", at))
                .test_support()
                .rounded(px(3.))
                .when(self.row_hover == Some(at), |d| d.bg(hover_wash))
                .child(el)
                .on_hover(cx.listener(move |this, over: &bool, _, cx| {
                    let now = over.then_some(at);
                    if this.row_hover != now && (*over || this.row_hover == Some(at)) {
                        this.row_hover = now;
                        cx.notify();
                    }
                }))
        };
        if let Some(lines) = grouped {
            use wowdps_gui_logic::tree::Node;
            let max = lines
                .iter()
                .filter(|l| l.depth == 0)
                .map(|l| l.row.amount)
                .max()
                .unwrap_or(1);
            for (at, l) in lines.iter().enumerate() {
                let group = matches!(l.node, Node::Group(_));
                let press: Press = match (&l.node, &l.fold_key, l.opens) {
                    (Node::Group(_), Some(key), _) => {
                        let key = key.clone();
                        Rc::new(move |this, cx| {
                            if !this.tree_open.remove(&key) {
                                this.tree_open.insert(key.clone());
                            }
                            cx.notify();
                        })
                    }
                    (Node::Row(_), _, Some(i)) => Rc::new(move |this, cx| this.open_spell(i, cx)),
                    _ => continue,
                };
                let label = match &l.tail {
                    Some(t) => format!("{} ({t})", l.name),
                    None => l.name.clone(),
                };
                let lead = match (group, l.fold) {
                    (true, Some(open)) => drill::Lead::Group { open },
                    _ => drill::Lead::Row,
                };
                let el = drill::line(
                    ov,
                    &l.row,
                    lead,
                    &label,
                    12.0 * f32::from(l.depth),
                    max,
                    false,
                );
                list = list.child(wrap(at, el, cx).on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, _, _, cx| press(this, cx)),
                ));
            }
        } else {
            let max = listed.iter().map(|r| r.amount).max().unwrap_or(1);
            for (i, r) in listed.iter().enumerate() {
                if recap {
                    list = list.child(drill::recap(ov, r, max));
                    continue;
                }
                let el = if enemy {
                    // R24: attackers are players — the meter's own row, its
                    // badge, its class bar, its rank.
                    div()
                        .flex()
                        .items_center()
                        .gap(ov.z(4.))
                        .when(self.cfg.show_ranks, |d| d.child(rank_cell(ov, i + 1)))
                        .child(class_icon(ov, r.class, r.spec, false, ov.z(14.)))
                        .child(div().flex_1().min_w_0().child(meter_row(
                            ov,
                            r,
                            None,
                            rows::fill(r.amount, max),
                        )))
                } else {
                    drill::line(ov, r, drill::Lead::Flat, &r.label, 0.0, max, counts)
                };
                list = list.child(wrap(i, el, cx).on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, _, _, cx| {
                        if enemy {
                            this.open_attacker(i, cx);
                        } else {
                            this.open_spell(i, cx);
                        }
                    }),
                ));
            }
        }
        // R17: the mitigation record under a Taken drill, one line.
        if let Some(line) = wowdps_gui_logic::drill::drill_mitigation_line(state) {
            // It wraps, as iced's does: the record is longer than the panel.
            list = list.child(
                div()
                    .py(px(2.))
                    .px(px(8.))
                    .font_family(ov.mono)
                    .text_size(ov.z(9.))
                    .text_color(dim)
                    .child(line),
            );
        }
        list
    }

    /// v14: the drilled player's graph and its legend, under the list —
    /// when the drill has a timeline with something in it.
    fn drill_graph(&self, ov: &Ov, cx: &mut Context<Self>) -> Option<AnyElement> {
        let state = self.state(cx);
        if state.screen == Screen::Compare {
            return None;
        }
        let t = state.drill_timeline().filter(|t| !t.buckets.is_empty())?;
        let class = state
            .drill
            .as_ref()
            .and_then(|d| state.rows().into_iter().find(|r| r.key == d.key))
            .and_then(|r| r.class);
        // v16: an ability drill draws its own curve in its school's colour
        // over the player's ghosted line.
        let focus_color = state
            .drill_spell_row()
            .and_then(|r| wowdps_gui_logic::theme::school_color(r.school))
            .unwrap_or(ov.t.yellow);
        let focus = state.spell_timeline().map(|ft| (ft, focus_color));
        let (g, legend) = graph::drill(
            ov,
            state,
            t,
            class,
            focus,
            self.graph_hover.clone(),
            self.graph_probe,
        );
        let this = cx.entity().downgrade();
        let on: graph::OnEvent = Rc::new(move |e, _, cx| {
            let _ = this.update(cx, |o, cx| o.on_graph(e, cx));
        });
        Some(
            div()
                .id("drill-graph")
                .test_support()
                .flex_none()
                .flex()
                .flex_col()
                .gap(px(4.))
                .child(g.element(ov.z(64.), self.graphs[0].clone(), on))
                .child(legend)
                .into_any_element(),
        )
    }

    /// What a graph's gesture says, for the graph it came from.
    pub(crate) fn on_graph(&mut self, e: graph::Event, cx: &mut Context<Self>) {
        match e {
            graph::Event::Range(range) => {
                if self.state(cx).screen == Screen::Compare {
                    self.act(|s| s.set_compare_range(range), cx);
                } else {
                    self.act(|s| s.set_drill_range(range), cx);
                }
            }
            graph::Event::Hover(label) => self.graph_hover = label,
            graph::Event::Probe(at) => self.graph_probe = at,
        }
        cx.notify();
    }

    /// v16: descend into ability `i` of the drill.
    fn open_spell(&mut self, i: usize, cx: &mut Context<Self>) {
        self.act(
            |s| {
                if let Some(d) = s.drill.as_mut() {
                    d.spell_sel = i;
                    d.pane = wowdps_model::Pane::Spell;
                }
                s.apply(Action::Open)
            },
            cx,
        );
    }

    /// R24: descend into attacker `i` of an enemy's drill.
    fn open_attacker(&mut self, i: usize, cx: &mut Context<Self>) {
        self.act(
            |s| {
                if let Some(d) = s.drill.as_mut() {
                    d.target_sel = i;
                    d.pane = wowdps_model::Pane::Target;
                }
                s.apply(Action::Open)
            },
            cx,
        );
    }

    fn meter(&self, ov: &Ov, window: &mut Window, cx: &mut Context<Self>) -> Div {
        let state = self.state(cx);
        let rows = state.rows();
        let list = div().flex().flex_col().gap(px(2.));
        if rows.is_empty() {
            return list.child(ov.words("no data yet", 12., ov.c(|t| t.dim)));
        }
        let enemies = state.view == View::EnemyTaken;
        let live = state.is_live();
        let max = rows.iter().map(|r| r.amount).max().unwrap_or(1);
        let split = enemy_split(&rows);
        let picked: Vec<bool> = rows
            .iter()
            .map(|r| state.compare_slot(&r.key).is_some())
            .collect();
        let mut list = list;
        for (i, row) in rows.iter().enumerate() {
            if split == Some(i) {
                list = list.child(team_divider(ov));
            }
            let mut drawn = row.clone();
            drawn.enemy = row.enemy || enemies;
            let target = rows::fill(row.amount, max);
            let frac = bar_frac(
                ElementId::from((
                    ElementId::Name("bar".into()),
                    SharedString::from(row.key.clone()),
                )),
                target,
                live,
                window,
                cx,
            );
            let icon = if enemies {
                enemy_icon(ov, ov.z(14.))
            } else {
                let picked = picked.get(i).copied().unwrap_or(false);
                div()
                    .id(ElementId::from((
                        ElementId::Name("pick".into()),
                        SharedString::from(row.key.clone()),
                    )))
                    .test_support()
                    .child(class_icon(ov, row.class, row.spec, picked, ov.z(14.)))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, _, _, cx| {
                            this.act(
                                |s| {
                                    s.set_list_selection(s.list_selection());
                                    let mut reqs = s.select_row(i);
                                    reqs.extend(s.apply(Action::PickCompare));
                                    reqs
                                },
                                cx,
                            );
                            cx.stop_propagation();
                        }),
                    )
                    .into_any_element()
            };
            let hovered = self.row_hover == Some(i);
            let bar = div()
                .id(crate::meter::row_id(&row.key))
                .test_support()
                .flex_1()
                .min_w_0()
                .rounded(px(3.))
                .when(hovered, |d| d.bg(ov.c(|t| t.hover)))
                .child(meter_row(ov, &drawn, None, frac))
                .on_hover(cx.listener(move |this, over: &bool, _, cx| {
                    let now = over.then_some(i);
                    if this.row_hover != now && (*over || this.row_hover == Some(i)) {
                        this.row_hover = now;
                        cx.notify();
                    }
                }))
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, _, _, cx| {
                        this.act(
                            |s| {
                                let mut reqs = s.select_row(i);
                                reqs.extend(s.apply(Action::Open));
                                reqs
                            },
                            cx,
                        );
                    }),
                );
            list = list.child(
                div()
                    .flex()
                    .items_center()
                    .gap(ov.z(4.))
                    .when(self.cfg.show_ranks, |d| d.child(rank_cell(ov, i + 1)))
                    .child(icon)
                    .child(bar),
            );
        }
        list
    }

    /// The Σ split: the visit's overall top rows under the fight's, fed by
    /// the split's own connection — a caption with the visit's clock, then
    /// rows with their rank inside, no badge and no gestures.
    fn split_rows(&self, ov: &Ov, window: &mut Window, cx: &mut Context<Self>) -> Vec<AnyElement> {
        if self.state(cx).drill.is_some() {
            return Vec::new();
        }
        let Some((rows, clock)) = self.aux_rows(cx) else {
            return Vec::new();
        };
        let caption = div()
            .flex()
            .items_center()
            .py(px(3.))
            .px(px(8.))
            .child(ov.words("Σ overall", 10., ov.c(|t| t.yellow)))
            .child(div().flex_1())
            .child(ov.nums(duration(clock), 10., ov.c(|t| t.dim)))
            .into_any_element();
        let max = rows.first().map_or(1, |r| r.amount);
        let live = self.state(cx).is_live();
        let ranks = self.cfg.show_ranks;
        std::iter::once(caption)
            .chain(rows.iter().enumerate().map(|(i, r)| {
                let frac = bar_frac(
                    ElementId::from((
                        ElementId::Name("split-bar".into()),
                        SharedString::from(r.key.clone()),
                    )),
                    rows::fill(r.amount, max),
                    live,
                    window,
                    cx,
                );
                meter_row(ov, r, ranks.then_some(i + 1), frac).into_any_element()
            }))
            .collect()
    }
}

/// A wheel event in notches: a line each, or 40 px of a touchpad's.
fn notches(event: &ScrollWheelEvent) -> f32 {
    match event.delta {
        gpui_kit::ScrollDelta::Lines(l) => l.y,
        gpui_kit::ScrollDelta::Pixels(p) => f32::from(p.y) / 40.0,
    }
}

// ---- the footer ---------------------------------------------------------------

impl Overlay {
    /// The staleness radar shows: the watched pull is live and nothing new
    /// has come for `STALE_AFTER` (the game flushes its log in bursts).
    fn radar_shown(&self, cx: &App) -> bool {
        self.state(cx).is_live()
            && self
                .session
                .read(cx)
                .last_snapshot()
                .is_some_and(|since| since.elapsed() >= STALE_AFTER)
    }

    fn footer(&self, ov: &Ov, instance: bool, cx: &mut Context<Self>) -> impl IntoElement {
        let state = self.state(cx);
        let view = state.view;
        let entries = state.entries();
        let blocks = timeline::blocks(entries);
        let bpos = timeline::watched_pos(state)
            .and_then(|p| timeline::block_of(&blocks, p))
            .unwrap_or(blocks.len().saturating_sub(1));
        let (dim, ink, yellow) = (ov.c(|t| t.dim), ov.c(|t| t.ink), ov.c(|t| t.yellow));

        let name = div()
            .id("view-name")
            .test_support()
            .cursor_pointer()
            .child(ov.words(
                view_name(view),
                11.,
                if self.view_menu { yellow } else { dim },
            ))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, _, _, cx| this.set_view(view.next(), cx)),
            )
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(|this, _, _, cx| {
                    this.view_menu = true;
                    this.view_hover = None;
                    cx.stop_propagation();
                    cx.notify();
                }),
            )
            .on_hover(cx.listener(|this, over: &bool, _, _| this.over_view_name = *over));
        let trash = div()
            .id("trash")
            .test_support()
            .child(ov.trash(dim, ov.z(11.)))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| {
                    this.session
                        .update(cx, |s, cx| s.act(|_| vec![ClientMsg::DiscardTrash], cx));
                }),
            );
        // Inside a visit: Σ shows the visit's overall under the fight.
        let split = instance.then(|| {
            div()
                .id("split")
                .test_support()
                .aria_selected(self.split)
                .cursor_pointer()
                .child(ov.words("Σ", 11., if self.split { yellow } else { dim }))
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, _, _, cx| this.toggle_split(cx)),
                )
        });
        // R12 / v14: a graph on screen earns the one control it needs, the
        // curve's mode, worded as the curve it would show next to.
        let graph_mode = (state.screen == Screen::Compare || state.drill_timeline().is_some())
            .then(|| {
                let label = match state.graph_mode() {
                    wowdps_model::GraphMode::Dps if state.screen != Screen::Compare => {
                        wowdps_gui_logic::labels::rate_label(view)
                    }
                    m => m.label(),
                };
                div()
                    .id("graph-mode")
                    .test_support()
                    .cursor_pointer()
                    .child(ov.words(label, 11., yellow))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this, _, _, cx| {
                            this.act(
                                |s| {
                                    s.toggle_graph();
                                    Vec::new()
                                },
                                cx,
                            );
                            cx.notify();
                        }),
                    )
            });
        let left = div()
            .flex()
            .items_center()
            .gap(px(8.))
            .child(name)
            .child(trash)
            .children(split)
            .children(graph_mode);

        let arrow = |id: &'static str, glyph: &'static str, enabled: bool, delta: isize| {
            div()
                .id(id)
                .test_support()
                .py(px(4.))
                .px(px(12.))
                .child(ov.words(glyph, 11., if enabled { ink } else { dim }))
                .when(enabled, |d| {
                    d.on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, _, _, cx| this.nav_block(delta, cx)),
                    )
                })
        };
        let nav = div()
            .flex()
            .items_center()
            .child(arrow("prev-block", "◀", bpos > 0, -1))
            .child(ov.nums(format!("{}/{}", bpos + 1, blocks.len().max(1)), 10., dim))
            .child(arrow("next-block", "▶", bpos + 1 < blocks.len(), 1));

        let mut right = div().flex().items_center().gap(px(8.));
        if !state.following_live() && state.segment_count() > 0 {
            right = right.child(
                div()
                    .id("go-live")
                    .test_support()
                    .flex()
                    .items_center()
                    .gap(ov.z(3.))
                    .child(ov.dot(yellow, ov.z(7.)))
                    .child(ov.words("live", 10., yellow))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this, _, _, cx| this.act(|s| s.pin_live(), cx)),
                    ),
            );
        }
        if state.is_live()
            && let Some(since) = self.session.read(cx).last_snapshot()
            && since.elapsed() >= STALE_AFTER
        {
            let turns = self.radar_from.elapsed().as_secs_f32() / RADAR_PERIOD.as_secs_f32();
            right = right.child(
                div()
                    .flex()
                    .items_center()
                    .gap(ov.z(3.))
                    .child(ov.radar(turns * std::f32::consts::TAU, ov.t.good, ov.z(13.)))
                    .child(ov.words(format!("{}s", since.elapsed().as_secs()), 10., dim)),
            );
        }
        right = right.child(
            div()
                .id("options")
                .test_support()
                .child(ov.words("⚙", 12., if self.options_open { yellow } else { dim }))
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, _, _, cx| {
                        this.options_open = !this.options_open;
                        cx.notify();
                    }),
                ),
        );

        div()
            .w_full()
            .flex()
            .items_center()
            .child(div().flex_1().child(left))
            .child(nav)
            .child(div().flex_1().flex().justify_end().child(right))
    }
}

// ---- the cards ----------------------------------------------------------------

impl Overlay {
    /// The options card, over the footer's ⚙: it closes when the pointer
    /// leaves it, and presses on it go nowhere else.
    fn options_card(&self, ov: &Ov, cx: &mut Context<Self>) -> AnyElement {
        let on = self.cfg.show_ranks;
        let check = div()
            .size(ov.z(12.))
            .flex()
            .items_center()
            .justify_center()
            .rounded(px(2.))
            .when(on, |d| d.bg(ov.c(|t| t.control)))
            .when(!on, |d| d.border_1().border_color(ov.c(|t| t.text)))
            .when(on, |d| d.child(ov.words("✓", 9., ov.c(|t| t.control_ink))));
        let card = div()
            .id("options-card")
            .test_support()
            .flex()
            .flex_col()
            .gap(ov.z(6.))
            .p(ov.z(8.))
            .bg(ov.c(|t| t.card))
            .border_1()
            .border_color(ov.c(|t| t.card_edge))
            .rounded(px(4.))
            .child(ov.words("options", 9., ov.c(|t| t.dim)))
            .child(
                div()
                    .id("row-ranks")
                    .test_support()
                    .aria_selected(on)
                    .flex()
                    .items_center()
                    .gap(ov.z(8.))
                    .child(check)
                    .child(ov.words("row ranks", 11., ov.c(|t| t.text)))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this, _, _, cx| {
                            this.cfg.show_ranks = !this.cfg.show_ranks;
                            let on = this.cfg.show_ranks;
                            Config::store(|c| c.show_ranks = on);
                            cx.notify();
                        }),
                    ),
            )
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_mouse_down(MouseButton::Right, |_, _, cx| cx.stop_propagation())
            .on_hover(cx.listener(|this, over: &bool, _, cx| {
                if !*over {
                    this.options_open = false;
                    cx.notify();
                }
            }));
        div()
            .absolute()
            .bottom(ov.z(26.))
            .right(ov.z(8.))
            .child(card)
            .into_any_element()
    }

    /// The view menu, over the footer's view name: every view, the watched
    /// one in yellow, the pointer's in a wash; a press picks.
    fn view_menu_card(&self, ov: &Ov, cx: &mut Context<Self>) -> AnyElement {
        let current = self.state(cx).view;
        let items = View::ALL.iter().map(|&v| {
            let lit = self.view_hover == Some(v);
            div()
                .id(ElementId::from((
                    ElementId::Name("view".into()),
                    SharedString::from(view_name(v)),
                )))
                .test_support()
                .aria_selected(v == current)
                .cursor_pointer()
                .w_full()
                .py(ov.z(2.))
                .px(ov.z(5.))
                .rounded(px(3.))
                .when(lit, |d| d.bg(ov.c(|t| t.menu_hover)))
                .child(ov.words(
                    view_name(v),
                    11.,
                    if v == current {
                        ov.c(|t| t.yellow)
                    } else {
                        ov.c(|t| t.ink)
                    },
                ))
                .on_hover(cx.listener(move |this, over: &bool, _, cx| {
                    let now = over.then_some(v);
                    if this.view_hover != now && (*over || this.view_hover == Some(v)) {
                        this.view_hover = now;
                        cx.notify();
                    }
                }))
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, _, _, cx| {
                        this.view_menu = false;
                        this.view_hover = None;
                        this.set_view(v, cx);
                    }),
                )
        });
        let card = div()
            .id("view-menu")
            .test_support()
            .flex()
            .flex_col()
            .gap(ov.z(2.))
            .p(ov.z(8.))
            .bg(ov.c(|t| t.card))
            .border_1()
            .border_color(ov.c(|t| t.card_edge))
            .rounded(px(4.))
            .children(items)
            .on_mouse_down(MouseButton::Right, |_, _, cx| cx.stop_propagation())
            .on_hover(cx.listener(|this, over: &bool, _, cx| {
                if !*over {
                    this.view_menu = false;
                    this.view_hover = None;
                    cx.notify();
                }
            }));
        div()
            .absolute()
            .bottom(ov.z(26.))
            .left(ov.z(8.))
            .right(ov.z(8.))
            .child(card)
            .into_any_element()
    }
}

/// A verdict word's colour in the overlay's palette.
fn tone_color(ov: &Ov, tone: Tone) -> gpui_kit::Hsla {
    match tone {
        Tone::Live => ov.c(|t| t.yellow),
        Tone::Good => ov.c(|t| t.good),
        Tone::Bad => ov.c(|t| t.bad),
        Tone::None => ov.c(|t| t.dim),
    }
}

mod autos;
mod compare;
mod surface;
pub use autos::start_view;
pub use surface::Reanchor;
#[cfg(test)]
pub(crate) mod tests;
