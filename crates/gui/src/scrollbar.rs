//! A list's scrollbar, as the iced lists wear theirs: a rail down the lane
//! every list leaves on its right, its thumb's length the share of the list
//! in view — and there only while the list overflows. Kit's own bar is thin
//! and hides itself; this is the one the iced surfaces have always shown.
//! The wheel scrolls the list; the thumb drags; a press on the rail pages
//! toward it, and held, keeps paging until the thumb reaches it. The
//! overlay's is a square thumb on a visible rail; the window's a rounded
//! one on a clear lane.

use std::cell::{Cell, RefCell};
use std::ops::Deref;
use std::rc::Rc;
use std::time::Duration;

use gpui_kit::prelude::*;
use gpui_kit::{
    Bounds, DispatchPhase, HitboxBehavior, Hsla, MouseButton, MouseDownEvent, MouseMoveEvent,
    MouseUpEvent, Pixels, ScrollHandle, Task, canvas, fill, point, px, size,
};

/// The overlay's rail width, and its gap from the list's right edge.
pub const WIDTH: f32 = 10.0;

/// A held press on the rail pages again after this, then every `REPEAT`
/// (Chromium's autoscroll timings).
const FIRST_REPEAT: Duration = Duration::from_millis(250);
const REPEAT: Duration = Duration::from_millis(50);

/// How a surface draws its bar.
#[derive(Debug, Clone, Copy)]
pub struct Style {
    /// The rail under the thumb; none leaves the lane clear.
    pub rail: Option<Hsla>,
    pub thumb: Hsla,
    /// The lane's width.
    pub width: Pixels,
    /// The thumb's corners.
    pub radius: Pixels,
}

/// A list's scroll handle with its bar's grip. The bar is built anew every
/// frame, so what a press starts — a drag of the thumb, a page held on the
/// rail — lives here beside the handle; kept in the bar, the next frame
/// forgot it and a drag stopped after its first move. Derefs to the handle,
/// so a list tracks it as it would the handle itself.
#[derive(Clone, Default)]
pub struct Scroll {
    handle: ScrollHandle,
    grip: Rc<Cell<Grip>>,
    /// A held page's repeat; dropping it stops the paging.
    repeat: Rc<RefCell<Option<Task<()>>>>,
}

/// What the press on the bar holds.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
enum Grip {
    #[default]
    Free,
    /// The thumb, held this far below its top.
    Thumb(f32),
    /// The rail, pressed this far down it, paging down or up toward it.
    Page { at: f32, down: bool },
}

impl Scroll {
    pub fn new() -> Self {
        Self::default()
    }

    /// Let go: the drag ends, a held page stops.
    fn release(&self) {
        self.grip.set(Grip::Free);
        self.repeat.borrow_mut().take();
    }
}

impl Deref for Scroll {
    type Target = ScrollHandle;

    fn deref(&self) -> &ScrollHandle {
        &self.handle
    }
}

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

/// One page of a view `view` long, as a browser pages: the view less a
/// line kept for context — 7/8 of it, or all but 40 px when that is more.
pub fn page(view: f32) -> f32 {
    (view * 0.875).max(view - 40.0).max(1.0)
}

/// The scroll a press `at` down the rail pages to: a page `down` or up
/// while the thumb has not reached the press, else none.
pub fn paged(rail: f32, view: f32, max: f32, scrolled: f32, at: f32, down: bool) -> Option<f32> {
    let (top, len) = thumb(rail, view, max, scrolled)?;
    let to = match down {
        true if at >= top + len => scrolled + page(view),
        false if at < top => scrolled - page(view),
        _ => return None,
    };
    Some(to.clamp(0.0, max))
}

/// The scroll that brings the thumb's top to `top` down the rail.
pub fn dragged(rail: f32, view: f32, max: f32, top: f32) -> f32 {
    let Some((_, len)) = thumb(rail, view, max, 0.0) else {
        return 0.0;
    };
    let travel = rail - len;
    if travel <= 0.0 {
        return 0.0;
    }
    max * (top / travel).clamp(0.0, 1.0)
}

/// The handle's view, scroll range and scroll, in plain numbers.
fn measures(handle: &ScrollHandle) -> (f32, f32, f32) {
    (
        handle.bounds().size.height.into(),
        handle.max_offset().y.into(),
        (-handle.offset().y).into(),
    )
}

fn scroll_to(handle: &ScrollHandle, to: f32) {
    handle.set_offset(point(handle.offset().x, px(-to)));
}

/// One step of a held page on a rail `rail` long; false once there is
/// nothing left to page.
fn page_step(handle: &ScrollHandle, grip: &Cell<Grip>, rail: f32) -> bool {
    let Grip::Page { at, down } = grip.get() else {
        return false;
    };
    let (view, max, scrolled) = measures(handle);
    match paged(rail, view, max, scrolled, at, down) {
        Some(to) => {
            scroll_to(handle, to);
            true
        }
        None => false,
    }
}

/// The bar over `scroll`'s list, drawn in the list's lane: place it in a
/// relative parent beside the scrolling element.
pub fn bar(style: Style, scroll: &Scroll) -> impl IntoElement + use<> {
    let Style {
        rail: rail_c,
        thumb: thumb_c,
        width,
        radius,
    } = style;
    let scroll = scroll.clone();
    canvas(
        // A hitbox, so a press reaches the bar only where nothing (a
        // palette's scrim, a menu) stands over it.
        |bounds, window, _| window.insert_hitbox(bounds, HitboxBehavior::Normal),
        move |bounds, hitbox, window, _| {
            // A release ends whatever the bar holds, overflowing or not, so
            // a list that stopped overflowing mid-drag leaves no grip behind.
            let up = scroll.clone();
            window.on_mouse_event(move |e: &MouseUpEvent, phase, _, cx| {
                if phase == DispatchPhase::Bubble
                    && e.button == MouseButton::Left
                    && up.grip.get() != Grip::Free
                {
                    up.release();
                    cx.stop_propagation();
                }
            });

            let (view, max, scrolled) = measures(&scroll);
            let rail_top = bounds.origin.y;
            let rail: f32 = bounds.size.height.into();
            let Some((top, len)) = thumb(rail, view, max, scrolled) else {
                return;
            };
            if let Some(rail_c) = rail_c {
                window.paint_quad(fill(bounds, rail_c));
            }
            let thumb_bounds = Bounds::new(
                point(bounds.origin.x, rail_top + px(top)),
                size(bounds.size.width, px(len)),
            );
            window.paint_quad(fill(thumb_bounds, thumb_c).corner_radii(radius));

            let down = scroll.clone();
            window.on_mouse_event(move |e: &MouseDownEvent, phase, window, cx| {
                if phase != DispatchPhase::Bubble
                    || e.button != MouseButton::Left
                    || !hitbox.is_hovered(window)
                {
                    return;
                }
                cx.stop_propagation();
                let at = f32::from(e.position.y - rail_top);
                if (top..top + len).contains(&at) {
                    down.grip.set(Grip::Thumb(at - top));
                    return;
                }
                down.grip.set(Grip::Page {
                    at,
                    down: at >= top + len,
                });
                page_step(&down, &down.grip, rail);
                window.refresh();
                let (handle, grip) = (down.handle.clone(), down.grip.clone());
                let repeat = window.spawn(cx, async move |cx| {
                    let mut wait = FIRST_REPEAT;
                    loop {
                        cx.background_executor().timer(wait).await;
                        wait = REPEAT;
                        let paged = cx.update(|window, _| {
                            let paged = page_step(&handle, &grip, rail);
                            if paged {
                                window.refresh();
                            }
                            paged
                        });
                        if !matches!(paged, Ok(true)) {
                            break;
                        }
                    }
                });
                *down.repeat.borrow_mut() = Some(repeat);
            });

            let moving = scroll.clone();
            window.on_mouse_event(move |e: &MouseMoveEvent, phase, window, cx| {
                let Grip::Thumb(held) = moving.grip.get() else {
                    return;
                };
                if phase != DispatchPhase::Bubble {
                    return;
                }
                if !e.dragging() {
                    // The release went somewhere we never heard it.
                    moving.release();
                    return;
                }
                cx.stop_propagation();
                let (view, max, _) = measures(&moving);
                let top = f32::from(e.position.y - rail_top) - held;
                scroll_to(&moving, dragged(rail, view, max, top));
                window.refresh();
            });
        },
    )
    .absolute()
    .top_0()
    .bottom_0()
    .right_0()
    .w(width)
}

#[cfg(test)]
mod tests;
