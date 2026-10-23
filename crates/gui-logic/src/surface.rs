//! The overlay surface's size, as both GUIs size it: the collapsed tab
//! (thin across the edge, long along it, scaled with the zoom so its glyphs
//! never outgrow it) and the expanded panel (the configured size, grown to
//! a comparison's floor while comparing), the edge a tab dragged across
//! a monitor lands on, and where macOS's panel opens. Moved from the iced
//! overlay.

use crate::config::{Config, Edge};

/// The tab: this thick across the edge and this long along it, at zoom 1.
pub const TAB_THICKNESS: u32 = 26;
pub const TAB_LENGTH: u32 = 96;

/// R12: a comparison is two spell tables and two graphs, and the meter
/// panel's width is one column of names. Floors, not sizes: a panel
/// already dragged bigger keeps its size.
pub const COMPARE_MIN: (u32, u32) = (620, 460);

/// The tab's surface size on `edge` at `zoom`.
pub fn tab_size(edge: Edge, zoom: f32) -> (u32, u32) {
    let thickness = (TAB_THICKNESS as f32 * zoom).round() as u32;
    let length = (TAB_LENGTH as f32 * zoom).round() as u32;
    if edge.is_vertical() {
        (thickness, length)
    } else {
        (length, thickness)
    }
}

/// The surface's size: the tab collapsed, the configured panel expanded,
/// grown to `COMPARE_MIN` at the zoom while `comparing`.
pub fn surface_size(cfg: &Config, expanded: bool, comparing: bool) -> (u32, u32) {
    if !expanded {
        return tab_size(cfg.edge, cfg.zoom);
    }
    let (w, h) = (cfg.width, cfg.height);
    if comparing {
        let z = cfg.zoom;
        return (
            w.max((COMPARE_MIN.0 as f32 * z) as u32),
            h.max((COMPARE_MIN.1 as f32 * z) as u32),
        );
    }
    (w, h)
}

/// The top-left corner of macOS's panel, in the display's logical pixels
/// (top-left origin), for `content` (width, height) on a `screen` (width,
/// height): at the configured `offset` along the edge, clamped so the
/// content stays whole (as the Linux strip's `place` clamps it), and flush
/// with the screen's side across it, so the expanded panel grows inward
/// from the edge. The room left on each axis is floored at 0, so content
/// larger than the screen opens at 0, never at a negative origin.
pub fn panel_origin(
    edge: Edge,
    offset: f32,
    content: (f32, f32),
    screen: (f32, f32),
) -> (f32, f32) {
    let room_x = (screen.0 - content.0).max(0.0);
    let room_y = (screen.1 - content.1).max(0.0);
    match edge {
        Edge::Left => (0.0, offset.clamp(0.0, room_y)),
        Edge::Right => (room_x, offset.clamp(0.0, room_y)),
        Edge::Top => (offset.clamp(0.0, room_x), 0.0),
        Edge::Bottom => (offset.clamp(0.0, room_x), room_y),
    }
}

/// The monitor edge nearest the pointer, when it is close enough to
/// capture the tab and beats the current edge by enough to be worth
/// flipping to. The near-edge gate keeps mid-screen drags from flailing
/// between two far-but-equidistant edges (dead center, every edge ties);
/// the hysteresis keeps corners from flickering.
pub fn nearest_edge(current: Edge, p: (f32, f32), mon: (i32, i32, i32, i32)) -> Option<Edge> {
    const NEAR: f32 = 150.0;
    const HYSTERESIS: f32 = 24.0;
    let (mx, my, mw, mh) = mon;
    let distances = [
        (Edge::Left, p.0 - mx as f32),
        (Edge::Right, (mx + mw - 1) as f32 - p.0),
        (Edge::Top, p.1 - my as f32),
        (Edge::Bottom, (my + mh - 1) as f32 - p.1),
    ];
    let to_current = distances.iter().find(|(e, _)| *e == current)?.1;
    let (best, to_best) = distances.into_iter().min_by(|a, b| a.1.total_cmp(&b.1))?;
    (best != current && to_best < NEAR && to_best + HYSTERESIS < to_current).then_some(best)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_tab_turns_with_its_edge_and_scales_with_the_zoom() {
        assert_eq!(tab_size(Edge::Right, 1.25), (33, 120));
        assert_eq!(tab_size(Edge::Top, 1.25), (120, 33));
        assert_eq!(tab_size(Edge::Left, 1.30), (34, 125));
    }

    #[test]
    fn a_comparison_grows_the_panel_to_its_floor_only() {
        let cfg = Config {
            zoom: 1.25,
            width: 410,
            height: 460,
            ..Config::default()
        };
        assert_eq!(surface_size(&cfg, true, false), (410, 460));
        assert_eq!(surface_size(&cfg, true, true), (775, 575));
        let big = Config {
            width: 900,
            height: 700,
            ..cfg.clone()
        };
        assert_eq!(surface_size(&big, true, true), (900, 700));
        assert_eq!(surface_size(&cfg, false, true), (33, 120));
    }

    #[test]
    fn the_panel_sits_at_the_offset_along_its_edge_and_flush_across_it() {
        let screen = (1920.0, 1080.0);
        let content = (410.0, 460.0);
        assert_eq!(
            panel_origin(Edge::Left, 200.0, content, screen),
            (0.0, 200.0)
        );
        assert_eq!(
            panel_origin(Edge::Right, 200.0, content, screen),
            (1510.0, 200.0)
        );
        assert_eq!(
            panel_origin(Edge::Top, 300.0, content, screen),
            (300.0, 0.0)
        );
        assert_eq!(
            panel_origin(Edge::Bottom, 300.0, content, screen),
            (300.0, 620.0)
        );
    }

    #[test]
    fn the_offset_is_clamped_so_the_panel_stays_whole() {
        let screen = (1920.0, 1080.0);
        let content = (410.0, 460.0);
        // Past the far end: the panel's far side meets the screen's.
        assert_eq!(
            panel_origin(Edge::Right, 5000.0, content, screen),
            (1510.0, 620.0)
        );
        assert_eq!(
            panel_origin(Edge::Bottom, 5000.0, content, screen),
            (1510.0, 620.0)
        );
        // Before the near end: the panel starts at the screen's.
        assert_eq!(panel_origin(Edge::Left, -40.0, content, screen), (0.0, 0.0));
        assert_eq!(panel_origin(Edge::Top, -40.0, content, screen), (0.0, 0.0));
    }

    #[test]
    fn a_panel_larger_than_the_screen_opens_at_zero() {
        let screen = (800.0, 600.0);
        let content = (1000.0, 900.0);
        for edge in [Edge::Left, Edge::Right, Edge::Top, Edge::Bottom] {
            assert_eq!(
                panel_origin(edge, 120.0, content, screen),
                (0.0, 0.0),
                "{edge:?}"
            );
        }
    }

    #[test]
    fn reorientation_needs_a_near_edge_and_a_clear_winner() {
        let mon = (0, 0, 3440, 1440);
        assert_eq!(
            nearest_edge(Edge::Right, (1720.0, 720.0), mon),
            None,
            "dead center: top/bottom are nearest but too far to capture"
        );
        assert_eq!(
            nearest_edge(Edge::Right, (1720.0, 100.0), mon),
            Some(Edge::Top),
            "near the top, far from the right: flip"
        );
        assert_eq!(
            nearest_edge(Edge::Top, (1720.0, 1339.0), mon),
            Some(Edge::Bottom)
        );
        assert_eq!(
            nearest_edge(Edge::Right, (3400.0, 1400.0), mon),
            None,
            "corner: bottom is equally near but not by the hysteresis margin"
        );
        assert_eq!(
            nearest_edge(Edge::Right, (3300.0, 1430.0), mon),
            Some(Edge::Bottom),
            "clearly past the corner diagonal: flip"
        );
    }
}
