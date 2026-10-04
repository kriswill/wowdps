//! The lanes under the curve (`.lanes`): a faint track per lane, and on it
//! each span in its caster's class colour, edged in the panel's surface so
//! two that overlap in one lane (Power Infusion inside a Heroism) read as
//! two — the hovered one lit and outlined (`.span:hover`). A comparison's
//! track holds two halves, the first player's spans over the second's;
//! where each stands is the geometry's (`span_rect`).

use gpui_kit::{BorderStyle, Window, fill, quad};
use wowdps_gui_logic::inspect::geometry::{
    self as geo, LANE_H, SPAN_ALPHA, SPAN_EDGE, SPAN_RADIUS, TICK_RADIUS, TRACK_RADIUS,
};

use super::Pen;
use crate::theme::hsla;

/// Every lane's track and spans; `lit` is the span under the pointer.
pub fn paint(
    g: &geo::Plot<'_>,
    w: f32,
    lit: Option<(usize, usize)>,
    pen: Pen,
    window: &mut Window,
) {
    let left = g.left();
    for (li, row) in g.lanes.iter().enumerate() {
        window.paint_quad(
            fill(
                pen.rect(left, g.track_y(li), (w - left).max(0.0), LANE_H),
                hsla(pen.t.lane_track),
            )
            .corner_radii(pen.r(TRACK_RADIUS)),
        );
        for (si, s) in row.spans.iter().enumerate() {
            let Some(r) = g.span_rect(li, s, w) else {
                continue;
            };
            let on = lit == Some((li, si));
            let radius = if s.dur_ms <= 0 {
                TICK_RADIUS
            } else {
                SPAN_RADIUS
            };
            // iced strokes the edge centred on the span's outline: the box
            // grows half the edge each way and the corner with it.
            let half = SPAN_EDGE / 2.0;
            window.paint_quad(quad(
                pen.rect(r.x - half, r.y - half, r.w + SPAN_EDGE, r.h + SPAN_EDGE),
                pen.r(radius + half),
                hsla(s.color.alpha(if on { 1.0 } else { SPAN_ALPHA })),
                pen.px(SPAN_EDGE),
                hsla(if on { pen.t.span_lit } else { pen.t.surface }),
                BorderStyle::Solid,
            ));
        }
    }
}
