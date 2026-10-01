//! The overlay's scrollbar, as the iced overlay's lists wear it: a 10 px
//! rail down the lane every list leaves on its right, its thumb's length
//! the share of the list in view, square — and there only while the list
//! overflows. Kit's own bar is thin and hides itself; this is the one the
//! overlay has always shown. The wheel scrolls the list; the thumb drags.

use std::cell::Cell;
use std::rc::Rc;

use gpui_kit::prelude::*;
use gpui_kit::{
    Bounds, DispatchPhase, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, Pixels,
    ScrollHandle, canvas, fill, point, px, size,
};

use super::ov::Ov;

/// The rail's width, and its gap from the list's right edge.
pub const WIDTH: f32 = 10.0;

/// The thumb's top and length on a rail `rail` long, for a list whose
/// view is `view` long and which scrolls `max` beyond it, `scrolled` down.
pub fn thumb(rail: f32, view: f32, max: f32, scrolled: f32) -> Option<(f32, f32)> {
    if max <= 0.5 || view <= 0.0 || rail <= 0.0 {
        return None;
    }
    let len = (rail * view / (view + max)).max(rail.min(20.0));
    let top = (rail - len) * (scrolled / max).clamp(0.0, 1.0);
    Some((top, len))
}

/// The bar over `handle`'s list, drawn in the list's lane: place it in a
/// relative parent beside the scrolling element.
pub fn bar(ov: &Ov, handle: &ScrollHandle) -> impl IntoElement + use<> {
    let (rail_c, thumb_c) = (ov.c(|t| t.rail), ov.c(|t| t.thumb));
    let handle = handle.clone();
    // Where the thumb was grabbed: the pointer's y and the offset then.
    let grab: Rc<Cell<Option<(Pixels, Pixels)>>> = Rc::default();
    canvas(
        |_, _, _| {},
        move |bounds, (), window, _| {
            let view = handle.bounds().size.height;
            let max = handle.max_offset().y;
            let scrolled = -handle.offset().y;
            let rail = bounds.size.height;
            let Some((top, len)) = thumb(rail.into(), view.into(), max.into(), scrolled.into())
            else {
                return;
            };
            window.paint_quad(fill(bounds, rail_c));
            let thumb_bounds = Bounds::new(
                point(bounds.origin.x, bounds.origin.y + px(top)),
                size(bounds.size.width, px(len)),
            );
            window.paint_quad(fill(thumb_bounds, thumb_c));

            let (down, moving, up) = (grab.clone(), grab.clone(), grab.clone());
            window.on_mouse_event(move |e: &MouseDownEvent, phase, _, cx| {
                if phase == DispatchPhase::Bubble
                    && e.button == MouseButton::Left
                    && thumb_bounds.contains(&e.position)
                {
                    down.set(Some((e.position.y, scrolled)));
                    cx.stop_propagation();
                }
            });
            let drag = handle.clone();
            let travel = rail - px(len);
            window.on_mouse_event(move |e: &MouseMoveEvent, phase, window, _| {
                let Some((y0, from)) = moving.get() else {
                    return;
                };
                if phase != DispatchPhase::Bubble || travel <= px(0.) {
                    return;
                }
                let to = (from + (e.position.y - y0) * (max / travel)).clamp(px(0.), max);
                drag.set_offset(point(drag.offset().x, -to));
                window.refresh();
            });
            window.on_mouse_event(move |e: &MouseUpEvent, phase, _, _| {
                if phase == DispatchPhase::Bubble && e.button == MouseButton::Left {
                    up.set(None);
                }
            });
        },
    )
    .absolute()
    .top_0()
    .bottom_0()
    .right_0()
    .w(px(WIDTH))
}

#[cfg(test)]
mod tests {
    use super::thumb;

    #[test]
    fn the_thumb_is_the_share_in_view_and_none_when_all_fits() {
        assert_eq!(thumb(100., 100., 0., 0.), None, "nothing to scroll");
        assert_eq!(thumb(100., 100., 100., 0.), Some((0., 50.)));
        assert_eq!(
            thumb(100., 100., 100., 100.),
            Some((50., 50.)),
            "at the end"
        );
        assert_eq!(thumb(100., 100., 100., 50.), Some((25., 50.)));
        let (_, len) = thumb(100., 10., 10_000., 0.).unwrap();
        assert_eq!(len, 20., "never thinner than a grip");
    }
}
