//! The window's small line icons, drawn as canvas paths: the redesign's
//! 16-unit SVG glyphs (`docs/design/window-redesign.html`, its `IC` table)
//! stroked at 1.5 units with round caps and joins, scaled to whatever size
//! the caller asks for, in one colour. They replace emoji the system font
//! may or may not have, and no image is decoded for them — iced's "svg"
//! feature would pull crates the dependency policy keeps out.
//!
//! Window-only: the overlay draws none of these.

use iced::widget::canvas::{self, Canvas, Path, Stroke, path};
use iced::{Color, Element, Length, Point, Radians, Rectangle, Renderer, Size, Theme};

use wowdps_model::View;

/// One glyph. The first eight are the views; the rest are the window's own
/// controls, for the steps that draw them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LineIcon {
    Sword,
    Cross,
    Shield,
    Skull,
    Stop,
    Chain,
    Spark,
    Target,
    // The window's own controls: the filter's search glyph, the strip's
    // step arrows, the picker's caret, the options and help buttons. The
    // list glyph is for the pulls rail's step.
    Search,
    #[allow(dead_code)]
    List,
    ChevronLeft,
    ChevronRight,
    ChevronDown,
    Gear,
    Help,
}

impl LineIcon {
    /// Every glyph, for the tests that draw them all.
    #[cfg(test)]
    pub(crate) const ALL: [LineIcon; 15] = [
        LineIcon::Sword,
        LineIcon::Cross,
        LineIcon::Shield,
        LineIcon::Skull,
        LineIcon::Stop,
        LineIcon::Chain,
        LineIcon::Spark,
        LineIcon::Target,
        LineIcon::Search,
        LineIcon::List,
        LineIcon::ChevronLeft,
        LineIcon::ChevronRight,
        LineIcon::ChevronDown,
        LineIcon::Gear,
        LineIcon::Help,
    ];

    /// The glyph a view's tab wears — the prototype's `VIEWS` table.
    pub(crate) fn of_view(view: View) -> LineIcon {
        match view {
            View::Damage => LineIcon::Sword,
            View::Healing => LineIcon::Cross,
            View::Taken => LineIcon::Shield,
            View::Deaths => LineIcon::Skull,
            View::Interrupts => LineIcon::Stop,
            View::CrowdControl => LineIcon::Chain,
            View::Dispels => LineIcon::Spark,
            View::EnemyTaken => LineIcon::Target,
        }
    }

    /// The glyph as paths in the 16×16 box, `at` mapping a point of the
    /// SVG's viewBox onto the frame. Each arm is its SVG `d` (and circles
    /// and rects), transcribed to absolute points.
    fn paths(self, at: impl Fn(f32, f32) -> Point, k: f32) -> Vec<Path> {
        let line = |a: (f32, f32), b: (f32, f32)| Path::line(at(a.0, a.1), at(b.0, b.1));
        let poly = |pts: &[(f32, f32)], closed: bool| {
            Path::new(|b| {
                let mut pts = pts.iter();
                if let Some(&(x, y)) = pts.next() {
                    b.move_to(at(x, y));
                }
                for &(x, y) in pts {
                    b.line_to(at(x, y));
                }
                if closed {
                    b.close();
                }
            })
        };
        let circle = |x: f32, y: f32, r: f32| Path::circle(at(x, y), r * k);
        match self {
            // M13 3 6.4 9.6 M4.5 7.6l3.9 3.9 M3 13l2.3-2.3 M13 3h-2.4 M13 3v2.4
            LineIcon::Sword => vec![
                line((13.0, 3.0), (6.4, 9.6)),
                line((4.5, 7.6), (8.4, 11.5)),
                line((3.0, 13.0), (5.3, 10.7)),
                line((13.0, 3.0), (10.6, 3.0)),
                line((13.0, 3.0), (13.0, 5.4)),
            ],
            // M8 3v10 M3 8h10
            LineIcon::Cross => vec![line((8.0, 3.0), (8.0, 13.0)), line((3.0, 8.0), (13.0, 8.0))],
            // M8 2.3 12.7 4v3.8c0 3-2.1 4.9-4.7 5.9C5.4 12.7 3.3 10.8 3.3 7.8V4z
            LineIcon::Shield => vec![Path::new(|b| {
                b.move_to(at(8.0, 2.3));
                b.line_to(at(12.7, 4.0));
                b.line_to(at(12.7, 7.8));
                b.bezier_curve_to(at(12.7, 10.8), at(10.6, 12.7), at(8.0, 13.7));
                b.bezier_curve_to(at(5.4, 12.7), at(3.3, 10.8), at(3.3, 7.8));
                b.line_to(at(3.3, 4.0));
                b.close();
            })],
            // M8 2.5c-2.9 0-4.8 1.9-4.8 4.4 0 1.5.8 2.6 1.7 3.2v2.4h6.2v-2.4
            // c.9-.6 1.7-1.7 1.7-3.2 0-2.5-1.9-4.4-4.8-4.4z, and two eyes
            LineIcon::Skull => vec![
                Path::new(|b| {
                    b.move_to(at(8.0, 2.5));
                    b.bezier_curve_to(at(5.1, 2.5), at(3.2, 4.4), at(3.2, 6.9));
                    b.bezier_curve_to(at(3.2, 8.4), at(4.0, 9.5), at(4.9, 10.1));
                    b.line_to(at(4.9, 12.5));
                    b.line_to(at(11.1, 12.5));
                    b.line_to(at(11.1, 10.1));
                    b.bezier_curve_to(at(12.0, 9.5), at(12.8, 8.4), at(12.8, 6.9));
                    b.bezier_curve_to(at(12.8, 4.4), at(10.9, 2.5), at(8.0, 2.5));
                    b.close();
                }),
                circle(6.2, 7.3, 0.8),
                circle(9.8, 7.3, 0.8),
            ],
            // circle 8,8 r5.3; m4.3 4.3 7.4 7.4
            LineIcon::Stop => vec![circle(8.0, 8.0, 5.3), line((4.3, 4.3), (11.7, 11.7))],
            // two rects 6.3×4, rx 2, overlapping
            LineIcon::Chain => [2.2, 7.5]
                .into_iter()
                .map(|x| {
                    Path::new(|b| {
                        b.rounded_rectangle(
                            at(x, 6.0),
                            Size::new(6.3 * k, 4.0 * k),
                            (2.0 * k).into(),
                        );
                    })
                })
                .collect(),
            // M8 2.5 9.2 6.8 13.5 8 9.2 9.2 8 13.5 6.8 9.2 2.5 8 6.8 6.8z
            LineIcon::Spark => vec![poly(
                &[
                    (8.0, 2.5),
                    (9.2, 6.8),
                    (13.5, 8.0),
                    (9.2, 9.2),
                    (8.0, 13.5),
                    (6.8, 9.2),
                    (2.5, 8.0),
                    (6.8, 6.8),
                ],
                true,
            )],
            // circles r5.2 and r1.5; four ticks
            LineIcon::Target => vec![
                circle(8.0, 8.0, 5.2),
                circle(8.0, 8.0, 1.5),
                line((8.0, 1.5), (8.0, 3.8)),
                line((8.0, 12.2), (8.0, 14.5)),
                line((1.5, 8.0), (3.8, 8.0)),
                line((12.2, 8.0), (14.5, 8.0)),
            ],
            // circle 7,7 r4.2; m10.2 10.2 3.3 3.3
            LineIcon::Search => vec![circle(7.0, 7.0, 4.2), line((10.2, 10.2), (13.5, 13.5))],
            // M3 4.5h10 M3 8h10 M3 11.5h10
            LineIcon::List => [4.5, 8.0, 11.5]
                .into_iter()
                .map(|y| line((3.0, y), (13.0, y)))
                .collect(),
            // M10 3.5 5.5 8l4.5 4.5
            LineIcon::ChevronLeft => vec![poly(&[(10.0, 3.5), (5.5, 8.0), (10.0, 12.5)], false)],
            // M6 3.5 10.5 8 6 12.5
            LineIcon::ChevronRight => vec![poly(&[(6.0, 3.5), (10.5, 8.0), (6.0, 12.5)], false)],
            // m4.5 6.5 3.5 3.5 3.5-3.5
            LineIcon::ChevronDown => vec![poly(&[(4.5, 6.5), (8.0, 10.0), (11.5, 6.5)], false)],
            // circle r2.2 and eight spokes
            LineIcon::Gear => vec![
                circle(8.0, 8.0, 2.2),
                line((8.0, 1.8), (8.0, 3.6)),
                line((8.0, 12.4), (8.0, 14.2)),
                line((1.8, 8.0), (3.6, 8.0)),
                line((12.4, 8.0), (14.2, 8.0)),
                line((3.6, 3.6), (4.9, 4.9)),
                line((11.1, 11.1), (12.4, 12.4)),
                line((3.6, 12.4), (4.9, 11.1)),
                line((11.1, 4.9), (12.4, 3.6)),
            ],
            // circle r6; M6.3 6.4a1.8 1.8 0 1 1 2.5 1.6c-.5.2-.8.6-.8 1.1v.3;
            // M8 11.3v.2
            LineIcon::Help => vec![
                circle(8.0, 8.0, 6.0),
                Path::new(|b| {
                    // The SVG arc from (6.3, 6.4) to (8.8, 8.0), large and
                    // clockwise, in centre form: its centre solves to
                    // (8.0991, 6.3421), and it sweeps 249° from 178°.
                    b.arc(path::Arc {
                        center: at(8.0991, 6.3421),
                        radius: 1.8 * k,
                        start_angle: Radians(3.1094),
                        end_angle: Radians(7.4538),
                    });
                    b.bezier_curve_to(at(8.3, 8.2), at(8.0, 8.6), at(8.0, 9.1));
                    b.line_to(at(8.0, 9.4));
                }),
                line((8.0, 11.3), (8.0, 11.5)),
            ],
        }
    }
}

struct Glyph {
    icon: LineIcon,
    color: Color,
}

impl<M> canvas::Program<M> for Glyph {
    type State = ();

    fn draw(
        &self,
        _state: &(),
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: iced::mouse::Cursor,
    ) -> Vec<canvas::Geometry> {
        let mut frame = canvas::Frame::new(renderer, bounds.size());
        // Centred in whatever box the layout gave, at the box's short side.
        let side = bounds.width.min(bounds.height);
        let k = side / 16.0;
        let (ox, oy) = ((bounds.width - side) / 2.0, (bounds.height - side) / 2.0);
        let at = |x: f32, y: f32| Point::new(ox + x * k, oy + y * k);
        let stroke = Stroke::default()
            .with_width(1.5 * k)
            .with_color(self.color)
            .with_line_cap(canvas::LineCap::Round)
            .with_line_join(canvas::LineJoin::Round);
        for p in self.icon.paths(at, k) {
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
    Canvas::new(Glyph { icon, color })
        .width(Length::Fixed(size))
        .height(Length::Fixed(size))
        .into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::window::testkit::render;

    /// Every view has its own glyph: no two tabs look alike.
    #[test]
    fn every_view_wears_its_own_glyph() {
        let glyphs: Vec<LineIcon> = View::ALL.into_iter().map(LineIcon::of_view).collect();
        for (i, g) in glyphs.iter().enumerate() {
            assert!(
                !glyphs.iter().skip(i + 1).any(|o| o == g),
                "{g:?} is on two tabs"
            );
        }
        assert_eq!(LineIcon::of_view(View::Damage), LineIcon::Sword);
        assert_eq!(LineIcon::of_view(View::EnemyTaken), LineIcon::Target);
    }

    /// Every glyph has paths, stays inside its box, and draws.
    #[test]
    fn every_glyph_draws_inside_its_box() {
        for icon in LineIcon::ALL {
            let seen = std::cell::RefCell::new(Vec::new());
            let paths = icon.paths(
                |x, y| {
                    seen.borrow_mut().push((x, y));
                    Point::new(x, y)
                },
                1.0,
            );
            assert!(!paths.is_empty(), "{icon:?}");
            for (x, y) in seen.borrow().iter() {
                assert!(
                    (0.0..=16.0).contains(x) && (0.0..=16.0).contains(y),
                    "{icon:?} reaches ({x}, {y})"
                );
            }
            let _ = render(line_icon::<()>(icon, 15.0, crate::theme::INK));
        }
    }

    /// The help glyph's arc is the SVG's: it starts and ends where the `d`
    /// string's endpoints are.
    #[test]
    fn the_help_arc_meets_the_svg_endpoints() {
        let (cx, cy, r) = (8.0991_f32, 6.3421_f32, 1.8_f32);
        let end = |a: f32| (cx + r * a.cos(), cy + r * a.sin());
        let (sx, sy) = end(3.1094);
        let (ex, ey) = end(7.4538);
        assert!(
            (sx - 6.3).abs() < 0.01 && (sy - 6.4).abs() < 0.01,
            "{sx},{sy}"
        );
        assert!(
            (ex - 8.8).abs() < 0.01 && (ey - 8.0).abs() < 0.01,
            "{ex},{ey}"
        );
    }
}
