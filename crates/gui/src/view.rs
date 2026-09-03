//! Rendering. Nothing here mutates state.
//!
//! Layout mirrors the TUI: a segment-list screen and a meter screen whose
//! rows are class-colored bars; an open drilldown replaces the rows with the
//! by-spell / by-target panes.

use iced::widget::{Space, checkbox, column, container, mouse_area, row, scrollable, stack, text};
use iced::{Border, Color, Element, Font, Length, Theme};

use wowdps_model::fmt::{duration, human, view_name};
use wowdps_model::{ListRow, Pane, Row, Screen, SegmentKind, View};
use wowdps_proto::ClientState;

use crate::compare;
use crate::window::{Gui, Message};

/// A right-lane wrapper for anything inside a `scrollable`: the scrollbar
/// paints OVER the content's right edge, and without this the last column
/// (a %, an amount) sits under it.
pub(crate) fn scroll_clear<'a, M: 'a>(
    content: impl Into<Element<'a, M>>,
) -> iced::widget::Container<'a, M> {
    container(content).padding(iced::Padding {
        top: 0.0,
        right: 10.0,
        bottom: 0.0,
        left: 0.0,
    })
}

pub(crate) const DIM: Color = Color::from_rgb(0.55, 0.57, 0.62);
pub(crate) const GREEN: Color = Color::from_rgb(0.60, 0.76, 0.47);
pub(crate) const RED: Color = Color::from_rgb(0.88, 0.42, 0.46);
pub(crate) const YELLOW: Color = Color::from_rgb(0.90, 0.75, 0.48);
/// Bar color for players whose COMBATANT_INFO has not been seen yet.
const CLASSLESS: Color = Color::from_rgb(0.42, 0.44, 0.52);

const METER_HINTS: &str = "d h i c x K views · [ ] segment · j/k move · enter drill · v compare · t talents · esc list · q quit";
const DRILL_HINTS: &str = "tab pane · j/k move · enter ability · g graph · esc back · q quit";
const SPELL_HINTS: &str = "g graph · esc back · q quit";
const COMPARE_HINTS: &str =
    "g graph mode · click a spell to drill both sides · right-click or esc backs out · q quit";
const LIST_HINTS: &str = "click or j/k + enter to open · q quit";

pub fn view(state: &Gui) -> Element<'_, Message> {
    let app = &state.state;
    // The talent viewer replaces the whole screen while open (`t` / Esc);
    // the ClientState machine underneath keeps running untouched.
    if let Some(ui) = &state.talents {
        return container(crate::talents::screen(ui).map(Message::Talents))
            .padding(10)
            .width(Length::Fill)
            .height(Length::Fill)
            .into();
    }
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
        scrollable(scroll_clear(list))
            .height(Length::Fill)
            .width(Length::Fill),
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
        if app.drill_spell().is_some() {
            SPELL_HINTS
        } else {
            DRILL_HINTS
        }
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
    mouse_area(
        scrollable(scroll_clear(list))
            .height(Length::Fill)
            .width(Length::Fill),
    )
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
        on_spell: std::rc::Rc::new(Message::CompareSpell),
    };
    column![
        meter_header(app, stale_secs, false),
        // R12: right-click anywhere else on the body clears the pair and
        // returns to the meter — pointer parity with Esc.
        mouse_area(compare::compare_body(app, 1.0, 120.0, true, ctl))
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
    // v16: the second level — one ability, its stats and its own curve over
    // the player's ghosted one.
    if let Some((_, spell_label)) = app.drill_spell().cloned() {
        let spell_row = app.drill_spell_row();
        let mut body = column![spell_breadcrumb::<Message>(
            &drill.label,
            &spell_label,
            spell_row.as_ref(),
            1.0
        ),]
        .spacing(10);
        match &spell_row {
            Some(r) => body = body.push(spell_stats::<Message>(r, app.view, 1.0)),
            None => body = body.push(text("no data yet").size(12).color(DIM)),
        }
        // v17: who the ability landed on.
        let targets = app.spell_target_rows();
        body = body
            .push(
                row![
                    text("targets").size(12).color(DIM),
                    Space::new().width(Length::Fill),
                    text("hits · total · %")
                        .size(10)
                        .color(DIM)
                        .font(Font::MONOSPACE),
                ]
                .padding([0, 8]),
            )
            .push(spell_target_list::<Message>(&targets, 20.0, 1.0));
        if let Some(t) = app.drill_timeline().filter(|t| !t.buckets.is_empty()) {
            let class = app
                .rows()
                .iter()
                .find(|r| r.key == drill.key)
                .and_then(|r| r.class);
            let focus_color = spell_row
                .as_ref()
                .and_then(|r| school_color(r.school))
                .unwrap_or(YELLOW);
            let ctl = compare::GraphCtl {
                on_range: std::rc::Rc::new(Message::DrillRange),
                on_hover: std::rc::Rc::new(Message::CompareHover),
                hover: state.compare_hover.clone(),
                on_probe: std::rc::Rc::new(Message::GraphProbe),
                probe: state.graph_probe,
                on_spell: std::rc::Rc::new(Message::CompareSpell),
            };
            let focus = app.spell_timeline().map(|ft| (ft, focus_color));
            let rate = if app.view == View::Healing {
                "hps"
            } else {
                "dps"
            };
            // Same height as the player drill's graph: consistent chart,
            // more room for the targets.
            body = body.push(compare::drill_graph(
                app, t, class, 1.0, 110.0, rate, true, focus, ctl,
            ));
        }
        return body.into();
    }

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
            drill.spell_sel,
            // v16: clicking a spell row descends into the ability.
            (!recap).then_some(Message::SpellRow as fn(usize) -> Message),
        ),
        drill_pane(
            target_title,
            caption,
            &by_target,
            false,
            drill.pane == Pane::Target,
            drill.target_sel,
            None,
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
            on_spell: std::rc::Rc::new(Message::CompareSpell),
        };
        let rate = if app.view == View::Healing {
            "hps"
        } else {
            "dps"
        };
        body = body.push(compare::drill_graph(
            app, t, class, 1.0, 110.0, rate, true, None, ctl,
        ));
    }
    body.into()
}

#[allow(clippy::too_many_arguments)]
fn drill_pane(
    title: &'static str,
    caption: &'static str,
    rows: &[Row],
    recap: bool,
    active: bool,
    selected: usize,
    // v16: what clicking row i becomes — the spell pane descends into the
    // ability drill; other panes stay inert.
    click: Option<fn(usize) -> Message>,
) -> Element<'static, Message> {
    let title_color = if active { Color::WHITE } else { DIM };
    let mut list = column![].spacing(2);
    if rows.is_empty() {
        list = list.push(text("—").size(12).color(DIM));
    }
    // Recap rows are chronological, not sorted, so the max is anywhere.
    let max = rows.iter().map(|r| r.amount).max().unwrap_or(1);
    for (i, r) in rows.iter().enumerate() {
        let el: Element<'static, Message> = if recap {
            recap_row(r, max, 20.0, 1.0, false)
        } else {
            bar_row(r, max, active && i == selected, 20.0, true, 1.0, None)
        };
        list = list.push(match click {
            Some(f) => mouse_area(el).on_press(f(i)).into(),
            None => el,
        });
    }
    column![
        row![
            text(title).size(12).color(title_color),
            Space::new().width(Length::Fill),
            text(caption).size(10).color(DIM).font(Font::MONOSPACE),
        ]
        .padding([0, 8]),
        scrollable(scroll_clear(list))
            .height(Length::Fill)
            .width(Length::Fill),
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
    // Fill + NoWrap inside a clipping container: NoWrap alone keeps the text
    // on one line but iced still PAINTS the overflow, which is how a long
    // "Spell (Pet Name)" label used to run under the number columns.
    let mut labels = labels
        .push(
            container(
                text(r.label.clone())
                    .size(13.0 * scale)
                    .wrapping(text::Wrapping::None),
            )
            .clip(true)
            .width(Length::Fill),
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
        let (primary, secondary, tertiary) = metric_palette(inverted_metrics(r, max));
        labels = labels
            .push(cell(extra, 11.0, tertiary, w_extra))
            .push(cell(human(r.amount), 13.0, primary, w_amount))
            .push(cell(rate, 12.0, secondary, w_rate))
            .push(cell(format!("{:>4.1}%", r.pct), 11.0, tertiary, w_pct));
    } else {
        let (primary, _, _) = metric_palette(inverted_metrics(r, max));
        labels = labels.push(
            text(human(r.amount))
                .size(12.0 * scale)
                .color(primary)
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
    let (primary, secondary, tertiary) = metric_palette(inverted_metrics(r, max));

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
            container(text(name).size(13.0 * scale).wrapping(text::Wrapping::None))
                .clip(true)
                .width(Length::Fill),
        )
        .push(metric(human(r.amount), 12.0, primary, 52.0))
        .push(metric(rate, 12.0, secondary, 50.0))
        .push(metric(format!("{:.1}%", r.pct), 11.0, tertiary, 44.0))
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
    // Fill + NoWrap inside a clipping container: without the clip, iced
    // paints the one-line overflow straight under the hits/crit/total
    // columns (see `bar_row`).
    let mut labels = labels
        .push(
            container(
                text(r.label.clone())
                    .size(12.0 * scale)
                    .wrapping(text::Wrapping::None),
            )
            .clip(true)
            .width(Length::Fill),
        )
        .align_y(iced::Alignment::Center)
        .height(Length::Fill);
    let (primary, secondary, _) = metric_palette(inverted_metrics(r, max));
    if count_only {
        labels = labels.push(metric(human(r.count), 12.0, primary, w_total));
    } else {
        // One size across the three columns — the total already leads by
        // color; a bigger font on top of that read as a mismatch.
        labels = labels
            .push(metric(human(r.count), 12.0, secondary, w_hits))
            .push(metric(
                format!("{:.0}%", r.crit_pct()),
                12.0,
                secondary,
                w_crit,
            ))
            .push(metric(human(r.amount), 12.0, primary, w_total));
    }

    container(stack![bar, labels])
        .height(height)
        .width(Length::Fill)
        .style(move |_: &Theme| row_style(false))
        .into()
}

/// The game's spell-school colors (its own UI palette, softened a touch for
/// bar duty). A multi-school mask (Shadowflame = Shadow|Fire) blends the
/// component colors, exactly how the game names blends.
const SCHOOL_COLORS: [(u32, Color); 7] = [
    (0x01, Color::from_rgb(0.90, 0.87, 0.52)), // Physical
    (0x02, Color::from_rgb(1.00, 0.90, 0.55)), // Holy
    (0x04, Color::from_rgb(1.00, 0.55, 0.25)), // Fire
    (0x08, Color::from_rgb(0.40, 0.87, 0.40)), // Nature
    (0x10, Color::from_rgb(0.55, 0.87, 1.00)), // Frost
    (0x20, Color::from_rgb(0.58, 0.47, 0.85)), // Shadow
    (0x40, Color::from_rgb(1.00, 0.55, 1.00)), // Arcane
];

/// v15: the color for a school bitmask — a component color, or the average
/// of a combo's components. None for 0 or a mask of only unknown bits.
pub(crate) fn school_color(mask: u32) -> Option<Color> {
    let mut acc = (0.0, 0.0, 0.0, 0u32);
    for (bit, c) in SCHOOL_COLORS {
        if mask & bit != 0 {
            acc = (acc.0 + c.r, acc.1 + c.g, acc.2 + c.b, acc.3 + 1);
        }
    }
    (acc.3 > 0).then(|| {
        let n = acc.3 as f32;
        Color::from_rgb(acc.0 / n, acc.1 / n, acc.2 / n)
    })
}

/// v16: the ability drill's context line — "Player ▸ ⬚ Spell", the spell in
/// its school color with its own icon when the cache knows it. Display-only;
/// Esc/right-click back out, so the crumb needs no hit area.
pub(crate) fn spell_breadcrumb<M: 'static>(
    player: &str,
    spell_label: &str,
    spell_row: Option<&Row>,
    scale: f32,
) -> Element<'static, M> {
    let color = spell_row
        .and_then(|r| school_color(r.school))
        .unwrap_or(Color::WHITE);
    let mut line = row![
        text(player.split('-').next().unwrap_or(player).to_string())
            .size(13.0 * scale)
            .color(YELLOW),
        text("▸").size(11.0 * scale).color(DIM),
    ]
    .spacing(6.0 * scale)
    .align_y(iced::Alignment::Center);
    if let Some(h) = spell_row.and_then(|r| crate::spell_icons::handle(r.spell_id)) {
        line = line.push(
            iced::widget::image(h)
                .width(Length::Fixed(14.0 * scale))
                .height(Length::Fixed(14.0 * scale)),
        );
    }
    // The NAME is the flexible part: a long "Spell (Pet Name)" clips inside
    // its Fill container instead of shoving the school tag off the panel —
    // the tag rides the line's right edge, always visible.
    line = line.push(
        container(
            text(spell_label.to_string())
                .size(13.0 * scale)
                .color(color)
                .wrapping(text::Wrapping::None),
        )
        .clip(true)
        .width(Length::Fill),
    );
    // v17: the damage type as a [tag] chip beside the name — bordered in
    // the school's color, so the type reads at a glance without a card.
    if let Some((name, sc)) =
        spell_row.and_then(|r| school_name(r.school).map(|n| (n, school_color(r.school))))
    {
        let sc = sc.unwrap_or(DIM);
        line = line.push(
            container(text(name).size(9.0 * scale).color(sc))
                .padding([1.0 * scale, 5.0 * scale])
                .style(move |_: &Theme| container::Style {
                    background: Some(Color { a: 0.10, ..sc }.into()),
                    border: Border {
                        color: Color { a: 0.55, ..sc },
                        width: 1.0,
                        radius: 3.into(),
                    },
                    ..container::Style::default()
                }),
        );
    }
    line.into()
}

/// v17: the game's name for a school bitmask — the singles, the named
/// combos players actually see, and a component join for the rest.
pub(crate) fn school_name(mask: u32) -> Option<String> {
    let named = match mask {
        0x01 => Some("Physical"),
        0x02 => Some("Holy"),
        0x04 => Some("Fire"),
        0x08 => Some("Nature"),
        0x10 => Some("Frost"),
        0x20 => Some("Shadow"),
        0x40 => Some("Arcane"),
        0x06 => Some("Radiant"),
        0x0C => Some("Volcanic"),
        0x14 => Some("Frostfire"),
        0x18 => Some("Froststorm"),
        0x22 => Some("Twilight"),
        0x24 => Some("Shadowflame"),
        0x28 => Some("Plague"),
        0x30 => Some("Shadowfrost"),
        0x44 => Some("Spellfire"),
        0x48 => Some("Astral"),
        0x50 => Some("Spellfrost"),
        0x60 => Some("Spellshadow"),
        0x7C => Some("Elemental"),
        0x7E => Some("Chromatic"),
        0x7F => Some("Chaos"),
        _ => None,
    };
    if let Some(n) = named {
        return Some(n.to_string());
    }
    let parts: Vec<&str> = [
        (0x01, "Physical"),
        (0x02, "Holy"),
        (0x04, "Fire"),
        (0x08, "Nature"),
        (0x10, "Frost"),
        (0x20, "Shadow"),
        (0x40, "Arcane"),
    ]
    .iter()
    .filter(|(bit, _)| mask & bit != 0)
    .map(|(_, n)| *n)
    .collect();
    (!parts.is_empty()).then(|| parts.join("+"))
}

/// v16: the ability drill's stat strip — the numbers its by-spell row
/// already carried but the table never showed, each in its own card:
/// total, share of the player, hits, crit rate, average hit, the school,
/// and the view's `extra` (overkill/overheal) when there is any.
pub(crate) fn spell_stats<M: 'static>(r: &Row, view: View, scale: f32) -> Element<'static, M> {
    // FillPortion: the cards SHARE the panel's width instead of demanding
    // their own — six of them always fit, at any zoom, with no scrollbar.
    let card = |label: &'static str, value: String, accent: Option<Color>| {
        container(
            column![
                text(value)
                    .size(13.0 * scale)
                    .color(accent.unwrap_or(Color::WHITE))
                    .font(Font::MONOSPACE),
                text(label).size(9.0 * scale).color(DIM),
            ]
            .spacing(2)
            .width(Length::Fill)
            .align_x(iced::Alignment::Center),
        )
        .width(Length::FillPortion(1))
        .padding([5.0 * scale, 4.0 * scale])
        .style(|_: &Theme| container::Style {
            background: Some(Color::from_rgba(1.0, 1.0, 1.0, 0.05).into()),
            border: Border {
                color: Color::from_rgba(1.0, 1.0, 1.0, 0.12),
                width: 1.0,
                radius: 5.into(),
            },
            ..container::Style::default()
        })
    };
    let avg = match r.amount.checked_div(r.count) {
        Some(v) if r.count > 0 => human(v),
        _ => "—".to_string(),
    };
    let crit = if r.count > 0 {
        format!("{:.0}%", r.crit_pct())
    } else {
        "—".to_string()
    };
    let mut line = row![
        card("total", human(r.amount), None),
        card("share", format!("{:.1}%", r.pct), None),
        card("hits", human(r.count), None),
        card("crit", crit, Some(YELLOW)),
        card("avg", avg, None),
    ]
    .spacing(6.0 * scale);
    if r.extra > 0 {
        let what = if view == View::Healing {
            "overheal"
        } else {
            "overkill"
        };
        line = line.push(card(what, human(r.extra), Some(RED)));
    }
    // No scrollbar: the school moved into the breadcrumb's tag, and what is
    // left fits; a rare overflow clips at the panel edge instead of growing
    // chrome.
    line.into()
}

/// v17: the ability drill's target list — who ate the spell, each row a
/// school-tinted bar with amount and share. Shared by the window and the
/// overlay; emits nothing.
pub(crate) fn spell_target_list<M: 'static>(
    rows: &[Row],
    height: f32,
    scale: f32,
) -> Element<'static, M> {
    let mut list = column![].spacing(2);
    if rows.is_empty() {
        list = list.push(text("no data yet").size(12.0 * scale).color(DIM));
    }
    let max = rows.first().map_or(1, |r| r.amount.max(1));
    for r in rows {
        list = list.push(spell_target_row(r, max, height, scale));
    }
    scrollable(scroll_clear(list))
        .height(Length::Fill)
        .width(Length::Fill)
        .into()
}

/// One target row: name over a school-tinted bar, hits · amount · share.
fn spell_target_row<M: 'static>(r: &Row, max: u64, height: f32, scale: f32) -> Element<'static, M> {
    let bar = class_bar(r, max);
    let metric = |s: String, size: f32, color: Color, width: f32| {
        text(s)
            .size(size * scale)
            .color(color)
            .font(Font::MONOSPACE)
            .width(Length::Fixed(width * scale))
            .align_x(iced::Alignment::End)
    };
    let (primary, secondary, tertiary) = metric_palette(inverted_metrics(r, max));
    let labels = row![
        container(
            text(r.label.clone())
                .size(12.0 * scale)
                .wrapping(text::Wrapping::None),
        )
        .clip(true)
        .width(Length::Fill),
        metric(human(r.count), 11.0, secondary, 44.0),
        metric(human(r.amount), 12.0, primary, 52.0),
        metric(format!("{:.1}%", r.pct), 11.0, tertiary, 44.0),
    ]
    .spacing(4)
    .padding([0, 8])
    .align_y(iced::Alignment::Center)
    .height(Length::Fill);
    container(stack![bar, labels])
        .height(height)
        .width(Length::Fill)
        .style(move |_: &Theme| row_style(false))
        .into()
}

/// The color a row's bar wears: its spell school (v15, drill rows), else the
/// player's class, else the classless gray.
fn bar_color(r: &Row) -> Color {
    if let Some(c) = school_color(r.school) {
        return c;
    }
    match r.class {
        Some(c) => {
            let (cr, cg, cb) = c.rgb();
            Color::from_rgb8(cr, cg, cb)
        }
        None => CLASSLESS,
    }
}

/// Whether a row's metric text should flip DARK: its bar is light (a Priest's
/// white, Holy's gold) and long enough to run under the number columns —
/// where the gradient's saturated end would otherwise swallow gray text.
fn inverted_metrics(r: &Row, max: u64) -> bool {
    let c = bar_color(r);
    let lum = 0.299 * c.r + 0.587 * c.g + 0.114 * c.b;
    lum > 0.65 && r.amount as f64 / max.max(1) as f64 >= 0.85
}

/// (primary, secondary, tertiary) metric text colors — the usual
/// white/dim trio, or their dark inversions over a light bar.
fn metric_palette(inverted: bool) -> (Color, Color, Color) {
    if inverted {
        (
            Color::from_rgba(0.05, 0.06, 0.10, 0.95),
            Color::from_rgba(0.05, 0.06, 0.10, 0.80),
            Color::from_rgba(0.05, 0.06, 0.10, 0.70),
        )
    } else {
        (Color::WHITE, Color::from_rgba(1.0, 1.0, 1.0, 0.75), DIM)
    }
}

/// The class-colored bar behind a row's labels. Widths are relative to the
/// list's top amount (`max`), like the in-game meters: rank 1 spans the full
/// row and everyone else is a fraction of it — not of the view total, which
/// squashes every bar once a fight has many contributors.
///
/// v15: a row that knows its spell SCHOOL (by-spell drill rows) wears the
/// school's color instead — Shadow purple, Fire orange, blends for combos —
/// so a drilldown reads damage types at a glance. Meter and by-target rows
/// carry school 0 and keep the class color.
fn class_bar<M: 'static>(r: &Row, max: u64) -> Element<'static, M> {
    let color = bar_color(r);

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
/// against the dark theme, with the text at full contrast on top — and as a
/// left-to-right ramp, dim at the tail and saturated at the bar's leading
/// (right) edge, so every bar reads as pointing at its own length.
fn bar_fill<M: 'static>(color: Color) -> iced::widget::Container<'static, M> {
    container(Space::new().width(Length::Fill).height(Length::Fill)).style(move |_: &Theme| {
        let ramp = iced::gradient::Linear::new(iced::Radians(std::f32::consts::FRAC_PI_2))
            .add_stop(0.0, Color { a: 0.16, ..color })
            .add_stop(1.0, Color { a: 0.55, ..color });
        container::Style {
            background: Some(iced::Background::Gradient(ramp.into())),
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::window::testkit::{self as tk, apply, render, simulator};
    use std::time::Duration;
    use wowdps_model::{Action, Class, Spec};

    fn row(label: &str, amount: u64, class: Option<Class>) -> Row {
        Row {
            key: label.to_string(),
            label: label.to_string(),
            amount,
            count: 10,
            crits: 4,
            per_sec: amount as f64 / 60.0,
            pct: 50.0,
            class,
            ..Row::default()
        }
    }

    /// The element contains a text widget reading exactly `s`.
    fn has<M: 'static>(el: Element<'static, M>, s: &str) -> bool {
        simulator(el).find(s).is_ok()
    }

    fn list_entry(kind: SegmentKind, success: Option<bool>) -> ListRow {
        ListRow {
            kind,
            name: "Somewhere".to_string(),
            start_ms: 0,
            success,
            duration_ms: 83_000,
            live: false,
            instance: None,
            pars_ms: None,
            arena: false,
            encounter: None,
        }
    }

    // ---- pure helpers ------------------------------------------------------

    #[test]
    fn enemy_split_needs_a_contiguous_enemy_block() {
        let mut rows = vec![row("a", 3, None), row("b", 2, None), row("c", 1, None)];
        assert_eq!(enemy_split(&rows), None, "no enemies, no divider");
        rows[1].enemy = true;
        rows[2].enemy = true;
        assert_eq!(enemy_split(&rows), Some(1));
        rows[1].enemy = false;
        rows[0].enemy = true;
        assert_eq!(enemy_split(&rows), None, "mixed order draws nothing");
    }

    #[test]
    fn school_colors_blend_their_components() {
        assert_eq!(school_color(0), None);
        assert_eq!(school_color(0x80), None, "unknown bits only");
        let fire = school_color(0x04).unwrap();
        let shadow = school_color(0x20).unwrap();
        let blend = school_color(0x24).unwrap();
        assert!((blend.r - (fire.r + shadow.r) / 2.0).abs() < 1e-6);
        assert!((blend.g - (fire.g + shadow.g) / 2.0).abs() < 1e-6);
        assert!((blend.b - (fire.b + shadow.b) / 2.0).abs() < 1e-6);
        // Unknown bits mixed in do not disturb a known one.
        assert_eq!(school_color(0x84), Some(fire));
    }

    #[test]
    fn school_names_cover_singles_combos_and_joins() {
        assert_eq!(school_name(0x01).as_deref(), Some("Physical"));
        assert_eq!(school_name(0x40).as_deref(), Some("Arcane"));
        assert_eq!(school_name(0x24).as_deref(), Some("Shadowflame"));
        assert_eq!(school_name(0x7F).as_deref(), Some("Chaos"));
        assert_eq!(school_name(0x41).as_deref(), Some("Physical+Arcane"));
        assert_eq!(school_name(0x23).as_deref(), Some("Physical+Holy+Shadow"));
        assert_eq!(school_name(0), None);
        assert_eq!(school_name(0x80), None);
        // The game's own combo names, every one.
        for (mask, name) in [
            (0x02, "Holy"),
            (0x04, "Fire"),
            (0x08, "Nature"),
            (0x10, "Frost"),
            (0x20, "Shadow"),
            (0x06, "Radiant"),
            (0x0C, "Volcanic"),
            (0x14, "Frostfire"),
            (0x18, "Froststorm"),
            (0x22, "Twilight"),
            (0x28, "Plague"),
            (0x30, "Shadowfrost"),
            (0x44, "Spellfire"),
            (0x48, "Astral"),
            (0x50, "Spellfrost"),
            (0x60, "Spellshadow"),
            (0x7C, "Elemental"),
            (0x7E, "Chromatic"),
        ] {
            assert_eq!(school_name(mask).as_deref(), Some(name), "{mask:#x}");
        }
    }

    #[test]
    fn light_bars_invert_their_metric_text_only_when_long() {
        let priest = row("p", 100, Some(Class::Priest));
        assert!(inverted_metrics(&priest, 100), "white bar at full width");
        assert!(
            !inverted_metrics(&priest, 1000),
            "a short white bar is fine"
        );
        let warlock = row("w", 100, Some(Class::Warlock));
        assert!(!inverted_metrics(&warlock, 100), "purple is dark enough");
        let mut holy = row("h", 100, None);
        holy.school = 0x02;
        assert!(inverted_metrics(&holy, 100), "Holy gold is light");
        assert_eq!(bar_color(&row("x", 1, None)), CLASSLESS);
        assert_eq!(bar_color(&holy), school_color(0x02).unwrap());
        let (a, b, c) = metric_palette(false);
        assert_eq!(a, Color::WHITE);
        assert!(b.a < 1.0);
        assert_eq!(c, DIM);
        let (a, b, c) = metric_palette(true);
        assert!(a.r < 0.1 && b.r < 0.1 && c.r < 0.1, "dark trio");
        assert!(a.a > b.a && b.a > c.a);
    }

    #[test]
    fn selected_rows_get_a_background_and_a_border() {
        let on = row_style(true);
        assert!(on.background.is_some());
        assert_eq!(on.border.width, 1.0);
        let off = row_style(false);
        assert!(off.background.is_none());
        assert_eq!(off.border.width, 0.0);
    }

    #[test]
    fn header_tag_words_each_outcome() {
        let (state, _) = tk::live();
        assert_eq!(header_tag(&state), ("LIVE", YELLOW));
        let (state, _) = tk::kill();
        assert_eq!(header_tag(&state), ("KILL", GREEN));
        let (state, _) = tk::wipe();
        assert_eq!(header_tag(&state), ("WIPE", RED));
        assert_eq!(header_tag(&ClientState::new()), ("", DIM));
        // The raid visit's Σ row: no key timer, so no verdict.
        let (mut state, mut mock) = tk::indexed();
        if let Some(pos) = state
            .list_rows()
            .iter()
            .position(|r| r.kind == SegmentKind::Overall)
        {
            state.set_list_selection(pos);
            apply(&mut state, &mut mock, Action::Open);
            assert_eq!(state.segment_kind(), Some(SegmentKind::Overall));
            // The visit's Σ is the newest entry, so opening it pins Live:
            // the daemon may still call it live.
            if state.is_live() {
                assert_eq!(header_tag(&state), ("LIVE", YELLOW));
            } else {
                assert_eq!(header_tag(&state), ("", DIM));
            }
        }
    }

    // ---- the list ------------------------------------------------------------

    #[test]
    fn the_list_names_every_segment_with_its_verdict() {
        let (state, _) = tk::indexed();
        let rows = state.list_rows();
        assert!(rows.len() >= 3, "{rows:?}");
        let mut ui = simulator(list_screen(&state));
        for r in &rows {
            let name = match r.kind {
                SegmentKind::Overall => format!("Σ {}", r.name),
                _ => r.name.clone(),
            };
            assert!(ui.find(name.as_str()).is_ok(), "{name} listed");
        }
        assert!(ui.find("KILL").is_ok());
        assert!(ui.find("WIPE").is_ok());
        assert!(ui.find(LIST_HINTS).is_ok());
        assert!(ui.find(state.source.as_deref().unwrap()).is_ok());
        let _ = ui.snapshot(&Theme::TokyoNight).unwrap();
    }

    #[test]
    fn an_empty_list_says_so() {
        let state = ClientState::new();
        let mut ui = simulator(list_screen(&state));
        assert!(ui.find("waiting for a combat log…").is_ok());
        assert!(ui.find("no encounters indexed yet").is_ok());
    }

    #[test]
    fn list_rows_word_arena_keystone_and_live_outcomes() {
        let mut live = list_entry(SegmentKind::Encounter, None);
        live.live = true;
        assert!(has(list_row(0, &live, true), "LIVE"));
        let mut win = list_entry(SegmentKind::Encounter, Some(true));
        win.arena = true;
        assert!(has(list_row(0, &win, false), "WIN"));
        let mut loss = list_entry(SegmentKind::Encounter, Some(false));
        loss.arena = true;
        assert!(has(list_row(0, &loss, false), "LOSS"));
        let timed = list_entry(SegmentKind::Overall, Some(true));
        assert!(has(list_row(0, &timed, false), "TIMED"));
        assert!(has(list_row(0, &timed, false), "Σ Somewhere"));
        let over = list_entry(SegmentKind::Overall, Some(false));
        assert!(has(list_row(0, &over, false), "OVER"));
        let open = list_entry(SegmentKind::Overall, None);
        assert!(has(list_row(0, &open, false), "1:23"));
        let trash = list_entry(SegmentKind::Trash, None);
        assert!(has(list_row(0, &trash, false), "Somewhere"));
        let _ = render(list_row(0, &trash, true));
        // A fight without a verdict yet, but no longer live (a cut log).
        let undecided = list_entry(SegmentKind::Encounter, None);
        let mut ui = simulator(list_row(0, &undecided, false));
        assert!(ui.find("Somewhere").is_ok());
        assert!(ui.find("LIVE").is_err());
    }

    // ---- the meter -------------------------------------------------------------

    #[test]
    fn every_view_renders_with_its_own_captions() {
        for view in [
            View::Damage,
            View::Healing,
            View::Interrupts,
            View::CrowdControl,
            View::Dispels,
            View::Deaths,
        ] {
            let (mut state, mut mock) = tk::kill();
            apply(&mut state, &mut mock, Action::SetView(view));
            let rows = state.rows();
            let (gui, _peer) = tk::gui_over(state);
            let mut ui = simulator(meter_screen(&gui));
            assert!(ui.find(view_name(view)).is_ok(), "{view:?} named");
            assert!(ui.find("The Ashen Warden").is_ok());
            assert!(ui.find("KILL").is_ok());
            assert!(ui.find(METER_HINTS).is_ok());
            let caption = match view {
                View::Damage => "(overkill)",
                View::Healing => "(overheal)",
                _ => "count",
            };
            assert!(ui.find(caption).is_ok(), "{view:?} caption");
            match rows.first() {
                Some(top) => {
                    assert!(ui.find(top.label.as_str()).is_ok(), "{view:?} top row");
                    assert!(ui.find("1").is_ok(), "ranked");
                }
                None => assert!(ui.find("nothing to show for this view yet").is_ok()),
            }
            let _ = ui.snapshot(&Theme::TokyoNight).unwrap();
        }
    }

    #[test]
    fn ranks_are_optional_and_the_options_panel_overlays_the_meter() {
        let (state, _) = tk::kill();
        let (mut gui, _peer) = tk::gui_over(state);
        gui.cfg.show_ranks = false;
        let mut ui = simulator(meter_screen(&gui));
        assert!(ui.find("#").is_err(), "no rank column");
        assert!(ui.find("options").is_err());
        gui.options_open = true;
        let mut ui = simulator(meter_screen(&gui));
        assert!(ui.find("options").is_ok());
        let _ = ui.snapshot(&Theme::TokyoNight).unwrap();
        let _ = render(options_panel(&gui.cfg));
    }

    #[test]
    fn a_meter_without_data_waits() {
        let mut state = ClientState::new();
        state.screen = Screen::Meter;
        let (gui, _peer) = tk::gui_over(state);
        let mut ui = simulator(view(&gui));
        assert!(ui.find("waiting for combat…").is_ok());
        assert!(ui.find("nothing to show for this view yet").is_ok());
        assert!(ui.find("1/1").is_ok());
    }

    #[test]
    fn a_live_fight_reports_how_far_behind_the_log_is() {
        let (state, _) = tk::live();
        let (mut gui, _peer) = tk::gui_over(state);
        gui.set_last_snapshot_at(Some(std::time::Instant::now() - Duration::from_secs(9)));
        let mut ui = simulator(meter_screen(&gui));
        assert!(ui.find("LIVE").is_ok());
        assert!(ui.find("no events for 9s").is_ok());
        // A closed fight never shows the notice, however old the data.
        let (state, _) = tk::kill();
        let (mut gui, _peer) = tk::gui_over(state);
        gui.set_last_snapshot_at(Some(std::time::Instant::now() - Duration::from_secs(9)));
        assert!(
            simulator(meter_screen(&gui))
                .find("no events for 9s")
                .is_err()
        );
    }

    #[test]
    fn footer_prefers_the_daemon_status() {
        let mut state = ClientState::new();
        assert!(has(footer(&state, LIST_HINTS), LIST_HINTS));
        state.status = Some("segment gone: the log rotated".to_string());
        assert!(has(
            footer(&state, LIST_HINTS),
            "segment gone: the log rotated"
        ));
        assert!(!has(footer(&state, LIST_HINTS), LIST_HINTS));
    }

    // ---- the drilldown ---------------------------------------------------------

    #[test]
    fn the_drilldown_shows_both_panes_and_the_graph() {
        let (state, mut mock) = tk::drilled();
        let label = state.drill.as_ref().unwrap().label.clone();
        let (by_spell, by_target) = state.breakdown();
        assert!(!by_spell.is_empty() && !by_target.is_empty());
        assert!(
            state.drill_timeline().is_some(),
            "Damage drills carry a timeline"
        );
        let (mut gui, _peer) = tk::gui_over(state);
        gui.graph_probe = Some(1234.0);
        let mut ui = simulator(meter_screen(&gui));
        assert!(ui.find(label.as_str()).is_ok());
        assert!(ui.find("— Damage").is_ok());
        assert!(ui.find("by spell").is_ok());
        assert!(ui.find("by target").is_ok());
        assert!(ui.find(by_spell[0].label.as_str()).is_ok());
        assert!(ui.find(by_target[0].label.as_str()).is_ok());
        assert!(ui.find(DRILL_HINTS).is_ok());
        assert!(ui.find("dps: 1.2k").is_ok(), "the probe readout");
        let _ = ui.snapshot(&Theme::TokyoNight).unwrap();

        // The target pane takes the selection; a zoom window words itself.
        let mut state = std::mem::replace(&mut gui.state, ClientState::new());
        apply(&mut state, &mut mock, Action::SwapPane);
        assert_eq!(state.drill.as_ref().unwrap().pane, Pane::Target);
        state.set_drill_range(Some((0, 10_000)));
        let (gui, _peer) = tk::gui_over(state);
        let mut ui = simulator(meter_screen(&gui));
        assert!(ui.find("0:00–0:10 · right-click resets").is_ok());
        let _ = ui.snapshot(&Theme::TokyoNight).unwrap();
    }

    #[test]
    fn count_views_drill_without_a_graph() {
        for view in [View::Interrupts, View::CrowdControl, View::Dispels] {
            let (mut state, mut mock) = tk::drilled();
            apply(&mut state, &mut mock, Action::SetView(view));
            assert!(state.drill.is_some(), "the drill follows the player");
            let (by_spell, _) = state.breakdown();
            let (gui, _peer) = tk::gui_over(state);
            let mut ui = simulator(meter_screen(&gui));
            assert!(ui.find(format!("— {}", view_name(view)).as_str()).is_ok());
            assert!(ui.find("count").is_ok());
            if by_spell.is_empty() {
                assert!(ui.find("—").is_ok(), "{view:?}: an empty pane says so");
            }
            let _ = ui.snapshot(&Theme::TokyoNight).unwrap();
        }
    }

    #[test]
    fn a_healing_drill_graphs_hps() {
        let (mut state, mut mock) = tk::drilled();
        apply(&mut state, &mut mock, Action::SetView(View::Healing));
        // Drill the healer instead: the graph words its rate as hps.
        apply(&mut state, &mut mock, Action::Back);
        let healer = state
            .rows()
            .iter()
            .position(|r| r.amount > 0)
            .expect("someone healed");
        state.row_sel = healer;
        apply(&mut state, &mut mock, Action::Open);
        let graphed = state
            .drill_timeline()
            .is_some_and(|t| !t.buckets.is_empty());
        let (mut gui, _peer) = tk::gui_over(state);
        gui.graph_probe = Some(2_500.0);
        let mut ui = simulator(meter_screen(&gui));
        assert!(ui.find("— Healing").is_ok());
        if graphed {
            assert!(ui.find("hps: 2.5k").is_ok());
        }
        let _ = ui.snapshot(&Theme::TokyoNight).unwrap();
    }

    #[test]
    fn the_deaths_drill_is_the_recap() {
        let (mut state, mut mock) = tk::wipe();
        apply(&mut state, &mut mock, Action::SetView(View::Deaths));
        assert!(!state.rows().is_empty(), "somebody died in the wipe");
        apply(&mut state, &mut mock, Action::Open);
        let (recap, attackers) = state.breakdown();
        assert!(!recap.is_empty(), "the recap timeline");
        let (gui, _peer) = tk::gui_over(state);
        let mut ui = simulator(meter_screen(&gui));
        assert!(ui.find("death recap").is_ok());
        assert!(ui.find("by attacker").is_ok());
        assert!(ui.find("amount · hp").is_ok());
        assert!(ui.find("— Deaths").is_ok());
        if let Some(a) = attackers.first() {
            assert!(ui.find(a.label.as_str()).is_ok());
        }
        let _ = ui.snapshot(&Theme::TokyoNight).unwrap();
    }

    #[test]
    fn the_ability_drill_shows_breadcrumb_stats_targets_and_focus_curve() {
        let (state, _) = tk::spell_drilled();
        let (_, spell_label) = state.drill_spell().cloned().unwrap();
        let spell_row = state.drill_spell_row().expect("the row behind the ability");
        let targets = state.spell_target_rows();
        assert!(!targets.is_empty());
        assert!(state.spell_timeline().is_some());
        let (mut gui, _peer) = tk::gui_over(state);
        gui.graph_probe = Some(500.0);
        let mut ui = simulator(meter_screen(&gui));
        assert!(ui.find(spell_label.as_str()).is_ok());
        assert!(ui.find("targets").is_ok());
        assert!(ui.find("hits · total · %").is_ok());
        for card in ["total", "share", "hits", "crit", "avg"] {
            assert!(ui.find(card).is_ok(), "{card} card");
        }
        assert!(ui.find(human(spell_row.amount).as_str()).is_ok());
        assert!(ui.find(targets[0].label.as_str()).is_ok());
        assert!(ui.find(SPELL_HINTS).is_ok());
        let _ = ui.snapshot(&Theme::TokyoNight).unwrap();
    }

    #[test]
    fn a_healing_ability_drill_words_its_rate_as_hps() {
        let (mut state, mut mock) = tk::drilled();
        apply(&mut state, &mut mock, Action::SetView(View::Healing));
        assert!(
            state.drill_spell().is_none(),
            "the view change closed the ability"
        );
        // Drill the healer, then their top heal.
        apply(&mut state, &mut mock, Action::Back);
        let healer = state
            .rows()
            .iter()
            .position(|r| r.amount > 0)
            .expect("someone healed");
        state.row_sel = healer;
        apply(&mut state, &mut mock, Action::Open);
        apply(&mut state, &mut mock, Action::Open);
        assert!(state.drill_spell().is_some());
        let graphed = state
            .drill_timeline()
            .is_some_and(|t| !t.buckets.is_empty());
        let (mut gui, _peer) = tk::gui_over(state);
        gui.graph_probe = Some(10.0);
        let mut ui = simulator(meter_screen(&gui));
        assert!(ui.find("targets").is_ok());
        assert!(ui.find(SPELL_HINTS).is_ok());
        if graphed {
            assert!(ui.find("hps: 10").is_ok(), "healing rate word");
        }
        let _ = ui.snapshot(&Theme::TokyoNight).unwrap();
    }

    #[test]
    fn a_drill_without_its_snapshot_falls_back_to_the_rows() {
        let mut state = ClientState::new();
        state.screen = Screen::Meter;
        state.drill = Some(wowdps_model::Drill {
            key: "Player-1".to_string(),
            label: "Ghost".to_string(),
            pane: Pane::Spell,
            spell_sel: 0,
            target_sel: 0,
            spell: Some(("Bolt".to_string(), "Bolt".to_string())),
        });
        let (gui, _peer) = tk::gui_over(state);
        let mut ui = simulator(drill_body(&gui, true));
        assert!(ui.find("no data yet").is_ok(), "no stats without the row");
        assert!(ui.find("Bolt").is_ok());
        assert!(ui.find("Ghost").is_ok());
        let mut plain = ClientState::new();
        plain.screen = Screen::Meter;
        let (gui, _peer) = tk::gui_over(plain);
        assert!(has(
            drill_body(&gui, true),
            "nothing to show for this view yet"
        ));
    }

    // ---- the comparison ----------------------------------------------------------

    #[test]
    fn the_comparison_screen_names_both_players() {
        let (state, _) = tk::compared();
        let (a, b) = state.compare_sides().unwrap();
        let (a_name, b_name) = (a.total.label.clone(), b.total.label.clone());
        let (mut gui, _peer) = tk::gui_over(state);
        gui.graph_probe = Some(2_000.0);
        gui.compare_hover = Some("nothing hovered by that name".to_string());
        let mut ui = simulator(view(&gui));
        let short = |s: &str| s.split('-').next().unwrap().to_string();
        assert!(ui.find(short(&a_name).as_str()).is_ok());
        assert!(ui.find(short(&b_name).as_str()).is_ok());
        assert!(ui.find(COMPARE_HINTS).is_ok());
        assert!(ui.find("The Ashen Warden").is_ok());
        assert!(ui.find("dps: 2.0k").is_ok());
        let _ = ui.snapshot(&Theme::TokyoNight).unwrap();
    }

    #[test]
    fn view_dispatches_every_screen() {
        let (state, _) = tk::indexed();
        let (gui, _peer) = tk::gui_over(state);
        let _ = render(view(&gui));
        let (state, _) = tk::wipe();
        let (mut gui, _peer) = tk::gui_over(state);
        let _ = render(view(&gui));
        gui.talents = Some(crate::talents::TalentsUi::open(None));
        let _ = render(view(&gui));
    }

    // ---- rows ----------------------------------------------------------------------

    #[test]
    fn bar_rows_lay_the_metrics_in_columns() {
        let mut r = row("Thraxx-Nebula-US", 185_370, Some(Class::Warrior));
        r.extra = 5_200;
        r.per_sec = 3_089.5;
        r.pct = 50.83;
        let mut ui = simulator(bar_row::<()>(&r, 185_370, true, 24.0, false, 1.0, Some(3)));
        assert!(ui.find("Thraxx-Nebula-US").is_ok());
        assert!(ui.find("(5.2k)").is_ok());
        assert!(ui.find("185.4k").is_ok());
        assert!(ui.find("3.1k").is_ok());
        assert!(ui.find("50.8%").is_ok());
        assert!(ui.find("3").is_ok(), "the rank");
        let _ = ui.snapshot(&Theme::TokyoNight).unwrap();

        // Compact: the amount only; a sub-1/s rate and no extra go blank.
        let mut quiet = row("Pet", 40, Some(Class::Hunter));
        quiet.per_sec = 0.5;
        let mut ui = simulator(bar_row::<()>(&quiet, 185_370, false, 20.0, true, 1.0, None));
        assert!(ui.find("40").is_ok());
        assert!(ui.find("0").is_err(), "no rate cell in compact rows");
        let _ = ui.snapshot(&Theme::TokyoNight).unwrap();
        let _ = render(bar_row::<()>(
            &quiet, 185_370, false, 20.0, false, 1.5, None,
        ));
        // Zero and full bars take their own branches.
        let _ = render(bar_row::<()>(
            &row("z", 0, None),
            10,
            false,
            20.0,
            false,
            1.0,
            None,
        ));
    }

    #[test]
    fn overlay_rows_strip_the_realm() {
        let r = row("Keanucleavês-Proudmoore-US", 1_000, Some(Class::Rogue));
        let mut ui = simulator(overlay_row::<()>(&r, 2_000, 18.0, 1.0, Some(2)));
        assert!(ui.find("Keanucleavês").is_ok());
        assert!(ui.find("Keanucleavês-Proudmoore-US").is_err());
        assert!(ui.find("1.0k").is_ok());
        assert!(ui.find("50.0%").is_ok());
        assert!(ui.find("2").is_ok());
        let _ = ui.snapshot(&Theme::TokyoNight).unwrap();
        // Under one per second the rate column goes blank.
        let mut idle = row("Idle", 30, None);
        idle.per_sec = 0.4;
        let mut ui = simulator(overlay_row::<()>(&idle, 2_000, 18.0, 1.0, None));
        assert!(ui.find("30").is_ok());
        assert!(ui.find("0").is_err());
    }

    #[test]
    fn overlay_drill_rows_collapse_to_one_column_for_counts() {
        let mut r = row("Chaos Bolt", 90_000, Some(Class::Warlock));
        r.count = 12;
        r.crits = 6;
        r.school = 0x24;
        let mut ui = simulator(overlay_drill_row::<()>(&r, 90_000, 18.0, 1.0, false));
        assert!(ui.find("12").is_ok());
        assert!(ui.find("50%").is_ok());
        assert!(ui.find("90.0k").is_ok());
        let _ = ui.snapshot(&Theme::TokyoNight).unwrap();
        let mut ui = simulator(overlay_drill_row::<()>(&r, 90_000, 18.0, 1.0, true));
        assert!(ui.find("12").is_ok());
        assert!(ui.find("50%").is_err(), "counts cannot crit");
        assert!(ui.find("90.0k").is_err());
    }

    #[test]
    fn recap_rows_word_heals_killing_blows_and_health() {
        let mut heal = row("Flash Heal (Mírelle-Nebula-US)", 1_200, None);
        heal.gain = true;
        heal.hp = Some((50, 100));
        let mut ui = simulator(recap_row::<()>(&heal, 5_000, 20.0, 1.0, false));
        assert!(ui.find("+1.2k").is_ok());
        assert!(ui.find("50%").is_ok());
        let _ = ui.snapshot(&Theme::TokyoNight).unwrap();

        let mut blow = row("Melee (Boss-Realm)", 5_000, None);
        blow.extra = 5_200;
        blow.hp = Some((0, 100));
        let mut ui = simulator(recap_row::<()>(&blow, 5_000, 20.0, 1.0, false));
        assert!(ui.find("(5.2k over)").is_ok());
        assert!(ui.find("5.0k").is_ok());
        assert!(ui.find("0%").is_ok());
        let _ = ui.snapshot(&Theme::TokyoNight).unwrap();

        // Compact: no overkill column, and the attacker loses its realm.
        let mut ui = simulator(recap_row::<()>(&blow, 5_000, 18.0, 1.0, true));
        assert!(ui.find("Melee (Boss)").is_ok());
        assert!(ui.find("(5.2k over)").is_err());

        let plain = row("Shadow Bolt", 2_500, None);
        let _ = render(recap_row::<()>(&plain, 5_000, 20.0, 1.0, true));
        let _ = render(recap_row::<()>(
            &row("nothing", 0, None),
            5_000,
            20.0,
            1.0,
            false,
        ));
    }

    #[test]
    fn spell_stats_cards_word_the_extra_by_view() {
        let mut r = row("Chaos Bolt", 90_000, Some(Class::Warlock));
        r.count = 12;
        r.crits = 6;
        r.pct = 33.3;
        r.extra = 4_000;
        let mut ui = simulator(spell_stats::<()>(&r, View::Damage, 1.0));
        assert!(ui.find("overkill").is_ok());
        assert!(ui.find("4.0k").is_ok());
        assert!(ui.find("33.3%").is_ok());
        assert!(ui.find("7.5k").is_ok(), "average hit");
        assert!(ui.find("50%").is_ok());
        let _ = ui.snapshot(&Theme::TokyoNight).unwrap();
        assert!(has(spell_stats::<()>(&r, View::Healing, 1.0), "overheal"));

        let mut never = row("Unused", 0, None);
        never.count = 0;
        let mut ui = simulator(spell_stats::<()>(&never, View::Damage, 1.0));
        assert!(ui.find("—").is_ok(), "no hits, no average, no crit rate");
        assert!(ui.find("overkill").is_err());
    }

    #[test]
    fn the_breadcrumb_tags_the_school() {
        let mut r = row("Chaos Bolt", 90_000, Some(Class::Warlock));
        r.school = 0x24;
        let mut ui = simulator(spell_breadcrumb::<()>(
            "Tranq-Nebula-US",
            "Chaos Bolt",
            Some(&r),
            1.0,
        ));
        assert!(ui.find("Tranq").is_ok());
        assert!(ui.find("Chaos Bolt").is_ok());
        assert!(ui.find("Shadowflame").is_ok());
        let _ = ui.snapshot(&Theme::TokyoNight).unwrap();
        let mut ui = simulator(spell_breadcrumb::<()>("Tranq", "Melee", None, 1.0));
        assert!(ui.find("Melee").is_ok());
        assert!(ui.find("Physical").is_err(), "no row, no tag");
        // A row with a school no name covers still gets no tag.
        let mut odd = row("Odd", 1, None);
        odd.school = 0x80;
        assert!(!has(
            spell_breadcrumb::<()>("A", "Odd", Some(&odd), 1.0),
            "Physical"
        ));
    }

    #[test]
    fn target_lists_share_the_top_amount() {
        assert!(has(spell_target_list::<()>(&[], 20.0, 1.0), "no data yet"));
        let mut boss = row("The Ashen Warden", 80_000, None);
        boss.count = 20;
        boss.pct = 80.0;
        let mut add = row("Ashen Acolyte", 20_000, None);
        add.count = 5;
        add.pct = 20.0;
        let mut ui = simulator(spell_target_list::<()>(&[boss, add], 20.0, 1.0));
        assert!(ui.find("The Ashen Warden").is_ok());
        assert!(ui.find("Ashen Acolyte").is_ok());
        assert!(ui.find("80.0k").is_ok());
        assert!(ui.find("20.0%").is_ok());
        assert!(ui.find("5").is_ok());
        let _ = ui.snapshot(&Theme::TokyoNight).unwrap();
    }

    #[test]
    fn the_team_divider_and_the_captions_render() {
        assert!(has(team_divider::<()>(11.0), "enemy team"));
        let _ = render(team_divider::<()>(9.0));
        let mut state = ClientState::new();
        state.view = View::Healing;
        let mut ui = simulator(meter_captions(&state, true));
        assert!(ui.find("(overheal)").is_ok());
        assert!(ui.find("hps").is_ok());
        assert!(ui.find("#").is_ok());
        assert!(ui.find("player").is_ok());
        state.view = View::Dispels;
        let mut ui = simulator(meter_captions(&state, false));
        assert!(ui.find("count").is_ok());
        assert!(ui.find("#").is_err());
        let _ = render(rank_cell::<()>(7, 11.0, RANK_W));
        let cleared: Element<'static, ()> = scroll_clear(text("x")).into();
        let _ = render(cleared);
        // A class with a spec: the icon takes the drawn-disc fallback here.
        let mut r = row("Spec", 10, Some(Class::Mage));
        r.spec = Some(Spec::Fire);
        let _ = render(bar_row::<()>(&r, 10, false, 20.0, false, 1.0, Some(1)));
    }
}
