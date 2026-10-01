//! The overlay's view (plan step 2.1): the collapsed tab, and the panel —
//! header, meter rows, footer — with its two cards, the options card and
//! the view menu. Drawn as the iced overlay draws them
//! (`overlay-anatomy.md` §3), from the overlay palette and at its zoom.
//!
//! The overlay has no keyboard: every gesture is the pointer's. Its state
//! lives in its `Session`'s `ClientState` (what is watched, the selection,
//! the drill) and in this entity (what the pointer is over, which card is
//! open, whether the panel is expanded).

use std::time::{Duration, Instant};

use gpui_kit::base::{Easing, Transition, transition};
use gpui_kit::prelude::*;
use gpui_kit::{
    AnyElement, App, Context, Div, ElementId, Entity, MouseButton, ScrollWheelEvent, SharedString,
    Subscription, TestSupportExt as _, Window, div, px, relative,
};
use wowdps_gui_logic::config::Config;
use wowdps_gui_logic::labels::{self, Tone};
use wowdps_gui_logic::surface::surface_size;
use wowdps_gui_logic::table::enemy_split;
use wowdps_gui_logic::timeline;
use wowdps_model::fmt::{duration, view_name};
use wowdps_model::{Action, Screen, View};
use wowdps_proto::{ClientMsg, ClientState};

use super::ov::Ov;
use super::rows::{self, class_icon, enemy_icon, meter_row, rank_cell, team_divider};
use crate::session::{Session, SessionEvent};

/// One revolution of the staleness radar's hand.
const RADAR_PERIOD: Duration = Duration::from_millis(2500);
/// Silence after which a live meter shows its radar.
const STALE_AFTER: Duration = Duration::from_secs(5);
/// How long a bar takes to reach a new value: long enough to read as
/// motion, short enough that a 10 Hz meter never lags what it shows.
const BAR_EASE: Duration = Duration::from_millis(280);

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
    _session: Vec<Subscription>,
}

impl Overlay {
    pub fn new(session: Entity<Session>, cfg: Config, cx: &mut Context<Self>) -> Self {
        let subscriptions = vec![
            cx.observe(&session, |this, _, cx| {
                this.open_newest(cx);
                cx.notify();
            }),
            cx.subscribe(&session, |this, _, event: &SessionEvent, cx| {
                this.on_session_event(event, cx);
            }),
        ];
        let mut overlay = Self {
            session,
            cfg,
            expanded: std::env::var_os("WOWDPS_OVERLAY_START_EXPANDED").is_some(),
            row_hover: None,
            view_menu: false,
            view_hover: None,
            over_view_name: false,
            options_open: false,
            radar_from: Instant::now(),
            _session: subscriptions,
        };
        overlay.open_newest(cx);
        overlay
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
            SessionEvent::SetVisible(_) => {}
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
    fn toggle(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.expanded = !self.expanded;
        self.fit(window, cx);
        cx.notify();
    }

    /// The surface's size for the state it is in (`gui_logic::surface`).
    pub(crate) fn fit(&self, window: &mut Window, cx: &App) {
        let comparing = self.state(cx).screen == Screen::Compare;
        let (w, h) = surface_size(&self.cfg, self.expanded, comparing);
        window.resize(gpui_kit::size(px(w as f32), px(h as f32)));
    }

    fn zoom(&mut self, event: &ScrollWheelEvent, window: &mut Window, cx: &mut Context<Self>) {
        let notches = match event.delta {
            gpui_kit::ScrollDelta::Lines(l) => l.y,
            gpui_kit::ScrollDelta::Pixels(p) => f32::from(p.y) / 40.0,
        };
        let old = self.cfg.zoom;
        let new = (old + 0.05 * notches).clamp(0.6, 2.5);
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
        self.fit(window, cx);
        cx.notify();
    }
}

impl Render for Overlay {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let ov = Ov::new(self.cfg.zoom, cx);
        if self.expanded {
            self.panel(&ov, window, cx).into_any_element()
        } else {
            self.tab(&ov, cx).into_any_element()
        }
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
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _, window, cx| this.toggle(window, cx)),
            )
            .on_scroll_wheel(
                cx.listener(|this, e: &ScrollWheelEvent, window, cx| this.zoom(e, window, cx)),
            )
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
            .child(self.header(ov, cx))
            .child(self.body(ov, window, cx))
            .child(self.footer(ov, cx));
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
    fn header(&self, ov: &Ov, cx: &mut Context<Self>) -> impl IntoElement {
        let state = self.state(cx);
        let entries = state.entries();
        let blocks = timeline::blocks(entries);
        let instance = timeline::watched_pos(state)
            .and_then(|p| timeline::block_of(&blocks, p))
            .and_then(|b| blocks.get(b))
            .filter(|b| b.is_instance());
        let (name, (tag, tone), clock) = match instance
            .and_then(|b| b.overall.and_then(|o| entries.get(o).map(|e| (b, o, e))))
        {
            Some((block, overall, entry)) => {
                let clock = timeline::instance_clock(state, block, overall, None);
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
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _, window, cx| this.toggle(window, cx)),
            )
            .on_scroll_wheel(
                cx.listener(|this, e: &ScrollWheelEvent, window, cx| this.zoom(e, window, cx)),
            )
    }

    /// The body: the meter's rows (the drill and the comparison arrive with
    /// steps 2.4 and 2.5), scrolling, with room on the right for the
    /// scrollbar.
    fn body(&self, ov: &Ov, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .id("body")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .pr(px(10.))
            .child(self.meter(ov, window, cx))
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(|this, _, _, cx| {
                    this.act(|s| s.clear_compare(), cx);
                    cx.stop_propagation();
                }),
            )
    }

    fn meter(&self, ov: &Ov, window: &mut Window, cx: &mut Context<Self>) -> Div {
        let state = self.state(cx);
        let rows = state.rows();
        if rows.is_empty() {
            return div().child(ov.words("no data yet", 12., ov.c(|t| t.dim)));
        }
        let enemies = state.view == View::EnemyTaken;
        let max = rows.iter().map(|r| r.amount).max().unwrap_or(1);
        let split = enemy_split(&rows);
        let picked: Vec<bool> = rows
            .iter()
            .map(|r| state.compare_slot(&r.key).is_some())
            .collect();
        let mut list = div().flex().flex_col().gap(px(2.));
        for (i, row) in rows.iter().enumerate() {
            if split == Some(i) {
                list = list.child(team_divider(ov));
            }
            let mut drawn = row.clone();
            drawn.enemy = row.enemy || enemies;
            let target = rows::fill(row.amount, max);
            let frac = transition(
                ElementId::from((
                    ElementId::Name("bar".into()),
                    SharedString::from(row.key.clone()),
                )),
                target,
                Transition::new(BAR_EASE).easing(Easing::EaseOut),
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
}

// ---- the footer ---------------------------------------------------------------

impl Overlay {
    fn footer(&self, ov: &Ov, cx: &mut Context<Self>) -> impl IntoElement {
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
        let left = div()
            .flex()
            .items_center()
            .gap(px(8.))
            .child(name)
            .child(trash);

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

#[cfg(test)]
pub(crate) mod tests;
