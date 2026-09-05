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
/// R18: active mitigation AND defensives — coral. Both are "the player
/// pressed something to take less"; the legend key and the hover name tell
/// them apart, a second warm hue would not at bar width.
const MITIGATION: Color = Color::from_rgb(1.0, 0.45, 0.40);
/// R18: support buffs on the player (Ebon Might, Prescience) — teal.
const SUPPORT: Color = Color::from_rgb(0.35, 0.90, 0.80);
/// R18: an offensive cooldown's window — the External bar's twin (a burst
/// window either way), so it stays in the violet family but steps to
/// indigo: a hue away from EXTERNAL's lavender, not a shade of it.
const COOLDOWN: Color = Color::from_rgb(0.50, 0.40, 1.0);

pub(crate) fn mark_color(kind: MarkKind) -> Color {
    match kind {
        MarkKind::TrinketUse => USE,
        MarkKind::TrinketProc => PROC,
        MarkKind::Consumable => CONSUMABLE,
        MarkKind::External => EXTERNAL,
        MarkKind::ActiveMitigation | MarkKind::Defensive => MITIGATION,
        MarkKind::SupportBuff => SUPPORT,
        MarkKind::Cooldown => COOLDOWN,
    }
}

/// R18: every kind, in wire-code order — the legend's key order.
const ALL_KINDS: [MarkKind; 8] = [
    MarkKind::TrinketUse,
    MarkKind::TrinketProc,
    MarkKind::Consumable,
    MarkKind::External,
    MarkKind::ActiveMitigation,
    MarkKind::Defensive,
    MarkKind::SupportBuff,
    MarkKind::Cooldown,
];

/// R18: the kinds with a mark inside the displayed window, in `ALL_KINDS`
/// order. The legend keys only these — a DPS's graph never explains a
/// mitigation key, and a tank's never a proc it has no marks for.
fn kinds_shown(timelines: &[&Timeline], view: (usize, usize)) -> Vec<MarkKind> {
    let visible = |t: &Timeline, m: &Mark| {
        let b = m.at_ms as f64 / t.bucket_ms.max(1) as f64;
        b >= view.0 as f64 && b <= view.1 as f64
    };
    ALL_KINDS
        .into_iter()
        .filter(|k| {
            timelines
                .iter()
                .any(|t| t.marks.iter().any(|m| m.kind == *k && visible(t, m)))
        })
        .collect()
}

/// R18: a mark's caster worded for the hover — the segment's name for the
/// guid when the renderer has one (`names` pairs guid → name), else the
/// guid's tail, which is at least stable across the two graphs.
fn caster_name<'a>(src: &'a str, names: &[(&str, &'a str)]) -> &'a str {
    names
        .iter()
        .find(|(g, _)| *g == src)
        .map(|(_, n)| *n)
        .unwrap_or_else(|| src.rsplit('-').next().unwrap_or(src))
}

fn mark_name(kind: MarkKind) -> &'static str {
    match kind {
        MarkKind::TrinketUse => "trinket use",
        MarkKind::TrinketProc => "proc",
        MarkKind::Consumable => "consumable",
        MarkKind::External => "external",
        MarkKind::ActiveMitigation => "mitigation",
        MarkKind::Defensive => "defensive",
        MarkKind::SupportBuff => "support",
        MarkKind::Cooldown => "cooldown",
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
pub(crate) type OnRange<M> = Rc<dyn Fn(Option<(u32, u32)>) -> M>;

pub(crate) struct GraphCtl<M> {
    pub on_range: OnRange<M>,
    pub on_hover: Rc<dyn Fn(Option<String>) -> M>,
    pub hover: Option<String>,
    /// The curve value under the cursor (dps or total, per the mode) — the
    /// canvas publishes it as the pointer moves, the frontend echoes it back
    /// in `probe`, and the legend words it where "graph: dps" sat.
    pub on_probe: Rc<dyn Fn(Option<f64>) -> M>,
    pub probe: Option<f64>,
    /// v18: a spell-table row was clicked — drill BOTH sides into that
    /// ability, as (by-spell key, label).
    pub on_spell: Rc<dyn Fn((String, String)) -> M>,
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
        }
    }
}

/// The whole comparison body: two columns, each a header, a spell table and a
/// graph. `scale` multiplies text sizes the way `view::bar_row` does, so the
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
    // comparison lies (see module docs). v18: while an ability is drilled,
    // the scale spans the ghosts AND both focus curves the same way.
    let mut scaled: Vec<&Timeline> = vec![&a.timeline, &b.timeline];
    for side in [a, b] {
        if let Some(ft) = &side.spell_timeline {
            scaled.push(ft);
        }
    }
    let peak = peak_of(&scaled, mode, view);

    let probe = ctl.probe;
    // R18: casters resolve through the two sides and the meter rows in hand
    // (key = guid, label = name); an external from a third player names them.
    let rows = app.rows();
    let names: Vec<(&str, &str)> = [(a.guid.as_str(), a.total.label.as_str())]
        .into_iter()
        .chain([(b.guid.as_str(), b.total.label.as_str())])
        .chain(rows.iter().map(|r| (r.key.as_str(), r.label.as_str())))
        .collect();
    let timelines = [&a.timeline, &b.timeline];
    let hovered = ctl
        .hover
        .as_deref()
        .and_then(|l| hover_line(&timelines, l, view, &names));
    let kinds = kinds_shown(&timelines, view);
    // v18: the comparison's ability drill — both sides locked to one spell,
    // stats + focus curve each; back out with the usual Esc/right-click.
    let spell = app.compare_spell().cloned();
    let panes = row![
        side_column(
            a,
            mode,
            peak,
            view,
            scale,
            graph_height,
            spell.clone(),
            ctl.clone()
        ),
        side_column(b, mode, peak, view, scale, graph_height, spell, ctl),
    ]
    .spacing(10)
    .height(Length::Fill);

    column![
        panes,
        legend(mode, shown, scale, probe, "dps", hovered, &kinds, idle_mode)
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
    let mode = app.graph_mode();
    let shown = app.drill_range();
    // The view window always spans the PLAYER's timeline: the ability's
    // buckets share the same grid, and the x-axis must not reshape when
    // drilling in or out.
    let span = t.buckets.len().max(1);
    let view = view_window(shown, t.bucket_ms.max(1) as usize, span);
    let probe = ctl.probe;
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
        .and_then(|l| hover_line(&[t], l, view, &names));
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
        ),
    };
    column![
        body,
        legend(mode, shown, scale, probe, rate, hovered, &kinds, idle_mode)
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

#[allow(clippy::too_many_arguments)]
fn side_column<M: Clone + 'static>(
    side: &CompareSide,
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

    // v18: the ability variant — breadcrumb-lite (name + school tag come
    // from the shared helpers), stat cards, and the focused graph.
    if let Some((key, label)) = spell {
        let srow = side.spells.iter().find(|r| r.key == key).cloned();
        let middle: Element<'static, M> = match &srow {
            Some(r) => column![
                crate::view::spell_breadcrumb::<M>(&side.total.label, &label, Some(r), scale),
                crate::view::spell_stats::<M>(r, wowdps_model::View::Damage, scale),
            ]
            .spacing(8.0 * scale)
            .height(Length::Fill)
            .into(),
            None => container(
                text(format!("did not cast {label}"))
                    .size(12.0 * scale)
                    .color(DIM),
            )
            .center_x(Length::Fill)
            .height(Length::Fill)
            .into(),
        };
        let focus_color = srow
            .and_then(|r| crate::view::school_color(r.school))
            .unwrap_or(YELLOW);
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
            ),
        };
        return column![header, middle, g]
            .spacing(6)
            .width(Length::FillPortion(1))
            .height(Length::Fill)
            .into();
    }

    column![
        header,
        spell_table(&side.spells, scale, &ctl),
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
        ),
    ]
    .spacing(6)
    .width(Length::FillPortion(1))
    .height(Length::Fill)
    .into()
}

/// Column widths for the per-spell table: (hits, crit%, average).
const COLS: (f32, f32, f32) = (44.0, 46.0, 56.0);

fn spell_table<M: Clone + 'static>(
    spells: &[Row],
    scale: f32,
    ctl: &GraphCtl<M>,
) -> Element<'static, M> {
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
        // v18: a spell row drills BOTH sides into that ability.
        list = list.push(
            iced::widget::mouse_area(spell_row::<M>(r, scale))
                .on_press((ctl.on_spell)((r.key.clone(), r.label.clone()))),
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
        // Fill + NoWrap inside a clipping container: without the clip, iced
        // paints the one-line overflow under the number columns.
        container(
            text(r.label.clone())
                .size(11.0 * scale)
                .wrapping(iced::widget::text::Wrapping::None),
        )
        .clip(true)
        .width(Length::Fill),
        cell(r.count.to_string(), COLS.0, Color::WHITE),
        cell(crit, COLS.1, YELLOW),
        cell(avg, COLS.2, Color::WHITE),
    ]
    .spacing(4)
    .padding([0, 6])
    .align_y(iced::Alignment::Center)
    .into()
}

/// The hovered item summarized for the legend row — kind, name, and a
/// details clause (uses, uptime, share of the displayed window). Computed
/// over every displayed timeline, so a comparison counts both players' uses.
/// R18: a mark with a caster names them — "Power Infusion from Gennar" —
/// every distinct caster of that label, resolved through `names`.
fn hover_line(
    timelines: &[&Timeline],
    label: &str,
    view: (usize, usize),
    names: &[(&str, &str)],
) -> Option<(MarkKind, String, String)> {
    let same: Vec<&Mark> = timelines
        .iter()
        .flat_map(|t| t.marks.iter())
        .filter(|m| m.label == label)
        .collect();
    let first = same.first()?;
    let mut casters: Vec<&str> = Vec::new();
    for m in same.iter().filter(|m| !m.src.is_empty()) {
        let who = caster_name(&m.src, names);
        if !casters.contains(&who) {
            casters.push(who);
        }
    }
    let name = if casters.is_empty() {
        label.to_string()
    } else {
        format!("{label} from {}", casters.join(", "))
    };
    let mut details = format!("{} ×{}", mark_name(first.kind), same.len());
    let uptime_ms: i64 = same.iter().map(|m| m.dur_ms.max(0)).sum();
    if uptime_ms > 0 {
        let bucket_ms = timelines
            .iter()
            .map(|t| t.bucket_ms)
            .max()
            .unwrap_or(1)
            .max(1) as i64;
        let window_ms = (view.1 - view.0).max(1) as i64 * bucket_ms;
        let pct = (uptime_ms as f64 / window_ms as f64 * 100.0).min(100.0);
        details.push_str(&format!(" · uptime {}s · {pct:.0}%", uptime_ms / 1000));
    }
    Some((first.kind, name, details))
}

#[allow(clippy::too_many_arguments)]
fn legend<M: 'static>(
    mode: GraphMode,
    shown: Option<(u32, u32)>,
    scale: f32,
    probe: Option<f64>,
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
            text(mark_name(kind)).size(10.0 * scale).color(DIM),
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
                .color(Color::WHITE)
                .wrapping(iced::widget::text::Wrapping::None),
            text(details).size(10.0 * scale).color(DIM),
        ]
        .spacing(6)
        .align_y(iced::Alignment::Center)
        .into();
    }
    // Hovering the graph turns the mode label into a readout of the curve
    // under the cursor: "dps: 674.5k" instead of "graph: dps" — and the rate
    // word is the view's own ("hps" on a Healing drilldown, v14).
    let word = match mode {
        GraphMode::Dps => rate,
        GraphMode::Total => mode.label(),
    };
    let label = match probe {
        Some(v) => Some((format!("{word}: {}", human(v as u64)), YELLOW)),
        None if idle_mode => Some((format!("graph: {word}"), DIM)),
        None => None,
    };
    let mut line = row![].spacing(10).align_y(iced::Alignment::Center);
    if let Some((label, color)) = label {
        line = line.push(text(label).size(10.0 * scale).color(color));
    }
    // v12: the active window, worded next to the mode so the numbers above
    // are never mistaken for the whole fight. Right-click zooms back out.
    if let Some((lo, hi)) = shown {
        line = line.push(
            text(format!("{}–{} · right-click resets", mmss(lo), mmss(hi)))
                .size(10.0 * scale)
                .color(YELLOW),
        );
    }
    line = line.push(Space::new().width(Length::Fill));
    for kind in kinds {
        line = line.push(key(*kind));
    }
    line.into()
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
            c.into_iter().take(hi).skip(lo)
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
    spans: Vec<(u32, u32)>,
    ghost: Option<(&Timeline, Color)>,
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
        spans,
        ghost: ghost.map(|(g, c)| (curve(g, mode), c)),
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
    /// v14: the Σ graph's encounter lane — `[lo, hi)` ms spans where the
    /// visit's boss fights ran, drawn as green bars along the bottom edge
    /// so an aggregated curve shows where the pulls were. Empty elsewhere.
    spans: Vec<(u32, u32)>,
    /// v16: a context curve drawn faded UNDER the main one — the player's
    /// whole line behind the drilled ability's, so "when did this spell
    /// matter" reads against "when did the player do anything". Shares the
    /// y-scale (`peak` covers both).
    ghost: Option<(Vec<f64>, Color)>,
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
        // With an encounter lane, the curve's FLOOR also rises above it, so
        // the line never runs through the green.
        let lane_h = 2.0 * self.scale;
        let floor = if self.spans.is_empty() {
            0.0
        } else {
            lane_h + 2.0 * self.scale
        };
        let curve_top = (ICON_BAND + 2.0 * ICON_SIZE).min(h * 0.5);
        let y_of = move |v: f64| {
            if self.peak <= 0.0 {
                h - floor
            } else {
                h - floor - (v / self.peak) as f32 * (h - floor - curve_top).max(0.0)
            }
        };

        // v14: the encounter lane — green bars along the bottom edge marking
        // where the visit's boss fights ran, in the same green the segment
        // navigation wears. Drawn under everything, and below the curve's
        // raised floor, so the line and the lane never touch.
        for &(lo, hi) in &self.spans {
            let x1 = self.x_of(lo as f64 / self.bucket_ms, w).clamp(0.0, w);
            let x2 = self.x_of(hi as f64 / self.bucket_ms, w).clamp(0.0, w);
            if x2 <= x1 {
                continue;
            }
            frame.fill(
                &Path::rectangle(Point::new(x1, h - lane_h), Size::new(x2 - x1, lane_h)),
                Color { a: 0.9, ..GREEN },
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

        // Where the curve sits at a marker's instant, for hanging its line.
        // With a ghost behind the focus (the ability drill), the line stops
        // at whichever curve is HIGHER there — a focus line hugging the
        // floor would otherwise let every marker span the whole graph, and
        // the picket fence would be back. Out-of-curve marks fall to the
        // floor.
        let curve_y_at = |m: &Mark| -> f32 {
            let b = (m.at_ms as f64 / self.bucket_ms).round() as usize;
            let own = self.points.get(b).map(|v| y_of(*v)).unwrap_or(h);
            let ghosted = self
                .ghost
                .as_ref()
                .and_then(|(g, _)| g.get(b))
                .map(|v| y_of(*v))
                .unwrap_or(h);
            own.min(ghosted)
        };

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
        Probe(Option<f64>),
        Spell((String, String)),
    }

    fn ctl(hover: Option<&str>, probe: Option<f64>) -> GraphCtl<Ev> {
        GraphCtl {
            on_range: Rc::new(Ev::Range),
            on_hover: Rc::new(Ev::Hover),
            hover: hover.map(str::to_string),
            on_probe: Rc::new(Ev::Probe),
            probe,
            on_spell: Rc::new(Ev::Spell),
        }
    }

    fn mark(at_ms: i64, kind: MarkKind, label: &str, dur_ms: i64) -> Mark {
        Mark {
            at_ms,
            kind,
            label: label.to_string(),
            spell_id: 0,
            dur_ms,
            src: String::new(),
        }
    }

    /// Ten 1s buckets with a use at 2s (10s buff) and a proc at 7s.
    fn marked() -> Timeline {
        Timeline {
            bucket_ms: 1000,
            buckets: vec![100, 200, 300, 400, 500, 400, 300, 200, 100, 50],
            marks: vec![
                mark(2_000, MarkKind::TrinketUse, "Trinket", 10_000),
                mark(7_000, MarkKind::TrinketProc, "Proc", 0),
                mark(4_000, MarkKind::Consumable, "Potion", 25_000),
                mark(1_000, MarkKind::External, "Bloodlust", 40_000),
            ],
        }
    }

    fn graph_of(t: &Timeline, view: (usize, usize), hover: Option<&str>) -> Graph<Ev> {
        Graph {
            points: curve(t, GraphMode::Dps),
            marks: t.marks.clone(),
            bucket_ms: t.bucket_ms as f64,
            color: YELLOW,
            peak: peak_of(&[t], GraphMode::Dps, view),
            view,
            scale: 1.0,
            spans: Vec::new(),
            ghost: None,
            ctl: ctl(hover, None),
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

    #[test]
    fn marker_colors_and_names_are_distinct_per_kind() {
        // R18: every kind has a name and a colour; the exhaustive list is
        // the model's eight, in code order. Names are all distinct; colours
        // too, except the one documented pair — active mitigation and
        // defensives share the coral.
        let kinds = ALL_KINDS;
        assert_eq!(kinds.len(), 8);
        for (i, k) in kinds.iter().enumerate() {
            assert_eq!(k.code() as usize, i, "{k:?} out of code order");
        }
        let shares_a_hue = |a: MarkKind, b: MarkKind| {
            matches!(
                (a, b),
                (MarkKind::ActiveMitigation, MarkKind::Defensive)
                    | (MarkKind::Defensive, MarkKind::ActiveMitigation)
            )
        };
        for (i, a) in kinds.iter().enumerate() {
            for b in kinds.iter().skip(i + 1) {
                if shares_a_hue(*a, *b) {
                    assert_eq!(mark_color(*a), mark_color(*b));
                } else {
                    assert_ne!(mark_color(*a), mark_color(*b), "{a:?} vs {b:?}");
                }
                assert_ne!(mark_name(*a), mark_name(*b));
            }
        }
        assert_eq!(mark_color(MarkKind::ActiveMitigation), MITIGATION);
        assert_eq!(mark_color(MarkKind::SupportBuff), SUPPORT);
        assert_eq!(mark_color(MarkKind::Cooldown), COOLDOWN);
        assert_eq!(mark_name(MarkKind::ActiveMitigation), "mitigation");
        assert_eq!(mark_name(MarkKind::Defensive), "defensive");
        assert_eq!(mark_name(MarkKind::SupportBuff), "support");
        assert_eq!(mark_name(MarkKind::Cooldown), "cooldown");
        assert_eq!(mark_name(MarkKind::Consumable), "consumable");
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

    #[test]
    fn the_view_window_clamps_and_falls_back() {
        assert_eq!(view_window(None, 1000, 10), (0, 10));
        assert_eq!(view_window(Some((1_000, 3_500)), 1000, 10), (1, 4));
        assert_eq!(
            view_window(Some((0, 99_000)), 1000, 10),
            (0, 10),
            "past the data"
        );
        assert_eq!(
            view_window(Some((5_000, 5_000)), 1000, 10),
            (0, 10),
            "zero width"
        );
        assert_eq!(view_window(Some((9_000, 20_000)), 1000, 10), (9, 10));
        assert_eq!(
            view_window(Some((20_000, 30_000)), 1000, 10),
            (0, 10),
            "beyond"
        );
        assert_eq!(mmss(0), "0:00");
        assert_eq!(mmss(83_000), "1:23");
        assert_eq!(mmss(3_599_999), "59:59");
    }

    #[test]
    fn the_peak_is_taken_over_the_displayed_window_only() {
        let t = timeline(vec![10, 1000, 10, 10]);
        assert_eq!(peak_of(&[&t], GraphMode::Total, (0, 4)), 1030.0);
        assert_eq!(peak_of(&[&t], GraphMode::Total, (0, 1)), 10.0);
        assert_eq!(peak_of(&[&t], GraphMode::Total, (2, 4)), 1030.0);
        assert_eq!(
            peak_of(&[&t], GraphMode::Total, (9, 12)),
            0.0,
            "window past the data"
        );
        assert_eq!(peak_of(&[], GraphMode::Dps, (0, 4)), 0.0);
    }

    #[test]
    fn hover_lines_count_uses_and_uptime_across_both_sides() {
        let a = marked();
        let mut b = marked();
        b.marks.retain(|m| m.label == "Trinket");
        let (kind, name, details) = hover_line(&[&a, &b], "Trinket", (0, 10), &[]).unwrap();
        assert_eq!(kind, MarkKind::TrinketUse);
        assert_eq!(name, "Trinket");
        assert_eq!(details, "trinket use ×2 · uptime 20s · 100%");
        let (_, _, details) = hover_line(&[&a], "Proc", (0, 10), &[]).unwrap();
        assert_eq!(details, "proc ×1", "no duration, no uptime clause");
        let (_, _, details) = hover_line(&[&a], "Trinket", (0, 40), &[]).unwrap();
        assert_eq!(details, "trinket use ×1 · uptime 10s · 25%");
        assert!(hover_line(&[&a], "Nothing", (0, 10), &[]).is_none());
    }

    /// R12's four item kinds — what the legend keyed unconditionally before
    /// R18 made the keys follow the marks on screen.
    const ITEM_KINDS: &[MarkKind] = &[
        MarkKind::TrinketUse,
        MarkKind::TrinketProc,
        MarkKind::Consumable,
        MarkKind::External,
    ];

    fn cast(at_ms: i64, kind: MarkKind, label: &str, dur_ms: i64, src: &str) -> Mark {
        Mark {
            src: src.to_string(),
            ..mark(at_ms, kind, label, dur_ms)
        }
    }

    /// A tank's ten seconds: two Shield Blocks (own), a Pain Suppression from
    /// the priest, an Ebon Might from the evoker, a Combustion-shaped
    /// cooldown, plus one trinket proc — R12 marks and R18 spans side by side.
    fn role_marked() -> Timeline {
        Timeline {
            bucket_ms: 1000,
            buckets: vec![100; 10],
            marks: vec![
                cast(
                    1_000,
                    MarkKind::ActiveMitigation,
                    "Shield Block",
                    6_000,
                    "Player-1-0A",
                ),
                cast(
                    8_000,
                    MarkKind::ActiveMitigation,
                    "Shield Block",
                    2_000,
                    "Player-1-0A",
                ),
                cast(
                    2_000,
                    MarkKind::Defensive,
                    "Shield Wall",
                    8_000,
                    "Player-1-0A",
                ),
                cast(
                    3_000,
                    MarkKind::External,
                    "Pain Suppression",
                    8_000,
                    "Player-1-0B",
                ),
                cast(
                    4_000,
                    MarkKind::SupportBuff,
                    "Ebon Might",
                    10_000,
                    "Player-1-0E",
                ),
                cast(
                    5_000,
                    MarkKind::Cooldown,
                    "Combustion",
                    12_000,
                    "Player-1-0A",
                ),
                mark(7_000, MarkKind::TrinketProc, "Proc", 0),
            ],
        }
    }

    /// R18: the legend keys only the kinds with a mark inside the displayed
    /// window — item marks alone still give the four R12 keys, a tank's
    /// graph adds theirs, and a window with no marks keys nothing.
    #[test]
    fn the_legend_keys_only_the_kinds_shown() {
        assert_eq!(kinds_shown(&[&marked()], (0, 10)), ITEM_KINDS);
        let role = role_marked();
        assert_eq!(
            kinds_shown(&[&role], (0, 10)),
            [
                MarkKind::TrinketProc,
                MarkKind::External,
                MarkKind::ActiveMitigation,
                MarkKind::Defensive,
                MarkKind::SupportBuff,
                MarkKind::Cooldown,
            ]
        );
        // Both graphs of a comparison pool their kinds, in code order.
        assert_eq!(kinds_shown(&[&marked(), &role], (0, 10)), ALL_KINDS);
        // A zoomed window drops the kinds whose marks fall outside it.
        assert_eq!(
            kinds_shown(&[&role], (8, 10)),
            [MarkKind::ActiveMitigation],
            "only the second Shield Block sits in 8..10"
        );
        assert!(kinds_shown(&[&Timeline::default()], (0, 10)).is_empty());

        let mut ui = simulator(legend::<()>(
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

    /// R18: the hover names the caster — resolved to a name when the graph
    /// knows the guid, else the guid's tail — and item marks stay as before.
    #[test]
    fn hover_lines_name_the_caster() {
        let t = role_marked();
        let names = [("Player-1-0B", "Gennar"), ("Player-1-0A", "Tank")];
        let (kind, name, details) = hover_line(&[&t], "Pain Suppression", (0, 10), &names).unwrap();
        assert_eq!(kind, MarkKind::External);
        assert_eq!(name, "Pain Suppression from Gennar");
        assert_eq!(details, "external ×1 · uptime 8s · 80%");
        // An unknown guid shows its tail rather than nothing.
        let (_, name, _) = hover_line(&[&t], "Ebon Might", (0, 10), &names).unwrap();
        assert_eq!(name, "Ebon Might from 0E");
        // Two marks of one label from one caster name them once.
        let (_, name, details) = hover_line(&[&t], "Shield Block", (0, 10), &names).unwrap();
        assert_eq!(name, "Shield Block from Tank");
        assert_eq!(details, "mitigation ×2 · uptime 8s · 80%");
        // Both sides of a comparison: the same external from two priests.
        let mut u = role_marked();
        u.marks.retain(|m| m.label == "Pain Suppression");
        u.marks[0].src = "Player-1-0C".to_string();
        let (_, name, _) = hover_line(&[&t, &u], "Pain Suppression", (0, 10), &names).unwrap();
        assert_eq!(name, "Pain Suppression from Gennar, 0C");
        // No caster, no clause.
        let (_, name, _) = hover_line(&[&t], "Proc", (0, 10), &names).unwrap();
        assert_eq!(name, "Proc");
        assert_eq!(caster_name("Player-1-0A", &names), "Tank");
        assert_eq!(caster_name("Creature-0-1-2-3-4-5", &names), "5");
        assert_eq!(caster_name("nohyphen", &names), "nohyphen");
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
    fn graph_geometry_maps_buckets_to_pixels_and_back() {
        let t = marked();
        let g = graph_of(&t, (0, 10), None);
        assert_eq!(g.span(), 10.0);
        assert_eq!(g.x_of(0.0, W), 0.0);
        assert_eq!(g.x_of(5.0, W), W / 2.0);
        assert_eq!(g.ms_at(0.0, W), 0);
        assert_eq!(g.ms_at(W / 2.0, W), 5_000);
        assert_eq!(g.ms_at(W * 3.0, W), 10_000, "clamped to the edge");
        assert_eq!(g.mark_x(&t.marks[0], W), W * 0.2);
        assert!(g.mark_visible(&t.marks[0]));
        assert_eq!(g.probe_at(0.0, W), Some((0, g.points[0])));
        assert_eq!(
            g.probe_at(W, W),
            Some((9, g.points[9])),
            "clamped to the last bucket"
        );

        // Zoomed to buckets 4..8: the use at 2s is off-screen, the proc at
        // 7s is three quarters across.
        let z = graph_of(&t, (4, 8), None);
        assert_eq!(z.span(), 4.0);
        assert!(!z.mark_visible(&t.marks[0]));
        assert!(z.mark_visible(&t.marks[1]));
        assert_eq!(z.mark_x(&t.marks[1], W), W * 0.75);
        assert_eq!(z.ms_at(0.0, W), 4_000);
        assert_eq!(z.probe_at(0.0, W).map(|(b, _)| b), Some(4));
        assert_eq!(z.probe_at(W, W).map(|(b, _)| b), Some(7));

        // Empty curve: nothing to probe.
        let e = graph_of(&timeline(Vec::new()), (0, 1), None);
        assert_eq!(e.probe_at(50.0, W), None);
    }

    #[test]
    fn the_icon_band_finds_the_nearest_marker_only() {
        let t = marked();
        let g = graph_of(&t, (0, 10), None);
        let proc_x = g.mark_x(&t.marks[1], W);
        assert_eq!(
            g.mark_at(Point::new(proc_x + 3.0, 5.0), W)
                .map(|m| m.label.as_str()),
            Some("Proc")
        );
        assert_eq!(
            g.mark_at(Point::new(proc_x, ICON_BAND + 1.0), W),
            None,
            "below the band"
        );
        assert_eq!(
            g.mark_at(Point::new(proc_x + 30.0, 5.0), W),
            None,
            "too far from any icon"
        );
        let use_x = g.mark_x(&t.marks[0], W);
        assert_eq!(
            g.mark_at(Point::new(use_x, 1.0), W)
                .map(|m| m.label.as_str()),
            Some("Trinket")
        );
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
        assert_eq!(message(a), Some(Ev::Probe(Some(g.points[0]))));
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
            points: curve(&t, GraphMode::Total),
            marks: t.marks.clone(),
            bucket_ms: 1000.0,
            color: GREEN,
            peak: peak_of(&[&t], GraphMode::Total, (0, 10)),
            view: (0, 10),
            scale: 2.0,
            spans: vec![(1_000, 3_000), (5_000, 5_000), (8_000, 12_000)],
            ghost: Some((curve(&t, GraphMode::Total), YELLOW)),
            ctl: ctl(Some("Bloodlust"), Some(3.0)),
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
            GraphMode::Total,
            Some((2_500, 7_500)),
            1.0,
            Some(674_500.0),
            "dps",
            None,
            &[],
            false,
        ));
        assert!(ui.find("total: 674.5k").is_ok());
        assert!(ui.find("0:02–0:07 · right-click resets").is_ok());
        // A hovered item takes the row over.
        let hover = Some((
            MarkKind::TrinketProc,
            "Proc".to_string(),
            "proc ×1".to_string(),
        ));
        let mut ui = simulator(legend::<()>(
            GraphMode::Dps,
            None,
            1.0,
            Some(1.0),
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
        let mut ui = simulator(spell_row::<Ev>(&r, 1.0));
        assert!(ui.find("Chaos Bolt").is_ok());
        assert!(ui.find("12").is_ok());
        assert!(ui.find("25%").is_ok());
        assert!(ui.find("7.5k").is_ok());
        r.count = 0;
        r.crits = 0;
        let mut ui = simulator(spell_row::<Ev>(&r, 1.0));
        assert!(ui.find("—").is_ok(), "no hits: no crit rate, no average");

        let c = ctl(None, None);
        let mut ui = simulator(spell_table::<Ev>(&[], 1.0, &c));
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
        let mut ui = simulator(spell_table::<Ev>(&rows, 1.0, &c));
        assert!(ui.find("Melee").is_ok());
        assert!(ui.find("hits").is_ok());
        assert!(ui.find("crit").is_ok());
        assert!(ui.find("avg").is_ok());
        // Clicking a row asks to drill both sides into it.
        let _ = ui.click("Melee").unwrap();
        let msgs: Vec<Ev> = ui.into_messages().collect();
        assert_eq!(
            msgs,
            vec![Ev::Spell(("Melee".to_string(), "Melee".to_string()))]
        );
    }

    #[test]
    fn waiting_words_each_stage_of_the_pick() {
        let (mut state, mut mock) = tk::kill();
        assert!(
            simulator(waiting::<()>(&state, 1.0))
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
        let mut ui = simulator(compare_body(
            &state,
            1.0,
            120.0,
            true,
            ctl(hover, Some(2_000.0)),
        ));
        assert!(ui.find(short_name(&a_label).as_str()).is_ok());
        assert!(ui.find(short_name(&b_label).as_str()).is_ok());
        assert!(ui.find(a_total.as_str()).is_ok());
        assert!(ui.find(first_spell.as_str()).is_ok());
        match hover {
            Some(l) => assert!(
                ui.find(l).is_ok(),
                "the hovered item names itself in the legend"
            ),
            None => assert!(ui.find("dps: 2.0k").is_ok()),
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
            ctl(None, Some(9.0)),
        ));
        assert!(ui.find("total: 9").is_ok());
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
