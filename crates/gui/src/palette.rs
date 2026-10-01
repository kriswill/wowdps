//! The command palette (the prototype's `.pal`): Ctrl K, or a press on the
//! top bar's jump box, puts a search over everything the window can go to
//! — the pulls on the rail (the newest few before anything is typed), the
//! players of the pull on the stage, the views and the window's screens —
//! grouped, and narrowed as the reader types by the row filter's
//! accent-folded substring (`fold`), so "akanos" finds Akanôs.
//!
//! The arrows or Ctrl N / Ctrl P move the selection, Enter runs it, Esc or
//! a press outside the card closes it, and so does Ctrl K again. It is
//! window-local like Home and the sheet: `ClientState` never learns it
//! exists, and its keys are the window's, never `keys::action_for`'s.
//!
//! The field is a `text_input`, which captures the press that focuses it,
//! so a click on it gives the focus back on its RELEASE — the row filter's
//! pattern. While the card is up every key belongs to it: what the field
//! takes is typed, and what it lets through (the arrows, Enter with the
//! field unfocused, a letter typed after a click on the list) is the
//! palette's to answer, never the meter's.

use iced::widget::{
    Space, column, container, mouse_area, opaque, row, scrollable, stack, text, text_input,
};
use iced::{Color, Element, Length, Theme, mouse};

pub(crate) use wowdps_gui_logic::palette::*;

use crate::ellipsis::ellipsis;
use crate::line_icons::{LineIcon, line_icon};
use crate::nav;
use crate::theme::{self, size};
use crate::window::Message;

/// The field, the list and its selected line, for the window's operations
/// and its tests.
pub(crate) fn input_id() -> iced::widget::Id {
    iced::widget::Id::new("palette-input")
}
pub(crate) fn list_id() -> iced::widget::Id {
    iced::widget::Id::new("palette-list")
}
pub(crate) fn selected_id() -> iced::widget::Id {
    iced::widget::Id::new("palette-selected")
}

/// The card (`.pal{width:min(580px,calc(100% - 24px));border-radius:10px}`),
/// hung 64 px down over the scrim (`.overlay{padding-top:64px}`).
const CARD_W: f32 = 580.0;
const CARD_RADIUS: f32 = 10.0;
const CARD_TOP: f32 = 64.0;
const CARD_SIDE: f32 = 12.0;
/// The field's row (`.pal-in{gap:10px;padding:0 14px}`, `input{height:46px;
/// font-size:17px}`).
const IN_GAP: f32 = 10.0;
const IN_PAD_X: f32 = 14.0;
const IN_H: f32 = 46.0;
const IN_PX: f32 = 17.0;
/// The field's line box: the frame's `line-height:1.3` (`.app`), which the
/// input's `all:unset` inherits, at its 17 px.
const IN_LINE: f32 = 22.0;
/// The list (`.pal-list{max-height:430px;padding:4px 0 8px}`), a group's
/// heading (`.pal-g{font-size:12.5px;padding:10px 14px 3px;font-weight:
/// 600}`) and an item (`.pal-it{gap:10px;padding:6px 14px;font-size:15px}`,
/// `.h{gap:6px;font-size:13px}`), a player's disc (`.disc{20px}`).
const LIST_H: f32 = 430.0;
const LIST_PAD: iced::Padding = iced::Padding {
    top: 4.0,
    right: 0.0,
    bottom: 8.0,
    left: 0.0,
};
const GROUP_PX: f32 = 12.5;
const GROUP_PAD: iced::Padding = iced::Padding {
    top: 10.0,
    right: 14.0,
    bottom: 3.0,
    left: 14.0,
};
const ITEM_GAP: f32 = 10.0;
const ITEM_PAD: [f32; 2] = [6.0, 14.0];
const ITEM_PX: f32 = 15.0;
const SUB_PX: f32 = 13.0;
const SUB_GAP: f32 = 6.0;
const DISC: f32 = 20.0;
/// The selected line's edge (`box-shadow:inset 2px 0 0 var(--accent)`, the
/// rail's current pull's): what tells the line Enter runs from the rest at
/// a glance, where the raised wash alone is a shade off the card.
const SEL_EDGE: f32 = 2.0;
/// The list's scrollbar: thin, so it floats in the lines' 14 px right
/// inset clear of their words (`.pal-it{padding:6px 14px}`).
const LIST_BAR: f32 = 4.0;
/// The note when nothing matches stands in the list (`.pal-list`'s own
/// padding round it) at its `.note{margin:10px 16px}`.
const EMPTY_MARGIN: [f32; 2] = [10.0, 16.0];

/// The words the field shows before anything is typed: the jump box's own.
pub(crate) const PLACEHOLDER: &str = crate::top_bar::JUMP_WORDS;
/// The card's words when nothing matches.
pub(crate) const NOTHING: &str = "Nothing matches. Try a boss, a player or a view.";

/// The palette over the window: the scrim, which a press closes it from,
/// and the card — the field over the grouped list, the selection raised
/// with the accent's edge.
pub(crate) fn overlay(
    p: &Palette,
    items: &[Item],
    accent: theme::Accent,
) -> Element<'static, Message> {
    let field = text_input(PLACEHOLDER, &p.query)
        .id(input_id())
        .on_input(Message::PaletteQuery)
        // Enter in the field: the field captures it, and says so.
        .on_submit(Message::PaletteSubmit)
        .size(IN_PX)
        .line_height(text::LineHeight::Absolute(IN_LINE.into()))
        .padding([(IN_H - IN_LINE) / 2.0, 0.0])
        .width(Length::Fill)
        .style(field_style);
    let input = row![
        line_icon(LineIcon::Search, size::ICON, theme::INK_3),
        // A press on the field is captured by it; its release gives the
        // window its word that the field has the keys (the filter's way).
        mouse_area(field).on_release(Message::PaletteFocus),
    ]
    .spacing(IN_GAP)
    .padding([0.0, IN_PAD_X])
    .align_y(iced::Alignment::Center);

    // The selection on a line the list holds, however it changed since the
    // window last clamped it.
    let sel = p.sel.min(items.len().saturating_sub(1));
    let mut list = column![];
    let mut group = None;
    for (i, item) in items.iter().enumerate() {
        if group != Some(item.group) {
            group = Some(item.group);
            list = list.push(
                container(
                    text(item.group.name())
                        .size(GROUP_PX)
                        .font(theme::UI_SEMIBOLD)
                        .color(theme::GOLD_DIM),
                )
                .padding(GROUP_PAD),
            );
        }
        list = list.push(line(item, i == sel, accent));
    }
    let body: Element<'static, Message> = if items.is_empty() {
        container(container(nav::note(NOTHING)).padding(EMPTY_MARGIN))
            .padding(LIST_PAD)
            .width(Length::Fill)
            .into()
    } else {
        container(
            scrollable(container(list).padding(LIST_PAD).width(Length::Fill))
                .id(list_id())
                .direction(scrollable::Direction::Vertical(
                    scrollable::Scrollbar::new()
                        .width(LIST_BAR)
                        .scroller_width(LIST_BAR),
                ))
                .height(Length::Shrink),
        )
        .max_height(LIST_H)
        .into()
    };
    let card = container(column![input, nav::hairline::<Message>(), body])
        .width(Length::Fill)
        .max_width(CARD_W)
        .clip(true)
        .style(|_: &Theme| nav::sheet_style(CARD_RADIUS));
    // A press anywhere off the card closes it; one on the card — a heading,
    // its edge — is the card's own, and closes nothing. The scrim is opaque
    // to the pointer as the card is: nothing under it hears a wheel, lights
    // a hover or pops a tooltip while the palette is up.
    let scrim = mouse_area(
        container(Space::new())
            .width(Length::Fill)
            .height(Length::Fill)
            .style(|_: &Theme| container::Style {
                background: Some(theme::SCRIM.into()),
                ..container::Style::default()
            }),
    )
    .on_press(Message::PaletteClose);
    stack![
        opaque(scrim),
        container(opaque(card))
            .width(Length::Fill)
            .height(Length::Fill)
            .padding(iced::Padding {
                top: CARD_TOP,
                right: CARD_SIDE,
                bottom: CARD_SIDE,
                left: CARD_SIDE,
            })
            .align_x(iced::Alignment::Center)
            .align_y(iced::Alignment::Start),
    ]
    .width(Length::Fill)
    .height(Length::Fill)
    .into()
}

/// One item (`.pal-it`): a player's disc, the title, and at the right its
/// words and its key; raised with the accent's edge when selected. A press
/// runs it.
fn line(item: &Item, selected: bool, accent: theme::Accent) -> Element<'static, Message> {
    let mut face = row![].spacing(ITEM_GAP).align_y(iced::Alignment::Center);
    if let Some((class, spec)) = item.disc {
        face = face.push(crate::compare::class_icon::<Message>(
            class, spec, None, DISC,
        ));
    }
    face = face.push(
        container(ellipsis(item.title.clone()).size(ITEM_PX).color(theme::INK)).width(Length::Fill),
    );
    let mut right = row![].spacing(SUB_GAP).align_y(iced::Alignment::Center);
    if !item.sub.is_empty() {
        right = right.push(
            text(item.sub.clone())
                .size(SUB_PX)
                .color(theme::INK_3_TEXT)
                .wrapping(text::Wrapping::None),
        );
    }
    if let Some(key) = item.key {
        right = right.push(nav::kbd::<Message>(key));
    }
    face = face.push(right);
    let body = container(face)
        .padding(ITEM_PAD)
        .width(Length::Fill)
        .style(move |_: &Theme| container::Style {
            background: selected.then(|| theme::RAISE.into()),
            ..container::Style::default()
        });
    let edge = container(Space::new())
        .width(Length::Fixed(SEL_EDGE))
        .height(Length::Fill)
        .style(move |_: &Theme| container::Style {
            background: selected.then(|| accent.base.into()),
            ..container::Style::default()
        });
    let mut cell = container(stack![body, edge]).width(Length::Fill);
    if selected {
        cell = cell.id(selected_id());
    }
    mouse_area(cell)
        .on_press(Message::PaletteRun(item.run.clone()))
        .interaction(mouse::Interaction::Pointer)
        .into()
}

/// The field on the card: no frame of its own (the card is its frame), the
/// value in parchment, the placeholder a hint in the faint ink's text
/// grade, the selection a wash of gold.
fn field_style(_: &Theme, _: text_input::Status) -> text_input::Style {
    text_input::Style {
        background: Color::TRANSPARENT.into(),
        border: iced::Border::default(),
        icon: theme::INK_3,
        placeholder: theme::INK_3_TEXT,
        value: theme::INK,
        selection: theme::SELECTION,
    }
}

#[cfg(test)]
mod tests;
