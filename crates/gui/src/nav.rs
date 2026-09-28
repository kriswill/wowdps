//! The shell around whatever a screen is showing: the tab bar, the jump
//! chips, the two-tone title, stat cards, panels, the row filter and the `?`
//! sheet.
//!
//! Everything here is message-generic and `'static`, taking the message to
//! emit as an argument rather than naming `window::Message`, so the overlay
//! can adopt any of it later without a message-type fight. Nothing in this
//! module reads state; callers hand it what to draw.

use iced::widget::{Space, button, column, container, mouse_area, row, stack, text, text_input};
use iced::{Border, Color, Element, Length, Theme, mouse};

use crate::keys;
use crate::line_icons::{LineIcon, line_icon, line_icon_lit};
use crate::theme::{self, Density, pitch, size};

/// One entry of the tab bar. It recites no key: the `?` sheet lists every
/// binding (`keys::BINDINGS`), and the prototype's bars carry none.
#[derive(Debug, Clone)]
pub(crate) struct Tab<M> {
    /// What leads the label: a view's line icon, or nothing.
    pub lead: Lead,
    pub label: &'static str,
    pub active: bool,
    /// `None` renders the tab disabled: dim and inert. A tab that leads
    /// nowhere yet must LOOK like it leads nowhere, never silently no-op.
    pub on_press: Option<M>,
    /// Said under the pointer: why a disabled tab leads nowhere.
    pub tip: Option<&'static str>,
}

/// What a tab's label leads with.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum Lead {
    None,
    /// A line icon, drawn in the tab's own ink.
    Icon(LineIcon),
}

/// Which strip a tab bar is: the top bar's places (`.place`: 15 px at 500,
/// 42 tall) or a fight's views (`.vtab`: 14.5 px at 500, 37 tall, a line
/// under the whole strip).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Strip {
    Places,
    Views,
}

impl Strip {
    fn height(self) -> f32 {
        match self {
            Strip::Places => pitch::PLACE,
            Strip::Views => pitch::TAB,
        }
    }

    fn text(self) -> f32 {
        match self {
            Strip::Places => size::PLACE,
            Strip::Views => size::TAB,
        }
    }

    fn pad(self) -> f32 {
        match self {
            Strip::Places => 11.0,
            Strip::Views => 9.0,
        }
    }
}

/// The accent a tab's underline is drawn in: the chrome's, on the active
/// tab only — the rule [`tab_strip`] draws by, held by its own test.
pub(crate) fn tab_underline(active: bool, accent: theme::Accent) -> Option<Color> {
    active.then_some(accent.base)
}

/// A tab's ink: parchment when active, and on the pointer (`.vtab:hover`);
/// secondary otherwise; a disabled tab sits back in the faintest ink.
fn tab_ink(active: bool, status: button::Status) -> Color {
    match status {
        _ if active => theme::INK,
        button::Status::Hovered | button::Status::Pressed => theme::INK,
        button::Status::Active => theme::INK_2,
        button::Status::Disabled => theme::INK_3,
    }
}

/// The tab bar: ink text, and a 2 px accent underline under the active tab
/// — no filled pill; the rest sit back in secondary ink and brighten under
/// the pointer, which wears the hand. A view strip is seen through
/// [`crate::reveal`], so a window too narrow for the strip shows part of it
/// with the ACTIVE tab whole in sight, and a wheel moves it along — no
/// scrollbar, as the prototype's has none (`scrollbar-width: none`); the
/// top bar's two places are as wide as they are. The top bar's places; a
/// view strip is [`view_strip`].
pub(crate) fn tab_bar<M: Clone + 'static>(
    tabs: Vec<Tab<M>>,
    accent: theme::Accent,
    strip_kind: Strip,
) -> Element<'static, M> {
    let strip = tab_strip(tabs, accent, strip_kind);
    match strip_kind {
        Strip::Places => strip,
        Strip::Views => column![strip, hairline()].into(),
    }
}

/// A fight's views as a strip (`.vtabs`): [`tab_bar`] over the hairline
/// the underline rests on — the hairline the whole width, the strip's row
/// inside `inset` (the meter's `.vtabs{padding:0 12px 0 10px}`, where the
/// fight runs edge to edge; nothing inside a framed screen). `trailing`
/// sits at the row's far end (`.vtabs .filter`: the meter's filter), over
/// the same hairline — the tabs move along in what is left of the row, the
/// active one always whole, and the trailing piece never moves.
pub(crate) fn view_strip<M: Clone + 'static>(
    tabs: Vec<Tab<M>>,
    accent: theme::Accent,
    trailing: Option<Element<'static, M>>,
    inset: iced::Padding,
) -> Element<'static, M> {
    let strip = tab_strip(tabs, accent, Strip::Views);
    let line: Element<'static, M> = match trailing {
        Some(trailing) => row![container(strip).width(Length::Fill), trailing]
            .spacing(8)
            .height(Length::Fixed(pitch::TAB))
            .align_y(iced::Alignment::Center)
            .into(),
        None => strip,
    };
    column![container(line).padding(inset), hairline()].into()
}

fn tab_strip<M: Clone + 'static>(
    tabs: Vec<Tab<M>>,
    accent: theme::Accent,
    strip_kind: Strip,
) -> Element<'static, M> {
    let active_at = tabs.iter().position(|t| t.active);
    let mut strip = row![].spacing(if strip_kind == Strip::Places { 2 } else { 0 });
    for t in tabs {
        let active = t.active;
        let mut label = row![].spacing(6).align_y(iced::Alignment::Center);
        match t.lead {
            Lead::None => {}
            Lead::Icon(icon) => {
                let ink = if active {
                    theme::INK
                } else if t.on_press.is_some() {
                    theme::INK_2
                } else {
                    theme::INK_3
                };
                label = label.push(line_icon(
                    icon,
                    size::TAB_ICON,
                    Color {
                        a: theme::TAB_ICON_ALPHA,
                        ..ink
                    },
                ));
            }
        }
        // No colour of its own: the label takes the button's, which is
        // what lets the pointer brighten it.
        label = label.push(
            text(t.label)
                .size(strip_kind.text())
                .font(theme::UI_MEDIUM)
                .wrapping(text::Wrapping::None),
        );
        let lit = tab_underline(active, accent);
        let underline = container(Space::new())
            .width(Length::Fill)
            .height(Length::Fixed(2.0))
            .style(move |_: &Theme| container::Style {
                background: lit.map(Into::into),
                ..container::Style::default()
            });
        let cell = column![
            container(label)
                .height(Length::Fill)
                .align_y(iced::Alignment::Center)
                .padding([0.0, strip_kind.pad()]),
            underline,
        ]
        .width(Length::Shrink)
        .height(Length::Fixed(strip_kind.height()));
        let tab =
            button(cell)
                .padding(0)
                .on_press_maybe(t.on_press)
                .style(move |_: &Theme, status| button::Style {
                    text_color: tab_ink(active, status),
                    ..button::Style::default()
                });
        strip = strip.push(match t.tip {
            Some(words) => tip(tab, words),
            None => tab.into(),
        });
    }
    // The top bar's two places always fit: they are the row itself, as wide
    // as they are (`.place` sits in the bar's flex row), so the jump box
    // after them is the bar's one stretch of free room and centres in it.
    // A window onto them would take the whole row's width (`reveal` fills
    // what it is given) and halve that room.
    if strip_kind == Strip::Places {
        return strip.into();
    }
    // The row's children are the tabs, in order: the window onto it keeps
    // the active one whole, however narrow the row it is given. A view
    // strip's hairline is drawn by its caller, under whatever shares the
    // strip's row ([`view_strip`]). An edge with more of the strip past it
    // fades into what the strip sits on, the stage's ground, so a tab cut
    // there reads as going on, not as a stray glyph.
    crate::reveal::reveal(strip, active_at)
        .fade(theme::GROUND)
        .into()
}

/// A tooltip's frame (`.tip{padding:5px 8px;border-radius:6px}`) and how
/// far under what it names it floats.
const TIP_PAD: [u16; 2] = [5, 8];
const TIP_RADIUS: f32 = 6.0;
const TIP_GAP: f32 = 4.0;

/// `content` with `words` under it while the pointer is on it (`.tip`):
/// 13 px ink on the floating surface.
pub(crate) fn tip<'a, M: 'a>(
    content: impl Into<Element<'a, M>>,
    words: impl text::IntoFragment<'a>,
) -> Element<'a, M> {
    iced::widget::tooltip(
        content,
        container(text(words).size(size::MICRO).color(theme::INK))
            .padding(TIP_PAD)
            .style(|_: &Theme| floating_style(TIP_RADIUS)),
        iced::widget::tooltip::Position::Bottom,
    )
    .gap(TIP_GAP)
    .into()
}

/// A full-width 1 px LINE rule.
pub(crate) fn hairline<M: 'static>() -> Element<'static, M> {
    container(Space::new())
        .width(Length::Fill)
        .height(Length::Fixed(1.0))
        .style(|_: &Theme| container::Style {
            background: Some(theme::LINE.into()),
            ..container::Style::default()
        })
        .into()
}

/// A full-height 1 px LINE rule: what stands between the meter and the
/// inspector (`.meter{border-right:1px solid var(--line)}`) and between a
/// comparison's two lists (`.cmp2>div+div{border-left:1px solid …}`).
pub(crate) fn vrule<M: 'static>() -> Element<'static, M> {
    container(Space::new())
        .width(Length::Fixed(1.0))
        .height(Length::Fill)
        .style(|_: &Theme| container::Style {
            background: Some(theme::LINE.into()),
            ..container::Style::default()
        })
        .into()
}

/// A filled circle `d` pixels across — the live dot.
pub(crate) fn dot<M: 'static>(color: Color, d: f32) -> Element<'static, M> {
    container(Space::new())
        .width(Length::Fixed(d))
        .height(Length::Fixed(d))
        .style(move |_: &Theme| container::Style {
            background: Some(color.into()),
            border: iced::border::rounded(d / 2.0),
            ..container::Style::default()
        })
        .into()
}

/// A hollow circle `d` pixels across: the dot's idle shape.
pub(crate) fn ring<M: 'static>(color: Color, d: f32) -> Element<'static, M> {
    container(Space::new())
        .width(Length::Fixed(d))
        .height(Length::Fixed(d))
        .style(move |_: &Theme| container::Style {
            border: Border {
                color,
                width: 1.5,
                radius: (d / 2.0).into(),
            },
            ..container::Style::default()
        })
        .into()
}

/// An outcome, worded: Kill and Timed in green, Wipe and Over in red, a
/// live pull as a red dot beside its word — and, for a wipe, how close it
/// came in secondary ink beside it. Never yellow.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Badge {
    /// The outcome's word as its source spells it ("KILL", "Over +0:26"):
    /// [`badge`] draws it in sentence case whatever the source's case.
    pub word: String,
    pub color: Color,
    /// Lead with the live dot.
    pub live: bool,
    /// What follows the word in secondary ink: a wipe's best %.
    pub detail: Option<String>,
}

impl Badge {
    pub(crate) fn new(word: impl Into<String>, color: Color) -> Self {
        Self {
            word: word.into(),
            color,
            live: false,
            detail: None,
        }
    }

    /// Live: the red dot and the word, in the bad-news red the prototype
    /// gives a pull still in progress.
    pub(crate) fn live() -> Self {
        Self {
            live: true,
            ..Self::new("Live", theme::BAD)
        }
    }
}

/// `s` in sentence case: its first letter capital, every other letter
/// lower — "KILL" → "Kill", "OVER +0:26" → "Over +0:26", "crowd control"
/// → "Crowd control". The window's one case (the prototype's).
pub(crate) fn sentence(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        Some(first) => first
            .to_uppercase()
            .chain(chars.flat_map(char::to_lowercase))
            .collect(),
        None => String::new(),
    }
}

/// "1 fight", "3 fights": a count and its noun, agreeing.
pub(crate) fn plural(n: usize, noun: &str) -> String {
    if n == 1 {
        format!("{n} {noun}")
    } else {
        format!("{n} {noun}s")
    }
}

/// A badge's frame around its word (`.badge{padding:1px 7px}`), the live
/// dot's diameter, and the gap after the dot and before the detail.
const BADGE_PAD_X: f32 = 7.0;
const BADGE_PAD_Y: f32 = 1.0;
const BADGE_DOT: f32 = 8.0;
const BADGE_GAP: f32 = 6.0;

/// What a badge takes on its line beyond its words — the word and the
/// detail, whose widths only the renderer knows: the frame's insets, the
/// live dot and its gap, and the gap before the detail. What a line that
/// must leave room for a badge (`fight_head`'s title) adds to their
/// measure.
pub(crate) fn badge_extra(b: &Badge) -> f32 {
    2.0 * BADGE_PAD_X
        + if b.live { BADGE_DOT + BADGE_GAP } else { 0.0 }
        + if b.detail.is_some() { BADGE_GAP } else { 0.0 }
}

/// A [`Badge`] (`.badge`: 600 at 13 px, padded 1 × 7 on a faint wash of
/// its own colour), its word in sentence case.
pub(crate) fn badge<M: 'static>(b: &Badge) -> Element<'static, M> {
    let color = b.color;
    let mut word = row![].spacing(BADGE_GAP).align_y(iced::Alignment::Center);
    if b.live {
        word = word.push(dot(color, BADGE_DOT));
    }
    word = word.push(
        text(sentence(&b.word))
            .size(size::MICRO)
            .color(color)
            .font(theme::UI_SEMIBOLD)
            .wrapping(text::Wrapping::None),
    );
    let mut line =
        row![
            container(word)
                .padding([BADGE_PAD_Y, BADGE_PAD_X])
                .style(move |_: &Theme| container::Style {
                    background: Some(Color { a: 0.12, ..color }.into()),
                    border: iced::border::rounded(4),
                    ..container::Style::default()
                })
        ]
        .spacing(BADGE_GAP)
        .align_y(iced::Alignment::Center);
    if let Some(detail) = &b.detail {
        line = line.push(
            text(detail.clone())
                .size(size::MICRO)
                .color(theme::INK_2)
                .wrapping(text::Wrapping::None),
        );
    }
    line.into()
}

/// The "jump to:" chip row over a long screen's sections. A chip is ink on
/// a hairline; the pressed one is bordered in the accent over a faint wash
/// of it, never filled.
pub(crate) fn chip_row<M: Clone + 'static>(
    sections: Vec<(String, M)>,
    active: Option<usize>,
    accent: theme::Accent,
) -> Element<'static, M> {
    let mut strip = row![text("Jump to").size(size::MICRO).color(theme::INK_2)].spacing(6);
    for (i, (label, msg)) in sections.into_iter().enumerate() {
        let on = active == Some(i);
        strip = strip.push(mouse_area(chip(label, on, accent)).on_press(msg));
    }
    strip.align_y(iced::Alignment::Center).into()
}

/// One chip: `label` in ink, pressed or not.
pub(crate) fn chip<M: 'static>(
    label: String,
    on: bool,
    accent: theme::Accent,
) -> iced::widget::Container<'static, M> {
    chip_around(
        text(label)
            .size(size::MICRO)
            .color(if on { theme::INK } else { theme::INK_2 })
            .wrapping(text::Wrapping::None),
        on,
        accent,
    )
}

/// The chip's frame around an element: ink on a hairline, the pressed one
/// bordered in the accent over a faint wash of it.
pub(crate) fn chip_around<'a, M: 'static>(
    content: impl Into<Element<'a, M>>,
    on: bool,
    accent: theme::Accent,
) -> iced::widget::Container<'a, M> {
    container(content)
        .padding([3, 10])
        .style(move |_: &Theme| container::Style {
            background: on.then(|| theme::accent_wash(accent).into()),
            border: Border {
                color: if on { accent.base } else { theme::LINE },
                width: 1.0,
                radius: 13.into(),
            },
            ..container::Style::default()
        })
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
    /// The one number the screen is about: drawn larger, on the same plain
    /// panel as the rest — no accent fill.
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

/// The summary band: plain panels, a gold-dim label, the value in ink —
/// full figures, commas as the caller formatted them. The headline card is
/// larger, never filled: a large accent fill shouted over the numbers.
/// Every line is ONE line, clipped at the card's edge: a card is a fixed
/// height, and a wrapped line would fall out of it unseen. The band is as
/// many rows as the width needs to give every card [`CARD_MIN`], balanced —
/// a narrow window draws four cards two and two rather than clip a figure
/// or leave one alone beside a hole.
pub(crate) fn stat_cards<M: 'static>(cards: &[Stat], density: Density) -> Element<'static, M> {
    let cards = cards.to_vec();
    iced::widget::responsive(move |bounds| stat_band(&cards, density, bounds.width))
        .height(Length::Shrink)
        .into()
}

/// The narrowest a stat card is drawn: room for a ten-digit figure with
/// its commas at `size::STAT`, and the card's padding.
pub(crate) const CARD_MIN: f32 = 130.0;

/// How many cards share a row at `width`.
pub(crate) fn cards_per_row(n: usize, width: f32, gap: f32) -> usize {
    let fit = ((width + gap) / (CARD_MIN + gap)).floor().max(1.0) as usize;
    let n = n.max(1);
    // As few rows as fit, then as even as they can be: four cards that fit
    // three to a row are drawn two and two, not three and one beside a hole.
    let rows = n.div_ceil(fit);
    n.div_ceil(rows)
}

fn stat_band<M: 'static>(cards: &[Stat], density: Density, width: f32) -> Element<'static, M> {
    let per_row = cards_per_row(cards.len(), width, density.gap());
    let mut band = column![].spacing(density.gap());
    for chunk in cards.chunks(per_row) {
        let mut line = stat_row(chunk, density);
        // A short last row keeps the cards above it company: the same
        // widths, the gap where a card would be.
        for _ in chunk.len()..per_row {
            line = line.push(Space::new().width(Length::FillPortion(1)));
        }
        band = band.push(line);
    }
    band.into()
}

fn stat_row<M: 'static>(cards: &[Stat], density: Density) -> iced::widget::Row<'static, M> {
    let mut strip = row![].spacing(density.gap());
    for c in cards {
        let headline = c.headline;
        let one_line = |t: iced::widget::Text<'static>| {
            container(t.wrapping(text::Wrapping::None))
                .clip(true)
                .width(Length::Fill)
        };
        let mut body = column![
            one_line(
                // Every stat label in the window's one case, whoever made it.
                text(sentence(&c.label))
                    .size(size::LABEL)
                    .color(theme::GOLD_DIM)
            ),
            one_line(
                text(c.value.clone())
                    .size(if headline { size::DISPLAY } else { size::STAT })
                    .color(c.value_color.unwrap_or(theme::INK))
                    .font(theme::UI_MEDIUM)
            ),
        ]
        .spacing(1);
        if let Some(sub) = c.sub.clone() {
            body = body.push(one_line(text(sub).size(size::MICRO).color(theme::INK_2)));
        }
        strip = strip.push(
            container(body)
                .padding(density.pad())
                .width(Length::FillPortion(1))
                // One height for every card, headline or not: a band of
                // uneven boxes reads as a mistake.
                .height(Length::Fixed(card_h(density)))
                .clip(true)
                .style(|_: &Theme| surface_style(6.0)),
        );
    }
    strip
}

/// A panel's own look: the surface a shade above the ground, a hairline
/// around it.
pub(crate) fn surface_style(radius: f32) -> container::Style {
    container::Style {
        background: Some(theme::SURFACE.into()),
        border: Border {
            color: theme::LINE,
            width: 1.0,
            radius: radius.into(),
        },
        ..container::Style::default()
    }
}

/// A floating surface — a menu, the options card: the panel's fill with
/// the brighter edge the prototype gives what sits over the window, and
/// the shadow that lifts it off whatever it covers (`.menu`).
pub(crate) fn floating_style(radius: f32) -> container::Style {
    container::Style {
        border: Border {
            color: theme::EDGE,
            width: 1.0,
            radius: radius.into(),
        },
        shadow: theme::SHADOW_MENU,
        ..surface_style(radius)
    }
}

/// The modal sheet's surface: [`floating_style`] cast deeper (`.sheet`).
pub(crate) fn sheet_style(radius: f32) -> container::Style {
    container::Style {
        shadow: theme::SHADOW_SHEET,
        ..floating_style(radius)
    }
}

/// A titled panel: its heading in ink, a right-aligned caption, the body,
/// and an optional footer line that leads somewhere (in gold, a link).
pub(crate) fn panel<'a, M: Clone + 'static>(
    title: &str,
    caption: Option<String>,
    body: impl Into<Element<'a, M>>,
    footer: Option<(String, M)>,
) -> Element<'a, M> {
    let mut head = row![
        text(title.to_string())
            .size(size::NAME)
            .color(theme::INK)
            .font(theme::UI_SEMIBOLD)
    ]
    .spacing(8)
    .align_y(iced::Alignment::Center);
    head = head.push(Space::new().width(Length::Fill));
    if let Some(caption) = caption {
        head = head.push(text(caption).size(size::MICRO).color(theme::INK_2));
    }
    let mut col = column![head, body.into()].spacing(4);
    if let Some((label, msg)) = footer {
        col = col.push(mouse_area(text(label).size(size::MICRO).color(theme::GOLD)).on_press(msg));
    }
    container(col)
        .padding(8)
        .width(Length::Fill)
        .style(|_: &Theme| surface_style(8.0))
        .into()
}

/// The filter field's id, so `update` can focus it from the `/` key.
pub(crate) fn filter_id() -> iced::widget::Id {
    iced::widget::Id::new("row-filter")
}

/// The filter's clear mark, so a test can find its target.
pub(crate) fn filter_clear_id() -> iced::widget::Id {
    iced::widget::Id::new("row-filter-clear")
}

/// The filter's text width at rest and with focus (`.filter input`: 92 px,
/// 140 px on `:focus-within`).
pub(crate) const FILTER_W: f32 = 92.0;
pub(crate) const FILTER_W_FOCUSED: f32 = 140.0;
/// The filter box's height (`.filter{height:28px}`): its text's line and
/// the inset above and below it.
pub(crate) const FILTER_H: f32 = 28.0;
const FILTER_LINE: f32 = 18.0;
/// Ahead of the text: the field's inset, the search glyph and the gap.
const FILTER_LEAD: f32 = 8.0 + size::ICON + 6.0;
/// After the text: the gap, the `/` keycap and the inset.
const FILTER_TRAIL: f32 = 6.0 + 18.0 + 8.0;
/// The clear mark's target while there is text to clear: a 24 px square
/// (WCAG 2.5.8's minimum), its 12 px glyph centred, its own air the gap
/// either side — so the text keeps its width beside it.
const FILTER_CLEAR: f32 = 24.0;
/// The clear mark's glyph (`x`).
const FILTER_CLEAR_GLYPH: f32 = 12.0;

/// The row filter (`.filter`): a search glyph, the field and its `/`
/// keycap, compact at the end of the view tabs — `placeholder` ("Filter
/// players", "Filter enemies") in faint ink, no frame until the pointer
/// finds it — and wider with focus, the ground filling it inside the
/// floating edge. Typing in it must not reach the meter keymap — the
/// caller owns that (`window.rs` swallows keys while it has focus);
/// `focused` is the window's word for it, which the width and the frame
/// follow.
///
/// The whole box IS the text field: the glyph, the keycap and the clear
/// mark are drawn over its insets on an inert layer, so a press anywhere
/// in the box lands on the field, and the field's own hover and focus
/// frame the box.
pub(crate) fn filter_box<M: Clone + 'static>(
    value: &str,
    focused: bool,
    placeholder: &str,
    on_input: impl Fn(String) -> M + 'static,
    on_clear: M,
    on_focus: M,
    on_done: M,
) -> Element<'static, M> {
    let clearing = !value.is_empty();
    let text_w = if focused { FILTER_W_FOCUSED } else { FILTER_W };
    let trail = FILTER_TRAIL + if clearing { FILTER_CLEAR } else { 0.0 };
    let inset = (FILTER_H - FILTER_LINE) / 2.0;
    let field = text_input(placeholder, value)
        .id(filter_id())
        .on_input(on_input)
        // Enter keeps the text and gives the keys back: the field says so
        // itself, so the window can drop iced's focus with its own flag.
        .on_submit(on_done)
        .size(size::FILTER)
        .line_height(text::LineHeight::Absolute(FILTER_LINE.into()))
        .padding(iced::Padding {
            top: inset,
            right: trail,
            bottom: inset,
            left: FILTER_LEAD,
        })
        .width(Length::Fixed(FILTER_LEAD + text_w + trail))
        .style(move |theme: &Theme, status| filter_style(theme, status, focused));
    let mut trail = row![].align_y(iced::Alignment::Center);
    if clearing {
        // Flush against the keycap: the target's own air is the gap.
        trail = trail.push(
            mouse_area(
                container(line_icon(LineIcon::Close, FILTER_CLEAR_GLYPH, theme::INK_2))
                    .id(filter_clear_id())
                    .center(Length::Fixed(FILTER_CLEAR)),
            )
            .on_press(on_clear)
            .interaction(mouse::Interaction::Pointer),
        );
    }
    let marks = row![
        line_icon(LineIcon::Search, size::ICON, theme::INK_3),
        Space::new().width(Length::Fill),
        trail.push(kbd::<M>("/")),
    ]
    .spacing(6)
    .align_y(iced::Alignment::Center);
    stack![
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
        // The marks answer no pointer (bar the clear mark's press), so the
        // stack hands every other event to the field under them.
        container(marks)
            .padding([0, 8])
            .width(Length::Fill)
            .height(Length::Fill)
            .align_y(iced::Alignment::Center),
    ]
    .into()
}

/// The filter field in the tokens (`.filter`): no frame until the pointer
/// finds it (a LINE edge); with focus — iced's, or the window's word for
/// it (`focused`) — the ground fills it inside the floating EDGE
/// (`.filter:focus-within`), no gold: the caret says where the keys go.
/// The placeholder is a hint in faint ink, the value parchment, the
/// selection a wash of gold.
pub(crate) fn filter_style(
    _: &Theme,
    status: text_input::Status,
    focused: bool,
) -> text_input::Style {
    let focused = focused || matches!(status, text_input::Status::Focused { .. });
    let edge = match status {
        _ if focused => theme::EDGE,
        text_input::Status::Hovered => theme::LINE,
        _ => Color::TRANSPARENT,
    };
    text_input::Style {
        background: if focused {
            theme::GROUND.into()
        } else {
            Color::TRANSPARENT.into()
        },
        border: Border {
            color: edge,
            width: 1.0,
            radius: 6.into(),
        },
        icon: theme::INK_3,
        // Words a reader reads: the faint ink's text grade, AA where
        // INK_3 is not.
        placeholder: theme::INK_3_TEXT,
        value: theme::INK,
        selection: Color {
            a: 0.3,
            ..theme::GOLD
        },
    }
}

/// A keycap's face's sides, its frame (and the frame's heavier bottom),
/// and the gap between two caps of one binding.
const KBD_PAD_X: f32 = 5.0;
const KBD_FRAME: f32 = 1.0;
const KBD_FRAME_BOTTOM: f32 = 2.0;
const KBD_GAP: f32 = 4.0;
/// A keycap's width beyond its key's words: the face's sides and the frame's.
pub(crate) const KBD_CHROME_X: f32 = 2.0 * (KBD_PAD_X + KBD_FRAME);

/// A keycap (`kbd`): the key in secondary ink at 11.5 px, weight 500, on
/// the raised fill, framed in the floating edge — 1 px, and 2 px along the
/// bottom, so it reads as a key standing on the sheet.
pub(crate) fn kbd<M: 'static>(key: impl Into<String>) -> Element<'static, M> {
    let face = container(
        text(key.into())
            .size(size::KBD)
            .font(theme::UI_MEDIUM)
            .color(theme::INK_2)
            .line_height(text::LineHeight::Absolute(17.0.into()))
            .wrapping(text::Wrapping::None),
    )
    .padding([0.0, KBD_PAD_X])
    .style(|_: &Theme| container::Style {
        background: Some(theme::RAISE.into()),
        border: iced::border::rounded(3),
        ..container::Style::default()
    });
    // The frame is the edge showing around the face: one pixel on three
    // sides and two along the bottom, which iced's uniform border cannot.
    container(face)
        .padding(iced::Padding {
            top: KBD_FRAME,
            right: KBD_FRAME,
            bottom: KBD_FRAME_BOTTOM,
            left: KBD_FRAME,
        })
        .style(|_: &Theme| container::Style {
            background: Some(theme::EDGE.into()),
            border: iced::border::rounded(4),
            ..container::Style::default()
        })
        .into()
}

/// A binding's keys as the keycaps a reader presses: "ctrl +" is two caps,
/// and a named key is written the way the keyboard prints it.
pub(crate) fn keycaps(keys: &str) -> Vec<String> {
    keys.split_whitespace()
        .map(|k| match k {
            "esc" | "enter" | "tab" | "ctrl" => sentence(k),
            other => other.to_string(),
        })
        .collect()
}

/// The narrowest a column of the sheet is (`.sheet .cols`: `minmax(180px,
/// 1fr)`) and the gap between two.
const SHEET_COL: f32 = 180.0;
const SHEET_GAP: f32 = 22.0;
/// The sheet card's sides, and the air between a line and its keycaps.
const SHEET_PAD_X: f32 = 18.0;
const SHEET_CAPS_GAP: f32 = 10.0;

/// The `?` sheet (`.sheet`): "Keyboard", then every binding that works on
/// this surface, grouped — each a label on the left and its keycaps on the
/// right, the groups in as many 180 px columns as the sheet holds. Over a
/// dimmed scrim; any press anywhere dismisses it. `jump`: opened from the
/// jump box, whose search is the palette's — the sheet says so, and that
/// what is typed into it goes nowhere.
pub(crate) fn shortcut_sheet<M: Clone + 'static>(
    surface: keys::Surface,
    inert: &[&'static str],
    jump: bool,
    on_dismiss: M,
) -> Element<'static, M> {
    let inert: Vec<&'static str> = inert.to_vec();
    // Only what works HERE, grouped: the sheet answers "what can I press
    // now", so every line on it is one a reader needs, in readable ink.
    let groups: Vec<(&'static str, Vec<&'static keys::Binding>)> = keys::GROUPS
        .into_iter()
        .map(|group| {
            let lines = keys::BINDINGS
                .iter()
                .filter(|b| b.group == group && b.applies(surface))
                .collect::<Vec<_>>();
            (group, lines)
        })
        .filter(|(_, lines)| !lines.is_empty())
        .collect();
    let body = iced::widget::responsive(move |bounds| {
        let per_row = (((bounds.width + SHEET_GAP) / (SHEET_COL + SHEET_GAP)).floor() as usize)
            .clamp(1, groups.len().max(1));
        let mut grid = column![].spacing(10);
        for chunk in groups.chunks(per_row) {
            let mut line = row![].spacing(SHEET_GAP);
            for (group, bindings) in chunk {
                line = line
                    .push(sheet_group::<M>(group, bindings, &inert).width(Length::FillPortion(1)));
            }
            // A short last row keeps the columns above it: the same widths.
            for _ in chunk.len()..per_row {
                line = line.push(Space::new().width(Length::FillPortion(1)));
            }
            grid = grid.push(line);
        }
        grid.into()
    })
    .height(Length::Shrink);
    let card = column![
        text("Keyboard")
            .size(size::TITLE)
            .color(theme::INK)
            .font(theme::UI_SEMIBOLD),
        text(if jump {
            format!(
                "Search comes with the command palette. Until then, what works on the {}. \
                 Esc or a click closes this.",
                surface.name()
            )
        } else {
            format!(
                "What works on the {}. Any key or click closes this.",
                surface.name()
            )
        })
        .size(size::SMALL)
        .color(theme::INK_2),
        Space::new().height(Length::Fixed(6.0)),
        body,
    ]
    .spacing(2);
    let sheet = container(card)
        .padding(iced::Padding {
            top: 16.0,
            right: SHEET_PAD_X,
            bottom: 18.0,
            left: SHEET_PAD_X,
        })
        .width(Length::Fill)
        .max_width(SHEET_W)
        .style(|_: &Theme| sheet_style(10.0));
    // The scrim is the dismiss target as much as the card is: a modal you
    // cannot click away from is a trap.
    let scrim = container(Space::new())
        .width(Length::Fill)
        .height(Length::Fill)
        .style(|_: &Theme| container::Style {
            background: Some(theme::SCRIM.into()),
            ..container::Style::default()
        });
    mouse_area(
        stack![
            scrim,
            // `.overlay`: the card hangs 64 px down, centred, 12 px clear of
            // a narrow window's edges.
            container(sheet)
                .width(Length::Fill)
                .height(Length::Fill)
                .padding(iced::Padding {
                    top: 64.0,
                    right: 12.0,
                    bottom: 12.0,
                    left: 12.0,
                })
                .align_x(iced::Alignment::Center)
                .align_y(iced::Alignment::Start),
        ]
        .width(Length::Fill)
        .height(Length::Fill),
    )
    .on_press(on_dismiss)
    .into()
}

/// The sheet's widest (`.sheet{width:min(640px, …)}`).
const SHEET_W: f32 = 640.0;

/// One group of the sheet: its gold-dim heading (`.sheet h4`), then a line
/// per binding — what it does, and its keycaps pushed to the right.
fn sheet_group<M: 'static>(
    group: &str,
    bindings: &[&keys::Binding],
    inert: &[&str],
) -> iced::widget::Column<'static, M> {
    let mut lines = column![
        text(sentence(group))
            .size(size::MICRO)
            .color(theme::GOLD_DIM)
            .font(theme::UI_SEMIBOLD)
    ]
    .spacing(4);
    for b in bindings {
        let mut caps = row![].spacing(KBD_GAP).align_y(iced::Alignment::Center);
        for cap in keycaps(b.keys) {
            caps = caps.push(kbd::<M>(cap));
        }
        // A line too long for its column wraps under itself rather than
        // lose its end under the keycaps.
        // A key the pull on the stage cannot answer (a stored pull keeps
        // no comparison, no enemies) is listed, dimmed: it works on the
        // surface, not on this pull.
        let ink = if inert.contains(&b.keys) {
            theme::INK_3
        } else {
            theme::INK
        };
        lines = lines.push(
            row![
                text(sentence(b.what))
                    .size(size::BODY)
                    .color(ink)
                    .wrapping(text::Wrapping::Word)
                    .width(Length::Fill),
                caps,
            ]
            .spacing(SHEET_CAPS_GAP)
            .align_y(iced::Alignment::Center),
        );
    }
    lines
}

/// The gear's id on the top bar, so a test can press it where a reader does.
pub(crate) fn gear_id() -> iced::widget::Id {
    iced::widget::Id::new("top-gear")
}

/// The `?` button's id on the top bar.
pub(crate) fn help_id() -> iced::widget::Id {
    iced::widget::Id::new("top-help")
}

/// An icon button (`.ibtn`): a line icon in secondary ink on a 30 px
/// target with its 6 px corners, washed and brightened to ink under the
/// pointer (`.ibtn:hover{background:var(--hover);color:var(--ink)}`) — the
/// top bar's gear and help, the fight header's pull steps. With no message
/// it is inert, the end of a list with nothing to step to: the glyph
/// faint and fainter still ([`INERT_ALPHA`]), no wash, no hand. `id`
/// names the target, for a test that presses it.
pub(crate) fn icon_button<M: Clone + 'static>(
    icon: LineIcon,
    on_press: Option<M>,
    id: Option<iced::widget::Id>,
) -> Element<'static, M> {
    let glyph = if on_press.is_some() {
        line_icon_lit(
            icon,
            size::ICON,
            theme::INK_2,
            theme::INK,
            pitch::ICON_BUTTON,
        )
    } else {
        line_icon(
            icon,
            size::ICON,
            Color {
                a: INERT_ALPHA,
                ..theme::INK_3
            },
        )
    };
    let mut face = container(glyph).center(Length::Fixed(pitch::ICON_BUTTON));
    if let Some(id) = id {
        face = face.id(id);
    }
    button(face)
        .padding(0)
        .on_press_maybe(on_press)
        .style(|_: &Theme, status| button::Style {
            background: matches!(status, button::Status::Hovered | button::Status::Pressed)
                .then(|| theme::HOVER.into()),
            border: iced::border::rounded(ICON_BUTTON_RADIUS),
            ..button::Style::default()
        })
        .into()
}

/// An icon button's corners (`.ibtn{border-radius:6px}`).
const ICON_BUTTON_RADIUS: f32 = 6.0;
/// How much of the faint ink an inert icon button keeps: told from a live
/// one by more than a step of ink.
pub(crate) const INERT_ALPHA: f32 = 0.6;

/// The `?` affordance at the end of the top bar: the one hint the footer
/// no longer needs to recite — the prototype's help icon.
pub(crate) fn help_glyph<M: Clone + 'static>(on_press: M) -> Element<'static, M> {
    icon_button(LineIcon::Help, Some(on_press), Some(help_id()))
}

/// The gear on the top bar: the options card.
pub(crate) fn gear<M: Clone + 'static>(on_press: M) -> Element<'static, M> {
    icon_button(LineIcon::Gear, Some(on_press), Some(gear_id()))
}

/// The wordmark (`.mark`): "wowdps" in Marcellus and the game's gold, the
/// chrome's one resting gold element, ahead of the places. A narrow window
/// drops it (the prototype hides it under 820 px).
pub(crate) fn wordmark<M: 'static>() -> Element<'static, M> {
    container(
        text("wowdps")
            .size(size::MARK)
            .font(theme::TITLE)
            .color(theme::GOLD)
            .wrapping(text::Wrapping::None),
    )
    .padding(iced::Padding {
        top: 0.0,
        right: 12.0,
        bottom: 0.0,
        left: 6.0,
    })
    .into()
}

/// The stat card height: room for the label, the headline-size value and
/// a sub line — each at its line height — so a plain card matches the
/// headline card beside it.
fn card_h(density: Density) -> f32 {
    let line = |px: f32| (px * 1.3).ceil();
    density.pad() * 2.0 + line(size::LABEL) + line(size::DISPLAY) + line(size::MICRO) + 2.0
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
    /// When they last played, on the log's clock (their newest card's
    /// start): what the menu says of them. `None` when no card says.
    pub last_local_ms: Option<i64>,
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

    /// The name's ink in a list of characters: the class colour lifted to
    /// read as text, as any player's name is.
    fn color(&self) -> Color {
        self.class.map_or(theme::INK_2, theme::class_text)
    }

    /// The name's ink where it names whose window this is (the picker,
    /// `.who`): the owner's `--you-text`, a step brighter than a player's.
    fn you_color(&self) -> Color {
        self.class.map_or(theme::INK, theme::you_text)
    }

    /// What the menu says of them (`.mi small`): when they last played —
    /// "played tonight", the weekday within the week, else the date — as
    /// the rail names the night; their fight count when no card says.
    pub(crate) fn note(&self, tonight: i64) -> String {
        let Some(ms) = self.last_local_ms else {
            return plural(self.fights as usize, "fight");
        };
        let night = crate::rail::night_of(ms);
        match tonight - night {
            0 => "played tonight".to_string(),
            1..=6 => crate::rail::weekday(night).to_string(),
            _ => crate::rail::night_label(night, tonight),
        }
    }
}

/// The picker's face as a control (`.who{height:30px;padding-inline:6px
/// 8px;gap:8px;border-radius:6px}`, `.who:hover{background:var(--hover)}`):
/// a press opens the menu.
pub(crate) fn picker_button<'a, M: Clone + 'static>(
    face: impl Into<Element<'a, M>>,
    on_toggle: M,
) -> Element<'a, M> {
    button(
        container(face)
            .height(Length::Fill)
            .align_y(iced::Alignment::Center),
    )
    .height(Length::Fixed(WHO_H))
    .padding(WHO_PAD)
    .on_press(on_toggle)
    .style(|_: &Theme, status| button::Style {
        background: matches!(status, button::Status::Hovered | button::Status::Pressed)
            .then(|| theme::HOVER.into()),
        border: iced::border::rounded(WHO_RADIUS),
        ..button::Style::default()
    })
    .into()
}

/// The picker's spec icon, `size` across, in a 1 px ring of its class
/// colour: a dark icon (Demonology's) on the bar's navy is a shape by its
/// ring, where it was all but gone.
pub(crate) fn ringed_icon<M: 'static>(
    class: Option<wowdps_model::Class>,
    spec: Option<wowdps_model::Spec>,
    size: f32,
) -> Element<'static, M> {
    let ring = class.map_or(theme::INK_3, theme::class_rgb);
    container(crate::compare::class_icon::<M>(
        class,
        spec,
        None,
        size - 2.0 * PICKER_RING,
    ))
    .padding(PICKER_RING)
    .style(move |_: &Theme| container::Style {
        border: iced::Border {
            color: ring,
            width: PICKER_RING,
            radius: (size / 2.0).into(),
        },
        ..container::Style::default()
    })
    .into()
}

/// The ring round the picker's icon.
const PICKER_RING: f32 = 1.0;

/// The picker's frame (`.who`), and the gap between its icon, its name and
/// its caret.
const WHO_H: f32 = 30.0;
const WHO_PAD: iced::Padding = iced::Padding {
    top: 0.0,
    right: 8.0,
    bottom: 0.0,
    left: 6.0,
};
const WHO_RADIUS: f32 = 6.0;
const WHO_GAP: f32 = 8.0;

/// The locked character's name, drawn where the NAME goes — Home's title,
/// or the top bar on any other screen — with its spec icon in its class
/// color and the caret: a press opens [`character_menu`], whose follow
/// switch is there to set with one character as with several. `pick_list`
/// cannot do this: it paints text and nothing else.
pub(crate) fn character_picker<M: Clone + 'static>(
    chars: &[CharPick],
    selected: Option<&str>,
    hide_realms: bool,
    on_toggle: M,
    size: f32,
    max_name: f32,
) -> Element<'static, M> {
    let current = selected
        .and_then(|guid| chars.iter().find(|c| c.guid == guid))
        .or_else(|| chars.first());
    let Some(c) = current else {
        // The wordmark, in the game's gold whatever the chrome.
        return text("wowdps")
            .size(size)
            .color(theme::GOLD)
            .font(theme::TITLE)
            .into();
    };
    // The name is as wide as it says, up to `max_name`: a long one (a
    // realm shown) ends in "…" before it crowds what shares its line.
    let line = row![
        ringed_icon::<M>(c.class, c.spec, size),
        container(
            crate::ellipsis::ellipsis(c.shown(hide_realms))
                .size(size)
                .color(c.you_color())
                .font(theme::UI_SEMIBOLD)
                .leaving(Vec::new(), 0.0),
        )
        .max_width(max_name),
        line_icon(LineIcon::ChevronDown, size * 0.8, theme::INK_3),
    ]
    .spacing(WHO_GAP)
    .align_y(iced::Alignment::Center);
    picker_button(line, on_toggle)
}

/// What the menu shows: the facts, apart from the messages it emits.
#[derive(Debug, Clone, Default)]
pub(crate) struct Menu<'a> {
    pub chars: &'a [CharPick],
    /// The locked character; `None` is the window following whoever
    /// played last — the menu's check item.
    pub selected: Option<&'a str>,
    pub hide_realms: bool,
    /// The row the pointer is over: the follow item is 0, the characters
    /// 1 on.
    pub hover: Option<usize>,
    /// Where the picker is: `Some(inset)` at the top bar's right end, the
    /// menu's right edge `inset` px in from the window's, under the
    /// picker's own; `None` at Home's title, the menu hanging from the left.
    pub at_end: Option<f32>,
    /// The night it is now, for when each character was last played.
    pub tonight: i64,
}

/// The follow switch's empty box, off: inset in the check's slot, its
/// edge and its corners.
const MENU_BOX_INSET: f32 = 2.0;
const MENU_BOX_EDGE: f32 = 1.5;
const MENU_BOX_RADIUS: f32 = 3.0;

/// What the check item says: the lock let go of, the window follows whoever
/// the newest pull was played on.
pub(crate) const FOLLOW: &str = "Follow the character I'm playing";

/// The menu the picker opens (`.menu`): the follow switch, a rule, then
/// every character — icon, class-coloured name, when they last played —
/// the locked one lit. Drawn at the window root over a scrim that takes
/// the press that closes it, hung from the picker it belongs to.
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
        hide_realms,
        hover,
        at_end,
        tonight,
    } = menu;
    let mut rows: Vec<(Element<'static, M>, bool, M)> = Vec::new();
    // `.mi` with its check: the window follows whoever is playing while no
    // character is locked. The check's slot is kept when it is off, so the
    // words stand where they do when it is on.
    let following = selected.is_none();
    // Off, the slot holds an empty box: the item reads as a switch that is
    // off, not as a heading over the characters.
    let check: Element<'static, M> = if following {
        line_icon(LineIcon::Check, MENU_ICON, theme::INK)
    } else {
        container(
            container(Space::new())
                .width(Length::Fixed(MENU_ICON - 2.0 * MENU_BOX_INSET))
                .height(Length::Fixed(MENU_ICON - 2.0 * MENU_BOX_INSET))
                .style(|_: &Theme| container::Style {
                    border: iced::Border {
                        color: theme::INK_3,
                        width: MENU_BOX_EDGE,
                        radius: MENU_BOX_RADIUS.into(),
                    },
                    ..container::Style::default()
                }),
        )
        .center(Length::Fixed(MENU_ICON))
        .into()
    };
    rows.push((
        row![
            check,
            text(FOLLOW)
                .size(MENU_PX)
                .color(theme::INK)
                .wrapping(text::Wrapping::None),
        ]
        .spacing(MENU_GAP)
        .align_y(iced::Alignment::Center)
        .into(),
        false,
        on_pick(None),
    ));
    for c in chars {
        let on = Some(c.guid.as_str()) == selected;
        // A person keeps their colour when picked: the pick is said by the
        // raised row and its accent edge, not by repainting the name.
        let line = row![
            crate::compare::class_icon::<M>(c.class, c.spec, None, MENU_ICON),
            container(
                text(c.shown(hide_realms))
                    .size(MENU_PX)
                    .font(theme::UI_SEMIBOLD)
                    .color(c.color())
                    .wrapping(text::Wrapping::None)
            )
            .clip(true)
            .width(Length::Fill),
            text(c.note(tonight))
                .size(size::SMALL)
                .color(theme::INK_3_TEXT)
                .wrapping(text::Wrapping::None),
        ]
        .spacing(MENU_GAP)
        .align_y(iced::Alignment::Center);
        rows.push((line.into(), on, on_pick(Some(c.guid.clone()))));
    }
    let mut list = column![].spacing(MENU_ROW_GAP);
    // The hover is the window's own wash (`--hover`): fainter than the lit
    // row and borderless, so it can sit on the selection.
    for (i, (line, on, msg)) in rows.into_iter().enumerate() {
        let hovered = hover == Some(i);
        let edge = container(Space::new())
            .width(Length::Fixed(MENU_EDGE))
            .height(Length::Fill)
            .style(move |_: &Theme| container::Style {
                background: on.then(|| accent.base.into()),
                ..container::Style::default()
            });
        let cell = container(
            row![
                edge,
                container(line).padding(MENU_ROW_PAD).width(Length::Fill)
            ]
            .height(Length::Shrink),
        )
        .width(Length::Fill)
        .style(move |_: &Theme| {
            if on {
                container::Style {
                    background: Some(theme::RAISE.into()),
                    border: iced::border::rounded(3),
                    ..container::Style::default()
                }
            } else {
                crate::view::hover_style_in(&theme::Look::WINDOW, hovered)
            }
        });
        list = list.push(
            mouse_area(cell)
                .on_press(msg)
                .on_enter(on_hover(Some(i)))
                .on_exit(on_hover(None)),
        );
        // `.menu hr`: the follow switch is ruled off from the characters.
        if i == 0 {
            list = list.push(container(hairline::<M>()).padding([MENU_RULE_Y, 0.0]));
        }
    }
    // `.menu`: at least 250 wide, its rows edge to edge (12 px in, the lit
    // row's 2 px accent edge taking two of them), 6 px above and below.
    let card = container(list.width(Length::Fixed(pitch::MENU_W)))
        .padding([MENU_PAD_Y, 0.0])
        .style(|_: &Theme| floating_style(MENU_RADIUS));
    // A press on the scrim — anywhere but a row — closes the menu; a row's
    // own press is taken by its mouse_area first.
    let scrim = mouse_area(
        container(Space::new())
            .width(Length::Fill)
            .height(Length::Fill),
    )
    .on_press(on_dismiss);
    // Under the bar (`.menu{top:40px}`), from the picker's own edge: at the
    // bar's right end its right edge is the picker's (`right:70px`), so the
    // card stands clear of what is under the gear and the help button.
    let (pad, align) = match at_end {
        Some(inset) => (
            iced::Padding {
                top: MENU_TOP,
                right: inset,
                bottom: MENU_SIDE,
                left: MENU_SIDE,
            },
            iced::Alignment::End,
        ),
        None => (
            iced::Padding {
                top: MENU_TOP,
                right: MENU_SIDE,
                bottom: MENU_SIDE,
                left: MENU_SIDE,
            },
            iced::Alignment::Start,
        ),
    };
    stack![
        scrim,
        container(card)
            .padding(pad)
            .width(Length::Fill)
            .height(Length::Fill)
            .align_x(align)
            .align_y(iced::Alignment::Start),
    ]
    .width(Length::Fill)
    .height(Length::Fill)
    .into()
}

/// The menu's pieces (`.menu{border-radius:8px;padding:6px 0}`, `.mi{gap:
/// 10px;padding:7px 12px;font-size:14.5px}`, `.menu hr{margin:6px 0}`), the
/// lit row's accent edge, and where the card hangs: 44 px down, under the
/// bar, and never nearer a window edge than 10 px.
const MENU_RADIUS: f32 = 8.0;
const MENU_PAD_Y: f32 = 6.0;
const MENU_PX: f32 = size::BODY;
const MENU_ICON: f32 = size::BODY;
const MENU_GAP: f32 = 10.0;
const MENU_ROW_GAP: f32 = 2.0;
const MENU_ROW_PAD: [f32; 2] = [7.0, 10.0];
const MENU_EDGE: f32 = 2.0;
const MENU_RULE_Y: f32 = 6.0;
const MENU_TOP: f32 = pitch::TOP_BAR;
const MENU_SIDE: f32 = 10.0;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::window::testkit::simulator;
    use wowdps_model::View;

    /// The night of 2026-09-27 on the log's clock, and a moment in it.
    fn tonight() -> i64 {
        crate::rail::night_of(crate::home::parse_ymd("2026-09-27").unwrap() + 20 * 3_600_000)
    }

    fn picks() -> Vec<CharPick> {
        let evening = |ymd: &str| crate::home::parse_ymd(ymd).unwrap() + 20 * 3_600_000;
        vec![
            CharPick {
                guid: "G-a".to_string(),
                name: "Alpha-Realm".to_string(),
                class: Some(wowdps_model::Class::Mage),
                spec: Some(wowdps_model::Spec::Fire),
                fights: 3,
                last_local_ms: Some(evening("2026-09-27")),
            },
            CharPick {
                guid: "G-b".to_string(),
                name: "Beta-Realm".to_string(),
                class: None,
                spec: None,
                fights: 1,
                last_local_ms: None,
            },
            CharPick {
                guid: "G-c".to_string(),
                name: "Gamma-Realm".to_string(),
                class: Some(wowdps_model::Class::Priest),
                spec: None,
                fights: 9,
                last_local_ms: Some(evening("2026-09-26")),
            },
        ]
    }

    #[test]
    fn the_character_picker_shows_the_locked_name_and_honours_hide_realms() {
        let mut ui = simulator(character_picker::<()>(
            &picks(),
            Some("G-b"),
            false,
            (),
            size::TITLE,
            f32::INFINITY,
        ));
        assert!(ui.find("Beta-Realm").is_ok(), "the lock is the name shown");
        // Nothing locked: the first character.
        let mut ui = simulator(character_picker::<()>(
            &picks(),
            None,
            false,
            (),
            size::TITLE,
            f32::INFINITY,
        ));
        assert!(ui.find("Alpha-Realm").is_ok());
        let mut ui = simulator(character_picker::<()>(
            &picks(),
            Some("G-b"),
            true,
            (),
            size::TITLE,
            f32::INFINITY,
        ));
        assert!(
            ui.find("Beta").is_ok(),
            "hide_realms strips the realm here too"
        );
        assert!(ui.find("Beta-Realm").is_err());
        // One character is still a control: the menu holds the follow
        // switch, which is there to set with one character as with several.
        let one: Vec<CharPick> = picks().into_iter().take(1).collect();
        let mut ui = simulator(character_picker::<&str>(
            &one,
            None,
            false,
            "open",
            size::TITLE,
            f32::INFINITY,
        ));
        ui.click("Alpha-Realm").expect("the name");
        let sent: Vec<&str> = ui.into_messages().collect();
        assert_eq!(sent, ["open"]);
    }

    /// The menu: the follow switch — checked while nothing is locked — ruled
    /// off from the characters, each with when they last played ("played
    /// tonight", a weekday within the week), or their fight count when no
    /// card says; never an "Everyone".
    #[test]
    fn the_character_menu_says_when_each_character_last_played() {
        let pick = picks();
        let menu = |selected| {
            simulator(character_menu::<String>(
                Menu {
                    chars: &pick,
                    selected,
                    hide_realms: true,
                    hover: Some(1),
                    at_end: None,
                    tonight: tonight(),
                },
                |_| "hover".to_string(),
                |g| g.map_or_else(|| "follow".to_string(), |g| format!("pick {g}")),
                "dismiss".to_string(),
                theme::NEUTRAL,
            ))
        };
        let mut ui = menu(Some("G-a"));
        assert!(ui.find("Everyone").is_err(), "the lock is never widened");
        assert!(ui.find(FOLLOW).is_ok());
        assert!(ui.find("Alpha").is_ok());
        assert!(ui.find("played tonight").is_ok());
        assert!(ui.find("Saturday").is_ok(), "the night before");
        assert!(ui.find("1 fight").is_ok(), "no card says when");
        ui.click(FOLLOW).unwrap();
        ui.click("Gamma").unwrap();
        let sent: Vec<String> = ui.into_messages().filter(|m| m != "hover").collect();
        assert_eq!(sent, ["follow", "pick G-c"]);
        // The check is the follow switch's state, drawn as a shape.
        let checked = |selected| {
            crate::window::testkit::pixels(
                character_menu::<String>(
                    Menu {
                        chars: &pick,
                        selected,
                        hide_realms: true,
                        hover: None,
                        at_end: None,
                        tonight: tonight(),
                    },
                    |_| String::new(),
                    |_| String::new(),
                    String::new(),
                    theme::NEUTRAL,
                ),
                iced::Size::new(640.0, 480.0),
                &iced::Theme::Dark,
            )
            .count(theme::INK, 30)
        };
        assert!(checked(None) > checked(Some("G-a")), "the check is drawn");
        // A week back and more, the date.
        let old = CharPick {
            last_local_ms: Some(crate::home::parse_ymd("2026-09-12").unwrap() + 3_600_000 * 20),
            ..pick[0].clone()
        };
        assert_eq!(old.note(tonight()), "Saturday, Sep 12");
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
                lead: Lead::Icon(LineIcon::of_view(v)),
                label: wowdps_model::fmt::view_name(v),
                active: v == View::Healing,
                on_press: Some(M::Pick(v)),
                tip: None,
            })
            .collect();
        // The History tab: built, disabled, deliberately inert.
        out.push(Tab {
            lead: Lead::None,
            label: "history",
            active: false,
            on_press: None,
            tip: None,
        });
        out
    }

    #[test]
    fn tab_bar_renders_every_view_and_marks_the_active_one() {
        let mut ui = simulator(tab_bar(tabs(), theme::NEUTRAL, Strip::Views));
        for v in View::ALL {
            assert!(
                ui.find(wowdps_model::fmt::view_name(v)).is_ok(),
                "{v:?} is missing"
            );
        }
        assert!(ui.find("history").is_ok());
        let _ = ui.snapshot(&Theme::TokyoNight).unwrap();
    }

    /// At 460 px the view strip overflows, and the gold underline under
    /// the active tab is still what is drawn there: no scrollbar floats
    /// over it (the old one sat on exactly those two pixels), and the
    /// strip's hairline runs under the rest.
    #[test]
    fn the_active_tab_is_underlined_in_gold_at_a_narrow_width() {
        use crate::window::testkit::pixels;
        let mut tabs = tabs();
        for t in &mut tabs {
            t.active = t.label == "Damage";
        }
        let size = iced::Size::new(460.0, 60.0);
        let px = pixels(
            tab_bar(tabs, theme::GOLD_ACCENT, Strip::Views),
            size,
            &theme::window_theme(),
        );
        // The underline's two logical rows, physical at 2x, under the
        // first tab (Damage leads the strip).
        let (top, bottom) = ((pitch::TAB as u32 - 2) * 2, pitch::TAB as u32 * 2);
        let under_damage = (8, top, 120, bottom);
        let gold = px.count_in(under_damage, theme::GOLD, 2);
        assert!(gold > 100, "only {gold} gold pixels under the active tab");
        // Nothing else on the underline's rows is gold: one tab is lit.
        assert_eq!(px.count_in((260, top, px.w, bottom), theme::GOLD, 2), 0);
        // The hairline under the strip, the whole width.
        let line = px.count_in((0, bottom, px.w, bottom + 2), theme::LINE, 2);
        assert!(line > px.w as usize, "the strip's rule: {line}");
        assert_eq!(px.count(theme::THUMB, 2), 0, "no scrollbar");
    }

    /// The window's own strip at a narrow window's width (460 less the
    /// frame), for EVERY view: the active tab's gold underline is drawn on
    /// the first frame — Taken, Deaths and Enemies included, with no scroll
    /// state to restore — because the strip hangs from its end when the
    /// active tab is in its second half.
    #[test]
    fn every_views_active_tab_is_on_screen_at_a_narrow_width() {
        use crate::window::testkit::pixels;
        let size = iced::Size::new(440.0, 60.0);
        let (top, bottom) = ((pitch::TAB as u32 - 2) * 2, pitch::TAB as u32 * 2);
        for v in crate::view::WINDOW_VIEWS {
            let px = pixels(
                crate::view::view_tabs_with(
                    theme::GOLD_ACCENT,
                    v,
                    false,
                    None,
                    iced::Padding::ZERO,
                ),
                size,
                &theme::window_theme(),
            );
            let gold = px.count_in((0, top, px.w, bottom), theme::GOLD, 2);
            assert!(gold > 100, "{v:?}: {gold} gold pixels on the underline");
        }
        // The prototype's order: the two views read most after damage and
        // healing come before the counts.
        assert_eq!(
            crate::view::WINDOW_VIEWS.map(crate::view::window_view_name),
            [
                "Damage",
                "Healing",
                "Taken",
                "Deaths",
                "Interrupts",
                "Crowd control",
                "Dispels",
                "Enemies"
            ]
        );
    }

    /// The view tabs as the meter's row draws them at `width`: the strip,
    /// and the filter at its end, with `active` lit.
    fn narrow_strip(active: usize, focused: bool) -> Element<'static, M> {
        let tabs: Vec<Tab<M>> = crate::view::WINDOW_VIEWS
            .into_iter()
            .enumerate()
            .map(|(i, v)| Tab {
                lead: Lead::Icon(LineIcon::of_view(v)),
                label: crate::view::window_view_name(v),
                active: i == active,
                on_press: Some(M::Pick(v)),
                tip: None,
            })
            .collect();
        let filter = filter_box(
            "",
            focused,
            "Filter players",
            |_| M::Dismiss,
            M::Dismiss,
            M::Dismiss,
            M::Dismiss,
        );
        view_strip(
            tabs,
            theme::GOLD_ACCENT,
            Some(filter),
            iced::Padding {
                top: 0.0,
                right: 12.0,
                bottom: 0.0,
                left: 10.0,
            },
        )
    }

    /// However narrow the row — a 460 px window, the filter taking a third
    /// of it and more with focus — the ACTIVE tab is whole in sight: its
    /// icon and its word, between the row's start and the filter. Index
    /// anchoring left a middle tab (Deaths, Interrupts) out of sight from
    /// either end.
    #[test]
    fn the_active_tab_is_whole_in_a_narrow_strip() {
        let width = 460.0;
        for focused in [false, true] {
            for (i, v) in crate::view::WINDOW_VIEWS.into_iter().enumerate() {
                let mut ui = crate::window::testkit::simulator_as(
                    crate::window::settings(),
                    iced::Size::new(width, 60.0),
                    narrow_strip(i, focused),
                );
                let word = ui
                    .find(crate::view::window_view_name(v))
                    .expect("the active tab")
                    .bounds();
                let field = ui.find(filter_id()).expect("the filter").bounds();
                // The icon and its gap ahead of the word.
                let start = word.x - size::TAB_ICON - 6.0;
                assert!(start >= 10.0 - 0.5, "{v:?} starts at {start} ({focused})");
                assert!(
                    word.x + word.width <= field.x - 8.0 + 0.5,
                    "{v:?} ends at {} under the filter at {} ({focused})",
                    word.x + word.width,
                    field.x
                );
                let _ = ui.snapshot(&Theme::TokyoNight).unwrap();
            }
        }
    }

    /// A plain wheel over the strip moves it along — a wheel turned toward
    /// the reader, further along — as far as its ends allow, and the
    /// active tab stays where the wheel put the strip until the view, the
    /// row or the strip changes.
    #[test]
    fn a_wheel_moves_the_view_strip_along() {
        let wheel = |y: f32| {
            iced::Event::Mouse(mouse::Event::WheelScrolled {
                delta: mouse::ScrollDelta::Lines { x: 0.0, y },
            })
        };
        let mut ui = crate::window::testkit::simulator_as(
            crate::window::settings(),
            iced::Size::new(460.0, 60.0),
            narrow_strip(0, false),
        );
        let before = ui.find("Damage").unwrap().bounds();
        ui.point_at(iced::Point::new(100.0, 18.0));
        let _ = ui.simulate([wheel(-1.0)]);
        let after = ui.find("Damage").unwrap().bounds();
        assert!(
            (before.x - after.x - 60.0).abs() < 0.5,
            "one notch along: {before:?} → {after:?}"
        );
        // Back past the start: the strip stops at its start.
        let _ = ui.simulate([wheel(5.0)]);
        let back = ui.find("Damage").unwrap().bounds();
        assert!((back.x - before.x).abs() < 0.5, "{back:?}");
        assert!(
            ui.into_messages().next().is_none(),
            "a wheel is the strip's own, never a message"
        );
    }

    #[test]
    fn only_the_active_tab_wears_the_underline() {
        assert_eq!(tab_underline(true, theme::GOLD_ACCENT), Some(theme::GOLD));
        assert_eq!(tab_underline(false, theme::GOLD_ACCENT), None);
        let class = theme::accent(Some(wowdps_model::Class::Mage), None);
        assert_eq!(tab_underline(true, class), Some(class.base));
    }

    /// The pointer brightens an idle tab to parchment; the active tab is
    /// parchment already, and a dead one stays faint under it.
    #[test]
    fn a_hovered_tab_brightens() {
        use button::Status;
        assert_eq!(tab_ink(false, Status::Active), theme::INK_2);
        assert_eq!(tab_ink(false, Status::Hovered), theme::INK);
        assert_eq!(tab_ink(true, Status::Active), theme::INK);
        assert_eq!(tab_ink(false, Status::Disabled), theme::INK_3);
    }

    #[test]
    fn keycaps_are_the_keys_as_the_keyboard_prints_them() {
        assert_eq!(keycaps("ctrl +"), ["Ctrl", "+"]);
        assert_eq!(keycaps("esc"), ["Esc"]);
        assert_eq!(keycaps("T"), ["T"]);
        assert_eq!(sentence("crowd control"), "Crowd control");
        assert_eq!(sentence("OVER +0:26"), "Over +0:26");
        assert_eq!(sentence(""), "");
        let mut ui = simulator(kbd::<()>("Ctrl"));
        assert!(ui.find("Ctrl").is_ok());
        let _ = ui.snapshot(&Theme::TokyoNight).unwrap();
    }

    #[test]
    fn a_disabled_tab_emits_no_message() {
        let only_history = vec![Tab::<M> {
            lead: Lead::None,
            label: "history",
            active: false,
            on_press: None,
            tip: None,
        }];
        let mut ui = simulator(tab_bar(only_history, theme::NEUTRAL, Strip::Places));
        let _ = ui.click("history");
        assert!(ui.into_messages().next().is_none(), "a dead tab is silent");
    }

    #[test]
    fn a_live_tab_emits_its_message() {
        let mut ui = simulator(tab_bar(tabs(), theme::NEUTRAL, Strip::Views));
        ui.click("Dispels").unwrap();
        assert_eq!(
            ui.into_messages().collect::<Vec<_>>(),
            vec![M::Pick(View::Dispels)]
        );
    }

    /// A band gives every card room for its figure: a narrow window stacks
    /// them, a wide one lays them in one row.
    #[test]
    fn the_stat_band_wraps_before_a_figure_would_clip() {
        assert_eq!(cards_per_row(4, 1420.0, 8.0), 4);
        // Four that fit three to a row are drawn two and two.
        assert_eq!(cards_per_row(4, 440.0, 8.0), 2);
        assert_eq!(cards_per_row(5, 440.0, 8.0), 3);
        assert_eq!(cards_per_row(6, 440.0, 8.0), 3);
        assert_eq!(cards_per_row(3, 440.0, 8.0), 3);
        assert_eq!(cards_per_row(3, 100.0, 8.0), 1);
        assert_eq!(cards_per_row(0, 440.0, 8.0), 1);
        let cards: Vec<Stat> = (0..5)
            .map(|i| Stat {
                label: format!("card {i}"),
                value: "1,488,795,375".to_string(),
                sub: None,
                value_color: None,
                headline: false,
            })
            .collect();
        let mut ui = simulator(stat_cards::<M>(&cards, Density::Comfortable));
        for i in 0..5 {
            assert!(ui.find(format!("Card {i}").as_str()).is_ok());
        }
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
        let mut ui = simulator(stat_cards::<M>(&cards, Density::Comfortable));
        assert!(ui.find("1.2M").is_ok());
        assert!(ui.find("—").is_ok());
        assert!(ui.find("not in the log").is_ok());
        // The gradient path only actually runs under the renderer.
        let _ = ui.snapshot(&Theme::TokyoNight).unwrap();
    }

    #[test]
    fn the_shortcut_sheet_lists_the_surfaces_bindings_only() {
        let mut ui = simulator(shortcut_sheet(keys::Surface::Meter, &[], false, M::Dismiss));
        for b in keys::BINDINGS {
            let listed = ui.find(sentence(b.what).as_str()).is_ok();
            assert_eq!(
                listed,
                b.applies(keys::Surface::Meter),
                "{b:?}: a key is on the sheet exactly when it works here"
            );
        }
        let _ = ui.snapshot(&Theme::TokyoNight).unwrap();
    }

    /// Every line of the `?` sheet reads whole in the column the sheet
    /// gives it at its widest (three columns in 640 px), measured in the
    /// window's own fonts beside its keycaps: the keys the inspector
    /// rewords stand on one line, and no line takes more than two — the
    /// lines wrap rather than clip, so a long one is never cut mid-word.
    #[test]
    fn every_sheet_line_fits_its_column() {
        type P = <iced::Renderer as iced::advanced::text::Renderer>::Paragraph;
        // The window's faces, loaded as the window loads them.
        drop(crate::window::testkit::simulator_as(
            crate::window::settings(),
            iced::Size::new(80.0, 20.0),
            Element::<M>::from(Space::new()),
        ));
        let inner = SHEET_W - 2.0 * SHEET_PAD_X;
        let per_row = ((inner + SHEET_GAP) / (SHEET_COL + SHEET_GAP)).floor();
        assert_eq!(per_row, 3.0, "three columns at the sheet's widest");
        let col = (inner - (per_row - 1.0) * SHEET_GAP) / per_row;
        let width = |s: &str, px: f32, font| crate::ellipsis::width_of::<P>(s, px, font);
        for b in keys::BINDINGS {
            let caps = keycaps(b.keys);
            // A keycap: its word, its face's 5 px sides and its frame's
            // 1 px ones; 4 px between two.
            let caps_w = caps
                .iter()
                .map(|c| width(c, size::KBD, theme::UI_MEDIUM) + 2.0 * (KBD_PAD_X + KBD_FRAME))
                .sum::<f32>()
                + KBD_GAP * caps.len().saturating_sub(1) as f32;
            let room = col - caps_w - SHEET_CAPS_GAP;
            let words = sentence(b.what);
            let w = width(&words, size::BODY, theme::UI);
            if ["enter", "tab", "v", "g"].contains(&b.keys) {
                assert!(w <= room, "{words:?} is {w:.0} px in {room:.0}");
            }
            assert!(
                w <= 1.8 * room,
                "{words:?} is {w:.0} px: two lines of {room:.0}"
            );
        }
    }

    #[test]
    fn the_sheet_dismisses_on_any_press() {
        let mut ui = simulator(shortcut_sheet(keys::Surface::Meter, &[], false, M::Dismiss));
        ui.click("Views").unwrap();
        assert_eq!(ui.into_messages().collect::<Vec<_>>(), vec![M::Dismiss]);
    }

    #[test]
    fn the_chrome_pieces_render() {
        let accent = theme::accent(Some(wowdps_model::Class::Priest), None);
        let mut ui = simulator(badge::<M>(&Badge::new("KILL", theme::GOOD)));
        assert!(
            ui.find("Kill").is_ok(),
            "a badge is worded in sentence case"
        );
        let _ = ui.snapshot(&Theme::TokyoNight).unwrap();

        let mut ui = simulator(chip_row(
            vec![
                ("keys".to_string(), M::Dismiss),
                ("raid".to_string(), M::Pick(View::Damage)),
            ],
            Some(0),
            accent,
        ));
        assert!(ui.find("Jump to").is_ok());
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
        ));
        assert!(ui.find("recent").is_ok());
        assert!(ui.find("last 24 h").is_ok());
        assert!(ui.find("nothing yet").is_ok());
        ui.click("all fights").unwrap();
        assert_eq!(ui.into_messages().collect::<Vec<_>>(), vec![M::Dismiss]);

        let mut ui = simulator(filter_box(
            "durgan",
            false,
            "Filter players",
            |_| M::Dismiss,
            M::Dismiss,
            M::Dismiss,
            M::Dismiss,
        ));
        assert!(ui.find("durgan").is_ok());
        // A filled filter offers a way out, on a target a finger or a
        // shaky pointer can hit (24 × 24, WCAG 2.5.8).
        let clear = ui
            .find(filter_clear_id())
            .expect("a filled filter offers a way out")
            .bounds();
        assert!(
            (clear.width - 24.0).abs() < 0.5 && (clear.height - 24.0).abs() < 0.5,
            "{clear:?}"
        );
        let _ = ui.snapshot(&Theme::TokyoNight).unwrap();
        let mut ui = simulator(filter_box(
            "",
            false,
            "Filter players",
            |_| M::Dismiss,
            M::Dismiss,
            M::Dismiss,
            M::Dismiss,
        ));
        assert!(ui.find(filter_clear_id()).is_err(), "an empty one does not");
        assert!(ui.find("/").is_ok(), "the key that focuses it is on it");
    }

    /// The filter is compact at rest and widens with focus (`.filter
    /// input`: 92 px, 140 px on `:focus-within`), and the glyph, the
    /// keycap and the clear mark sit over the field rather than beside it:
    /// the box is the field, so a press anywhere on it lands there.
    #[test]
    fn the_filter_widens_with_focus_and_is_one_field() {
        let width = |value: &str, focused: bool| {
            let mut ui = simulator(filter_box(
                value,
                focused,
                "Filter players",
                |_| M::Dismiss,
                M::Dismiss,
                M::Dismiss,
                M::Dismiss,
            ));
            ui.find(filter_id()).expect("the field").bounds().width
        };
        let (rest, wide) = (width("", false), width("", true));
        assert_eq!(wide - rest, FILTER_W_FOCUSED - FILTER_W);
        assert!(rest < 170.0, "compact at rest: {rest}");
        // `.filter:focus-within`: some 202 px overall, 28 tall.
        assert!((wide - 202.0).abs() < 1.0, "{wide}");
        let mut ui = simulator(filter_box(
            "",
            true,
            "Filter players",
            |_| M::Dismiss,
            M::Dismiss,
            M::Dismiss,
            M::Dismiss,
        ));
        let tall = ui.find(filter_id()).unwrap().bounds().height;
        assert!((tall - FILTER_H).abs() < 0.5, "{tall}");
        // With focus the ground fills it inside the floating edge — never
        // the gold of a focus ring — whether iced or the window says so.
        let th = Theme::TokyoNight;
        for style in [
            filter_style(
                &th,
                text_input::Status::Focused { is_hovered: false },
                false,
            ),
            filter_style(&th, text_input::Status::Active, true),
        ] {
            assert_eq!(style.border.color, theme::EDGE);
            assert_eq!(style.background, theme::GROUND.into());
        }
        let idle = filter_style(&th, text_input::Status::Active, false);
        assert_eq!(idle.border.color, Color::TRANSPARENT);
        let hover = filter_style(&th, text_input::Status::Hovered, false);
        assert_eq!(hover.border.color, theme::LINE);
        // A value to clear makes room for the mark, not less for the text.
        assert!(width("dur", false) > rest);
        // A press on the search glyph lands on the field under it, and its
        // release tells the window the field has focus.
        let focus = M::Pick(View::Healing);
        let mut ui = simulator(filter_box(
            "",
            false,
            "Filter players",
            |_| M::Dismiss,
            M::Dismiss,
            focus.clone(),
            M::Dismiss,
        ));
        let field = ui.find(filter_id()).unwrap().bounds();
        ui.point_at(iced::Point::new(field.x + 14.0, field.center_y()));
        let _ = ui.simulate(iced_test::simulator::click());
        let _ = ui.snapshot(&Theme::TokyoNight).unwrap();
        assert_eq!(ui.into_messages().collect::<Vec<_>>(), vec![focus]);
    }
}
