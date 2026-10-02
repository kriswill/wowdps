//! The curves through gui-logic's Catmull-Rom spline (the prototype's
//! `smooth()`), so a rate reads as hills rather than a saw: R26's stacked
//! bands, each from the running sum of those before it to its own top with
//! a 2 px gap of the panel along its top; then the lines — ghosts first,
//! so a focus reads on top of its context — an area of the player's own
//! under theirs, a comparison's second dashed when one colour draws both.
//! And what the pointer adds over them: a drag's window and the gold
//! crosshair.

use gpui_kit::{App, PathBuilder, PathStyle, Pixels, Point, StrokeOptions, Window, fill, px};
use wowdps_gui_logic::inspect::geometry::{
    self as geo, AREA_ALPHA, CURVE_W, DASH, Face, GHOST_ALPHA, GHOST_W, HAIRLINE, STACK_GAP,
    TICK_PX, XHAIR_ALPHA,
};
use wowdps_gui_logic::inspect::plot::Ink;
use wowdps_model::fmt::duration;

use super::Pen;
use super::tip::Shaper;
use crate::theme::hsla;
use crate::window::paint::fill_disc;

/// The crosshair's glow under its hairline: wider, fainter strokes of the
/// gold (the ribbon's).
const XHAIR_GLOW: [(f32, f32); 2] = [(7.0, 0.07), (3.0, 0.16)];
/// A curve's reading under the crosshair: a dot of its colour on a ring of
/// the panel.
const DOT_R: f32 = 2.5;
const DOT_RING: f32 = 1.0;
/// A drag's window words: this far in from its edge and down from the top.
const DRAG_WORDS: (f32, f32) = (4.0, 2.0);

/// A stroke whose bends never spike: GPUI exports no round join, and a
/// miter limit of 1 bevels every bend, which at these widths reads as
/// iced's round one.
fn stroke(width: Pixels) -> PathBuilder {
    PathBuilder::stroke(width).with_style(PathStyle::Stroke(
        StrokeOptions::default()
            .with_line_width(f32::from(width))
            .with_miter_limit(1.0),
    ))
}

/// Trace `pts` (after the pen is at the first) through gui-logic's spline.
fn smooth(path: &mut PathBuilder, pts: &[(f32, f32)], plot_h: f32, pen: Pen) {
    for [c1, c2, to] in geo::smooth(pts, plot_h) {
        path.cubic_bezier_to(pen.at(to.0, to.1), pen.at(c1.0, c1.1), pen.at(c2.0, c2.1));
    }
}

fn at(pen: Pen, p: (f32, f32)) -> Point<Pixels> {
    pen.at(p.0, p.1)
}

/// R26: the stack, bottom band first.
pub fn bands(g: &geo::Plot<'_>, w: f32, top: f32, pen: Pen, window: &mut Window) {
    for band in g.bands(w, top) {
        let (Some(first), Some(last)) = (band.upper.first(), band.upper.last()) else {
            continue;
        };
        let mut area = PathBuilder::fill();
        area.move_to(at(pen, *first));
        smooth(&mut area, &band.upper, g.plot_h, pen);
        if band.lower.is_empty() {
            area.line_to(pen.at(last.0, g.plot_h));
            area.line_to(pen.at(first.0, g.plot_h));
        } else {
            let back: Vec<(f32, f32)> = band.lower.iter().rev().copied().collect();
            if let Some(b0) = back.first() {
                area.line_to(at(pen, *b0));
            }
            smooth(&mut area, &back, g.plot_h, pen);
        }
        area.close();
        if let Ok(path) = area.build() {
            window.paint_path(path, hsla(band.color));
        }
        let mut edge = stroke(pen.px(STACK_GAP));
        edge.move_to(at(pen, *first));
        smooth(&mut edge, &band.upper, g.plot_h, pen);
        if let Ok(path) = edge.build() {
            window.paint_path(path, hsla(pen.t.surface));
        }
    }
}

/// Every curve but the stack's bands, ghosts first.
pub fn lines(g: &geo::Plot<'_>, w: f32, top: f32, pen: Pen, window: &mut Window) {
    for c in g.lines() {
        let pts = g.points(c, w, top);
        let (Some(first), Some(last)) = (pts.first(), pts.last()) else {
            continue;
        };
        if c.ink == Ink::Area {
            let mut area = PathBuilder::fill();
            area.move_to(pen.at(first.0, g.plot_h));
            area.line_to(at(pen, *first));
            smooth(&mut area, &pts, g.plot_h, pen);
            area.line_to(pen.at(last.0, g.plot_h));
            area.close();
            if let Ok(path) = area.build() {
                window.paint_path(path, hsla(c.color.alpha(AREA_ALPHA)));
            }
        }
        let (width, color) = match c.ink {
            Ink::Ghost => (GHOST_W, c.color.alpha(GHOST_ALPHA)),
            Ink::Area | Ink::Line | Ink::Dashed | Ink::Stack => (CURVE_W, c.color),
        };
        let mut line = stroke(pen.px(width));
        if c.ink == Ink::Dashed {
            line = line.dash_array(&DASH.map(|v| px(v * pen.z)));
        }
        line.move_to(at(pen, *first));
        smooth(&mut line, &pts, g.plot_h, pen);
        if let Ok(path) = line.build() {
            window.paint_path(path, hsla(color));
        }
    }
}

/// A drag in flight, `lo..hi` across the plot, over everything but the
/// tooltip — and, the delight, its edges in gold and the window it would
/// select written at its top.
#[allow(clippy::too_many_arguments)]
pub fn drag(
    g: &geo::Plot<'_>,
    w: f32,
    (lo, hi): (f32, f32),
    still: bool,
    shaper: &Shaper,
    pen: Pen,
    window: &mut Window,
    cx: &mut App,
) {
    window.paint_quad(fill(
        pen.rect(lo, 0.0, hi - lo, g.plot_h),
        hsla(pen.t.drag_fill),
    ));
    if still {
        return;
    }
    let gold = pen.t.gold.alpha(XHAIR_ALPHA);
    for x in [lo, hi] {
        window.paint_quad(fill(
            pen.rect(x - HAIRLINE / 2.0, 0.0, HAIRLINE, g.plot_h),
            hsla(gold),
        ));
    }
    let words = format!(
        "{}–{}",
        duration(i64::from(g.ms_at(lo, w))),
        duration(i64::from(g.ms_at(hi, w)))
    );
    let width = shaper.width(&words, TICK_PX, Face::Semibold);
    let x = if lo + DRAG_WORDS.0 + width <= hi {
        lo + DRAG_WORDS.0
    } else if hi + DRAG_WORDS.0 + width <= w {
        hi + DRAG_WORDS.0
    } else {
        (lo - DRAG_WORDS.0 - width).max(g.left())
    };
    shaper.words(
        &words,
        (x, DRAG_WORDS.1),
        TICK_PX,
        Face::Semibold,
        hsla(pen.t.gold),
        window,
        cx,
    );
}

/// The crosshair at `x` (`.xhair`): a gold hairline down the plot — over a
/// soft glow, with a dot where it meets each curve, unless motion is
/// reduced.
pub fn crosshair(
    g: &geo::Plot<'_>,
    w: f32,
    x: f32,
    top: f32,
    still: bool,
    pen: Pen,
    window: &mut Window,
) {
    let gold = pen.t.gold;
    let rule = |window: &mut Window, width: f32, alpha: f32| {
        let mut p = stroke(pen.px(width));
        p.move_to(pen.at(x, 0.0));
        p.line_to(pen.at(x, g.plot_h));
        if let Ok(path) = p.build() {
            window.paint_path(path, hsla(gold.alpha(alpha)));
        }
    };
    if !still {
        for (width, alpha) in XHAIR_GLOW {
            rule(window, width, alpha);
        }
    }
    rule(window, HAIRLINE, XHAIR_ALPHA);
    if still {
        return;
    }
    let ms = g.ms_at(x, w);
    for c in g
        .curves
        .iter()
        .filter(|c| matches!(c.ink, Ink::Area | Ink::Line | Ink::Dashed))
    {
        let Some(v) = c.at(ms) else {
            continue;
        };
        let y = g.y_of(v, top);
        fill_disc(
            window,
            pen.at(x, y),
            pen.px(DOT_R + DOT_RING),
            hsla(pen.t.surface),
        );
        fill_disc(window, pen.at(x, y), pen.px(DOT_R), hsla(c.color));
    }
}
