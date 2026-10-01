//! The inspector's graph (the prototype's `.igraph`): one plot with the
//! curve to itself — an area in the player's class colour, or two lines on
//! one scale for a comparison — a time axis in minutes shared with the
//! fight, a death hatched from the moment it happened to the rez, and the
//! lanes of spans under it ([`super::lanes`]) — a comparison's lanes split
//! in two, the first player's spans over the second's. With lanes the plot
//! starts where their tracks do, so one x is one instant in the curve, the
//! axis and every lane: a cooldown sits under the bump it caused. The
//! gutter that leaves beside the plot is its scale — the peak at the top,
//! 0 at the baseline, as a chart's y axis — and the graph's top line then
//! names the measure alone. Without lanes (an enemy) the plot runs the
//! graph's whole width, as the prototype's `.iplot` does, and the top line
//! says the peak. The curves are drawn through a Catmull-Rom spline (the
//! prototype's `smooth()`), so a rate reads as hills rather than a saw.
//! A death's words stand in a band over the curves, which are scaled to
//! peak under it, so no curve runs through them.
//!
//! One canvas, so a span's tooltip floats over the plot above its lane.
//! iced draws a canvas's text above all of its shapes, whatever order they
//! were made in: every label a tooltip covers is left out rather than
//! printed through it ([`Plot::labels`]).
//!
//! Hover is the canvas's own — a crosshair and the values under it, a
//! span's name, time, length and caster — and never a message; a drag
//! selects a zoom window and a right-click resets it, the drill's zoom as
//! the old graph had it.
//!
//! Window-only. The overlay's graph is `compare::drill_graph`, untouched.

use iced::widget::canvas::{self, Canvas, LineDash, Path, Stroke};
use iced::{Color, Element, Font, Length, Point, Rectangle, Renderer, Size, Theme, mouse};

use super::lanes::{self, Row as LaneRow};
// The graph's data is gui-logic's (`inspect::plot`); this canvas draws it.
use crate::table::figure;
use crate::theme;
pub(crate) use wowdps_gui_logic::inspect::plot::{Curve, Dead, Ink, value_words};

/// The plot's height (`.iplot{height:96px}`).
pub(crate) const PLOT_H: f32 = 96.0;
/// The axis under it (`.iaxis{height:16px;margin-top:2px}`).
const AXIS_GAP: f32 = 2.0;
const AXIS_H: f32 = 16.0;
/// The ticks' words stand this far into the axis's line.
const AXIS_WORDS_Y: f32 = 1.0;
/// The lanes (`.lanes{margin-top:6px;row-gap:4px}`, `.lane{height:13px}`)
/// and a span inside its lane (`.span{top:2px;bottom:2px}`). A grid row is
/// as tall as its tallest cell — the label's line (`.lane-l{font-size:
/// 12px}` at the default 1.3), 15.6 px — with the 13 px track centred in
/// it (`align-self:center`).
const LANES_TOP: f32 = 6.0;
const LANE_H: f32 = 13.0;
const LANE_ROW: f32 = if LANE_PX * LINE > LANE_H {
    LANE_PX * LINE
} else {
    LANE_H
};
const LANE_GAP: f32 = 4.0;
const SPAN_INSET: f32 = 2.0;
/// A comparison's track holds two halves: the first player's spans over the
/// second's, each this far from the track's edge and this far apart.
const SPLIT_INSET: f32 = 1.5;
const SPLIT_GAP: f32 = 1.0;
/// Corners: a span's, a moment's tick (`.span.tick{border-radius:1px}`)
/// and the track's (`.lane{border-radius:2px}`).
const SPAN_RADIUS: f32 = 2.0;
const TICK_RADIUS: f32 = 1.0;
const TRACK_RADIUS: f32 = 2.0;
/// A span's outline in the panel's own surface, so two that overlap in one
/// lane (Power Infusion inside a Heroism) read as two; a hovered one's in
/// white (`.span:hover{outline:1px solid #fff}`).
const SPAN_EDGE: f32 = 1.0;
/// How far either side of a thin span the pointer still finds it.
const SPAN_SLOP: f32 = 2.0;
/// The narrowest a span is drawn, and a moment's tick (`.span{min-width:
/// 3px}`, `.span.tick{width:3px}`).
const SPAN_MIN: f32 = 3.0;
/// The lane labels' column and the gap after it (`.lanes{grid-template-
/// columns:74px …;column-gap:8px}`): where every track — and, with lanes,
/// the plot — starts.
pub(crate) const LABEL_W: f32 = 74.0;
const LABEL_GAP: f32 = 8.0;
const LEFT: f32 = LABEL_W + LABEL_GAP;
/// Axis ticks (`.iaxis span{font-size:11px}`), lane labels (`.lane-l{font-
/// size:12px}`), a hatch's words (`.hatch b{11px 600}`) and the tooltip's
/// (`.tip{font-size:13px;padding:5px 8px;radius:6px}`).
const TICK_PX: f32 = 11.0;
const LANE_PX: f32 = 12.0;
const HATCH_PX: f32 = 11.0;
const TIP_PX: f32 = 13.0;
const TIP_PAD: (f32, f32) = (8.0, 5.0);
const TIP_LINE: f32 = 17.0;
const TIP_RADIUS: f32 = 6.0;
/// R26: a stacked band's swatch before its line in the tooltip, and the
/// gap after it.
const SWATCH: f32 = 9.0;
const SWATCH_GAP: f32 = 6.0;
const SWATCH_RADIUS: f32 = 2.0;
/// R26: the panel's gap between two stacked bands.
const STACK_GAP: f32 = 2.0;
/// The tooltip's distance from the crosshair, and above a span, and how
/// far down the plot it hangs beside the crosshair.
const TIP_OFF_X: f32 = 12.0;
const TIP_OFF_Y: f32 = 6.0;
const TIP_PLOT_Y: f32 = 14.0;
/// A tick at the axis's very start stands from it (`.iaxis span:first-child
/// {transform:none}`), one past 96 % of it back from it (`x>96 ?
/// translateX(-100%)`); the rest are centred on their minute.
const TICK_FIRST: f32 = 0.001;
const TICK_LAST: f32 = 0.96;
/// A press-release wander below this is a click, not a zoom window.
const DRAG_MIN_PX: f32 = 3.0;
/// The hatch's stripes (`repeating-linear-gradient(135deg, … 0 4px,
/// transparent 4px 8px)`): 4 px of ink and 4 px of air measured ACROSS the
/// 45° stripes — 8√2 px apart along the row, a stroke 4 px wide.
const STRIPE_STEP: f32 = 8.0 * std::f32::consts::SQRT_2;
const STRIPE_W: f32 = 4.0;
/// The hatch's ink (`rgba(255,92,99,.16)`) and its dashed edge
/// (`border-left:1.5px dashed`), and the patch its words sit on.
const HATCH_ALPHA: f32 = 0.16;
const HATCH_EDGE: f32 = 1.5;
const HATCH_DASH: [f32; 2] = [3.0, 3.0];
const HATCH_PATCH_ALPHA: f32 = 0.85;
/// The patch reaches this far past the words on every side, its corners
/// rounded this much.
const HATCH_PATCH_PAD: f32 = 3.0;
const HATCH_PATCH_RADIUS: f32 = 3.0;
/// A hatch's words stand this far from its edge (`.hatch b{left:5px}`) and
/// this far under the plot's top; a second row of them — two deaths whose
/// words would overlap — one line and this much more under the first.
const HATCH_WORDS_X: f32 = 5.0;
const HATCH_WORDS_Y: f32 = 1.0;
const HATCH_ROW_GAP: f32 = 1.0;
/// The air between the words' band and the highest a curve is drawn.
const HATCH_BAND_GAP: f32 = 2.0;
/// The scale in the lanes' gutter (`.iaxis span{font-size:11px}`): the
/// peak at the plot's top, 0 at its baseline, right-aligned to the gutter's
/// label column so they end [`LABEL_GAP`] before the plot.
const SCALE_PX: f32 = 11.0;
/// The track under a lane (`.lane{background:rgba(255,255,255,.028)}`), a
/// span at rest and lit (`.span{opacity:.85}`, `:hover{opacity:1}`), a
/// curve's area (`fill-opacity=".14"`), a ghost's line, a drag's window.
const TRACK: Color = Color::from_rgba(1.0, 1.0, 1.0, 0.028);
const SPAN_ALPHA: f32 = 0.85;
const AREA_ALPHA: f32 = 0.14;
const GHOST_ALPHA: f32 = 0.30;
const DRAG_FILL: Color = Color::from_rgba(1.0, 1.0, 1.0, 0.10);
/// A curve's stroke (`stroke-width="1.6"`), a ghost's, and the crosshair's
/// gold (`.xhair{background:rgba(242,193,75,.55)}`).
const CURVE_W: f32 = 1.6;
const GHOST_W: f32 = 1.0;
/// The baseline, the crosshair and the tooltip's frame: a 1 px rule.
const HAIRLINE: f32 = 1.0;
const XHAIR_ALPHA: f32 = 0.55;
/// The peak stands this far under the plot's top (`H - v/max*(H-4)`).
const PEAK_INSET: f32 = 4.0;
/// A one-line text box's height for its size: iced's default line height.
const LINE: f32 = 1.3;
/// A second curve is dashed (`stroke-dasharray: 5 4`), so the two stay
/// apart by more than their hue.
const DASH: [f32; 2] = [5.0, 4.0];

/// Everything the graph draws, owned: the inspector builds it from the
/// snapshot, the canvas draws it.
pub(crate) struct Plot<M> {
    /// The stretch of the fight on show, ms: the whole of it, or a zoom.
    pub window: (u32, u32),
    /// The one scale every curve is drawn on.
    pub peak: f64,
    pub curves: Vec<Curve>,
    pub dead: Vec<Dead>,
    pub lanes: Vec<LaneRow>,
    /// The hover reads the curve as a running total ("63.0M") rather than
    /// a rate ("149,258").
    pub total: bool,
    /// What the hover calls a lone curve's value ("dps").
    pub word: &'static str,
    /// What a drag-selected window (or a right-click's "the whole fight
    /// back") becomes; `None` leaves the graph unzoomable.
    pub on_range: Option<crate::compare::OnRange<M>>,
}

/// The graph, as tall as its lanes make it.
pub(crate) fn view<M: 'static>(plot: Plot<M>) -> Element<'static, M> {
    let height = plot.height();
    Canvas::new(plot)
        .width(Length::Fill)
        .height(Length::Fixed(height))
        .into()
}

pub(crate) use wowdps_gui_logic::axis::ticks;

/// The canvas's own memory: a drag in progress, and what the pointer was
/// last over (so a move that changes nothing redraws nothing).
#[derive(Default)]
pub(crate) struct State {
    drag: Option<(f32, f32)>,
    hover: Option<Hover>,
}

/// What the pointer is over.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Hover {
    /// A span: its lane and its place on it.
    Span(usize, usize),
    /// The plot, at this x (canvas px), snapped to a bucket.
    Plot(f32),
}

/// One line of canvas text, placed and measured: what the plot says
/// besides the tooltip, so the tooltip can keep it out from under itself.
#[derive(Debug, Clone, PartialEq)]
struct Label {
    words: String,
    at: Point,
    px: f32,
    color: Color,
    font: Font,
    width: f32,
}

impl Label {
    /// The box the words stand in.
    fn rect(&self) -> Rectangle {
        Rectangle::new(self.at, Size::new(self.width, self.px * LINE))
    }
}

/// The tooltip, placed: its box and its lines.
struct Tip {
    rect: Rectangle,
    lines: Vec<(String, Color, Font)>,
    /// R26: a swatch before each line that names a stacked band — the
    /// words stay in ink, the band's colour beside them.
    swatches: Vec<Option<Color>>,
}

/// Do two boxes overlap (edges touching do not)?
fn overlaps(a: Rectangle, b: Rectangle) -> bool {
    a.x < b.x + b.width && b.x < a.x + a.width && a.y < b.y + b.height && b.y < a.y + a.height
}

impl<M> Plot<M> {
    /// The whole canvas: the plot, its axis and the lanes.
    fn height(&self) -> f32 {
        PLOT_H + AXIS_GAP + AXIS_H + self.lanes_h()
    }

    fn lanes_h(&self) -> f32 {
        let n = self.lanes.len() as f32;
        if n == 0.0 {
            0.0
        } else {
            LANES_TOP + n * LANE_ROW + (n - 1.0) * LANE_GAP
        }
    }

    /// Is this a comparison's graph, each lane split between the two?
    fn split(&self) -> bool {
        self.lanes
            .iter()
            .flat_map(|r| r.spans.iter())
            .any(|s| s.whose.is_some())
    }

    /// Where the plot and its axis start: at the lanes' tracks when there
    /// are lanes, at the canvas's edge when there are none.
    fn left(&self) -> f32 {
        if self.lanes.is_empty() { 0.0 } else { LEFT }
    }

    fn span_ms(&self) -> f64 {
        f64::from(self.window.1.saturating_sub(self.window.0).max(1))
    }

    /// The canvas x of `ms` from the fight's start, on a canvas `w` wide.
    fn x_of(&self, ms: f64, w: f32) -> f32 {
        let left = self.left();
        left + ((ms - f64::from(self.window.0)) / self.span_ms()) as f32 * (w - left).max(1.0)
    }

    /// The ms from the fight's start a canvas x lands on, held inside the
    /// window.
    fn ms_at(&self, x: f32, w: f32) -> u32 {
        let left = self.left();
        let frac = ((x - left) / (w - left).max(1.0)).clamp(0.0, 1.0) as f64;
        (f64::from(self.window.0) + frac * self.span_ms()).max(0.0) as u32
    }

    /// The plot's y for a value: the peak `top` under the plot's top (a
    /// few px, or under the hatches' words — [`Plot::top_of`]).
    fn y_of(&self, v: f64, top: f32) -> f32 {
        if self.peak <= 0.0 {
            return PLOT_H;
        }
        PLOT_H - (v / self.peak).clamp(0.0, 1.0) as f32 * (PLOT_H - top)
    }

    /// Where the peak is drawn, under the hatches' words `hatches` when
    /// there are any — so a curve never runs through them.
    fn top_of(hatches: &[Label]) -> f32 {
        hatches
            .iter()
            .map(|l| l.at.y + l.px * LINE + HATCH_BAND_GAP)
            .fold(PEAK_INSET, f32::max)
    }

    /// The top of lane `lane`'s row (its label's line).
    fn lane_y(&self, lane: usize) -> f32 {
        PLOT_H + AXIS_GAP + AXIS_H + LANES_TOP + lane as f32 * (LANE_ROW + LANE_GAP)
    }

    /// The top of lane `lane`'s track, centred in its row.
    fn track_y(&self, lane: usize) -> f32 {
        self.lane_y(lane) + (LANE_ROW - LANE_H) / 2.0
    }

    /// A span's top and height in its track: the whole track's less the
    /// inset, or — a comparison's — the first player's half over the
    /// second's.
    fn span_band(&self, lane: usize, s: &lanes::Span) -> (f32, f32) {
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
    fn span_rect(&self, lane: usize, s: &lanes::Span, w: f32) -> Option<Rectangle> {
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
        Some(Rectangle {
            x,
            y,
            width,
            height,
        })
    }

    /// The span under `p`, the topmost (latest drawn) where they overlap;
    /// a thin one answers a couple of pixels either side of itself. On a
    /// comparison's split track the pointer's half says whose.
    fn span_at(&self, p: Point, w: f32) -> Option<(usize, usize)> {
        let lane = (0..self.lanes.len()).find(|&i| {
            let y = self.track_y(i);
            p.y >= y && p.y < y + LANE_H
        })?;
        let row = self.lanes.get(lane)?;
        let second = p.y >= self.track_y(lane) + LANE_H / 2.0;
        let split = self.split();
        row.spans
            .iter()
            .enumerate()
            .rev()
            .filter(|(_, s)| !split || s.second == second)
            .find(|(_, s)| {
                self.span_rect(lane, s, w)
                    .is_some_and(|r| p.x >= r.x - SPAN_SLOP && p.x <= r.x + r.width + SPAN_SLOP)
            })
            .map(|(i, _)| (lane, i))
    }

    /// What the pointer at `p` is over.
    fn hover_at(&self, p: Point, w: f32) -> Option<Hover> {
        if let Some((lane, i)) = self.span_at(p, w) {
            return Some(Hover::Span(lane, i));
        }
        if p.y < 0.0 || p.y > PLOT_H || p.x < self.left() || p.x > w {
            return None;
        }
        // Snapped to the first curve's bucket, so the readout and the
        // crosshair move together.
        let bucket = self.curves.first().map_or(1000, |c| c.bucket_ms.max(1));
        let ms = self.ms_at(p.x, w);
        let snapped = (ms / bucket) * bucket + bucket / 2;
        Some(Hover::Plot(self.x_of(f64::from(snapped), w)))
    }

    /// The words the tooltip for `hover` shows, first line first.
    fn tip_words(&self, hover: Hover, w: f32) -> Vec<(String, Color, Font)> {
        match hover {
            Hover::Span(lane, i) => {
                let Some(s) = self.lanes.get(lane).and_then(|r| r.spans.get(i)) else {
                    return Vec::new();
                };
                let (name, details) = lanes::span_words(s);
                vec![
                    (name, theme::INK, theme::UI_SEMIBOLD),
                    (details, theme::INK_2, theme::UI),
                ]
            }
            Hover::Plot(x) => {
                let ms = self.ms_at(x, w);
                let mut lines = vec![(super::mmss(ms), theme::INK, theme::UI_SEMIBOLD)];
                for c in self.curves.iter().filter(|c| c.ink != Ink::Ghost) {
                    let v = c
                        .at(ms)
                        .map_or_else(|| "—".to_string(), |v| value_words(v, self.total));
                    let who = if c.name.is_empty() {
                        self.word.to_string()
                    } else {
                        c.name.clone()
                    };
                    lines.push((format!("{who}  {v}"), theme::INK, theme::UI));
                }
                lines
            }
        }
    }

    /// The tooltip for `hover` (`.tip`), placed: beside the crosshair on
    /// the plot (flipped left at the edge), above a span on a lane so it
    /// lands on the plot, never under the pointer — held inside the canvas.
    fn tip(&self, hover: Hover, w: f32) -> Option<Tip> {
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
                        .map(|c| (c.ink == Ink::Stack).then_some(theme::c(c.color))),
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
            .map(|(s, _, f)| text_w(s, TIP_PX, *f))
            .fold(0.0_f32, f32::max)
            + inset
            + 2.0 * TIP_PAD.0;
        let th = lines.len() as f32 * TIP_LINE + 2.0 * TIP_PAD.1;
        let mut at = match hover {
            Hover::Plot(x) => {
                let right = x + TIP_OFF_X;
                if right + tw > w {
                    Point::new(x - TIP_OFF_X - tw, TIP_PLOT_Y)
                } else {
                    Point::new(right, TIP_PLOT_Y)
                }
            }
            Hover::Span(l, i) => {
                let r = self
                    .lanes
                    .get(l)
                    .and_then(|row| row.spans.get(i))
                    .and_then(|s| self.span_rect(l, s, w))?;
                Point::new(r.x + r.width / 2.0 - tw / 2.0, r.y - th - TIP_OFF_Y)
            }
        };
        at.x = at.x.clamp(0.0, (w - tw).max(0.0));
        at.y = at.y.clamp(0.0, (self.height() - th).max(0.0));
        Some(Tip {
            rect: Rectangle::new(at, Size::new(tw, th)),
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
    fn hatch_labels(&self, w: f32) -> Vec<Label> {
        let left = self.left();
        let line = HATCH_PX * LINE;
        let mut placed: Vec<Label> = Vec::new();
        for d in &self.dead {
            let x1 = self.x_of(d.at_ms as f64, w).clamp(left, w);
            let x2 = self.x_of(d.end_ms as f64, w).clamp(left, w);
            if x2 <= x1 {
                continue;
            }
            let tw = text_w(&d.words, HATCH_PX, theme::UI_SEMIBOLD);
            let x = if x1 + HATCH_WORDS_X + tw > w {
                (x1 - HATCH_WORDS_X - tw).max(left)
            } else {
                x1 + HATCH_WORDS_X
            };
            let rows = [HATCH_WORDS_Y, HATCH_WORDS_Y + line + HATCH_ROW_GAP];
            let free = rows.into_iter().find(|&y| {
                let rect = Rectangle::new(Point::new(x, y), Size::new(tw, line));
                placed.iter().all(|l| !overlaps(l.rect(), rect))
            });
            if let Some(y) = free {
                placed.push(Label {
                    words: d.words.clone(),
                    at: Point::new(x, y),
                    px: HATCH_PX,
                    color: theme::BAD,
                    font: theme::UI_SEMIBOLD,
                    width: tw,
                });
            }
        }
        placed
    }

    /// The scale in the gutter beside the plot, when it has one (it has
    /// lanes): the peak level with where the peak is drawn, `top`, and 0
    /// on the baseline, each ending [`LABEL_GAP`] before the plot.
    fn scale_labels(&self, top: f32) -> Vec<Label> {
        if self.lanes.is_empty() || self.peak <= 0.0 {
            return Vec::new();
        }
        let line = SCALE_PX * LINE;
        [
            (
                figure(self.peak.round() as u64),
                (top - line / 2.0).max(0.0),
            ),
            ("0".to_string(), PLOT_H - line),
        ]
        .into_iter()
        .map(|(words, y)| {
            let width = text_w(&words, SCALE_PX, theme::UI);
            Label {
                at: Point::new(LABEL_W - width, y),
                words,
                px: SCALE_PX,
                color: theme::INK_3_TEXT,
                font: theme::UI,
                width,
            }
        })
        .collect()
    }

    /// Everything the plot says besides a tooltip: the hatches' words, the
    /// scale, the axis's ticks (the first from its left edge, one near the
    /// end back from it) and the lanes' labels — less whatever the tooltip
    /// for `hover` would cover. iced sets a canvas's text over every shape
    /// in it, so a label under the tooltip would print through it.
    fn labels(&self, hover: Option<Hover>, w: f32) -> Vec<Label> {
        let left = self.left();
        let plot_w = (w - left).max(1.0);
        let mut out = self.hatch_labels(w);
        out.extend(self.scale_labels(Self::top_of(&out)));
        let axis_y = PLOT_H + AXIS_GAP + AXIS_WORDS_Y;
        for t in ticks(self.window, plot_w) {
            let x = self.x_of(f64::from(t), w);
            let words = super::mmss(t);
            let tw = text_w(&words, TICK_PX, theme::UI);
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
                at: Point::new(at, axis_y),
                px: TICK_PX,
                color: theme::INK_3_TEXT,
                font: theme::UI,
                width: tw,
            });
        }
        for (li, row) in self.lanes.iter().enumerate() {
            let y = self.lane_y(li);
            let words = row.lane.label().to_string();
            let width = text_w(&words, LANE_PX, theme::UI);
            out.push(Label {
                words,
                at: Point::new(0.0, y + (LANE_ROW - LANE_PX * LINE) / 2.0),
                px: LANE_PX,
                color: theme::INK_3_TEXT,
                font: theme::UI,
                width,
            });
        }
        match hover.and_then(|h| self.tip(h, w)) {
            Some(tip) => out
                .into_iter()
                .filter(|l| !overlaps(l.rect(), tip.rect))
                .collect(),
            None => out,
        }
    }

    /// The curve's points inside the window, as canvas points — with one
    /// at the window's start holding the first value, as the prototype's
    /// `curve()` pads it, so the line meets the plot's edge.
    fn points(&self, c: &Curve, w: f32, top: f32) -> Vec<Point> {
        let b = f64::from(c.bucket_ms.max(1));
        let (lo, hi) = (f64::from(self.window.0), f64::from(self.window.1));
        let mut pts: Vec<(f64, f64)> = c
            .points
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
        let reach = c.points.len() as f64 * b;
        if let Some(&(last, v)) = pts.last()
            && last < hi
            && reach >= hi - b / 2.0
        {
            pts.push((hi, v));
        }
        pts.into_iter()
            .map(|(ms, v)| Point::new(self.x_of(ms, w), self.y_of(v, top)))
            .collect()
    }
}

/// Trace `pts` into `path` as the prototype's `smooth()` does: a
/// Catmull-Rom spline as cubic Béziers, each control point a sixth of the
/// neighbours' span along — held inside the plot, so a steep step cannot
/// swing the line under its baseline or over its top.
fn smooth(path: &mut canvas::path::Builder, pts: &[Point]) {
    let hold = |p: Point| Point::new(p.x, p.y.clamp(0.0, PLOT_H));
    let at = |j: usize| {
        pts.get(j)
            .or_else(|| pts.last())
            .copied()
            .unwrap_or(Point::ORIGIN)
    };
    for i in 0..pts.len().saturating_sub(1) {
        let (p0, p1, p2, p3) = (at(i.saturating_sub(1)), at(i), at(i + 1), at(i + 2));
        let c1 = hold(Point::new(
            p1.x + (p2.x - p0.x) / 6.0,
            p1.y + (p2.y - p0.y) / 6.0,
        ));
        let c2 = hold(Point::new(
            p2.x - (p3.x - p1.x) / 6.0,
            p2.y - (p3.y - p1.y) / 6.0,
        ));
        path.bezier_curve_to(c1, c2, p2);
    }
}

/// `content`'s one-line width at `px` in `font`, as the renderer shapes it.
fn text_w(content: &str, px: f32, font: Font) -> f32 {
    crate::ellipsis::width_of::<<Renderer as iced::advanced::text::Renderer>::Paragraph>(
        content, px, font,
    )
}

/// One line of canvas text.
fn words(frame: &mut canvas::Frame, s: &str, at: Point, px: f32, color: Color, font: Font) {
    frame.fill_text(canvas::Text {
        content: s.to_string(),
        position: at,
        color,
        size: px.into(),
        font,
        align_x: iced::alignment::Horizontal::Left.into(),
        align_y: iced::alignment::Vertical::Top,
        ..canvas::Text::default()
    });
}

impl<M> canvas::Program<M> for Plot<M> {
    type State = State;

    fn update(
        &self,
        state: &mut State,
        event: &canvas::Event,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Option<canvas::Action<M>> {
        use iced::mouse::{Button, Event as Mouse};
        let iced::Event::Mouse(mouse) = event else {
            return None;
        };
        let w = bounds.width;
        let left = self.left();
        let pos = cursor.position_in(bounds);
        let in_plot = |p: Point| p.y <= PLOT_H && p.x >= left;
        match mouse {
            Mouse::ButtonPressed(Button::Left) if self.on_range.is_some() => {
                let p = pos.filter(|p| in_plot(*p))?;
                state.drag = Some((p.x, p.x));
                Some(canvas::Action::request_redraw().and_capture())
            }
            Mouse::ButtonReleased(Button::Left) => {
                let (a, b) = state.drag.take()?;
                let on_range = self.on_range.as_ref()?;
                if (b - a).abs() < DRAG_MIN_PX {
                    return Some(canvas::Action::request_redraw());
                }
                let range = (self.ms_at(a.min(b), w), self.ms_at(a.max(b), w));
                Some(canvas::Action::publish(on_range(Some(range))).and_capture())
            }
            Mouse::ButtonPressed(Button::Right) => {
                let on_range = self.on_range.as_ref()?;
                pos.filter(|p| in_plot(*p))?;
                Some(canvas::Action::publish(on_range(None)).and_capture())
            }
            Mouse::CursorMoved { .. } | Mouse::CursorLeft => {
                if let Some((_, cur)) = state.drag.as_mut() {
                    let x = cursor.position().map(|p| (p.x - bounds.x).clamp(left, w))?;
                    *cur = x;
                    return Some(canvas::Action::request_redraw());
                }
                let hover = pos.and_then(|p| self.hover_at(p, w));
                if hover == state.hover {
                    return None;
                }
                state.hover = hover;
                Some(canvas::Action::request_redraw())
            }
            _ => None,
        }
    }

    fn mouse_interaction(
        &self,
        state: &State,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> mouse::Interaction {
        if state.drag.is_some() {
            return mouse::Interaction::ResizingHorizontally;
        }
        match cursor
            .position_in(bounds)
            .and_then(|p| self.hover_at(p, bounds.width))
        {
            Some(Hover::Plot(_)) if self.on_range.is_some() => mouse::Interaction::Crosshair,
            _ => mouse::Interaction::default(),
        }
    }

    fn draw(
        &self,
        state: &State,
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<canvas::Geometry> {
        let mut frame = canvas::Frame::new(renderer, bounds.size());
        let w = bounds.width;
        let left = self.left();
        let plot_w = (w - left).max(1.0);
        // The hatches' words, placed once: their band is what the curves
        // peak under.
        let hatches = self.hatch_labels(w);
        let top = Self::top_of(&hatches);

        // The baseline, a hairline.
        frame.stroke(
            &Path::line(Point::new(left, PLOT_H - 0.5), Point::new(w, PLOT_H - 0.5)),
            Stroke::default()
                .with_width(HAIRLINE)
                .with_color(theme::LINE),
        );

        // A death, hatched from the moment to the rez (or the end), its
        // edge a dashed red rule.
        for d in &self.dead {
            let x1 = self.x_of(d.at_ms as f64, w).clamp(left, w);
            let x2 = self.x_of(d.end_ms as f64, w).clamp(left, w);
            if x2 <= x1 {
                continue;
            }
            let ink = Color {
                a: HATCH_ALPHA,
                ..theme::BAD
            };
            // "/" stripes, each cut to the span: from (s, bottom) up to
            // (s + H, top), kept where it is between x1 and x2.
            let mut s = x1 - PLOT_H;
            while s < x2 {
                let t0 = ((x1 - s) / PLOT_H).clamp(0.0, 1.0);
                let t1 = ((x2 - s) / PLOT_H).clamp(0.0, 1.0);
                if t1 > t0 {
                    let at = |t: f32| Point::new(s + t * PLOT_H, PLOT_H - t * PLOT_H);
                    frame.stroke(
                        &Path::line(at(t0), at(t1)),
                        Stroke::default().with_width(STRIPE_W).with_color(ink),
                    );
                }
                s += STRIPE_STEP;
            }
            frame.stroke(
                &Path::line(Point::new(x1, 0.0), Point::new(x1, PLOT_H)),
                Stroke {
                    line_dash: LineDash {
                        segments: &HATCH_DASH,
                        offset: 0,
                    },
                    ..Stroke::default()
                        .with_width(HATCH_EDGE)
                        .with_color(theme::BAD)
                },
            );
        }

        // R26: the stack — each band from the running sum of those before it
        // up to its own top, in list order (bottom first), its top edge a
        // 2 px line of the panel so two bands read as two.
        let mut base: Vec<f64> = Vec::new();
        for c in self.curves.iter().filter(|c| c.ink == Ink::Stack) {
            let mut sum = base.clone();
            if sum.len() < c.points.len() {
                sum.resize(c.points.len(), 0.0);
            }
            for (s, v) in sum.iter_mut().zip(&c.points) {
                *s += v.max(0.0);
            }
            let at = |values: &[f64]| {
                self.points(
                    &Curve {
                        points: values.to_vec(),
                        ..c.clone()
                    },
                    w,
                    top,
                )
            };
            let upper = at(&sum);
            let lower = if base.is_empty() {
                Vec::new()
            } else {
                let mut b = base.clone();
                b.resize(sum.len(), 0.0);
                at(&b)
            };
            let (Some(first), Some(last)) = (upper.first(), upper.last()) else {
                base = sum;
                continue;
            };
            let mut band = canvas::path::Builder::new();
            band.move_to(*first);
            smooth(&mut band, &upper);
            if lower.is_empty() {
                band.line_to(Point::new(last.x, PLOT_H));
                band.line_to(Point::new(first.x, PLOT_H));
            } else {
                let back: Vec<Point> = lower.iter().rev().copied().collect();
                if let Some(b0) = back.first() {
                    band.line_to(*b0);
                }
                smooth(&mut band, &back);
            }
            band.close();
            frame.fill(&band.build(), theme::c(c.color));
            let mut edge = canvas::path::Builder::new();
            edge.move_to(*first);
            smooth(&mut edge, &upper);
            frame.stroke(
                &edge.build(),
                Stroke::default()
                    .with_width(STACK_GAP)
                    .with_color(theme::SURFACE)
                    .with_line_join(canvas::LineJoin::Round),
            );
            base = sum;
        }

        // The curves: ghosts first, so a focus reads on top of its context.
        let mut order: Vec<&Curve> = self.curves.iter().filter(|c| c.ink != Ink::Stack).collect();
        order.sort_by_key(|c| c.ink != Ink::Ghost);
        for c in order {
            let pts = self.points(c, w, top);
            let (Some(first), Some(last)) = (pts.first(), pts.last()) else {
                continue;
            };
            let mut line = canvas::path::Builder::new();
            line.move_to(*first);
            smooth(&mut line, &pts);
            let line = line.build();
            if c.ink == Ink::Area {
                let mut area = canvas::path::Builder::new();
                area.move_to(Point::new(first.x, PLOT_H));
                area.line_to(*first);
                smooth(&mut area, &pts);
                area.line_to(Point::new(last.x, PLOT_H));
                area.close();
                frame.fill(
                    &area.build(),
                    Color {
                        a: AREA_ALPHA,
                        ..theme::c(c.color)
                    },
                );
            }
            let stroke = match c.ink {
                Ink::Ghost => Stroke::default().with_width(GHOST_W).with_color(Color {
                    a: GHOST_ALPHA,
                    ..theme::c(c.color)
                }),
                Ink::Dashed => Stroke {
                    line_dash: LineDash {
                        segments: &DASH,
                        offset: 0,
                    },
                    ..Stroke::default()
                        .with_width(CURVE_W)
                        .with_color(theme::c(c.color))
                },
                Ink::Area | Ink::Line | Ink::Stack => Stroke::default()
                    .with_width(CURVE_W)
                    .with_color(theme::c(c.color)),
            };
            frame.stroke(&line, stroke.with_line_join(canvas::LineJoin::Round));
        }

        // The hatches' words sit on a patch of the panel, over the hatch's
        // stripes and its dashed edge.
        for l in &hatches {
            frame.fill(
                &Path::rounded_rectangle(
                    Point::new(l.at.x - HATCH_PATCH_PAD, l.at.y - HATCH_WORDS_Y),
                    Size::new(
                        l.width + 2.0 * HATCH_PATCH_PAD,
                        l.px * LINE + 2.0 * HATCH_WORDS_Y,
                    ),
                    HATCH_PATCH_RADIUS.into(),
                ),
                Color {
                    a: HATCH_PATCH_ALPHA,
                    ..theme::SURFACE
                },
            );
        }

        // The lanes: the faint track, the spans in their casters' colours,
        // each edged in the panel's surface so two that overlap read as
        // two — the hovered one lit and outlined in white.
        let hovered_span = match state.hover {
            Some(Hover::Span(l, i)) => Some((l, i)),
            _ => None,
        };
        for (li, row) in self.lanes.iter().enumerate() {
            frame.fill(
                &Path::rounded_rectangle(
                    Point::new(left, self.track_y(li)),
                    Size::new(plot_w, LANE_H),
                    TRACK_RADIUS.into(),
                ),
                TRACK,
            );
            for (si, s) in row.spans.iter().enumerate() {
                let Some(r) = self.span_rect(li, s, w) else {
                    continue;
                };
                let lit = hovered_span == Some((li, si));
                let radius = if s.dur_ms <= 0 {
                    TICK_RADIUS
                } else {
                    SPAN_RADIUS
                };
                let shape = Path::rounded_rectangle(Point::new(r.x, r.y), r.size(), radius.into());
                frame.fill(
                    &shape,
                    Color {
                        a: if lit { 1.0 } else { SPAN_ALPHA },
                        ..theme::c(s.color)
                    },
                );
                frame.stroke(
                    &shape,
                    Stroke::default().with_width(SPAN_EDGE).with_color(if lit {
                        Color::WHITE
                    } else {
                        theme::SURFACE
                    }),
                );
            }
        }

        // A drag in progress, over everything but the tooltip.
        if let Some((a, b)) = state.drag
            && (b - a).abs() >= DRAG_MIN_PX
        {
            let (lo, hi) = (a.min(b), a.max(b));
            frame.fill(
                &Path::rectangle(Point::new(lo, 0.0), Size::new(hi - lo, PLOT_H)),
                DRAG_FILL,
            );
        }

        // The crosshair, in gold (`.xhair`).
        if let Some(Hover::Plot(x)) = state.hover {
            frame.stroke(
                &Path::line(Point::new(x, 0.0), Point::new(x, PLOT_H)),
                Stroke::default().with_width(HAIRLINE).with_color(Color {
                    a: XHAIR_ALPHA,
                    ..theme::GOLD
                }),
            );
        }

        // Every label the tooltip leaves clear, then the tooltip over all.
        for l in self.labels(state.hover, w) {
            words(&mut frame, &l.words, l.at, l.px, l.color, l.font);
        }
        if let Some(tip) = state.hover.and_then(|h| self.tip(h, w)) {
            let shape =
                Path::rounded_rectangle(tip.rect.position(), tip.rect.size(), TIP_RADIUS.into());
            frame.fill(&shape, theme::SURFACE);
            frame.stroke(
                &shape,
                Stroke::default()
                    .with_width(HAIRLINE)
                    .with_color(theme::EDGE),
            );
            let inset = if tip.swatches.iter().any(Option::is_some) {
                SWATCH + SWATCH_GAP
            } else {
                0.0
            };
            for (i, (s, color, font)) in tip.lines.iter().enumerate() {
                let y = tip.rect.y + TIP_PAD.1 + i as f32 * TIP_LINE;
                if let Some(Some(swatch)) = tip.swatches.get(i) {
                    frame.fill(
                        &Path::rounded_rectangle(
                            Point::new(tip.rect.x + TIP_PAD.0, y + (TIP_LINE - SWATCH) / 2.0),
                            Size::new(SWATCH, SWATCH),
                            SWATCH_RADIUS.into(),
                        ),
                        *swatch,
                    );
                }
                words(
                    &mut frame,
                    s,
                    Point::new(tip.rect.x + TIP_PAD.0 + inset, y),
                    TIP_PX,
                    *color,
                    *font,
                );
            }
        }
        vec![frame.into_geometry()]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::inspector::lanes::{Lane, Span};

    fn plot(window: (u32, u32), lanes: Vec<LaneRow>) -> Plot<()> {
        Plot {
            window,
            peak: 100.0,
            curves: vec![Curve {
                name: String::new(),
                color: theme::g(Color::WHITE),
                points: vec![10.0, 50.0, 100.0, 40.0],
                bucket_ms: 1000,
                ink: Ink::Area,
            }],
            dead: Vec::new(),
            lanes,
            total: false,
            word: "dps",
            on_range: None,
        }
    }

    fn span(at_ms: i64, dur_ms: i64) -> Span {
        Span {
            at_ms,
            dur_ms,
            label: "Heroism".into(),
            caster: Some("Vingsham".into()),
            color: theme::g(Color::WHITE),
            whose: None,
            second: false,
        }
    }

    fn one_lane(spans: Vec<Span>) -> Vec<LaneRow> {
        vec![LaneRow {
            lane: Lane::Externals,
            spans,
        }]
    }

    /// Minutes while they fit; wider steps when they would crowd, finer
    /// ones in a short zoom.
    #[test]
    fn the_axis_ticks_in_minutes_while_they_fit() {
        assert_eq!(
            ticks((0, 422_040), 438.0),
            (0..=7).map(|m| m * 60_000).collect::<Vec<_>>()
        );
        let long = ticks((0, 30 * 60_000), 300.0);
        assert_eq!(long.get(1), Some(&300_000), "every 5 minutes");
        let zoomed = ticks((61_000, 92_000), 300.0);
        assert_eq!(zoomed, vec![65_000, 70_000, 75_000, 80_000, 85_000, 90_000]);
    }

    /// The plot, the axis and every lane share one x: with lanes the plot
    /// starts at the lanes' track, without them at the canvas's edge, and
    /// a zoom maps its window across all of them.
    #[test]
    fn the_plot_and_the_lanes_share_one_x() {
        let p = plot(
            (0, 4_000),
            one_lane(vec![span(2_000, 1_000), span(3_000, 0)]),
        );
        let w = LEFT + 400.0;
        assert_eq!(p.x_of(0.0, w), LEFT, "the plot starts at the track");
        assert_eq!(p.x_of(2_000.0, w), LEFT + 200.0);
        let r = p.span_rect(0, &p.lanes[0].spans[0], w).unwrap();
        assert_eq!((r.x, r.width), (LEFT + 200.0, 100.0), "a span is its time");
        let tick = p.span_rect(0, &p.lanes[0].spans[1], w).unwrap();
        assert_eq!(tick.width, SPAN_MIN, "a moment is a 3 px tick");
        assert_eq!(p.ms_at(LEFT + 100.0, w), 1_000);
        let bare = plot((1_000, 3_000), Vec::new());
        assert_eq!(bare.x_of(1_000.0, 400.0), 0.0, "no lanes: the whole width");
        assert_eq!(bare.x_of(2_000.0, 400.0), 200.0, "a zoom fills the plot");
        assert_eq!(
            p.height(),
            PLOT_H + AXIS_GAP + AXIS_H + LANES_TOP + LANE_ROW,
            "one lane"
        );
        assert_eq!(bare.height(), PLOT_H + AXIS_GAP + AXIS_H, "no lanes");
    }

    /// Zoomed, a span that began before the window starts at the track's
    /// edge — never over the lane labels — one that runs past it stops at
    /// the canvas's, and one wholly outside is not drawn or hovered.
    #[test]
    fn a_zoom_holds_every_span_to_the_track() {
        let spans = vec![
            span(0, 2_000),
            span(2_500, 5_000),
            span(0, 500),
            span(500, 0),
            span(4_000, 1_000),
        ];
        let p = plot((1_000, 3_000), one_lane(spans));
        let w = LEFT + 400.0;
        let rect = |i: usize| p.span_rect(0, &p.lanes[0].spans[i], w);
        let early = rect(0).expect("it overlaps the window");
        assert!(early.x >= LEFT, "{early:?}");
        assert_eq!((early.x, early.width), (LEFT, 200.0), "cut at the start");
        let late = rect(1).expect("it overlaps the window");
        assert_eq!(late.x + late.width, w, "cut at the end");
        assert_eq!(rect(2), None, "over before the window");
        assert_eq!(rect(3), None, "a moment before it");
        assert_eq!(rect(4), None, "after it");
        let over_label = Point::new(LABEL_W / 2.0, p.lane_y(0) + 6.0);
        assert_eq!(p.hover_at(over_label, w), None, "the label is no span");
    }

    /// The pointer on a span names it (its caster, its time and length);
    /// on the plot, the instant and the curve's value there.
    #[test]
    fn hovering_names_a_span_or_reads_the_curve() {
        let p = plot((0, 4_000), one_lane(vec![span(2_000, 1_000)]));
        let w = LEFT + 400.0;
        let on_span = Point::new(LEFT + 250.0, p.lane_y(0) + 6.0);
        assert_eq!(p.hover_at(on_span, w), Some(Hover::Span(0, 0)));
        let words = p.tip_words(Hover::Span(0, 0), w);
        assert_eq!(words[0].0, "Heroism");
        assert_eq!(words[1].0, "0:02, 1s, from Vingsham");
        let on_plot = Point::new(LEFT + 210.0, 40.0);
        let Some(Hover::Plot(x)) = p.hover_at(on_plot, w) else {
            panic!("the plot answers the pointer");
        };
        assert_eq!(x, LEFT + 250.0, "snapped to the bucket's middle");
        let words = p.tip_words(Hover::Plot(x), w);
        assert_eq!(words[0].0, "0:02");
        assert_eq!(words[1].0, "dps  100");
        assert_eq!(
            p.hover_at(Point::new(10.0, 40.0), w),
            None,
            "the lane labels' column"
        );
    }

    /// With lanes the gutter beside the plot is its scale — the peak level
    /// with the peak, 0 on the baseline, both ending before the plot — in
    /// the readable faint ink its ticks and lane labels are in; without
    /// lanes there is no gutter and no scale (the top line says the peak).
    #[test]
    fn the_gutter_is_the_plot_s_scale() {
        let p = plot((0, 4_000), one_lane(vec![span(2_000, 1_000)]));
        let labels = p.labels(None, LEFT + 400.0);
        assert!(labels.iter().any(|l| l.words == "Externals"));
        assert!(labels.iter().any(|l| l.words == "0:00"));
        let scale = |words: &str| labels.iter().find(|l| l.words == words).cloned();
        let peak = scale("100").expect("the peak on the scale");
        let zero = scale("0").expect("0 on the scale");
        for l in [&peak, &zero] {
            assert!(
                (l.at.x + l.width - LABEL_W).abs() < 0.01,
                "right-aligned, a gap before the plot: {l:?}"
            );
        }
        let at_peak = PEAK_INSET;
        assert!(
            peak.rect().y <= at_peak && at_peak <= peak.rect().y + peak.rect().height,
            "level with the peak: {peak:?}"
        );
        assert!(
            (zero.rect().y + zero.rect().height - PLOT_H).abs() < 0.01,
            "on the baseline: {zero:?}"
        );
        assert!(labels.iter().all(|l| l.color == theme::INK_3_TEXT));
        let bare = plot((0, 4_000), Vec::new());
        assert!(
            bare.labels(None, 400.0).iter().all(|l| l.words != "100"),
            "no lanes, no gutter, no scale"
        );
    }

    /// A lane's row is as tall as its label's line, the 13 px track centred
    /// in it and the label on the same line: rows 19.6 px apart.
    #[test]
    fn a_lane_row_is_its_label_s_line() {
        let p = plot(
            (0, 4_000),
            vec![
                LaneRow {
                    lane: Lane::Cooldowns,
                    spans: vec![span(0, 1_000)],
                },
                LaneRow {
                    lane: Lane::Items,
                    spans: vec![span(0, 0)],
                },
            ],
        );
        assert!((LANE_ROW - 15.6).abs() < 0.01);
        assert!((p.lane_y(1) - p.lane_y(0) - 19.6).abs() < 0.01);
        let inset = p.track_y(0) - p.lane_y(0);
        assert!((inset - 1.3).abs() < 0.01, "centred: {inset}");
        let r = p.span_rect(0, &p.lanes[0].spans[0], LEFT + 400.0).unwrap();
        assert_eq!(
            r.y,
            p.track_y(0) + SPAN_INSET,
            "the span inset in its track"
        );
        let label = p
            .labels(None, LEFT + 400.0)
            .into_iter()
            .find(|l| l.words == "Cooldowns")
            .unwrap();
        let (mid_label, mid_track) = (label.rect().center_y(), p.track_y(0) + LANE_H / 2.0);
        assert!((mid_label - mid_track).abs() < 0.01, "on the track's line");
    }

    /// A comparison's track is split: the first player's span over the
    /// second's, each found by the pointer's half.
    #[test]
    fn a_pair_s_lanes_split_the_track_between_the_two() {
        let half = |second: bool, at_ms| Span {
            whose: Some(if second { "B" } else { "A" }.into()),
            second,
            ..span(at_ms, 1_000)
        };
        let p = plot(
            (0, 4_000),
            one_lane(vec![half(false, 1_000), half(true, 1_000)]),
        );
        assert!(p.split());
        let w = LEFT + 400.0;
        let (a, b) = (
            p.span_rect(0, &p.lanes[0].spans[0], w).unwrap(),
            p.span_rect(0, &p.lanes[0].spans[1], w).unwrap(),
        );
        assert_eq!(a.x, b.x, "one clock");
        assert!(a.y + a.height <= b.y, "A over B: {a:?} {b:?}");
        assert!(b.y + b.height <= p.track_y(0) + LANE_H, "inside the track");
        let x = a.x + a.width / 2.0;
        assert_eq!(
            p.hover_at(Point::new(x, a.center_y()), w),
            Some(Hover::Span(0, 0))
        );
        assert_eq!(
            p.hover_at(Point::new(x, b.center_y()), w),
            Some(Hover::Span(0, 1))
        );
        assert_eq!(
            p.tip_words(Hover::Span(0, 1), w)[1].0,
            "B, 0:01, 1s, from Vingsham"
        );
        assert!(!plot((0, 4_000), one_lane(vec![span(0, 1)])).split());
    }

    /// A death's words stand in a band over the curves: the curves peak
    /// under it, and a second death's words that would overlap the first's
    /// drop to a second row — never printed over them.
    #[test]
    fn hatch_words_keep_a_band_over_the_curves() {
        let mut p = plot((0, 4_000), Vec::new());
        assert_eq!(Plot::<()>::top_of(&p.hatch_labels(400.0)), PEAK_INSET);
        p.dead.push(Dead {
            at_ms: 1_000,
            end_ms: 4_000,
            words: "Tranqlock died 0:01".into(),
        });
        p.dead.push(Dead {
            at_ms: 1_200,
            end_ms: 4_000,
            words: "Swampert died 0:01".into(),
        });
        let words = p.hatch_labels(400.0);
        assert_eq!(words.len(), 2);
        assert!(!overlaps(words[0].rect(), words[1].rect()), "{words:?}");
        let top = Plot::<()>::top_of(&words);
        assert!(
            words.iter().all(|l| l.rect().y + l.rect().height < top),
            "the curves peak under every word: {top}"
        );
        assert_eq!(p.y_of(p.peak, top), top);
        assert_eq!(p.y_of(0.0, top), PLOT_H);
    }

    /// A drag over the plot selects a window and publishes it; a click
    /// that barely moves publishes nothing; a right-click asks for the
    /// whole fight back.
    #[test]
    fn a_drag_zooms_a_click_does_not_and_a_right_click_resets() {
        use iced::mouse::{Button, Event as Mouse};
        use iced::widget::canvas::Program;
        let mut p = plot((0, 4_000), Vec::new());
        let on_range: crate::compare::OnRange<Option<(u32, u32)>> = std::rc::Rc::new(|r| r);
        let p: Plot<Option<(u32, u32)>> = Plot {
            window: p.window,
            peak: p.peak,
            curves: std::mem::take(&mut p.curves),
            dead: Vec::new(),
            lanes: Vec::new(),
            total: false,
            word: "dps",
            on_range: Some(on_range),
        };
        let bounds = Rectangle::new(Point::ORIGIN, Size::new(400.0, p.height()));
        let at = |x: f32| mouse::Cursor::Available(Point::new(x, 40.0));
        let send = |state: &mut State, e: Mouse, x: f32| {
            p.update(state, &iced::Event::Mouse(e), bounds, at(x))
                .map(|a| a.into_inner().0)
        };
        let mut state = State::default();
        send(&mut state, Mouse::ButtonPressed(Button::Left), 100.0);
        send(
            &mut state,
            Mouse::CursorMoved {
                position: Point::new(200.0, 40.0),
            },
            200.0,
        );
        let zoom = send(&mut state, Mouse::ButtonReleased(Button::Left), 200.0);
        assert_eq!(
            zoom,
            Some(Some(Some((1_000, 2_000)))),
            "a quarter to a half"
        );
        send(&mut state, Mouse::ButtonPressed(Button::Left), 100.0);
        let click = send(&mut state, Mouse::ButtonReleased(Button::Left), 101.0);
        assert_eq!(click, Some(None), "a click: a redraw, nothing published");
        let reset = send(&mut state, Mouse::ButtonPressed(Button::Right), 150.0);
        assert_eq!(reset, Some(Some(None)), "the whole fight back");
    }

    /// A tooltip over a lane-0 span lands on the axis's ticks; every tick
    /// it covers is left out, never printed through it.
    #[test]
    fn a_tooltip_hides_the_labels_under_it() {
        let spans = (0..7).map(|m| span(m * 60_000, 20_000)).collect();
        let p = plot((0, 420_000), one_lane(spans));
        let w = LEFT + 438.0;
        let everything = p.labels(None, w);
        for i in 0..7 {
            let hover = Hover::Span(0, i);
            let tip = p.tip(hover, w).expect("a tooltip").rect;
            let shown = p.labels(Some(hover), w);
            assert!(
                shown.iter().all(|l| !overlaps(l.rect(), tip)),
                "span {i}: {shown:?} under {tip:?}"
            );
            let covered = everything
                .iter()
                .filter(|l| overlaps(l.rect(), tip))
                .count();
            assert_eq!(shown.len() + covered, everything.len(), "only those");
        }
        let tip = p.tip(Hover::Span(0, 3), w).unwrap().rect;
        assert!(
            everything
                .iter()
                .any(|l| l.words.contains(':') && overlaps(l.rect(), tip)),
            "the case is real: a lane-0 tooltip covers the ticks"
        );
    }

    /// Every state draws — hatched, dashed, ghosted, hovered and dragging —
    /// through the software renderer without a panic.
    #[test]
    fn the_graph_draws_in_every_state() {
        use iced::widget::canvas::Program;
        let lanes = vec![LaneRow {
            lane: Lane::Cooldowns,
            spans: vec![span(500, 1_500), span(3_000, 0)],
        }];
        let mut p = plot((0, 4_000), lanes);
        p.dead.push(Dead {
            at_ms: 2_500,
            end_ms: 4_000,
            words: "died 0:02".into(),
        });
        for ink in [Ink::Line, Ink::Dashed, Ink::Ghost] {
            p.curves.push(Curve {
                ink,
                ..p.curves[0].clone()
            });
        }
        let renderer = crate::window::testkit::renderer();
        let bounds = Rectangle::new(Point::ORIGIN, Size::new(LEFT + 400.0, p.height()));
        let w = bounds.width;
        for hover in [
            None,
            Some(Hover::Span(0, 0)),
            p.hover_at(Point::new(LEFT + 100.0, 50.0), w),
        ] {
            let state = State {
                drag: Some((LEFT + 10.0, LEFT + 90.0)),
                hover,
            };
            let geometry = p.draw(
                &state,
                &renderer,
                &Theme::Dark,
                bounds,
                mouse::Cursor::Unavailable,
            );
            assert_eq!(geometry.len(), 1);
        }
    }
}
