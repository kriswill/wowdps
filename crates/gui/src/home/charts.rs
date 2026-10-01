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
// The charts' words, axis and geometry are gui-logic's; re-exported for the
// tests.
pub(super) use wowdps_gui_logic::home::chart::{
    self, AXIS_DROP, AXIS_PX, BEST_RING, BEST_RISE, CAP_OF_TYPE, DAY_FOOT, DAY_PX, GRID_END,
    GRID_LABEL_X, GRID_X, GUIDE_END, GUIDE_FOOT, GUIDE_FOOT_WORDS, GUIDE_PX, GUIDE_TOP,
    GUIDE_TOP_WORDS, GUIDE_X, MID_OF_TYPE, RANK_PX, RANK_RISE, SLOPE_H, SLOPE_LINE, SLOPE_MAX_W,
    SLOPE_R, SLOPE_W, TREND_H, TREND_R, TREND_W, TrendAxis, type_scale,
};
pub(super) use wowdps_gui_logic::home::{axis_label, best_label, day_labels, hollow};

/// The pull a press at `p` lands on: the nearest dot within `chart::HIT`.
fn hit(dots: &[(Point, String)], p: Point) -> Option<String> {
    let dots: Vec<chart::Dot> = dots
        .iter()
        .map(|(at, id)| ((at.x, at.y), id.clone()))
        .collect();
    chart::hit(&dots, (p.x, p.y))
}

/// gui-logic's dots, as iced's points.
fn points(dots: Vec<chart::Dot>) -> Vec<(Point, String)> {
    dots.into_iter()
        .map(|((x, y), id)| (Point::new(x, y), id))
        .collect()
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
    /// Where each pull's dot stands in a chart `w` wide (gui-logic's
    /// `chart::slope_dots`).
    pub(super) fn dots(&self, w: f32) -> Vec<(Point, String)> {
        points(chart::slope_dots(&self.pulls, w))
    }

    /// Which dots keep their rank above them (`chart::slope_labelled`).
    pub(super) fn labelled(&self, w: f32) -> Vec<bool> {
        chart::slope_labelled(&self.pulls, w)
    }
}

/// A rank printed above its dot at `at` would print over the top guide's
/// words, so it hangs under it (`chart::rank_under`).
pub(super) fn rank_under(at: Point, w: f32) -> bool {
    chart::rank_under((at.x, at.y), w)
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

pub(super) struct Trend {
    points: Vec<TrendPoint>,
    axis: TrendAxis,
}

impl Trend {
    pub(super) fn new(points: &[TrendPoint]) -> Self {
        Self {
            points: points.to_vec(),
            axis: TrendAxis::of(points),
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
        self.axis.y_of(v, s)
    }

    fn dots(&self, w: f32) -> Vec<(Point, String)> {
        points(self.axis.dots(&self.points, w))
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
        for v in &self.axis.ticks {
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
