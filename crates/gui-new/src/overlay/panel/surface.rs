//! The overlay on its edge strip (plan step 2.2, spec §7.2): where the
//! content sits on the strip, the input region that is exactly the content,
//! the grip that tells a click from a drag and slides the content along the
//! edge, the tab dragged onto another edge, and whether the overlay shows
//! at all — the daemon's wish (`SetVisible`) composed with the game's
//! workspace being on screen (`follow_game`).
//!
//! gpui-pre sets a layer surface's margin only when it is created, so the
//! surface spans its edge's whole length and never moves; a drag moves the
//! content and the input region inside it. Changing edge is the one thing
//! that recreates the surface (`Reanchor`).

use std::time::Duration;

use gpui_kit::prelude::*;
use gpui_kit::{
    App, Bounds, Context, DispatchPhase, EventEmitter, MouseButton, MouseMoveEvent, MouseUpEvent,
    Pixels, Point, Window, canvas, point, px, size,
};
use wowdps_gui_logic::config::{Config, Edge};
use wowdps_gui_logic::hypr;
use wowdps_gui_logic::surface::{nearest_edge, surface_size, tab_size};
use wowdps_model::Screen;

use super::super::strip::{Rect, Strip};
use super::Overlay;

/// A press that travels less than this many pixels is a click, not a drag.
pub const DRAG_THRESHOLD: f32 = 5.0;

/// How often the game's workspace is asked about.
const GAME_POLL: Duration = Duration::from_millis(100);

/// The grip pressed: where the pointer went down (window coordinates), the
/// content's offset then, and whether it has travelled far enough to be a
/// drag.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Grip {
    at: Point<Pixels>,
    from: f32,
    pub moved: bool,
}

/// The tab was dropped by another edge: the surface is recreated there.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Reanchor(pub Edge);

impl EventEmitter<Reanchor> for Overlay {}

/// What the surface holds this frame: the strip, and the content's size
/// and place on it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Placed {
    pub strip: Strip,
    pub content: (f32, f32),
    pub rect: Rect,
}

impl Overlay {
    /// Shown: the daemon wants it and the game's workspace is on screen.
    pub(crate) fn shown(&self) -> bool {
        self.daemon_visible && self.game_visible
    }

    /// Size the surface for the state, place the content on the strip, and
    /// fit the input region to it. `None` when hidden: a 1 px surface that
    /// draws nothing and takes no input, since layer-shell has no unmap.
    pub(super) fn place(&mut self, window: &mut Window, cx: &App) -> Option<Placed> {
        let vp = window.viewport_size();
        let strip = Strip {
            edge: self.cfg.edge,
            screen: (f32::from(vp.width), f32::from(vp.height)),
        };
        let comparing = self.state(cx).screen == Screen::Compare;
        let (w, h) = surface_size(&self.cfg, self.expanded, comparing);
        let content = (w as f32, h as f32);
        let shown = self.shown();
        let across = if shown { content } else { (1.0, 1.0) };
        let want = strip.surface(across);
        let want_px = (want.0.round() as u32, want.1.round() as u32);
        // An unconfigured surface has no length yet: the compositor's
        // configure gives it the edge's, and the next frame sizes it.
        if strip.length() > 0.0 && self.sized != Some(want_px) {
            self.sized = Some(want_px);
            window.resize(size(px(want.0), px(want.1)));
        }
        let rect = strip.place(content, self.offset);
        let region = shown.then_some(rect);
        if self.region != Some(region) {
            self.region = Some(region);
            let bounds: Vec<Bounds<Pixels>> = region
                .map(|r| Bounds::new(point(px(r.x), px(r.y)), size(px(r.w), px(r.h))))
                .into_iter()
                .collect();
            window.set_input_region(Some(&bounds));
        }
        let placed = Placed {
            strip,
            content,
            rect,
        };
        self.placed = Some(placed);
        shown.then_some(placed)
    }

    /// The grip pressed (the header, the tab): a click or a drag, decided
    /// by how far it travels before it is let go.
    pub(super) fn grip_down(&mut self, at: Point<Pixels>) {
        self.grip = Some(Grip {
            at,
            from: self.offset,
            moved: false,
        });
    }

    /// The pointer moved with the grip held: past the threshold it is a
    /// drag, and the content follows it along the edge.
    pub(super) fn grip_moved(&mut self, to: Point<Pixels>, cx: &mut Context<Self>) {
        let (Some(grip), Some(placed)) = (self.grip.as_mut(), self.placed) else {
            return;
        };
        let travel = (f32::from(to.x - grip.at.x), f32::from(to.y - grip.at.y));
        if !grip.moved && travel.0.hypot(travel.1) > DRAG_THRESHOLD {
            grip.moved = true;
        }
        if grip.moved {
            let offset = placed.strip.dragged(placed.content, grip.from, travel);
            if offset != self.offset {
                self.offset = offset;
                cx.notify();
            }
        }
    }

    /// The grip let go: a click toggles the panel; a drag is a deliberate
    /// placement, remembered — and a tab dropped by another edge of its
    /// monitor moves there.
    pub(super) fn grip_up(&mut self, cx: &mut Context<Self>) {
        let Some(grip) = self.grip.take() else {
            return;
        };
        if !grip.moved {
            self.toggle(cx);
            return;
        }
        if !self.expanded
            && let Some((edge, offset)) = self.dropped_edge()
        {
            self.cfg.edge = edge;
            self.offset = offset;
            Config::store(|c| {
                c.edge = edge;
                c.offset = offset.round() as i32;
            });
            cx.emit(Reanchor(edge));
            cx.notify();
            return;
        }
        let offset = self.offset.round() as i32;
        self.cfg.offset = offset;
        Config::store(|c| c.offset = offset);
        cx.notify();
    }

    /// Under Hyprland, the edge of the pointer's monitor a dropped tab
    /// lands on, when it is near enough another edge to flip to it (gui-logic
    /// `nearest_edge`'s capture and hysteresis), with the tab centred under
    /// the pointer along it.
    fn dropped_edge(&self) -> Option<(Edge, f32)> {
        let dir = self.hypr.as_deref()?;
        let (x, y) = hypr::cursor_pos(dir)?;
        let mon = hypr::monitor_at(dir, (x, y))?;
        let (mx, my, mw, mh) = mon;
        let p = (
            x.clamp(mx, mx + mw - 1) as f32,
            y.clamp(my, my + mh - 1) as f32,
        );
        let edge = nearest_edge(self.cfg.edge, p, mon)?;
        let (tw, th) = tab_size(edge, self.cfg.zoom);
        let (along, len, span) = if edge.is_vertical() {
            (p.1 - my as f32, mh as f32, th as f32)
        } else {
            (p.0 - mx as f32, mw as f32, tw as f32)
        };
        Some((edge, (along - span / 2.0).clamp(0.0, (len - span).max(0.0))))
    }

    /// The window-wide ear for a drag in flight: Wayland keeps sending the
    /// pointer to the surface that took the press until it is let go, even
    /// off the content and off the strip.
    pub(super) fn grip_listener(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let this = cx.entity().downgrade();
        canvas(
            |_, _, _| {},
            move |_, (), window, _| {
                let moving = this.clone();
                window.on_mouse_event(move |e: &MouseMoveEvent, phase, _, cx| {
                    if phase == DispatchPhase::Bubble {
                        let _ = moving.update(cx, |o, cx| o.grip_moved(e.position, cx));
                    }
                });
                let releasing = this.clone();
                window.on_mouse_event(move |e: &MouseUpEvent, phase, _, cx| {
                    if phase == DispatchPhase::Bubble && e.button == MouseButton::Left {
                        let _ = releasing.update(cx, |o, cx| o.grip_up(cx));
                    }
                });
            },
        )
        .absolute()
        .size_0()
    }

    /// The daemon's wish, composed with the game's (`shown`).
    pub(super) fn set_daemon_visible(&mut self, visible: bool, cx: &mut Context<Self>) {
        if self.daemon_visible != visible {
            self.daemon_visible = visible;
            cx.notify();
        }
    }

    /// Under Hyprland with `follow_game`, follow the game's workspace: the
    /// tracking thread says whether it is on screen, and this polls it.
    pub(super) fn follow_game(&mut self, cx: &mut Context<Self>) {
        let Some(rx) = (self.cfg.follow_game && self.hypr.is_some())
            .then(|| hypr::spawn(self.cfg.game_match.clone()))
            .flatten()
        else {
            return;
        };
        self._game = Some(cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(GAME_POLL).await;
                let mut latest = None;
                while let Ok(v) = rx.try_recv() {
                    latest = Some(v);
                }
                let alive = this.update(cx, |o, cx| {
                    if let Some(v) = latest
                        && o.game_visible != v
                    {
                        o.game_visible = v;
                        cx.notify();
                    }
                });
                if alive.is_err() {
                    break;
                }
            }
        }));
    }
}

impl Overlay {
    /// A new surface (a new edge): nothing has been asked of it yet.
    pub(crate) fn fresh_surface(&mut self) {
        self.sized = None;
        self.region = None;
    }
}
