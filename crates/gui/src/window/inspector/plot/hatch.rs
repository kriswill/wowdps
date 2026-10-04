//! A death on the graph (R23): hatched from the moment to the rez (or the
//! fight's end) in the prototype's 45° stripes of the bad ink — 4 px of
//! ink and 4 px of air measured across them — its edge a dashed red rule,
//! and its words on a patch of the panel over both.

use gpui_kit::{PathBuilder, Window, fill, px};
use wowdps_gui_logic::inspect::geometry::{
    self as geo, HATCH_ALPHA, HATCH_DASH, HATCH_EDGE, HATCH_PATCH_ALPHA, HATCH_PATCH_PAD,
    HATCH_PATCH_RADIUS, HATCH_WORDS_Y, LINE, Label, STRIPE_W,
};

use super::Pen;
use crate::theme::hsla;

/// Every death's stripes and its dashed edge.
pub fn stripes(g: &geo::Plot<'_>, w: f32, pen: Pen, window: &mut Window) {
    let left = g.left();
    for d in g.dead {
        let x1 = g.x_of(d.at_ms as f64, w).clamp(left, w);
        let x2 = g.x_of(d.end_ms as f64, w).clamp(left, w);
        if x2 <= x1 {
            continue;
        }
        let mut p = PathBuilder::stroke(pen.px(STRIPE_W));
        for (a, b) in geo::stripes(x1, x2, g.plot_h) {
            p.move_to(pen.at(a.0, a.1));
            p.line_to(pen.at(b.0, b.1));
        }
        if let Ok(path) = p.build() {
            let hatch = pen.t.death_hatch;
            window.paint_path(path, hsla(hatch.alpha(hatch.a * HATCH_ALPHA)));
        }
        let mut edge =
            PathBuilder::stroke(pen.px(HATCH_EDGE)).dash_array(&HATCH_DASH.map(|v| px(v * pen.z)));
        edge.move_to(pen.at(x1, 0.0));
        edge.line_to(pen.at(x1, g.plot_h));
        if let Ok(path) = edge.build() {
            window.paint_path(path, hsla(pen.t.bad));
        }
    }
}

/// The patch of the panel each death's words sit on, over its stripes and
/// its edge.
pub fn patches(hatches: &[Label], pen: Pen, window: &mut Window) {
    for l in hatches {
        window.paint_quad(
            fill(
                pen.rect(
                    l.x - HATCH_PATCH_PAD,
                    l.y - HATCH_WORDS_Y,
                    l.width + 2.0 * HATCH_PATCH_PAD,
                    l.px * LINE + 2.0 * HATCH_WORDS_Y,
                ),
                hsla(pen.t.surface.alpha(HATCH_PATCH_ALPHA)),
            )
            .corner_radii(pen.r(HATCH_PATCH_RADIUS)),
        );
    }
}
