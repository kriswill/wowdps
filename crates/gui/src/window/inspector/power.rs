//! R28 (v44): a healer's mana under the inspector's graph — a thin line of
//! the pool as a fraction of its max, broken where the log reported none,
//! on the plot's own clock: the strip starts where the plot starts (past
//! the lanes' gutter when there are lanes, whose labels it shares the
//! column of) and shows the plot's window. Where its points stand is
//! gui-logic's `inspect::power`; this paints them in the theme's `power`
//! data token. Nothing moves: no animation, so reduced motion changes
//! nothing here.

use gpui_kit::prelude::*;
use gpui_kit::{AnyElement, PathBuilder, TestSupportExt as _, canvas, div, point, px};
use wowdps_gui_logic::inspect::geometry::{HAIRLINE, LABEL_W, LEFT};
use wowdps_gui_logic::inspect::power::{self as pw, PowerLine};

use crate::theme::hsla;
use crate::window::w::W;

/// The strip under a plot showing `shown` (ms from the fight's start), with
/// or without `lanes` (the gutter they put the plot past).
pub fn strip(line: &PowerLine, shown: (u32, u32), lanes: bool, w: &W) -> AnyElement {
    let left = if lanes { LEFT } else { 0.0 };
    let color = w.data.power;
    let track = w.t.line;
    let zoom = w.zoom;
    let points = line.clone();
    // The gutter holds the pool's name alone; with the strip's width to
    // itself, its low point too.
    let words = w.text(
        if lanes {
            line.label.clone()
        } else {
            line.words()
        },
        pw::WORDS_PX,
        w.c(|t| t.ink_3_text),
        gpui_kit::FontWeight::NORMAL,
    );
    let paint = canvas(
        |_, _, _| (),
        move |b, (), window, _| {
            let width = f32::from(b.size.width) / zoom;
            let at = |x: f32, y: f32| point(b.origin.x + px(x * zoom), b.origin.y + px(y * zoom));
            // The track: a hairline at the empty pool's height, under the
            // plot's width alone.
            window.paint_quad(gpui_kit::fill(
                gpui_kit::Bounds::new(
                    at(left, pw::STRIP_H - HAIRLINE),
                    gpui_kit::size(px((width - left).max(0.0) * zoom), px(HAIRLINE * zoom)),
                ),
                hsla(track),
            ));
            for run in pw::runs(&points, shown, left, width, pw::STRIP_H) {
                let Some((first, rest)) = run.split_first() else {
                    continue;
                };
                let mut path = PathBuilder::stroke(px(pw::LINE_W * zoom));
                path.move_to(at(first.0, first.1));
                for p in rest {
                    path.line_to(at(p.0, p.1));
                }
                if let Ok(path) = path.build() {
                    window.paint_path(path, hsla(color));
                }
            }
        },
    )
    .size_full();
    // The words stand in the lanes' gutter, right-aligned like a lane's
    // label; with no gutter, over the strip's start.
    let label = if lanes {
        div()
            .absolute()
            .left_0()
            .top_0()
            .w(w.z(LABEL_W))
            .h_full()
            .flex()
            .items_center()
            .justify_end()
            .overflow_hidden()
            .child(words)
    } else {
        div()
            .absolute()
            .left_0()
            .top_0()
            .h_full()
            .flex()
            .items_center()
            .child(words)
    };
    div()
        .id("inspector-power")
        .test_support()
        .relative()
        .flex_none()
        .w_full()
        .mt(w.z(pw::STRIP_GAP))
        .h(w.z(pw::STRIP_H))
        .child(paint)
        .child(label)
        .into_any_element()
}
