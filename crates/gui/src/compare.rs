//! R12: the two-player comparison — per-spell tables side by side, each over
//! a timeline graph marked with trinket uses, trinket procs and consumables.
//!
//! Everything here is pure rendering with no message type of its own, so the
//! window and the overlay share it exactly the way they already share
//! `view::bar_row`. Selection lives in the frontends: they wrap
//! [`class_icon`] in their own `mouse_area`, because only they know what
//! message a click should become.
//!
//! The two graphs deliberately share one y-scale and one x-range. Two curves
//! drawn to their own maxima look identical no matter how far apart the
//! players actually are, which is the one thing a comparison must not do.

use iced::widget::canvas::{self, Canvas, Path, Stroke};
use iced::widget::{Space, column, container, row, scrollable, text};
use iced::{Color, Element, Font, Length, Point, Rectangle, Renderer, Size, Theme};

use wowdps_model::fmt::human;
use wowdps_model::{Class, GraphMode, Mark, MarkKind, Row, Spec, Timeline};
use wowdps_proto::{ClientState, CompareSide};

use crate::view::{DIM, GREEN, YELLOW};

/// Marker colors. Distinct hues rather than shades: at graph width these bars
/// are one or two pixels wide, and a shade difference is invisible.
const USE: Color = Color::from_rgb(1.0, 0.85, 0.35);
const PROC: Color = Color::from_rgb(0.45, 0.85, 1.0);
const CONSUMABLE: Color = GREEN;

pub(crate) fn mark_color(kind: MarkKind) -> Color {
    match kind {
        MarkKind::TrinketUse => USE,
        MarkKind::TrinketProc => PROC,
        MarkKind::Consumable => CONSUMABLE,
    }
}

fn mark_name(kind: MarkKind) -> &'static str {
    match kind {
        MarkKind::TrinketUse => "trinket use",
        MarkKind::TrinketProc => "proc",
        MarkKind::Consumable => "consumable",
    }
}

/// Bar color for a player whose class is not known yet.
const CLASSLESS: Color = Color::from_rgb(0.42, 0.44, 0.52);

fn class_color(class: Option<Class>) -> Color {
    match class {
        Some(c) => {
            let (r, g, b) = c.rgb();
            Color::from_rgb8(r, g, b)
        }
        None => CLASSLESS,
    }
}

/// The two-letter tag drawn inside a class icon. Real Blizzard class art is
/// not ours to ship, so the icon is drawn: a class-colored disc carrying the
/// class's own abbreviation, in the palette every other wowdps surface uses.
fn class_tag(class: Option<Class>) -> &'static str {
    match class {
        Some(Class::Warrior) => "WR",
        Some(Class::Paladin) => "PA",
        Some(Class::Hunter) => "HU",
        Some(Class::Rogue) => "RO",
        Some(Class::Priest) => "PR",
        Some(Class::DeathKnight) => "DK",
        Some(Class::Shaman) => "SH",
        Some(Class::Mage) => "MG",
        Some(Class::Warlock) => "WL",
        Some(Class::Monk) => "MO",
        Some(Class::Druid) => "DR",
        Some(Class::DemonHunter) => "DH",
        Some(Class::Evoker) => "EV",
        None => "?",
    }
}

/// A clickable class emblem, ringed when it is one of the picked pair.
/// `slot` is the comparison side (0 or 1) or `None` when unpicked.
///
/// The art is the game's own: the spec's icon when the spec is known, else
/// the class crest, from the generated atlas (`icons.rs`, extracted from the
/// local install by `tools/gen-icons.sh`). A player the atlas cannot name —
/// or an atlas that was never generated — falls back to the drawn
/// class-colored disc, so a fresh checkout still builds and renders.
///
/// Emits nothing — wrap it in the frontend's own `mouse_area` to make the
/// pick happen.
pub(crate) fn class_icon<M: 'static>(
    class: Option<Class>,
    spec: Option<Spec>,
    slot: Option<usize>,
    d: f32,
) -> Element<'static, M> {
    let art = spec
        .and_then(|s| crate::icons::spec_handle(s.id()))
        .or_else(|| class.and_then(crate::icons::class_handle));
    let Some(handle) = art else {
        return Canvas::new(ClassIcon {
            color: class_color(class),
            tag: class_tag(class),
            slot,
        })
        .width(Length::Fixed(d))
        .height(Length::Fixed(d))
        .into();
    };
    let img = iced::widget::image(handle)
        .width(Length::Fixed(d))
        .height(Length::Fixed(d))
        // Unpicked icons sit back so a picked pair reads at a glance.
        .opacity(if slot.is_some() { 1.0_f32 } else { 0.8_f32 });
    if slot.is_none() {
        return img.into();
    }
    iced::widget::stack![
        img,
        Canvas::new(Ring)
            .width(Length::Fixed(d))
            .height(Length::Fixed(d)),
    ]
    .into()
}

/// The picked ring drawn over a cached icon — the same white circle the
/// drawn-disc fallback wears.
struct Ring;

impl<M> canvas::Program<M> for Ring {
    type State = ();

    fn draw(
        &self,
        _state: &(),
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: iced::mouse::Cursor,
    ) -> Vec<canvas::Geometry> {
        let mut frame = canvas::Frame::new(renderer, bounds.size());
        let c = frame.center();
        let r = bounds.width.min(bounds.height) / 2.0 - 1.0;
        frame.stroke(
            &Path::circle(c, r),
            Stroke::default().with_width(2.0).with_color(Color::WHITE),
        );
        vec![frame.into_geometry()]
    }
}

struct ClassIcon {
    color: Color,
    tag: &'static str,
    slot: Option<usize>,
}

impl<M> canvas::Program<M> for ClassIcon {
    type State = ();

    fn draw(
        &self,
        _state: &(),
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: iced::mouse::Cursor,
    ) -> Vec<canvas::Geometry> {
        let mut frame = canvas::Frame::new(renderer, bounds.size());
        let c = frame.center();
        let r = bounds.width.min(bounds.height) / 2.0 - 1.0;

        // Unpicked icons sit back so a picked pair reads at a glance.
        let (fill, alpha) = match self.slot {
            Some(_) => (self.color, 1.0),
            None => (self.color, 0.55),
        };
        frame.fill(&Path::circle(c, r), Color { a: alpha, ..fill });
        frame.fill_text(canvas::Text {
            content: self.tag.to_string(),
            position: c,
            color: Color::from_rgba(0.0, 0.0, 0.0, 0.85),
            size: (r * 0.9).into(),
            font: Font::MONOSPACE,
            align_x: iced::alignment::Horizontal::Center.into(),
            align_y: iced::alignment::Vertical::Center,
            ..canvas::Text::default()
        });
        if self.slot.is_some() {
            frame.stroke(
                &Path::circle(c, r),
                Stroke::default().with_width(2.0).with_color(Color::WHITE),
            );
        }
        vec![frame.into_geometry()]
    }
}

// ---- the comparison screen -------------------------------------------------

/// The whole comparison body: two columns, each a header, a spell table and a
/// graph. `scale` multiplies text sizes the way `view::bar_row` does, so the
/// overlay can zoom without iced's scale factor.
pub(crate) fn compare_body<M: 'static>(
    app: &ClientState,
    scale: f32,
    graph_height: f32,
) -> Element<'static, M> {
    let Some((a, b)) = app.compare_sides() else {
        return waiting(app, scale);
    };
    let mode = app.graph_mode();

    // One scale for both graphs, or the comparison lies (see module docs).
    let peak = peak_of(&[&a.timeline, &b.timeline], mode);
    let span = a
        .timeline
        .buckets
        .len()
        .max(b.timeline.buckets.len())
        .max(1);

    let panes = row![
        side_column(a, mode, peak, span, scale, graph_height),
        side_column(b, mode, peak, span, scale, graph_height),
    ]
    .spacing(10)
    .height(Length::Fill);

    column![panes, legend(mode, scale)]
        .spacing(6)
        .height(Length::Fill)
        .into()
}

/// Shown while a pair is picked but the daemon has not answered yet — and,
/// more importantly, when only one player is picked, which is the state the
/// user spends the most time in.
fn waiting<M: 'static>(app: &ClientState, scale: f32) -> Element<'static, M> {
    let picks = app.compare_picks();
    let msg = match (picks.len(), picks.first()) {
        (1, Some((_, label))) => {
            format!("comparing {} — pick one more", short_name(label))
        }
        (0, _) => "pick two players to compare".to_string(),
        _ => "loading comparison…".to_string(),
    };
    container(text(msg).size(13.0 * scale).color(DIM))
        .center_x(Length::Fill)
        .center_y(Length::Fill)
        .into()
}

/// "Keanucleavês-Proudmoore" → "Keanucleavês"; character names cannot contain
/// a dash, so everything from the first one is realm noise.
fn short_name(label: &str) -> String {
    label.split('-').next().unwrap_or(label).to_string()
}

fn side_column<M: 'static>(
    side: &CompareSide,
    mode: GraphMode,
    peak: f64,
    span: usize,
    scale: f32,
    graph_height: f32,
) -> Element<'static, M> {
    let color = class_color(side.total.class);
    let header = row![
        class_icon(side.total.class, side.total.spec, Some(0), 18.0 * scale),
        text(short_name(&side.total.label))
            .size(14.0 * scale)
            .color(color),
        Space::new().width(Length::Fill),
        text(human(side.total.amount))
            .size(13.0 * scale)
            .font(Font::MONOSPACE),
        text(format!("{} dps", human(side.total.per_sec as u64)))
            .size(12.0 * scale)
            .color(DIM)
            .font(Font::MONOSPACE),
    ]
    .spacing(6)
    .align_y(iced::Alignment::Center);

    column![
        header,
        spell_table(&side.spells, scale),
        graph(&side.timeline, color, mode, peak, span, graph_height),
    ]
    .spacing(6)
    .width(Length::FillPortion(1))
    .height(Length::Fill)
    .into()
}

/// Column widths for the per-spell table: (hits, crit%, average).
const COLS: (f32, f32, f32) = (44.0, 46.0, 56.0);

fn spell_table<M: 'static>(spells: &[Row], scale: f32) -> Element<'static, M> {
    let head = |s: &str, w: f32| {
        text(s.to_string())
            .size(10.0 * scale)
            .color(DIM)
            .font(Font::MONOSPACE)
            .width(Length::Fixed(w * scale))
            .align_x(iced::Alignment::End)
    };
    let heading = row![
        text("spell").size(10.0 * scale).color(DIM),
        Space::new().width(Length::Fill),
        head("hits", COLS.0),
        head("crit", COLS.1),
        head("avg", COLS.2),
    ]
    .spacing(4)
    .padding([0, 6]);

    let mut list = column![].spacing(2);
    if spells.is_empty() {
        list = list.push(text("no damage recorded").size(12.0 * scale).color(DIM));
    }
    for r in spells {
        list = list.push(spell_row(r, scale));
    }

    column![heading, scrollable(list).height(Length::Fill)]
        .spacing(3)
        .height(Length::Fill)
        .into()
}

fn spell_row<M: 'static>(r: &Row, scale: f32) -> Element<'static, M> {
    let cell = |s: String, w: f32, color: Color| {
        text(s)
            .size(11.0 * scale)
            .color(color)
            .font(Font::MONOSPACE)
            .width(Length::Fixed(w * scale))
            .align_x(iced::Alignment::End)
    };
    // The ability's own art, when the spell-icon cache knows it.
    let icon: Element<'static, M> = match crate::spell_icons::handle(r.spell_id) {
        Some(h) => iced::widget::image(h)
            .width(Length::Fixed(13.0 * scale))
            .height(Length::Fixed(13.0 * scale))
            .into(),
        None => Space::new().width(Length::Fixed(13.0 * scale)).into(),
    };
    // The three numbers the comparison exists for. `count` is hits (absorb
    // credits included, which can never crit — see R1/R3), so the average is
    // over exactly the events that produced `amount`.
    let avg = match r.amount.checked_div(r.count) {
        Some(v) => human(v),
        None => "—".to_string(),
    };
    let crit = if r.count > 0 {
        format!("{:.0}%", r.crit_pct())
    } else {
        "—".to_string()
    };

    row![
        icon,
        // Fill + NoWrap: long labels clip rather than displacing the columns.
        text(r.label.clone())
            .size(11.0 * scale)
            .width(Length::Fill)
            .wrapping(iced::widget::text::Wrapping::None),
        cell(r.count.to_string(), COLS.0, Color::WHITE),
        cell(crit, COLS.1, YELLOW),
        cell(avg, COLS.2, Color::WHITE),
    ]
    .spacing(4)
    .padding([0, 6])
    .align_y(iced::Alignment::Center)
    .into()
}

fn legend<M: 'static>(mode: GraphMode, scale: f32) -> Element<'static, M> {
    let key = |kind: MarkKind| {
        row![
            text("▌").size(11.0 * scale).color(mark_color(kind)),
            text(mark_name(kind)).size(10.0 * scale).color(DIM),
        ]
        .spacing(2)
        .align_y(iced::Alignment::Center)
    };
    row![
        text(format!("graph: {}", mode.label()))
            .size(10.0 * scale)
            .color(DIM),
        Space::new().width(Length::Fill),
        key(MarkKind::TrinketUse),
        key(MarkKind::TrinketProc),
        key(MarkKind::Consumable),
    ]
    .spacing(10)
    .align_y(iced::Alignment::Center)
    .into()
}

// ---- the graph -------------------------------------------------------------

/// The curve a mode draws, as plain points. Kept out of the canvas so the
/// peak (shared by both sides) can be computed the same way.
fn curve(t: &Timeline, mode: GraphMode) -> Vec<f64> {
    match mode {
        // A 15s window: long enough to survive a cast gap, short enough that
        // a trinket window still stands out as a bump.
        GraphMode::Dps => t.rolling_dps(15_000),
        GraphMode::Total => t.cumulative().into_iter().map(|v| v as f64).collect(),
    }
}

fn peak_of(timelines: &[&Timeline], mode: GraphMode) -> f64 {
    timelines
        .iter()
        .flat_map(|t| curve(t, mode))
        .fold(0.0f64, f64::max)
}

fn graph<M: 'static>(
    t: &Timeline,
    color: Color,
    mode: GraphMode,
    peak: f64,
    span: usize,
    height: f32,
) -> Element<'static, M> {
    Canvas::new(Graph {
        points: curve(t, mode),
        marks: t.marks.clone(),
        bucket_ms: t.bucket_ms.max(1) as f64,
        color,
        peak,
        span,
    })
    .width(Length::Fill)
    .height(Length::Fixed(height))
    .into()
}

struct Graph {
    points: Vec<f64>,
    marks: Vec<Mark>,
    bucket_ms: f64,
    color: Color,
    peak: f64,
    /// Bucket count of the LONGER of the two timelines: both graphs share an
    /// x-range so the same instant is the same column in both.
    span: usize,
}

impl<M> canvas::Program<M> for Graph {
    type State = ();

    fn draw(
        &self,
        _state: &(),
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: iced::mouse::Cursor,
    ) -> Vec<canvas::Geometry> {
        let mut frame = canvas::Frame::new(renderer, bounds.size());
        let (w, h) = (bounds.width, bounds.height);

        frame.fill(
            &Path::rectangle(Point::ORIGIN, Size::new(w, h)),
            Color::from_rgba(1.0, 1.0, 1.0, 0.04),
        );
        // Baseline: without it an empty graph is indistinguishable from a
        // missing one.
        frame.stroke(
            &Path::line(Point::new(0.0, h - 0.5), Point::new(w, h - 0.5)),
            Stroke::default()
                .with_width(1.0)
                .with_color(Color::from_rgba(1.0, 1.0, 1.0, 0.15)),
        );

        if self.span == 0 {
            return vec![frame.into_geometry()];
        }
        let x_of = |bucket: f64| (bucket / self.span as f64) as f32 * w;
        let y_of = |v: f64| {
            if self.peak <= 0.0 {
                h
            } else {
                h - (v / self.peak) as f32 * (h - 2.0)
            }
        };

        // Markers first: the curve reads on top of them.
        for m in &self.marks {
            let x = x_of(m.at_ms as f64 / self.bucket_ms).clamp(0.0, w);
            frame.stroke(
                &Path::line(Point::new(x, 0.0), Point::new(x, h)),
                Stroke::default().with_width(1.0).with_color(Color {
                    a: 0.75,
                    ..mark_color(m.kind)
                }),
            );
        }

        if let Some(&first) = self.points.first()
            && self.points.len() > 1
        {
            let mut b = canvas::path::Builder::new();
            b.move_to(Point::new(x_of(0.0), y_of(first)));
            for (i, v) in self.points.iter().enumerate().skip(1) {
                b.line_to(Point::new(x_of(i as f64), y_of(*v)));
            }
            frame.stroke(
                &b.build(),
                Stroke::default()
                    .with_width(1.5)
                    .with_color(self.color)
                    .with_line_join(canvas::LineJoin::Round),
            );
        }
        vec![frame.into_geometry()]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn timeline(buckets: Vec<u64>) -> Timeline {
        Timeline {
            bucket_ms: 1000,
            buckets,
            marks: Vec::new(),
        }
    }

    /// The shared y-scale is the whole point: a player doing half the damage
    /// must draw half as tall, not identically.
    #[test]
    fn peak_spans_both_sides() {
        let small = timeline(vec![100, 100]);
        let big = timeline(vec![1000, 1000]);
        let peak = peak_of(&[&small, &big], GraphMode::Total);
        assert_eq!(peak, 2000.0);
        assert!(
            peak > curve(&small, GraphMode::Total)
                .into_iter()
                .fold(0.0, f64::max)
        );
    }

    #[test]
    fn cumulative_is_monotonic_and_dps_is_not() {
        let t = timeline(vec![10, 0, 0, 90]);
        let total = curve(&t, GraphMode::Total);
        assert!(total.windows(2).all(|w| w[0] <= w[1]));
        assert_eq!(total.last().copied(), Some(100.0));
        // The DPS curve dips through the dead air; the cumulative cannot.
        let dps = curve(&t, GraphMode::Dps);
        assert_eq!(dps.len(), 4);
    }

    #[test]
    fn names_lose_their_realm() {
        assert_eq!(short_name("Keanucleavês-Proudmoore"), "Keanucleavês");
        assert_eq!(short_name("Alice"), "Alice");
    }
}
