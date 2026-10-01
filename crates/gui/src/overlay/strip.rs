//! The edge strip's arithmetic (spec §7.2, spike S3), in logical pixels and
//! no GPUI types. gpui-pre sets a layer surface's margin only when it is
//! created, and the iced overlay moved its surface along the edge by
//! changing the margin live. So the surface here spans its edge's whole
//! LENGTH, anchored to the edge and both of its neighbours; its THICKNESS
//! follows the state (`Window::resize`). The content sits inside it at the
//! configured offset, and the input region is exactly the content's
//! rectangle, so the rest of the strip passes every click to the game. A
//! drag moves the content and the input region, never the surface.

use wowdps_gui_logic::config::Edge;

/// A rectangle in the strip's own coordinates: origin at the strip's
/// top-left, x right, y down.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

/// The strip on one edge of an output whose logical size is `screen`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Strip {
    pub edge: Edge,
    /// The output's logical width and height.
    pub screen: (f32, f32),
}

impl Strip {
    /// How long the edge is: the output's height for a side edge.
    pub fn length(&self) -> f32 {
        if self.edge.is_vertical() {
            self.screen.1
        } else {
            self.screen.0
        }
    }

    /// The surface's size when its content is `content` (width, height):
    /// as thick as the content across the edge, the edge's length along it.
    pub fn surface(&self, content: (f32, f32)) -> (f32, f32) {
        if self.edge.is_vertical() {
            (content.0, self.length())
        } else {
            (self.length(), content.1)
        }
    }

    /// Where `content` (width, height) sits for a configured `offset` along
    /// the edge: the offset clamped so the content stays whole on the
    /// edge, and flush with the screen's side across it.
    pub fn place(&self, content: (f32, f32), offset: f32) -> Rect {
        let (w, h) = content;
        let along = if self.edge.is_vertical() { h } else { w };
        let start = offset.clamp(0.0, (self.length() - along).max(0.0));
        if self.edge.is_vertical() {
            Rect {
                x: 0.0,
                y: start,
                w,
                h,
            }
        } else {
            Rect {
                x: start,
                y: 0.0,
                w,
                h,
            }
        }
    }

    /// The offset a drag leaves behind: where the content started, moved by
    /// the pointer's travel along the edge, clamped as `place` would.
    pub fn dragged(&self, content: (f32, f32), from: f32, travel: (f32, f32)) -> f32 {
        let delta = if self.edge.is_vertical() {
            travel.1
        } else {
            travel.0
        };
        let along = if self.edge.is_vertical() {
            content.1
        } else {
            content.0
        };
        (from + delta).clamp(0.0, (self.length() - along).max(0.0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SCREEN: (f32, f32) = (1920.0, 1080.0);

    #[test]
    fn the_surface_spans_the_edge_and_is_as_thick_as_its_content() {
        let right = Strip {
            edge: Edge::Right,
            screen: SCREEN,
        };
        assert_eq!(right.surface((26.0, 96.0)), (26.0, 1080.0));
        assert_eq!(right.surface((495.0, 557.0)), (495.0, 1080.0));
        let top = Strip {
            edge: Edge::Top,
            screen: SCREEN,
        };
        assert_eq!(top.surface((96.0, 26.0)), (1920.0, 26.0));
    }

    #[test]
    fn the_content_sits_at_its_offset_and_stays_whole() {
        let right = Strip {
            edge: Edge::Right,
            screen: SCREEN,
        };
        let tab = (26.0, 96.0);
        assert_eq!(
            right.place(tab, 272.0),
            Rect {
                x: 0.0,
                y: 272.0,
                w: 26.0,
                h: 96.0
            }
        );
        assert_eq!(right.place(tab, -40.0).y, 0.0, "never above the top");
        assert_eq!(
            right.place(tab, 5000.0).y,
            1080.0 - 96.0,
            "never past the bottom"
        );
        // A panel taller than the screen pins to the top rather than going
        // negative.
        assert_eq!(right.place((495.0, 2000.0), 300.0).y, 0.0);
        let bottom = Strip {
            edge: Edge::Bottom,
            screen: SCREEN,
        };
        assert_eq!(
            bottom.place((96.0, 26.0), 100.0),
            Rect {
                x: 100.0,
                y: 0.0,
                w: 96.0,
                h: 26.0
            }
        );
    }

    #[test]
    fn a_drag_moves_along_the_edge_only() {
        let left = Strip {
            edge: Edge::Left,
            screen: SCREEN,
        };
        let tab = (26.0, 96.0);
        assert_eq!(
            left.dragged(tab, 300.0, (80.0, -50.0)),
            250.0,
            "the across travel is ignored"
        );
        assert_eq!(left.dragged(tab, 300.0, (0.0, -900.0)), 0.0);
        let top = Strip {
            edge: Edge::Top,
            screen: SCREEN,
        };
        assert_eq!(top.dragged((96.0, 26.0), 300.0, (50.0, 400.0)), 350.0);
    }
}
