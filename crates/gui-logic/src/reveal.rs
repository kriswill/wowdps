//! A strip of tabs too wide for its row, seen through a window onto it that
//! keeps the ACTIVE tab whole in sight (the prototype's `.vscroll`, whose
//! browser scrolls the selected tab into view): where the window stands,
//! and which of its edges have more of the strip past them to fade. Each
//! GUI lays the strip out and moves it.

/// A wheel notch, in pixels along the strip.
pub const LINE: f32 = 60.0;
/// How far in from an edge the strip fades into what it sits on, where
/// more of it lies past that edge: a tab cut there reads as going on
/// rather than as a stray glyph (a skull and a sliver of "D" read "[").
pub const FADE: f32 = 24.0;

/// Which edges have more of the strip past them, for a strip `long` wide
/// seen `offset` along through a window `width` wide: (left, right).
pub fn cut_edges(offset: f32, width: f32, long: f32) -> (bool, bool) {
    const EPS: f32 = 0.5;
    let far = (long - width).max(0.0);
    (offset > EPS, offset < far - EPS)
}

/// The offset nearest `offset` at which `[start, end]` is whole inside a
/// window `width` wide: unchanged when it already is; its start at the
/// window's left edge when it is off to the left (or wider than the
/// window); its end at the right edge when it is off to the right.
pub fn nearest(offset: f32, width: f32, start: f32, end: f32) -> f32 {
    if end - start >= width || start < offset {
        start
    } else if end > offset + width {
        end - width
    } else {
        offset
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An edge fades only where more of the strip lies past it.
    #[test]
    fn only_an_edge_with_more_beyond_it_fades() {
        assert_eq!(cut_edges(0.0, 300.0, 300.0), (false, false), "all in sight");
        assert_eq!(
            cut_edges(0.0, 300.0, 500.0),
            (false, true),
            "more to the right"
        );
        assert_eq!(
            cut_edges(200.0, 300.0, 500.0),
            (true, false),
            "at the far end"
        );
        assert_eq!(cut_edges(100.0, 300.0, 500.0), (true, true));
    }

    #[test]
    fn a_tab_in_sight_stays_where_it_is() {
        assert_eq!(nearest(0.0, 300.0, 100.0, 200.0), 0.0);
        assert_eq!(nearest(50.0, 300.0, 100.0, 200.0), 50.0);
    }

    #[test]
    fn a_tab_out_of_sight_is_brought_to_the_nearer_edge() {
        // Off to the right: its end at the window's right edge.
        assert_eq!(nearest(0.0, 300.0, 350.0, 450.0), 150.0);
        // Off to the left: its start at the left edge.
        assert_eq!(nearest(400.0, 300.0, 100.0, 200.0), 100.0);
        // Cut by the right edge.
        assert_eq!(nearest(0.0, 300.0, 250.0, 340.0), 40.0);
        // Wider than the window: its start, its words' beginning.
        assert_eq!(nearest(0.0, 80.0, 100.0, 200.0), 100.0);
    }
}
