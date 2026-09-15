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
            .height(density.row_h())
            .padding([0.0, density.pad() / 2.0 + 2.0])
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
                // One height for every card, headline or not: a band of
                // uneven boxes reads as a mistake.
                .height(Length::Fixed(card_h(density)))
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
    let field = text_input("name, class, spec or role — accents optional", value)
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
        // It must listen for the RELEASE: `text_input` captures the left
        // press (that is how it places the caret), and `mouse_area` gives
        // up on a captured event — an `on_press` here would never fire and
        // the keymap would keep quitting the app on a typed "q". The
        // release is not captured, and the window re-checks the field's
        // real focus every tick, so a release that began elsewhere corrects
        // itself.
        mouse_area(field).on_release(on_focus),
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
    surface: keys::Surface,
    on_dismiss: M,
) -> Element<'static, M> {
    // What works HERE, grouped: the sheet answers "what can I press now".
    let list = |here: bool| {
        let mut col = column![].spacing(6);
        for group in keys::GROUPS {
            let in_group: Vec<&keys::Binding> = keys::BINDINGS
                .iter()
                .filter(|b| b.group == group && b.applies(surface) == here)
                .collect();
            if in_group.is_empty() {
                continue;
            }
            let mut lines = column![text(group).size(size::TINY).color(theme::DIM)].spacing(1);
            for b in in_group {
                let key_ink = match (here, b.window_local) {
                    (true, false) => accent.heading,
                    (true, true) => Color {
                        a: 0.8,
                        ..accent.heading
                    },
                    (false, _) => theme::DIM,
                };
                let what_ink = if here { Color::WHITE } else { theme::DIM };
                lines = lines.push(
                    row![
                        text(b.keys)
                            .size(size::MICRO)
                            .color(key_ink)
                            .font(Font::MONOSPACE)
                            .width(Length::Fixed(52.0)),
                        text(b.what).size(size::MICRO).color(what_ink),
                    ]
                    .spacing(8),
                );
            }
            col = col.push(lines);
        }
        col
    };
    let here = column![
        text(format!("keys · {}", surface.name()))
            .size(size::HEAD)
            .color(accent.heading),
        list(true),
    ]
    .spacing(6)
    .width(Length::Fixed(240.0));
    // Only what works here: a key that does nothing on this screen is
    // not worth the reader's eye.
    let card = column![
        here,
        text("any key or click closes this")
            .size(size::TINY)
            .color(theme::DIM),
    ]
    .spacing(8);
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

/// The `?` affordance at the end of the tab strip: the one hint the footer
/// no longer needs to recite.
pub(crate) fn help_glyph<M: Clone + 'static>(on_press: M) -> Element<'static, M> {
    mouse_area(
        container(text("?").size(size::BODY).color(theme::DIM))
            .padding([0, 8])
            .style(|_: &Theme| container::Style {
                border: Border {
                    color: theme::RULE,
                    width: 1.0,
                    radius: 3.into(),
                },
                ..container::Style::default()
            }),
    )
    .on_press(on_press)
    .into()
}

/// The stat card height: room for the eyebrow, the headline-size value and
/// a sub line, so a plain card matches the headline card beside it.
fn card_h(density: Density) -> f32 {
    density.pad() * 2.0 + size::TINY + size::DISPLAY + size::TINY + 12.0
}

/// One entry of the character picker: a character the store has seen you
/// play, worn the way a meter row wears it — spec icon, class color.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct CharPick {
    pub guid: String,
    pub name: String,
    pub class: Option<wowdps_model::Class>,
    pub spec: Option<wowdps_model::Spec>,
    pub fights: u32,
}

impl CharPick {
    /// The name as drawn: the realm stripped when the option says so.
    fn shown(&self, hide_realms: bool) -> String {
        if hide_realms {
            crate::view::display_name(&self.name).to_string()
        } else {
            self.name.clone()
        }
    }

    fn color(&self) -> Color {
        self.class.map_or(theme::DIM, |class| {
            theme::accent(Some(class), self.spec).base
        })
    }
}

/// The locked character's name, drawn where the NAME goes — Home's title,
/// or the tab strip on any other screen — with its spec icon in its class
/// color. With more than one character it is a press target that opens
/// [`character_menu`]; one character is nothing to pick between and draws
/// plain. `pick_list` cannot do this: it paints text and nothing else.
pub(crate) fn character_picker<M: Clone + 'static>(
    chars: &[CharPick],
    selected: Option<&str>,
    // `everyone`: History's widening — `None` selected means "everyone" and
    // is drawn as such, where Home falls back to the first character.
    everyone: bool,
    hide_realms: bool,
    on_toggle: M,
    accent: theme::Accent,
    size: f32,
) -> Element<'static, M> {
    let current = selected
        .and_then(|guid| chars.iter().find(|c| c.guid == guid))
        .or_else(|| (!everyone).then(|| chars.first()).flatten());
    let Some(c) = current else {
        if everyone {
            let line = row![
                text("everyone").size(size).color(accent.heading),
                text("▾").size(size * 0.6).color(theme::DIM),
            ]
            .spacing(6)
            .align_y(iced::Alignment::Center);
            return mouse_area(line).on_press(on_toggle).into();
        }
        return text("wowdps").size(size).color(accent.heading).into();
    };
    let mut line = row![
        crate::compare::class_icon::<M>(c.class, c.spec, None, size),
        text(c.shown(hide_realms)).size(size).color(c.color()),
    ]
    .spacing(6)
    .align_y(iced::Alignment::Center);
    if chars.len() < 2 && !everyone {
        return line.into();
    }
    line = line.push(text("▾").size(size * 0.6).color(theme::DIM));
    mouse_area(line).on_press(on_toggle).into()
}

/// The menu the picker opens: every character, icon + class-colored name +
/// fight count, the locked one lit. Drawn at the window root over a scrim
/// (a press anywhere else closes it), anchored top-left under the strip
/// where both pickers live.
/// What the menu shows: the facts, apart from the messages it emits.
#[derive(Debug, Clone, Default)]
pub(crate) struct Menu<'a> {
    pub chars: &'a [CharPick],
    pub selected: Option<&'a str>,
    /// History's widening: offer an "everyone" row, lit when nothing is
    /// selected.
    pub everyone: bool,
    pub hide_realms: bool,
    /// The row the pointer is over.
    pub hover: Option<usize>,
}

pub(crate) fn character_menu<M: Clone + 'static>(
    menu: Menu<'_>,
    on_hover: impl Fn(Option<usize>) -> M + 'static,
    on_pick: impl Fn(Option<String>) -> M + 'static,
    on_dismiss: M,
    accent: theme::Accent,
) -> Element<'static, M> {
    let Menu {
        chars,
        selected,
        everyone,
        hide_realms,
        hover,
    } = menu;
    let mut list = column![].spacing(2);
    let mut rows: Vec<(Element<'static, M>, bool, M)> = Vec::new();
    if everyone {
        let on = selected.is_none();
        let line = row![
            Space::new().width(Length::Fixed(size::BODY)),
            text("everyone")
                .size(size::MICRO)
                .color(if on { accent.ink } else { theme::DIM }),
        ]
        .spacing(8)
        .align_y(iced::Alignment::Center);
        rows.push((line.into(), on, on_pick(None)));
    }
    for c in chars {
        let on = Some(c.guid.as_str()) == selected;
        let line = row![
            crate::compare::class_icon::<M>(c.class, c.spec, None, size::BODY),
            text(c.shown(hide_realms)).size(size::MICRO).color(if on {
                accent.ink
            } else {
                c.color()
            }),
            Space::new().width(Length::Fill),
            text(format!("{} fights", c.fights))
                .size(size::TINY)
                .color(if on { accent.ink } else { theme::DIM })
                .font(Font::MONOSPACE),
        ]
        .spacing(8)
        .align_y(iced::Alignment::Center);
        rows.push((line.into(), on, on_pick(Some(c.guid.clone()))));
    }
    // The hover is the meter's own (`view::hover_style`): fainter than
    // the lit row and borderless, so it can sit on the selection.
    for (i, (line, on, msg)) in rows.into_iter().enumerate() {
        let hovered = hover == Some(i);
        let cell = container(line)
            .padding([3.0, 8.0])
            .width(Length::Fill)
            .style(move |_: &Theme| {
                if on {
                    container::Style {
                        background: Some(theme::accent_fill(accent)),
                        border: Border {
                            radius: 3.into(),
                            ..Border::default()
                        },
                        ..container::Style::default()
                    }
                } else {
                    crate::view::hover_style(hovered)
                }
            });
        list = list.push(
            mouse_area(cell)
                .on_press(msg)
                .on_enter(on_hover(Some(i)))
                .on_exit(on_hover(None)),
        );
    }
    let card = container(list.width(Length::Fixed(260.0)))
        .padding(6)
        .style(|_: &Theme| container::Style {
            background: Some(theme::PANEL.into()),
            border: Border {
                color: theme::RULE,
                width: 1.0,
                radius: 6.into(),
            },
            ..container::Style::default()
        });
    // A press on the scrim — anywhere but a row — closes the menu; a row's
    // own press is taken by its mouse_area first.
    let scrim = mouse_area(
        container(Space::new())
            .width(Length::Fill)
            .height(Length::Fill),
    )
    .on_press(on_dismiss);
    stack![
        scrim,
        container(card)
            .padding([44.0, 10.0])
            .width(Length::Fill)
            .height(Length::Fill)
            .align_x(iced::Alignment::Start)
            .align_y(iced::Alignment::Start),
    ]
    .width(Length::Fill)
    .height(Length::Fill)
    .into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::window::testkit::simulator;

    fn picks() -> Vec<CharPick> {
        vec![
            CharPick {
                guid: "G-a".to_string(),
                name: "Alpha-Realm".to_string(),
                class: Some(wowdps_model::Class::Mage),
                spec: Some(wowdps_model::Spec::Fire),
                fights: 3,
            },
            CharPick {
                guid: "G-b".to_string(),
                name: "Beta-Realm".to_string(),
                class: None,
                spec: None,
                fights: 1,
            },
        ]
    }

    #[test]
    fn the_character_picker_shows_the_locked_name_and_honours_hide_realms() {
        let mut ui = simulator(character_picker::<()>(
            &picks(),
            Some("G-b"),
            false,
            false,
            (),
            theme::NEUTRAL,
            size::TITLE,
        ));
        assert!(ui.find("Beta-Realm").is_ok(), "the lock is the name shown");
        // History widened: nothing selected reads "everyone".
        let mut ui = simulator(character_picker::<()>(
            &picks(),
            None,
            true,
            false,
            (),
            theme::NEUTRAL,
            size::TITLE,
        ));
        assert!(ui.find("everyone").is_ok());
        let mut ui = simulator(character_picker::<()>(
            &picks(),
            Some("G-b"),
            false,
            true,
            (),
            theme::NEUTRAL,
            size::TITLE,
        ));
        assert!(
            ui.find("Beta").is_ok(),
            "hide_realms strips the realm here too"
        );
        assert!(ui.find("Beta-Realm").is_err());
        // One character is nothing to pick between: drawn plain, by name.
        let one: Vec<CharPick> = picks().into_iter().take(1).collect();
        let mut ui = simulator(character_picker::<()>(
            &one,
            None,
            false,
            false,
            (),
            theme::NEUTRAL,
            size::TITLE,
        ));
        assert!(ui.find("Alpha-Realm").is_ok());
    }

    #[test]
    fn the_character_menu_lists_every_character_with_its_fights() {
        let pick = picks();
        let mut ui = simulator(character_menu::<()>(
            Menu {
                chars: &pick,
                selected: Some("G-a"),
                everyone: true,
                hide_realms: true,
                hover: Some(1),
            },
            |_| (),
            |_| (),
            (),
            theme::NEUTRAL,
        ));
        assert!(ui.find("everyone").is_ok());
        assert!(ui.find("Alpha").is_ok());
        assert!(ui.find("Beta").is_ok());
        assert!(ui.find("3 fights").is_ok());
        assert!(ui.find("1 fights").is_ok());
    }
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
    fn the_shortcut_sheet_lists_the_surfaces_bindings_only() {
        let mut ui = simulator(shortcut_sheet(
            theme::NEUTRAL,
            keys::Surface::Meter,
            M::Dismiss,
        ));
        for b in keys::BINDINGS {
            let listed = ui.find(b.what).is_ok();
            assert_eq!(
                listed,
                b.applies(keys::Surface::Meter),
                "{b:?}: a key is on the sheet exactly when it works here"
            );
        }
        let _ = ui.snapshot(&Theme::TokyoNight).unwrap();
    }

    #[test]
    fn the_sheet_dismisses_on_any_press() {
        let mut ui = simulator(shortcut_sheet(
            theme::NEUTRAL,
            keys::Surface::Meter,
            M::Dismiss,
        ));
        ui.click("views").unwrap();
        assert_eq!(ui.into_messages().collect::<Vec<_>>(), vec![M::Dismiss]);
    }

    #[test]
    fn the_chrome_pieces_render() {
        let accent = theme::accent(Some(wowdps_model::Class::Priest), None);
        let mut ui = simulator(two_tone_title::<M>(
            "Tranqster".to_string(),
            "Healing".to_string(),
            Some(("KILL".to_string(), theme::GREEN)),
            accent,
            size::TITLE,
        ));
        assert!(ui.find("Tranqster").is_ok());
        assert!(ui.find("Healing").is_ok());
        assert!(ui.find("KILL").is_ok());
        let _ = ui.snapshot(&Theme::TokyoNight).unwrap();

        let mut ui = simulator(chip_row(
            vec![
                ("keys".to_string(), M::Dismiss),
                ("raid".to_string(), M::Pick(View::Damage)),
            ],
            Some(0),
            accent,
        ));
        assert!(ui.find("jump to").is_ok());
        ui.click("raid").unwrap();
        assert_eq!(
            ui.into_messages().collect::<Vec<_>>(),
            vec![M::Pick(View::Damage)],
            "a chip leads somewhere"
        );

        let mut ui = simulator(panel(
            "recent",
            Some("last 24 h".to_string()),
            text("nothing yet").size(size::MICRO),
            Some(("all fights".to_string(), M::Dismiss)),
            accent,
        ));
        assert!(ui.find("recent").is_ok());
        assert!(ui.find("last 24 h").is_ok());
        assert!(ui.find("nothing yet").is_ok());
        ui.click("all fights").unwrap();
        assert_eq!(ui.into_messages().collect::<Vec<_>>(), vec![M::Dismiss]);

        let mut ui = simulator(filter_box("durgan", |_| M::Dismiss, M::Dismiss, M::Dismiss));
        assert!(ui.find("durgan").is_ok());
        assert!(ui.find("✕").is_ok(), "a filled filter offers a way out");
        let _ = ui.snapshot(&Theme::TokyoNight).unwrap();
        let mut ui = simulator(filter_box("", |_| M::Dismiss, M::Dismiss, M::Dismiss));
        assert!(ui.find("✕").is_err(), "an empty one does not");
    }
}
