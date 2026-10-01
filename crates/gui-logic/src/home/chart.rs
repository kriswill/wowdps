//! Home's two charts as geometry, free of any GUI (moved from the iced
//! window's `home/charts.rs`): "Your rank, pull by pull" — each pull's
//! place in the role as a height, the top of the role at the top — and
//! "Effective dps on keys" — a dot per run over a round value axis. The
//! boxes are the prototype's SVG `viewBox`es; every position here is in a
//! chart `w` wide, scaled from its box.

use super::{NightPull, TrendPoint, axis};

/// The rank chart's box (`viewBox="0 0 300 140"`), and the widest it is
/// drawn (`max-width:360px`).
pub const SLOPE_W: f32 = 300.0;
pub const SLOPE_H: f32 = 140.0;
pub const SLOPE_MAX_W: f32 = 360.0;
/// Its two guides — the top of the role and the bottom — and where they
/// run (`x1=18`, `x2=W-10`); its dots' first x and the room the last one
/// leaves (`26 + i·(W−70)/(n−1)`); its dots and their labels.
pub const GUIDE_TOP: f32 = 18.0;
pub const GUIDE_FOOT: f32 = 26.0;
pub const GUIDE_X: f32 = 18.0;
pub const GUIDE_END: f32 = 10.0;
pub const DOT_X: f32 = 26.0;
pub const DOT_ROOM: f32 = 70.0;
pub const SLOPE_R: f32 = 4.5;
pub const SLOPE_LINE: f32 = 2.0;
pub const GUIDE_PX: f32 = 10.5;
pub const RANK_PX: f32 = 11.5;
pub const RANK_RISE: f32 = 9.0;
/// The guides' words: "top of the role" on its baseline 13 down the box,
/// "bottom" 12 up from its foot (`y=13`, `y=H-12`), both ending at the
/// guides' right end — and about how wide the top one runs back from it
/// at its type size (65 of the box's 300 at 10.5 px), and a rank's half
/// width ("17th" at 11.5 px): what tells a rank that would print over
/// them ([`rank_under`]).
pub const GUIDE_TOP_WORDS: f32 = 13.0;
pub const GUIDE_FOOT_WORDS: f32 = 12.0;
pub const GUIDE_WORDS_W: f32 = 66.0;
pub const RANK_HALF_W: f32 = 12.0;
/// A line of type's cap height, of its size: a rank under its dot hangs
/// from its top, where one above stands on its baseline.
pub const CAP_OF_TYPE: f32 = 0.7;
/// Under this many pixels between two dots, only the first, the last, the
/// highest and the lowest keep their rank above them: a crowded night's
/// labels would print over each other.
pub const LABEL_ROOM: f32 = 22.0;

/// The throughput chart's box (`viewBox="0 0 420 170"`): its plot's foot
/// and head room (`th − 22 − f·(th − 34)`), its gridlines' run and their
/// labels' right edge, its dots' first x and the room the last leaves.
pub const TREND_W: f32 = 420.0;
pub const TREND_H: f32 = 170.0;
pub const PLOT_FOOT: f32 = 22.0;
pub const PLOT_ROOM: f32 = 34.0;
pub const GRID_X: f32 = 40.0;
pub const GRID_END: f32 = 6.0;
pub const GRID_LABEL_X: f32 = 34.0;
pub const TREND_X: f32 = 56.0;
pub const TREND_ROOM: f32 = 80.0;
pub const TREND_R: f32 = 5.0;
pub const BEST_RING: f32 = 2.5;
pub const BEST_RISE: f32 = 10.0;
pub const AXIS_PX: f32 = 11.0;
pub const DAY_PX: f32 = 10.5;
pub const DAY_FOOT: f32 = 6.0;
/// A gridline's figure stands on its line: its baseline this far under it
/// (`y+4`).
pub const AXIS_DROP: f32 = 4.0;

/// How near a press must land to a dot to open its pull.
pub const HIT: f32 = 8.0;

/// A chart's words sit on their SVG baseline: this much of the type's size
/// above it is where the middle of a lower-case line stands.
pub const MID_OF_TYPE: f32 = 0.33;

/// A dot of a chart: where it stands, and the stored pull a press opens.
pub type Dot = ((f32, f32), String);

/// How much a chart's words are scaled at the box's scale `s`: with the
/// box, as an SVG's are, but never below their own size — a panel of a
/// three-column Home is narrower than the box, and 11 px axis figures at
/// 0.7 of it are under what reads.
pub fn type_scale(s: f32) -> f32 {
    s.max(1.0)
}

/// The pull a press at `p` lands on: the nearest dot within [`HIT`].
pub fn hit(dots: &[Dot], p: (f32, f32)) -> Option<String> {
    dots.iter()
        .map(|((x, y), id)| ((x - p.0).hypot(y - p.1), id))
        .filter(|(d, _)| *d <= HIT)
        .min_by(|a, b| a.0.total_cmp(&b.0))
        .map(|(_, id)| id.clone())
}

// ---- the night's rank --------------------------------------------------------

/// Where each pull's dot stands in a rank chart `w` wide. A night of one
/// pull stands it in the middle of the box: at the first dot's place it
/// would read as the start of a line that never came.
pub fn slope_dots(pulls: &[NightPull], w: f32) -> Vec<Dot> {
    let s = w / SLOPE_W;
    let n = pulls.len();
    let step = (SLOPE_W - DOT_ROOM) / n.saturating_sub(1).max(1) as f32;
    let first = if n == 1 { SLOPE_W / 2.0 } else { DOT_X };
    pulls
        .iter()
        .enumerate()
        .map(|(i, p)| {
            let f = p.standing.percentile();
            let y = GUIDE_TOP + (1.0 - f) * (SLOPE_H - GUIDE_TOP - GUIDE_FOOT);
            (((first + i as f32 * step) * s, y * s), p.fight_id.clone())
        })
        .collect()
}

/// Which dots keep their rank above them: all of them, while there is room
/// between two; else the first, the last, the highest and the lowest.
pub fn slope_labelled(pulls: &[NightPull], w: f32) -> Vec<bool> {
    let n = pulls.len();
    let gap = (SLOPE_W - DOT_ROOM) / n.saturating_sub(1).max(1) as f32 * w / SLOPE_W;
    if gap >= LABEL_ROOM {
        return vec![true; n];
    }
    let f = |i: usize| pulls.get(i).map_or(0.0, |p| p.standing.percentile());
    let high = (0..n).max_by(|a, b| f(*a).total_cmp(&f(*b)));
    let low = (0..n).min_by(|a, b| f(*a).total_cmp(&f(*b)));
    (0..n)
        .map(|i| i == 0 || i + 1 == n || Some(i) == high || Some(i) == low)
        .collect()
}

/// A rank printed above its dot at `at`, in a chart `w` wide, would print
/// over the top guide's words: its dot within a line of type of their
/// baseline, and within their reach from the guide's right end. It hangs
/// under its dot instead — the night's best pull is the one that tops the
/// role, and the one the words would otherwise break.
pub fn rank_under(at: (f32, f32), w: f32) -> bool {
    let s = w / SLOPE_W;
    let ts = type_scale(s);
    let baseline = at.1 - RANK_RISE * s;
    let near = baseline < GUIDE_TOP_WORDS * s + RANK_PX * ts;
    let words_left = (SLOPE_W - GUIDE_END) * s - GUIDE_WORDS_W * ts;
    near && at.0 + RANK_HALF_W * ts > words_left
}

// ---- the week's key throughput ----------------------------------------------

/// The throughput chart's value axis over its runs: its ends and the round
/// figures between them that get a gridline.
#[derive(Debug, Clone, PartialEq)]
pub struct TrendAxis {
    pub lo: f64,
    pub hi: f64,
    pub ticks: Vec<f64>,
}

impl TrendAxis {
    pub fn of(points: &[TrendPoint]) -> Self {
        let values: Vec<f64> = points.iter().map(|p| p.value).collect();
        let (lo, hi, ticks) = axis(&values);
        Self { lo, hi, ticks }
    }

    /// The height of value `v` in a chart at the box's scale `s`.
    pub fn y_of(&self, v: f64, s: f32) -> f32 {
        let f = if self.hi > self.lo {
            ((v - self.lo) / (self.hi - self.lo)) as f32
        } else {
            0.5
        };
        (TREND_H - PLOT_FOOT - f * (TREND_H - PLOT_ROOM)) * s
    }

    /// Where each run's dot stands in a chart `w` wide.
    pub fn dots(&self, points: &[TrendPoint], w: f32) -> Vec<Dot> {
        let s = w / TREND_W;
        let step = (TREND_W - TREND_ROOM) / points.len().saturating_sub(1).max(1) as f32;
        points
            .iter()
            .enumerate()
            .map(|(i, p)| {
                (
                    ((TREND_X + i as f32 * step) * s, self.y_of(p.value, s)),
                    p.fight_id.clone(),
                )
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::home::{Standing, TrendPoint};
    use crate::rail::Mark;

    fn pull(place: usize) -> NightPull {
        NightPull {
            fight_id: format!("p{place}"),
            name: "Boss".to_string(),
            mark: Mark::Good,
            wipe_pct: None,
            standing: Standing {
                place,
                of: 19,
                value: 1.0,
            },
        }
    }

    /// A night of three spans the box; one pull stands in its middle; a
    /// crowded night keeps the first, the lowest, the highest and the last
    /// ranks; the night's best at the right end hangs its rank under it.
    #[test]
    fn the_rank_chart_places_its_dots_and_their_ranks() {
        let few = [pull(17), pull(10), pull(1)];
        let dots = slope_dots(&few, SLOPE_W);
        assert_eq!(dots[0].0.0, DOT_X);
        assert_eq!(dots[2].0.0, SLOPE_W - DOT_ROOM + DOT_X);
        assert_eq!(dots[2].0.1, GUIDE_TOP, "the top of the role at the top");
        assert_eq!(slope_dots(&[pull(5)], SLOPE_W)[0].0.0, SLOPE_W / 2.0);
        assert_eq!(slope_labelled(&few, 300.0), [true, true, true]);
        let crowd: Vec<NightPull> = [9, 12, 3, 15, 19, 8, 11, 14, 2, 10, 7, 13, 16, 5]
            .into_iter()
            .map(pull)
            .collect();
        let kept: Vec<usize> = slope_labelled(&crowd, 300.0)
            .into_iter()
            .enumerate()
            .filter_map(|(i, k)| k.then_some(i))
            .collect();
        assert_eq!(kept, [0, 4, 8, 13]);
        let topped = slope_dots(&[pull(17), pull(1)], 300.0);
        assert!(!rank_under(topped[0].0, 300.0));
        assert!(rank_under(topped[1].0, 300.0));
        assert_eq!(hit(&topped, topped[1].0).as_deref(), Some("p1"));
        assert_eq!(hit(&topped, (0.0, 0.0)), None, "nowhere near a dot");
    }

    /// The throughput chart stands its runs on a round axis.
    #[test]
    fn the_throughput_chart_stands_its_runs_on_its_axis() {
        let point = |v: f64| TrendPoint {
            fight_id: String::new(),
            who: crate::home::Char::default(),
            day: 0,
            value: v,
            best: false,
        };
        let points = [point(142_613.0), point(292_570.0)];
        let axis = TrendAxis::of(&points);
        assert_eq!(axis.ticks, [150_000.0, 200_000.0, 250_000.0, 300_000.0]);
        let dots = axis.dots(&points, TREND_W);
        assert_eq!(dots[0].0.0, TREND_X);
        assert!(dots[1].0.1 < dots[0].0.1, "the higher run stands higher");
        let flat = TrendAxis {
            lo: 1.0,
            hi: 1.0,
            ticks: Vec::new(),
        };
        assert_eq!(
            flat.y_of(1.0, 1.0),
            TREND_H - PLOT_FOOT - 0.5 * (TREND_H - PLOT_ROOM)
        );
    }
}
