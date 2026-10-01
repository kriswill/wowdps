//! The window's small line icons, drawn as canvas paths: gui-logic's
//! `glyph` table (the redesign's 16-unit SVG glyphs) stroked at 1.5 units
//! with round caps and joins, scaled to whatever size the caller asks for,
//! in one colour. No image is decoded for them — iced's "svg" feature would
//! pull crates the dependency policy keeps out.
//!
//! Window-only: the overlay draws none of these.

use iced::widget::canvas::{self, Canvas, Path, Stroke, path};
use iced::{Color, Element, Length, Point, Radians, Rectangle, Renderer, Size, Theme};

pub(crate) use wowdps_gui_logic::glyph::Glyph as LineIcon;
use wowdps_gui_logic::glyph::{STROKE, Seg, Shape};

/// The glyph as paths, `at` mapping a point of the 16-unit box onto the
/// frame and `k` the frame's pixels per unit.
fn paths(icon: LineIcon, at: impl Fn(f32, f32) -> Point, k: f32) -> Vec<Path> {
    let p = |(x, y): (f32, f32)| at(x, y);
    icon.shapes()
        .into_iter()
        .map(|shape| match shape {
            Shape::Line(a, b) => Path::line(p(a), p(b)),
            Shape::Poly(pts, closed) => Path::new(|b| {
                let mut pts = pts.iter();
                if let Some(&first) = pts.next() {
                    b.move_to(p(first));
                }
                for &pt in pts {
                    b.line_to(p(pt));
                }
                if closed {
                    b.close();
                }
            }),
            Shape::Circle(c, r) => Path::circle(p(c), r * k),
            Shape::RoundRect {
                at: top,
                size,
                radius,
            } => Path::new(|b| {
                b.rounded_rectangle(
                    p(top),
                    Size::new(size.0 * k, size.1 * k),
                    (radius * k).into(),
                );
            }),
            Shape::Path(segs) => Path::new(|b| {
                for seg in segs {
                    match seg {
                        Seg::Move(pt) => b.move_to(p(pt)),
                        Seg::Line(pt) => b.line_to(p(pt)),
                        Seg::Cubic(c1, c2, to) => b.bezier_curve_to(p(c1), p(c2), p(to)),
                        Seg::Arc {
                            center,
                            radius,
                            start,
                            end,
                        } => b.arc(path::Arc {
                            center: p(center),
                            radius: radius * k,
                            start_angle: Radians(start),
                            end_angle: Radians(end),
                        }),
                        Seg::Close => b.close(),
                    }
                }
            }),
        })
        .collect()
}

struct Glyph {
    icon: LineIcon,
    color: Color,
    /// Drawn in this ink instead while the pointer is over the square of
    /// this side centred on the glyph — its button's target — which is how
    /// an icon button brightens its glyph with no state of its own
    /// (`.ibtn:hover{color:var(--ink)}`).
    lit: Option<(Color, f32)>,
}

impl<M> canvas::Program<M> for Glyph {
    type State = ();

    fn draw(
        &self,
        _state: &(),
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        cursor: iced::mouse::Cursor,
    ) -> Vec<canvas::Geometry> {
        let color = match self.lit {
            Some((lit, target)) if cursor.is_over(around(bounds, target)) => lit,
            _ => self.color,
        };
        let mut frame = canvas::Frame::new(renderer, bounds.size());
        // Centred in whatever box the layout gave, at the box's short side.
        let side = bounds.width.min(bounds.height);
        let k = side / 16.0;
        let (ox, oy) = ((bounds.width - side) / 2.0, (bounds.height - side) / 2.0);
        let at = |x: f32, y: f32| Point::new(ox + x * k, oy + y * k);
        let stroke = Stroke::default()
            .with_width(STROKE * k)
            .with_color(color)
            .with_line_cap(canvas::LineCap::Round)
            .with_line_join(canvas::LineJoin::Round);
        for p in paths(self.icon, at, k) {
            frame.stroke(&p, stroke);
        }
        vec![frame.into_geometry()]
    }
}

/// `icon` in `color`, `size` logical pixels square. Emits nothing: wrap it
/// in the caller's own `mouse_area`.
pub(crate) fn line_icon<M: 'static>(
    icon: LineIcon,
    size: f32,
    color: Color,
) -> Element<'static, M> {
    Canvas::new(Glyph {
        icon,
        color,
        lit: None,
    })
    .width(Length::Fixed(size))
    .height(Length::Fixed(size))
    .into()
}

/// [`line_icon`] drawn in `lit` while the pointer is over the `target`-side
/// square centred on it: an icon button's glyph, brightening under the
/// pointer anywhere on its button.
pub(crate) fn line_icon_lit<M: 'static>(
    icon: LineIcon,
    size: f32,
    color: Color,
    lit: Color,
    target: f32,
) -> Element<'static, M> {
    Canvas::new(Glyph {
        icon,
        color,
        lit: Some((lit, target)),
    })
    .width(Length::Fixed(size))
    .height(Length::Fixed(size))
    .into()
}

/// The `side`-square centred on `bounds`.
fn around(bounds: Rectangle, side: f32) -> Rectangle {
    let (dx, dy) = ((side - bounds.width) / 2.0, (side - bounds.height) / 2.0);
    Rectangle {
        x: bounds.x - dx,
        y: bounds.y - dy,
        width: side,
        height: side,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::window::testkit::render;

    /// Every glyph has paths and draws. (Their geometry is gui-logic's,
    /// tested there.)
    #[test]
    fn every_glyph_draws() {
        for icon in LineIcon::ALL {
            let paths = paths(icon, Point::new, 1.0);
            assert!(!paths.is_empty(), "{icon:?}");
            let _ = render(line_icon::<()>(icon, 15.0, crate::theme::INK));
        }
    }

    /// A lit glyph answers the pointer over its whole button's square, not
    /// its own box alone — and draws either way.
    #[test]
    fn a_lit_glyph_lights_across_its_target() {
        let glyph = Rectangle::new(Point::new(7.0, 7.0), Size::new(16.0, 16.0));
        let target = around(glyph, 30.0);
        assert_eq!(target, Rectangle::new(Point::ORIGIN, Size::new(30.0, 30.0)));
        let at = |x, y| iced::mouse::Cursor::Available(Point::new(x, y));
        assert!(at(1.0, 29.0).is_over(target), "a corner of the button");
        assert!(!at(31.0, 15.0).is_over(target));
        let _ = render(line_icon_lit::<()>(
            LineIcon::ChevronLeft,
            16.0,
            crate::theme::INK_2,
            crate::theme::INK,
            30.0,
        ));
    }
}
