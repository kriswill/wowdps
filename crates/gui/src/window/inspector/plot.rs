//! The inspector's graph (plan step 3.3; the prototype's `.igraph`, the
//! iced window's `inspector/plot.rs`): one canvas with the curve to itself
//! — an area in the player's class colour, two lines on one scale for a
//! comparison, an ability over its ghost, or R26's stacked bands — a time
//! axis in minutes, a death hatched from the moment to the rez, and the
//! lanes of spans under it, a comparison's split in two.
//!
//! Where everything stands is gui-logic's `inspect::geometry`, laid out at
//! zoom 1 exactly as the iced window lays it out; this component measures
//! its words, multiplies by the window's zoom and paints in iced's order.
//! GPUI keeps a canvas's paint order (spike S8), so the words land over the
//! shapes as written — every label a tooltip covers is still left out, as
//! the iced canvas must, so the two pictures agree.
//!
//! [`plot`] is a component over plain data ([`Input`]): its hover and its
//! drag are its own keyed state (or an [`Entity<State>`] its owner hands
//! it), and a drag-selected window — or a right press's "the whole fight
//! back" — goes to its owner through [`OnRange`]. Its height follows its
//! lanes ([`Input::height`]), as iced's `Plot::height()` does.
//!
//! The delights, each gone under reduced motion so the picture is iced's:
//! - a zoom glides — the axis, the curves, the hatches and the spans slide
//!   to the new window over 220 ms (ease-out);
//! - the crosshair glows, with a dot where it meets each curve;
//! - a drag in flight shows its edges in gold and the window it selects.

mod curves;
mod hatch;
mod lanes;
mod tip;

#[cfg(test)]
mod shots;
#[cfg(test)]
mod tests;

use std::rc::Rc;
use std::time::Duration;

use gpui_kit::base::{MotionStatus, MotionValue, Transition, transition_with_status};
use gpui_kit::prelude::*;
use gpui_kit::{
    App, Bounds, CursorStyle, DispatchPhase, ElementId, Entity, Hitbox, HitboxBehavior,
    MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, Pixels, Point, TestSupportExt as _,
    Window, canvas, div, point, px, size,
};
use wowdps_gui_logic::inspect::geometry::{self as geo, DRAG_MIN_PX, Hover};
use wowdps_gui_logic::inspect::lanes::Row as LaneRow;
use wowdps_gui_logic::theme::{DataTokens, Effects, Shape, WindowTokens};

pub use wowdps_gui_logic::inspect::plot::{Curve, Dead};

use crate::window::w::W;

/// A zoom's glide to its new window.
const GLIDE: Duration = Duration::from_millis(220);

/// Everything the graph draws, owned: the inspector builds it from the
/// snapshot, the component draws it.
#[derive(Debug, Clone, PartialEq)]
pub struct Input {
    /// The stretch of the fight on show, ms: the whole of it, or a zoom.
    pub window: (u32, u32),
    /// The one scale every curve is drawn on.
    pub peak: f64,
    pub curves: Vec<Curve>,
    pub dead: Vec<Dead>,
    pub lanes: Vec<LaneRow>,
    /// The hover reads the curve as a running total ("63.0M") rather than
    /// a rate ("149,258").
    pub total: bool,
    /// What the hover calls a lone curve's value ("dps").
    pub word: &'static str,
    /// The plot's height at zoom 1: `geo::PLOT_H` beside the meter, more in
    /// an inspector widened over the stage.
    pub plot_h: f32,
}

impl Input {
    /// The graph's geometry over `window` (the one on show, mid-glide).
    pub fn geo(&self, window: (u32, u32)) -> geo::Plot<'_> {
        geo::Plot {
            window,
            peak: self.peak,
            curves: &self.curves,
            dead: &self.dead,
            lanes: &self.lanes,
            total: self.total,
            word: self.word,
            plot_h: self.plot_h,
        }
    }

    /// The graph's height at zoom 1: the plot, its axis and its lanes.
    pub fn height(&self) -> f32 {
        self.geo(self.window).height()
    }
}

/// What a drag-selected window (or a right press's "the whole fight
/// back", `None`) becomes.
pub type OnRange = Rc<dyn Fn(Option<(u32, u32)>, &mut Window, &mut App)>;

/// The graph's memory between frames: what the pointer is over, a drag in
/// flight (anchor x, current x, canvas units at zoom 1) — and what the
/// last frame drew: its window and its tooltip.
#[derive(Debug, Default)]
pub struct State {
    hover: Option<Hover>,
    drag: Option<(f32, f32)>,
    shown: Option<(u32, u32)>,
    tip: Option<geo::Tip>,
}

#[cfg(test)]
impl State {
    pub fn hover(&self) -> Option<Hover> {
        self.hover
    }

    pub fn drag(&self) -> Option<(f32, f32)> {
        self.drag
    }

    /// The window the last frame drew: the input's, or one on its way
    /// there.
    pub fn shown(&self) -> Option<(u32, u32)> {
        self.shown
    }

    /// The tooltip the last frame painted, if any.
    pub fn tip(&self) -> Option<&geo::Tip> {
        self.tip.as_ref()
    }
}

/// The graph as an element: `id` names it (and keys its state).
#[derive(IntoElement)]
pub struct Plot {
    id: ElementId,
    input: Input,
    w: W,
    on_range: Option<OnRange>,
    state: Option<Entity<State>>,
}

/// The graph of `input`, at the window's zoom; unzoomable until
/// [`Plot::on_range`] gives it somewhere to send a window.
pub fn plot(id: impl Into<ElementId>, input: Input, w: &W) -> Plot {
    Plot {
        id: id.into(),
        input,
        w: w.clone(),
        on_range: None,
        state: None,
    }
}

impl Plot {
    /// Where a drag's window and a right press's reset go.
    pub fn on_range(mut self, on_range: OnRange) -> Self {
        self.on_range = Some(on_range);
        self
    }

    #[cfg(test)]
    /// Keep the hover and the drag in `state` (the owner's) rather than the
    /// element's own keyed state.
    pub fn state(mut self, state: Entity<State>) -> Self {
        self.state = Some(state);
        self
    }
}

/// One frame's graph: what it draws, over which window, and its ears.
struct Frame {
    input: Input,
    /// The window on show: the input's, or mid-glide toward it.
    shown: (u32, u32),
    zoom: f32,
    t: WindowTokens,
    ui: gpui_kit::SharedString,
    shape: Shape,
    fx: Effects,
    data: DataTokens,
    on_range: Option<OnRange>,
    state: Entity<State>,
    /// Reduced motion: the parity pixels, no delights.
    still: bool,
}

impl Frame {
    fn geo(&self) -> geo::Plot<'_> {
        self.input.geo(self.shown)
    }
}

/// Where everything is painted from: the canvas's origin, the zoom and
/// the tokens — every geometry unit times `z`. The face is the shaper's.
#[derive(Clone, Copy)]
pub(super) struct Pen {
    pub o: Point<Pixels>,
    pub z: f32,
    pub t: WindowTokens,
    /// The theme's corners and effects.
    pub shape: Shape,
    pub fx: Effects,
    /// The data hues: the stack's rest is drawn by them.
    pub data: DataTokens,
}

impl Pen {
    pub fn at(&self, x: f32, y: f32) -> Point<Pixels> {
        point(self.o.x + px(x * self.z), self.o.y + px(y * self.z))
    }

    pub fn rect(&self, x: f32, y: f32, w: f32, h: f32) -> Bounds<Pixels> {
        Bounds::new(self.at(x, y), size(px(w * self.z), px(h * self.z)))
    }

    pub fn px(&self, v: f32) -> Pixels {
        px(v * self.z)
    }

    /// A corner the design draws at `v`, at the theme's shape.
    pub fn r(&self, v: f32) -> Pixels {
        px(self.shape.radius(v) * self.z)
    }
}

impl RenderOnce for Plot {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let state = match self.state {
            Some(s) => s,
            None => window.use_keyed_state(self.id.clone(), cx, |_, _| State::default()),
        };
        let shown = glide(&self.id, self.input.window, window, cx);
        let height = self.input.height();
        let frame = Rc::new(Frame {
            input: self.input,
            shown,
            zoom: self.w.zoom,
            t: self.w.t,
            ui: self.w.ui.clone(),
            shape: self.w.shape,
            fx: self.w.fx,
            data: self.w.data,
            on_range: self.on_range,
            state,
            still: cx.reduce_motion(),
        });
        div()
            .id(self.id)
            .test_support()
            .flex_none()
            .w_full()
            .h(px(height * self.w.zoom))
            .child(
                canvas(
                    |b, window, _| window.insert_hitbox(b, HitboxBehavior::Normal),
                    move |b, hitbox, window, cx| {
                        paint(&frame, b, &hitbox, window, cx);
                        listen(frame, b, hitbox, window);
                    },
                )
                .size_full(),
            )
    }
}

/// The window on show: `to` at once the first time and under reduced
/// motion, else gliding to it from wherever the last one stood.
fn glide(id: &ElementId, to: (u32, u32), window: &mut Window, cx: &mut App) -> (u32, u32) {
    let lo = transition_with_status(
        (id.clone(), "glide-lo"),
        to.0 as f32,
        Transition::new(GLIDE),
        window,
        cx,
    );
    let hi = transition_with_status(
        (id.clone(), "glide-hi"),
        to.1 as f32,
        Transition::new(GLIDE),
        window,
        cx,
    );
    let moving = |m: &MotionValue<f32>| matches!(m.status, MotionStatus::Running);
    if !moving(&lo) && !moving(&hi) {
        return to;
    }
    let (a, b) = (lo.value.max(0.0).round(), hi.value.max(0.0).round());
    if b > a { (a as u32, b as u32) } else { to }
}

/// The frame painted in the iced canvas's order: the baseline, the
/// hatches, the stack, the curves, the hatches' word patches, the lanes,
/// a drag, the crosshair, the words, the tooltip over all.
fn paint(f: &Frame, b: Bounds<Pixels>, hitbox: &Hitbox, window: &mut Window, cx: &mut App) {
    let pen = Pen {
        o: b.origin,
        z: f.zoom,
        t: f.t,
        shape: f.shape,
        fx: f.fx,
        data: f.data,
    };
    let w = f32::from(b.size.width) / f.zoom;
    let g = f.geo();
    let (hover, drag) = {
        let s = f.state.read(cx);
        (s.hover, s.drag)
    };
    let shaper = tip::Shaper::new(window, pen, f.ui.clone());
    let measure = |s: &str, size: f32, face: geo::Face| shaper.width(s, size, face);
    // The hatches' words, placed once: their band is what the curves
    // peak under.
    let hatches = g.hatch_labels(w, &measure);
    let top = geo::Plot::top_of(&hatches);

    // The baseline, a hairline.
    let left = g.left();
    window.paint_quad(gpui_kit::fill(
        pen.rect(
            left,
            g.plot_h - geo::HAIRLINE,
            (w - left).max(0.0),
            geo::HAIRLINE,
        ),
        crate::theme::hsla(pen.t.line),
    ));
    hatch::stripes(&g, w, pen, window);
    curves::bands(&g, w, top, pen, window);
    curves::lines(&g, w, top, pen, window);
    hatch::patches(&hatches, pen, window);
    let lit = match hover {
        Some(Hover::Span(l, i)) => Some((l, i)),
        _ => None,
    };
    lanes::paint(&g, w, lit, pen, window);
    if let Some((a, b)) = drag
        && (b - a).abs() >= DRAG_MIN_PX
    {
        curves::drag(
            &g,
            w,
            (a.min(b), a.max(b)),
            f.still,
            &shaper,
            pen,
            window,
            cx,
        );
    }
    if let Some(Hover::Plot(x)) = hover {
        curves::crosshair(&g, w, x, top, f.still, pen, window);
    }
    // The reticle, where the theme frames its instruments: an L at each
    // corner of the plot, its scale and axis words outside them.
    if pen.fx.brackets {
        crate::window::instruments::paint_brackets(
            window,
            pen.rect(left, 0.0, (w - left).max(0.0), g.plot_h),
            pen.z,
            crate::theme::hsla(pen.t.bracket),
        );
    }
    for l in g.labels(hover, w, &measure) {
        shaper.paint_label(&l, window, cx);
    }
    let tip = hover.and_then(|h| g.tip(h, w, &measure));
    if let Some(tip) = &tip {
        shaper.paint_tip(tip, window, cx);
    }
    // What this frame drew, for the owner (and the tests) to read back;
    // no notify, so recording it never asks for another frame.
    let s = f.state.read(cx);
    if s.tip != tip || s.shown != Some(f.shown) {
        f.state.update(cx, |s, _| {
            s.tip = tip;
            s.shown = Some(f.shown);
        });
    }

    // The pointer's shape: a drag's, the crosshair where a press zooms.
    let style = if drag.is_some() {
        Some(CursorStyle::ResizeLeftRight)
    } else if matches!(hover, Some(Hover::Plot(_))) && f.on_range.is_some() {
        Some(CursorStyle::Crosshair)
    } else {
        None
    };
    if let Some(style) = style {
        window.set_cursor_style(style, hitbox);
    }
}

/// The graph's gestures, for the frame just painted: a press on the plot
/// starts a drag, the release publishes its window (a wander under 3 px
/// is a click and publishes nothing), a right press on the plot asks for
/// the whole fight back, and a move hovers — or, mid-drag, scrubs the
/// drag's end, held to the plot even off the canvas.
fn listen(f: Rc<Frame>, b: Bounds<Pixels>, hitbox: Hitbox, window: &mut Window) {
    let z = f.zoom;
    let w = f32::from(b.size.width) / z;
    let local = move |p: Point<Pixels>| {
        (
            f32::from(p.x - b.origin.x) / z,
            f32::from(p.y - b.origin.y) / z,
        )
    };
    {
        let (f, hitbox) = (f.clone(), hitbox.clone());
        window.on_mouse_event(move |e: &MouseDownEvent, phase, window, cx| {
            if phase != DispatchPhase::Bubble || !hitbox.is_hovered(window) {
                return;
            }
            let Some(on_range) = f.on_range.clone() else {
                return;
            };
            let (x, y) = local(e.position);
            if !f.geo().in_plot(x, y) {
                return;
            }
            match e.button {
                MouseButton::Left => {
                    f.state.update(cx, |s, cx| {
                        s.drag = Some((x, x));
                        cx.notify();
                    });
                    cx.stop_propagation();
                }
                MouseButton::Right => {
                    on_range(None, window, cx);
                    cx.stop_propagation();
                }
                _ => {}
            }
        });
    }
    {
        let f = f.clone();
        window.on_mouse_event(move |e: &MouseMoveEvent, phase, window, cx| {
            if phase != DispatchPhase::Bubble {
                return;
            }
            let (x, y) = local(e.position);
            let g = f.geo();
            if f.state.read(cx).drag.is_some() {
                let x = x.clamp(g.left(), w);
                f.state.update(cx, |s, cx| {
                    if let Some((_, cur)) = s.drag.as_mut()
                        && *cur != x
                    {
                        *cur = x;
                        cx.notify();
                    }
                });
                return;
            }
            let now = if hitbox.is_hovered(window) {
                g.hover_at(x, y, w)
            } else {
                None
            };
            if f.state.read(cx).hover != now {
                f.state.update(cx, |s, cx| {
                    s.hover = now;
                    cx.notify();
                });
            }
        });
    }
    window.on_mouse_event(move |e: &MouseUpEvent, phase, window, cx| {
        if phase != DispatchPhase::Bubble || e.button != MouseButton::Left {
            return;
        }
        let Some((a, b)) = f.state.update(cx, |s, cx| {
            let d = s.drag.take();
            if d.is_some() {
                cx.notify();
            }
            d
        }) else {
            return;
        };
        if let (Some(range), Some(on_range)) = (f.geo().drag_range(a, b, w), f.on_range.clone()) {
            on_range(Some(range), window, cx);
            cx.stop_propagation();
        }
    });
}
