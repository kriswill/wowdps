//! The bar's numbers, and its gestures through Kit's harness: GPUI's own
//! hit testing and pointer dispatch, a frame drawn after every event as a
//! live window draws them.

use std::time::Duration;

use gpui_kit::prelude::*;
use gpui_kit::test::TestWindowExt as _;
use gpui_kit::{
    AnyWindowHandle, App, Context, Entity, InputEvent as _, MouseButton, MouseDownEvent,
    MouseUpEvent, Pixels, Point, TestAppContext, TestSupportExt as _, Window, black, div, point,
    px, size,
};

use super::{Scroll, Style, bar, dragged, page, paged, thumb};
use crate::testkit;

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

#[test]
fn a_page_keeps_a_line_of_the_last() {
    assert_eq!(page(200.), 175., "7/8 of a short view");
    assert_eq!(page(800.), 760., "all but 40 px of a tall one");
}

#[test]
fn a_press_on_the_rail_pages_toward_itself_until_the_thumb_is_there() {
    // A 200 px rail over 1000 px of list: a 40 px thumb, 800 px to scroll.
    assert_eq!(paged(200., 200., 800., 0., 150., true), Some(175.));
    assert_eq!(paged(200., 200., 800., 400., 10., false), Some(225.));
    assert_eq!(
        paged(200., 200., 800., 700., 199., true),
        Some(800.),
        "held at the end"
    );
    assert_eq!(
        paged(200., 200., 800., 100., 0., false),
        Some(0.),
        "and at the top"
    );
    assert_eq!(paged(200., 200., 800., 0., 20., true), None, "on the thumb");
    assert_eq!(
        paged(200., 200., 800., 400., 150., false),
        None,
        "the thumb went past"
    );
    assert_eq!(
        paged(200., 200., 0., 0., 150., true),
        None,
        "nothing to scroll"
    );
}

#[test]
fn a_dragged_thumb_scrolls_its_share_of_the_travel() {
    assert_eq!(dragged(200., 200., 800., 0.), 0.);
    assert_eq!(
        dragged(200., 200., 800., 80.),
        400.,
        "half the 160 px travel"
    );
    assert_eq!(dragged(200., 200., 800., 500.), 800.);
    assert_eq!(dragged(200., 200., 800., -30.), 0.);
    assert_eq!(dragged(200., 200., 0., 80.), 0., "nothing to scroll");
}

const ROWS: usize = 50;
const ROW: f32 = 20.0;
const VIEW: f32 = 200.0;
const LANE: f32 = 10.0;

/// A 200 px list of 1000 px of rows with its bar, and, on `cover`, a
/// layer over both that takes the pointer (a palette's scrim).
#[derive(Default)]
struct Host {
    scroll: Scroll,
    cover: bool,
}

impl Render for Host {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let style = Style {
            rail: None,
            thumb: black(),
            width: px(LANE),
            radius: px(0.),
        };
        let list = div()
            .id("list")
            .size_full()
            .flex()
            .flex_col()
            .overflow_y_scroll()
            .track_scroll(&self.scroll)
            .children((0..ROWS).map(|_| div().h(px(ROW)).flex_shrink_0()));
        let frame = div()
            .id("frame")
            .test_support()
            .relative()
            .w(px(300.))
            .h(px(VIEW))
            .child(list)
            .child(bar(style, &self.scroll));
        div()
            .relative()
            .size_full()
            .child(frame)
            .when(self.cover, |d| {
                d.child(
                    div()
                        .id("cover")
                        .absolute()
                        .top_0()
                        .left_0()
                        .size_full()
                        .occlude(),
                )
            })
    }
}

struct Rig {
    window: AnyWindowHandle,
    host: Entity<Host>,
}

fn rig(cx: &mut TestAppContext) -> Rig {
    let (window, host) = testkit::open(cx, size(px(400.), px(300.)), |_, cx| {
        cx.new(|_| Host::default())
    });
    Rig { window, host }
}

impl Rig {
    fn act<R>(&self, cx: &mut TestAppContext, f: impl FnOnce(&mut Window, &mut App) -> R) -> R {
        cx.update_window(self.window, |_, window, cx| f(window, cx))
            .unwrap()
    }

    /// A point `y` down the bar's lane.
    fn lane(&self, cx: &mut TestAppContext, y: f32) -> Point<Pixels> {
        self.act(cx, |window, cx| {
            window.render_frame(cx);
            let b = window.find("frame").bounds();
            point(b.right() - px(LANE / 2.), b.top() + px(y))
        })
    }

    fn scrolled(&self, cx: &mut TestAppContext) -> f32 {
        self.host
            .read_with(cx, |h, _| -f32::from(h.scroll.offset().y))
    }

    fn scroll_to(&self, cx: &mut TestAppContext, to: f32) {
        self.host.update(cx, |h, _| super::scroll_to(&h.scroll, to));
        self.act(cx, |window, cx| window.render_frame(cx));
    }

    fn press(&self, cx: &mut TestAppContext, at: Point<Pixels>) {
        self.act(cx, |window, cx| {
            window.dispatch_event(
                MouseDownEvent {
                    button: MouseButton::Left,
                    position: at,
                    modifiers: Default::default(),
                    click_count: 1,
                    first_mouse: false,
                }
                .to_platform_input(),
                cx,
            );
            window.render_frame(cx);
        });
    }

    fn release(&self, cx: &mut TestAppContext, at: Point<Pixels>) {
        self.act(cx, |window, cx| {
            window.dispatch_event(
                MouseUpEvent {
                    button: MouseButton::Left,
                    position: at,
                    modifiers: Default::default(),
                    click_count: 1,
                }
                .to_platform_input(),
                cx,
            );
            window.render_frame(cx);
        });
    }

    fn wait(&self, cx: &mut TestAppContext, ms: u64) {
        cx.executor().advance_clock(Duration::from_millis(ms));
        cx.run_until_parked();
    }
}

/// The thumb follows the pointer the whole way, through a frame drawn
/// after every move — the bar is built anew each frame, and a grip kept in
/// it was forgotten after the first move — and holds at the rail's end.
#[gpui_kit::test]
fn a_dragged_thumb_follows_the_pointer_frame_after_frame(cx: &mut TestAppContext) {
    let rig = rig(cx);
    // The 40 px thumb, grabbed in its middle, dragged 80 px: half the
    // 160 px travel, half the 800 px scroll.
    let (a, b) = (rig.lane(cx, 20.), rig.lane(cx, 100.));
    rig.act(cx, |window, cx| window.drag(a, b, cx));
    assert_eq!(rig.scrolled(cx), 400.);

    // Grabbed where it now stands (80..120), past the rail's end.
    let (a, b) = (rig.lane(cx, 90.), rig.lane(cx, 290.));
    rig.act(cx, |window, cx| window.drag(a, b, cx));
    assert_eq!(rig.scrolled(cx), 800., "held at the end");

    // Let go, a drag elsewhere moves the bar no more.
    let (a, b) = (rig.lane(cx, 190.), rig.lane(cx, 10.));
    let (a, b) = (point(a.x - px(150.), a.y), point(b.x - px(150.), b.y));
    rig.act(cx, |window, cx| window.drag(a, b, cx));
    assert_eq!(rig.scrolled(cx), 800.);
}

/// A click on the rail pages toward it, once a click; a click above the
/// thumb pages back.
#[gpui_kit::test]
fn a_click_on_the_rail_pages(cx: &mut TestAppContext) {
    let rig = rig(cx);
    let below = rig.lane(cx, 150.);
    rig.press(cx, below);
    rig.release(cx, below);
    assert_eq!(rig.scrolled(cx), 175.);
    rig.press(cx, below);
    rig.release(cx, below);
    assert_eq!(rig.scrolled(cx), 350.);
    rig.wait(cx, 1_000);
    assert_eq!(rig.scrolled(cx), 350., "a click pages once");

    let above = rig.lane(cx, 5.);
    rig.press(cx, above);
    rig.release(cx, above);
    assert_eq!(rig.scrolled(cx), 175.);
}

/// A held press keeps paging after a pause, until the thumb is under the
/// pointer; letting go stops it short.
#[gpui_kit::test]
fn a_held_press_pages_until_the_thumb_reaches_it(cx: &mut TestAppContext) {
    let rig = rig(cx);
    let at = rig.lane(cx, 190.);
    rig.press(cx, at);
    assert_eq!(rig.scrolled(cx), 175.);
    rig.wait(cx, 200);
    assert_eq!(rig.scrolled(cx), 175., "not before the pause");
    rig.wait(cx, 50);
    assert_eq!(rig.scrolled(cx), 350.);
    rig.wait(cx, 50);
    assert_eq!(rig.scrolled(cx), 525.);
    rig.wait(cx, 100);
    assert_eq!(rig.scrolled(cx), 800., "the thumb reached the pointer");
    rig.wait(cx, 500);
    assert_eq!(rig.scrolled(cx), 800.);
    rig.release(cx, at);

    rig.scroll_to(cx, 0.);
    rig.press(cx, at);
    rig.wait(cx, 250);
    assert_eq!(rig.scrolled(cx), 350.);
    rig.release(cx, at);
    rig.wait(cx, 500);
    assert_eq!(rig.scrolled(cx), 350., "let go, it stops");
}

/// Under a layer that takes the pointer, the bar takes no press.
#[gpui_kit::test]
fn a_covered_bar_takes_no_press(cx: &mut TestAppContext) {
    let rig = rig(cx);
    rig.host.update(cx, |h, cx| {
        h.cover = true;
        cx.notify();
    });
    let below = rig.lane(cx, 150.);
    rig.press(cx, below);
    rig.release(cx, below);
    assert_eq!(rig.scrolled(cx), 0.);
    let (a, b) = (rig.lane(cx, 20.), rig.lane(cx, 100.));
    rig.act(cx, |window, cx| window.drag(a, b, cx));
    assert_eq!(rig.scrolled(cx), 0.);
}
