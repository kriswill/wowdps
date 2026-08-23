//! R12: the two-player comparison — per-spell tables side by side, each over
//! a timeline graph marked with trinket uses, trinket procs and consumables.
//!
//! Everything here is message-generic, so the window and the overlay share it
//! exactly the way they already share `view::bar_row`. Selection lives in the
//! frontends: they wrap [`class_icon`] in their own `mouse_area`, and they
//! hand [`compare_body`] a [`GraphCtl`] naming the messages the graph's own
//! gestures become (drag-select a window, hover a marker, right-click reset),
//! because only they know what a message is.
//!
//! The two graphs deliberately share one y-scale and one x-range. Two curves
//! drawn to their own maxima look identical no matter how far apart the
//! players actually are, which is the one thing a comparison must not do.

use std::rc::Rc;

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
/// v13: externals (Bloodlust, Power Infusion) — violet, nothing else is.
const EXTERNAL: Color = Color::from_rgb(0.85, 0.55, 1.0);

pub(crate) fn mark_color(kind: MarkKind) -> Color {
    match kind {
        MarkKind::TrinketUse => USE,
        MarkKind::TrinketProc => PROC,
        MarkKind::Consumable => CONSUMABLE,
        MarkKind::External => EXTERNAL,
    }
}

fn mark_name(kind: MarkKind) -> &'static str {
    match kind {
        MarkKind::TrinketUse => "trinket use",
        MarkKind::TrinketProc => "proc",
        MarkKind::Consumable => "consumable",
        MarkKind::External => "external",
    }
}

/// Bar color for a player whose class is not known yet.
const CLASSLESS: Color = Color::from_rgb(0.42, 0.44, 0.52);

/// Toward white by `f` — the probe dot's "slightly brighter than the curve".
fn lighten(c: Color, f: f32) -> Color {
    Color::from_rgb(
        c.r + (1.0 - c.r) * f,
        c.g + (1.0 - c.g) * f,
        c.b + (1.0 - c.b) * f,
    )
}

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

/// The graph gestures, named by the frontend (R12/v12): what a drag-selected
/// time window, a marker hover and a right-click reset each become. `hover`
/// echoes the frontend's current hover back in, so BOTH graphs light up
/// every use of the hovered item.
pub(crate) struct GraphCtl<M> {
    pub on_range: Rc<dyn Fn(Option<(u32, u32)>) -> M>,
    pub on_hover: Rc<dyn Fn(Option<String>) -> M>,
    pub hover: Option<String>,
    /// The curve value under the cursor (dps or total, per the mode) — the
    /// canvas publishes it as the pointer moves, the frontend echoes it back
    /// in `probe`, and the legend words it where "graph: dps" sat.
    pub on_probe: Rc<dyn Fn(Option<f64>) -> M>,
    pub probe: Option<f64>,
}

// Manual: a derive would demand `M: Clone` for no reason.
impl<M> Clone for GraphCtl<M> {
    fn clone(&self) -> Self {
        Self {
            on_range: self.on_range.clone(),
            on_hover: self.on_hover.clone(),
            hover: self.hover.clone(),
            on_probe: self.on_probe.clone(),
            probe: self.probe,
        }
    }
}

/// The whole comparison body: two columns, each a header, a spell table and a
/// graph. `scale` multiplies text sizes the way `view::bar_row` does, so the
/// overlay can zoom without iced's scale factor.
pub(crate) fn compare_body<M: 'static>(
    app: &ClientState,
    scale: f32,
    graph_height: f32,
    ctl: GraphCtl<M>,
) -> Element<'static, M> {
    let Some((a, b)) = app.compare_sides() else {
        return waiting(app, scale);
    };
    let mode = app.graph_mode();

    let span = a
        .timeline
        .buckets
        .len()
        .max(b.timeline.buckets.len())
        .max(1);
    // v12: the zoom follows the DAEMON'S echo, not the last request, so the
    // graphs never zoom ahead of the tables they sit under.
    let shown = app.compare_shown_range();
    let bms = a.timeline.bucket_ms.max(b.timeline.bucket_ms).max(1) as usize;
    let view = view_window(shown, bms, span);

    // One scale for both graphs — over the DISPLAYED window — or the
    // comparison lies (see module docs).
    let peak = peak_of(&[&a.timeline, &b.timeline], mode, view);

    let probe = ctl.probe;
    let panes = row![
        side_column(a, mode, peak, view, scale, graph_height, ctl.clone()),
        side_column(b, mode, peak, view, scale, graph_height, ctl),
    ]
    .spacing(10)
    .height(Length::Fill);

    column![panes, legend(mode, shown, scale, probe)]
        .spacing(6)
        .height(Length::Fill)
        .into()
}

/// v14: one player's timeline under the drilldown panes — the comparison's
/// graph and legend for a single side. The frontends hand it the same
/// [`GraphCtl`] gestures (drag zooms, right-click resets, marker hover), but
/// the zoom is purely client-side: the drill timeline always arrives whole,
/// so `shown` is the client's own slice, not a daemon echo.
pub(crate) fn drill_graph<M: 'static>(
    app: &ClientState,
    t: &Timeline,
    class: Option<Class>,
    scale: f32,
    graph_height: f32,
    ctl: GraphCtl<M>,
) -> Element<'static, M> {
    let mode = app.graph_mode();
    let shown = app.drill_range();
    let span = t.buckets.len().max(1);
    let view = view_window(shown, t.bucket_ms.max(1) as usize, span);
    let peak = peak_of(&[t], mode, view);
    let probe = ctl.probe;
    column![
        graph(
            t,
            class_color(class),
            mode,
            peak,
            view,
            graph_height,
            scale,
            ctl
        ),
        legend(mode, shown, scale, probe),
    ]
    .spacing(4)
    .into()
}

/// The displayed bucket window `[lo, hi)` for an echoed ms range. Anything
/// degenerate (a window past the data, a zero-width slice) falls back to the
/// whole span rather than a blank graph.
fn view_window(shown: Option<(u32, u32)>, bucket_ms: usize, span: usize) -> (usize, usize) {
    let Some((lo, hi)) = shown else {
        return (0, span);
    };
    let lo_b = lo as usize / bucket_ms;
    let hi_b = (hi as usize).div_ceil(bucket_ms).min(span);
    if lo_b < hi_b { (lo_b, hi_b) } else { (0, span) }
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
    view: (usize, usize),
    scale: f32,
    graph_height: f32,
    ctl: GraphCtl<M>,
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
        graph(
            &side.timeline,
            color,
            mode,
            peak,
            view,
            graph_height,
            scale,
            ctl,
        ),
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

fn legend<M: 'static>(
    mode: GraphMode,
    shown: Option<(u32, u32)>,
    scale: f32,
    probe: Option<f64>,
) -> Element<'static, M> {
    let key = |kind: MarkKind| {
        row![
            text("▌").size(11.0 * scale).color(mark_color(kind)),
            text(mark_name(kind)).size(10.0 * scale).color(DIM),
        ]
        .spacing(2)
        .align_y(iced::Alignment::Center)
    };
    // Hovering the graph turns the mode label into a readout of the curve
    // under the cursor: "dps: 674.5k" instead of "graph: dps".
    let (label, label_color) = match probe {
        Some(v) => (format!("{}: {}", mode.label(), human(v as u64)), YELLOW),
        None => (format!("graph: {}", mode.label()), DIM),
    };
    let mut line = row![text(label).size(10.0 * scale).color(label_color),]
        .spacing(10)
        .align_y(iced::Alignment::Center);
    // v12: the active window, worded next to the mode so the numbers above
    // are never mistaken for the whole fight. Right-click zooms back out.
    if let Some((lo, hi)) = shown {
        line = line.push(
            text(format!("{}–{} · right-click resets", mmss(lo), mmss(hi)))
                .size(10.0 * scale)
                .color(YELLOW),
        );
    }
    line.push(Space::new().width(Length::Fill))
        .push(key(MarkKind::TrinketUse))
        .push(key(MarkKind::TrinketProc))
        .push(key(MarkKind::Consumable))
        .push(key(MarkKind::External))
        .into()
}

/// "1:23" from ms — graph-axis wording for a moment inside the fight.
fn mmss(ms: u32) -> String {
    let s = ms / 1000;
    format!("{}:{:02}", s / 60, s % 60)
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

fn peak_of(timelines: &[&Timeline], mode: GraphMode, view: (usize, usize)) -> f64 {
    timelines
        .iter()
        .flat_map(|t| {
            let c = curve(t, mode);
            let hi = view.1.min(c.len());
            let lo = view.0.min(hi);
            c[lo..hi].to_vec()
        })
        .fold(0.0f64, f64::max)
}

/// Marker icon strip metrics, in canvas units: the icons sit in a band along
/// the graph's top edge, and hovering that band is what lights an item up.
const ICON_SIZE: f32 = 16.0;
const ICON_BAND: f32 = 20.0;
/// A press-release wander below this is a click, not a selection.
const DRAG_MIN_PX: f32 = 3.0;

#[allow(clippy::too_many_arguments)]
fn graph<M: 'static>(
    t: &Timeline,
    color: Color,
    mode: GraphMode,
    peak: f64,
    view: (usize, usize),
    height: f32,
    scale: f32,
    ctl: GraphCtl<M>,
) -> Element<'static, M> {
    Canvas::new(Graph {
        points: curve(t, mode),
        marks: t.marks.clone(),
        bucket_ms: t.bucket_ms.max(1) as f64,
        color,
        peak,
        view,
        scale,
        ctl,
    })
    .width(Length::Fill)
    .height(Length::Fixed(height))
    .into()
}

struct Graph<M> {
    points: Vec<f64>,
    marks: Vec<Mark>,
    bucket_ms: f64,
    color: Color,
    peak: f64,
    /// Displayed bucket window `[lo, hi)`, shared by both graphs so the same
    /// instant is the same column in both; `(0, span)` when unzoomed.
    view: (usize, usize),
    /// The frontend's zoom: canvas drawing ignores iced's scale factor (the
    /// overlay renders at 1.0 and zooms manually), so anything with a fixed
    /// pixel size — the hover tooltip — must multiply by this itself.
    scale: f32,
    ctl: GraphCtl<M>,
}

#[derive(Default)]
struct GraphState {
    /// An in-progress drag selection: (anchor x, current x).
    drag: Option<(f32, f32)>,
    /// The marker label last reported hovered, so moves don't spam messages.
    hover: Option<String>,
    /// The bucket last reported to `on_probe`, so a move inside one bucket
    /// publishes nothing.
    probe: Option<usize>,
}

impl<M> Graph<M> {
    fn span(&self) -> f64 {
        (self.view.1 - self.view.0).max(1) as f64
    }

    /// Canvas x for a bucket position (fractional buckets fine).
    fn x_of(&self, bucket: f64, w: f32) -> f32 {
        ((bucket - self.view.0 as f64) / self.span()) as f32 * w
    }

    /// The ms-from-segment-start a canvas x lands on.
    fn ms_at(&self, x: f32, w: f32) -> u32 {
        let frac = (x / w).clamp(0.0, 1.0) as f64;
        let bucket = self.view.0 as f64 + frac * self.span();
        (bucket * self.bucket_ms).max(0.0) as u32
    }

    /// The marker whose icon the cursor is over, if any.
    fn mark_at(&self, pos: Point, w: f32) -> Option<&Mark> {
        if pos.y > ICON_BAND {
            return None;
        }
        self.marks
            .iter()
            .filter(|m| self.mark_visible(m))
            .min_by(|a, b| {
                let d = |m: &Mark| (self.mark_x(m, w) - pos.x).abs();
                d(a).total_cmp(&d(b))
            })
            .filter(|m| (self.mark_x(m, w) - pos.x).abs() <= ICON_SIZE / 2.0 + 2.0)
    }

    fn mark_x(&self, m: &Mark, w: f32) -> f32 {
        self.x_of(m.at_ms as f64 / self.bucket_ms, w)
    }

    /// The curve bucket and value under a canvas x, if the curve has one.
    fn probe_at(&self, x: f32, w: f32) -> Option<(usize, f64)> {
        let frac = (x / w).clamp(0.0, 1.0) as f64;
        let b = (self.view.0 as f64 + frac * self.span()).round() as usize;
        let b = b.min(self.view.1.saturating_sub(1));
        self.points.get(b).map(|v| (b, *v))
    }

    fn mark_visible(&self, m: &Mark) -> bool {
        let b = m.at_ms as f64 / self.bucket_ms;
        b >= self.view.0 as f64 && b <= self.view.1 as f64
    }
}

impl<M> canvas::Program<M> for Graph<M> {
    type State = GraphState;

    fn update(
        &self,
        state: &mut GraphState,
        event: &canvas::Event,
        bounds: Rectangle,
        cursor: iced::mouse::Cursor,
    ) -> Option<canvas::Action<M>> {
        use iced::mouse::{Button, Event as Mouse};
        let iced::Event::Mouse(mouse) = event else {
            return None;
        };
        let pos = cursor.position_in(bounds);
        match mouse {
            Mouse::ButtonPressed(Button::Left) => {
                let p = pos?;
                state.drag = Some((p.x, p.x));
                Some(canvas::Action::request_redraw().and_capture())
            }
            Mouse::CursorMoved { .. } => {
                if let Some((_, cur)) = state.drag.as_mut() {
                    // Off-canvas motion keeps scrubbing: clamp to the edge.
                    let x = cursor
                        .position()
                        .map(|p| (p.x - bounds.x).clamp(0.0, bounds.width))?;
                    *cur = x;
                    return Some(canvas::Action::request_redraw());
                }
                // Hover the icon band: report the item under the cursor —
                // both graphs receive the same echo and light up together.
                let over = pos
                    .and_then(|p| self.mark_at(p, bounds.width))
                    .map(|m| m.label.clone());
                if over != state.hover {
                    state.hover = over.clone();
                    return Some(canvas::Action::publish((self.ctl.on_hover)(over)));
                }
                // The curve probe: publish the value under the cursor when
                // the pointer crosses into a new bucket (or leaves), so the
                // legend can word it. A hover change above wins the turn;
                // the probe catches up on the next move.
                let probed = pos.and_then(|p| self.probe_at(p.x, bounds.width));
                if probed.map(|(b, _)| b) != state.probe {
                    state.probe = probed.map(|(b, _)| b);
                    return Some(canvas::Action::publish((self.ctl.on_probe)(
                        probed.map(|(_, v)| v),
                    )));
                }
                None
            }
            Mouse::ButtonReleased(Button::Left) => {
                let (a, b) = state.drag.take()?;
                if (b - a).abs() < DRAG_MIN_PX {
                    return Some(canvas::Action::request_redraw());
                }
                let (lo, hi) = (a.min(b), a.max(b));
                let range = (self.ms_at(lo, bounds.width), self.ms_at(hi, bounds.width));
                Some(canvas::Action::publish((self.ctl.on_range)(Some(range))).and_capture())
            }
            Mouse::ButtonPressed(Button::Right) => {
                pos?;
                // Zoom back out. Captured even when already unzoomed, so a
                // missed right-click never falls through and closes the
                // whole comparison.
                Some(canvas::Action::publish((self.ctl.on_range)(None)).and_capture())
            }
            _ => None,
        }
    }

    fn mouse_interaction(
        &self,
        state: &GraphState,
        bounds: Rectangle,
        cursor: iced::mouse::Cursor,
    ) -> iced::mouse::Interaction {
        if state.drag.is_some() {
            return iced::mouse::Interaction::ResizingHorizontally;
        }
        match cursor.position_in(bounds) {
            Some(p) if self.mark_at(p, bounds.width).is_some() => iced::mouse::Interaction::Pointer,
            Some(_) => iced::mouse::Interaction::Crosshair,
            None => iced::mouse::Interaction::default(),
        }
    }

    fn draw(
        &self,
        state: &GraphState,
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        cursor: iced::mouse::Cursor,
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

        // The curve's 100% mark sits well below the icon strip — two icon
        // heights of air under the band — so a long fight's peaks never run
        // into the icons. Short graphs (the overlay's) cap the headroom at
        // half their height rather than crushing the curve into a ribbon.
        let curve_top = (ICON_BAND + 2.0 * ICON_SIZE).min(h * 0.5);
        let y_of = |v: f64| {
            if self.peak <= 0.0 {
                h
            } else {
                h - (v / self.peak) as f32 * (h - curve_top)
            }
        };

        // Markers first: the curve reads on top of them. While an item is
        // hovered — on either graph — its uses flare and the rest recede.
        let hovered = self.ctl.hover.as_deref();

        // v13: the buff's active span, a light wash from application to
        // removal, under everything else.
        for m in self.marks.iter().filter(|m| self.mark_visible(m)) {
            if m.dur_ms <= 0 {
                continue;
            }
            let x1 = self.mark_x(m, w).clamp(0.0, w);
            let x2 = self
                .x_of((m.at_ms + m.dur_ms) as f64 / self.bucket_ms, w)
                .clamp(0.0, w);
            if x2 <= x1 {
                continue;
            }
            let hit = hovered == Some(m.label.as_str());
            let a = match (hovered, hit) {
                (Some(_), true) => 0.20,
                (Some(_), false) => 0.04,
                (None, _) => 0.10,
            };
            frame.fill(
                &Path::rectangle(Point::new(x1, 0.0), Size::new(x2 - x1, h)),
                Color {
                    a,
                    ..mark_color(m.kind)
                },
            );
        }

        // Where the curve sits at a marker's instant, for hanging its line.
        // Out-of-curve marks (a use after the last bucket) fall to the floor.
        let curve_y_at = |m: &Mark| -> f32 {
            let b = (m.at_ms as f64 / self.bucket_ms).round() as usize;
            self.points.get(b).map(|v| y_of(*v)).unwrap_or(h)
        };

        for m in self.marks.iter().filter(|m| self.mark_visible(m)) {
            let x = self.mark_x(m, w).clamp(0.0, w);
            let hit = hovered == Some(m.label.as_str());
            let (a, width) = match (hovered, hit) {
                (Some(_), true) => (1.0, 2.5),
                (Some(_), false) => (0.25, 1.0),
                (None, _) => (0.75, 1.0),
            };
            // The line drops from the icon and stops where it meets the
            // curve — a full-height line per marker turns a long fight's
            // graph into a picket fence.
            frame.stroke(
                &Path::line(Point::new(x, 0.0), Point::new(x, curve_y_at(m))),
                Stroke::default().with_width(width).with_color(Color {
                    a,
                    ..mark_color(m.kind)
                }),
            );
        }

        let (lo, hi) = (self.view.0.min(self.points.len()), self.view.1);
        let visible = &self.points[lo..hi.min(self.points.len())];
        if visible.len() > 1 {
            let mut b = canvas::path::Builder::new();
            b.move_to(Point::new(self.x_of(lo as f64, w), y_of(visible[0])));
            for (i, v) in visible.iter().enumerate().skip(1) {
                b.line_to(Point::new(self.x_of((lo + i) as f64, w), y_of(*v)));
            }
            frame.stroke(
                &b.build(),
                Stroke::default()
                    .with_width(1.5)
                    .with_color(self.color)
                    .with_line_join(canvas::LineJoin::Round),
            );
        }

        // The probe dot: the curve lit at the bucket under the cursor — a
        // soft glow around a bright core, snapped to the same bucket the
        // legend's readout words.
        if let Some((b, v)) = cursor
            .position_in(bounds)
            .and_then(|p| self.probe_at(p.x, w))
        {
            let c = Point::new(self.x_of(b as f64, w), y_of(v));
            let lit = lighten(self.color, 0.45);
            frame.fill(&Path::circle(c, 6.0 * self.scale), Color { a: 0.20, ..lit });
            frame.fill(&Path::circle(c, 3.5 * self.scale), Color { a: 0.50, ..lit });
            frame.fill(
                &Path::circle(c, 1.0 * self.scale),
                lighten(self.color, 0.65),
            );
        }

        // The item icons over the line, in the top band: the game's own art
        // when the spell-icon cache knows the id, else a kind-colored chip.
        for m in self.marks.iter().filter(|m| self.mark_visible(m)) {
            let x = self
                .mark_x(m, w)
                .clamp(ICON_SIZE / 2.0, w - ICON_SIZE / 2.0);
            let hit = hovered == Some(m.label.as_str());
            let r = Rectangle {
                x: x - ICON_SIZE / 2.0,
                y: 2.0,
                width: ICON_SIZE,
                height: ICON_SIZE,
            };
            match crate::spell_icons::handle(m.spell_id) {
                Some(handle) => {
                    let img = canvas::Image::new(handle).opacity(if hovered.is_some() && !hit {
                        0.35_f32
                    } else {
                        1.0_f32
                    });
                    frame.draw_image(r, img);
                }
                None => frame.fill(
                    &Path::rectangle(Point::new(r.x, r.y), Size::new(r.width, r.height)),
                    Color {
                        a: if hovered.is_some() && !hit { 0.3 } else { 0.9 },
                        ..mark_color(m.kind)
                    },
                ),
            }
            if hit {
                frame.stroke(
                    &Path::rectangle(
                        Point::new(r.x - 1.0, r.y - 1.0),
                        Size::new(r.width + 2.0, r.height + 2.0),
                    ),
                    Stroke::default().with_width(1.5).with_color(Color::WHITE),
                );
            }
        }

        // v13: the info panel for the hovered item — name, kind, use count
        // and uptime, next to the cursor on the graph being hovered (the
        // other graph only lights its marks).
        if let (Some(label), Some(pos)) = (hovered, cursor.position_in(bounds)) {
            let same: Vec<&Mark> = self.marks.iter().filter(|m| m.label == label).collect();
            if let Some(first) = same.first() {
                let uptime_ms: i64 = same.iter().map(|m| m.dur_ms.max(0)).sum();
                let window_ms = (self.span() * self.bucket_ms).max(1.0);
                let mut lines = vec![
                    label.to_string(),
                    format!("{} ×{}", mark_name(first.kind), same.len()),
                ];
                if uptime_ms > 0 {
                    let pct = (uptime_ms as f64 / window_ms * 100.0).min(100.0);
                    lines.push(format!("uptime {}s · {pct:.0}%", uptime_ms / 1000));
                }
                draw_tooltip(
                    &mut frame,
                    w,
                    h,
                    pos,
                    &lines,
                    mark_color(first.kind),
                    self.scale,
                );
            }
        }

        // The in-progress drag selection, over everything.
        if let Some((a, b)) = state.drag
            && (b - a).abs() >= DRAG_MIN_PX
        {
            let (lo, hi) = (a.min(b), a.max(b));
            frame.fill(
                &Path::rectangle(Point::new(lo, 0.0), Size::new(hi - lo, h)),
                Color::from_rgba(1.0, 1.0, 1.0, 0.12),
            );
            for x in [lo, hi] {
                frame.stroke(
                    &Path::line(Point::new(x, 0.0), Point::new(x, h)),
                    Stroke::default()
                        .with_width(1.0)
                        .with_color(Color::from_rgba(1.0, 1.0, 1.0, 0.6)),
                );
            }
        }
        vec![frame.into_geometry()]
    }
}

/// v13: the hover info panel. Hand-drawn — the icons live inside a canvas,
/// where iced's tooltip widget cannot reach. Monospace so the width estimate
/// (canvas text has no measure API here) holds.
#[allow(clippy::too_many_arguments)]
fn draw_tooltip(
    frame: &mut canvas::Frame,
    w: f32,
    h: f32,
    pos: Point,
    lines: &[String],
    accent: Color,
    scale: f32,
) {
    // Reading size beats matching the 10px axis chrome — this panel is the
    // one thing on the graph the user actually stops to read — and it
    // multiplies by the frontend's zoom like every widget outside the canvas.
    let size: f32 = 12.0 * scale;
    let line_h: f32 = 15.0 * scale;
    let pad: f32 = 7.0 * scale;
    // ~0.62em per monospace glyph; chars() so «…» does not overcount.
    let chars = lines.iter().map(|l| l.chars().count()).max().unwrap_or(0);
    let bw = chars as f32 * size * 0.62 + pad * 2.0;
    let bh = lines.len() as f32 * line_h + pad * 2.0 - 3.0 * scale;
    // Beside the cursor, flipped when the edge is near.
    let x = if pos.x + 12.0 + bw > w {
        (pos.x - 12.0 - bw).max(0.0)
    } else {
        pos.x + 12.0
    };
    let y = (pos.y + 10.0).min(h - bh).max(0.0);

    frame.fill(
        &Path::rectangle(Point::new(x, y), Size::new(bw, bh)),
        Color::from_rgba(0.08, 0.09, 0.12, 0.95),
    );
    frame.stroke(
        &Path::rectangle(Point::new(x, y), Size::new(bw, bh)),
        Stroke::default()
            .with_width(1.0)
            .with_color(Color { a: 0.8, ..accent }),
    );
    for (i, line) in lines.iter().enumerate() {
        frame.fill_text(canvas::Text {
            content: line.clone(),
            position: Point::new(x + pad, y + pad + i as f32 * line_h),
            color: if i == 0 { Color::WHITE } else { DIM },
            size: size.into(),
            font: Font::MONOSPACE,
            align_x: iced::alignment::Horizontal::Left.into(),
            align_y: iced::alignment::Vertical::Top,
            ..canvas::Text::default()
        });
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
        let peak = peak_of(&[&small, &big], GraphMode::Total, (0, 2));
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
