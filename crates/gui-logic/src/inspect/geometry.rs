//! The inspector graph's geometry (the prototype's `.igraph`), free of any
//! GUI: where every instant, value, span, death and word of the window's
//! drill graph stands, in canvas units at zoom 1 — moved from the iced
//! window's `inspector/plot.rs`, so both window GUIs lay the graph out the
//! same. A renderer brings only its text measure ([`Measure`]) and its
//! paint.
//!
//! With lanes the plot starts where their tracks do, so one x is one
//! instant in the curve, the axis and every lane; the gutter that leaves
//! beside the plot is its scale. Without lanes the plot runs the canvas's
//! whole width. A death's words stand in a band over the curves, which are
//! scaled to peak under it ([`Plot::top_of`]).

use wowdps_model::fmt::duration;

use super::lanes::{self, Row as LaneRow, Span};
use super::plot::{Curve, Dead, Ink, value_words};
use crate::axis::ticks;
use crate::table::figure;
use crate::theme::Color;

/// The plot's height (`.iplot{height:96px}`) — beside the meter; a widened
/// inspector draws a taller one ([`Plot::plot_h`]).
pub const PLOT_H: f32 = 96.0;
/// The axis under it (`.iaxis{height:16px;margin-top:2px}`).
pub const AXIS_GAP: f32 = 2.0;
pub const AXIS_H: f32 = 16.0;
/// The ticks' words stand this far into the axis's line.
pub const AXIS_WORDS_Y: f32 = 1.0;
/// The lanes (`.lanes{margin-top:6px;row-gap:4px}`, `.lane{height:13px}`)
/// and a span inside its lane (`.span{top:2px;bottom:2px}`). A grid row is
/// as tall as its tallest cell — the label's line (`.lane-l{font-size:
/// 12px}` at the default 1.3), 15.6 px — with the 13 px track centred in
/// it (`align-self:center`).
pub const LANES_TOP: f32 = 6.0;
pub const LANE_H: f32 = 13.0;
pub const LANE_ROW: f32 = if LANE_PX * LINE > LANE_H {
    LANE_PX * LINE
} else {
    LANE_H
};
pub const LANE_GAP: f32 = 4.0;
pub const SPAN_INSET: f32 = 2.0;
/// A comparison's track holds two halves: the first player's spans over the
/// second's, each this far from the track's edge and this far apart.
pub const SPLIT_INSET: f32 = 1.5;
pub const SPLIT_GAP: f32 = 1.0;
/// Corners: a span's, a moment's tick (`.span.tick{border-radius:1px}`)
/// and the track's (`.lane{border-radius:2px}`).
pub const SPAN_RADIUS: f32 = 2.0;
pub const TICK_RADIUS: f32 = 1.0;
pub const TRACK_RADIUS: f32 = 2.0;
/// A span's outline in the panel's own surface, so two that overlap in one
/// lane (Power Infusion inside a Heroism) read as two; a hovered one's in
/// white (`.span:hover{outline:1px solid #fff}`).
pub const SPAN_EDGE: f32 = 1.0;
/// How far either side of a thin span the pointer still finds it.
pub const SPAN_SLOP: f32 = 2.0;
/// The narrowest a span is drawn, and a moment's tick (`.span{min-width:
/// 3px}`, `.span.tick{width:3px}`).
pub const SPAN_MIN: f32 = 3.0;
/// The lane labels' column and the gap after it (`.lanes{grid-template-
/// columns:74px …;column-gap:8px}`): where every track — and, with lanes,
/// the plot — starts.
pub const LABEL_W: f32 = 74.0;
pub const LABEL_GAP: f32 = 8.0;
pub const LEFT: f32 = LABEL_W + LABEL_GAP;
/// Axis ticks (`.iaxis span{font-size:11px}`), lane labels (`.lane-l{font-
/// size:12px}`), a hatch's words (`.hatch b{11px 600}`) and the tooltip's
/// (`.tip{font-size:13px;padding:5px 8px;radius:6px}`).
pub const TICK_PX: f32 = 11.0;
pub const LANE_PX: f32 = 12.0;
pub const HATCH_PX: f32 = 11.0;
pub const TIP_PX: f32 = 13.0;
pub const TIP_PAD: (f32, f32) = (8.0, 5.0);
pub const TIP_LINE: f32 = 17.0;
pub const TIP_RADIUS: f32 = 6.0;
/// R26: a stacked band's swatch before its line in the tooltip, and the
/// gap after it.
pub const SWATCH: f32 = 9.0;
pub const SWATCH_GAP: f32 = 6.0;
pub const SWATCH_RADIUS: f32 = 2.0;
/// R26: the panel's gap between two stacked bands.
pub const STACK_GAP: f32 = 2.0;
/// The tooltip's distance from the crosshair, and above a span, and how
/// far down the plot it hangs beside the crosshair.
pub const TIP_OFF_X: f32 = 12.0;
pub const TIP_OFF_Y: f32 = 6.0;
pub const TIP_PLOT_Y: f32 = 14.0;
/// A tick at the axis's very start stands from it (`.iaxis span:first-child
/// {transform:none}`), one past 96 % of it back from it (`x>96 ?
/// translateX(-100%)`); the rest are centred on their minute.
pub const TICK_FIRST: f32 = 0.001;
pub const TICK_LAST: f32 = 0.96;
/// A press-release wander below this is a click, not a zoom window.
pub const DRAG_MIN_PX: f32 = 3.0;
/// The hatch's stripes (`repeating-linear-gradient(135deg, … 0 4px,
/// transparent 4px 8px)`): 4 px of ink and 4 px of air measured ACROSS the
/// 45° stripes — 8√2 px apart along the row, a stroke 4 px wide.
pub const STRIPE_STEP: f32 = 8.0 * std::f32::consts::SQRT_2;
pub const STRIPE_W: f32 = 4.0;
/// The hatch's ink (`rgba(255,92,99,.16)`) and its dashed edge
/// (`border-left:1.5px dashed`), and the patch its words sit on.
pub const HATCH_ALPHA: f32 = 0.16;
pub const HATCH_EDGE: f32 = 1.5;
pub const HATCH_DASH: [f32; 2] = [3.0, 3.0];
pub const HATCH_PATCH_ALPHA: f32 = 0.85;
/// The patch reaches this far past the words on every side, its corners
/// rounded this much.
pub const HATCH_PATCH_PAD: f32 = 3.0;
pub const HATCH_PATCH_RADIUS: f32 = 3.0;
/// A hatch's words stand this far from its edge (`.hatch b{left:5px}`) and
/// this far under the plot's top; a second row of them — two deaths whose
/// words would overlap — one line and this much more under the first.
pub const HATCH_WORDS_X: f32 = 5.0;
pub const HATCH_WORDS_Y: f32 = 1.0;
pub const HATCH_ROW_GAP: f32 = 1.0;
/// The air between the words' band and the highest a curve is drawn.
pub const HATCH_BAND_GAP: f32 = 2.0;
/// The scale in the lanes' gutter (`.iaxis span{font-size:11px}`): the
/// peak at the plot's top, 0 at its baseline, right-aligned to the gutter's
/// label column so they end [`LABEL_GAP`] before the plot.
pub const SCALE_PX: f32 = 11.0;
/// A span at rest and lit (`.span{opacity:.85}`, `:hover{opacity:1}`), a
/// curve's area (`fill-opacity=".14"`), a ghost's line.
pub const SPAN_ALPHA: f32 = 0.85;
pub const AREA_ALPHA: f32 = 0.14;
pub const GHOST_ALPHA: f32 = 0.30;
/// A curve's stroke (`stroke-width="1.6"`), a ghost's, and the crosshair's
/// gold (`.xhair{background:rgba(242,193,75,.55)}`).
pub const CURVE_W: f32 = 1.6;
pub const GHOST_W: f32 = 1.0;
/// The baseline, the crosshair and the tooltip's frame: a 1 px rule.
pub const HAIRLINE: f32 = 1.0;
pub const XHAIR_ALPHA: f32 = 0.55;
/// The peak stands this far under the plot's top (`H - v/max*(H-4)`).
pub const PEAK_INSET: f32 = 4.0;
/// A one-line text box's height for its size: the default line height.
pub const LINE: f32 = 1.3;
/// A second curve is dashed (`stroke-dasharray: 5 4`), so the two stay
/// apart by more than their hue.
pub const DASH: [f32; 2] = [5.0, 4.0];

/// A box in canvas units: origin at the top-left, x right, y down.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl Rect {
    pub fn new(x: f32, y: f32, w: f32, h: f32) -> Self {
        Self { x, y, w, h }
    }

    pub fn center_y(&self) -> f32 {
        self.y + self.h / 2.0
    }

    /// Do two boxes overlap (edges touching do not)?
    pub fn overlaps(&self, b: &Rect) -> bool {
        self.x < b.x + b.w && b.x < self.x + self.w && self.y < b.y + b.h && b.y < self.y + self.h
    }
}

/// What the pointer is over.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Hover {
    /// A span: its lane and its place on it.
    Span(usize, usize),
    /// The plot, at this x (canvas units), snapped to a bucket.
    Plot(f32),
}

/// The two faces the graph's words wear: the window's regular and its
/// semibold.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Face {
    Regular,
    Semibold,
}

/// What ink a word wears, by role: the window's readable faint ink
/// (`INK_3_TEXT`: ticks, the scale, lane labels), its bad (a death's
/// words), its first ink (`INK`) and its second (`INK_2`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ink3 {
    Quiet,
    Bad,
    Ink,
    Ink2,
}

/// A text measure: `words`' one-line width at `px` in `face`, as the
/// renderer shapes it (canvas units at zoom 1).
pub type Measure<'a> = &'a dyn Fn(&str, f32, Face) -> f32;

/// One line of canvas text, placed and measured: what the plot says
/// besides the tooltip, so the tooltip can keep it out from under itself.
#[derive(Debug, Clone, PartialEq)]
pub struct Label {
    pub words: String,
    pub x: f32,
    pub y: f32,
    pub px: f32,
    pub ink: Ink3,
    pub face: Face,
    pub width: f32,
}

impl Label {
    /// The box the words stand in.
    pub fn rect(&self) -> Rect {
        Rect::new(self.x, self.y, self.width, self.px * LINE)
    }
}

/// The tooltip, placed: its box and its lines.
#[derive(Debug, Clone, PartialEq)]
pub struct Tip {
    pub rect: Rect,
    pub lines: Vec<(String, Ink3, Face)>,
    /// R26: a swatch before each line that names a stacked band — the
    /// words stay in ink, the band's colour beside them.
    pub swatches: Vec<Option<Color>>,
}

impl Tip {
    /// How far in the words stand from the box's padding: past a swatch
    /// when any line has one.
    pub fn inset(&self) -> f32 {
        if self.swatches.iter().any(Option::is_some) {
            SWATCH + SWATCH_GAP
        } else {
            0.0
        }
    }
}

/// One stacked band (R26), as points: its top, and — every band but the
/// first — the top of the band under it, which is its bottom.
#[derive(Debug, Clone, PartialEq)]
pub struct Band {
    pub color: Color,
    pub upper: Vec<(f32, f32)>,
    pub lower: Vec<(f32, f32)>,
    /// The rest ("Other"): its upper edge is the whole curve.
    pub rest: bool,
}

/// Everything the graph draws, borrowed: the window on show, its one
/// scale, the curves, the deaths, the lanes, and how the hover reads.
#[derive(Debug, Clone, Copy)]
pub struct Plot<'a> {
    /// The stretch of the fight on show, ms: the whole of it, or a zoom.
    pub window: (u32, u32),
    /// The one scale every curve is drawn on.
    pub peak: f64,
    pub curves: &'a [Curve],
    pub dead: &'a [Dead],
    pub lanes: &'a [LaneRow],
    /// The hover reads the curve as a running total ("63.0M") rather than
    /// a rate ("149,258").
    pub total: bool,
    /// What the hover calls a lone curve's value ("dps").
    pub word: &'a str,
    /// The plot's own height: [`PLOT_H`] beside the meter, taller in an
    /// inspector widened over the stage.
    pub plot_h: f32,
}

impl Plot<'_> {
    /// The whole canvas: the plot, its axis and the lanes.
    pub fn height(&self) -> f32 {
        self.plot_h + AXIS_GAP + AXIS_H + self.lanes_h()
    }

    pub fn lanes_h(&self) -> f32 {
        let n = self.lanes.len() as f32;
        if n == 0.0 {
            0.0
        } else {
            LANES_TOP + n * LANE_ROW + (n - 1.0) * LANE_GAP
        }
    }

    /// Is this a comparison's graph, each lane split between the two?
    pub fn split(&self) -> bool {
        self.lanes
            .iter()
            .flat_map(|r| r.spans.iter())
            .any(|s| s.whose.is_some())
    }

    /// Where the plot and its axis start: at the lanes' tracks when there
    /// are lanes, at the canvas's edge when there are none.
    pub fn left(&self) -> f32 {
        if self.lanes.is_empty() { 0.0 } else { LEFT }
    }

    pub fn span_ms(&self) -> f64 {
        f64::from(self.window.1.saturating_sub(self.window.0).max(1))
    }

    /// The canvas x of `ms` from the fight's start, on a canvas `w` wide.
    pub fn x_of(&self, ms: f64, w: f32) -> f32 {
        let left = self.left();
        left + ((ms - f64::from(self.window.0)) / self.span_ms()) as f32 * (w - left).max(1.0)
    }

    /// The ms from the fight's start a canvas x lands on, held inside the
    /// window.
    pub fn ms_at(&self, x: f32, w: f32) -> u32 {
        let left = self.left();
        let frac = ((x - left) / (w - left).max(1.0)).clamp(0.0, 1.0) as f64;
        (f64::from(self.window.0) + frac * self.span_ms()).max(0.0) as u32
    }

    /// The plot's y for a value: the peak `top` under the plot's top (a
    /// few px, or under the hatches' words — [`Plot::top_of`]).
    pub fn y_of(&self, v: f64, top: f32) -> f32 {
        if self.peak <= 0.0 {
            return self.plot_h;
        }
        self.plot_h - (v / self.peak).clamp(0.0, 1.0) as f32 * (self.plot_h - top)
    }

    /// Where the peak is drawn, under the hatches' words `hatches` when
    /// there are any — so a curve never runs through them.
    pub fn top_of(hatches: &[Label]) -> f32 {
        hatches
            .iter()
            .map(|l| l.y + l.px * LINE + HATCH_BAND_GAP)
            .fold(PEAK_INSET, f32::max)
    }

    /// The top of lane `lane`'s row (its label's line).
    pub fn lane_y(&self, lane: usize) -> f32 {
        self.plot_h + AXIS_GAP + AXIS_H + LANES_TOP + lane as f32 * (LANE_ROW + LANE_GAP)
    }

    /// The top of lane `lane`'s track, centred in its row.
    pub fn track_y(&self, lane: usize) -> f32 {
        self.lane_y(lane) + (LANE_ROW - LANE_H) / 2.0
    }

    /// A span's top and height in its track: the whole track's less the
    /// inset, or — a comparison's — the first player's half over the
    /// second's.
    pub fn span_band(&self, lane: usize, s: &Span) -> (f32, f32) {
        let y = self.track_y(lane);
        if !self.split() {
            return (y + SPAN_INSET, LANE_H - 2.0 * SPAN_INSET);
        }
        let half = (LANE_H - 2.0 * SPLIT_INSET - SPLIT_GAP) / 2.0;
        let top = y + SPLIT_INSET;
        if s.second {
            (top + half + SPLIT_GAP, half)
        } else {
            (top, half)
        }
    }

    /// Where a span is drawn, held to the plot's stretch: a span that began
    /// before a zoom's window starts at the track's edge and one that runs
    /// past it stops at the canvas's; one wholly outside is not drawn. A
    /// span that starts inside is at least [`SPAN_MIN`] wide, a moment
    /// exactly that.
    pub fn span_rect(&self, lane: usize, s: &Span, w: f32) -> Option<Rect> {
        let left = self.left();
        let x1 = self.x_of(s.at_ms as f64, w);
        let (x, width) = if s.dur_ms <= 0 {
            if x1 < left || x1 > w {
                return None;
            }
            (x1.min(w - SPAN_MIN).max(left), SPAN_MIN)
        } else {
            let x2 = self.x_of((s.at_ms + s.dur_ms) as f64, w);
            let (a, b) = (x1.max(left), x2.min(w));
            if b <= left || a >= w {
                return None;
            }
            if x1 >= left {
                let width = (b - a).max(SPAN_MIN);
                (a.min(w - width).max(left), width)
            } else {
                (a, b - a)
            }
        };
        let (y, height) = self.span_band(lane, s);
        Some(Rect::new(x, y, width, height))
    }

    /// The span under `(x, y)`, the topmost (latest drawn) where they
    /// overlap; a thin one answers a couple of pixels either side of
    /// itself. On a comparison's split track the pointer's half says whose.
    pub fn span_at(&self, x: f32, y: f32, w: f32) -> Option<(usize, usize)> {
        let lane = (0..self.lanes.len()).find(|&i| {
            let t = self.track_y(i);
            y >= t && y < t + LANE_H
        })?;
        let row = self.lanes.get(lane)?;
        let second = y >= self.track_y(lane) + LANE_H / 2.0;
        let split = self.split();
        row.spans
            .iter()
            .enumerate()
            .rev()
            .filter(|(_, s)| !split || s.second == second)
            .find(|(_, s)| {
                self.span_rect(lane, s, w)
                    .is_some_and(|r| x >= r.x - SPAN_SLOP && x <= r.x + r.w + SPAN_SLOP)
            })
            .map(|(i, _)| (lane, i))
    }

    /// Is `(x, y)` on the plot itself — where a press starts a zoom?
    pub fn in_plot(&self, x: f32, y: f32) -> bool {
        y <= self.plot_h && x >= self.left()
    }

    /// What the pointer at `(x, y)` is over.
    pub fn hover_at(&self, x: f32, y: f32, w: f32) -> Option<Hover> {
        if let Some((lane, i)) = self.span_at(x, y, w) {
            return Some(Hover::Span(lane, i));
        }
        if y < 0.0 || y > self.plot_h || x < self.left() || x > w {
            return None;
        }
        // Snapped to the first curve's bucket, so the readout and the
        // crosshair move together.
        let bucket = self.curves.first().map_or(1000, |c| c.bucket_ms.max(1));
        let ms = self.ms_at(x, w);
        let snapped = (ms / bucket) * bucket + bucket / 2;
        Some(Hover::Plot(self.x_of(f64::from(snapped), w)))
    }

    /// The ms window a drag from `a` to `b` selects, or `None` for a click
    /// (a wander under [`DRAG_MIN_PX`]).
    pub fn drag_range(&self, a: f32, b: f32, w: f32) -> Option<(u32, u32)> {
        if (b - a).abs() < DRAG_MIN_PX {
            return None;
        }
        Some((self.ms_at(a.min(b), w), self.ms_at(a.max(b), w)))
    }

    /// The words the tooltip for `hover` shows, first line first.
    pub fn tip_words(&self, hover: Hover, w: f32) -> Vec<(String, Ink3, Face)> {
        match hover {
            Hover::Span(lane, i) => {
                let Some(s) = self.lanes.get(lane).and_then(|r| r.spans.get(i)) else {
                    return Vec::new();
                };
                let (name, details) = lanes::span_words(s);
                vec![
                    (name, Ink3::Ink, Face::Semibold),
                    (details, Ink3::Ink2, Face::Regular),
                ]
            }
            Hover::Plot(x) => {
                let ms = self.ms_at(x, w);
                let mut lines = vec![(duration(i64::from(ms)), Ink3::Ink, Face::Semibold)];
                for c in self.curves.iter().filter(|c| c.ink != Ink::Ghost) {
                    let v = c
                        .at(ms)
                        .map_or_else(|| "—".to_string(), |v| value_words(v, self.total));
                    let who = if c.name.is_empty() {
                        self.word.to_string()
                    } else {
                        c.name.clone()
                    };
                    lines.push((format!("{who}  {v}"), Ink3::Ink, Face::Regular));
                }
                lines
            }
        }
    }

    /// The tooltip for `hover` (`.tip`), placed: beside the crosshair on
    /// the plot (flipped left at the edge), above a span on a lane so it
    /// lands on the plot, never under the pointer — held inside the canvas.
    pub fn tip(&self, hover: Hover, w: f32, measure: Measure) -> Option<Tip> {
        let lines = self.tip_words(hover, w);
        if lines.is_empty() {
            return None;
        }
        // R26: a stacked band's line wears its colour in a swatch; the
        // time on the first line and a plain curve's line wear none.
        let swatches: Vec<Option<Color>> = match hover {
            Hover::Plot(_) => std::iter::once(None)
                .chain(
                    self.curves
                        .iter()
                        .filter(|c| c.ink != Ink::Ghost)
                        .map(|c| c.ink.is_stack().then_some(c.color)),
                )
                .collect(),
            Hover::Span(..) => Vec::new(),
        };
        let inset = if swatches.iter().any(Option::is_some) {
            SWATCH + SWATCH_GAP
        } else {
            0.0
        };
        let tw = lines
            .iter()
            .map(|(s, _, f)| measure(s, TIP_PX, *f))
            .fold(0.0_f32, f32::max)
            + inset
            + 2.0 * TIP_PAD.0;
        let th = lines.len() as f32 * TIP_LINE + 2.0 * TIP_PAD.1;
        let (mut x, mut y) = match hover {
            Hover::Plot(x) => {
                let right = x + TIP_OFF_X;
                if right + tw > w {
                    (x - TIP_OFF_X - tw, TIP_PLOT_Y)
                } else {
                    (right, TIP_PLOT_Y)
                }
            }
            Hover::Span(l, i) => {
                let r = self
                    .lanes
                    .get(l)
                    .and_then(|row| row.spans.get(i))
                    .and_then(|s| self.span_rect(l, s, w))?;
                (r.x + r.w / 2.0 - tw / 2.0, r.y - th - TIP_OFF_Y)
            }
        };
        x = x.clamp(0.0, (w - tw).max(0.0));
        y = y.clamp(0.0, (self.height() - th).max(0.0));
        Some(Tip {
            rect: Rect::new(x, y, tw, th),
            lines,
            swatches,
        })
    }

    /// The hatches' words (`.hatch b`): after the dashed edge, as the
    /// prototype sets them — or before it, when a death late in the fight
    /// would run its words off the plot. Words that would overlap words
    /// already placed (a pair who both died, a player who died twice)
    /// drop to a second row; with no room there either, they are left
    /// out rather than printed through the others.
    pub fn hatch_labels(&self, w: f32, measure: Measure) -> Vec<Label> {
        let left = self.left();
        let line = HATCH_PX * LINE;
        let mut placed: Vec<Label> = Vec::new();
        for d in self.dead {
            let x1 = self.x_of(d.at_ms as f64, w).clamp(left, w);
            let x2 = self.x_of(d.end_ms as f64, w).clamp(left, w);
            if x2 <= x1 {
                continue;
            }
            let tw = measure(&d.words, HATCH_PX, Face::Semibold);
            let x = if x1 + HATCH_WORDS_X + tw > w {
                (x1 - HATCH_WORDS_X - tw).max(left)
            } else {
                x1 + HATCH_WORDS_X
            };
            let rows = [HATCH_WORDS_Y, HATCH_WORDS_Y + line + HATCH_ROW_GAP];
            let free = rows.into_iter().find(|&y| {
                let rect = Rect::new(x, y, tw, line);
                placed.iter().all(|l| !l.rect().overlaps(&rect))
            });
            if let Some(y) = free {
                placed.push(Label {
                    words: d.words.clone(),
                    x,
                    y,
                    px: HATCH_PX,
                    ink: Ink3::Bad,
                    face: Face::Semibold,
                    width: tw,
                });
            }
        }
        placed
    }

    /// The scale in the gutter beside the plot, when it has one (it has
    /// lanes): the peak level with where the peak is drawn, `top`, and 0
    /// on the baseline, each ending [`LABEL_GAP`] before the plot.
    pub fn scale_labels(&self, top: f32, measure: Measure) -> Vec<Label> {
        if self.lanes.is_empty() || self.peak <= 0.0 {
            return Vec::new();
        }
        let line = SCALE_PX * LINE;
        [
            (
                figure(self.peak.round() as u64),
                (top - line / 2.0).max(0.0),
            ),
            ("0".to_string(), self.plot_h - line),
        ]
        .into_iter()
        .map(|(words, y)| {
            let width = measure(&words, SCALE_PX, Face::Regular);
            Label {
                x: LABEL_W - width,
                y,
                words,
                px: SCALE_PX,
                ink: Ink3::Quiet,
                face: Face::Regular,
                width,
            }
        })
        .collect()
    }

    /// Everything the plot says besides a tooltip: the hatches' words, the
    /// scale, the axis's ticks (the first from its left edge, one near the
    /// end back from it) and the lanes' labels — less whatever the tooltip
    /// for `hover` would cover, so no label prints through it.
    pub fn labels(&self, hover: Option<Hover>, w: f32, measure: Measure) -> Vec<Label> {
        let left = self.left();
        let plot_w = (w - left).max(1.0);
        let mut out = self.hatch_labels(w, measure);
        out.extend(self.scale_labels(Self::top_of(&out), measure));
        let axis_y = self.plot_h + AXIS_GAP + AXIS_WORDS_Y;
        for t in ticks(self.window, plot_w) {
            let x = self.x_of(f64::from(t), w);
            let words = duration(i64::from(t));
            let tw = measure(&words, TICK_PX, Face::Regular);
            let frac = (x - left) / plot_w;
            let at = if frac <= TICK_FIRST {
                x
            } else if frac > TICK_LAST {
                x - tw
            } else {
                x - tw / 2.0
            };
            out.push(Label {
                words,
                x: at,
                y: axis_y,
                px: TICK_PX,
                ink: Ink3::Quiet,
                face: Face::Regular,
                width: tw,
            });
        }
        for (li, row) in self.lanes.iter().enumerate() {
            let y = self.lane_y(li);
            let words = row.lane.label().to_string();
            let width = measure(&words, LANE_PX, Face::Regular);
            out.push(Label {
                words,
                x: 0.0,
                y: y + (LANE_ROW - LANE_PX * LINE) / 2.0,
                px: LANE_PX,
                ink: Ink3::Quiet,
                face: Face::Regular,
                width,
            });
        }
        match hover.and_then(|h| self.tip(h, w, measure)) {
            Some(tip) => out
                .into_iter()
                .filter(|l| !l.rect().overlaps(&tip.rect))
                .collect(),
            None => out,
        }
    }

    /// The curve's points inside the window, as canvas points — with one
    /// at the window's start holding the first value, as the prototype's
    /// `curve()` pads it, so the line meets the plot's edge.
    pub fn points(&self, c: &Curve, w: f32, top: f32) -> Vec<(f32, f32)> {
        self.points_of(&c.points, c.bucket_ms, w, top)
    }

    /// [`Plot::points`] for `values` on a grid of `bucket_ms`.
    pub fn points_of(&self, values: &[f64], bucket_ms: u32, w: f32, top: f32) -> Vec<(f32, f32)> {
        let b = f64::from(bucket_ms.max(1));
        let (lo, hi) = (f64::from(self.window.0), f64::from(self.window.1));
        let mut pts: Vec<(f64, f64)> = values
            .iter()
            .enumerate()
            .map(|(i, v)| ((i as f64 + 0.5) * b, *v))
            .filter(|(ms, _)| *ms >= lo - b && *ms <= hi + b)
            .map(|(ms, v)| (ms.clamp(lo, hi), v))
            .collect();
        if let Some(&(first, v)) = pts.first()
            && first > lo
        {
            pts.insert(0, (lo, v));
        }
        // And one at its end holding the last, when the curve runs that far
        // (the prototype pads a curve whose buckets reach the fight's end).
        let reach = values.len() as f64 * b;
        if let Some(&(last, v)) = pts.last()
            && last < hi
            && reach >= hi - b / 2.0
        {
            pts.push((hi, v));
        }
        pts.into_iter()
            .map(|(ms, v)| (self.x_of(ms, w), self.y_of(v, top)))
            .collect()
    }

    /// R26: the stack — each band from the running sum of those before it
    /// up to its own top, in list order (bottom first). A band with no
    /// points in the window is left out, its values still under the next.
    pub fn bands(&self, w: f32, top: f32) -> Vec<Band> {
        let mut out = Vec::new();
        let mut base: Vec<f64> = Vec::new();
        for c in self.curves.iter().filter(|c| c.ink.is_stack()) {
            let mut sum = base.clone();
            if sum.len() < c.points.len() {
                sum.resize(c.points.len(), 0.0);
            }
            for (s, v) in sum.iter_mut().zip(&c.points) {
                *s += v.max(0.0);
            }
            let upper = self.points_of(&sum, c.bucket_ms, w, top);
            let lower = if base.is_empty() {
                Vec::new()
            } else {
                let mut b = base.clone();
                b.resize(sum.len(), 0.0);
                self.points_of(&b, c.bucket_ms, w, top)
            };
            if !upper.is_empty() {
                out.push(Band {
                    color: c.color,
                    upper,
                    lower,
                    rest: c.ink == Ink::StackRest,
                });
            }
            base = sum;
        }
        out
    }

    /// The curves a line is drawn for — every one but the stack's bands —
    /// ghosts first, so a focus reads on top of its context.
    pub fn lines(&self) -> Vec<&Curve> {
        let mut order: Vec<&Curve> = self.curves.iter().filter(|c| !c.ink.is_stack()).collect();
        order.sort_by_key(|c| c.ink != Ink::Ghost);
        order
    }
}

/// `pts` as the prototype's `smooth()` traces them: a Catmull-Rom spline
/// as cubic Béziers, each control point a sixth of the neighbours' span
/// along — held inside the plot, so a steep step cannot swing the line
/// under its baseline or over its top. One `[c1, c2, to]` per segment,
/// after a move to the first point.
pub fn smooth(pts: &[(f32, f32)], plot_h: f32) -> Vec<[(f32, f32); 3]> {
    let hold = |(x, y): (f32, f32)| (x, y.clamp(0.0, plot_h));
    let at = |j: usize| {
        pts.get(j)
            .or_else(|| pts.last())
            .copied()
            .unwrap_or((0.0, 0.0))
    };
    (0..pts.len().saturating_sub(1))
        .map(|i| {
            let (p0, p1, p2, p3) = (at(i.saturating_sub(1)), at(i), at(i + 1), at(i + 2));
            let c1 = hold((p1.0 + (p2.0 - p0.0) / 6.0, p1.1 + (p2.1 - p0.1) / 6.0));
            let c2 = hold((p2.0 - (p3.0 - p1.0) / 6.0, p2.1 - (p3.1 - p1.1) / 6.0));
            [c1, c2, p2]
        })
        .collect()
}

/// A death's "/" stripes between `x1` and `x2`, each cut to the span: from
/// `(s, bottom)` up to `(s + H, top)`, kept where it is between the two.
pub fn stripes(x1: f32, x2: f32, plot_h: f32) -> Vec<((f32, f32), (f32, f32))> {
    let mut out = Vec::new();
    let mut s = x1 - plot_h;
    while s < x2 {
        let t0 = ((x1 - s) / plot_h).clamp(0.0, 1.0);
        let t1 = ((x2 - s) / plot_h).clamp(0.0, 1.0);
        if t1 > t0 {
            let at = |t: f32| (s + t * plot_h, plot_h - t * plot_h);
            out.push((at(t0), at(t1)));
        }
        s += STRIPE_STEP;
    }
    out
}

/// The graph's states, for the shots and tests of every crate that draws it.
#[doc(hidden)]
pub mod samples;

#[cfg(test)]
mod tests;
