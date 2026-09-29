//! The ribbon (R25, v35; the prototype's `.ribbon`): the pull's signature,
//! between the fight header's stat line and the view tabs. One canvas: the
//! whole group's rate for the view on screen — raid dps, hps on Healing,
//! dtps on Taken and Deaths — as an area of the ink fading from 26 % to 2 %
//! under a line of it, its peak named at the top-left, the minute ticks
//! under it, the lust windows as a faint wash with their name, and a skull
//! at every death in the dead player's class colour on a faint red
//! hairline, the owner's labelled "you" (the daemon's `mine`, else the
//! window's own hints, as the meter's "you" is found). A skull is a press
//! away from its recap (the Deaths view with that death selected, its
//! hairline lit); anywhere else on the plot the pointer reads the time and
//! the rate under a gold crosshair.
//!
//! The daemon's series is 1 s buckets (`RaidTimeline`); the curve is drawn
//! from 10 s rates, as the prototype draws it — finer in a short fight, so
//! a 30 s pull still has a shape. Hover is the canvas's own, never a
//! message; a press on a skull is the one message it sends.
//!
//! iced sets a canvas's text over every shape in it, so the tooltip keeps
//! the words it would cover out of the picture rather than print them
//! through itself (the inspector's graph does the same), and the peak's
//! words and "you" stand on a plate of the ground they sit on, drawn after
//! the curve and the hairlines, so no line runs through a glyph. A lust's
//! name stands on none, as the prototype's `.band b` has no background: it
//! sits over the burst, the curve's peak, which a plate would hide from
//! the words that name it. One placement pass keeps those words apart:
//! "you" stays over its skull, the peak and a lust's name step down a line
//! when they would meet another, and a word with no room left is dropped.
//! The words are the prototype's INK_3 in the window's text grade of it
//! (`theme::INK_3_TEXT`), as the inspector's axis is: words a reader must
//! read clear AA.
//!
//! A stored pull carries its raid timeline too (v35), rebuilt by the store
//! — its series coarser (10 s) or, with its details demoted, none: the
//! ribbon then draws the axis, the lust and the deaths alone. An arena's
//! enemy deaths are skulls in outline, never a solid one of the group's.
//!
//! Window-only: the overlay draws nothing of it.

use iced::widget::canvas::{self, Canvas, Path, Stroke};
use iced::widget::container;
use iced::{Color, Element, Font, Length, Point, Rectangle, Renderer, Size, Theme, mouse};

use wowdps_model::fmt::{commas, duration};
use wowdps_model::{LustWindow, RaidTimeline, View};

use crate::deaths::Pick;
use crate::table::figure;
use crate::theme;
use crate::view::display_name;
use crate::window::{Gui, Message};

/// The ribbon's height (`.ribbon{height:86px}`, 74 at 820 px and under),
/// its 1 px top rule included, and its side margins (`margin:0 18px`,
/// `0 12px`).
const H: f32 = 86.0;
const H_NARROW: f32 = 74.0;
const MARGIN: f32 = 18.0;
const MARGIN_NARROW: f32 = 12.0;

/// The plot (`.plot{top:8px;height:58px}`, 46 narrow), under the rule.
const PLOT_TOP: f32 = 1.0 + 8.0;
const PLOT_H: f32 = 58.0;
const PLOT_H_NARROW: f32 = 46.0;
/// The axis (`.axis{bottom:2px;height:16px}`, `span{font-size:11.5px}`).
const AXIS_BOTTOM: f32 = 2.0;
const AXIS_H: f32 = 16.0;
const TICK_PX: f32 = 11.5;
/// The peak stands this far under the plot's top (`H - v/max*(H-4)`).
const PEAK_INSET: f32 = 4.0;
/// The peak's words (`.ymax{top:0;left:4px;font-size:11.5px}`).
const PEAK_X: f32 = 4.0;
/// The curve: the area's ink at its top and its foot (the gradient's
/// `.26` → `.02`), the line's (`stroke-opacity=".8"`, `1.4` wide).
const AREA_TOP: f32 = 0.26;
const AREA_FOOT: f32 = 0.02;
const LINE_ALPHA: f32 = 0.8;
const LINE_W: f32 = 1.4;
/// A lust window (`.band{background:rgba(237,233,223,.05);border-left:1px
/// solid rgba(237,233,223,.18)}`) and its name (`b{top:-1px;left:5px;
/// font-size:11.5px;font-weight:500}`).
const BAND_ALPHA: f32 = 0.05;
const BAND_EDGE_ALPHA: f32 = 0.18;
const BAND_WORDS_X: f32 = 5.0;
const BAND_WORDS_Y: f32 = -1.0;
/// A death (`.dmark{width:16px;height:16px;bottom:-3px}`, its skull
/// `13px`), its hairline (`::before{bottom:14px;height:48px}`, 36 narrow,
/// `rgba(255,92,99,.28)`) and the owner's word over it (`.me::after{top:
/// -50px;font-size:11px;font-weight:600}`, −38 narrow).
const SKULL_BOX: f32 = 16.0;
/// What a pointer must come within to press a skull: the 24 px target
/// WCAG 2.5.8 asks for, centred on it — the nearest skull wins where two
/// such targets overlap, so a stack of deaths is still every skull's.
const SKULL_REACH: f32 = 12.0;
const SKULL: f32 = 13.0;
/// An enemy's skull (an arena's other team, R13): its outline alone.
const SKULL_OUTLINE: f32 = 1.2;
const SKULL_DROP: f32 = 3.0;
const MARK_LINE_GAP: f32 = 14.0;
const MARK_LINE_H: f32 = 48.0;
const MARK_LINE_H_NARROW: f32 = 36.0;
const MARK_LINE_ALPHA: f32 = 0.28;
/// The open death's hairline — the Deaths view's selection — lit beyond
/// the prototype's glow alone, which a 13 px skull wears too faintly to
/// tell which one is open.
const MARK_LINE_ON_ALPHA: f32 = 0.8;
const YOU_PX: f32 = 11.0;
const YOU_ABOVE: f32 = 50.0;
const YOU_ABOVE_NARROW: f32 = 38.0;
/// A hovered or selected skull glows in its colour (`drop-shadow(0 0 4px
/// var(--c))`): its outline stroked wide and faint, then narrower and
/// stronger, under the solid skull — a halo that hugs the skull and falls
/// off like the 4 px blur, widest first.
const GLOW: [(f32, f32); 3] = [(6.0, 0.06), (4.0, 0.12), (2.0, 0.25)];
/// A word inside the plot stands on a plate of the ground under it, this
/// much wider than the word each side, its corners this round.
const PLATE_PAD: f32 = 3.0;
const PLATE_RADIUS: f32 = 3.0;
/// The crosshair (`.xhair{background:rgba(242,193,75,.55)}`) and every
/// hairline here.
const XHAIR_ALPHA: f32 = 0.55;
const HAIRLINE: f32 = 1.0;
/// The tooltip (`.tip{font-size:13px;padding:5px 8px;border-radius:6px}`),
/// beside the crosshair or over a skull, never under the pointer.
const TIP_PX: f32 = 13.0;
const TIP_PAD: (f32, f32) = (8.0, 5.0);
const TIP_LINE: f32 = 17.0;
const TIP_RADIUS: f32 = 6.0;
const TIP_OFF_X: f32 = 12.0;
const TIP_OFF_Y: f32 = 4.0;
/// A one-line text box's height for its size: iced's default line height.
const LINE: f32 = 1.3;
/// The curve's rate is taken over this many of the daemon's 1 s buckets
/// (the prototype's 10 s), fewer in a fight too short to show a shape.
const STEP_BUCKETS: usize = 10;
const MIN_POINTS: usize = 12;
/// The tick labels' own room: the axis steps up from minutes when they
/// would crowd it (the inspector's rule).
const TICK_FIRST: f32 = 0.001;
const TICK_LAST: f32 = 0.96;

/// Everything the ribbon draws, owned — built from the snapshot, laid out
/// at whatever width the stage gives it.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Ribbon {
    /// What the curve is, as the peak's words name it: "Raid dps".
    pub word: String,
    /// The raid's rate, one value per `step_ms` from the fight's start.
    pub rate: Vec<f64>,
    pub step_ms: u32,
    /// The stretch the ribbon spans, ms from the fight's start.
    pub span_ms: u32,
    pub skulls: Vec<Skull>,
    pub lust: Vec<LustWindow>,
}

/// One death on the ribbon.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Skull {
    pub at_ms: i64,
    /// What a press opens.
    pub pick: Pick,
    /// Who, as the window shows names ("Tueur").
    pub name: String,
    /// The class colour, raw: a skull is data, as a bar is.
    pub color: Color,
    /// The owner's text colour — their name in the tooltip, and "you".
    pub text: Color,
    /// "died 1:10, Venom Rupture".
    pub words: String,
    /// One of the reader's own characters: labelled "you".
    pub mine: bool,
    /// An arena's other team (R13): drawn in outline, never as one of the
    /// group's.
    pub enemy: bool,
    /// The death the Deaths view has selected: it glows.
    pub on: bool,
}

impl Ribbon {
    /// The ribbon for the pull on the stage — `None` without a raid
    /// timeline (nothing watched yet; a stored pull whose store kept only
    /// its card).
    pub(crate) fn of(state: &Gui) -> Option<Self> {
        let app = state.fight();
        let raid = app.raid()?;
        let hide = state.cfg.hide_realms;
        let selected = (app.view == View::Deaths)
            .then(|| crate::deaths::selected(app, raid))
            .flatten();
        let owner = crate::deaths::owner(state, raid);
        Some(Self::from_raid(
            raid,
            app.duration_ms(),
            selected,
            hide,
            owner.as_deref(),
        ))
    }

    /// The same from the parts: `raid`, the fight's `duration_ms`, the
    /// death the Deaths view has selected (its place in `raid.deaths`), and
    /// the reader's guid when the daemon marked none of the deaths
    /// ([`crate::deaths::owner`]).
    pub(crate) fn from_raid(
        raid: &RaidTimeline,
        duration_ms: i64,
        selected: Option<usize>,
        hide_realms: bool,
        owner: Option<&str>,
    ) -> Self {
        let bucket = i64::from(raid.bucket_ms.max(1));
        // The fight's clock, or further when the series, a death or a lust
        // runs past it — a visit's Σ is timed by its members' combat but
        // drawn on the visit's wall clock.
        let span = [
            duration_ms,
            raid.series.len() as i64 * bucket,
            raid.deaths.iter().map(|d| d.at_ms).max().unwrap_or(0),
            raid.lust
                .iter()
                .map(|w| w.at_ms + w.dur_ms)
                .max()
                .unwrap_or(0),
        ]
        .into_iter()
        .max()
        .unwrap_or(0)
        .clamp(1, i64::from(u32::MAX));
        let (step_ms, rate) = rates(&raid.series, raid.bucket_ms.max(1), span);
        let word = format!("Raid {}", crate::view::rate_label(raid.view));
        let skulls = raid
            .deaths
            .iter()
            .enumerate()
            .map(|(i, d)| {
                let name = if hide_realms {
                    display_name(&d.name).to_string()
                } else {
                    d.name.clone()
                };
                // As the Deaths table words it: a cheat death "ran out".
                let blow = crate::deaths::words(d, hide_realms).blow;
                Skull {
                    at_ms: d.at_ms,
                    pick: Pick {
                        key: d.guid.clone(),
                        label: d.name.clone(),
                        index: d.index,
                    },
                    words: if d.enemy {
                        format!("of the enemy team, died {}, {blow}", duration(d.at_ms))
                    } else {
                        format!("died {}, {blow}", duration(d.at_ms))
                    },
                    name,
                    color: d.class.map_or(crate::view::CLASSLESS, theme::class_rgb),
                    text: d.class.map_or(theme::INK, theme::you_text),
                    mine: crate::deaths::is_mine(d, owner),
                    enemy: d.enemy,
                    on: selected == Some(i),
                }
            })
            .collect();
        Ribbon {
            word,
            rate,
            step_ms,
            span_ms: span as u32,
            skulls,
            lust: raid.lust.clone(),
        }
    }

    /// The highest rate drawn — what the peak's words name.
    pub(crate) fn peak(&self) -> f64 {
        self.rate.iter().copied().fold(0.0, f64::max)
    }

    /// "Raid dps, peak 10.7M".
    pub(crate) fn peak_words(&self) -> String {
        format!("{}, peak {}", self.word, figure(self.peak().round() as u64))
    }

    /// The ribbon laid out: `narrow` is the window's own breakpoint.
    pub(crate) fn view(self, narrow: bool) -> Element<'static, Message> {
        let (h, margin) = if narrow {
            (H_NARROW, MARGIN_NARROW)
        } else {
            (H, MARGIN)
        };
        container(
            Canvas::new(Plot {
                ribbon: self,
                narrow,
            })
            .width(Length::Fill)
            .height(Length::Fixed(h)),
        )
        .padding([0.0, margin])
        .width(Length::Fill)
        .into()
    }
}

/// The raid's rate from the series: 10 s at a time (`STEP_BUCKETS` of the
/// live 1 s buckets, fewer in a short fight; a stored pull's coarse 10 s
/// buckets one each), each over the buckets it holds — the last one may
/// hold fewer — in amount per second. `(step_ms, rates)`.
fn rates(series: &[u64], bucket_ms: u32, span_ms: i64) -> (u32, Vec<f64>) {
    let buckets = (span_ms / i64::from(bucket_ms)).max(1) as usize;
    let most = (STEP_BUCKETS * 1000 / bucket_ms.max(1) as usize).max(1);
    let per = (buckets / MIN_POINTS).clamp(1, most);
    let secs = f64::from(bucket_ms) / 1000.0;
    let rate = series
        .chunks(per)
        .map(|c| c.iter().sum::<u64>() as f64 / (c.len() as f64 * secs))
        .collect();
    (bucket_ms * per as u32, rate)
}

/// The axis's ticks over `span_ms`, `w` wide: whole minutes, as the
/// prototype's `axisTicks()` steps them — the inspector's wider steps when
/// minutes would crowd the axis, and its finer ones only in a pull too
/// short to hold two minutes, which would otherwise read no time at all.
fn minute_ticks(span_ms: u32, w: f32) -> Vec<u32> {
    let ticks = crate::inspector::ticks((0, span_ms), w);
    let fine = ticks.get(1).is_some_and(|t| *t < MINUTE);
    if !fine || span_ms < 2 * MINUTE {
        return ticks;
    }
    (0..=span_ms).step_by(MINUTE as usize).collect()
}

const MINUTE: u32 = 60_000;

/// The canvas: the ribbon and the breakpoint it is drawn at.
struct Plot {
    ribbon: Ribbon,
    narrow: bool,
}

/// What the pointer is over.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Hover {
    /// The plot, at this x.
    Plot(f32),
    /// A skull, by its place in `Ribbon::skulls`.
    Skull(usize),
}

/// The canvas's own memory: what the pointer was last over.
#[derive(Default)]
pub(crate) struct State {
    hover: Option<Hover>,
}

/// One line of canvas text, placed and measured.
struct Label {
    words: String,
    at: Point,
    px: f32,
    color: Color,
    font: Font,
    width: f32,
    /// Its plate is drawn, over the curve and the hairlines, so no line
    /// runs through it — the peak's words and "you". A lust's name has
    /// none (`.band b` has no background, under the curve): a plate there
    /// would cover the burst, the curve's peak, where the peak's words
    /// send the eye.
    cover: bool,
}

impl Label {
    fn rect(&self) -> Rectangle {
        Rectangle::new(self.at, Size::new(self.width, self.px * LINE))
    }

    /// The plate under it: the word's box, [`PLATE_PAD`] wider each side.
    fn plate_rect(&self) -> Rectangle {
        let r = self.rect();
        Rectangle::new(
            Point::new(r.x - PLATE_PAD, r.y),
            Size::new(r.width + 2.0 * PLATE_PAD, r.height),
        )
    }
}

/// The tooltip, placed: its box and its one line, in pieces of their own
/// face (the time or the name in 600, the rest regular).
struct Tip {
    rect: Rectangle,
    pieces: Vec<(String, Color, Font)>,
}

fn overlaps(a: Rectangle, b: Rectangle) -> bool {
    a.x < b.x + b.width && b.x < a.x + a.width && a.y < b.y + b.height && b.y < a.y + a.height
}

impl Plot {
    fn plot_h(&self) -> f32 {
        if self.narrow { PLOT_H_NARROW } else { PLOT_H }
    }

    fn plot_bottom(&self) -> f32 {
        PLOT_TOP + self.plot_h()
    }

    fn height(&self) -> f32 {
        if self.narrow { H_NARROW } else { H }
    }

    /// The canvas x of `ms` from the fight's start, `w` wide.
    fn x_of(&self, ms: f64, w: f32) -> f32 {
        (ms / f64::from(self.ribbon.span_ms.max(1))) as f32 * w
    }

    fn ms_at(&self, x: f32, w: f32) -> u32 {
        let frac = (x / w.max(1.0)).clamp(0.0, 1.0);
        (f64::from(frac) * f64::from(self.ribbon.span_ms)) as u32
    }

    /// The plot's y for a rate: the peak [`PEAK_INSET`] under its top.
    fn y_of(&self, v: f64) -> f32 {
        let peak = self.ribbon.peak();
        let foot = self.plot_bottom();
        if peak <= 0.0 {
            return foot;
        }
        foot - (v / peak).clamp(0.0, 1.0) as f32 * (self.plot_h() - PEAK_INSET)
    }

    /// A skull's box: centred on its moment, standing on the plot's foot
    /// and [`SKULL_DROP`] past it.
    fn skull_box(&self, s: &Skull, w: f32) -> Rectangle {
        let x = self.x_of(s.at_ms as f64, w);
        let bottom = self.plot_bottom() + SKULL_DROP;
        Rectangle::new(
            Point::new(x - SKULL_BOX / 2.0, bottom - SKULL_BOX),
            Size::new(SKULL_BOX, SKULL_BOX),
        )
    }

    /// What the pointer at `p` is over: a skull first — the nearest whose
    /// 24 px target ([`SKULL_REACH`] each way from its centre) holds `p`,
    /// the latest drawn of equals — else the plot.
    fn hover_at(&self, p: Point, w: f32) -> Option<Hover> {
        let near = self
            .ribbon
            .skulls
            .iter()
            .enumerate()
            .filter_map(|(i, s)| {
                let c = self.skull_box(s, w).center();
                let (dx, dy) = ((p.x - c.x).abs(), (p.y - c.y).abs());
                (dx <= SKULL_REACH && dy <= SKULL_REACH).then_some((i, dx + dy))
            })
            .min_by(|a, b| a.1.total_cmp(&b.1).then(b.0.cmp(&a.0)));
        if let Some((i, _)) = near {
            return Some(Hover::Skull(i));
        }
        let inside = p.y >= PLOT_TOP && p.y <= self.plot_bottom() && p.x >= 0.0 && p.x <= w;
        inside.then_some(Hover::Plot(p.x))
    }

    /// The curve's points, as the prototype's `curve()` lays them: each
    /// rate at the middle of its step, one at the plot's left edge holding
    /// the first, and one at its right edge holding the last when the
    /// series runs to the end of the fight.
    fn points(&self, w: f32) -> Vec<Point> {
        let step = f64::from(self.ribbon.step_ms.max(1));
        let span = f64::from(self.ribbon.span_ms.max(1));
        let mut pts: Vec<Point> = self
            .ribbon
            .rate
            .iter()
            .enumerate()
            .map(|(i, v)| {
                let ms = ((i as f64 + 0.5) * step).min(span);
                Point::new(self.x_of(ms, w), self.y_of(*v))
            })
            .collect();
        if let Some(first) = pts.first().copied() {
            pts.insert(0, Point::new(0.0, first.y));
        }
        let reach = self.ribbon.rate.len() as f64 * step;
        if let Some(last) = pts.last().copied()
            && reach >= span - 5_000.0
        {
            pts.push(Point::new(w, last.y));
        }
        pts
    }

    /// The tooltip for `hover`: beside the crosshair (flipped left at the
    /// edge), or over a skull — held inside the canvas.
    fn tip(&self, hover: Hover, w: f32) -> Option<Tip> {
        let (pieces, anchor) = match hover {
            Hover::Plot(x) => {
                let ms = self.ms_at(x, w);
                let v = self
                    .ribbon
                    .rate
                    .get((ms / self.ribbon.step_ms.max(1)) as usize)
                    .copied()
                    .unwrap_or(0.0);
                (
                    vec![
                        (duration(i64::from(ms)), theme::INK, theme::UI_SEMIBOLD),
                        (
                            format!(
                                "  {} {}",
                                self.ribbon.word.to_lowercase(),
                                commas(v.max(0.0).round() as u64)
                            ),
                            theme::INK,
                            theme::UI,
                        ),
                    ],
                    None,
                )
            }
            Hover::Skull(i) => {
                let s = self.ribbon.skulls.get(i)?;
                (
                    vec![
                        (s.name.clone(), s.text, theme::UI_SEMIBOLD),
                        (format!(" {}", s.words), theme::INK, theme::UI),
                    ],
                    Some(self.skull_box(s, w)),
                )
            }
        };
        let tw = pieces
            .iter()
            .map(|(s, _, f)| text_w(s, TIP_PX, *f))
            .sum::<f32>()
            + 2.0 * TIP_PAD.0;
        let th = TIP_LINE + 2.0 * TIP_PAD.1;
        let mut at = match (hover, anchor) {
            (_, Some(b)) => Point::new(b.center_x() - tw / 2.0, b.y - th - TIP_OFF_Y),
            (Hover::Plot(x), None) if x + TIP_OFF_X + tw > w => {
                Point::new(x - TIP_OFF_X - tw, PLOT_TOP + TIP_OFF_Y)
            }
            (Hover::Plot(x), None) => Point::new(x + TIP_OFF_X, PLOT_TOP + TIP_OFF_Y),
            (Hover::Skull(_), None) => return None,
        };
        at.x = at.x.clamp(0.0, (w - tw).max(0.0));
        at.y = at.y.clamp(0.0, (self.height() - th).max(0.0));
        Some(Tip {
            rect: Rectangle::new(at, Size::new(tw, th)),
            pieces,
        })
    }

    /// Everything the ribbon says besides a tooltip — the owner's "you",
    /// the peak, each lust's name, the ticks — placed so no two meet, less
    /// what the tooltip for `hover` would cover.
    ///
    /// The placement pass: "you" stands over its skull, where it points;
    /// the peak takes the plot's top-left, or the line under it when a
    /// "you" is there; each lust's name its band's top-left, else one of
    /// the two lines under it, held inside the canvas — and a word with no
    /// spot free is left out rather than printed over another. The ticks
    /// sit on the axis under the plot, where nothing else is.
    fn labels(&self, hover: Option<Hover>, w: f32) -> Vec<Label> {
        let mut out: Vec<Label> = Vec::new();
        let label = |words: String, at: Point, px: f32, color: Color, font: Font| {
            let width = text_w(&words, px, font);
            Label {
                words,
                at,
                px,
                color,
                font,
                width,
                cover: true,
            }
        };
        // Where a placed word may go: the first of `spots` (each a top-left)
        // whose plate meets no word already placed, held inside the canvas.
        let place = |out: &mut Vec<Label>, mut l: Label, spots: &[Point]| {
            for spot in spots {
                l.at = Point::new(
                    spot.x
                        .clamp(PLATE_PAD, (w - l.width - PLATE_PAD).max(PLATE_PAD)),
                    spot.y,
                );
                let r = l.plate_rect();
                if !out.iter().any(|o| overlaps(o.plate_rect(), r)) {
                    out.push(l);
                    return;
                }
            }
        };
        let line = TICK_PX * LINE;
        let above = if self.narrow {
            YOU_ABOVE_NARROW
        } else {
            YOU_ABOVE
        };
        for s in self.ribbon.skulls.iter().filter(|s| s.mine) {
            let b = self.skull_box(s, w);
            let l = label(
                "you".to_string(),
                Point::ORIGIN,
                YOU_PX,
                s.text,
                theme::UI_SEMIBOLD,
            );
            let at = Point::new(b.center_x() - l.width / 2.0, b.y - above);
            place(&mut out, l, &[at]);
        }
        if !self.ribbon.rate.is_empty() {
            let l = label(
                self.ribbon.peak_words(),
                Point::ORIGIN,
                TICK_PX,
                theme::INK_3_TEXT,
                theme::UI,
            );
            let top = Point::new(PEAK_X, PLOT_TOP);
            place(&mut out, l, &[top, Point::new(top.x, top.y + line)]);
        }
        for band in &self.ribbon.lust {
            let l = Label {
                cover: false,
                ..label(
                    band.label.clone(),
                    Point::ORIGIN,
                    TICK_PX,
                    theme::INK_3_TEXT,
                    theme::UI_MEDIUM,
                )
            };
            let top = Point::new(
                self.x_of(band.at_ms as f64, w) + BAND_WORDS_X,
                PLOT_TOP + BAND_WORDS_Y,
            );
            // The lines under the first stand on the peak's grid, so a name
            // stepped down meets no word stepped down beside it.
            let lines =
                [top.y, PLOT_TOP + line, PLOT_TOP + 2.0 * line].map(|y| Point::new(top.x, y));
            place(&mut out, l, &lines);
        }
        let axis_y = self.height() - AXIS_BOTTOM - AXIS_H;
        for t in minute_ticks(self.ribbon.span_ms, w) {
            let words = duration(i64::from(t));
            let tw = text_w(&words, TICK_PX, theme::UI);
            let x = self.x_of(f64::from(t), w);
            let frac = x / w.max(1.0);
            let at = if frac <= TICK_FIRST {
                x
            } else if frac > TICK_LAST {
                x - tw
            } else {
                x - tw / 2.0
            };
            out.push(Label {
                cover: false,
                ..label(
                    words,
                    Point::new(at, axis_y),
                    TICK_PX,
                    theme::INK_3_TEXT,
                    theme::UI,
                )
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

    /// The ground a plate at `r` stands on: the ribbon's own, or a lust
    /// band's wash where the word starts inside one.
    fn ground_at(&self, r: Rectangle, w: f32) -> Color {
        let banded = self.ribbon.lust.iter().any(|band| {
            let x1 = self.x_of(band.at_ms as f64, w);
            let x2 = self.x_of((band.at_ms + band.dur_ms) as f64, w);
            r.x + PLATE_PAD >= x1 && r.x + PLATE_PAD < x2
        });
        if banded {
            mix(theme::GROUND, theme::INK, BAND_ALPHA)
        } else {
            theme::GROUND
        }
    }
}

/// `a` with `t` of `b` over it, opaque — a wash of `b` at alpha `t` on `a`.
fn mix(a: Color, b: Color, t: f32) -> Color {
    Color::from_rgb(
        a.r + (b.r - a.r) * t,
        a.g + (b.g - a.g) * t,
        a.b + (b.b - a.b) * t,
    )
}

/// The skull (`SKULL`'s path in the prototype, a 12-unit box): a rounded
/// dome over a jaw, its two eyes holes in it — `scale` px a unit, `at` its
/// top-left. `eyes: false` is its outline alone, what a glow hugs.
fn skull(at: Point, scale: f32, eyes: bool) -> Path {
    let p = |x: f32, y: f32| Point::new(at.x + x * scale, at.y + y * scale);
    Path::new(|b| {
        b.move_to(p(6.0, 0.9));
        b.bezier_curve_to(p(3.1, 0.9), p(1.3, 2.8), p(1.3, 5.2));
        b.bezier_curve_to(p(1.3, 6.7), p(2.0, 7.8), p(3.0, 8.4));
        b.line_to(p(3.0, 10.8));
        b.line_to(p(9.0, 10.8));
        b.line_to(p(9.0, 8.4));
        b.bezier_curve_to(p(10.0, 7.8), p(10.7, 6.7), p(10.7, 5.2));
        b.bezier_curve_to(p(10.7, 2.8), p(8.9, 0.9), p(6.0, 0.9));
        b.close();
        if eyes {
            b.circle(p(4.3, 5.5), 1.1 * scale);
            b.circle(p(7.7, 5.5), 1.1 * scale);
        }
    })
}

/// Trace `pts` into `path` as the prototype's `smooth()` does: a
/// Catmull-Rom spline as cubic Béziers, held inside the plot.
fn smooth(path: &mut canvas::path::Builder, pts: &[Point], top: f32, foot: f32) {
    let hold = |p: Point| Point::new(p.x, p.y.clamp(top, foot));
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

impl canvas::Program<Message> for Plot {
    type State = State;

    fn update(
        &self,
        state: &mut State,
        event: &canvas::Event,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Option<canvas::Action<Message>> {
        use iced::mouse::{Button, Event as Mouse};
        let iced::Event::Mouse(mouse) = event else {
            return None;
        };
        let w = bounds.width;
        let hover = cursor.position_in(bounds).and_then(|p| self.hover_at(p, w));
        match mouse {
            Mouse::ButtonPressed(Button::Left) => {
                let Some(Hover::Skull(i)) = hover else {
                    return None;
                };
                let pick = self.ribbon.skulls.get(i)?.pick.clone();
                Some(canvas::Action::publish(Message::OpenDeath(pick)).and_capture())
            }
            Mouse::CursorMoved { .. } | Mouse::CursorLeft => {
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
        _state: &State,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> mouse::Interaction {
        match cursor
            .position_in(bounds)
            .and_then(|p| self.hover_at(p, bounds.width))
        {
            Some(Hover::Skull(_)) => mouse::Interaction::Pointer,
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
        let (top, foot) = (PLOT_TOP, self.plot_bottom());
        let hairline = |c: Color| Stroke::default().with_width(HAIRLINE).with_color(c);

        // The rule over the ribbon (`border-top:1px solid var(--line)`).
        frame.stroke(
            &Path::line(Point::new(0.0, 0.5), Point::new(w, 0.5)),
            hairline(theme::LINE),
        );

        // The lust windows: a faint wash of the ink, its left edge a rule.
        for l in &self.ribbon.lust {
            let x1 = self.x_of(l.at_ms as f64, w);
            let x2 = self.x_of((l.at_ms + l.dur_ms) as f64, w).min(w);
            if x2 <= x1 {
                continue;
            }
            frame.fill_rectangle(
                Point::new(x1, top),
                Size::new(x2 - x1, self.plot_h()),
                Color {
                    a: BAND_ALPHA,
                    ..theme::INK
                },
            );
            frame.stroke(
                &Path::line(Point::new(x1 + 0.5, top), Point::new(x1 + 0.5, foot)),
                hairline(Color {
                    a: BAND_EDGE_ALPHA,
                    ..theme::INK
                }),
            );
        }

        // The curve: an area fading down its height, under the line.
        let pts = self.points(w);
        if let (Some(first), Some(last)) = (pts.first(), pts.last()) {
            let mut area = canvas::path::Builder::new();
            area.move_to(Point::new(first.x, foot));
            area.line_to(*first);
            smooth(&mut area, &pts, top, foot);
            area.line_to(Point::new(last.x, foot));
            area.close();
            let fade = canvas::gradient::Linear::new(Point::new(0.0, top), Point::new(0.0, foot))
                .add_stop(
                    0.0,
                    Color {
                        a: AREA_TOP,
                        ..theme::INK
                    },
                )
                .add_stop(
                    1.0,
                    Color {
                        a: AREA_FOOT,
                        ..theme::INK
                    },
                );
            frame.fill(&area.build(), fade);
            let mut line = canvas::path::Builder::new();
            line.move_to(*first);
            smooth(&mut line, &pts, top, foot);
            frame.stroke(
                &line.build(),
                Stroke::default()
                    .with_width(LINE_W)
                    .with_color(Color {
                        a: LINE_ALPHA,
                        ..theme::INK
                    })
                    .with_line_join(canvas::LineJoin::Round),
            );
        }

        // The deaths: each a faint red hairline up the plot, and its skull
        // on the foot in the dead player's colour — glowing when hovered
        // or selected; an enemy's (an arena's other team) in outline.
        let line_h = if self.narrow {
            MARK_LINE_H_NARROW
        } else {
            MARK_LINE_H
        };
        let hovered = match state.hover {
            Some(Hover::Skull(i)) => Some(i),
            _ => None,
        };
        let scale = SKULL / 12.0;
        for (i, s) in self.ribbon.skulls.iter().enumerate() {
            let b = self.skull_box(s, w);
            let x = b.center_x().round() + 0.5;
            let low = b.y + b.height - MARK_LINE_GAP;
            frame.stroke(
                // Held inside the plot: its top a pixel over it would show
                // above a word's plate there. The open death's is lit, so
                // which skull the recap is about reads at a glance.
                &Path::line(Point::new(x, (low - line_h).max(top)), Point::new(x, low)),
                hairline(Color {
                    a: if s.on {
                        MARK_LINE_ON_ALPHA
                    } else {
                        MARK_LINE_ALPHA
                    },
                    ..theme::BAD
                }),
            );
            let at = Point::new(b.center_x() - SKULL / 2.0, b.center_y() - SKULL / 2.0);
            if s.on || hovered == Some(i) {
                let outline = skull(at, scale, false);
                for (width, a) in GLOW {
                    frame.stroke(
                        &outline,
                        Stroke::default()
                            .with_width(width)
                            .with_color(Color { a, ..s.color })
                            .with_line_join(canvas::LineJoin::Round),
                    );
                }
            }
            if s.enemy {
                frame.stroke(
                    &skull(at, scale, true),
                    Stroke::default()
                        .with_width(SKULL_OUTLINE)
                        .with_color(s.color),
                );
            } else {
                frame.fill(
                    &skull(at, scale, true),
                    canvas::Fill {
                        style: canvas::Style::Solid(s.color),
                        rule: canvas::fill::Rule::EvenOdd,
                    },
                );
            }
        }

        // Every word the tooltip leaves clear — the peak's and "you" on
        // their plates, over the curve and the hairlines (a canvas sets its
        // text over every shape, so a plate is what keeps a line out of a
        // glyph); a lust's name on none, so the burst under it stays in
        // sight.
        let labels = self.labels(state.hover, w);
        for l in labels.iter().filter(|l| l.cover) {
            let r = l.plate_rect();
            frame.fill(
                &Path::rounded_rectangle(r.position(), r.size(), PLATE_RADIUS.into()),
                self.ground_at(r, w),
            );
        }

        // The crosshair, in gold (`.xhair`).
        if let Some(Hover::Plot(x)) = state.hover {
            frame.stroke(
                &Path::line(Point::new(x, top), Point::new(x, foot)),
                hairline(Color {
                    a: XHAIR_ALPHA,
                    ..theme::GOLD
                }),
            );
        }

        // The words, then the tooltip over all.
        for l in labels {
            words(&mut frame, &l.words, l.at, l.px, l.color, l.font);
        }
        if let Some(tip) = state.hover.and_then(|h| self.tip(h, w)) {
            let shape =
                Path::rounded_rectangle(tip.rect.position(), tip.rect.size(), TIP_RADIUS.into());
            frame.fill(&shape, theme::SURFACE);
            frame.stroke(&shape, hairline(theme::EDGE));
            let mut x = tip.rect.x + TIP_PAD.0;
            for (s, color, font) in &tip.pieces {
                words(
                    &mut frame,
                    s,
                    Point::new(x, tip.rect.y + TIP_PAD.1),
                    TIP_PX,
                    *color,
                    *font,
                );
                x += text_w(s, TIP_PX, *font);
            }
        }
        vec![frame.into_geometry()]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::window::testkit::{self as tk, pixels, pixels_at, simulator_as};

    /// The prototype's kill, near enough: 7:02, six deaths, one Heroism.
    fn ribbon() -> Ribbon {
        Ribbon::from_raid(&tk::raid_timeline(), 422_040, None, true, None)
    }

    /// The ribbon at `w` × its height, drawn as the app draws.
    fn ui(r: Ribbon, w: f32, narrow: bool) -> iced_test::Simulator<'static, Message> {
        let h = if narrow { H_NARROW } else { H };
        simulator_as(crate::window::settings(), Size::new(w, h), r.view(narrow))
    }

    /// Where a skull stands in a ribbon `w` wide (its margins included).
    fn skull_at(r: &Ribbon, i: usize, w: f32) -> Point {
        let plot = Plot {
            ribbon: r.clone(),
            narrow: false,
        };
        let inner = w - 2.0 * MARGIN;
        let b = plot.skull_box(&r.skulls[i], inner);
        Point::new(MARGIN + b.center_x(), b.center_y())
    }

    /// The curve is the raid's rate over 10 s steps, each over the seconds
    /// it holds — so the steps give back every amount of the series — and
    /// its peak is named in the prototype's words.
    #[test]
    fn the_curve_is_the_raid_rate_in_ten_second_steps() {
        let r = ribbon();
        assert_eq!(r.word, "Raid dps");
        assert_eq!(r.step_ms, 10_000);
        assert_eq!(r.rate.len(), 43, "422 s in tens, the last short");
        let raid = tk::raid_timeline();
        let total: u64 = raid.series.iter().sum();
        let back: f64 = raid
            .series
            .chunks(10)
            .zip(&r.rate)
            .map(|(c, v)| v * c.len() as f64)
            .sum();
        assert!((back - total as f64).abs() < 1.0, "{back} vs {total}");
        let peak = r.peak();
        assert!(
            peak > 9_000_000.0 && peak < 10_500_000.0,
            "the Heroism's {peak}"
        );
        assert_eq!(
            r.peak_words(),
            format!("Raid dps, peak {}", figure(peak.round() as u64))
        );
        // The axis in minutes, wider when they crowd, finer only in a pull
        // too short to hold two.
        assert_eq!(
            minute_ticks(422_040, 1164.0),
            (0..=7).map(|m| m * 60_000).collect::<Vec<_>>()
        );
        assert_eq!(minute_ticks(30 * 60_000, 300.0).get(1), Some(&300_000));
        assert!(
            minute_ticks(90_000, 1164.0)
                .get(1)
                .is_some_and(|t| *t < 60_000)
        );
        // A 30 s pull still has a shape: steps of 2 s.
        let (step, rate) = rates(&[100; 30], 1000, 30_000);
        assert_eq!((step, rate.len()), (2_000, 15));
        assert_eq!(rate[0], 100.0);
    }

    /// Each death is a skull in its player's class colour, worded with its
    /// time and its killing blow (or that no damage was logged), realms
    /// hidden as the options say; the owner's alone is "you".
    #[test]
    fn every_death_is_a_skull_and_the_owners_says_you() {
        let r = ribbon();
        assert_eq!(r.skulls.len(), 6);
        let owner = &r.skulls[4];
        assert_eq!(owner.name, "Raider16");
        assert_eq!(owner.words, "died 5:45, Coalesced Venom");
        assert!(owner.mine);
        assert_eq!(r.skulls.iter().filter(|s| s.mine).count(), 1);
        assert_eq!(r.skulls[2].words, "died 3:01, No damage logged");
        let class = tk::raid_timeline().deaths[4].class.unwrap();
        assert_eq!(owner.color, theme::class_rgb(class));
        // The Deaths view's selection glows; nothing else does.
        let lit = Ribbon::from_raid(&tk::raid_timeline(), 422_040, Some(4), true, None);
        let on: Vec<bool> = lit.skulls.iter().map(|s| s.on).collect();
        assert_eq!(on, [false, false, false, false, true, false]);
        // A daemon that marks nobody (its store off): the window's own
        // owner — the guid `deaths::owner` resolved — is "you" all the same.
        let mut unmarked = tk::raid_timeline();
        for d in &mut unmarked.deaths {
            d.mine = false;
        }
        let guid = unmarked.deaths[4].guid.clone();
        let r = Ribbon::from_raid(&unmarked, 422_040, None, true, Some(&guid));
        let mine: Vec<bool> = r.skulls.iter().map(|s| s.mine).collect();
        assert_eq!(mine, [false, false, false, false, true, false]);
    }

    /// The words a canvas sets, as [`Plot::labels`] places them.
    fn said(plot: &Plot, hover: Option<Hover>, w: f32) -> Vec<String> {
        plot.labels(hover, w).into_iter().map(|l| l.words).collect()
    }

    /// Drawn: the peak's words, the Heroism's name, the minute ticks and
    /// the owner's "you" — and the skulls in their colours. A tooltip keeps
    /// the words it would cover out of the picture.
    #[test]
    fn the_ribbon_draws_its_words_and_its_skulls() {
        let r = ribbon();
        let plot = Plot {
            ribbon: r.clone(),
            narrow: false,
        };
        let words = said(&plot, None, 1164.0);
        for w in [
            r.peak_words().as_str(),
            "Heroism",
            "you",
            "0:00",
            "3:00",
            "7:00",
        ] {
            assert!(words.iter().any(|s| s == w), "{w}: {words:?}");
        }
        assert!(!words.iter().any(|s| s == "0:15"), "minutes, not quarters");
        let near_peak = Some(Hover::Plot(20.0));
        assert!(
            !said(&plot, near_peak, 1164.0).contains(&r.peak_words()),
            "the tooltip covers the peak's words, which leave"
        );
        let class = tk::raid_timeline().deaths[4].class.unwrap();
        let at = skull_at(&r, 4, 1200.0);
        let px = pixels(
            r.view(false),
            Size::new(1200.0, H),
            &crate::theme::window_theme(),
        );
        // The picture is at a scale of 2.
        let (x, y) = ((at.x * 2.0) as u32, (at.y * 2.0) as u32);
        let area = (x - 12, y - 12, x + 12, y + 12);
        assert!(px.count_in(area, theme::class_rgb(class), 6) > 20);
    }

    /// A press on a skull opens that death; a press on the curve does
    /// nothing.
    #[test]
    fn a_skull_is_a_press_away_from_its_recap() {
        let r = ribbon();
        let mut sim = ui(r.clone(), 1200.0, false);
        sim.point_at(skull_at(&r, 4, 1200.0));
        let _ = sim.simulate(iced_test::simulator::click());
        let got: Vec<Message> = sim.into_messages().collect();
        match got.as_slice() {
            [Message::OpenDeath(pick)] => {
                assert_eq!(pick.key, "Player-1-16");
                assert_eq!(pick.index, 0);
            }
            other => panic!("{other:?}"),
        }
        let mut sim = ui(r, 1200.0, false);
        sim.point_at(Point::new(300.0, PLOT_TOP + 20.0));
        let _ = sim.simulate(iced_test::simulator::click());
        assert_eq!(sim.into_messages().count(), 0);
    }

    /// The pointer on the plot reads the time and the rate under it; on a
    /// skull, who died when and to what — held inside the ribbon.
    #[test]
    fn hovering_reads_the_time_and_the_rate_or_the_death() {
        let r = ribbon();
        let plot = Plot {
            ribbon: r.clone(),
            narrow: false,
        };
        let w = 1000.0;
        let x = plot.x_of(245_000.0, w);
        assert_eq!(
            plot.hover_at(Point::new(x, PLOT_TOP + 10.0), w),
            Some(Hover::Plot(x))
        );
        let tip = plot.tip(Hover::Plot(x), w).expect("a tip");
        assert_eq!(tip.pieces[0].0, "4:05");
        let v = r.rate[24];
        assert_eq!(
            tip.pieces[1].0,
            format!("  raid dps {}", commas(v.round() as u64))
        );
        assert!(tip.rect.x >= 0.0 && tip.rect.x + tip.rect.width <= w);
        let at = plot.skull_box(&r.skulls[0], w);
        assert_eq!(plot.hover_at(at.center(), w), Some(Hover::Skull(0)));
        let tip = plot.tip(Hover::Skull(0), w).expect("a tip");
        assert_eq!(tip.pieces[0].0, "Raider3");
        assert_eq!(tip.pieces[1].0, " died 1:10, Venom Rupture");
        assert!(tip.rect.y + tip.rect.height <= at.y, "over the skull");
        // Off the plot is nothing.
        assert_eq!(plot.hover_at(Point::new(x, 2.0), w), None);
    }

    /// A narrow window's ribbon is 74 px with a 46 px plot, as the
    /// prototype's `@container` rule has it.
    #[test]
    fn a_narrow_ribbon_is_shorter() {
        let plot = Plot {
            ribbon: ribbon(),
            narrow: true,
        };
        assert_eq!(plot.height(), 74.0);
        assert_eq!(plot.plot_bottom(), PLOT_TOP + 46.0);
        let words = said(&plot, None, 416.0);
        assert!(words.iter().any(|s| s == "Heroism"), "{words:?}");
        // The owner's word stands just over the plot's top, as the wide
        // ribbon's does.
        let you = plot
            .labels(None, 416.0)
            .into_iter()
            .find(|l| l.words == "you")
            .map(|l| l.at.y);
        assert_eq!(you, Some(PLOT_TOP - 5.0));
        // Drawn at 440 px in its 74: the owner's skull in their colour.
        let r = ribbon();
        let class = tk::raid_timeline().deaths[4].class.unwrap();
        let px = pixels(
            r.view(true),
            Size::new(440.0, H_NARROW),
            &crate::theme::window_theme(),
        );
        assert_eq!(px.h, (H_NARROW * 2.0) as u32, "74 px tall");
        assert!(
            px.count(theme::class_rgb(class), 6) > 20,
            "the skull is drawn"
        );
    }

    /// A ribbon whose words all want the same corner: a lust at 0:02 and
    /// the owner dying at 0:20, over a curve — the prototype's normal case
    /// of lust on pull.
    fn crowded() -> Ribbon {
        let raid = RaidTimeline {
            view: View::Damage,
            bucket_ms: 1000,
            series: (0..300).map(|s| 1_000_000 + (s % 13) * 50_000).collect(),
            deaths: vec![wowdps_model::RaidDeath {
                guid: "Player-1-16".into(),
                name: "Raider16-Realm-US".into(),
                class: Some(wowdps_model::Class::Warlock),
                spec: None,
                index: 0,
                at_ms: 20_000,
                blow: "Coalesced Venom".into(),
                source: "Zul'jan".into(),
                hit: 8_883,
                overkill: Some(1_554),
                rez: None,
                mine: true,
                enemy: false,
            }],
            lust: vec![LustWindow {
                at_ms: 2_000,
                dur_ms: 40_000,
                label: "Heroism".into(),
            }],
        };
        Ribbon::from_raid(&raid, 300_000, None, true, None)
    }

    /// No two words meet: "you" holds its skull, the peak and the lust's
    /// name step down a line each rather than print over it or over each
    /// other — wide and narrow. The peak's words and "you" stand on plates,
    /// so no curve or hairline runs through them; the lust's name stands on
    /// none, so the burst it names stays in sight under it.
    #[test]
    fn the_ribbons_words_never_meet() {
        for (narrow, w) in [(false, 1164.0), (true, 416.0)] {
            let plot = Plot {
                ribbon: crowded(),
                narrow,
            };
            let labels = plot.labels(None, w);
            let words: Vec<&str> = labels.iter().map(|l| l.words.as_str()).collect();
            for want in ["you", "Heroism", "Raid dps, peak"] {
                assert!(
                    words.iter().any(|s| s.starts_with(want)),
                    "{narrow}: {want} in {words:?}"
                );
            }
            // The words placed in the plot: every one over the axis.
            let placed: Vec<&Label> = labels
                .iter()
                .filter(|l| l.at.y < plot.plot_bottom())
                .collect();
            for (i, a) in placed.iter().enumerate() {
                for b in placed.iter().skip(i + 1) {
                    assert!(
                        !overlaps(a.plate_rect(), b.plate_rect()),
                        "{narrow}: {} meets {}",
                        a.words,
                        b.words
                    );
                }
                // Inside the canvas and above the axis.
                let r = a.plate_rect();
                assert!(r.x >= 0.0 && r.x + r.width <= w, "{}", a.words);
                assert!(r.y + r.height <= plot.plot_bottom(), "{}", a.words);
            }
            let peak = labels.iter().find(|l| l.words.starts_with("Raid dps"));
            assert!(
                peak.is_some_and(|l| l.at.y > PLOT_TOP),
                "the peak stepped down"
            );
        }
        // The peak's words and "you" stand on plates; the lust's name on
        // none (`.band b`), so the burst under it — the curve's top, where
        // the peak's words send the eye — is never covered: the line's ink
        // shows inside the word's own box.
        let plot = Plot {
            ribbon: crowded(),
            narrow: false,
        };
        let labels = plot.labels(None, 1164.0);
        let covered = |w: &str| {
            labels
                .iter()
                .find(|l| l.words.starts_with(w))
                .map(|l| l.cover)
        };
        assert_eq!(covered("Raid dps"), Some(true));
        assert_eq!(covered("you"), Some(true));
        assert_eq!(covered("Heroism"), Some(false));
        let heroism = labels
            .iter()
            .find(|l| l.words == "Heroism")
            .map(Label::rect)
            .expect("placed");
        let px = pixels(
            crowded().view(false),
            Size::new(1164.0 + 2.0 * MARGIN, H),
            &crate::theme::window_theme(),
        );
        let box_px = (
            ((MARGIN + heroism.x) * 2.0) as u32,
            (heroism.y * 2.0) as u32,
            ((MARGIN + heroism.x + heroism.width) * 2.0) as u32,
            ((heroism.y + heroism.height) * 2.0) as u32,
        );
        let line = mix(
            mix(theme::GROUND, theme::INK, BAND_ALPHA),
            theme::INK,
            LINE_ALPHA,
        );
        assert!(
            px.count_in(box_px, line, 24) > 6,
            "the curve's line runs on under the lust's name"
        );
    }

    /// A skull answers a pointer anywhere within 12 px of its centre — the
    /// 24 px target — and the nearest of two close ones wins.
    #[test]
    fn a_skull_is_a_24_px_target() {
        let r = ribbon();
        let plot = Plot {
            ribbon: r.clone(),
            narrow: false,
        };
        let w = 1164.0;
        let c = plot.skull_box(&r.skulls[0], w).center();
        for (dx, dy) in [(11.0, 0.0), (0.0, 11.0), (-11.0, -11.0)] {
            assert_eq!(
                plot.hover_at(Point::new(c.x + dx, c.y + dy), w),
                Some(Hover::Skull(0)),
                "{dx},{dy}"
            );
        }
        assert_ne!(
            plot.hover_at(Point::new(c.x + 14.0, c.y), w),
            Some(Hover::Skull(0))
        );
        // At 460 px the 5:45 and 6:01 skulls stand ~17 px apart: each is
        // still its own nearest.
        let (a, b) = (
            plot.skull_box(&r.skulls[4], 424.0).center(),
            plot.skull_box(&r.skulls[5], 424.0).center(),
        );
        assert!(b.x - a.x < 24.0, "their targets overlap");
        assert_eq!(
            plot.hover_at(Point::new(a.x + 2.0, a.y), 424.0),
            Some(Hover::Skull(4))
        );
        assert_eq!(
            plot.hover_at(Point::new(b.x - 2.0, b.y), 424.0),
            Some(Hover::Skull(5))
        );
    }

    /// The pointer on the plot draws the gold crosshair down it (at the
    /// prototype's `.55`), through the canvas's own update.
    #[test]
    fn hovering_the_plot_draws_a_gold_crosshair() {
        let r = ribbon();
        // A column the tooltip, the words and the skulls leave clear: the
        // canvas starts after the margin; the line is 1 px at x + 0.5.
        let x = MARGIN + 300.5;
        let at = Point::new(x, PLOT_TOP + 20.0);
        let theme = crate::theme::window_theme();
        let size = Size::new(1200.0, H);
        let lit = pixels_at(r.clone().view(false), size, &theme, Some(at));
        let dark = pixels(r.view(false), size, &theme);
        let gold = mix(theme::GROUND, theme::GOLD, XHAIR_ALPHA);
        let column = (
            (x * 2.0) as u32 - 1,
            (PLOT_TOP * 2.0) as u32,
            (x * 2.0) as u32 + 1,
            ((PLOT_TOP + 12.0) * 2.0) as u32,
        );
        assert!(
            lit.count_in(column, gold, 10) >= 10,
            "the crosshair's gold down the plot's top"
        );
        assert_eq!(
            dark.count_in(column, gold, 10),
            0,
            "none without the pointer"
        );
    }

    /// An arena's enemy death (R13) is a skull of its own kind: never the
    /// owner's "you", worded as the other team's.
    #[test]
    fn an_enemy_death_is_no_skull_of_ours() {
        let mut raid = tk::raid_timeline();
        raid.deaths[4].enemy = true;
        let r = Ribbon::from_raid(&raid, 422_040, None, true, None);
        assert!(r.skulls[4].enemy);
        assert!(!r.skulls[4].mine, "never \"you\"");
        assert!(
            r.skulls[4]
                .words
                .starts_with("of the enemy team, died 5:45")
        );
    }

    /// A stored pull whose details were demoted carries no damage series:
    /// the ribbon draws the axis, the lust and the deaths alone — no peak
    /// named over a curve that is not there — and its skulls still press.
    #[test]
    fn a_ribbon_without_a_series_draws_its_axis_lust_and_deaths() {
        let mut raid = tk::raid_timeline();
        raid.series.clear();
        raid.bucket_ms = 1000;
        let r = Ribbon::from_raid(&raid, 422_040, None, true, None);
        assert!(r.rate.is_empty());
        let plot = Plot {
            ribbon: r.clone(),
            narrow: false,
        };
        let words = said(&plot, None, 1164.0);
        assert!(
            !words.iter().any(|w| w.starts_with("Raid dps")),
            "{words:?}"
        );
        for w in ["Heroism", "you", "0:00", "7:00"] {
            assert!(words.iter().any(|s| s == w), "{w}: {words:?}");
        }
        let class = raid.deaths[4].class.unwrap();
        let px = pixels(
            r.clone().view(false),
            Size::new(1200.0, H),
            &crate::theme::window_theme(),
        );
        assert!(px.count(theme::class_rgb(class), 6) > 20, "the skull");
        let mut sim = ui(r.clone(), 1200.0, false);
        sim.point_at(skull_at(&r, 4, 1200.0));
        let _ = sim.simulate(iced_test::simulator::click());
        assert!(matches!(
            sim.into_messages().collect::<Vec<_>>().as_slice(),
            [Message::OpenDeath(_)]
        ));
    }

    /// An arena's other team dies in outline: the skull's class colour is
    /// only its edge, never a solid mark of the group's.
    #[test]
    fn an_enemy_skull_is_drawn_in_outline() {
        let mut raid = tk::raid_timeline();
        raid.deaths.truncate(1);
        raid.deaths[0].enemy = true;
        let solid = {
            let mut own = raid.clone();
            own.deaths[0].enemy = false;
            Ribbon::from_raid(&own, 422_040, None, true, None)
        };
        let outline = Ribbon::from_raid(&raid, 422_040, None, true, None);
        let class = raid.deaths[0].class.unwrap();
        let at = skull_at(&outline, 0, 1200.0);
        let (x, y) = ((at.x * 2.0) as u32, (at.y * 2.0) as u32);
        // The whole skull, and the inside of its dome over the eyes (a unit
        // of the 12-unit skull is 13/12 px): a fill paints it, an outline
        // leaves it the ground.
        let whole = (x - 12, y - 12, x + 12, y + 12);
        let dome = (x - 2, y - 8, x + 2, y - 5);
        let count = |r: Ribbon, area| {
            pixels(
                r.view(false),
                Size::new(1200.0, H),
                &crate::theme::window_theme(),
            )
            .count_in(area, theme::class_rgb(class), 20)
        };
        assert!(count(outline.clone(), whole) > 0, "the outline is drawn");
        assert!(count(solid, dome) > 0, "a solid skull fills its dome");
        assert_eq!(count(outline, dome), 0, "an outline leaves it empty");
    }

    /// A stored pull's coarse series (10 s buckets) draws one point a
    /// bucket — never folds them into 30 s steps.
    #[test]
    fn a_coarse_series_keeps_its_ten_second_steps() {
        let (step, rate) = rates(&[100_000; 43], 10_000, 422_040);
        assert_eq!((step, rate.len()), (10_000, 43));
        assert_eq!(rate[0], 10_000.0);
    }
}
