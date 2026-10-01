//! The graph's gestures through Kit's harness: GPUI's own hit testing and
//! pointer dispatch, over the shared samples (`geometry::samples`).

use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

use gpui_kit::test::TestWindowExt as _;
use gpui_kit::{
    AnyWindowHandle, App, AppContext as _, Context, Entity, InputEvent as _, IntoElement,
    MouseMoveEvent, ParentElement as _, Pixels, Point, Render, Styled as _, TestAppContext, Window,
    div, point, px, size,
};
use wowdps_gui_logic::inspect::geometry::{Hover, samples};
use wowdps_gui_logic::theme::GOLD;

use super::{Input, State, plot};
use crate::testkit;
use crate::window::w::W;

type Ranges = Rc<RefCell<Vec<Option<(u32, u32)>>>>;

/// A window holding one graph `samples::WIDTH` wide: zoomable into
/// `ranges` or not, its state its own or `state`.
struct Host {
    input: Input,
    zoomable: bool,
    ranges: Ranges,
    state: Option<Entity<State>>,
}

impl Render for Host {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let w = W::new(1.0, 1440.0, cx);
        let mut p = plot("plot", self.input.clone(), &w);
        if self.zoomable {
            let ranges = self.ranges.clone();
            p = p.on_range(Rc::new(move |r, _, _| ranges.borrow_mut().push(r)));
        }
        if let Some(s) = &self.state {
            p = p.state(s.clone());
        }
        div().w(px(samples::WIDTH)).child(p)
    }
}

fn input(name: &str) -> Input {
    let s = samples::all()
        .into_iter()
        .find(|s| s.name == name)
        .expect("a sample of that name");
    Input {
        window: s.window,
        peak: s.peak,
        curves: s.curves,
        dead: s.dead,
        lanes: s.lanes,
        total: s.total,
        word: s.word,
    }
}

struct Rig {
    window: AnyWindowHandle,
    host: Entity<Host>,
    ranges: Ranges,
    state: Entity<State>,
}

/// The sample `name` in a window, zoomable or not, its state observable.
fn rig(cx: &mut TestAppContext, name: &str, zoomable: bool) -> Rig {
    let ranges: Ranges = Rc::default();
    let state = cx.new(|_| State::default());
    let (r, s) = (ranges.clone(), state.clone());
    let input = input(name);
    let (window, host) = testkit::open(cx, size(px(560.), px(320.)), move |_, cx| {
        crate::theme::apply(&GOLD, None, cx);
        cx.new(|cx| {
            cx.observe(&s, |_, _, cx| cx.notify()).detach();
            Host {
                input,
                zoomable,
                ranges: r,
                state: Some(s),
            }
        })
    });
    Rig {
        window,
        host,
        ranges,
        state,
    }
}

impl Rig {
    /// A point on the graph, in its own canvas units.
    fn at(&self, cx: &mut TestAppContext, x: f32, y: f32) -> Point<Pixels> {
        cx.update_window(self.window, |_, window, cx| {
            window.render_frame(cx);
            let o = window.find("plot").bounds().origin;
            point(o.x + px(x), o.y + px(y))
        })
        .unwrap()
    }

    fn act(&self, cx: &mut TestAppContext, f: impl FnOnce(&mut Window, &mut App)) {
        cx.update_window(self.window, |_, window, cx| f(window, cx))
            .unwrap();
    }

    fn move_to(&self, cx: &mut TestAppContext, p: Point<Pixels>) {
        self.act(cx, |window, cx| {
            window.dispatch_event(
                MouseMoveEvent {
                    position: p,
                    pressed_button: None,
                    modifiers: Default::default(),
                }
                .to_platform_input(),
                cx,
            );
            window.render_frame(cx);
        });
    }

    fn ranges(&self) -> Vec<Option<(u32, u32)>> {
        self.ranges.borrow().clone()
    }
}

/// A drag over the plot publishes its window; a click, or a wander under
/// 3 px, publishes nothing; a drag past the canvas holds at its edge; a
/// right press asks for the whole fight back.
#[gpui_kit::test]
fn a_drag_zooms_a_click_does_not_and_a_right_press_resets(cx: &mut TestAppContext) {
    let rig = rig(cx, "alone", true);
    let g_in = input("alone");
    let g = g_in.geo(g_in.window);
    let w = samples::WIDTH;

    rig.act(cx, |window, cx| {
        window.click_at("plot", point(px(100.), px(40.)), cx)
    });
    let (a, b) = (rig.at(cx, 100., 40.), rig.at(cx, 102., 40.));
    rig.act(cx, |window, cx| window.drag(a, b, cx));
    assert!(rig.ranges().is_empty(), "a click is not a zoom");
    rig.state.read_with(cx, |s, _| assert_eq!(s.drag(), None));

    let (a, b) = (rig.at(cx, 100., 40.), rig.at(cx, 300., 40.));
    rig.act(cx, |window, cx| window.drag(a, b, cx));
    assert_eq!(rig.ranges(), vec![g.drag_range(100., 300., w)]);
    assert!(rig.ranges()[0].is_some_and(|(lo, hi)| lo < hi));

    let (a, b) = (rig.at(cx, 300., 40.), rig.at(cx, 900., 40.));
    rig.act(cx, |window, cx| window.drag(a, b, cx));
    assert_eq!(
        rig.ranges().last().copied().flatten(),
        Some((g.ms_at(300., w), g_in.window.1)),
        "held at the canvas's edge"
    );

    rig.act(cx, |window, cx| window.right_click("plot", cx));
    assert_eq!(rig.ranges().last(), Some(&None), "the whole fight back");
    rig.state.read_with(cx, |s, _| assert_eq!(s.drag(), None));

    // A press under the plot, on its axis, starts nothing.
    let (a, b) = (rig.at(cx, 100., 105.), rig.at(cx, 300., 105.));
    let before = rig.ranges().len();
    rig.act(cx, |window, cx| window.drag(a, b, cx));
    assert_eq!(rig.ranges().len(), before);
}

/// Without an owner to tell, the graph cannot be zoomed: a drag leaves
/// nothing in flight and a right press is nobody's.
#[gpui_kit::test]
fn an_unzoomable_graph_takes_no_drag(cx: &mut TestAppContext) {
    let rig = rig(cx, "alone", false);
    let (a, b) = (rig.at(cx, 100., 40.), rig.at(cx, 300., 40.));
    rig.act(cx, |window, cx| {
        window.dispatch_event(
            gpui_kit::MouseDownEvent {
                button: gpui_kit::MouseButton::Left,
                position: a,
                modifiers: Default::default(),
                click_count: 1,
                first_mouse: false,
            }
            .to_platform_input(),
            cx,
        );
        window.render_frame(cx);
    });
    rig.move_to(cx, b);
    rig.state.read_with(cx, |s, _| {
        assert_eq!(s.drag(), None);
        assert!(matches!(s.hover(), Some(Hover::Plot(_))), "it still hovers");
    });
    rig.act(cx, |window, cx| window.right_click("plot", cx));
    assert!(rig.ranges().is_empty());
}

/// The pointer on the plot reads the instant and the curve under a
/// crosshair; on a span, the span's name, time, length and caster; off
/// the canvas, nothing.
#[gpui_kit::test]
fn the_pointer_reads_the_plot_and_the_spans(cx: &mut TestAppContext) {
    let rig = rig(cx, "lanes", true);
    let g_in = input("lanes");
    let g = g_in.geo(g_in.window);
    let w = samples::WIDTH;

    let p = rig.at(cx, 300., 50.);
    rig.move_to(cx, p);
    rig.state.read_with(cx, |s, _| {
        let want = g.hover_at(300., 50., w);
        assert!(matches!(want, Some(Hover::Plot(_))));
        assert_eq!(s.hover(), want);
        let tip = s.tip().expect("a tooltip");
        assert_eq!(tip.lines, g.tip_words(want.unwrap(), w));
        assert!(tip.lines[1].0.starts_with("dps  "), "{:?}", tip.lines);
    });

    let pointer = samples::all()
        .into_iter()
        .find(|s| s.name == "hover-span")
        .and_then(|s| s.pointer)
        .expect("the span sample's pointer");
    let p = rig.at(cx, pointer.0, pointer.1);
    rig.move_to(cx, p);
    rig.state.read_with(cx, |s, _| {
        let Some(Hover::Span(lane, i)) = s.hover() else {
            panic!("on a span: {:?}", s.hover());
        };
        assert_eq!(g_in.lanes[lane].spans[i].label, "Power Infusion");
        let tip = s.tip().expect("a span's tooltip");
        assert_eq!(tip.lines[0].0, "Power Infusion");
        assert!(tip.rect.y + tip.rect.h <= g.track_y(lane), "above its lane");
    });

    let p = rig.at(cx, 300., g_in.height() + 40.);
    rig.move_to(cx, p);
    rig.state.read_with(cx, |s, _| {
        assert_eq!(s.hover(), None);
        assert!(s.tip().is_none());
    });
}

/// The delight: a new window glides there — the first frame still at the
/// old one, mid-way part of the way, settled exactly on it — and under
/// reduced motion it is there at once.
#[gpui_kit::test]
fn a_zoom_glides_to_its_window_and_reduced_motion_jumps(cx: &mut TestAppContext) {
    let rig = rig(cx, "lanes", true);
    let whole = input("lanes").window;
    let zoom = (60_000, 150_000);
    let shown = |cx: &mut TestAppContext| {
        rig.act(cx, |window, cx| window.render_frame(cx));
        rig.state.read_with(cx, |s, _| s.shown())
    };
    assert_eq!(
        shown(cx),
        Some(whole),
        "the first window is adopted at once"
    );

    rig.host.update(cx, |h, cx| {
        h.input.window = zoom;
        cx.notify();
    });
    let first = shown(cx).expect("drawn");
    assert_ne!(first, zoom, "a glide starts where the last window stood");
    cx.executor().advance_clock(Duration::from_millis(110));
    let mid = shown(cx).expect("drawn");
    assert!(
        mid.0 > whole.0 && mid.0 < zoom.0 && mid.1 < whole.1 && mid.1 > zoom.1,
        "{mid:?} lies between"
    );
    cx.executor().advance_clock(Duration::from_millis(200));
    assert_eq!(shown(cx), Some(zoom), "settled exactly");

    cx.update(|cx| cx.set_reduce_motion(true));
    rig.host.update(cx, |h, cx| {
        h.input.window = whole;
        cx.notify();
    });
    assert_eq!(shown(cx), Some(whole), "no glide under reduced motion");
}

/// The graph is as tall as its lanes make it, at the window's zoom.
#[gpui_kit::test]
fn the_graph_is_as_tall_as_its_lanes(cx: &mut TestAppContext) {
    let rig = rig(cx, "lanes", true);
    let want = input("lanes").height();
    let h = cx
        .update_window(rig.window, |_, window, cx| {
            window.render_frame(cx);
            f32::from(window.find("plot").bounds().size.height)
        })
        .unwrap();
    // GPUI's layout rounds to the device pixel; iced keeps the fraction.
    assert!((h - want).abs() <= 0.5, "{h} vs {want}");
    assert!(input("alone").height() < want, "no lanes, no rows");
}
