//! Rendering. Nothing here mutates state.
//!
//! Layout mirrors the TUI: a segment-list screen and a meter screen whose
//! rows are class-colored bars; an open drilldown replaces the rows with the
//! by-spell / by-target panes.

use iced::widget::{Space, column, container, mouse_area, row, scrollable, stack, text};
use iced::{Border, Color, Element, Font, Length, Theme};

use wowdps_core::app::{App, ListRow, Pane, Screen};
use wowdps_core::fmt::{duration, human, view_name};
use wowdps_core::model::{Class, Row, SegmentKind};

use crate::window::{Gui, Message};

pub(crate) const DIM: Color = Color::from_rgb(0.55, 0.57, 0.62);
pub(crate) const GREEN: Color = Color::from_rgb(0.60, 0.76, 0.47);
pub(crate) const RED: Color = Color::from_rgb(0.88, 0.42, 0.46);
pub(crate) const YELLOW: Color = Color::from_rgb(0.90, 0.75, 0.48);
/// Bar color for players whose COMBATANT_INFO has not been seen yet.
const CLASSLESS: Color = Color::from_rgb(0.42, 0.44, 0.52);

const METER_HINTS: &str =
    "d h i c x K views · [ ] segment · j/k move · enter drill · esc list · q quit";
const DRILL_HINTS: &str = "tab pane · j/k move · esc back · q quit";
const LIST_HINTS: &str = "click or j/k + enter to open · q quit";

pub fn view(state: &Gui) -> Element<'_, Message> {
    let app = &state.app;
    let content: Element<'_, Message> = match app.screen {
        Screen::List => list_screen(app),
        Screen::Meter => meter_screen(app, state.stale_secs()),
    };
    container(content)
        .padding(10)
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}

// ---- the segment list ------------------------------------------------------

fn list_screen(app: &App) -> Element<'static, Message> {
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
            (SegmentKind::Encounter, Some(true)) => ("KILL", GREEN),
            (SegmentKind::Encounter, Some(false)) => ("WIPE", RED),
            (SegmentKind::Encounter, None) => ("", DIM),
            (SegmentKind::Trash, _) => ("", DIM),
        }
    };
    let name_color = match r.kind {
        SegmentKind::Encounter => Color::WHITE,
        SegmentKind::Trash => DIM,
    };
    let line = row![
        text(r.name.clone()).size(13).color(name_color),
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

fn meter_screen(app: &App, stale_secs: Option<u64>) -> Element<'static, Message> {
    let body: Element<'static, Message> = match app.drill.as_ref() {
        Some(_) => drill_body(app),
        None => meter_rows(app),
    };
    let hints = if app.drill.is_some() {
        DRILL_HINTS
    } else {
        METER_HINTS
    };
    column![meter_header(app, stale_secs), body, footer(app, hints)]
        .spacing(8)
        .height(Length::Fill)
        .into()
}

fn meter_header(app: &App, stale_secs: Option<u64>) -> Element<'static, Message> {
    let name = app
        .segment_name()
        .unwrap_or_else(|| "waiting for combat…".to_string());
    let (tag, tag_color) = if app.is_live() {
        ("LIVE", YELLOW)
    } else {
        match app.segment_success() {
            Some(true) => ("KILL", GREEN),
            Some(false) => ("WIPE", RED),
            None => ("", DIM),
        }
    };
    let position = format!("{}/{}", app.segment_index() + 1, app.segment_count().max(1));

    column![
        row![
            text(name).size(16),
            text(tag).size(11).color(tag_color).font(Font::MONOSPACE),
            Space::new().width(Length::Fill),
            text(duration(app.duration_ms()))
                .size(14)
                .font(Font::MONOSPACE),
        ]
        .spacing(8)
        .align_y(iced::Alignment::Center),
        {
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
        },
    ]
    .spacing(2)
    .into()
}

fn meter_rows(app: &App) -> Element<'static, Message> {
    let rows = app.rows();
    let mut list = column![].spacing(2);
    if rows.is_empty() {
        list = list.push(text("nothing to show for this view yet").size(13).color(DIM));
    }
    for (i, r) in rows.iter().enumerate() {
        list = list.push(
            mouse_area(bar_row(r, i == app.row_sel, 24.0, false, 1.0))
                .on_press(Message::MeterRow(i)),
        );
    }
    scrollable(list)
        .height(Length::Fill)
        .width(Length::Fill)
        .into()
}

fn drill_body(app: &App) -> Element<'static, Message> {
    let Some(drill) = app.drill.as_ref() else {
        return meter_rows(app);
    };
    let (by_spell, by_target) = app.breakdown();
    let title = row![
        text(drill.label.clone()).size(14),
        text(format!("— {}", view_name(app.view))).size(12).color(DIM),
    ]
    .spacing(8);

    let panes = row![
        drill_pane("by spell", &by_spell, drill.pane == Pane::Spell, drill.spell_sel),
        drill_pane("by target", &by_target, drill.pane == Pane::Target, drill.target_sel),
    ]
    .spacing(10)
    .height(Length::Fill);

    column![title, panes].spacing(6).into()
}

fn drill_pane(
    title: &'static str,
    rows: &[Row],
    active: bool,
    selected: usize,
) -> Element<'static, Message> {
    let title_color = if active { Color::WHITE } else { DIM };
    let mut list = column![].spacing(2);
    if rows.is_empty() {
        list = list.push(text("—").size(12).color(DIM));
    }
    for (i, r) in rows.iter().enumerate() {
        list = list.push(bar_row(r, active && i == selected, 20.0, true, 1.0));
    }
    column![
        text(title).size(12).color(title_color),
        scrollable(list).height(Length::Fill).width(Length::Fill),
    ]
    .spacing(4)
    .width(Length::FillPortion(1))
    .into()
}

/// One class-colored bar with its labels on top. The bar's width is the row's
/// share of the view total. `compact` drops the secondary columns — drill
/// panes are half a window wide and clip anything more than name + amount.
/// Emits no messages, so it serves any frontend's message type. `scale`
/// multiplies the text sizes: the window renders at 1.0 and zooms through
/// iced's scale factor, but the overlay must zoom manually (iced_layershell
/// 0.19 does not scale pointer coordinates by a custom scale factor, which
/// breaks hit-testing).
pub(crate) fn bar_row<M: 'static>(
    r: &Row,
    selected: bool,
    height: f32,
    compact: bool,
    scale: f32,
) -> Element<'static, M> {
    let (cr, cg, cb) = r.class.map(Class::rgb).unwrap_or((0, 0, 0));
    let color = if r.class.is_some() {
        Color::from_rgb8(cr, cg, cb)
    } else {
        CLASSLESS
    };

    let fill = r.pct.clamp(0.0, 100.0).round() as u16;
    let bar: Element<'static, M> = if fill >= 100 {
        bar_fill(color).width(Length::Fill).into()
    } else if fill == 0 {
        Space::new().width(Length::Fill).height(Length::Fill).into()
    } else {
        row![
            bar_fill(color).width(Length::FillPortion(fill)),
            Space::new().width(Length::FillPortion(100 - fill)).height(Length::Fill),
        ]
        .into()
    };

    let mut labels = row![text(r.label.clone()).size(13.0 * scale)]
        .spacing(10)
        .padding([0, 8])
        .align_y(iced::Alignment::Center)
        .height(Length::Fill);
    labels = labels.push(Space::new().width(Length::Fill));
    if !compact {
        if r.extra > 0 {
            labels = labels.push(
                text(format!("({})", human(r.extra)))
                    .size(11.0 * scale)
                    .color(Color::from_rgba(1.0, 1.0, 1.0, 0.6))
                    .font(Font::MONOSPACE),
            );
        }
        labels = labels.push(
            text(human(r.amount))
                .size(13.0 * scale)
                .font(Font::MONOSPACE),
        );
        if r.per_sec >= 1.0 {
            labels = labels.push(
                text(human(r.per_sec as u64))
                    .size(12.0 * scale)
                    .color(Color::from_rgba(1.0, 1.0, 1.0, 0.75))
                    .font(Font::MONOSPACE),
            );
        }
        labels = labels.push(
            text(format!("{:>4.1}%", r.pct))
                .size(11.0 * scale)
                .color(DIM)
                .font(Font::MONOSPACE),
        );
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

fn footer(app: &App, hints: &'static str) -> Element<'static, Message> {
    match app.status.as_deref() {
        Some(status) => text(status.to_string()).size(12).color(RED).into(),
        None => text(hints).size(11).color(DIM).into(),
    }
}
