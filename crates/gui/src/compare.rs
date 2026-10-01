//! R12: the two-player comparison — per-spell tables side by side, each over
//! a timeline graph marked with trinket uses, trinket procs and consumables.
//!
//! Everything here is message-generic, so either surface can use it.
//! Selection lives in the frontends: they wrap [`class_icon`] in their own
//! `mouse_area`, and the overlay hands [`compare_body`] a [`GraphCtl`]
//! naming the messages the graph's own gestures become (drag-select a
//! window, hover a marker, right-click reset), because only it knows what a
//! message is.
//!
//! [`compare_body`] and [`drill_graph`] are the overlay's, pixel for pixel;
//! their `_in` twins take a [`Look`] and now draw for the overlay alone
//! (tests aside). The window draws its comparison and its graph in the
//! inspector (`inspector::plot`) and uses only [`class_icon`],
//! [`enemy_icon`] and the [`OnRange`] callback type from here.
//!
//! The two graphs deliberately share one y-scale and one x-range. Two curves
//! drawn to their own maxima look identical no matter how far apart the
//! players actually are, which is the one thing a comparison must not do.

use std::rc::Rc;

use iced::widget::canvas::{self, Canvas, Path, Stroke};
use iced::widget::{Space, column, container, row, scrollable, text};
use iced::{Color, Element, Font, Length, Point, Rectangle, Renderer, Size, Theme};

use wowdps_model::fmt::human;
use wowdps_model::{Class, GraphMode, Mark, MarkKind, Row, Spec, Timeline, View};
use wowdps_proto::{ClientState, CompareSide};

use wowdps_gui_logic::graph::{
    self, DRAG_MIN_PX, ICON_BAND, ICON_SIZE, Plot, curve, for_view, hover_line, kinds_shown,
    mark_name, mmss, mode_word, peak_of, probe_line, short_name, side_for_view, view_window,
};

use crate::theme::Look;

/// A marker's colour (gui-logic's `graph::mark_color`), in iced's type.
fn mark_color(kind: MarkKind) -> Color {
    let c = graph::mark_color(kind);
    Color::from_rgba(c.r, c.g, c.b, c.a)
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
use wowdps_gui_logic::labels::class_tag;

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

/// R24: an enemy row's icon — the drawn disc in the hostile red with a skull,
/// the way the game's own enemy pane marks them. An enemy has no class and
/// no spec, so the atlas has nothing for it; the disc is the design.
pub(crate) fn enemy_icon<M: 'static>(slot: Option<usize>, d: f32) -> Element<'static, M> {
    Canvas::new(ClassIcon {
        color: crate::view::HOSTILE,
        tag: "☠",
        slot,
    })
    .width(Length::Fixed(d))
    .height(Length::Fixed(d))
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
pub(crate) type OnRange<M> = Rc<dyn Fn(Option<(u32, u32)>) -> M>;

pub(crate) struct GraphCtl<M> {
    pub on_range: OnRange<M>,
    pub on_hover: Rc<dyn Fn(Option<String>) -> M>,
    pub hover: Option<String>,
    /// The BUCKET under the cursor — an instant, not a value. The canvas
    /// publishes it as the pointer crosses into a new bucket, the frontend
    /// echoes it back in `probe`, and every graph sharing the echo draws its
    /// time cursor there: one instant, the same column in both, each curve
    /// read at it. The legend words each side's value at that instant where
    /// "graph: dps" sat.
    pub on_probe: Rc<dyn Fn(Option<usize>) -> M>,
    pub probe: Option<usize>,
    /// v18: a spell-table row was clicked — drill BOTH sides into that
    /// ability, as (by-spell key, label).
    pub on_spell: Rc<dyn Fn((String, String)) -> M>,
    /// The pointer entered (or left) a spell-table row, by by-spell key. The
    /// frontend echoes it back in `spell_hover`, and BOTH tables light that
    /// ability: a comparison is read across the two lists, and the same
    /// spell sits at different ranks on each.
    pub on_spell_hover: Rc<dyn Fn(Option<String>) -> M>,
    pub spell_hover: Option<String>,
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
            on_spell: self.on_spell.clone(),
            on_spell_hover: self.on_spell_hover.clone(),
            spell_hover: self.spell_hover.clone(),
        }
    }
}

/// The whole comparison body: two columns, each a header, a spell table and a
/// graph. `scale` multiplies text sizes the way `view::bar_row_tagged` does, so the
/// overlay can zoom without iced's scale factor.
pub(crate) fn compare_body<M: Clone + 'static>(
    app: &ClientState,
    scale: f32,
    graph_height: f32,
    // See `legend`: false on the overlay, whose footer toggle already
    // names the curve.
    idle_mode: bool,
    ctl: GraphCtl<M>,
) -> Element<'static, M> {
    compare_body_in(&Look::OVERLAY, app, scale, graph_height, idle_mode, ctl)
}

/// [`compare_body`] in a surface's own [`Look`]: the window's tables in
/// its tabular type with crit in plain ink, where the overlay's are
/// monospace with a yellow crit.
pub(crate) fn compare_body_in<M: Clone + 'static>(
    look: &Look,
    app: &ClientState,
    scale: f32,
    graph_height: f32,
    idle_mode: bool,
    ctl: GraphCtl<M>,
) -> Element<'static, M> {
    let Some((a, b)) = app.compare_sides() else {
        return waiting(look, app, scale);
    };
    let mode = app.graph_mode();
    // v29: WHICH metric this comparison is about, from the snapshot's own
    // echo. A Taken comparison's tables are the abilities that hit them, its
    // curves the taken series, and its mitigation records ride under them.
    let metric = app.compare_view();
    // v34: the graphs draw only the marks this view is about.
    let (a, b) = (&side_for_view(a, metric), &side_for_view(b, metric));

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
    // comparison lies (see module docs). v18: while an ability is drilled,
    // the scale spans the ghosts AND both focus curves the same way.
    let mut scaled: Vec<&Timeline> = vec![&a.timeline, &b.timeline];
    for side in [a, b] {
        if let Some(ft) = &side.spell_timeline {
            scaled.push(ft);
        }
    }
    let peak = peak_of(&scaled, mode, view);

    // v27: one instant, read on BOTH curves — the cursor is shared, so the
    // readout names each side rather than reporting whichever graph the
    // pointer happens to be over.
    let probe = ctl.probe.map(|at| {
        probe_line(
            at,
            bms,
            mode_word(mode, crate::view::rate_label(metric)),
            &[
                (short_name(&a.total.label), curve(&a.timeline, mode)),
                (short_name(&b.total.label), curve(&b.timeline, mode)),
            ],
        )
    });
    // R18: casters resolve through the two sides and the meter rows in hand
    // (key = guid, label = name); an external from a third player names them.
    let rows = app.rows();
    let names: Vec<(&str, &str)> = [(a.guid.as_str(), a.total.label.as_str())]
        .into_iter()
        .chain([(b.guid.as_str(), b.total.label.as_str())])
        .chain(rows.iter().map(|r| (r.key.as_str(), r.label.as_str())))
        .collect();
    let timelines = [&a.timeline, &b.timeline];
    // Same order as `timelines`, so a hovered marker's count splits by side.
    let side_names = [short_name(&a.total.label), short_name(&b.total.label)];
    let hovered = ctl
        .hover
        .as_deref()
        .and_then(|l| hover_line(&timelines, l, view, &names, idle_mode, &side_names));
    let kinds = kinds_shown(&timelines, view);
    // v18: the comparison's ability drill — both sides locked to one spell,
    // stats + focus curve each; back out with the usual Esc/right-click.
    let spell = app.compare_spell().cloned();
    let panes = row![
        side_column(
            look,
            a,
            metric,
            mode,
            peak,
            view,
            scale,
            graph_height,
            spell.clone(),
            ctl.clone()
        ),
        side_column(
            look,
            b,
            metric,
            mode,
            peak,
            view,
            scale,
            graph_height,
            spell,
            ctl
        ),
    ]
    .spacing(10)
    .height(Length::Fill);

    column![
        panes,
        legend(
            look,
            mode,
            shown,
            scale,
            probe,
            crate::view::rate_label(metric),
            hovered,
            &kinds,
            idle_mode
        )
    ]
    .spacing(6)
    .height(Length::Fill)
    .into()
}

/// v14: one player's timeline under the drilldown panes — the comparison's
/// graph and legend for a single side. The frontends hand it the same
/// [`GraphCtl`] gestures (drag zooms, right-click resets, marker hover), but
/// the zoom is purely client-side: the drill timeline always arrives whole,
/// so `shown` is the client's own slice, not a daemon echo.
#[allow(clippy::too_many_arguments)]
pub(crate) fn drill_graph<M: 'static>(
    app: &ClientState,
    t: &Timeline,
    class: Option<Class>,
    scale: f32,
    graph_height: f32,
    // What the rate curve is called here — "dps", or "hps" on a Healing
    // drilldown (the buckets are that view's own metric, v14).
    rate: &'static str,
    // See `legend`: false on the overlay, whose footer toggle already
    // names the curve.
    idle_mode: bool,
    // v16: the ability drill — this timeline becomes the FOCUS curve in the
    // given color (its school's), and `t` fades into the ghost behind it.
    focus: Option<(&Timeline, Color)>,
    ctl: GraphCtl<M>,
) -> Element<'static, M> {
    drill_graph_in(
        &Look::OVERLAY,
        app,
        t,
        class,
        scale,
        graph_height,
        rate,
        idle_mode,
        focus,
        ctl,
    )
}

/// [`drill_graph`] in a surface's own [`Look`]: the legend's read-out and
/// zoom window in the window's gold, where the overlay's are yellow.
#[allow(clippy::too_many_arguments)]
pub(crate) fn drill_graph_in<M: 'static>(
    look: &Look,
    app: &ClientState,
    t: &Timeline,
    class: Option<Class>,
    scale: f32,
    graph_height: f32,
    rate: &'static str,
    idle_mode: bool,
    focus: Option<(&Timeline, Color)>,
    ctl: GraphCtl<M>,
) -> Element<'static, M> {
    // v34: the marks this VIEW is about, on the ghost and the focus alike.
    let narrowed = for_view(t, app.view);
    let t = &narrowed;
    let focus_narrowed = focus.map(|(ft, c)| (for_view(ft, app.view), c));
    let focus = focus_narrowed.as_ref().map(|(ft, c)| (ft, *c));
    let mode = app.graph_mode();
    let shown = app.drill_range();
    // The view window always spans the PLAYER's timeline: the ability's
    // buckets share the same grid, and the x-axis must not reshape when
    // drilling in or out.
    let span = t.buckets.len().max(1);
    let view = view_window(shown, t.bucket_ms.max(1) as usize, span);
    // One side: the readout keeps the bare "dps: 674.5k" wording, now with
    // the instant in front of it — the same cursor the comparison draws.
    let probe = ctl.probe.map(|at| {
        let points = focus.map_or_else(|| curve(t, mode), |(ft, _)| curve(ft, mode));
        probe_line(
            at,
            t.bucket_ms.max(1) as usize,
            mode_word(mode, rate),
            &[(String::new(), points)],
        )
    });
    // R18: casters resolve through the meter rows in hand (key = guid,
    // label = name) — the drilled player's segment-mates included.
    let rows = app.rows();
    let names: Vec<(&str, &str)> = rows
        .iter()
        .map(|r| (r.key.as_str(), r.label.as_str()))
        .collect();
    let hovered = ctl
        .hover
        .as_deref()
        .and_then(|l| hover_line(&[t], l, view, &names, idle_mode, &[]));
    let kinds = kinds_shown(&[t], view);
    let body = match focus {
        Some((ft, fc)) => graph(
            ft,
            fc,
            mode,
            // One y-scale over both curves, or the share reads wrong.
            peak_of(&[t, ft], mode, view),
            view,
            graph_height,
            scale,
            app.encounter_spans(),
            Some((t, class_color(class))),
            ctl,
            look,
        ),
        None => graph(
            t,
            class_color(class),
            mode,
            peak_of(&[t], mode, view),
            view,
            graph_height,
            scale,
            // v14: on a Σ drilldown, underline where the boss fights ran.
            app.encounter_spans(),
            None,
            ctl,
            look,
        ),
    };
    column![
        body,
        legend(
            look, mode, shown, scale, probe, rate, hovered, &kinds, idle_mode
        )
    ]
    .spacing(4)
    .into()
}

/// Shown while a pair is picked but the daemon has not answered yet — and,
/// more importantly, when only one player is picked, which is the state the
/// user spends the most time in.
fn waiting<M: 'static>(look: &Look, app: &ClientState, scale: f32) -> Element<'static, M> {
    let msg = graph::waiting_words(app.compare_picks());
    container(text(msg).size(13.0 * scale).color(look.dim))
        .center_x(Length::Fill)
        .center_y(Length::Fill)
        .into()
}

#[allow(clippy::too_many_arguments)]
fn side_column<M: Clone + 'static>(
    look: &Look,
    side: &CompareSide,
    // v29: what the numbers mean — the table's own wording, the header's
    // rate, and whether a mitigation record belongs under the table.
    metric: View,
    mode: GraphMode,
    peak: f64,
    view: (usize, usize),
    scale: f32,
    graph_height: f32,
    // v18: the drilled ability — replaces the spell table with its stats
    // and focuses its curve over this side's ghost.
    spell: Option<(String, String)>,
    ctl: GraphCtl<M>,
) -> Element<'static, M> {
    // The curve is data and keeps the raw class colour; the NAME is text,
    // and the window lifts it to read.
    let color = class_color(side.total.class);
    let name_ink = side.total.class.map_or(color, |c| look.class_ink(c));
    let header = row![
        class_icon(side.total.class, side.total.spec, Some(0), 18.0 * scale),
        text(short_name(&side.total.label))
            .size(14.0 * scale)
            .color(name_ink),
        Space::new().width(Length::Fill),
        text(human(side.total.amount))
            .size(13.0 * scale)
            .font(look.num),
        // v29: the rate is the METRIC's — "dtps" on a Taken comparison, not
        // "dps" over a number that is damage taken.
        text(format!(
            "{} {}",
            human(side.total.per_sec as u64),
            crate::view::rate_label(metric)
        ))
        .size(12.0 * scale)
        .color(look.dim)
        .font(look.num),
    ]
    .spacing(6)
    .align_y(iced::Alignment::Center);

    // v18: the ability variant — breadcrumb-lite (name + school tag come
    // from the shared helpers), stat cards, and the focused graph.
    if let Some((key, label)) = spell {
        let srow = side.spells.iter().find(|r| r.key == key).cloned();
        let middle: Element<'static, M> = match &srow {
            Some(r) => column![
                crate::view::spell_breadcrumb_in::<M>(
                    look,
                    &short_name(&side.total.label),
                    // The window names the player in their class ink, as
                    // the header above does; the overlay in its focus.
                    look.class_text.then_some(name_ink),
                    &label,
                    Some(r),
                    scale
                ),
                crate::view::spell_stats_in::<M>(look, r, wowdps_model::View::Damage, scale),
            ]
            .spacing(8.0 * scale)
            .height(Length::Fill)
            .into(),
            None => container(
                text(format!("did not cast {label}"))
                    .size(12.0 * scale)
                    .color(look.dim),
            )
            .center_x(Length::Fill)
            .height(Length::Fill)
            .into(),
        };
        let focus_color = srow
            .and_then(|r| crate::view::school_color(r.school))
            .unwrap_or(look.focus);
        let g = match &side.spell_timeline {
            Some(ft) => graph(
                ft,
                focus_color,
                mode,
                peak,
                view,
                graph_height,
                scale,
                Vec::new(),
                Some((&side.timeline, color)),
                ctl,
                look,
            ),
            // No focus curve: this side never cast it — its own line keeps
            // the pane comparable, dimmed by the shared y-scale as it is.
            None => graph(
                &side.timeline,
                color,
                mode,
                peak,
                view,
                graph_height,
                scale,
                Vec::new(),
                None,
                ctl,
                look,
            ),
        };
        return column![header, middle, g]
            .spacing(6)
            .width(Length::FillPortion(1))
            .height(Length::Fill)
            .into();
    }

    // R17: what this side AVOIDED, under what it took — the same one-line
    // record a Taken drill carries, per side, so "he took more" and "he
    // dodged less" are read together. Absent on every other metric.
    let mitigation: Element<'static, M> = match &side.mitigation {
        Some(m) => container(
            text(crate::view::mitigation_line(m, side.total.amount))
                .size(look.text(9.0 * scale))
                .color(look.dim)
                .font(look.num)
                .wrapping(iced::widget::text::Wrapping::None),
        )
        .clip(true)
        .padding([0, 6])
        .into(),
        None => Space::new().height(Length::Fixed(0.0)).into(),
    };
    column![
        header,
        spell_table(look, &side.spells, metric, scale, &ctl),
        mitigation,
        graph(
            &side.timeline,
            color,
            mode,
            peak,
            view,
            graph_height,
            scale,
            // No encounter lane on the comparison: while comparing, the
            // cached snapshot can lag the watched segment, so the spans
            // could belong to another fight.
            Vec::new(),
            None,
            ctl,
            look,
        ),
    ]
    .spacing(6)
    .width(Length::FillPortion(1))
    .height(Length::Fill)
    .into()
}

/// A numeric column of the window's comparison table ([`Look::fit`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Cell {
    Hits,
    Crit,
    Avg,
    /// The one number of a count view.
    Count,
}

impl Cell {
    /// Its width before the surface's scale: the widest figure it holds in
    /// the window's tabular type ("2140", "100%", "136.8k"), with air.
    fn width(self) -> f32 {
        match self {
            Cell::Hits | Cell::Crit => 34.0,
            Cell::Avg | Cell::Count => 42.0,
        }
    }

    fn head(self) -> &'static str {
        match self {
            Cell::Hits => "Hits",
            Cell::Crit => "Crit",
            Cell::Avg => "Avg",
            Cell::Count => "Count",
        }
    }

    fn text(self, r: &Row) -> String {
        match self {
            Cell::Hits | Cell::Count => r.count.to_string(),
            Cell::Crit if r.count > 0 => format!("{:.0}%", r.crit_pct()),
            Cell::Avg => r
                .amount
                .checked_div(r.count)
                .map_or_else(|| "—".to_string(), human),
            Cell::Crit => "—".to_string(),
        }
    }

    fn ink(self, look: &Look) -> Color {
        match self {
            Cell::Crit => look.crit,
            _ => look.ink,
        }
    }
}

/// What a comparison row's name keeps before a column gives way: ten or so
/// characters of the window's type — enough to tell "Melee (Phuulum)" from
/// "Melee (Dreadstalker)" by their start.
const CMP_NAME_MIN: f32 = 64.0;

/// The right lane a table keeps clear of its scrollbar — rows and heads
/// alike, so a head ends where its figures do.
const CMP_LANE: f32 = 10.0;

/// The cells a window table of `width` keeps: crit gives way first, then
/// the average — the order a meter pane gave its columns up in — and one
/// always stays.
fn fit_cells(cells: &[Cell], width: f32, scale: f32) -> Vec<Cell> {
    // Padding both sides, the lane, the icon, and a 4 px gap before each
    // cell and the name.
    let fixed = |kept: &[Cell]| {
        2.0 * 6.0 * scale
            + CMP_LANE
            + 13.0 * scale
            + 4.0 * (kept.len() + 1) as f32
            + kept.iter().map(|c| c.width() * scale).sum::<f32>()
    };
    let mut kept = cells.to_vec();
    for drop in [Cell::Crit, Cell::Avg] {
        if width - fixed(&kept) >= CMP_NAME_MIN || kept.len() == 1 {
            break;
        }
        kept.retain(|c| *c != drop);
    }
    kept
}

/// The window's comparison table: the overlay's columns fitted to the
/// pane — a narrow window's half-width pane keeps the names by giving up
/// crit, then the average — and its heads seated over its figures: one
/// column list, one padding and one scrollbar lane for both.
fn fitted_spell_table<M: Clone + 'static>(
    look: &Look,
    spells: &[Row],
    scale: f32,
    ctl: &GraphCtl<M>,
    (title, empty): (&'static str, &'static str),
    count_only: bool,
) -> Element<'static, M> {
    let look = *look;
    let spells = spells.to_vec();
    let ctl = ctl.clone();
    iced::widget::responsive(move |bounds| {
        let all: &[Cell] = if count_only {
            &[Cell::Count]
        } else {
            &[Cell::Hits, Cell::Crit, Cell::Avg]
        };
        let cells = fit_cells(all, bounds.width, scale);
        let lane = |content: iced::widget::Row<'static, M>| {
            container(
                content
                    .spacing(4)
                    .padding([0.0, 6.0 * scale])
                    .align_y(iced::Alignment::Center),
            )
            .padding(iced::Padding {
                top: 0.0,
                right: CMP_LANE,
                bottom: 0.0,
                left: 0.0,
            })
        };
        let mut heading = row![
            Space::new().width(Length::Fixed(13.0 * scale)),
            text(crate::nav::sentence(title))
                .size(10.0 * scale)
                .color(look.label)
                .width(Length::Fill),
        ];
        for c in &cells {
            heading = heading.push(
                text(c.head())
                    .size(10.0 * scale)
                    .color(look.label)
                    .font(look.num)
                    .width(Length::Fixed(c.width() * scale))
                    .align_x(iced::Alignment::End),
            );
        }
        let mut list = column![].spacing(2);
        if spells.is_empty() {
            list = list.push(
                text(crate::nav::sentence(empty))
                    .size(12.0 * scale)
                    .color(look.dim),
            );
        }
        for r in &spells {
            let hovered = ctl.spell_hover.as_deref() == Some(r.key.as_str());
            let icon: Element<'static, M> = match crate::spell_icons::handle(r.spell_id) {
                Some(h) => iced::widget::image(h)
                    .width(Length::Fixed(13.0 * scale))
                    .height(Length::Fixed(13.0 * scale))
                    .into(),
                None => Space::new().width(Length::Fixed(13.0 * scale)).into(),
            };
            let mut line = row![
                icon,
                container(crate::ellipsis::ellipsis(r.label.clone()).size(11.0 * scale))
                    .clip(true)
                    .width(Length::Fill),
            ];
            for c in &cells {
                line = line.push(
                    text(c.text(r))
                        .size(11.0 * scale)
                        .color(c.ink(&look))
                        .font(look.num)
                        .width(Length::Fixed(c.width() * scale))
                        .align_x(iced::Alignment::End),
                );
            }
            let wash = look;
            list = list.push(
                iced::widget::mouse_area(
                    container(
                        line.spacing(4)
                            .padding([0.0, 6.0 * scale])
                            .align_y(iced::Alignment::Center),
                    )
                    .style(move |_: &iced::Theme| crate::view::hover_style_in(&wash, hovered)),
                )
                .on_press((ctl.on_spell)((r.key.clone(), r.label.clone())))
                .on_enter((ctl.on_spell_hover)(Some(r.key.clone())))
                .on_exit((ctl.on_spell_hover)(None)),
            );
        }
        let rows = container(list).padding(iced::Padding {
            top: 0.0,
            right: CMP_LANE,
            bottom: 0.0,
            left: 0.0,
        });
        column![lane(heading), scrollable(rows).height(Length::Fill)]
            .spacing(3)
            .height(Length::Fill)
            .into()
    })
    .into()
}

/// Column widths for the per-spell table: (hits, crit%, average).
const COLS: (f32, f32, f32) = (44.0, 46.0, 56.0);

fn spell_table<M: Clone + 'static>(
    look: &Look,
    spells: &[Row],
    metric: View,
    scale: f32,
    ctl: &GraphCtl<M>,
) -> Element<'static, M> {
    let head = |s: &str, w: f32| {
        text(s.to_string())
            .size(10.0 * scale)
            .color(look.label)
            .font(look.num)
            .width(Length::Fixed(w * scale))
            .align_x(iced::Alignment::End)
    };
    // v29: a Taken table lists the abilities that hit them, so it says so —
    // and the count views have no crits and no meaningful average, exactly
    // as the drill's panes already word them.
    let (title, empty) = graph::table_words(metric);
    let count_only = wowdps_gui_logic::drill::counts(metric);
    if look.fit {
        return fitted_spell_table(look, spells, scale, ctl, (title, empty), count_only);
    }
    let mut heading = row![
        text(title).size(10.0 * scale).color(look.label),
        Space::new().width(Length::Fill),
    ]
    .spacing(4)
    .padding([0, 6]);
    heading = if count_only {
        heading.push(head("count", COLS.2))
    } else {
        heading
            .push(head("hits", COLS.0))
            .push(head("crit", COLS.1))
            .push(head("avg", COLS.2))
    };

    let mut list = column![].spacing(2);
    if spells.is_empty() {
        list = list.push(text(empty).size(12.0 * scale).color(look.dim));
    }
    for r in spells {
        // v18: a spell row drills BOTH sides into that ability. Hovering it
        // lights the SAME ability on the other side too — the two lists are
        // sorted independently, so finding it by eye is the work the
        // comparison is supposed to save.
        let hovered = ctl.spell_hover.as_deref() == Some(r.key.as_str());
        list = list.push(
            iced::widget::mouse_area(spell_row::<M>(look, r, count_only, scale, hovered))
                .on_press((ctl.on_spell)((r.key.clone(), r.label.clone())))
                .on_enter((ctl.on_spell_hover)(Some(r.key.clone())))
                .on_exit((ctl.on_spell_hover)(None)),
        );
    }

    // The right lane keeps the avg column clear of the scrollbar's overlay.
    let cleared = container(list).padding(iced::Padding {
        top: 0.0,
        right: 10.0,
        bottom: 0.0,
        left: 0.0,
    });
    column![heading, scrollable(cleared).height(Length::Fill)]
        .spacing(3)
        .height(Length::Fill)
        .into()
}

fn spell_row<M: 'static>(
    look: &Look,
    r: &Row,
    // v29: an interrupt or a dispel has one number — the count. Drawing a
    // crit rate and an average over it would be three columns of noise.
    count_only: bool,
    scale: f32,
    hovered: bool,
) -> Element<'static, M> {
    let cell = |s: String, w: f32, color: Color| {
        text(s)
            .size(11.0 * scale)
            .color(color)
            .font(look.num)
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
    // The three numbers the comparison exists for (gui-logic's words).
    let avg = wowdps_gui_logic::drill::avg_text(r);
    let crit = wowdps_gui_logic::drill::crit_text(r);

    let mut line = row![
        icon,
        // Fill + NoWrap inside a clipping container: without the clip, iced
        // paints the one-line overflow under the number columns.
        container(
            text(r.label.clone())
                .size(11.0 * scale)
                .wrapping(iced::widget::text::Wrapping::None),
        )
        .clip(true)
        .width(Length::Fill),
    ];
    line = if count_only {
        line.push(cell(r.count.to_string(), COLS.2, look.ink))
    } else {
        line.push(cell(r.count.to_string(), COLS.0, look.ink))
            .push(cell(crit, COLS.1, look.crit))
            .push(cell(avg, COLS.2, look.ink))
    };
    let line = line
        .spacing(4)
        .padding([0, 6])
        .align_y(iced::Alignment::Center);
    let wash = *look;
    container(line)
        .style(move |_: &iced::Theme| crate::view::hover_style_in(&wash, hovered))
        .into()
}

#[allow(clippy::too_many_arguments)]
fn legend<M: 'static>(
    look: &Look,
    mode: GraphMode,
    shown: Option<(u32, u32)>,
    scale: f32,
    // The probed instant and what each curve says there (`probe_line`), or
    // `None` when the pointer is off the curve. The instant is context; the
    // readings are the answer, ONE PER GRAPH in graph order — with two of
    // them the row splits into halves that meet at the divider, so each
    // number sits under the curve it describes.
    probe: Option<(String, Vec<String>)>,
    rate: &'static str,
    hover: Option<(MarkKind, String, String)>,
    // R18: the kinds that get a key — `kinds_shown` over the displayed
    // marks, so the row explains only bars that are actually on screen.
    kinds: &[MarkKind],
    // Show "graph: dps" while idle. The overlay passes false — its footer's
    // dps/total toggle already says which curve is up — the window, which
    // has no toggle, keeps the label. The hover readout shows regardless.
    idle_mode: bool,
) -> Element<'static, M> {
    let key = |kind: MarkKind| {
        row![
            text("▌").size(11.0 * scale).color(mark_color(kind)),
            text(mark_name(kind)).size(10.0 * scale).color(look.dim),
        ]
        .spacing(2)
        .align_y(iced::Alignment::Center)
    };
    // A hovered marker takes the whole row over: its name and numbers where
    // the mode label and keys usually sit, so nothing draws over the curve.
    if let Some((kind, name, details)) = hover {
        return row![
            text("▌").size(11.0 * scale).color(mark_color(kind)),
            text(name)
                .size(10.0 * scale)
                .color(look.ink)
                .wrapping(iced::widget::text::Wrapping::None),
            text(details).size(10.0 * scale).color(look.dim),
        ]
        .spacing(6)
        .align_y(iced::Alignment::Center)
        .into();
    }
    // Hovering the graph turns the mode label into a readout of the curve
    // under the cursor: "dps: 674.5k" instead of "graph: dps" — and the rate
    // word is the view's own ("hps" on a Healing drilldown, v14).
    let word = mode_word(mode, rate);
    let focus = look.focus;
    let reading = |s: String| text(s).size(10.0 * scale).color(focus);
    let window = shown.map(|(lo, hi)| {
        text(format!("{}–{} · right-click resets", mmss(lo), mmss(hi)))
            .size(10.0 * scale)
            .color(focus)
    });
    // Two curves, two readings: the row becomes two halves the width of the
    // panes above it, the left one right-aligned and the right one
    // left-aligned, so the pair meets AT the divider and each number is
    // under its own graph. One number under the left graph describing both
    // is what this replaces.
    if let Some((when, values)) = &probe
        && let [left_read, right_read] = values.as_slice()
    {
        let mut left = row![
            text(format!("{when} · {word}"))
                .size(10.0 * scale)
                .color(look.dim)
        ]
        .spacing(10)
        .align_y(iced::Alignment::Center);
        if let Some(w) = window {
            left = left.push(w);
        }
        let left = left
            .push(Space::new().width(Length::Fill))
            .push(reading(left_read.clone()));
        let mut right = row![reading(right_read.clone())]
            .spacing(10)
            .align_y(iced::Alignment::Center)
            .push(Space::new().width(Length::Fill));
        for kind in kinds {
            right = right.push(key(*kind));
        }
        return row![
            container(left).width(Length::FillPortion(1)),
            container(right).width(Length::FillPortion(1)),
        ]
        // The panes above are `FillPortion(1)` a `spacing(10)` apart; the
        // halves match so the two readings land either side of the seam.
        .spacing(10)
        .align_y(iced::Alignment::Center)
        .into();
    }
    let mut line = row![].spacing(10).align_y(iced::Alignment::Center);
    match probe {
        // One graph: the instant leads, dim — it is the context — and the
        // single reading follows it.
        Some((when, values)) => {
            line = line.push(text(when).size(10.0 * scale).color(look.dim));
            for v in values.into_iter().filter(|v| !v.is_empty()) {
                line = line.push(reading(v));
            }
        }
        None if idle_mode => {
            line = line.push(
                text(format!("graph: {word}"))
                    .size(10.0 * scale)
                    .color(look.dim),
            );
        }
        None => {}
    }
    // v12: the active window, worded next to the mode so the numbers above
    // are never mistaken for the whole fight. Right-click zooms back out.
    if let Some(w) = window {
        line = line.push(w);
    }
    line = line.push(Space::new().width(Length::Fill));
    for kind in kinds {
        line = line.push(key(*kind));
    }
    line.into()
}

// ---- the graph -------------------------------------------------------------

#[allow(clippy::too_many_arguments)]
fn graph<M: 'static>(
    t: &Timeline,
    color: Color,
    mode: GraphMode,
    peak: f64,
    view: (usize, usize),
    height: f32,
    scale: f32,
    spans: Vec<(u32, u32)>,
    ghost: Option<(&Timeline, Color)>,
    ctl: GraphCtl<M>,
    look: &Look,
) -> Element<'static, M> {
    Canvas::new(Graph {
        geo: Plot::new(t, mode, peak, view, spans),
        color,
        plot: look.plot,
        lane: look.good,
        scale,
        ghost: ghost.map(|(g, c)| (curve(g, mode), c)),
        ctl,
    })
    .width(Length::Fill)
    .height(Length::Fixed(height))
    .into()
}

struct Graph<M> {
    /// The curve, its marks and the displayed window, scaled (gui-logic).
    geo: Plot,
    color: Color,
    /// The frontend's zoom: canvas drawing ignores iced's scale factor (the
    /// overlay renders at 1.0 and zooms manually), so anything with a fixed
    /// pixel size — the hover tooltip — must multiply by this itself.
    scale: f32,
    /// v16: a context curve drawn faded UNDER the main one — the player's
    /// whole line behind the drilled ability's, so "when did this spell
    /// matter" reads against "when did the player do anything". Shares the
    /// y-scale (`peak` covers both).
    ghost: Option<(Vec<f64>, Color)>,
    ctl: GraphCtl<M>,
    /// The plot area's fill: the surface's `Look::plot`.
    plot: Color,
    /// The encounter lane's green: the surface's `Look::good`, the colour
    /// its kills wear.
    lane: Color,
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

/// The graph's geometry is gui-logic's [`Plot`]; this wraps it with what
/// only iced draws — the colours, the zoom, the ghost, the gestures — and
/// reads through to it.
impl<M> std::ops::Deref for Graph<M> {
    type Target = Plot;

    fn deref(&self) -> &Plot {
        &self.geo
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
                    .and_then(|p| self.mark_at(p.x, p.y, bounds.width))
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
                    return Some(canvas::Action::publish((self.ctl.on_probe)(state.probe)));
                }
                None
            }
            Mouse::ButtonReleased(Button::Left) => {
                let (a, b) = state.drag.take()?;
                let Some(range) = self.drag_range(a, b, bounds.width) else {
                    return Some(canvas::Action::request_redraw());
                };
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
            Some(p) if self.mark_at(p.x, p.y, bounds.width).is_some() => {
                iced::mouse::Interaction::Pointer
            }
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

        frame.fill(&Path::rectangle(Point::ORIGIN, Size::new(w, h)), self.plot);
        // Baseline: without it an empty graph is indistinguishable from a
        // missing one.
        frame.stroke(
            &Path::line(Point::new(0.0, h - 0.5), Point::new(w, h - 0.5)),
            Stroke::default()
                .with_width(1.0)
                .with_color(Color::from_rgba(1.0, 1.0, 1.0, 0.15)),
        );

        // The curve's 100% mark sits well below the icon strip, its floor
        // over the encounter lane when there is one (`Plot::y_of`).
        let lane_h = Plot::lane_h(self.scale);
        let floor = self.floor(self.scale);
        let y_of = move |v: f64| self.y_of(v, h, floor);

        // v14: the encounter lane — bars along the bottom edge marking where
        // the visit's boss fights ran, in the green the surface's kills wear
        // (`Look::good`). Drawn under everything, and below the curve's
        // raised floor, so the line and the lane never touch.
        for &(lo, hi) in &self.spans {
            let x1 = self.x_of(lo as f64 / self.bucket_ms, w).clamp(0.0, w);
            let x2 = self.x_of(hi as f64 / self.bucket_ms, w).clamp(0.0, w);
            if x2 <= x1 {
                continue;
            }
            frame.fill(
                &Path::rectangle(Point::new(x1, h - lane_h), Size::new(x2 - x1, lane_h)),
                Color {
                    a: 0.9,
                    ..self.lane
                },
            );
        }

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

        // Where the curve sits at a marker's instant, for hanging its line
        // (`Plot::hang_y`: the higher of the focus and the ghost there).
        let ghost = self.ghost.as_ref().map(|(g, _)| g.as_slice());
        let curve_y_at = |m: &Mark| -> f32 { self.hang_y(m, ghost, h, floor) };

        for m in self.marks.iter().filter(|m| self.mark_visible(m)) {
            let x = self.mark_x(m, w).clamp(0.0, w);
            let hit = hovered == Some(m.label.as_str());
            // Quiet by default: the markers are wayfinding, the curve is the
            // content — many procs must never bury the line they annotate.
            let (a, width) = match (hovered, hit) {
                (Some(_), true) => (1.0, 2.5),
                (Some(_), false) => (0.12, 1.0),
                (None, _) => (0.40, 1.0),
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

        // v16: the context curve first, faded, so the focus line on top
        // reads as "this ability's share of that".
        if let Some((ghost, gc)) = &self.ghost {
            let hi = self.view.1.min(ghost.len());
            let lo = self.view.0.min(hi);
            if let Some(visible) = ghost.get(lo..hi)
                && let Some(first) = visible.first()
            {
                let mut b = canvas::path::Builder::new();
                b.move_to(Point::new(self.x_of(lo as f64, w), y_of(*first)));
                for (i, v) in visible.iter().enumerate().skip(1) {
                    b.line_to(Point::new(self.x_of((lo + i) as f64, w), y_of(*v)));
                }
                frame.stroke(
                    &b.build(),
                    Stroke::default()
                        .with_width(1.0)
                        .with_color(Color { a: 0.30, ..*gc })
                        .with_line_join(canvas::LineJoin::Round),
                );
            }
        }

        let (lo, hi) = (self.view.0.min(self.points.len()), self.view.1);
        let visible = self
            .points
            .get(lo..hi.min(self.points.len()))
            .unwrap_or_default();
        if let Some(first) = visible.first()
            && visible.len() > 1
        {
            let mut b = canvas::path::Builder::new();
            b.move_to(Point::new(self.x_of(lo as f64, w), y_of(*first)));
            for (i, v) in visible.iter().enumerate().skip(1) {
                b.line_to(Point::new(self.x_of((lo + i) as f64, w), y_of(*v)));
            }
            frame.stroke(
                &b.build(),
                Stroke::default()
                    // A touch heavier than the markers and the ghost, so the
                    // main line reads first at any zoom.
                    .with_width(2.0)
                    .with_color(self.color)
                    .with_line_join(canvas::LineJoin::Round),
            );
        }

        // The probe highlight: the curve itself lit around the bucket under
        // the cursor — layered strokes ALONG the line, widest and faintest
        // over the longest window, so the glow is masked by the line instead
        // of sitting on it as a blob. Ends taper as the layers shorten.
        if let Some((b, _)) = cursor
            .position_in(bounds)
            .and_then(|p| self.probe_at(p.x, w))
        {
            let lit = lighten(self.color, 0.45);
            let segment = |half: usize| -> Option<Path> {
                let lo = b.saturating_sub(half).max(self.view.0);
                let hi = (b + half + 1).min(self.view.1).min(self.points.len());
                let pts = self.points.get(lo..hi)?;
                if pts.len() < 2 {
                    return None;
                }
                let mut path = canvas::path::Builder::new();
                path.move_to(Point::new(self.x_of(lo as f64, w), y_of(*pts.first()?)));
                for (i, v) in pts.iter().enumerate().skip(1) {
                    path.line_to(Point::new(self.x_of((lo + i) as f64, w), y_of(*v)));
                }
                Some(path.build())
            };
            for (half, width, color) in [
                (4, 4.5, Color { a: 0.22, ..lit }),
                (2, 2.5, Color { a: 0.55, ..lit }),
                (1, 1.5, lighten(self.color, 0.65)),
            ] {
                if let Some(path) = segment(half) {
                    frame.stroke(
                        &path,
                        Stroke::default()
                            .with_width(width * self.scale)
                            .with_color(color)
                            .with_line_join(canvas::LineJoin::Round),
                    );
                }
            }
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

        // The hovered item's numbers live in the LEGEND row (`hover_line`)
        // below the graph — nothing draws over the curve.

        // v27: the time cursor. The probed bucket is an INSTANT, echoed to
        // every graph sharing the ctl, so hovering one side marks the same
        // moment on the other — the comparison's whole point is reading the
        // two curves at one instant. The dot sits on THIS graph's own curve
        // there; the legend words both sides' values.
        if let Some(b) = self.ctl.probe
            && b >= self.view.0
            && b < self.view.1
        {
            let x = self.x_of(b as f64 + 0.5, w).clamp(0.0, w);
            frame.stroke(
                &Path::line(Point::new(x, ICON_BAND), Point::new(x, h)),
                Stroke::default()
                    .with_width(1.0)
                    .with_color(Color::from_rgba(1.0, 1.0, 1.0, 0.45)),
            );
            if let Some(v) = self.points.get(b) {
                frame.fill(
                    &Path::circle(Point::new(x, y_of(*v)), 2.5),
                    lighten(self.color, 0.4),
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::view::{GREEN, YELLOW};

    // ---- the rest: geometry, gestures, drawing, and the rendered screens ----

    use crate::window::testkit::{self as tk, apply, render, renderer, simulator};
    use iced::mouse::{Button, Cursor, Event as Mouse, Interaction};
    use iced::widget::canvas::Program;
    use wowdps_model::Action;

    /// What the graph's gestures become in these tests.
    #[derive(Debug, Clone, PartialEq)]
    enum Ev {
        Range(Option<(u32, u32)>),
        Hover(Option<String>),
        Probe(Option<usize>),
        Spell((String, String)),
        SpellHover(Option<String>),
    }

    fn ctl(hover: Option<&str>, probe: Option<usize>) -> GraphCtl<Ev> {
        ctl_over_spell(hover, probe, None)
    }

    /// The same gestures, with a spell-table row reported hovered.
    fn ctl_over_spell(
        hover: Option<&str>,
        probe: Option<usize>,
        spell: Option<&str>,
    ) -> GraphCtl<Ev> {
        GraphCtl {
            on_range: Rc::new(Ev::Range),
            on_hover: Rc::new(Ev::Hover),
            hover: hover.map(str::to_string),
            on_probe: Rc::new(Ev::Probe),
            probe,
            on_spell: Rc::new(Ev::Spell),
            on_spell_hover: Rc::new(Ev::SpellHover),
            spell_hover: spell.map(str::to_string),
        }
    }

    use wowdps_gui_logic::graph::samples::{ITEM_KINDS, mark, marked, role_marked, timeline};

    fn graph_of(t: &Timeline, view: (usize, usize), hover: Option<&str>) -> Graph<Ev> {
        Graph {
            geo: Plot::new(
                t,
                GraphMode::Dps,
                peak_of(&[t], GraphMode::Dps, view),
                view,
                Vec::new(),
            ),
            color: YELLOW,
            scale: 1.0,
            ghost: None,
            ctl: ctl(hover, None),
            plot: Look::OVERLAY.plot,
            lane: Look::OVERLAY.good,
        }
    }

    const W: f32 = 200.0;
    const H: f32 = 100.0;

    fn bounds() -> Rectangle {
        Rectangle::new(Point::new(10.0, 20.0), Size::new(W, H))
    }

    /// A cursor at canvas-local (x, y).
    fn at(x: f32, y: f32) -> Cursor {
        Cursor::Available(Point::new(10.0 + x, 20.0 + y))
    }

    fn message(action: Option<canvas::Action<Ev>>) -> Option<Ev> {
        action.and_then(|a| a.into_inner().0)
    }

    /// The marker palette is gui-logic's (`graph::mark_color`), read in
    /// iced's type; the probe's lift and the class bars are iced's own.
    #[test]
    fn marks_wear_gui_logics_colours_and_bars_their_class() {
        let coral = graph::mark_color(MarkKind::Defensive);
        assert_eq!(
            mark_color(MarkKind::Defensive),
            Color::from_rgba(coral.r, coral.g, coral.b, coral.a)
        );
        assert_eq!(mark_color(MarkKind::Consumable), crate::view::GREEN);
        let lit = lighten(Color::from_rgb(0.0, 0.5, 1.0), 0.5);
        assert!((lit.r - 0.5).abs() < 1e-6 && (lit.g - 0.75).abs() < 1e-6);
        assert!((lit.b - 1.0).abs() < 1e-6);
        assert_eq!(class_color(None), CLASSLESS);
        let (r, g, b) = Class::Warlock.rgb();
        assert_eq!(class_color(Some(Class::Warlock)), Color::from_rgb8(r, g, b));
    }
    #[test]
    fn every_class_has_a_two_letter_tag() {
        let classes = [
            Class::Warrior,
            Class::Paladin,
            Class::Hunter,
            Class::Rogue,
            Class::Priest,
            Class::DeathKnight,
            Class::Shaman,
            Class::Mage,
            Class::Warlock,
            Class::Monk,
            Class::Druid,
            Class::DemonHunter,
            Class::Evoker,
        ];
        let mut tags: Vec<&str> = classes.iter().map(|c| class_tag(Some(*c))).collect();
        assert!(tags.iter().all(|t| t.len() == 2));
        tags.sort_unstable();
        tags.dedup();
        assert_eq!(tags.len(), classes.len(), "no two classes share a tag");
        assert_eq!(class_tag(None), "?");
    }

    /// The split layout is the two-reading one, and only that: a single
    /// graph's legend keeps its one line.
    #[test]
    fn two_readings_split_the_legend_at_the_divider() {
        let pair = Some((
            "0:43".to_string(),
            vec![
                "Mehna 117.0k".to_string(),
                "Dawgoneefour 115.3k".to_string(),
            ],
        ));
        let mut ui = simulator(legend::<()>(
            &Look::OVERLAY,
            GraphMode::Dps,
            None,
            1.0,
            pair,
            "dtps",
            None,
            ITEM_KINDS,
            true,
        ));
        // The instant carries the metric word once, and each side's reading
        // stands alone — under its own graph.
        assert!(ui.find("0:43 · dtps").is_ok());
        assert!(ui.find("Mehna 117.0k").is_ok());
        assert!(ui.find("Dawgoneefour 115.3k").is_ok());
        let _ = ui.snapshot(&Theme::TokyoNight).unwrap();

        // One reading: one line, the word on the number as before.
        let mut ui = simulator(legend::<()>(
            &Look::OVERLAY,
            GraphMode::Dps,
            Some((2_500, 7_500)),
            1.0,
            Some(("0:43".to_string(), vec!["dps: 117.0k".to_string()])),
            "dps",
            None,
            &[],
            true,
        ));
        assert!(ui.find("0:43").is_ok());
        assert!(ui.find("dps: 117.0k").is_ok());
        assert!(ui.find("0:02–0:07 · right-click resets").is_ok());
        assert!(ui.find("0:43 · dps").is_err(), "not the split wording");
    }

    /// R18: the legend keys only the kinds it is handed — the kinds
    /// `graph::kinds_shown` finds on screen.
    #[test]
    fn the_legend_keys_only_the_kinds_shown() {
        let role = role_marked();
        let mut ui = simulator(legend::<()>(
            &Look::OVERLAY,
            GraphMode::Dps,
            None,
            1.0,
            None,
            "dps",
            None,
            ITEM_KINDS,
            true,
        ));
        for k in ["trinket use", "proc", "consumable", "external"] {
            assert!(ui.find(k).is_ok(), "{k} key");
        }
        for k in ["mitigation", "defensive", "support", "cooldown"] {
            assert!(ui.find(k).is_err(), "{k} key drawn without a mark");
        }
        let kinds = kinds_shown(&[&role], (0, 10));
        let mut ui = simulator(legend::<()>(
            &Look::OVERLAY,
            GraphMode::Dps,
            None,
            1.0,
            None,
            "dtps",
            None,
            &kinds,
            true,
        ));
        for k in [
            "proc",
            "external",
            "mitigation",
            "defensive",
            "support",
            "cooldown",
        ] {
            assert!(ui.find(k).is_ok(), "{k} key");
        }
        assert!(ui.find("trinket use").is_err());
        assert!(ui.find("consumable").is_err());
        let _ = ui.snapshot(&Theme::TokyoNight).unwrap();
    }

    /// R18: a role span washes the graph like an external's, in its own
    /// colour — the draw path is kind-agnostic, so this just has to render.
    #[test]
    fn role_spans_draw_with_their_hues() {
        let t = role_marked();
        let r = renderer();
        let g = graph_of(&t, (0, 10), Some("Shield Block"));
        let geo = g.draw(
            &GraphState::default(),
            &r,
            &Theme::TokyoNight,
            bounds(),
            at(50.0, 50.0),
        );
        assert_eq!(geo.len(), 1);
        for k in [
            MarkKind::ActiveMitigation,
            MarkKind::Defensive,
            MarkKind::SupportBuff,
            MarkKind::Cooldown,
        ] {
            assert!(t.marks.iter().any(|m| m.kind == k && m.dur_ms > 0));
        }
    }

    #[test]
    fn a_drag_selects_a_window_and_a_click_does_not() {
        let t = marked();
        let g = graph_of(&t, (0, 10), None);
        let mut state = GraphState::default();

        // A press outside the canvas is not ours.
        let press = iced::Event::Mouse(Mouse::ButtonPressed(Button::Left));
        assert!(
            g.update(&mut state, &press, bounds(), Cursor::Unavailable)
                .is_none()
        );
        // Non-mouse events pass through.
        let win = iced::Event::Window(iced::window::Event::Focused);
        assert!(
            g.update(&mut state, &win, bounds(), at(50.0, 50.0))
                .is_none()
        );

        // Press at 25%, drag to 75%, release: the window is 2.5s–7.5s.
        let a = g.update(&mut state, &press, bounds(), at(50.0, 50.0));
        assert!(a.is_some());
        assert_eq!(state.drag, Some((50.0, 50.0)));
        assert_eq!(
            g.mouse_interaction(&state, bounds(), at(50.0, 50.0)),
            Interaction::ResizingHorizontally
        );
        let moved = iced::Event::Mouse(Mouse::CursorMoved {
            position: Point::ORIGIN,
        });
        let a = g.update(&mut state, &moved, bounds(), at(150.0, 50.0));
        assert!(message(a).is_none(), "moves only redraw");
        assert_eq!(state.drag, Some((50.0, 150.0)));
        // Off-canvas motion clamps to the edge.
        let _ = g.update(&mut state, &moved, bounds(), at(W + 500.0, 50.0));
        assert_eq!(state.drag, Some((50.0, W)));
        let _ = g.update(&mut state, &moved, bounds(), at(150.0, 50.0));
        let release = iced::Event::Mouse(Mouse::ButtonReleased(Button::Left));
        let a = g.update(&mut state, &release, bounds(), at(150.0, 50.0));
        assert_eq!(message(a), Some(Ev::Range(Some((2_500, 7_500)))));
        assert_eq!(state.drag, None);

        // A wander under the threshold is a click: nothing published.
        let _ = g.update(&mut state, &press, bounds(), at(50.0, 50.0));
        let _ = g.update(&mut state, &moved, bounds(), at(51.0, 50.0));
        let a = g.update(&mut state, &release, bounds(), at(51.0, 50.0));
        assert!(a.is_some());
        assert_eq!(message(a), None);
        // A release with no drag in flight is not ours.
        assert!(
            g.update(&mut state, &release, bounds(), at(51.0, 50.0))
                .is_none()
        );

        // A backwards drag still yields lo < hi.
        let _ = g.update(&mut state, &press, bounds(), at(150.0, 50.0));
        let _ = g.update(&mut state, &moved, bounds(), at(50.0, 50.0));
        let a = g.update(&mut state, &release, bounds(), at(50.0, 50.0));
        assert_eq!(message(a), Some(Ev::Range(Some((2_500, 7_500)))));

        // Right-click zooms out, and is captured even when unzoomed.
        let right = iced::Event::Mouse(Mouse::ButtonPressed(Button::Right));
        let a = g.update(&mut state, &right, bounds(), at(50.0, 50.0));
        assert_eq!(message(a), Some(Ev::Range(None)));
        assert!(
            g.update(&mut state, &right, bounds(), Cursor::Unavailable)
                .is_none()
        );
        // Other buttons are ignored.
        let middle = iced::Event::Mouse(Mouse::ButtonPressed(Button::Middle));
        assert!(
            g.update(&mut state, &middle, bounds(), at(50.0, 50.0))
                .is_none()
        );
    }

    #[test]
    fn hovering_the_band_reports_the_item_and_the_curve_probes_elsewhere() {
        let t = marked();
        let g = graph_of(&t, (0, 10), None);
        let mut state = GraphState::default();
        let moved = iced::Event::Mouse(Mouse::CursorMoved {
            position: Point::ORIGIN,
        });
        let proc_x = g.mark_x(&t.marks[1], W);

        let a = g.update(&mut state, &moved, bounds(), at(proc_x, 4.0));
        assert_eq!(message(a), Some(Ev::Hover(Some("Proc".to_string()))));
        assert_eq!(
            g.mouse_interaction(&state, bounds(), at(proc_x, 4.0)),
            Interaction::Pointer
        );
        // Still over it: the hover is settled, so the probe gets its turn.
        let a = g.update(&mut state, &moved, bounds(), at(proc_x, 4.0));
        assert!(matches!(message(a), Some(Ev::Probe(Some(_)))));
        assert_eq!(state.probe, Some(7));
        // Same bucket again: nothing new to say.
        assert!(
            g.update(&mut state, &moved, bounds(), at(proc_x + 1.0, 4.0))
                .is_none()
        );

        // Down onto the curve: the hover clears first...
        let a = g.update(&mut state, &moved, bounds(), at(proc_x, 60.0));
        assert_eq!(message(a), Some(Ev::Hover(None)));
        assert_eq!(
            g.mouse_interaction(&state, bounds(), at(proc_x, 60.0)),
            Interaction::Crosshair
        );
        // ...and a new bucket probes.
        let a = g.update(&mut state, &moved, bounds(), at(0.0, 60.0));
        assert_eq!(message(a), Some(Ev::Probe(Some(0))));
        // Leaving the canvas clears the probe.
        let a = g.update(&mut state, &moved, bounds(), Cursor::Unavailable);
        assert_eq!(message(a), Some(Ev::Probe(None)));
        assert_eq!(
            g.mouse_interaction(&state, bounds(), Cursor::Unavailable),
            Interaction::default()
        );
    }

    #[test]
    fn the_graph_draws_in_every_state() {
        let r = renderer();
        let theme = Theme::TokyoNight;
        let t = marked();
        let plain = graph_of(&t, (0, 10), None);
        let idle = GraphState::default();
        assert_eq!(
            plain
                .draw(&idle, &r, &theme, bounds(), Cursor::Unavailable)
                .len(),
            1
        );

        // Hover lights one item and dims the rest; the cursor on the curve
        // adds the probe glow.
        let hovered = graph_of(&t, (0, 10), Some("Trinket"));
        assert_eq!(
            hovered
                .draw(&idle, &r, &theme, bounds(), at(100.0, 60.0))
                .len(),
            1
        );
        let other = graph_of(&t, (0, 10), Some("Proc"));
        assert_eq!(
            other.draw(&idle, &r, &theme, bounds(), at(2.0, 60.0)).len(),
            1
        );

        // A drag in flight paints the selection; a sub-threshold one does not.
        let dragging = GraphState {
            drag: Some((40.0, 120.0)),
            ..GraphState::default()
        };
        assert_eq!(
            plain
                .draw(&dragging, &r, &theme, bounds(), at(120.0, 50.0))
                .len(),
            1
        );
        let clicking = GraphState {
            drag: Some((40.0, 41.0)),
            ..GraphState::default()
        };
        assert_eq!(
            plain
                .draw(&clicking, &r, &theme, bounds(), Cursor::Unavailable)
                .len(),
            1
        );

        // Zoomed: off-window marks skip, the curve slice starts mid-way.
        let zoomed = graph_of(&t, (4, 8), Some("Proc"));
        assert_eq!(
            zoomed.draw(&idle, &r, &theme, bounds(), at(W, 90.0)).len(),
            1
        );

        // Encounter lane + ghost curve (the Σ / ability drill shapes) in
        // cumulative mode, with a tall scale.
        let ghost = Graph {
            geo: Plot::new(
                &t,
                GraphMode::Total,
                peak_of(&[&t], GraphMode::Total, (0, 10)),
                (0, 10),
                vec![(1_000, 3_000), (5_000, 5_000), (8_000, 12_000)],
            ),
            color: GREEN,
            scale: 2.0,
            ghost: Some((curve(&t, GraphMode::Total), YELLOW)),
            ctl: ctl(Some("Bloodlust"), Some(3)),
            plot: Look::WINDOW.plot,
            lane: Look::WINDOW.good,
        };
        assert_eq!(
            ghost
                .draw(&idle, &r, &theme, bounds(), at(150.0, 30.0))
                .len(),
            1
        );

        // Nothing at all: the baseline still draws, the peak guard holds.
        let empty = graph_of(&timeline(Vec::new()), (0, 1), None);
        assert_eq!(empty.peak, 0.0);
        assert_eq!(
            empty.draw(&idle, &r, &theme, bounds(), at(5.0, 5.0)).len(),
            1
        );
        // A single point cannot make a line; a mark past the curve falls
        // to the floor.
        let mut one = timeline(vec![10]);
        one.marks.push(mark(5_000, MarkKind::Consumable, "Late", 0));
        let single = graph_of(&one, (0, 6), None);
        assert_eq!(
            single
                .draw(&idle, &r, &theme, bounds(), at(5.0, 50.0))
                .len(),
            1
        );

        // A flat-zero fight: the peak guard pins the line to the floor. A
        // buff starting exactly at the window's edge has no span to wash,
        // and an empty ghost has nothing to trace.
        let mut flat = timeline(vec![0, 0, 0, 0]);
        flat.marks
            .push(mark(4_000, MarkKind::External, "Edge", 9_000));
        let zero = Graph {
            ghost: Some((Vec::new(), GREEN)),
            ..graph_of(&flat, (0, 4), None)
        };
        assert_eq!(zero.peak, 0.0);
        assert_eq!(
            zero.draw(&idle, &r, &theme, bounds(), at(50.0, 50.0)).len(),
            1
        );
    }

    #[test]
    fn class_icons_and_rings_draw_without_the_art_cache() {
        let r = renderer();
        let theme = Theme::TokyoNight;
        let b = Rectangle::new(Point::ORIGIN, Size::new(18.0, 18.0));
        let picked = ClassIcon {
            color: YELLOW,
            tag: "WL",
            slot: Some(1),
        };
        let draw_icon = |icon: &ClassIcon| {
            <ClassIcon as Program<()>>::draw(icon, &(), &r, &theme, b, Cursor::Unavailable).len()
        };
        assert_eq!(draw_icon(&picked), 1);
        let idle = ClassIcon {
            color: YELLOW,
            tag: "?",
            slot: None,
        };
        assert_eq!(draw_icon(&idle), 1);
        assert_eq!(
            <Ring as Program<()>>::draw(&Ring, &(), &r, &theme, b, Cursor::Unavailable).len(),
            1
        );
        // Through the element path, picked and unpicked, with and without a
        // spec: no cache means the drawn disc every time.
        let _ = render(class_icon::<()>(
            Some(Class::Warlock),
            Some(Spec::Destruction),
            Some(0),
            18.0,
        ));
        let _ = render(class_icon::<()>(Some(Class::Mage), None, None, 24.0));
        let _ = render(class_icon::<()>(None, None, None, 18.0));
    }

    #[test]
    fn the_legend_words_the_mode_the_probe_and_the_window() {
        let mut ui = simulator(legend::<()>(
            &Look::OVERLAY,
            GraphMode::Dps,
            None,
            1.0,
            None,
            "dps",
            None,
            ITEM_KINDS,
            true,
        ));
        assert!(ui.find("graph: dps").is_ok());
        for k in ["trinket use", "proc", "consumable", "external"] {
            assert!(ui.find(k).is_ok(), "{k} key");
        }
        let _ = ui.snapshot(&Theme::TokyoNight).unwrap();
        assert!(
            simulator(legend::<()>(
                &Look::OVERLAY,
                GraphMode::Dps,
                None,
                1.0,
                None,
                "hps",
                None,
                ITEM_KINDS,
                true
            ))
            .find("graph: hps")
            .is_ok()
        );
        // The overlay passes idle_mode = false: no label while idle.
        let mut ui = simulator(legend::<()>(
            &Look::OVERLAY,
            GraphMode::Total,
            None,
            1.0,
            None,
            "dps",
            None,
            &[],
            false,
        ));
        assert!(ui.find("graph: total").is_err());
        assert!(ui.find("graph: dps").is_err());
        // A probe reads the curve; the total mode words itself.
        let mut ui = simulator(legend::<()>(
            &Look::OVERLAY,
            GraphMode::Total,
            Some((2_500, 7_500)),
            1.0,
            Some(("0:02".to_string(), vec!["total: 674.5k".to_string()])),
            "dps",
            None,
            &[],
            false,
        ));
        assert!(ui.find("total: 674.5k").is_ok());
        assert!(ui.find("0:02").is_ok(), "the instant leads the numbers");
        assert!(ui.find("0:02–0:07 · right-click resets").is_ok());
        // A hovered item takes the row over.
        let hover = Some((
            MarkKind::TrinketProc,
            "Proc".to_string(),
            "proc ×1".to_string(),
        ));
        let mut ui = simulator(legend::<()>(
            &Look::OVERLAY,
            GraphMode::Dps,
            None,
            1.0,
            Some(("0:00".to_string(), vec!["dps: 1".to_string()])),
            "dps",
            hover,
            ITEM_KINDS,
            true,
        ));
        assert!(ui.find("Proc").is_ok());
        assert!(ui.find("proc ×1").is_ok());
        assert!(ui.find("graph: dps").is_err());
        assert!(ui.find("dps: 1").is_err());
        let _ = ui.snapshot(&Theme::TokyoNight).unwrap();
    }

    #[test]
    fn spell_tables_and_rows_show_hits_crit_and_average() {
        let mut r = Row {
            key: "Chaos Bolt".to_string(),
            label: "Chaos Bolt".to_string(),
            amount: 90_000,
            count: 12,
            crits: 3,
            ..Row::default()
        };
        let mut ui = simulator(spell_row::<Ev>(&Look::OVERLAY, &r, false, 1.0, false));
        assert!(ui.find("Chaos Bolt").is_ok());
        assert!(ui.find("12").is_ok());
        assert!(ui.find("25%").is_ok());
        assert!(ui.find("7.5k").is_ok());
        r.count = 0;
        r.crits = 0;
        let mut ui = simulator(spell_row::<Ev>(&Look::OVERLAY, &r, false, 1.0, true));
        assert!(ui.find("—").is_ok(), "no hits: no crit rate, no average");

        let c = ctl(None, None);
        let mut ui = simulator(spell_table::<Ev>(
            &Look::OVERLAY,
            &[],
            View::Damage,
            1.0,
            &c,
        ));
        assert!(ui.find("no damage recorded").is_ok());
        assert!(ui.find("spell").is_ok());
        r.count = 12;
        let rows = vec![
            r.clone(),
            Row {
                key: "Melee".to_string(),
                label: "Melee".to_string(),
                amount: 1,
                count: 1,
                ..Row::default()
            },
        ];
        let mut ui = simulator(spell_table::<Ev>(
            &Look::OVERLAY,
            &rows,
            View::Damage,
            1.0,
            &c,
        ));
        assert!(ui.find("Melee").is_ok());
        assert!(ui.find("hits").is_ok());
        assert!(ui.find("crit").is_ok());
        assert!(ui.find("avg").is_ok());
        // Clicking a row asks to drill both sides into it — and the cursor
        // moving onto it first reports the hover, which is what lights the
        // same ability on the OTHER side's table.
        let _ = ui.click("Melee").unwrap();
        let msgs: Vec<Ev> = ui.into_messages().collect();
        assert_eq!(
            msgs,
            vec![
                Ev::SpellHover(Some("Melee".to_string())),
                Ev::Spell(("Melee".to_string(), "Melee".to_string())),
            ]
        );

        // The echo lights the row by KEY, wherever it sits: the two tables
        // are sorted independently, so an index would light the wrong
        // ability on the other side. Both states render, and the mark
        // itself is `view::hover_style`'s.
        let hovering = ctl_over_spell(None, None, Some("Melee"));
        let _ = render(spell_table::<Ev>(
            &Look::OVERLAY,
            &rows,
            View::Damage,
            1.0,
            &hovering,
        ));
        // A key neither table carries lights nothing at all.
        let stranger = ctl_over_spell(None, None, Some("Not Cast"));
        let _ = render(spell_table::<Ev>(
            &Look::OVERLAY,
            &rows,
            View::Damage,
            1.0,
            &stranger,
        ));
        assert!(crate::view::hover_style(true).background.is_some());
        assert!(crate::view::hover_style(false).background.is_none());

        // The window's table: sentence-case heads over the same rows, the
        // same clicks.
        let mut ui = simulator(spell_table::<Ev>(
            &Look::WINDOW,
            &rows,
            View::Damage,
            1.2,
            &c,
        ));
        for head in ["Spell", "Hits", "Crit", "Avg"] {
            assert!(ui.find(head).is_ok(), "{head}");
        }
        let _ = ui.click("Melee").unwrap();
        assert!(
            ui.into_messages()
                .any(|m| m == Ev::Spell(("Melee".to_string(), "Melee".to_string())))
        );
    }

    /// A window table gives up crit, then the average, before its names
    /// fall under what a name keeps — and never its last column.
    #[test]
    fn a_window_table_fits_its_columns_to_its_pane() {
        let all = [Cell::Hits, Cell::Crit, Cell::Avg];
        assert_eq!(fit_cells(&all, 600.0, 1.2), all.to_vec());
        // A 460 px window's half pane.
        assert_eq!(fit_cells(&all, 215.0, 1.2), vec![Cell::Hits, Cell::Avg]);
        assert_eq!(fit_cells(&all, 120.0, 1.2), vec![Cell::Hits]);
        assert_eq!(fit_cells(&[Cell::Count], 10.0, 1.2), vec![Cell::Count]);
        // What the name keeps in that half pane.
        let kept = [Cell::Hits, Cell::Avg];
        let fixed = 2.0 * 6.0 * 1.2
            + CMP_LANE
            + 13.0 * 1.2
            + 4.0 * 3.0
            + kept.iter().map(|c| c.width() * 1.2).sum::<f32>();
        assert!(215.0 - fixed >= CMP_NAME_MIN, "{}", 215.0 - fixed);
        // The heads name the cells; a count view has one.
        assert_eq!(Cell::Crit.head(), "Crit");
        let r = Row {
            amount: 900,
            count: 3,
            crits: 1,
            ..Row::default()
        };
        assert_eq!(
            [Cell::Hits, Cell::Crit, Cell::Avg, Cell::Count].map(|c| c.text(&r)),
            ["3", "33%", "300", "3"].map(str::to_string)
        );
        let none = Row::default();
        assert_eq!(Cell::Crit.text(&none), "—");
        assert_eq!(Cell::Avg.text(&none), "—");
    }

    #[test]
    fn waiting_words_each_stage_of_the_pick() {
        let (mut state, mut mock) = tk::kill();
        assert!(
            simulator(waiting::<()>(&Look::OVERLAY, &state, 1.0))
                .find("pick two players to compare")
                .is_ok()
        );
        apply(&mut state, &mut mock, Action::PickCompare);
        let top = state.rows()[0].label.clone();
        let want = format!("comparing {} — pick one more", short_name(&top));
        assert!(
            simulator(compare_body(&state, 1.0, 100.0, true, ctl(None, None)))
                .find(want.as_str())
                .is_ok()
        );
        // The second pick, but the answer not yet in hand: navigating a
        // compared pair drops the stale sides until the daemon re-answers.
        apply(&mut state, &mut mock, Action::Down);
        apply(&mut state, &mut mock, Action::PickCompare);
        assert!(state.compare_sides().is_some());
        let _ = state.apply(Action::NewerSegment);
        assert!(state.compare_sides().is_none());
        assert!(
            simulator(compare_body(&state, 1.0, 100.0, true, ctl(None, None)))
                .find("loading comparison…")
                .is_ok()
        );
    }

    #[test]
    fn the_comparison_body_renders_both_sides_over_one_scale() {
        let (mut state, mut mock) = tk::compared();
        let (a, b) = state.compare_sides().unwrap();
        let (a_label, b_label) = (a.total.label.clone(), b.total.label.clone());
        let a_total = human(a.total.amount);
        let first_spell = a.spells.first().map(|r| r.label.clone()).unwrap();
        let first_key = a.spells.first().map(|r| r.key.clone()).unwrap();
        let some_mark = a.timeline.marks.first().map(|m| m.label.clone());
        let hover = some_mark.as_deref();
        let mut ui = simulator(compare_body(&state, 1.0, 120.0, true, ctl(hover, Some(2))));
        assert!(ui.find(short_name(&a_label).as_str()).is_ok());
        assert!(ui.find(short_name(&b_label).as_str()).is_ok());
        assert!(ui.find(a_total.as_str()).is_ok());
        assert!(ui.find(first_spell.as_str()).is_ok());
        match hover {
            Some(l) => assert!(
                ui.find(l).is_ok(),
                "the hovered item names itself in the legend"
            ),
            // v27: the probe is an INSTANT, and the readout names what each
            // side's curve says at it — computed here from the timelines,
            // not from the renderer's own helper.
            None => {
                let at = |t: &Timeline| human(t.rolling_dps(15_000)[2] as u64);
                // One reading per graph, each under its own: two separate
                // texts, not one line describing both.
                for (label, t) in [(&a_label, &a.timeline), (&b_label, &b.timeline)] {
                    let want = format!("{} {}", short_name(label), at(t));
                    assert!(ui.find(want.as_str()).is_ok(), "{want}");
                }
                assert!(ui.find("0:02 · dps").is_ok(), "the instant, said once");
            }
        }
        let _ = ui.snapshot(&Theme::TokyoNight).unwrap();

        // Cumulative mode, zoomed, overlay flavour (no idle label).
        state.toggle_graph();
        let reqs = state.set_compare_range(Some((0, 20_000)));
        wowdps_daemon::mock::pump(&mut state, &mut mock, reqs);
        assert_eq!(state.compare_shown_range(), Some((0, 20_000)));
        let mut ui = simulator(compare_body(&state, 1.5, 90.0, false, ctl(None, None)));
        assert!(ui.find("0:00–0:20 · right-click resets").is_ok());
        assert!(ui.find("graph: total").is_err());
        let _ = ui.snapshot(&Theme::TokyoNight).unwrap();

        // The ability drill: both sides lock to one spell; a side that
        // never cast it says so.
        let reqs = state.drill_compare_spell(&first_key, &first_spell);
        wowdps_daemon::mock::pump(&mut state, &mut mock, reqs);
        assert!(state.compare_spell().is_some());
        let mut ui = simulator(compare_body(&state, 1.0, 120.0, true, ctl(None, None)));
        assert!(ui.find(first_spell.as_str()).is_ok());
        for card in ["share", "hits", "avg"] {
            assert!(ui.find(card).is_ok(), "{card} card");
        }
        let _ = ui.snapshot(&Theme::TokyoNight).unwrap();
        let reqs = state.drill_compare_spell("no such spell", "Nothing");
        wowdps_daemon::mock::pump(&mut state, &mut mock, reqs);
        let mut ui = simulator(compare_body(&state, 1.0, 120.0, true, ctl(None, None)));
        assert!(ui.find("did not cast Nothing").is_ok());
        let _ = ui.snapshot(&Theme::TokyoNight).unwrap();
    }

    #[test]
    fn the_drill_graph_renders_plain_and_focused() {
        let (mut state, mut mock) = tk::drilled();
        let t = state.drill_timeline().cloned().unwrap();
        let class = state.rows()[0].class;
        let mut ui = simulator(drill_graph(
            &state,
            &t,
            class,
            1.0,
            110.0,
            "dps",
            true,
            None,
            ctl(None, None),
        ));
        assert!(ui.find("graph: dps").is_ok());
        let _ = ui.snapshot(&Theme::TokyoNight).unwrap();

        state.set_drill_range(Some((5_000, 15_000)));
        state.toggle_graph();
        let mut ui = simulator(drill_graph(
            &state,
            &t,
            None,
            2.0,
            64.0,
            "hps",
            false,
            None,
            ctl(None, Some(9)),
        ));
        // One side: the readout stays the bare "total: N", with the instant
        // beside it — the cumulative curve read at bucket 9.
        let want = format!("total: {}", human(t.cumulative()[9]));
        assert!(ui.find(want.as_str()).is_ok(), "{want}");
        assert!(ui.find("0:09").is_ok(), "the probed instant");
        assert!(ui.find("0:05–0:15 · right-click resets").is_ok());
        let _ = ui.snapshot(&Theme::TokyoNight).unwrap();

        apply(&mut state, &mut mock, Action::Open);
        let ft = state
            .spell_timeline()
            .cloned()
            .expect("the ability's curve");
        let t = state.drill_timeline().cloned().unwrap();
        let mark = t.marks.first().map(|m| m.label.clone());
        let _ = render(drill_graph(
            &state,
            &t,
            class,
            1.0,
            110.0,
            "dps",
            true,
            Some((&ft, YELLOW)),
            ctl(mark.as_deref(), None),
        ));
    }
}
