//! Phase 1 spikes that answer by pixels (S6 fonts, S7 images, S8 canvas):
//! ignored tests that render through the headless renderer under the real
//! text system and, with `WOWDPS_SHOTS_DIR` set, save what they drew for a
//! look. Run by hand:
//! `WOWDPS_SHOTS_DIR=/tmp/s cargo test -p wowdps-gui probes -- --ignored`.

use std::borrow::Cow;
use std::path::PathBuf;

use gpui_kit::prelude::*;
use gpui_kit::{AnyWindowHandle, Context, HeadlessAppContext, Window, div, font, px, size};
use wowdps_gui_logic::fonts;

use crate::testkit;

fn save(cx: &mut HeadlessAppContext, window: AnyWindowHandle, name: &str) -> image::RgbaImage {
    let shot = cx.capture_screenshot(window).expect("a headless renderer");
    if let Some(dir) = std::env::var_os("WOWDPS_SHOTS_DIR") {
        let path = PathBuf::from(dir).join(format!("{name}.png"));
        shot.save(&path).expect("png written");
        eprintln!("probes: {}", path.display());
    }
    shot
}

/// The families a probe line is drawn in: the window's two bundled faces,
/// GPUI's system-UI alias, the generic names the overlay's iced theme
/// leaned on, and their fontconfig answers on this machine.
const FAMILIES: [&str; 7] = [
    fonts::UI_FAMILY,
    fonts::TITLE_FAMILY,
    ".SystemUIFont",
    "sans-serif",
    "monospace",
    "DejaVu Sans",
    "DejaVu Sans Mono",
];

struct Faces;

impl Render for Faces {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .bg(gpui_kit::black())
            .text_color(gpui_kit::white())
            .p_2()
            .flex()
            .flex_col()
            .gap_1()
            .children(FAMILIES.iter().map(|family| {
                div().font_family(*family).text_size(px(16.)).child(format!(
                    "{family}: Coiled Altar 1,234,567 0123456789 ⚙ Σ ☠ ● ⚑ ① ◀ ▶"
                ))
            }))
    }
}

/// S6: the bundled faces register and render by name; what the symbols
/// fall back to.
#[test]
#[ignore = "needs a wgpu adapter"]
fn s6_fonts() {
    let mut cx = testkit::headless();
    cx.update(|cx| {
        cx.text_system()
            .add_fonts(
                fonts::FONTS
                    .iter()
                    .map(|bytes| Cow::Borrowed(*bytes))
                    .collect(),
            )
            .expect("the bundled fonts load");
        let names = cx.text_system().all_font_names();
        for family in [fonts::UI_FAMILY, fonts::TITLE_FAMILY] {
            assert!(
                names.iter().any(|n| n == family),
                "{family} registered: {names:?}"
            );
        }
        let ids: Vec<_> = FAMILIES
            .iter()
            .map(|family| (*family, cx.text_system().resolve_font(&font(*family))))
            .collect();
        eprintln!("probes: resolved {ids:?}");
    });
    let window = cx
        .open_window(size(px(900.), px(260.)), |_, cx| cx.new(|_| Faces))
        .expect("a window")
        .into();
    save(&mut cx, window, "s6-fonts");
}

struct Icons;

impl Render for Icons {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let classes = wowdps_gui_logic::theme::CLASSES
            .iter()
            .filter_map(|c| crate::images::class_icon(*c));
        // Fire, Frost DK, Holy Priest, Brewmaster, Devastation.
        let specs = [63, 251, 257, 268, 1467]
            .into_iter()
            .filter_map(crate::images::spec_icon);
        // Fireball, Frostbolt, Power Word: Shield, Shadow Word: Pain, Bloodlust.
        let spells = [133, 116, 17, 589, 2825]
            .into_iter()
            .filter_map(crate::images::spell_icon);
        let row = |tiles: Vec<crate::images::Tile>| {
            div()
                .flex()
                .gap_1()
                .children(tiles.into_iter().map(|t| gpui_kit::img(t).size(px(32.))))
        };
        div()
            .size_full()
            .bg(gpui_kit::rgb(0x1b1a17))
            .p_2()
            .flex()
            .flex_col()
            .gap_2()
            .child(row(classes.collect()))
            .child(row(specs.collect()))
            .child(row(spells.collect()))
    }
}

/// S7: cache tiles become `RenderImage`s that paint the right colours.
#[test]
#[ignore = "needs a wgpu adapter and the per-machine art caches"]
fn s7_images() {
    let mut cx = testkit::headless();
    let window = cx
        .open_window(size(px(480.), px(140.)), |_, cx| cx.new(|_| Icons))
        .expect("a window")
        .into();
    save(&mut cx, window, "s7-images");
}

/// The Catmull-Rom curve through `pts` as cubic Bézier segments (each
/// `(c1, c2, to)`), the inspector plot's spline.
fn catmull_rom(
    pts: &[gpui_kit::Point<gpui_kit::Pixels>],
) -> Vec<[gpui_kit::Point<gpui_kit::Pixels>; 3]> {
    let at = |i: isize| pts[i.clamp(0, pts.len() as isize - 1) as usize];
    (0..pts.len().saturating_sub(1) as isize)
        .map(|i| {
            let (p0, p1, p2, p3) = (at(i - 1), at(i), at(i + 1), at(i + 2));
            let c1 = gpui_kit::point(p1.x + (p2.x - p0.x) / 6., p1.y + (p2.y - p0.y) / 6.);
            let c2 = gpui_kit::point(p2.x - (p3.x - p1.x) / 6., p2.y - (p3.y - p1.y) / 6.);
            [c1, c2, p2]
        })
        .collect()
}

struct Plot;

impl Render for Plot {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        use gpui_kit::{PathBuilder, canvas, linear_color_stop, linear_gradient, point, rgb, rgba};
        let series = [12., 40., 35., 70., 52., 90., 64., 30., 48., 20.];
        div().size_full().bg(rgb(0x1b1a17)).child(
            canvas(
                |bounds, _, _| bounds,
                move |_, b, window, cx| {
                    let (x0, y0, w, h) = (b.origin.x, b.origin.y, b.size.width, b.size.height);
                    let pts: Vec<_> = series
                        .iter()
                        .enumerate()
                        .map(|(i, v)| point(x0 + w * (i as f32 / 9.), y0 + h - h * (*v / 100.)))
                        .collect();
                    let curve = catmull_rom(&pts);
                    // The area under the spline, a gold wash fading to nothing.
                    let mut area = PathBuilder::fill();
                    area.move_to(point(x0, y0 + h));
                    area.line_to(pts[0]);
                    for [c1, c2, to] in &curve {
                        area.cubic_bezier_to(*to, *c1, *c2);
                    }
                    area.line_to(point(x0 + w, y0 + h));
                    area.close();
                    if let Ok(path) = area.build() {
                        window.paint_path(
                            path,
                            linear_gradient(
                                180.,
                                linear_color_stop(rgba(0xd6b25e66), 0.),
                                linear_color_stop(rgba(0xd6b25e00), 1.),
                            ),
                        );
                    }
                    // The line itself.
                    let mut line = PathBuilder::stroke(px(2.));
                    line.move_to(pts[0]);
                    for [c1, c2, to] in &curve {
                        line.cubic_bezier_to(*to, *c1, *c2);
                    }
                    if let Ok(path) = line.build() {
                        window.paint_path(path, rgb(0xd6b25e));
                    }
                    // A dashed rule at the peak.
                    let mut rule = PathBuilder::stroke(px(1.)).dash_array(&[px(4.), px(4.)]);
                    rule.move_to(point(x0, y0 + h * 0.1));
                    rule.line_to(point(x0 + w, y0 + h * 0.1));
                    if let Ok(path) = rule.build() {
                        window.paint_path(path, rgba(0xffffff55));
                    }
                    // Text in the canvas, then an image over the line.
                    let label: gpui_kit::SharedString = "Raid dps, peak 10.7M ☠".into();
                    let run = gpui_kit::TextRun {
                        len: label.len(),
                        font: font(fonts::UI_FAMILY),
                        color: gpui_kit::white(),
                        background_color: None,
                        underline: None,
                        strikethrough: None,
                    };
                    let shaped = window
                        .text_system()
                        .shape_line(label, px(14.), &[run], None);
                    let _ = shaped.paint(
                        point(x0 + px(8.), y0 + px(4.)),
                        px(18.),
                        gpui_kit::TextAlign::Left,
                        None,
                        window,
                        cx,
                    );
                    if let Some(icon) = crate::images::class_icon(wowdps_model::Class::Mage) {
                        let at = pts[5];
                        let image = gpui_kit::Bounds::new(
                            point(at.x - px(12.), at.y - px(12.)),
                            size(px(24.), px(24.)),
                        );
                        let _ =
                            window.paint_image(image, image, Default::default(), icon, 0, false);
                    }
                },
            )
            .size_full(),
        )
    }
}

/// S8: a canvas paints a gradient area, a spline, a dashed rule, text and an
/// image over the line, in the order written.
#[test]
#[ignore = "needs a wgpu adapter"]
fn s8_canvas() {
    let mut cx = testkit::headless();
    cx.update(|cx| {
        cx.text_system()
            .add_fonts(
                fonts::FONTS
                    .iter()
                    .map(|bytes| Cow::Borrowed(*bytes))
                    .collect(),
            )
            .expect("the bundled fonts load");
    });
    let window = cx
        .open_window(size(px(480.), px(200.)), |_, cx| cx.new(|_| Plot))
        .expect("a window")
        .into();
    save(&mut cx, window, "s8-canvas");
}

mod s12 {
    use gpui_kit::component::Sizable as _;
    use gpui_kit::component::button::{Button, ButtonVariants as _};
    use gpui_kit::component::input::{Input, InputState};
    use gpui_kit::component::tab::{Tab, TabBar};
    use gpui_kit::component::tooltip::Tooltip;
    use gpui_kit::prelude::*;
    use gpui_kit::{
        Context, Entity, PathBuilder, SharedString, Window, canvas, div, point, px, size,
    };
    use wowdps_gui_logic::theme::{FROST, GOLD};

    use super::{fonts_loaded, save};
    use crate::testkit;
    use crate::theme::{Look, apply};

    /// The five Kit controls S12 restyles, dressed in the prototype's values
    /// (`.btn`, `.vtab`, `.filter`, `.tip`, `.menu`), beside a bespoke
    /// canvas that draws from `Look` — the two kinds of surface a theme
    /// switch must repaint alike.
    pub(super) struct Controls {
        filter: Entity<InputState>,
    }

    impl Controls {
        pub(super) fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
            Self {
                filter: cx.new(|cx| InputState::new(window, cx).placeholder("Filter")),
            }
        }
    }

    impl Render for Controls {
        fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            let look = Look::global(cx);
            let w = &look.def.window;
            let h = crate::theme::hsla;
            let (ground, surface, edge, ink, ink_2, gold) = (
                h(w.ground),
                h(w.surface),
                h(w.edge),
                h(w.ink),
                h(w.ink_2),
                h(look.accent.base),
            );
            let tabs = TabBar::new("views").underline().selected_index(1).children(
                ["Damage", "Healing", "Taken", "Deaths"].map(|t| {
                    Tab::new()
                        .label(t)
                        .h(px(37.))
                        .px(px(9.))
                        .text_size(px(14.5))
                }),
            );
            let menu = div()
                .id("menu")
                .w(px(250.))
                .bg(surface)
                .border_1()
                .border_color(edge)
                .rounded(px(8.))
                .shadow_lg()
                .py(px(6.))
                .children(
                    ["Hide realm names", "Show ranks", "Class chrome"].map(|item| {
                        div()
                            .px(px(12.))
                            .py(px(7.))
                            .text_size(px(14.5))
                            .text_color(ink)
                            .child(item)
                    }),
                );
            div()
                .size_full()
                .bg(ground)
                .text_color(ink)
                .p_4()
                .flex()
                .flex_col()
                .gap_4()
                .child(tabs)
                .child(
                    div()
                        .flex()
                        .gap_3()
                        .items_center()
                        .child(
                            Button::new("compare")
                                .outline()
                                .small()
                                .label("Compare")
                                .h(px(26.))
                                .px(px(9.))
                                .rounded(px(5.))
                                .border_color(edge)
                                .text_color(ink_2),
                        )
                        .child(Button::new("primary").primary().small().label("Pin"))
                        .child(Input::new(&self.filter).small().w(px(160.)).h(px(28.)))
                        .child(
                            div()
                                .id("tip-host")
                                .px_2()
                                .text_color(ink_2)
                                .child("hover me")
                                .tooltip(|window, cx| {
                                    Tooltip::new(SharedString::from("Raid dps, peak 10.7M"))
                                        .build(window, cx)
                                }),
                        ),
                )
                .child(
                    div().flex().gap_4().child(menu).child(
                        canvas(
                            |b, _, _| b,
                            move |_, b, window, _| {
                                let mut path = PathBuilder::stroke(px(2.));
                                path.move_to(point(b.origin.x, b.origin.y + b.size.height));
                                path.line_to(point(
                                    b.origin.x + b.size.width * 0.4,
                                    b.origin.y + px(10.),
                                ));
                                path.line_to(point(
                                    b.origin.x + b.size.width,
                                    b.origin.y + b.size.height * 0.6,
                                ));
                                if let Ok(path) = path.build() {
                                    window.paint_path(path, gold);
                                }
                            },
                        )
                        .w(px(200.))
                        .h(px(110.)),
                    ),
                )
        }
    }

    /// S12: one definition dresses Kit's controls and our canvas; a runtime
    /// switch to another repaints both.
    #[test]
    #[ignore = "needs a wgpu adapter"]
    fn s12_theming() {
        let mut cx = testkit::headless();
        fonts_loaded(&mut cx);
        cx.update(|cx| apply(&GOLD, None, cx));
        let (window, _) =
            testkit::open_headless(&mut cx, size(px(560.), px(260.)), |window, cx| {
                cx.new(|cx| Controls::new(window, cx))
            });
        let gold = save(&mut cx, window, "s12-gold");
        cx.update(|cx| apply(&FROST, None, cx));
        // A switch marks every window dirty; the next frame paints it.
        cx.run_until_parked();
        cx.update_window(window, |_, window, cx| {
            use gpui_kit::test::TestWindowExt as _;
            window.render_frame(cx);
        })
        .expect("the window is open");
        let frost = save(&mut cx, window, "s12-frost");
        assert_eq!(gold.dimensions(), frost.dimensions());
        // The tab strip (Kit) and the canvas's line (ours) both changed.
        let changed = |x0: u32, y0: u32, x1: u32, y1: u32| {
            (y0..y1).any(|y| (x0..x1).any(|x| gold.get_pixel(x, y) != frost.get_pixel(x, y)))
        };
        let s = gold.width() / 560;
        assert!(
            changed(16 * s, 16 * s, 400 * s, 53 * s),
            "Kit's tab bar repainted"
        );
        assert!(
            changed(290 * s, 140 * s, 540 * s, 250 * s),
            "the canvas repainted"
        );
    }
}

/// The bundled faces, registered as the app does before its first window.
fn fonts_loaded(cx: &mut HeadlessAppContext) {
    cx.update(|cx| {
        cx.text_system()
            .add_fonts(
                fonts::FONTS
                    .iter()
                    .map(|bytes| Cow::Borrowed(*bytes))
                    .collect(),
            )
            .expect("the bundled fonts load");
    });
}
