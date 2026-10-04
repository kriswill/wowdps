//! A death on the graph (R23): two dashed rules, the moment they died in
//! the bad ink and the moment they were alive again in the good — raised,
//! or seen alive — each with its words on a patch of the panel. The stretch
//! between is left clear: the prototype hatched it, one fill per death, and
//! a player who died over and over wore a wall of stacked stripes.

use gpui_kit::{PathBuilder, Window, fill, px};
use wowdps_gui_logic::inspect::geometry::{
    self as geo, DEATH_DASH, DEATH_PATCH_ALPHA, DEATH_PATCH_PAD, DEATH_PATCH_RADIUS, DEATH_RULE,
    DEATH_WORDS_Y, Ink3, LINE, Label,
};

use super::Pen;
use crate::theme::hsla;

/// Every death's rules inside the window.
pub fn rules(g: &geo::Plot<'_>, w: f32, pen: Pen, window: &mut Window) {
    for (x, _, ink, _) in g.rules(w) {
        let mut rule =
            PathBuilder::stroke(pen.px(DEATH_RULE)).dash_array(&DEATH_DASH.map(|v| px(v * pen.z)));
        rule.move_to(pen.at(x, 0.0));
        rule.line_to(pen.at(x, g.plot_h));
        if let Ok(path) = rule.build() {
            let color = if ink == Ink3::Good {
                pen.t.good
            } else {
                pen.t.bad
            };
            window.paint_path(path, hsla(color));
        }
    }
}

/// The patch of the panel each rule's words sit on, over the rules and the
/// curves.
pub fn patches(words: &[Label], pen: Pen, window: &mut Window) {
    for l in words {
        window.paint_quad(
            fill(
                pen.rect(
                    l.x - DEATH_PATCH_PAD,
                    l.y - DEATH_WORDS_Y,
                    l.width + 2.0 * DEATH_PATCH_PAD,
                    l.px * LINE + 2.0 * DEATH_WORDS_Y,
                ),
                hsla(pen.t.surface.alpha(DEATH_PATCH_ALPHA)),
            )
            .corner_radii(pen.r(DEATH_PATCH_RADIUS)),
        );
    }
}
