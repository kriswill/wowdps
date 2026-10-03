//! The instrument marks a theme may add (`theme::Effects`), drawn only
//! where it asks for them — `navy` asks for none, so its pixels are the
//! prototype's — and only on what is an instrument:
//!
//! - **the sub-dial** (`effects.dial`): the inspector's player crest set in
//!   a chronograph's bezel — sixty ticks, every fifth longer — with the
//!   player's share of the view swept from twelve o'clock, clockwise, as an
//!   arc in their class colour, the ticks under the sweep lit. The one place
//!   a theme spends its boldness, and it carries a figure, not a flourish;
//! - **reticle brackets** (`effects.brackets`): an L at each corner of an
//!   instrument's face (the ribbon, the inspector's graph), the way a
//!   sight frames what it reads;
//! - **the chapter ring** (`effects.fine_ticks`): a fine tick per ten
//!   seconds under the ribbon's minute ticks, its own buckets' grid.

use std::f32::consts::{PI, TAU};

use gpui_kit::prelude::*;
use gpui_kit::{
    AnyElement, Bounds, Hsla, PathBuilder, Pixels, Point, Window, canvas, div, point, px,
};

use super::w::W;

/// The bezel's ring beyond the crest, in logical pixels: the ticks sit in
/// it, the sweep at its outer edge.
pub const RING: f32 = 8.0;
/// The ticks, from the crest's edge: a minor one's inner and outer radius,
/// a major (every fifth) one's, and how wide each is stroked.
const MINOR: (f32, f32) = (3.0, 4.6);
const MAJOR: (f32, f32) = (1.2, 5.2);
const MINOR_W: f32 = 0.9;
const MAJOR_W: f32 = 1.2;
/// The sweep's radius from the crest's edge, and its width.
const SWEEP_AT: f32 = 6.6;
const SWEEP_W: f32 = 2.2;
/// The hand at the sweep's end: from just outside the crest to the ring's
/// edge, in lume.
const HAND: (f32, f32) = (0.5, 7.7);
const HAND_W: f32 = 1.3;
/// How the sweep is drawn: a polyline, a vertex per this many radians.
const STEP: f32 = PI / 90.0;

/// `crest` (`d` across) in its sub-dial, `sweep` (0..=1) of the turn swept
/// in `color`; `None` draws the bezel with no sweep.
pub fn dial(crest: AnyElement, d: Pixels, sweep: Option<f32>, color: Hsla, w: &W) -> AnyElement {
    let ring = w.z(RING);
    let side = d + ring * 2.;
    let (z, tick, lit, track) = (w.zoom, w.c(|t| t.dial), w.c(|t| t.ink), w.c(|t| t.edge));
    let r0 = f32::from(d) / 2.0;
    let face = canvas(
        |_, _, _| {},
        move |b, (), window, _| {
            let c = b.center();
            let at = |r: f32, a: f32| {
                // From twelve o'clock, clockwise.
                point(c.x + px(r * a.sin()), c.y - px(r * a.cos()))
            };
            let swept = sweep.unwrap_or(0.0).clamp(0.0, 1.0);
            // Four sets of ticks: minor and major, each dim or lit by the sweep.
            let mut sets: [(PathBuilder, f32, Hsla); 4] = [
                (PathBuilder::stroke(px(MINOR_W * z)), MINOR_W, tick),
                (PathBuilder::stroke(px(MINOR_W * z)), MINOR_W, lit),
                (PathBuilder::stroke(px(MAJOR_W * z)), MAJOR_W, tick),
                (PathBuilder::stroke(px(MAJOR_W * z)), MAJOR_W, lit),
            ];
            for i in 0..60 {
                let a = i as f32 / 60.0 * TAU;
                let major = i % 5 == 0;
                let under = sweep.is_some() && (i as f32) < swept * 60.0;
                let (from, to) = if major { MAJOR } else { MINOR };
                let set = usize::from(major) * 2 + usize::from(under);
                if let Some((path, _, _)) = sets.get_mut(set) {
                    path.move_to(at(r0 + from * z, a));
                    path.line_to(at(r0 + to * z, a));
                }
            }
            for (path, _, color) in sets {
                if let Ok(p) = path.build() {
                    window.paint_path(p, color);
                }
            }
            let r = r0 + SWEEP_AT * z;
            arc(window, &at, r, 0.0, TAU, SWEEP_W * 0.5 * z, track);
            if sweep.is_some() && swept > 0.0 {
                arc(window, &at, r, 0.0, swept * TAU, SWEEP_W * z, color);
                let end = swept * TAU;
                let mut hand = PathBuilder::stroke(px(HAND_W * z));
                hand.move_to(at(r0 + HAND.0 * z, end));
                hand.line_to(at(r0 + HAND.1 * z, end));
                if let Ok(p) = hand.build() {
                    window.paint_path(p, lit);
                }
            }
        },
    )
    .absolute()
    .size_full();
    div()
        .relative()
        .flex_none()
        .size(side)
        .flex()
        .items_center()
        .justify_center()
        .child(face)
        .child(crest)
        .into_any_element()
}

/// An arc of radius `r` from angle `a0` to `a1` (radians from twelve,
/// clockwise), stroked `width` wide.
fn arc(
    window: &mut Window,
    at: &dyn Fn(f32, f32) -> Point<Pixels>,
    r: f32,
    a0: f32,
    a1: f32,
    width: f32,
    color: Hsla,
) {
    let steps = (((a1 - a0) / STEP).ceil() as usize).max(1);
    let mut path = PathBuilder::stroke(px(width));
    path.move_to(at(r, a0));
    for i in 1..=steps {
        path.line_to(at(r, a0 + (a1 - a0) * i as f32 / steps as f32));
    }
    if let Ok(p) = path.build() {
        window.paint_path(p, color);
    }
}

/// The reticle's arm, in logical pixels: short enough that a corner set
/// beside a plot never reaches the words at its edge.
const BRACKET: f32 = 4.0;
/// How far outside a plot its corners stand: wide to the sides, where the
/// scale's words begin at the plot's edge, close above and below, where a
/// legend or the time axis sits.
pub const BRACKET_OUT: (f32, f32) = (6.0, 3.0);

/// Reticle brackets round the plot `b`: an L at each corner, `z` the zoom.
pub fn paint_brackets(window: &mut Window, b: Bounds<Pixels>, z: f32, color: Hsla) {
    let (ox, oy, arm) = (
        px(BRACKET_OUT.0 * z),
        px(BRACKET_OUT.1 * z),
        px(BRACKET * z),
    );
    let (x0, y0) = (b.origin.x - ox, b.origin.y - oy);
    let (x1, y1) = (
        b.origin.x + b.size.width + ox,
        b.origin.y + b.size.height + oy,
    );
    let mut path = PathBuilder::stroke(px(z));
    for (cx, cy, dx, dy) in [
        (x0, y0, arm, arm),
        (x1, y0, -arm, arm),
        (x0, y1, arm, -arm),
        (x1, y1, -arm, -arm),
    ] {
        path.move_to(point(cx + dx, cy));
        path.line_to(point(cx, cy));
        path.line_to(point(cx, cy + dy));
    }
    if let Ok(p) = path.build() {
        window.paint_path(p, color);
    }
}
