//! The overlay surface's size, as both GUIs size it: the collapsed tab
//! (thin across the edge, long along it, scaled with the zoom so its glyphs
//! never outgrow it) and the expanded panel (the configured size, grown to
//! a comparison's floor while comparing). Moved from the iced overlay.

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
}
