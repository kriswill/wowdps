//! The overlay's timeline graph (R12, R18, R23; plan steps 2.4 and 2.5):
//! a curve over its marks, drawn as the iced overlay's `compare::Graph`
//! draws it, from gui-logic's `graph::Plot`. One canvas paints, in order:
//! the plot area and its baseline, the encounter lane, each buff's wash,
//! each marker's line hung from the band to the curve, the ghost curve,
//! the curve, the probe's glow, the marker icons over the line, the time
//! cursor and the drag's selection. GPUI keeps that order (spike S8), so
//! the icons sit over the curve inside the one canvas.
//!
//! Gestures are the canvas's own, through window mouse listeners: a drag
//! selects a window, a click does not, a right press resets the zoom (and
//! never reaches the panel's back-out), the icon band hovers a marker and
//! the curve probes an instant. What a gesture says goes to its owner as a
//! [`Event`]; what each graph last said lives in its [`Local`], so two
//! graphs sharing one echo (a comparison) never argue over it.

use std::cell::RefCell;
use std::rc::Rc;

use gpui_kit::prelude::*;
use gpui_kit::{
    App, Bounds, Corners, CursorStyle, DispatchPhase, Div, Hitbox, HitboxBehavior, Hsla,
    MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, PathBuilder, PathStyle, Pixels,
    Point, StrokeOptions, Window, canvas, div, fill, point, px, size,
};
use wowdps_gui_logic::graph::{
    self as gg, ICON_BAND, ICON_SIZE, Plot, curve, for_view, hover_line, kinds_shown, mark_name,
    mmss, mode_word, peak_of, probe_line, view_window,
};
use wowdps_gui_logic::theme::{Color, DataTokens, OverlayTokens};
use wowdps_model::{Class, GraphMode, MarkKind, Timeline};
use wowdps_proto::ClientState;

use super::ov::{Ov, circle};
use crate::images;
use crate::theme::hsla;

/// What one graph last told its owner, and its drag in flight.
#[derive(Debug, Default)]
pub struct Local {
    /// An in-progress drag selection: (anchor x, current x), canvas units.
    pub drag: Option<(f32, f32)>,
    /// The marker label last reported hovered.
    hover: Option<String>,
    /// The bucket last reported probed.
    probe: Option<usize>,
}

pub type Shared = Rc<RefCell<Local>>;

/// What a graph's gestures say.
#[derive(Debug, Clone, PartialEq)]
pub enum Event {
    /// A drag selected this ms window; `None` resets the zoom.
    Range(Option<(u32, u32)>),
    /// The pointer is over this marker's icon (by label), or none.
    Hover(Option<String>),
    /// The pointer is over this bucket of the curve, or off it.
    Probe(Option<usize>),
}

pub type OnEvent = Rc<dyn Fn(Event, &mut Window, &mut App)>;

/// One graph to draw.
pub struct Graph {
    pub plot: Plot,
    pub color: Color,
    /// v16: a context curve drawn faded UNDER the main one, on the same
    /// y-scale (`plot.peak` covers both).
    pub ghost: Option<(Vec<f64>, Color)>,
    /// The overlay's zoom; the icon band and its icons keep their size.
    pub scale: f32,
    /// The marker every graph sharing the echo lights.
    pub hover: Option<String>,
    /// The instant every graph sharing the echo marks.
    pub probe: Option<usize>,
    pub t: OverlayTokens,
    /// The theme's data hues: the marks' colours.
    pub data: DataTokens,
}

/// A stroke whose joins never spike: GPUI exports no round join, and a
/// miter limit of 1 bevels every bend, which at 2 px reads as round.
fn stroke(width: f32) -> PathBuilder {
    PathBuilder::stroke(px(width)).with_style(PathStyle::Stroke(
        StrokeOptions::default()
            .with_line_width(width)
            .with_miter_limit(1.0),
    ))
}

fn line(window: &mut Window, a: Point<Pixels>, b: Point<Pixels>, width: f32, color: Hsla) {
    let mut p = stroke(width);
    p.move_to(a);
    p.line_to(b);
    if let Ok(path) = p.build() {
        window.paint_path(path, color);
    }
}

fn polyline(window: &mut Window, pts: &[Point<Pixels>], width: f32, color: Hsla) {
    let Some((first, rest)) = pts.split_first() else {
        return;
    };
    if rest.is_empty() {
        return;
    }
    let mut p = stroke(width);
    p.move_to(*first);
    for pt in rest {
        p.line_to(*pt);
    }
    if let Ok(path) = p.build() {
        window.paint_path(path, color);
    }
}

fn rect(window: &mut Window, x: f32, y: f32, w: f32, h: f32, color: Hsla) {
    window.paint_quad(fill(
        Bounds::new(point(px(x), px(y)), size(px(w), px(h))),
        color,
    ));
}

impl Graph {
    /// The graph `height` tall across its parent, `local` its own state,
    /// `on` its owner's ear.
    pub fn element(self, height: Pixels, local: Shared, on: OnEvent) -> impl IntoElement {
        let graph = Rc::new(self);
        canvas(
            |bounds, window, _| window.insert_hitbox(bounds, HitboxBehavior::Normal),
            move |bounds, hitbox, window, _| {
                graph.paint(bounds, &hitbox, &local.borrow(), window);
                listen(graph, bounds, local, on, window);
            },
        )
        .w_full()
        .h(height)
    }

    fn paint(&self, bounds: Bounds<Pixels>, hitbox: &Hitbox, local: &Local, window: &mut Window) {
        let (ox, oy) = (f32::from(bounds.origin.x), f32::from(bounds.origin.y));
        let (w, h) = (f32::from(bounds.size.width), f32::from(bounds.size.height));
        let at = |x: f32, y: f32| point(px(ox + x), px(oy + y));
        let p = &self.plot;
        let t = &self.t;

        rect(window, ox, oy, w, h, hsla(t.plot));
        // Baseline: without it an empty graph is indistinguishable from a
        // missing one.
        line(window, at(0.0, h - 0.5), at(w, h - 0.5), 1.0, hsla(t.rule));

        let lane_h = Plot::lane_h(self.scale);
        let floor = p.floor(self.scale);
        let y_of = |v: f64| p.y_of(v, h, floor);

        // v14: the encounter lane, under everything and below the curve's
        // raised floor.
        for &(lo, hi) in &p.spans {
            let x1 = p.x_of(lo as f64 / p.bucket_ms, w).clamp(0.0, w);
            let x2 = p.x_of(hi as f64 / p.bucket_ms, w).clamp(0.0, w);
            if x2 > x1 {
                rect(
                    window,
                    ox + x1,
                    oy + h - lane_h,
                    x2 - x1,
                    lane_h,
                    hsla(t.good.alpha(0.9)),
                );
            }
        }

        // Markers first: the curve reads on top of them. While an item is
        // hovered — on either graph — its uses flare and the rest recede.
        let hovered = self.hover.as_deref();
        let visible: Vec<_> = p.marks.iter().filter(|m| p.mark_visible(m)).collect();
        // v13: a buff's active span, a light wash from application to
        // removal, under everything else.
        for m in visible.iter().filter(|m| m.dur_ms > 0) {
            let x1 = p.mark_x(m, w).clamp(0.0, w);
            let x2 = p
                .x_of((m.at_ms + m.dur_ms) as f64 / p.bucket_ms, w)
                .clamp(0.0, w);
            if x2 <= x1 {
                continue;
            }
            let a = match (hovered, hovered == Some(m.label.as_str())) {
                (Some(_), true) => 0.20,
                (Some(_), false) => 0.04,
                (None, _) => 0.10,
            };
            rect(
                window,
                ox + x1,
                oy,
                x2 - x1,
                h,
                hsla(gg::mark_color(m.kind, &self.data).alpha(a)),
            );
        }
        // Each marker's line drops from its icon and stops where it meets
        // the curve — full-height lines turn a long fight into a picket
        // fence.
        let ghost = self.ghost.as_ref().map(|(g, _)| g.as_slice());
        for m in &visible {
            let x = p.mark_x(m, w).clamp(0.0, w);
            let (a, width) = match (hovered, hovered == Some(m.label.as_str())) {
                (Some(_), true) => (1.0, 2.5),
                (Some(_), false) => (0.12, 1.0),
                (None, _) => (0.40, 1.0),
            };
            let hang = p.hang_y(m, ghost, h, floor);
            line(
                window,
                at(x, 0.0),
                at(x, hang),
                width,
                hsla(gg::mark_color(m.kind, &self.data).alpha(a)),
            );
        }

        // The window's slice of a curve, as points.
        let trace = |points: &[f64], lo: usize, hi: usize| -> Vec<Point<Pixels>> {
            let hi = hi.min(points.len());
            let lo = lo.min(hi);
            points
                .get(lo..hi)
                .unwrap_or_default()
                .iter()
                .enumerate()
                .map(|(i, v)| at(p.x_of((lo + i) as f64, w), y_of(*v)))
                .collect()
        };
        // v16: the context curve first, faded, so the focus line on top
        // reads as "this ability's share of that".
        if let Some((g, gc)) = &self.ghost {
            polyline(
                window,
                &trace(g, p.view.0, p.view.1),
                1.0,
                hsla(gc.alpha(0.30)),
            );
        }
        polyline(
            window,
            &trace(&p.points, p.view.0, p.view.1),
            2.0,
            hsla(self.color),
        );

        // The probe's glow: the curve itself lit around the bucket under
        // the pointer, in layered strokes ALONG the line — widest and
        // faintest over the longest stretch — so the line masks the glow.
        let mouse = window.mouse_position();
        let pointer = bounds
            .contains(&mouse)
            .then(|| (f32::from(mouse.x) - ox, f32::from(mouse.y) - oy));
        if let Some((b, _)) = pointer.and_then(|(x, _)| p.probe_at(x, w)) {
            let lit = self.color.lighten(0.45);
            for (half, width, color) in [
                (4, 4.5, lit.alpha(0.22)),
                (2, 2.5, lit.alpha(0.55)),
                (1, 1.5, self.color.lighten(0.65)),
            ] {
                let lo = b.saturating_sub(half).max(p.view.0);
                let hi = (b + half + 1).min(p.view.1);
                polyline(
                    window,
                    &trace(&p.points, lo, hi),
                    width * self.scale,
                    hsla(color),
                );
            }
        }

        // The item icons over the line, in the top band: the game's own
        // art when the spell-icon cache knows the id, else a chip in the
        // kind's colour. A receding icon is the plot's own ground laid
        // over it at the opacity it loses.
        for m in &visible {
            let x = p
                .mark_x(m, w)
                .clamp(ICON_SIZE / 2.0, (w - ICON_SIZE / 2.0).max(ICON_SIZE / 2.0));
            let hit = hovered == Some(m.label.as_str());
            let recede = hovered.is_some() && !hit;
            let r = Bounds::new(
                point(px(ox + x - ICON_SIZE / 2.0), px(oy + 2.0)),
                size(px(ICON_SIZE), px(ICON_SIZE)),
            );
            match images::spell_icon(m.spell_id) {
                Some(tile) => {
                    let _ = window.paint_image(r, r, Corners::default(), tile, 0, false);
                    if recede {
                        window.paint_quad(fill(r, hsla(t.panel.alpha(0.65))));
                    }
                }
                None => window.paint_quad(fill(
                    r,
                    hsla(gg::mark_color(m.kind, &self.data).alpha(if recede { 0.3 } else { 0.9 })),
                )),
            }
            if hit {
                let ring = r.dilate(px(1.0));
                let (l, tp) = (ring.origin.x, ring.origin.y);
                let (rr, bt) = (l + ring.size.width, tp + ring.size.height);
                let mut path = stroke(1.5);
                path.move_to(point(l, tp));
                path.line_to(point(rr, tp));
                path.line_to(point(rr, bt));
                path.line_to(point(l, bt));
                path.close();
                if let Ok(path) = path.build() {
                    window.paint_path(path, hsla(t.ink));
                }
            }
        }

        // v27: the time cursor — one instant, echoed to every graph sharing
        // it, a dot where THIS graph's curve is then.
        if let Some(b) = self.probe
            && b >= p.view.0
            && b < p.view.1
        {
            let x = p.x_of(b as f64 + 0.5, w).clamp(0.0, w);
            line(window, at(x, ICON_BAND), at(x, h), 1.0, hsla(t.cursor));
            if let Some(v) = p.points.get(b) {
                let mut dot = PathBuilder::fill();
                let c = at(x, y_of(*v));
                circle(&mut dot, c.x, c.y, px(2.5));
                if let Ok(path) = dot.build() {
                    window.paint_path(path, hsla(self.color.lighten(0.4)));
                }
            }
        }

        // The drag in flight, over everything.
        if let Some((a, b)) = local.drag
            && (b - a).abs() >= gg::DRAG_MIN_PX
        {
            let (lo, hi) = (a.min(b), a.max(b));
            rect(window, ox + lo, oy, hi - lo, h, hsla(t.select));
            for x in [lo, hi] {
                line(window, at(x, 0.0), at(x, h), 1.0, hsla(t.select_edge));
            }
        }

        let over_icon = pointer.is_some_and(|(x, y)| p.mark_at(x, y, w).is_some());
        let style = if local.drag.is_some() {
            CursorStyle::ResizeLeftRight
        } else if over_icon {
            CursorStyle::PointingHand
        } else {
            CursorStyle::Crosshair
        };
        window.set_cursor_style(style, hitbox);
    }
}

/// The graph's gestures, for the frame just painted.
fn listen(
    graph: Rc<Graph>,
    bounds: Bounds<Pixels>,
    local: Shared,
    on: OnEvent,
    window: &mut Window,
) {
    let w = f32::from(bounds.size.width);
    let local_x = move |pos: Point<Pixels>| f32::from(pos.x - bounds.origin.x);
    {
        let (local, on) = (local.clone(), on.clone());
        window.on_mouse_event(move |e: &MouseDownEvent, phase, window, cx| {
            if phase != DispatchPhase::Bubble || !bounds.contains(&e.position) {
                return;
            }
            match e.button {
                MouseButton::Left => {
                    let x = local_x(e.position);
                    local.borrow_mut().drag = Some((x, x));
                    window.refresh();
                    cx.stop_propagation();
                }
                // Zoom back out — taken even when already unzoomed, so a
                // right press on the graph never backs out of the drill.
                MouseButton::Right => {
                    on(Event::Range(None), window, cx);
                    cx.stop_propagation();
                }
                _ => {}
            }
        });
    }
    {
        let (local, on, graph) = (local.clone(), on.clone(), graph.clone());
        window.on_mouse_event(move |e: &MouseMoveEvent, phase, window, cx| {
            if phase != DispatchPhase::Bubble {
                return;
            }
            let mut l = local.borrow_mut();
            if let Some((a, _)) = l.drag {
                // Off-canvas motion keeps scrubbing, clamped to the edge.
                l.drag = Some((a, local_x(e.position).clamp(0.0, w)));
                drop(l);
                window.refresh();
                return;
            }
            let inside = bounds.contains(&e.position);
            let (x, y) = (
                local_x(e.position),
                f32::from(e.position.y - bounds.origin.y),
            );
            // The icon band hovers a marker; a hover change wins the turn,
            // the probe catches up on the next move.
            let over = inside
                .then(|| graph.plot.mark_at(x, y, w).map(|m| m.label.clone()))
                .flatten();
            if over != l.hover {
                l.hover = over.clone();
                drop(l);
                on(Event::Hover(over), window, cx);
                return;
            }
            let probed = inside
                .then(|| graph.plot.probe_at(x, w).map(|(b, _)| b))
                .flatten();
            if probed != l.probe {
                l.probe = probed;
                drop(l);
                on(Event::Probe(probed), window, cx);
            }
        });
    }
    window.on_mouse_event(move |e: &MouseUpEvent, phase, window, cx| {
        if phase != DispatchPhase::Bubble || e.button != MouseButton::Left {
            return;
        }
        let Some((a, b)) = local.borrow_mut().drag.take() else {
            return;
        };
        match graph.plot.drag_range(a, b, w) {
            Some(range) => {
                on(Event::Range(Some(range)), window, cx);
                cx.stop_propagation();
            }
            None => window.refresh(),
        }
    });
}

/// The legend under a graph: a hovered marker's name and numbers when one
/// is lit; else the probed instant and its reading, the zoom window, and
/// a key per kind on screen.
pub fn legend(
    ov: &Ov,
    mode: GraphMode,
    shown: Option<(u32, u32)>,
    probe: Option<(String, Vec<String>)>,
    rate: &'static str,
    hover: Option<(MarkKind, String, String)>,
    kinds: &[MarkKind],
) -> Div {
    let dim = ov.c(|t| t.dim);
    let focus = ov.c(|t| t.yellow);
    let swatch = |kind: MarkKind| ov.words("▌", 11., hsla(gg::mark_color(kind, &ov.data)));
    if let Some((kind, name, details)) = hover {
        return div()
            .flex()
            .items_center()
            .gap(px(6.))
            .child(swatch(kind))
            .child(ov.words(name, 10., ov.c(|t| t.ink)))
            .child(ov.words(details, 10., dim));
    }
    let window = shown.map(|(lo, hi)| {
        ov.words(
            format!("{}–{} · right-click resets", mmss(lo), mmss(hi)),
            10.,
            focus,
        )
    });
    let keys = || {
        kinds.iter().map(|&k| {
            div()
                .flex()
                .items_center()
                .gap(px(2.))
                .child(swatch(k))
                .child(ov.words(mark_name(k), 10., dim))
        })
    };
    // Two curves, two readings: the row splits into halves the width of
    // the panes above, meeting AT the divider, so each number sits under
    // its own graph.
    if let Some((when, values)) = &probe
        && let [left_read, right_read] = values.as_slice()
    {
        let left = div()
            .flex_1()
            .min_w_0()
            .flex()
            .items_center()
            .gap(px(10.))
            .child(ov.words(format!("{when} · {}", mode_word(mode, rate)), 10., dim))
            .children(window)
            .child(div().flex_1())
            .child(ov.words(left_read.clone(), 10., focus));
        let right = div()
            .flex_1()
            .min_w_0()
            .flex()
            .items_center()
            .gap(px(10.))
            .child(ov.words(right_read.clone(), 10., focus))
            .child(div().flex_1())
            .children(keys());
        return div()
            .flex()
            .items_center()
            .gap(px(10.))
            .child(left)
            .child(right);
    }
    let mut line = div().flex().items_center().gap(px(10.));
    if let Some((when, values)) = probe {
        line = line.child(ov.words(when, 10., dim));
        for v in values.into_iter().filter(|v| !v.is_empty()) {
            line = line.child(ov.words(v, 10., focus));
        }
    }
    line.children(window).child(div().flex_1()).children(keys())
}

/// A class's bar colour on a graph: the class's own, else classless grey.
pub fn class_color(ov: &Ov, class: Option<Class>) -> Color {
    class.map_or(ov.t.classless, Color::of_class)
}

/// The drilled player's graph (v14), as the iced overlay's `drill_graph`:
/// their timeline, narrowed to the view's marks; on an ability drill (v16)
/// the ability's own curve in `focus`'s colour over the player's ghosted
/// line, one y-scale over both. The zoom is the client's own window over
/// a timeline that always arrives whole.
pub fn drill(
    ov: &Ov,
    app: &ClientState,
    t: &Timeline,
    class: Option<Class>,
    focus: Option<(&Timeline, Color)>,
    hover: Option<String>,
    probe: Option<usize>,
) -> (Graph, Div) {
    // v34: the marks this VIEW is about, on the ghost and the focus alike.
    let t = for_view(t, app.view);
    let focus = focus.map(|(ft, c)| (for_view(ft, app.view), c));
    let mode = app.graph_mode();
    let shown = app.drill_range();
    let rate = wowdps_gui_logic::labels::rate_label(app.view);
    // The window always spans the PLAYER's timeline: the x-axis must not
    // reshape when drilling in or out of an ability.
    let span = t.buckets.len().max(1);
    let window = view_window(shown, t.bucket_ms.max(1) as usize, span);
    let reading = probe.map(|at| {
        let points = focus
            .as_ref()
            .map_or_else(|| curve(&t, mode), |(ft, _)| curve(ft, mode));
        probe_line(
            at,
            t.bucket_ms.max(1) as usize,
            mode_word(mode, rate),
            &[(String::new(), points)],
        )
    });
    // R18: casters resolve through the meter rows in hand.
    let rows = app.rows();
    let names: Vec<(&str, &str)> = rows
        .iter()
        .map(|r| (r.key.as_str(), r.label.as_str()))
        .collect();
    let hovered = hover
        .as_deref()
        .and_then(|l| hover_line(&[&t], l, window, &names, false, &[]));
    let kinds = kinds_shown(&[&t], window);
    let spans = app.encounter_spans();
    let player = class_color(ov, class);
    let graph = match &focus {
        Some((ft, fc)) => Graph {
            plot: Plot::new(ft, mode, peak_of(&[&t, ft], mode, window), window, spans),
            color: *fc,
            ghost: Some((curve(&t, mode), player)),
            scale: ov.zoom,
            hover,
            probe,
            t: ov.t,
            data: ov.data,
        },
        None => Graph {
            plot: Plot::new(&t, mode, peak_of(&[&t], mode, window), window, spans),
            color: player,
            ghost: None,
            scale: ov.zoom,
            hover,
            probe,
            t: ov.t,
            data: ov.data,
        },
    };
    let legend = legend(ov, mode, shown, reading, rate, hovered, &kinds);
    (graph, legend)
}
