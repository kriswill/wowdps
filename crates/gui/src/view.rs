//! Rendering. Nothing here mutates state.
//!
//! Layout mirrors the TUI: a segment-list screen and a meter screen whose
//! rows are class-colored bars; an open drilldown replaces the rows with the
//! by-spell / by-target panes.

use iced::widget::{Space, checkbox, column, container, mouse_area, row, scrollable, stack, text};
use iced::{Border, Color, Element, Font, Length, Theme};

use wowdps_model::fmt::{duration, human, view_name};
use wowdps_model::{Class, ListRow, Pane, Row, Screen, SegmentKind, View};
use wowdps_proto::ClientState;

use crate::compare;
use crate::window::{Gui, Message};

pub(crate) const DIM: Color = Color::from_rgb(0.55, 0.57, 0.62);
pub(crate) const GREEN: Color = Color::from_rgb(0.60, 0.76, 0.47);
pub(crate) const RED: Color = Color::from_rgb(0.88, 0.42, 0.46);
pub(crate) const YELLOW: Color = Color::from_rgb(0.90, 0.75, 0.48);
/// Bar color for players whose COMBATANT_INFO has not been seen yet.
const CLASSLESS: Color = Color::from_rgb(0.42, 0.44, 0.52);

const METER_HINTS: &str =
    "d h i c x K views · [ ] segment · j/k move · enter drill · v compare · esc list · q quit";
const DRILL_HINTS: &str = "tab pane · j/k move · g graph · esc back · q quit";
const COMPARE_HINTS: &str =
    "g graph mode · click a class icon to change a pick · right-click or esc to clear · q quit";
const LIST_HINTS: &str = "click or j/k + enter to open · q quit";

pub fn view(state: &Gui) -> Element<'_, Message> {
    let app = &state.state;
    let content: Element<'_, Message> = match app.screen {
        Screen::List => list_screen(app),
        Screen::Meter => meter_screen(state),
        Screen::Compare => compare_screen(
            app,
            state.stale_secs(),
            state.compare_hover.clone(),
            state.graph_probe,
        ),
    };
    container(content)
        .padding(10)
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}

// ---- the segment list ------------------------------------------------------

fn list_screen(app: &ClientState) -> Element<'static, Message> {
    let source = match app.source.as_deref() {
        Some(name) => name.to_string(),
        None => "waiting for a combat log…".to_string(),
    };
    let header = row![
        text(source).size(16),
        Space::new().width(Length::Fill),
        text("encounters").size(12).color(DIM),
    ]
    .align_y(iced::Alignment::Center)
    .spacing(8);

    let rows = app.list_rows();
    let selected = app.list_selection();
    let mut list = column![].spacing(2);
    if rows.is_empty() {
        list = list.push(text("no encounters indexed yet").size(13).color(DIM));
    }
    for (i, r) in rows.iter().enumerate() {
        list = list.push(list_row(i, r, i == selected));
    }

    column![
        header,
        scrollable(list).height(Length::Fill).width(Length::Fill),
        footer(app, LIST_HINTS),
    ]
    .spacing(8)
    .height(Length::Fill)
    .into()
}

fn list_row(i: usize, r: &ListRow, selected: bool) -> Element<'static, Message> {
    let (tag, tag_color) = if r.live {
        ("LIVE", YELLOW)
    } else {
        match (r.kind, r.success) {
            // R13: an arena match's outcome is the home team's, not a boss's.
            (SegmentKind::Encounter, Some(true)) if r.arena => ("WIN", GREEN),
            (SegmentKind::Encounter, Some(false)) if r.arena => ("LOSS", RED),
            (SegmentKind::Encounter, Some(true)) => ("KILL", GREEN),
            (SegmentKind::Encounter, Some(false)) => ("WIPE", RED),
            (SegmentKind::Encounter, None) => ("", DIM),
            // R10: a completed key reads as timed/depleted.
            (SegmentKind::Overall, Some(true)) => ("TIMED", GREEN),
            (SegmentKind::Overall, Some(false)) => ("OVER", RED),
            (SegmentKind::Overall, None) => ("", DIM),
            (SegmentKind::Trash, _) => ("", DIM),
        }
    };
    let name_color = match r.kind {
        SegmentKind::Encounter => Color::WHITE,
        SegmentKind::Overall => YELLOW,
        SegmentKind::Trash => DIM,
    };
    // R10: the Overall header row wears a Σ so it can't be mistaken for a
    // fight with the instance's name.
    let name = match r.kind {
        SegmentKind::Overall => format!("Σ {}", r.name),
        _ => r.name.clone(),
    };
    let line = row![
        text(name).size(13).color(name_color),
        Space::new().width(Length::Fill),
        text(tag).size(11).color(tag_color).font(Font::MONOSPACE),
        text(duration(r.duration_ms))
            .size(12)
            .color(DIM)
            .font(Font::MONOSPACE),
    ]
    .spacing(10)
    .align_y(iced::Alignment::Center);

    mouse_area(
        container(line)
            .padding([4, 8])
            .width(Length::Fill)
            .style(move |_: &Theme| row_style(selected)),
    )
    .on_press(Message::ListRow(i))
    .into()
}

// ---- the meter -------------------------------------------------------------

fn meter_screen(state: &Gui) -> Element<'static, Message> {
    let app = &state.state;
    let show_ranks = state.cfg.show_ranks;
    let mut content = column![meter_header(app, state.stale_secs(), true)].spacing(8);
    let hints = if app.drill.is_some() {
        content = content.push(drill_body(state, show_ranks));
        DRILL_HINTS
    } else {
        content = content
            .push(meter_captions(app, show_ranks))
            .push(meter_rows(app, show_ranks));
        METER_HINTS
    };
    let base = content.push(footer(app, hints)).height(Length::Fill);
    if state.options_open {
        stack![base, options_panel(&state.cfg)].into()
    } else {
        base.into()
    }
}

/// The ⚙ dropdown: durable presentation toggles, saved to the config as
/// they change. One checkbox today; the panel is where later options land.
fn options_panel(cfg: &crate::config::Config) -> Element<'static, Message> {
    let panel = container(
        column![
            text("options").size(10).color(DIM),
            checkbox(cfg.show_ranks)
                .label("row ranks")
                .on_toggle(Message::SetShowRanks)
                .size(14)
                .text_size(12),
        ]
        .spacing(8),
    )
    .padding(10)
    .style(|_: &Theme| container::Style {
        background: Some(Color::from_rgba(0.09, 0.10, 0.14, 0.97).into()),
        border: Border {
            color: Color::from_rgba(1.0, 1.0, 1.0, 0.25),
            width: 1.0,
            radius: 4.into(),
        },
        ..container::Style::default()
    });
    // Anchored under the header's ⚙; the wrapper itself is inert, but the
    // panel swallows presses so rows underneath don't fire through it, and
    // the pointer wandering off the panel dismisses it.
    container(
        mouse_area(panel)
            .on_press(Message::Noop)
            .on_exit(Message::CloseOptions),
    )
    .width(Length::Fill)
    .align_x(iced::Alignment::End)
    .padding([26, 2])
    .into()
}

/// Header badge for the watched segment: LIVE while accumulating, else
/// success worded by kind — KILL/WIPE for fights, TIMED/OVER for a keyed
/// visit's overall (R10).
pub(crate) fn header_tag(app: &ClientState) -> (&'static str, Color) {
    if app.is_live() {
        return ("LIVE", YELLOW);
    }
    let overall = app.segment_kind() == Some(SegmentKind::Overall);
    match (app.segment_success(), overall) {
        // R13: arena matches word the home team's outcome.
        (Some(true), false) if app.segment_arena() => ("WIN", GREEN),
        (Some(false), false) if app.segment_arena() => ("LOSS", RED),
        (Some(true), false) => ("KILL", GREEN),
        (Some(false), false) => ("WIPE", RED),
        (Some(true), true) => ("TIMED", GREEN),
        (Some(false), true) => ("OVER", RED),
        (None, _) => ("", DIM),
    }
}

fn meter_header(
    app: &ClientState,
    stale_secs: Option<u64>,
    gear: bool,
) -> Element<'static, Message> {
    let name = app
        .segment_name()
        .unwrap_or_else(|| "waiting for combat…".to_string());
    let (tag, tag_color) = header_tag(app);
    let position = format!("{}/{}", app.segment_index() + 1, app.segment_count().max(1));

    let mut top = row![
        text(name).size(16),
        text(tag).size(11).color(tag_color).font(Font::MONOSPACE),
        Space::new().width(Length::Fill),
        text(duration(app.duration_ms()))
            .size(14)
            .font(Font::MONOSPACE),
    ]
    .spacing(8)
    .align_y(iced::Alignment::Center);
    if gear {
        top = top.push(mouse_area(text("⚙").size(14).color(DIM)).on_press(Message::ToggleOptions));
    }
    column![top, {
        let mut line = row![text(view_name(app.view)).size(12).color(DIM)].spacing(10);
        // The game buffers log writes; say how far behind the file is
        // rather than let a live fight look frozen.
        if let (true, Some(secs)) = (app.is_live(), stale_secs) {
            line = line.push(
                text(format!("no events for {secs}s"))
                    .size(11)
                    .color(YELLOW),
            );
        }
        line.push(Space::new().width(Length::Fill))
            .push(text(position).size(12).color(DIM).font(Font::MONOSPACE))
    },]
    .spacing(2)
    .into()
}

/// R13: where the enemy team's block starts — the first `enemy` row, but only
/// when the teams are contiguous (sorted views group them; the Deaths view is
/// in death order and stays mixed, so it draws no divider).
pub(crate) fn enemy_split(rows: &[wowdps_model::Row]) -> Option<usize> {
    let split = rows.iter().position(|r| r.enemy)?;
    rows.iter().skip(split).all(|r| r.enemy).then_some(split)
}

/// R13: the line between the teams in a PvP chart. Message-generic like
/// `compare::class_icon`, so both surfaces can use it.
pub(crate) fn team_divider<M: 'static>(size: f32) -> Element<'static, M> {
    let line = || {
        iced::widget::container(iced::widget::Space::new())
            .width(Length::Fill)
            .height(1)
            .style(|_| iced::widget::container::Style {
                background: Some(iced::Background::Color(Color { a: 0.4, ..RED })),
                ..Default::default()
            })
    };
    iced::widget::row![line(), text("enemy team").size(size).color(RED), line(),]
        .spacing(8)
        .align_y(iced::Alignment::Center)
        .into()
}

/// Rank column width (window): fits two digits of 11pt monospace with air.
const RANK_W: f32 = 20.0;

/// The rank label drawn on a bar's left edge, ahead of the name: the row's
/// 1-based sort position, dim so the name still leads. Message-generic so
/// the overlay's rows can use it too (scaled).
fn rank_cell<M: 'static>(rank: usize, size: f32, width: f32) -> Element<'static, M> {
    text(rank.to_string())
        .size(size)
        .color(DIM)
        .font(Font::MONOSPACE)
        .width(Length::Fixed(width))
        .align_x(iced::Alignment::End)
        .into()
}

fn meter_rows(app: &ClientState, show_ranks: bool) -> Element<'static, Message> {
    let rows = app.rows();
    let split = enemy_split(&rows);
    let mut list = column![].spacing(2);
    if rows.is_empty() {
        list = list.push(
            text("nothing to show for this view yet")
                .size(13)
                .color(DIM),
        );
    }
    let max = rows.iter().map(|r| r.amount).max().unwrap_or(1);
    for (i, r) in rows.iter().enumerate() {
        // R13: the teams are grouped; mark where the enemy block starts.
        if split == Some(i) {
            list = list.push(team_divider(11.0));
        }
        // R12: the class icon is the pick target, the rest of the row still
        // drills — two different questions, two different hit areas.
        let icon = mouse_area(compare::class_icon(
            r.class,
            r.spec,
            app.compare_slot(&r.key),
            18.0,
        ))
        .on_press(Message::CompareRow(i));
        list = list.push(
            row![
                icon,
                mouse_area(bar_row(
                    r,
                    max,
                    i == app.row_sel,
                    24.0,
                    false,
                    1.0,
                    show_ranks.then_some(i + 1),
                ))
                .on_press(Message::MeterRow(i)),
            ]
            .spacing(6)
            .align_y(iced::Alignment::Center),
        );
    }
    // R12: right-click clears a lone half-pick (the badged icon) without
    // touching the drill or the selection. The row areas only claim left
    // presses, so the right press reaches this wrapper.
    mouse_area(scrollable(list).height(Length::Fill).width(Length::Fill))
        .on_right_press(Message::ClearCompare)
        .into()
}

// ---- the comparison (R12) --------------------------------------------------

fn compare_screen(
    app: &ClientState,
    stale_secs: Option<u64>,
    hover: Option<String>,
    probe: Option<f64>,
) -> Element<'static, Message> {
    // R12/v12: the graphs' own gestures — drag-select a window, hover a
    // marker, right-click zoom-out (captured by the canvas, so it never
    // falls through to the clear-compare area below).
    let ctl = compare::GraphCtl {
        on_range: std::rc::Rc::new(Message::CompareRange),
        on_hover: std::rc::Rc::new(Message::CompareHover),
        hover,
        on_probe: std::rc::Rc::new(Message::GraphProbe),
        probe,
    };
    column![
        meter_header(app, stale_secs, false),
        // R12: right-click anywhere else on the body clears the pair and
        // returns to the meter — pointer parity with Esc.
        mouse_area(compare::compare_body(app, 1.0, 120.0, ctl))
            .on_right_press(Message::ClearCompare),
        footer(app, COMPARE_HINTS),
    ]
    .spacing(8)
    .height(Length::Fill)
    .into()
}

fn drill_body(state: &Gui, show_ranks: bool) -> Element<'static, Message> {
    let app = &state.state;
    let Some(drill) = app.drill.as_ref() else {
        return meter_rows(app, show_ranks);
    };
    let (by_spell, by_target) = app.breakdown();
    let title = row![
        text(drill.label.clone()).size(14),
        text(format!("— {}", view_name(app.view)))
            .size(12)
            .color(DIM),
    ]
    .spacing(8);

    // Deaths drill into the recap timeline + attacker totals (R9).
    let recap = app.view == View::Deaths;
    let (spell_title, target_title) = if recap {
        ("death recap", "by attacker")
    } else {
        ("by spell", "by target")
    };
    // What the pane's number means in this view, so the columns are as
    // self-describing as the meter's caption line.
    let caption = match app.view {
        View::Damage | View::Healing => "total",
        View::Interrupts | View::CrowdControl | View::Dispels => "count",
        View::Deaths => "total",
    };
    let panes = row![
        drill_pane(
            spell_title,
            if recap { "amount · hp" } else { caption },
            &by_spell,
            recap,
            drill.pane == Pane::Spell,
            drill.spell_sel
        ),
        drill_pane(
            target_title,
            caption,
            &by_target,
            false,
            drill.pane == Pane::Target,
            drill.target_sel
        ),
    ]
    .spacing(10)
    .height(Length::Fill);

    let mut body = column![title, panes].spacing(6);
    // v14: the player's timeline under the panes — the comparison's graph
    // for one side (Damage view only; the daemon sends no timeline
    // otherwise). Drag zooms client-side, right-click zooms out, `g`
    // toggles the curve.
    if let Some(t) = app.drill_timeline().filter(|t| !t.buckets.is_empty()) {
        let class = app
            .rows()
            .iter()
            .find(|r| r.key == drill.key)
            .and_then(|r| r.class);
        let ctl = compare::GraphCtl {
            on_range: std::rc::Rc::new(Message::DrillRange),
            on_hover: std::rc::Rc::new(Message::CompareHover),
            hover: state.compare_hover.clone(),
            on_probe: std::rc::Rc::new(Message::GraphProbe),
            probe: state.graph_probe,
        };
        body = body.push(compare::drill_graph(app, t, class, 1.0, 110.0, ctl));
    }
    body.into()
}

fn drill_pane(
    title: &'static str,
    caption: &'static str,
    rows: &[Row],
    recap: bool,
    active: bool,
    selected: usize,
) -> Element<'static, Message> {
    let title_color = if active { Color::WHITE } else { DIM };
    let mut list = column![].spacing(2);
    if rows.is_empty() {
        list = list.push(text("—").size(12).color(DIM));
    }
    // Recap rows are chronological, not sorted, so the max is anywhere.
    let max = rows.iter().map(|r| r.amount).max().unwrap_or(1);
    for (i, r) in rows.iter().enumerate() {
        list = list.push(if recap {
            recap_row(r, max, 20.0, 1.0, false)
        } else {
            bar_row(r, max, active && i == selected, 20.0, true, 1.0, None)
        });
    }
    column![
        row![
            text(title).size(12).color(title_color),
            Space::new().width(Length::Fill),
            text(caption).size(10).color(DIM).font(Font::MONOSPACE),
        ]
        .padding([0, 8]),
        scrollable(list).height(Length::Fill).width(Length::Fill),
    ]
    .spacing(4)
    .width(Length::FillPortion(1))
    .into()
}

/// Window meter-row column widths: (extra, amount, per-sec, pct). Shared by
/// `bar_row` and the caption line above the list, so the headings sit over
/// their columns by construction.
const WINDOW_COLS: (f32, f32, f32, f32) = (64.0, 56.0, 52.0, 44.0);

/// The caption line over the meter rows: what each column means in the
/// current view. Overkill/overheal ride in `extra` for the rate views;
/// count views show occurrences and no rate.
fn meter_captions(app: &ClientState, show_ranks: bool) -> Element<'static, Message> {
    let (extra_h, amount_h, rate_h) = match app.view {
        View::Damage => ("(overkill)", "total", "dps"),
        View::Healing => ("(overheal)", "total", "hps"),
        View::Interrupts | View::CrowdControl | View::Dispels | View::Deaths => ("", "count", ""),
    };
    let head = |s: &'static str, w: f32| {
        text(s)
            .size(10)
            .color(DIM)
            .font(Font::MONOSPACE)
            .width(Length::Fixed(w))
            .align_x(iced::Alignment::End)
    };
    let (w_extra, w_amount, w_rate, w_pct) = WINDOW_COLS;
    // Mirrors the row shape: 8px padding + 14 ≈ the class icon + gap +
    // the bar's own label padding, so "player" starts where names do.
    let mut line = row![Space::new().width(Length::Fixed(14.0))]
        .spacing(10)
        .padding([0, 8]);
    if show_ranks {
        line = line.push(head("#", RANK_W));
    }
    line.push(text("player").size(10).color(DIM).width(Length::Fill))
        .push(head(extra_h, w_extra))
        .push(head(amount_h, w_amount))
        .push(head(rate_h, w_rate))
        .push(head("%", w_pct))
        .into()
}

/// One class-colored bar with its labels on top. The bar's width is the row's
/// amount relative to `max`, the list's top amount ([`class_bar`]).
/// `compact` drops the secondary columns — drill
/// panes are half a window wide and clip anything more than name + amount.
/// Emits no messages, so it serves any frontend's message type. `scale`
/// multiplies the text sizes: the window renders at 1.0 and zooms through
/// iced's scale factor, but the overlay must zoom manually (iced_layershell
/// 0.19 does not scale pointer coordinates by a custom scale factor, which
/// breaks hit-testing).
pub(crate) fn bar_row<M: 'static>(
    r: &Row,
    max: u64,
    selected: bool,
    height: f32,
    compact: bool,
    scale: f32,
    rank: Option<usize>,
) -> Element<'static, M> {
    let bar = class_bar(r, max);

    let mut labels = row![].spacing(10).padding([0, 8]);
    // The rank rides on the bar itself, ahead of the name, so the bar can
    // hug the class icon.
    if let Some(rank) = rank {
        labels = labels.push(rank_cell(rank, 11.0 * scale, RANK_W * scale));
    }
    // v9: only by-spell drill rows carry a spell id; meter rows are players
    // (id 0), so this never fires for them.
    if let Some(h) = crate::spell_icons::handle(r.spell_id) {
        labels = labels.push(
            iced::widget::image(h)
                .width(Length::Fixed(14.0 * scale))
                .height(Length::Fixed(14.0 * scale)),
        );
    }
    // Fill + NoWrap, like `overlay_row`: the label clips rather than
    // wrapping to a second (hidden) line or displacing the numbers.
    let mut labels = labels
        .push(
            text(r.label.clone())
                .size(13.0 * scale)
                .width(Length::Fill)
                .wrapping(text::Wrapping::None),
        )
        .align_y(iced::Alignment::Center)
        .height(Length::Fill);
    if !compact {
        // Fixed-width right-aligned columns (matching WINDOW_COLS), present
        // even when empty: shrink-width cells made every row's numbers land
        // wherever its text ended, so nothing lined up down the list and a
        // caption line above was impossible.
        let cell = |s: String, size: f32, color: Color, width: f32| {
            text(s)
                .size(size * scale)
                .color(color)
                .font(Font::MONOSPACE)
                .width(Length::Fixed(width * scale))
                .align_x(iced::Alignment::End)
        };
        let (w_extra, w_amount, w_rate, w_pct) = WINDOW_COLS;
        let extra = if r.extra > 0 {
            format!("({})", human(r.extra))
        } else {
            String::new()
        };
        let rate = if r.per_sec >= 1.0 {
            human(r.per_sec as u64)
        } else {
            String::new()
        };
        labels = labels
            .push(cell(
                extra,
                11.0,
                Color::from_rgba(1.0, 1.0, 1.0, 0.6),
                w_extra,
            ))
            .push(cell(human(r.amount), 13.0, Color::WHITE, w_amount))
            .push(cell(
                rate,
                12.0,
                Color::from_rgba(1.0, 1.0, 1.0, 0.75),
                w_rate,
            ))
            .push(cell(format!("{:>4.1}%", r.pct), 11.0, DIM, w_pct));
    } else {
        labels = labels.push(
            text(human(r.amount))
                .size(12.0 * scale)
                .font(Font::MONOSPACE),
        );
    }

    container(stack![bar, labels])
        .height(height)
        .width(Length::Fill)
        .style(move |_: &Theme| row_style(selected))
        .into()
}

/// An overlay meter row: the same class-colored bar, but built for a narrow
/// panel glanced at mid-fight — realm suffixes are stripped from player
/// names, and the metrics sit in fixed-width right-aligned columns
/// (amount · per-second · percent) so the numbers line up down the panel.
/// The overhead/overkill extra is dropped entirely: at this width it is
/// clutter, and the window still shows it.
pub(crate) fn overlay_row<M: 'static>(
    r: &Row,
    max: u64,
    height: f32,
    scale: f32,
    rank: Option<usize>,
) -> Element<'static, M> {
    let bar = class_bar(r, max);

    // "Keanucleavês-Proudmoore-US" → "Keanucleavês". Character names cannot
    // contain '-', so everything from the first dash is realm noise.
    let name = r.label.split('-').next().unwrap_or(&r.label).to_string();

    let metric = |s: String, size: f32, color: Color, width: f32| {
        text(s)
            .size(size * scale)
            .color(color)
            .font(Font::MONOSPACE)
            .width(Length::Fixed(width * scale))
            .align_x(iced::Alignment::End)
    };
    let rate = if r.per_sec >= 1.0 {
        human(r.per_sec as u64)
    } else {
        String::new()
    };

    // Column widths fit their worst case ("108.0M", "211.4k") with a step of
    // air on top — right-aligned columns whose text can touch its left edge
    // read as one smear ("108.0M211.4k") the moment the raid does numbers.
    // The NAME is the flexible part, not a Fill spacer after it: a name is a
    // single unwrappable word, and if it owned its intrinsic width the widest
    // name in the raid would shove the metric columns off-grid at high zoom
    // (three lowercase m's were enough). Fill + NoWrap clips the name instead;
    // the columns never move.
    let mut labels = row![].spacing(8.0 * scale).padding([0, 8]);
    // The rank rides on the bar, ahead of the name (narrower than the
    // window's: the overlay has no room for a two-digit column plus air).
    if let Some(rank) = rank {
        labels = labels.push(rank_cell(rank, 10.0 * scale, 14.0 * scale));
    }
    let labels = labels
        .push(
            text(name)
                .size(13.0 * scale)
                .width(Length::Fill)
                .wrapping(text::Wrapping::None),
        )
        .push(metric(human(r.amount), 12.0, Color::WHITE, 52.0))
        .push(metric(
            rate,
            12.0,
            Color::from_rgba(1.0, 1.0, 1.0, 0.75),
            50.0,
        ))
        .push(metric(format!("{:.1}%", r.pct), 11.0, DIM, 44.0))
        .align_y(iced::Alignment::Center)
        .height(Length::Fill);

    container(stack![bar, labels])
        .height(height)
        .width(Length::Fill)
        .style(move |_: &Theme| row_style(false))
        .into()
}

/// Column widths shared by the overlay drilldown rows and their caption line,
/// so the numbers sit under their headings: (hits, crit%, total).
pub(crate) const OVERLAY_DRILL_COLS: (f32, f32, f32) = (40.0, 40.0, 48.0);

/// A death-recap row (R9): the event bar on top — red for damage, green for
/// heals and consumed absorbs, scaled to the pane's biggest event — and a
/// thin strip under it showing the victim's HP right after the event. The
/// killing blow (overkill in `extra`) gets a hotter red.
pub(crate) fn recap_row<M: 'static>(
    r: &Row,
    max: u64,
    height: f32,
    scale: f32,
    compact: bool,
) -> Element<'static, M> {
    let (color, alpha) = if r.gain {
        (GREEN, 0.30)
    } else if r.extra > 0 {
        (RED, 0.55)
    } else {
        (RED, 0.30)
    };
    let fill = (r.amount as f64 / max.max(1) as f64 * 100.0)
        .clamp(0.0, 100.0)
        .round() as u16;
    let event_bar = part_bar(Color { a: alpha, ..color }, fill);

    // The HP strip: a faint track with the remaining-health fraction lit.
    let hp_strip: Element<'static, M> = match r.hp {
        Some((cur, max_hp)) => {
            let pct = (cur as f64 / max_hp.max(1) as f64 * 100.0)
                .clamp(0.0, 100.0)
                .round() as u16;
            container(part_bar(
                Color {
                    a: 0.55,
                    ..Color::from_rgb(0.35, 0.78, 0.42)
                },
                pct,
            ))
            .style(|_: &Theme| container::Style {
                background: Some(Color::from_rgba(1.0, 1.0, 1.0, 0.06).into()),
                ..container::Style::default()
            })
            .height(Length::Fixed(3.0 * scale))
            .width(Length::Fill)
            .into()
        }
        None => Space::new().height(Length::Fixed(3.0 * scale)).into(),
    };

    let metric = |s: String, size: f32, color: Color, width: f32| {
        text(s)
            .size(size * scale)
            .color(color)
            .font(Font::MONOSPACE)
            .width(Length::Fixed(width * scale))
            .align_x(iced::Alignment::End)
    };
    let sign = if r.gain { "+" } else { "" };
    let hp_txt =
        r.hp.map(|(cur, max_hp)| format!("{:.0}%", cur as f64 / max_hp.max(1) as f64 * 100.0))
            .unwrap_or_default();
    // The overlay is narrow: strip realm suffixes from the attacker/healer in
    // parens, like the meter rows do for player names.
    let label = if compact {
        match r.label.split_once(" (") {
            Some((head, tail)) => {
                let who = tail.trim_end_matches(')');
                let short = who.split('-').next().unwrap_or(who);
                format!("{head} ({short})")
            }
            None => r.label.clone(),
        }
    } else {
        r.label.clone()
    };
    let mut labels = row![text(label).size((if compact { 12.0 } else { 13.0 }) * scale)]
        .spacing(4)
        .padding([0, 8])
        .align_y(iced::Alignment::Center)
        .height(Length::Fill);
    labels = labels.push(Space::new().width(Length::Fill));
    if !compact && r.extra > 0 && !r.gain {
        labels = labels.push(metric(
            format!("({} over)", human(r.extra)),
            11.0,
            Color::from_rgba(1.0, 1.0, 1.0, 0.6),
            72.0,
        ));
    }
    labels = labels.push(metric(
        format!("{sign}{}", human(r.amount)),
        12.0,
        if r.gain { GREEN } else { Color::WHITE },
        52.0,
    ));
    labels = labels.push(metric(hp_txt, 11.0, DIM, 40.0));

    container(stack![
        column![
            container(event_bar)
                .height(Length::Fill)
                .width(Length::Fill),
            hp_strip
        ],
        labels
    ])
    .height(height)
    .width(Length::Fill)
    .style(move |_: &Theme| row_style(false))
    .into()
}

/// A partial-width fill used by the recap bars: `pct` of the row, 0..100.
fn part_bar<M: 'static>(color: Color, pct: u16) -> Element<'static, M> {
    if pct >= 100 {
        recap_fill(color).width(Length::Fill).into()
    } else if pct == 0 {
        Space::new().width(Length::Fill).height(Length::Fill).into()
    } else {
        row![
            recap_fill(color).width(Length::FillPortion(pct)),
            Space::new()
                .width(Length::FillPortion(100 - pct))
                .height(Length::Fill),
        ]
        .into()
    }
}

fn recap_fill<M: 'static>(color: Color) -> iced::widget::Container<'static, M> {
    container(Space::new().width(Length::Fill).height(Length::Fill)).style(move |_: &Theme| {
        container::Style {
            background: Some(color.into()),
            border: iced::border::rounded(2),
            ..container::Style::default()
        }
    })
}

/// An overlay drilldown row: one of the player's spells, with hit count,
/// crit rate and total in the same fixed-width columnar layout as
/// [`overlay_row`]. Count views (interrupts, CC, dispels) can't crit and
/// their total IS the count, so `count_only` collapses to one column.
pub(crate) fn overlay_drill_row<M: 'static>(
    r: &Row,
    max: u64,
    height: f32,
    scale: f32,
    count_only: bool,
) -> Element<'static, M> {
    let bar = class_bar(r, max);
    let metric = |s: String, size: f32, color: Color, width: f32| {
        text(s)
            .size(size * scale)
            .color(color)
            .font(Font::MONOSPACE)
            .width(Length::Fixed(width * scale))
            .align_x(iced::Alignment::End)
    };
    let (w_hits, w_crit, w_total) = OVERLAY_DRILL_COLS;
    let mut labels = row![].spacing(4).padding([0, 8]);
    // v9: by-spell rows carry their spell id — the ability's own art leads
    // the label when the spell-icon cache knows it.
    if let Some(h) = crate::spell_icons::handle(r.spell_id) {
        labels = labels.push(
            iced::widget::image(h)
                .width(Length::Fixed(12.0 * scale))
                .height(Length::Fixed(12.0 * scale)),
        );
    }
    // Fill + NoWrap: a long spell label clips rather than shoving the
    // hits/crit/total columns off-grid (see `overlay_row`).
    let mut labels = labels
        .push(
            text(r.label.clone())
                .size(12.0 * scale)
                .width(Length::Fill)
                .wrapping(text::Wrapping::None),
        )
        .align_y(iced::Alignment::Center)
        .height(Length::Fill);
    if count_only {
        labels = labels.push(metric(human(r.count), 12.0, Color::WHITE, w_total));
    } else {
        labels = labels
            .push(metric(
                human(r.count),
                11.0,
                Color::from_rgba(1.0, 1.0, 1.0, 0.75),
                w_hits,
            ))
            .push(metric(
                format!("{:.0}%", r.crit_pct()),
                11.0,
                Color::from_rgba(1.0, 1.0, 1.0, 0.75),
                w_crit,
            ))
            .push(metric(human(r.amount), 12.0, Color::WHITE, w_total));
    }

    container(stack![bar, labels])
        .height(height)
        .width(Length::Fill)
        .style(move |_: &Theme| row_style(false))
        .into()
}

/// The class-colored bar behind a row's labels. Widths are relative to the
/// list's top amount (`max`), like the in-game meters: rank 1 spans the full
/// row and everyone else is a fraction of it — not of the view total, which
/// squashes every bar once a fight has many contributors.
fn class_bar<M: 'static>(r: &Row, max: u64) -> Element<'static, M> {
    let (cr, cg, cb) = r.class.map(Class::rgb).unwrap_or((0, 0, 0));
    let color = if r.class.is_some() {
        Color::from_rgb8(cr, cg, cb)
    } else {
        CLASSLESS
    };

    let fill = (r.amount as f64 / max.max(1) as f64 * 100.0)
        .clamp(0.0, 100.0)
        .round() as u16;
    if fill >= 100 {
        bar_fill(color).width(Length::Fill).into()
    } else if fill == 0 {
        Space::new().width(Length::Fill).height(Length::Fill).into()
    } else {
        row![
            bar_fill(color).width(Length::FillPortion(fill)),
            Space::new()
                .width(Length::FillPortion(100 - fill))
                .height(Length::Fill),
        ]
        .into()
    }
}

/// The colored part of a bar. Class colors read best a touch translucent
/// against the dark theme, with the text at full contrast on top.
fn bar_fill<M: 'static>(color: Color) -> iced::widget::Container<'static, M> {
    container(Space::new().width(Length::Fill).height(Length::Fill)).style(move |_: &Theme| {
        container::Style {
            background: Some(Color { a: 0.35, ..color }.into()),
            border: iced::border::rounded(3),
            ..container::Style::default()
        }
    })
}

fn row_style(selected: bool) -> container::Style {
    let background = if selected {
        Some(Color::from_rgba(1.0, 1.0, 1.0, 0.06).into())
    } else {
        None
    };
    let border = if selected {
        Border {
            color: Color::from_rgba(1.0, 1.0, 1.0, 0.35),
            width: 1.0,
            radius: 3.into(),
        }
    } else {
        iced::border::rounded(3)
    };
    container::Style {
        background,
        border,
        ..container::Style::default()
    }
}

// ---- shared chrome ---------------------------------------------------------

fn footer(app: &ClientState, hints: &'static str) -> Element<'static, Message> {
    match app.status.as_deref() {
        Some(status) => text(status.to_string()).size(12).color(RED).into(),
        None => text(hints).size(11).color(DIM).into(),
    }
}
