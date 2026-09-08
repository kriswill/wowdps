//! The shell around whatever a screen is showing: the tab bar, the jump
//! chips, the two-tone title, stat cards, panels, the row filter and the `?`
//! sheet.
//!
//! Everything here is message-generic and `'static`, taking the message to
//! emit as an argument rather than naming `window::Message`, so the overlay
//! can adopt any of it later without a message-type fight. Nothing in this
//! module reads state; callers hand it what to draw.

use iced::widget::{
    Space, column, container, mouse_area, row, scrollable, stack, text, text_input,
};
use iced::{Border, Color, Element, Font, Length, Theme};

use wowdps_model::View;

use crate::keys;
use crate::theme::{self, Density, size};

/// One entry of the tab bar.
#[derive(Debug, Clone)]
pub(crate) struct Tab<M> {
    /// A text glyph, not an image: spell art would cost a cache lookup per
    /// tab for decoration a letter already carries.
    pub glyph: &'static str,
    pub label: &'static str,
    /// The key that also does it, or "" — the tab bar doubles as a keymap
    /// reminder.
    pub hint: &'static str,
    pub active: bool,
    /// `None` renders the tab disabled: dim and inert. A tab that leads
    /// nowhere yet must LOOK like it leads nowhere, never silently no-op.
    pub on_press: Option<M>,
}

/// The glyph each view wears, in `View::ALL` order. One place, so a later
/// change is one edit.
pub(crate) const TAB_GLYPHS: [(View, &str); View::COUNT] = [
    (View::Damage, "⚔"),
    (View::Healing, "✚"),
    (View::Interrupts, "⛔"),
    (View::CrowdControl, "✋"),
    (View::Dispels, "✨"),
    (View::Deaths, "⚰"),
    (View::Taken, "🛡"),
];

pub(crate) fn tab_glyph(view: View) -> &'static str {
    TAB_GLYPHS
        .iter()
        .find(|(v, _)| *v == view)
        .map_or("·", |(_, g)| *g)
}

/// The icon tab bar. The active tab wears the accent gradient and its ink;
/// the rest are dim. Wrapped in a horizontal `scrollable` so a narrow window
/// scrolls the strip rather than clipping a tab off the end.
pub(crate) fn tab_bar<M: Clone + 'static>(
    tabs: Vec<Tab<M>>,
    accent: theme::Accent,
    density: Density,
) -> Element<'static, M> {
    let mut strip = row![].spacing(density.gap() / 2.0);
    for t in tabs {
        let enabled = t.on_press.is_some();
        let ink = if t.active {
            accent.ink
        } else if enabled {
            theme::DIM
        } else {
            Color {
                a: 0.45,
                ..theme::DIM
            }
        };
        let mut label = row![text(t.glyph).size(size::BODY).color(ink)].spacing(4);
        label = label.push(text(t.label).size(size::MICRO).color(ink));
        if !t.hint.is_empty() {
            label = label.push(
                text(t.hint)
                    .size(size::TINY)
                    .color(Color { a: 0.7, ..ink })
                    .font(Font::MONOSPACE),
            );
        }
        let cell = container(label.align_y(iced::Alignment::Center))
            .padding([2.0, density.pad() / 2.0 + 2.0])
            .style(move |_: &Theme| container::Style {
                background: t.active.then(|| theme::accent_fill(accent)),
                border: Border {
                    radius: 3.into(),
                    ..Border::default()
                },
                ..container::Style::default()
            });
        strip = strip.push(match t.on_press {
            Some(msg) => mouse_area(cell).on_press(msg).into(),
            None => Element::from(cell),
        });
    }
    scrollable(strip)
        .direction(scrollable::Direction::Horizontal(
            scrollable::Scrollbar::new().width(2).scroller_width(2),
        ))
        .into()
}

/// The "jump to:" chip row over a long screen's sections.
pub(crate) fn chip_row<M: Clone + 'static>(
    sections: Vec<(String, M)>,
    active: Option<usize>,
    accent: theme::Accent,
) -> Element<'static, M> {
    let mut strip = row![text("jump to").size(size::TINY).color(theme::DIM)].spacing(6);
    for (i, (label, msg)) in sections.into_iter().enumerate() {
        let on = active == Some(i);
        let chip = container(text(label).size(size::MICRO).color(if on {
            accent.ink
        } else {
            theme::DIM
        }))
        .padding([1, 6])
        .style(move |_: &Theme| container::Style {
            background: Some(if on {
                theme::accent_fill(accent)
            } else {
                theme::PANEL.into()
            }),
            border: Border {
                radius: 8.into(),
                ..Border::default()
            },
            ..container::Style::default()
        });
        strip = strip.push(mouse_area(chip).on_press(msg));
    }
    strip.align_y(iced::Alignment::Center).into()
}

/// The two-tone title: who this screen is about in the accent's heading
/// color, then what it shows, dim after it.
pub(crate) fn two_tone_title<M: 'static>(
    who: String,
    what: String,
    tag: Option<(String, Color)>,
    accent: theme::Accent,
    size: f32,
) -> Element<'static, M> {
    let mut line = row![
        text(who).size(size).color(accent.heading),
        text(what).size(size * 0.8).color(theme::DIM),
    ]
    .spacing(8)
    .align_y(iced::Alignment::Center);
    if let Some((tag, color)) = tag {
        line = line.push(
            text(tag)
                .size(size * 0.7)
                .color(color)
                .font(Font::MONOSPACE),
        );
    }
    line.into()
}

/// One stat card.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Stat {
    pub label: String,
    /// Already formatted. "—" means "we cannot know this" — never a 0 that
    /// looks like a measurement.
    pub value: String,
    pub sub: Option<String>,
    pub value_color: Option<Color>,
    /// Draw it on the accent gradient: the one number the screen is about.
    pub headline: bool,
}

impl Stat {
    /// The dash a value we cannot derive renders as, with the reason under
    /// it. Kept here so every "we don't know" on every screen looks alike.
    pub(crate) fn unknown(label: &str, why: &str) -> Self {
        Self {
            label: label.to_string(),
            value: "—".to_string(),
            sub: Some(why.to_string()),
            value_color: None,
            headline: false,
        }
    }
}

pub(crate) fn stat_cards<M: 'static>(
    cards: &[Stat],
    accent: theme::Accent,
    density: Density,
) -> Element<'static, M> {
    let mut strip = row![].spacing(density.gap());
    for c in cards {
        let headline = c.headline;
        let value_color = match (headline, c.value_color) {
            (true, _) => accent.ink,
            (false, Some(color)) => color,
            (false, None) => Color::WHITE,
        };
        let label_color = if headline {
            Color {
                a: 0.8,
                ..accent.ink
            }
        } else {
            theme::DIM
        };
        let mut body = column![
            text(c.label.clone()).size(size::TINY).color(label_color),
            text(c.value.clone())
                .size(if headline { size::DISPLAY } else { size::HEAD })
                .color(value_color)
                .font(Font::MONOSPACE),
        ]
        .spacing(1);
        if let Some(sub) = c.sub.clone() {
            body = body.push(text(sub).size(size::TINY).color(label_color));
        }
        strip = strip.push(
            container(body)
                .padding(density.pad())
                .width(Length::FillPortion(1))
                .style(move |_: &Theme| container::Style {
                    background: Some(if headline {
                        theme::accent_fill(accent)
                    } else {
                        theme::PANEL.into()
                    }),
                    border: Border {
                        color: theme::RULE,
                        width: if headline { 0.0 } else { 1.0 },
                        radius: 4.into(),
                    },
                    ..container::Style::default()
                }),
        );
    }
    strip.into()
}

/// A titled panel: eyebrow label, right-aligned caption, body, and an
/// optional footer line that leads somewhere.
pub(crate) fn panel<'a, M: Clone + 'static>(
    title: &str,
    caption: Option<String>,
    body: impl Into<Element<'a, M>>,
    footer: Option<(String, M)>,
    accent: theme::Accent,
) -> Element<'a, M> {
    let mut head = row![
        text(title.to_string())
            .size(size::TINY)
            .color(accent.heading)
    ]
    .spacing(8)
    .align_y(iced::Alignment::Center);
    head = head.push(Space::new().width(Length::Fill));
    if let Some(caption) = caption {
        head = head.push(
            text(caption)
                .size(size::TINY)
                .color(theme::DIM)
                .font(Font::MONOSPACE),
        );
    }
    let mut col = column![head, body.into()].spacing(4);
    if let Some((label, msg)) = footer {
        col =
            col.push(mouse_area(text(label).size(size::TINY).color(accent.heading)).on_press(msg));
    }
    container(col)
        .padding(8)
        .width(Length::Fill)
        .style(|_: &Theme| container::Style {
            background: Some(theme::PANEL.into()),
            border: Border {
                color: theme::RULE,
                width: 1.0,
                radius: 4.into(),
            },
            ..container::Style::default()
        })
        .into()
}

/// The filter field's id, so `update` can focus it from the `/` key.
pub(crate) fn filter_id() -> iced::widget::Id {
    iced::widget::Id::new("row-filter")
}

/// The row filter box. Typing in it must not reach the meter keymap — the
/// caller owns that (`window.rs` swallows keys while it has focus).
pub(crate) fn filter_box<M: Clone + 'static>(
    value: &str,
    on_input: impl Fn(String) -> M + 'static,
    on_clear: M,
    on_focus: M,
) -> Element<'static, M> {
    let field = text_input("filter rows…", value)
        .id(filter_id())
        .on_input(on_input)
        .size(size::MICRO)
        .padding([2, 6])
        .width(Length::Fill);
    let mut line = row![
        text("/")
            .size(size::TINY)
            .color(theme::DIM)
            .font(Font::MONOSPACE),
        // A click on the field itself focuses it; the wrapper tells the
        // window so the keymap starts being swallowed at the same moment.
        mouse_area(field).on_press(on_focus),
    ]
    .spacing(6)
    .align_y(iced::Alignment::Center);
    if !value.is_empty() {
        line =
            line.push(mouse_area(text("✕").size(size::MICRO).color(theme::DIM)).on_press(on_clear));
    }
    line.into()
}

/// The `?` sheet: every binding, grouped, on a card over a dimmed scrim.
/// Any press anywhere dismisses it.
pub(crate) fn shortcut_sheet<M: Clone + 'static>(
    accent: theme::Accent,
    on_dismiss: M,
) -> Element<'static, M> {
    let mut card = column![text("keys").size(size::HEAD).color(accent.heading),].spacing(6);
    for group in keys::GROUPS {
        let mut lines = column![text(group).size(size::TINY).color(theme::DIM)].spacing(1);
        for b in keys::BINDINGS.iter().filter(|b| b.group == group) {
            lines = lines.push(
                row![
                    text(b.keys)
                        .size(size::MICRO)
                        .color(accent.heading)
                        .font(Font::MONOSPACE)
                        .width(Length::Fixed(52.0)),
                    text(b.what).size(size::MICRO).color(Color::WHITE),
                ]
                .spacing(8),
            );
        }
        card = card.push(lines);
    }
    let sheet = container(card)
        .padding(12)
        .style(|_: &Theme| container::Style {
            background: Some(theme::PANEL.into()),
            border: Border {
                color: theme::RULE,
                width: 1.0,
                radius: 6.into(),
            },
            ..container::Style::default()
        });
    // The scrim is the dismiss target as much as the card is: a modal you
    // cannot click away from is a trap.
    let scrim = container(Space::new())
        .width(Length::Fill)
        .height(Length::Fill)
        .style(|_: &Theme| container::Style {
            background: Some(Color::from_rgba(0.0, 0.0, 0.0, 0.55).into()),
            ..container::Style::default()
        });
    mouse_area(
        stack![
            scrim,
            container(sheet)
                .width(Length::Fill)
                .height(Length::Fill)
                .align_x(iced::Alignment::Center)
                .align_y(iced::Alignment::Center),
        ]
        .width(Length::Fill)
        .height(Length::Fill),
    )
    .on_press(on_dismiss)
    .into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::window::testkit::{render, simulator};

    #[derive(Debug, Clone, PartialEq)]
    enum M {
        Pick(View),
        Dismiss,
    }

    fn tabs() -> Vec<Tab<M>> {
        let mut out: Vec<Tab<M>> = View::ALL
            .into_iter()
            .map(|v| Tab {
                glyph: tab_glyph(v),
                label: wowdps_model::fmt::view_name(v),
                hint: "",
                active: v == View::Healing,
                on_press: Some(M::Pick(v)),
            })
            .collect();
        // The History tab: built, disabled, deliberately inert.
        out.push(Tab {
            glyph: "⏱",
            label: "history",
            hint: "",
            active: false,
            on_press: None,
        });
        out
    }

    #[test]
    fn tab_bar_renders_every_view_and_marks_the_active_one() {
        let mut ui = simulator(tab_bar(tabs(), theme::NEUTRAL, Density::Comfortable));
        for v in View::ALL {
            assert!(
                ui.find(wowdps_model::fmt::view_name(v)).is_ok(),
                "{v:?} is missing"
            );
        }
        assert!(ui.find("history").is_ok());
        let _ = ui.snapshot(&Theme::TokyoNight).unwrap();
    }

    #[test]
    fn a_disabled_tab_emits_no_message() {
        let only_history = vec![Tab::<M> {
            glyph: "⏱",
            label: "history",
            hint: "",
            active: false,
            on_press: None,
        }];
        let mut ui = simulator(tab_bar(only_history, theme::NEUTRAL, Density::Comfortable));
        let _ = ui.click("history");
        assert!(ui.into_messages().next().is_none(), "a dead tab is silent");
    }

    #[test]
    fn a_live_tab_emits_its_message() {
        let mut ui = simulator(tab_bar(tabs(), theme::NEUTRAL, Density::Comfortable));
        ui.click("Dispels").unwrap();
        assert_eq!(
            ui.into_messages().collect::<Vec<_>>(),
            vec![M::Pick(View::Dispels)]
        );
    }

    #[test]
    fn stat_cards_render_a_headline_and_an_em_dash() {
        let cards = [
            Stat {
                label: "median dps".to_string(),
                value: "1.2M".to_string(),
                sub: Some("this season".to_string()),
                value_color: None,
                headline: true,
            },
            Stat::unknown("season score", "not in the log"),
        ];
        let mut ui = simulator(stat_cards::<M>(
            &cards,
            theme::accent(Some(wowdps_model::Class::Mage), None),
            Density::Comfortable,
        ));
        assert!(ui.find("1.2M").is_ok());
        assert!(ui.find("—").is_ok());
        assert!(ui.find("not in the log").is_ok());
        // The gradient path only actually runs under the renderer.
        let _ = ui.snapshot(&Theme::TokyoNight).unwrap();
    }

    #[test]
    fn the_shortcut_sheet_lists_every_binding() {
        let mut ui = simulator(shortcut_sheet(theme::NEUTRAL, M::Dismiss));
        for b in keys::BINDINGS {
            assert!(ui.find(b.keys).is_ok(), "{b:?} is not on the sheet");
            assert!(ui.find(b.what).is_ok(), "{b:?} has no description");
        }
        let _ = ui.snapshot(&Theme::TokyoNight).unwrap();
    }

    #[test]
    fn the_sheet_dismisses_on_any_press() {
        let mut ui = simulator(shortcut_sheet(theme::NEUTRAL, M::Dismiss));
        ui.click("keys").unwrap();
        assert_eq!(ui.into_messages().collect::<Vec<_>>(), vec![M::Dismiss]);
    }

    #[test]
    fn the_chrome_pieces_render() {
        let accent = theme::accent(Some(wowdps_model::Class::Priest), None);
        let _ = render(two_tone_title::<M>(
            "Tranqster".to_string(),
            "Healing".to_string(),
            Some(("KILL".to_string(), theme::GREEN)),
            accent,
            size::TITLE,
        ));
        let _ = render(chip_row(
            vec![("keys".to_string(), M::Dismiss)],
            Some(0),
            accent,
        ));
        let _ = render(panel(
            "recent",
            Some("last 24 h".to_string()),
            text("nothing yet").size(size::MICRO),
            Some(("all fights".to_string(), M::Dismiss)),
            accent,
        ));
        let mut ui = simulator(filter_box("durgan", |_| M::Dismiss, M::Dismiss, M::Dismiss));
        assert!(ui.find("durgan").is_ok());
        let _ = ui.snapshot(&Theme::TokyoNight).unwrap();
    }
}
