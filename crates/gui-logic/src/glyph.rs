//! The window's small line icons as data: the redesign's 16-unit SVG
//! glyphs (`docs/design/window-redesign.html`, its `IC` table), each a list
//! of shapes in the 16 × 16 box, stroked at [`STROKE`] units with round caps
//! and joins in one colour by whichever GUI draws them. They replace emoji
//! the system font may or may not have; no image is decoded for them and no
//! SVG renderer is needed, so a theme recolours them as it does text.

use wowdps_model::View;

/// The stroke width, in glyph units.
pub const STROKE: f32 = 1.5;

/// A point in the glyph's 16 × 16 box.
pub type P = (f32, f32);

/// One stroked shape.
#[derive(Debug, Clone, PartialEq)]
pub enum Shape {
    /// A straight line.
    Line(P, P),
    /// Points joined in order, closed back to the first when `closed`.
    Poly(Vec<P>, bool),
    /// A circle by its centre and radius.
    Circle(P, f32),
    /// A rectangle by its top-left, size and corner radius.
    RoundRect { at: P, size: P, radius: f32 },
    /// A path of segments.
    Path(Vec<Seg>),
}

/// A path segment.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Seg {
    Move(P),
    Line(P),
    /// A cubic Bézier: its two control points and its end.
    Cubic(P, P, P),
    /// A circular arc in centre form: centre, radius, start and end angle
    /// in radians (y down, so increasing angles turn clockwise on screen),
    /// drawn from the start angle's point to the end's.
    Arc {
        center: P,
        radius: f32,
        start: f32,
        end: f32,
    },
    Close,
}

/// One glyph. The first eight are the views; the rest are the window's own
/// controls.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Glyph {
    Sword,
    Cross,
    Shield,
    Skull,
    Stop,
    Chain,
    Spark,
    Target,
    // The window's own controls: the filter's search glyph, the fight
    // header's rail button and step arrows, the picker's caret, the
    // options and help buttons.
    Search,
    List,
    ChevronLeft,
    ChevronRight,
    ChevronDown,
    Gear,
    Help,
    // A sorted heading's direction (the prototype's ` ↓` / ` ↑`, which the
    // window's faces carry no glyph for) and the filter's clear mark (`x`).
    ArrowDown,
    ArrowUp,
    Close,
    // The inspector's buttons: pin for comparison, the talent viewer, the
    // graph's mode.
    Compare,
    Book,
    Graph,
    // The pull rail's kill and timed mark (a wipe's is `Close`).
    Check,
    // The inspector's width: widened over the stage, and back beside the
    // meter.
    Expand,
    Collapse,
}

impl Glyph {
    /// Every glyph, for the tests that draw them all.
    pub const ALL: [Glyph; 24] = [
        Glyph::Sword,
        Glyph::Cross,
        Glyph::Shield,
        Glyph::Skull,
        Glyph::Stop,
        Glyph::Chain,
        Glyph::Spark,
        Glyph::Target,
        Glyph::Search,
        Glyph::List,
        Glyph::ChevronLeft,
        Glyph::ChevronRight,
        Glyph::ChevronDown,
        Glyph::Gear,
        Glyph::Help,
        Glyph::ArrowDown,
        Glyph::ArrowUp,
        Glyph::Close,
        Glyph::Compare,
        Glyph::Book,
        Glyph::Graph,
        Glyph::Check,
        Glyph::Expand,
        Glyph::Collapse,
    ];

    /// The glyph a view's tab wears — the prototype's `VIEWS` table.
    pub fn of_view(view: View) -> Glyph {
        match view {
            View::Damage => Glyph::Sword,
            View::Healing => Glyph::Cross,
            View::Taken => Glyph::Shield,
            View::Deaths => Glyph::Skull,
            View::Interrupts => Glyph::Stop,
            View::CrowdControl => Glyph::Chain,
            View::Dispels => Glyph::Spark,
            View::EnemyTaken => Glyph::Target,
        }
    }

    /// The glyph's shapes in the 16 × 16 box. Each arm is its SVG `d` (and
    /// circles and rects), transcribed to absolute points.
    pub fn shapes(self) -> Vec<Shape> {
        use Seg::{Close, Cubic, Line, Move};
        let line = |a: P, b: P| Shape::Line(a, b);
        let poly = |pts: &[P], closed: bool| Shape::Poly(pts.to_vec(), closed);
        let circle = |x: f32, y: f32, r: f32| Shape::Circle((x, y), r);
        match self {
            // M13 3 6.4 9.6 M4.5 7.6l3.9 3.9 M3 13l2.3-2.3 M13 3h-2.4 M13 3v2.4
            Glyph::Sword => vec![
                line((13.0, 3.0), (6.4, 9.6)),
                line((4.5, 7.6), (8.4, 11.5)),
                line((3.0, 13.0), (5.3, 10.7)),
                line((13.0, 3.0), (10.6, 3.0)),
                line((13.0, 3.0), (13.0, 5.4)),
            ],
            // M8 3v10 M3 8h10
            Glyph::Cross => vec![line((8.0, 3.0), (8.0, 13.0)), line((3.0, 8.0), (13.0, 8.0))],
            // M8 2.3 12.7 4v3.8c0 3-2.1 4.9-4.7 5.9C5.4 12.7 3.3 10.8 3.3 7.8V4z
            Glyph::Shield => vec![Shape::Path(vec![
                Move((8.0, 2.3)),
                Line((12.7, 4.0)),
                Line((12.7, 7.8)),
                Cubic((12.7, 10.8), (10.6, 12.7), (8.0, 13.7)),
                Cubic((5.4, 12.7), (3.3, 10.8), (3.3, 7.8)),
                Line((3.3, 4.0)),
                Close,
            ])],
            // M8 2.5c-2.9 0-4.8 1.9-4.8 4.4 0 1.5.8 2.6 1.7 3.2v2.4h6.2v-2.4
            // c.9-.6 1.7-1.7 1.7-3.2 0-2.5-1.9-4.4-4.8-4.4z, and two eyes
            Glyph::Skull => vec![
                Shape::Path(vec![
                    Move((8.0, 2.5)),
                    Cubic((5.1, 2.5), (3.2, 4.4), (3.2, 6.9)),
                    Cubic((3.2, 8.4), (4.0, 9.5), (4.9, 10.1)),
                    Line((4.9, 12.5)),
                    Line((11.1, 12.5)),
                    Line((11.1, 10.1)),
                    Cubic((12.0, 9.5), (12.8, 8.4), (12.8, 6.9)),
                    Cubic((12.8, 4.4), (10.9, 2.5), (8.0, 2.5)),
                    Close,
                ]),
                circle(6.2, 7.3, 0.8),
                circle(9.8, 7.3, 0.8),
            ],
            // circle 8,8 r5.3; m4.3 4.3 7.4 7.4
            Glyph::Stop => vec![circle(8.0, 8.0, 5.3), line((4.3, 4.3), (11.7, 11.7))],
            // two rects 6.3×4, rx 2, overlapping
            Glyph::Chain => [2.2, 7.5]
                .into_iter()
                .map(|x| Shape::RoundRect {
                    at: (x, 6.0),
                    size: (6.3, 4.0),
                    radius: 2.0,
                })
                .collect(),
            // M8 2.5 9.2 6.8 13.5 8 9.2 9.2 8 13.5 6.8 9.2 2.5 8 6.8 6.8z
            Glyph::Spark => vec![poly(
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
            Glyph::Target => vec![
                circle(8.0, 8.0, 5.2),
                circle(8.0, 8.0, 1.5),
                line((8.0, 1.5), (8.0, 3.8)),
                line((8.0, 12.2), (8.0, 14.5)),
                line((1.5, 8.0), (3.8, 8.0)),
                line((12.2, 8.0), (14.5, 8.0)),
            ],
            // circle 7,7 r4.2; m10.2 10.2 3.3 3.3
            Glyph::Search => vec![circle(7.0, 7.0, 4.2), line((10.2, 10.2), (13.5, 13.5))],
            // M3 4.5h10 M3 8h10 M3 11.5h10
            Glyph::List => [4.5, 8.0, 11.5]
                .into_iter()
                .map(|y| line((3.0, y), (13.0, y)))
                .collect(),
            // M10 3.5 5.5 8l4.5 4.5
            Glyph::ChevronLeft => vec![poly(&[(10.0, 3.5), (5.5, 8.0), (10.0, 12.5)], false)],
            // M6 3.5 10.5 8 6 12.5
            Glyph::ChevronRight => vec![poly(&[(6.0, 3.5), (10.5, 8.0), (6.0, 12.5)], false)],
            // m4.5 6.5 3.5 3.5 3.5-3.5
            Glyph::ChevronDown => vec![poly(&[(4.5, 6.5), (8.0, 10.0), (11.5, 6.5)], false)],
            // M8 3.5v9 m-3.2-3.2 3.2 3.2 3.2-3.2: a stem and its head, the
            // chevrons' stroke
            Glyph::ArrowDown => vec![
                line((8.0, 3.5), (8.0, 12.5)),
                poly(&[(4.8, 9.3), (8.0, 12.5), (11.2, 9.3)], false),
            ],
            // the same, pointing up
            Glyph::ArrowUp => vec![
                line((8.0, 12.5), (8.0, 3.5)),
                poly(&[(4.8, 6.7), (8.0, 3.5), (11.2, 6.7)], false),
            ],
            // m4.5 4.5 7 7 M11.5 4.5l-7 7
            Glyph::Close => vec![
                line((4.5, 4.5), (11.5, 11.5)),
                line((11.5, 4.5), (4.5, 11.5)),
            ],
            // M5.5 2.5v11 M10.5 2.5v11 M2.5 6h3 M10.5 10h3
            Glyph::Compare => vec![
                line((5.5, 2.5), (5.5, 13.5)),
                line((10.5, 2.5), (10.5, 13.5)),
                line((2.5, 6.0), (5.5, 6.0)),
                line((10.5, 10.0), (13.5, 10.0)),
            ],
            // M2.8 3.5h3.7A1.5 1.5 0 0 1 8 5v8a1.5 1.5 0 0 0-1.5-1.5H2.8z
            // M13.2 3.5H9.5A1.5 1.5 0 0 0 8 5 M13.2 3.5v8H9.5A1.5 1.5 0 0
            // 0 8 13 — each quarter arc as its cubic (0.828 = 1.5 × 0.552).
            Glyph::Book => vec![
                Shape::Path(vec![
                    Move((2.8, 3.5)),
                    Line((6.5, 3.5)),
                    Cubic((7.328, 3.5), (8.0, 4.172), (8.0, 5.0)),
                    Line((8.0, 13.0)),
                    Cubic((8.0, 12.172), (7.328, 11.5), (6.5, 11.5)),
                    Line((2.8, 11.5)),
                    Close,
                ]),
                Shape::Path(vec![
                    Move((13.2, 3.5)),
                    Line((9.5, 3.5)),
                    Cubic((8.672, 3.5), (8.0, 4.172), (8.0, 5.0)),
                ]),
                Shape::Path(vec![
                    Move((13.2, 3.5)),
                    Line((13.2, 11.5)),
                    Line((9.5, 11.5)),
                    Cubic((8.672, 11.5), (8.0, 12.172), (8.0, 13.0)),
                ]),
            ],
            // M2.5 12.5h11 M3.5 10l3-3.5 2.5 2 4-5
            Glyph::Graph => vec![
                line((2.5, 12.5), (13.5, 12.5)),
                poly(&[(3.5, 10.0), (6.5, 6.5), (9.0, 8.5), (13.0, 3.5)], false),
            ],
            // m3.5 8.4 2.9 2.9 6.1-6.6
            Glyph::Check => vec![poly(&[(3.5, 8.4), (6.4, 11.3), (12.5, 4.7)], false)],
            // M9.5 3.5h3v3 M12.5 3.5 8.8 7.2 M6.5 12.5h-3v-3 M3.5 12.5l3.7-3.7:
            // two corners pointing out, the chevrons' stroke
            Glyph::Expand => vec![
                poly(&[(9.5, 3.5), (12.5, 3.5), (12.5, 6.5)], false),
                line((12.5, 3.5), (8.8, 7.2)),
                poly(&[(6.5, 12.5), (3.5, 12.5), (3.5, 9.5)], false),
                line((3.5, 12.5), (7.2, 8.8)),
            ],
            // M9.2 3.6v3.2h3.2 M9.2 6.8l3.6-3.6 M6.8 12.4V9.2H3.6 M6.8 9.2
            // 3.2 12.8: the same corners pointing in
            Glyph::Collapse => vec![
                poly(&[(9.2, 3.6), (9.2, 6.8), (12.4, 6.8)], false),
                line((9.2, 6.8), (12.8, 3.2)),
                poly(&[(6.8, 12.4), (6.8, 9.2), (3.6, 9.2)], false),
                line((6.8, 9.2), (3.2, 12.8)),
            ],
            // circle r2.2 and eight spokes
            Glyph::Gear => vec![
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
            Glyph::Help => vec![
                circle(8.0, 8.0, 6.0),
                // The SVG arc from (6.3, 6.4) to (8.8, 8.0), large and
                // clockwise, in centre form: its centre solves to
                // (8.0991, 6.3421), and it sweeps 249° from 178°.
                Shape::Path(vec![
                    Seg::Arc {
                        center: (8.0991, 6.3421),
                        radius: 1.8,
                        start: 3.1094,
                        end: 7.4538,
                    },
                    Cubic((8.3, 8.2), (8.0, 8.6), (8.0, 9.1)),
                    Line((8.0, 9.4)),
                ]),
                line((8.0, 11.3), (8.0, 11.5)),
            ],
        }
    }
}

impl Seg {
    /// An arc's start and end points: where a path that draws it by its
    /// endpoints (SVG's `A`) starts and stops, and whether it takes the
    /// long way round. `None` for any other segment.
    pub fn arc_ends(self) -> Option<(P, P, bool)> {
        let Seg::Arc {
            center,
            radius,
            start,
            end,
        } = self
        else {
            return None;
        };
        let at = |a: f32| (center.0 + radius * a.cos(), center.1 + radius * a.sin());
        Some((
            at(start),
            at(end),
            (end - start).abs() > std::f32::consts::PI,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The help glyph's arc is the SVG's: from (6.3, 6.4) to (8.8, 8.0),
    /// the long way round.
    #[test]
    fn the_help_arc_runs_between_the_svg_s_points() {
        let shapes = Glyph::Help.shapes();
        let Some(Shape::Path(segs)) = shapes.get(1) else {
            panic!("the help glyph's hook is a path");
        };
        let (from, to, large) = segs[0].arc_ends().expect("an arc");
        assert!(
            (from.0 - 6.3).abs() < 0.01 && (from.1 - 6.4).abs() < 0.01,
            "{from:?}"
        );
        assert!(
            (to.0 - 8.8).abs() < 0.01 && (to.1 - 8.0).abs() < 0.01,
            "{to:?}"
        );
        assert!(large);
    }

    /// Every glyph has shapes, and every point sits in the 16-unit box.
    #[test]
    fn every_glyph_stays_in_its_box() {
        let inside = |p: P| (0.0..=16.0).contains(&p.0) && (0.0..=16.0).contains(&p.1);
        for g in Glyph::ALL {
            let shapes = g.shapes();
            assert!(!shapes.is_empty(), "{g:?}");
            for s in shapes {
                let ok = match s {
                    Shape::Line(a, b) => inside(a) && inside(b),
                    Shape::Poly(pts, _) => pts.into_iter().all(inside),
                    Shape::Circle(c, r) => inside((c.0 - r, c.1 - r)) && inside((c.0 + r, c.1 + r)),
                    Shape::RoundRect { at, size, .. } => {
                        inside(at) && inside((at.0 + size.0, at.1 + size.1))
                    }
                    Shape::Path(segs) => segs.into_iter().all(|seg| match seg {
                        Seg::Move(p) | Seg::Line(p) => inside(p),
                        Seg::Cubic(a, b, c) => inside(a) && inside(b) && inside(c),
                        Seg::Arc { .. } => seg
                            .arc_ends()
                            .is_some_and(|(a, b, _)| inside(a) && inside(b)),
                        Seg::Close => true,
                    }),
                };
                assert!(ok, "{g:?} leaves its box");
            }
        }
    }

    /// Every view has its own glyph.
    #[test]
    fn every_view_has_its_own_glyph() {
        let views = [
            View::Damage,
            View::Healing,
            View::Taken,
            View::Deaths,
            View::Interrupts,
            View::CrowdControl,
            View::Dispels,
            View::EnemyTaken,
        ];
        let glyphs: std::collections::HashSet<Glyph> =
            views.into_iter().map(Glyph::of_view).collect();
        assert_eq!(glyphs.len(), views.len());
    }
}
