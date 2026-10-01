//! Home's three drawings, each a canvas scaled from the prototype's own
//! SVG `viewBox` so its proportions hold at any width — its type with
//! them, as an SVG's, but never smaller than its own size ([`type_scale`]):
//! the rank across the night (`300 × 140`), the key throughput dots (`420
//! × 170`), and a key's run against its timers (`.par`). A dot on either
//! chart is a jump point, as the tile or row it stands for is: a press
//! opens its pull.

use iced::widget::canvas::{self, Canvas, Path, Stroke};
use iced::{Color, Element, Font, Length, Point, Rectangle, Renderer, Size, Theme, mouse};

use crate::fight_head::ordinal;
use crate::theme;
use crate::window::Message;

use super::{CharInk, NightPull, TrendPoint};
// The charts' words and axis are gui-logic's; re-exported for the tests.
pub(super) use wowdps_gui_logic::home::{axis, axis_label, best_label, day_labels, hollow};

/// The rank chart's box (`viewBox="0 0 300 140"`), and the widest it is
/// drawn (`max-width:360px`).
const SLOPE_W: f32 = 300.0;
const SLOPE_H: f32 = 140.0;
pub(super) const SLOPE_MAX_W: f32 = 360.0;
/// Its two guides — the top of the role and the bottom — and where they
/// run (`x1=18`, `x2=W-10`); its dots' first x and the room the last one
/// leaves (`26 + i·(W−70)/(n−1)`); its dots and their labels.
const GUIDE_TOP: f32 = 18.0;
const GUIDE_FOOT: f32 = 26.0;
const GUIDE_X: f32 = 18.0;
const GUIDE_END: f32 = 10.0;
const DOT_X: f32 = 26.0;
const DOT_ROOM: f32 = 70.0;
const SLOPE_R: f32 = 4.5;
const SLOPE_LINE: f32 = 2.0;
const GUIDE_PX: f32 = 10.5;
const RANK_PX: f32 = 11.5;
const RANK_RISE: f32 = 9.0;
/// The guides' words: "top of the role" on its baseline 13 down the box,
/// "bottom" 12 up from its foot (`y=13`, `y=H-12`), both ending at the
/// guides' right end — and about how wide the top one runs back from it
/// at its type size (65 of the box's 300 at 10.5 px), and a rank's half
/// width ("17th" at 11.5 px): what tells a rank that would print over
/// them ([`rank_under`]).
const GUIDE_TOP_WORDS: f32 = 13.0;
const GUIDE_FOOT_WORDS: f32 = 12.0;
const GUIDE_WORDS_W: f32 = 66.0;
const RANK_HALF_W: f32 = 12.0;
/// A line of type's cap height, of its size: a rank under its dot hangs
/// from its top, where one above stands on its baseline.
const CAP_OF_TYPE: f32 = 0.7;
/// Under this many pixels between two dots, only the first, the last, the
/// highest and the lowest keep their rank above them: a crowded night's
/// labels would print over each other.
const LABEL_ROOM: f32 = 22.0;

/// The throughput chart's box (`viewBox="0 0 420 170"`): its plot's foot
/// and head room (`th − 22 − f·(th − 34)`), its gridlines' run and their
/// labels' right edge, its dots' first x and the room the last leaves.
const TREND_W: f32 = 420.0;
const TREND_H: f32 = 170.0;
const PLOT_FOOT: f32 = 22.0;
const PLOT_ROOM: f32 = 34.0;
const GRID_X: f32 = 40.0;
const GRID_END: f32 = 6.0;
const GRID_LABEL_X: f32 = 34.0;
const TREND_X: f32 = 56.0;
const TREND_ROOM: f32 = 80.0;
const TREND_R: f32 = 5.0;
const BEST_RING: f32 = 2.5;
const BEST_RISE: f32 = 10.0;
const AXIS_PX: f32 = 11.0;
const DAY_PX: f32 = 10.5;
const DAY_FOOT: f32 = 6.0;
/// A gridline's figure stands on its line: its baseline this far under it
/// (`y+4`).
const AXIS_DROP: f32 = 4.0;

/// How near a press must land to a dot to open its pull.
const HIT: f32 = 8.0;

/// A chart's words sit on their SVG baseline: this much of the type's size
/// above it is where the middle of a lower-case line stands.
const MID_OF_TYPE: f32 = 0.33;

/// How much a chart's words are scaled at the box's scale `s`: with the
/// box, as an SVG's are, but never below their own size — a panel of a
/// three-column Home is narrower than the box, and 11 px axis figures at
/// 0.7 of it are under what reads.
fn type_scale(s: f32) -> f32 {
    s.max(1.0)
}

/// A word on a chart, its baseline at `at` as the SVG sets it, anchored
/// left, centred or right as `align`.
fn words(
    frame: &mut canvas::Frame,
    s: String,
    at: Point,
    px: f32,
    color: Color,
    font: Font,
    align: iced::alignment::Horizontal,
) {
    frame.fill_text(canvas::Text {
        content: s,
        position: Point::new(at.x, at.y - px * MID_OF_TYPE),
        color,
        size: px.into(),
        font,
        align_x: align.into(),
        align_y: iced::alignment::Vertical::Center,
        ..canvas::Text::default()
    });
}

/// The pull a press at `p` lands on: the nearest dot within [`HIT`].
fn hit(dots: &[(Point, String)], p: Point) -> Option<String> {
    dots.iter()
        .map(|(at, id)| (at.distance(p), id))
        .filter(|(d, _)| *d <= HIT)
        .min_by(|a, b| a.0.total_cmp(&b.0))
        .map(|(_, id)| id.clone())
}

fn press(
    dots: &[(Point, String)],
    event: &canvas::Event,
    bounds: Rectangle,
    cursor: mouse::Cursor,
) -> Option<canvas::Action<Message>> {
    let iced::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) = event else {
        return None;
    };
    let id = hit(dots, cursor.position_in(bounds)?)?;
    Some(canvas::Action::publish(Message::OpenStored(id)).and_capture())
}

fn pointer(
    dots: &[(Point, String)],
    bounds: Rectangle,
    cursor: mouse::Cursor,
) -> mouse::Interaction {
    match cursor.position_in(bounds).and_then(|p| hit(dots, p)) {
        Some(_) => mouse::Interaction::Pointer,
        None => mouse::Interaction::default(),
    }
}

// ---- the night's rank --------------------------------------------------------

/// "Your rank, pull by pull": each pull's place in the role as a height —
/// the top of the role at the top, the bottom at the foot, so 17th of 19
/// and 3rd of 5 share one scale — joined in the character's colour. A kill
/// or a timed key is a filled dot, a wipe or a key over time a ring; each
/// carries its rank above it.
pub(super) struct RankSlope {
    pulls: Vec<NightPull>,
    color: Color,
}

impl RankSlope {
    pub(super) fn new(pulls: &[NightPull], color: Color) -> Self {
        Self {
            pulls: pulls.to_vec(),
            color,
        }
    }

    /// The chart `w` wide, as tall as its box makes it.
    pub(super) fn view(self, w: f32) -> Element<'static, Message> {
        Canvas::new(self)
            .width(Length::Fixed(w))
            .height(Length::Fixed(w * SLOPE_H / SLOPE_W))
            .into()
    }

    /// Where each pull's dot stands in a chart `w` wide. A night of one
    /// pull stands it in the middle of the box: at the first dot's place
    /// it would read as the start of a line that never came.
    pub(super) fn dots(&self, w: f32) -> Vec<(Point, String)> {
        let s = w / SLOPE_W;
        let n = self.pulls.len();
        let step = (SLOPE_W - DOT_ROOM) / n.saturating_sub(1).max(1) as f32;
        let first = if n == 1 { SLOPE_W / 2.0 } else { DOT_X };
        self.pulls
            .iter()
            .enumerate()
            .map(|(i, p)| {
                let f = p.standing.percentile();
                let y = GUIDE_TOP + (1.0 - f) * (SLOPE_H - GUIDE_TOP - GUIDE_FOOT);
                (
                    Point::new((first + i as f32 * step) * s, y * s),
                    p.fight_id.clone(),
                )
            })
            .collect()
    }

    /// Which dots keep their rank above them: all of them, while there is
    /// room between two; else the first, the last, the highest and the
    /// lowest.
    pub(super) fn labelled(&self, w: f32) -> Vec<bool> {
        let n = self.pulls.len();
        let gap = (SLOPE_W - DOT_ROOM) / n.saturating_sub(1).max(1) as f32 * w / SLOPE_W;
        if gap >= LABEL_ROOM {
            return vec![true; n];
        }
        let f = |i: usize| self.pulls.get(i).map_or(0.0, |p| p.standing.percentile());
        let high = (0..n).max_by(|a, b| f(*a).total_cmp(&f(*b)));
        let low = (0..n).min_by(|a, b| f(*a).total_cmp(&f(*b)));
        (0..n)
            .map(|i| i == 0 || i + 1 == n || Some(i) == high || Some(i) == low)
            .collect()
    }
}

/// A rank printed above its dot at `at`, in a chart `w` wide, would print
/// over the top guide's words: its dot within a line of type of their
/// baseline, and within their reach from the guide's right end. It hangs
/// under its dot instead — the night's best pull is the one that tops
/// the role, and the one the words would otherwise break.
pub(super) fn rank_under(at: Point, w: f32) -> bool {
    let s = w / SLOPE_W;
    let ts = type_scale(s);
    let baseline = at.y - RANK_RISE * s;
    let near = baseline < GUIDE_TOP_WORDS * s + RANK_PX * ts;
    let words_left = (SLOPE_W - GUIDE_END) * s - GUIDE_WORDS_W * ts;
    near && at.x + RANK_HALF_W * ts > words_left
}

impl canvas::Program<Message> for RankSlope {
    type State = ();

    fn update(
        &self,
        _state: &mut (),
        event: &canvas::Event,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Option<canvas::Action<Message>> {
        press(&self.dots(bounds.width), event, bounds, cursor)
    }

    fn mouse_interaction(
        &self,
        _state: &(),
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> mouse::Interaction {
        pointer(&self.dots(bounds.width), bounds, cursor)
    }

    fn draw(
        &self,
        _state: &(),
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<canvas::Geometry> {
        let mut frame = canvas::Frame::new(renderer, bounds.size());
        let w = bounds.width;
        let s = w / SLOPE_W;
        let ts = type_scale(s);
        let rule = Stroke::default().with_width(1.0).with_color(theme::LINE);
        let (x1, x2) = (GUIDE_X * s, (SLOPE_W - GUIDE_END) * s);
        for y in [GUIDE_TOP, SLOPE_H - GUIDE_FOOT] {
            let y = (y * s).round() + 0.5;
            frame.stroke(&Path::line(Point::new(x1, y), Point::new(x2, y)), rule);
        }
        let right = iced::alignment::Horizontal::Right;
        words(
            &mut frame,
            "top of the role".to_string(),
            Point::new(x2, GUIDE_TOP_WORDS * s),
            GUIDE_PX * ts,
            theme::INK_3_TEXT,
            theme::UI,
            right,
        );
        words(
            &mut frame,
            "bottom".to_string(),
            Point::new(x2, (SLOPE_H - GUIDE_FOOT_WORDS) * s),
            GUIDE_PX * ts,
            theme::INK_3_TEXT,
            theme::UI,
            right,
        );
        let dots = self.dots(w);
        if dots.len() > 1 {
            let mut line = canvas::path::Builder::new();
            for (i, (p, _)) in dots.iter().enumerate() {
                if i == 0 {
                    line.move_to(*p);
                } else {
                    line.line_to(*p);
                }
            }
            frame.stroke(
                &line.build(),
                Stroke::default()
                    .with_width(SLOPE_LINE * s)
                    .with_color(self.color)
                    .with_line_join(canvas::LineJoin::Round),
            );
        }
        let labelled = self.labelled(w);
        for (((at, _), pull), label) in dots.iter().zip(&self.pulls).zip(labelled) {
            let dot = Path::circle(*at, SLOPE_R * s);
            let fill = if hollow(pull.mark) {
                theme::SURFACE
            } else {
                self.color
            };
            frame.fill(&dot, fill);
            frame.stroke(
                &dot,
                Stroke::default()
                    .with_width(SLOPE_LINE * s)
                    .with_color(self.color),
            );
            if label {
                // Above its dot, on its baseline; under it, hanging from
                // its cap, where above would print over the guide's words.
                let y = if rank_under(*at, w) {
                    at.y + RANK_RISE * s + RANK_PX * ts * CAP_OF_TYPE
                } else {
                    at.y - RANK_RISE * s
                };
                words(
                    &mut frame,
                    ordinal(pull.standing.place),
                    Point::new(at.x, y),
                    RANK_PX * ts,
                    theme::INK,
                    theme::UI,
                    iced::alignment::Horizontal::Center,
                );
            }
        }
        vec![frame.into_geometry()]
    }
}

// ---- the week's key throughput ----------------------------------------------

/// "Effective dps on keys": a dot per run in its character's colour, the
/// runs in the order they were played, each character's best of the week
/// ringed in legendary orange with its figure over it; the night named
/// under the first run of each.
pub(super) struct Trend {
    points: Vec<TrendPoint>,
    lo: f64,
    hi: f64,
    ticks: Vec<f64>,
}

impl Trend {
    pub(super) fn new(points: &[TrendPoint]) -> Self {
        let values: Vec<f64> = points.iter().map(|p| p.value).collect();
        let (lo, hi, ticks) = axis(&values);
        Self {
            points: points.to_vec(),
            lo,
            hi,
            ticks,
        }
    }

    /// The chart `w` wide, as tall as its box makes it.
    pub(super) fn view(self, w: f32) -> Element<'static, Message> {
        Canvas::new(self)
            .width(Length::Fixed(w))
            .height(Length::Fixed(w * TREND_H / TREND_W))
            .into()
    }

    fn y_of(&self, v: f64, s: f32) -> f32 {
        let f = if self.hi > self.lo {
            ((v - self.lo) / (self.hi - self.lo)) as f32
        } else {
            0.5
        };
        (TREND_H - PLOT_FOOT - f * (TREND_H - PLOT_ROOM)) * s
    }

    fn dots(&self, w: f32) -> Vec<(Point, String)> {
        let s = w / TREND_W;
        let step = (TREND_W - TREND_ROOM) / self.points.len().saturating_sub(1).max(1) as f32;
        self.points
            .iter()
            .enumerate()
            .map(|(i, p)| {
                (
                    Point::new((TREND_X + i as f32 * step) * s, self.y_of(p.value, s)),
                    p.fight_id.clone(),
                )
            })
            .collect()
    }
}

impl canvas::Program<Message> for Trend {
    type State = ();

    fn update(
        &self,
        _state: &mut (),
        event: &canvas::Event,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Option<canvas::Action<Message>> {
        press(&self.dots(bounds.width), event, bounds, cursor)
    }

    fn mouse_interaction(
        &self,
        _state: &(),
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> mouse::Interaction {
        pointer(&self.dots(bounds.width), bounds, cursor)
    }

    fn draw(
        &self,
        _state: &(),
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<canvas::Geometry> {
        let mut frame = canvas::Frame::new(renderer, bounds.size());
        let w = bounds.width;
        let s = w / TREND_W;
        let ts = type_scale(s);
        let rule = Stroke::default().with_width(1.0).with_color(theme::LINE);
        for v in &self.ticks {
            let y = self.y_of(*v, s).round() + 0.5;
            frame.stroke(
                &Path::line(
                    Point::new(GRID_X * s, y),
                    Point::new((TREND_W - GRID_END) * s, y),
                ),
                rule,
            );
            words(
                &mut frame,
                axis_label(*v),
                Point::new(GRID_LABEL_X * s, y + AXIS_DROP * s),
                AXIS_PX * ts,
                theme::INK_3_TEXT,
                theme::UI,
                iced::alignment::Horizontal::Right,
            );
        }
        let dots = self.dots(w);
        let days = day_labels(&self.points);
        for (((at, _), p), day) in dots.iter().zip(&self.points).zip(days) {
            let dot = Path::circle(*at, TREND_R * s);
            frame.fill(&dot, p.who.color());
            if p.best {
                frame.stroke(
                    &dot,
                    Stroke::default()
                        .with_width(BEST_RING * s)
                        .with_color(theme::LEGENDARY),
                );
                words(
                    &mut frame,
                    best_label(p.value),
                    Point::new(at.x, at.y - BEST_RISE * s),
                    AXIS_PX * ts,
                    theme::LEGENDARY,
                    theme::UI_SEMIBOLD,
                    iced::alignment::Horizontal::Center,
                );
            }
            if day {
                words(
                    &mut frame,
                    crate::rail::weekday(p.day).to_string(),
                    Point::new(at.x, (TREND_H - DAY_FOOT) * s),
                    DAY_PX * ts,
                    theme::INK_3_TEXT,
                    theme::UI,
                    iced::alignment::Horizontal::Center,
                );
            }
        }
        vec![frame.into_geometry()]
    }
}

// ---- a key against its timers -------------------------------------------------

/// The par bar's box: 8 px of track with its round ends (`.par{height:8px;
/// border-radius:4px}`) and the ticks 3 px proud of it either side (`.tk{
/// top:-3px;bottom:-3px}`).
const PAR_TRACK_H: f32 = 8.0;
const PAR_PROUD: f32 = 3.0;
const PAR_H: f32 = PAR_TRACK_H + 2.0 * PAR_PROUD;
/// The run's fill over its track (`theme::PAR_TRACK`), `opacity:.8`.
const PAR_FILL_ALPHA: f32 = 0.8;

/// A key's run against its timers (`.par`): the track a quarter longer
/// than the timer, the run's time filled in green when it was timed and in
/// red when it went over, and a tick at +3, +2 and the timer.
pub(super) struct ParBar {
    pub clock_ms: i64,
    pub pars: (i64, i64, i64),
    pub timed: bool,
}

impl ParBar {
    pub(super) fn view(self, w: f32) -> Element<'static, Message> {
        Canvas::new(self)
            .width(Length::Fixed(w))
            .height(Length::Fixed(PAR_H))
            .into()
    }

    /// Where `ms` stands along a bar `w` wide.
    pub(super) fn x_of(&self, ms: i64, w: f32) -> f32 {
        wowdps_gui_logic::home::par_x(self.pars.0, ms, w)
    }
}

impl canvas::Program<Message> for ParBar {
    type State = ();

    fn draw(
        &self,
        _state: &(),
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<canvas::Geometry> {
        let mut frame = canvas::Frame::new(renderer, bounds.size());
        let w = bounds.width;
        let r = PAR_TRACK_H / 2.0;
        frame.fill(
            &Path::rounded_rectangle(
                Point::new(0.0, PAR_PROUD),
                Size::new(w, PAR_TRACK_H),
                r.into(),
            ),
            theme::PAR_TRACK,
        );
        let fill = self.x_of(self.clock_ms, w);
        if fill > 0.0 {
            let color = if self.timed { theme::GOOD } else { theme::BAD };
            frame.fill(
                &Path::rounded_rectangle(
                    Point::new(0.0, PAR_PROUD),
                    Size::new(fill, PAR_TRACK_H),
                    r.min(fill / 2.0).into(),
                ),
                Color {
                    a: PAR_FILL_ALPHA,
                    ..color
                },
            );
        }
        let tick = Stroke::default().with_width(1.0).with_color(theme::INK_3);
        for ms in [self.pars.2, self.pars.1, self.pars.0] {
            let x = self.x_of(ms, w).round() + 0.5;
            frame.stroke(&Path::line(Point::new(x, 0.0), Point::new(x, PAR_H)), tick);
        }
        vec![frame.into_geometry()]
    }
}
