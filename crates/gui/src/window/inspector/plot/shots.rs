//! The graph's shots: every state of gui-logic's shared samples
//! (`geometry::samples`) rendered headless on the inspector's surface, 16 px
//! around — the same inputs, sizes and pointer the iced window's
//! `inspector::plot::tests::plot_shots` renders, so the two sets can be laid
//! side by side or diffed, pixel for pixel. Reduced motion, so a shot is
//! the parity picture.
//!
//! With `WOWDPS_SHOTS_DELIGHT` set, two more with motion on, for review of
//! what reduced motion leaves out: the crosshair's glow and its dots
//! (`plot-hover-plot-delight`), and a drag in flight with its gold edges
//! and its window's words (`plot-drag-delight`).
//!
//! `WOWDPS_SHOTS_DIR=/tmp/s cargo test -p wowdps-gui inspector_plot_shots -- --ignored --nocapture`

use std::path::{Path, PathBuf};
use std::rc::Rc;

use gpui_kit::test::TestWindowExt as _;
use gpui_kit::{
    AppContext as _, Context, HeadlessAppContext, InputEvent as _, IntoElement, MouseButton,
    MouseDownEvent, MouseMoveEvent, ParentElement as _, Render, Styled as _, Window, div, point,
    px, size,
};
use wowdps_gui_logic::inspect::geometry::samples::{self, Sample};

use super::{Input, plot};
use crate::testkit;
use crate::window::w::W;

/// The surface around the graph, as the iced shots pad it.
const PAD: f32 = 16.0;

struct Shot {
    input: Input,
}

impl Render for Shot {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let w = W::new(1.0, 1440.0, cx);
        div()
            .size_full()
            .bg(w.c(|t| t.surface))
            .p(px(PAD))
            .child(plot("plot", self.input.clone(), &w).on_range(Rc::new(|_, _, _| {})))
    }
}

/// The inspector's other two pieces on the same surface: R21's matrices
/// over the ledger sample, R9's chips over three deaths (the last shown,
/// one more dropped) — iced's `taken::tests::taken_shots` draws the same.
enum Piece {
    Matrix,
    Chips,
}

impl Render for Piece {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let w = W::new(1.0, 1440.0, cx);
        let body = match self {
            Piece::Matrix => {
                let (d, c, b) = wowdps_gui_logic::inspect::matrix::samples::ledger();
                let m = wowdps_gui_logic::inspect::matrix::matrices(&d, &c, &b);
                super::super::matrix::matrix("stacks", &m, 3, &w, cx)
            }
            Piece::Chips => {
                let deaths: Vec<wowdps_proto::DeathWindow> = [64_000, 158_000, 231_000]
                    .into_iter()
                    .enumerate()
                    .map(|(i, at_ms)| wowdps_proto::DeathWindow {
                        index: i as u32,
                        at_ms,
                    })
                    .collect();
                super::super::chips::chips(&deaths, Some(2), 1, &w, Rc::new(|_, _, _| {}))
            }
        };
        div()
            .size_full()
            .bg(w.c(|t| t.surface))
            .p(px(PAD))
            .children(body)
    }
}

fn take_piece(piece: Piece, height: f32, path: Option<&Path>) {
    let mut cx: HeadlessAppContext = crate::window::shots::app();
    let frame = size(px(samples::WIDTH + 2.0 * PAD), px(height));
    let (window, _) = testkit::open_headless(&mut cx, frame, |_, cx| cx.new(|_| piece));
    cx.run_until_parked();
    let _ = cx.update_window(window, |_, window, cx| window.render_frame(cx));
    let image = cx.capture_screenshot(window).expect("a headless renderer");
    if let Some(path) = path {
        image.save(path).expect("png written");
        eprintln!("inspector_plot_shots: {}", path.display());
    }
}

/// What the pointer does before the picture: rests, or presses and drags.
enum Pointer {
    Rest(f32, f32),
    Drag((f32, f32), f32),
}

fn input(s: Sample) -> Input {
    Input {
        window: s.window,
        peak: s.peak,
        curves: s.curves,
        dead: s.dead,
        lanes: s.lanes,
        total: s.total,
        word: s.word,
        plot_h: wowdps_gui_logic::inspect::geometry::PLOT_H,
    }
}

fn take(input: Input, pointer: Option<Pointer>, motion: bool, path: Option<&Path>) {
    let frame = size(
        px(samples::WIDTH + 2.0 * PAD),
        px(input.height() + 2.0 * PAD),
    );
    let mut cx: HeadlessAppContext = crate::window::shots::app();
    cx.update(|cx| cx.set_reduce_motion(!motion));
    let (window, _) = testkit::open_headless(&mut cx, frame, |_, cx| cx.new(|_| Shot { input }));
    let _ = cx.update_window(window, |_, window, cx| window.render_frame(cx));
    let at = |(x, y): (f32, f32)| point(px(PAD + x), px(PAD + y));
    let _ = cx.update_window(window, |_, window, cx| {
        let mut send = |e: gpui_kit::PlatformInput| {
            window.dispatch_event(e, cx);
        };
        match pointer {
            Some(Pointer::Rest(x, y)) => send(
                MouseMoveEvent {
                    position: at((x, y)),
                    pressed_button: None,
                    modifiers: Default::default(),
                }
                .to_platform_input(),
            ),
            Some(Pointer::Drag(from, to)) => {
                send(
                    MouseDownEvent {
                        button: MouseButton::Left,
                        position: at(from),
                        modifiers: Default::default(),
                        click_count: 1,
                        first_mouse: false,
                    }
                    .to_platform_input(),
                );
                send(
                    MouseMoveEvent {
                        position: at((to, from.1)),
                        pressed_button: Some(MouseButton::Left),
                        modifiers: Default::default(),
                    }
                    .to_platform_input(),
                );
            }
            None => {}
        }
    });
    cx.run_until_parked();
    let _ = cx.update_window(window, |_, window, cx| window.render_frame(cx));
    let image = cx.capture_screenshot(window).expect("a headless renderer");
    if let Some(path) = path {
        image.save(path).expect("png written");
        eprintln!("inspector_plot_shots: {}", path.display());
    }
}

#[test]
#[ignore = "needs a wgpu adapter"]
fn inspector_plot_shots() {
    let dir = std::env::var_os("WOWDPS_SHOTS_DIR").map(PathBuf::from);
    let named = |name: &str| dir.as_ref().map(|d| d.join(format!("plot-{name}.png")));
    for s in samples::all() {
        let (name, pointer) = (s.name, s.pointer.map(|(x, y)| Pointer::Rest(x, y)));
        take(input(s), pointer, false, named(name).as_deref());
    }
    let piece = |name: &str| dir.as_ref().map(|d| d.join(format!("{name}.png")));
    take_piece(Piece::Matrix, 150.0, piece("matrix").as_deref());
    take_piece(Piece::Chips, 64.0, piece("chips").as_deref());
    if std::env::var_os("WOWDPS_SHOTS_DELIGHT").is_none() {
        return;
    }
    let sample = |name: &str| {
        samples::all()
            .into_iter()
            .find(|s| s.name == name)
            .expect("a sample of that name")
    };
    let hover = sample("hover-plot");
    let rest = hover.pointer.map(|(x, y)| Pointer::Rest(x, y));
    take(
        input(hover),
        rest,
        true,
        named("hover-plot-delight").as_deref(),
    );
    take(
        input(sample("pair")),
        Some(Pointer::Rest(300.0, 50.0)),
        true,
        named("pair-delight").as_deref(),
    );
    take(
        input(sample("lanes")),
        Some(Pointer::Drag((140.0, 40.0), 330.0)),
        true,
        named("drag-delight").as_deref(),
    );
}
