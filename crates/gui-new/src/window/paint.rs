//! The window's drawn pieces: the line icons, stroked from gui-logic's
//! `glyph` table, and the small shapes (a dot, a ring) the frame wears.
//!
//! The icon decision (plan step 3.1): the prototype's 16-unit glyphs are
//! strokes on a canvas, as the iced window draws them — not SVG assets
//! beside Kit's Lucide set. One table feeds both GUIs; a theme recolours a
//! glyph as it does text; no asset pipeline or SVG renderer joins the build.
//!
//! GPUI's `PathBuilder` takes lyon's stroke options without re-exporting
//! lyon's caps and joins, so a round cap or join cannot be asked for. Each
//! segment is stroked as a sub-path of its own (no joins at all) and a disc
//! of the stroke's width is filled at every segment's ends: the round caps
//! and joins, drawn.

use gpui_kit::prelude::*;
use gpui_kit::{Bounds, Hsla, PathBuilder, Pixels, Point, Window, canvas, point, px};
use wowdps_gui_logic::glyph::{Glyph, STROKE, Seg, Shape};

/// A full circle into `path`, as two half arcs.
pub fn circle(path: &mut PathBuilder, c: Point<Pixels>, r: Pixels) {
    let radii = point(r, r);
    path.move_to(point(c.x + r, c.y));
    path.arc_to(radii, px(0.), false, true, point(c.x - r, c.y));
    path.arc_to(radii, px(0.), false, true, point(c.x + r, c.y));
    path.close();
}

/// A filled disc.
pub fn fill_disc(window: &mut Window, c: Point<Pixels>, r: Pixels, color: Hsla) {
    let mut path = PathBuilder::fill();
    circle(&mut path, c, r);
    if let Ok(path) = path.build() {
        window.paint_path(path, color);
    }
}

/// A stroked ring.
pub fn stroke_ring(window: &mut Window, c: Point<Pixels>, r: Pixels, width: Pixels, color: Hsla) {
    let mut path = PathBuilder::stroke(width);
    circle(&mut path, c, r);
    if let Ok(path) = path.build() {
        window.paint_path(path, color);
    }
}

/// A rounded rectangle's outline into `path`.
pub fn rounded_rect(path: &mut PathBuilder, b: Bounds<Pixels>, r: Pixels) {
    let (x0, y0) = (b.origin.x, b.origin.y);
    let (x1, y1) = (x0 + b.size.width, y0 + b.size.height);
    let r = r.min(b.size.width / 2.).min(b.size.height / 2.);
    let radii = point(r, r);
    path.move_to(point(x0 + r, y0));
    path.line_to(point(x1 - r, y0));
    path.arc_to(radii, px(0.), false, true, point(x1, y0 + r));
    path.line_to(point(x1, y1 - r));
    path.arc_to(radii, px(0.), false, true, point(x1 - r, y1));
    path.line_to(point(x0 + r, y1));
    path.arc_to(radii, px(0.), false, true, point(x0, y1 - r));
    path.line_to(point(x0, y0 + r));
    path.arc_to(radii, px(0.), false, true, point(x0 + r, y0));
    path.close();
}

/// Paint `glyph` in `color` inside `b`, centred at its short side, stroked
/// at the table's width.
pub fn paint_glyph(window: &mut Window, glyph: Glyph, b: Bounds<Pixels>, color: Hsla) {
    paint_glyph_at(window, glyph, b, color, STROKE);
}

/// [`paint_glyph`] at a stroke of `stroke` units of the 16-unit box.
pub fn paint_glyph_at(
    window: &mut Window,
    glyph: Glyph,
    b: Bounds<Pixels>,
    color: Hsla,
    stroke: f32,
) {
    let side = b.size.width.min(b.size.height);
    let k = side / 16.;
    let ox = b.origin.x + (b.size.width - side) / 2.;
    let oy = b.origin.y + (b.size.height - side) / 2.;
    let at = |(x, y): (f32, f32)| point(ox + k * x, oy + k * y);
    let width = k * stroke;
    // Straight segments, each stroked alone; where a round cap or join
    // goes: every segment's ends, the curves' included.
    let mut lines: Vec<(Point<Pixels>, Point<Pixels>)> = Vec::new();
    let mut ends: Vec<Point<Pixels>> = Vec::new();
    let mut seg = |from: Point<Pixels>, to: Point<Pixels>| lines.push((from, to));
    let mut curves = PathBuilder::stroke(width);
    for shape in glyph.shapes() {
        match shape {
            Shape::Line(a, b) => seg(at(a), at(b)),
            Shape::Poly(pts, closed) => {
                for pair in pts.windows(2) {
                    if let [a, b] = pair {
                        seg(at(*a), at(*b));
                    }
                }
                if closed && let (Some(first), Some(last)) = (pts.first(), pts.last()) {
                    seg(at(*last), at(*first));
                }
            }
            Shape::Circle(c, r) => circle(&mut curves, at(c), k * r),
            Shape::RoundRect {
                at: top,
                size,
                radius,
            } => {
                let b = Bounds::new(at(top), gpui_kit::size(k * size.0, k * size.1));
                rounded_rect(&mut curves, b, k * radius);
            }
            Shape::Path(segs) => {
                let mut pen: Option<Point<Pixels>> = None;
                let mut start: Option<Point<Pixels>> = None;
                for s in segs {
                    match s {
                        Seg::Move(p) => {
                            pen = Some(at(p));
                            start = pen;
                        }
                        Seg::Line(p) => {
                            if let Some(from) = pen {
                                seg(from, at(p));
                            }
                            pen = Some(at(p));
                        }
                        Seg::Cubic(c1, c2, to) => {
                            if let Some(from) = pen {
                                curves.move_to(from);
                                curves.cubic_bezier_to(at(to), at(c1), at(c2));
                                ends.push(from);
                                ends.push(at(to));
                            }
                            pen = Some(at(to));
                        }
                        Seg::Arc { .. } => {
                            if let (Some((a, b, large)), Seg::Arc { radius, .. }) =
                                (s.arc_ends(), s)
                            {
                                let r = k * radius;
                                curves.move_to(at(a));
                                curves.arc_to(point(r, r), px(0.), large, true, at(b));
                                ends.push(at(a));
                                ends.push(at(b));
                                pen = Some(at(b));
                                start = start.or(pen);
                            }
                        }
                        Seg::Close => {
                            if let (Some(from), Some(to)) = (pen, start) {
                                seg(from, to);
                            }
                            pen = start;
                        }
                    }
                }
            }
        }
    }
    let mut strokes = PathBuilder::stroke(width);
    for (from, to) in lines {
        strokes.move_to(from);
        strokes.line_to(to);
        ends.push(from);
        ends.push(to);
    }
    for path in [strokes.build(), curves.build()].into_iter().flatten() {
        window.paint_path(path, color);
    }
    // The caps and joins, as discs the stroke's width across.
    if !ends.is_empty() {
        let mut dots = PathBuilder::fill();
        for p in ends {
            dots.move_to(point(p.x + width / 2., p.y));
            dots.arc_to(
                point(width / 2., width / 2.),
                px(0.),
                false,
                true,
                point(p.x - width / 2., p.y),
            );
            dots.arc_to(
                point(width / 2., width / 2.),
                px(0.),
                false,
                true,
                point(p.x + width / 2., p.y),
            );
            dots.close();
        }
        if let Ok(path) = dots.build() {
            window.paint_path(path, color);
        }
    }
}

/// `glyph` in `color`, `side` square.
pub fn glyph(glyph: Glyph, side: Pixels, color: Hsla) -> impl IntoElement {
    canvas(
        |_, _, _| {},
        move |b, (), window, _| paint_glyph(window, glyph, b, color),
    )
    .size(side)
    .flex_none()
}

/// `glyph` in its parent's text colour (CSS's `currentColor`), `side`
/// square: a glyph inside a control brightens with the control's words,
/// its hover included, with no state of its own.
pub fn glyph_ink(glyph: Glyph, side: Pixels) -> impl IntoElement {
    canvas(
        |_, _, _| {},
        move |b, (), window, _| {
            let color = window.text_style().color;
            paint_glyph(window, glyph, b, color);
        },
    )
    .size(side)
    .flex_none()
}

/// A filled dot `d` across.
pub fn dot(d: Pixels, color: Hsla) -> impl IntoElement {
    canvas(
        |_, _, _| {},
        move |b, (), window, _| {
            fill_disc(
                window,
                b.center(),
                b.size.width.min(b.size.height) / 2.,
                color,
            )
        },
    )
    .size(d)
    .flex_none()
}

/// A hollow ring `d` across, its 1.5 px edge inside it (the dot's idle
/// shape).
pub fn ring(d: Pixels, color: Hsla) -> impl IntoElement {
    canvas(
        |_, _, _| {},
        move |b, (), window, _| {
            let r = b.size.width.min(b.size.height) / 2. - px(0.75);
            stroke_ring(window, b.center(), r, px(1.5), color);
        },
    )
    .size(d)
    .flex_none()
}
