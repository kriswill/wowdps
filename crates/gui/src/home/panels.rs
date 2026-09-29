//! Home as the prototype lays it out (`.home`): the title and its scope
//! chips (`.home-top`), the night (`.lastnight`), and the week's panels in
//! a grid (`.hgrid`, `.panel`) — keys against their timers (`.krow`,
//! `.par`), a raid's bosses with a dot per pull (`.brow`, `.pdot`), and
//! key throughput across characters. Every tile, row and dot is a jump
//! point into its pull.

use iced::widget::{Space, button, column, container, mouse_area, row, scrollable, stack, text};
use iced::{Border, Color, Element, Length, Theme, mouse};

use wowdps_model::fmt::{duration, human};

use super::charts::{self, ParBar, RankSlope, Trend};
use super::{BossLine, CharLine, KeyRun, Meta, NightPanel, NightPull, Panels, PullDot, RaidPanel};
use crate::ellipsis::ellipsis;
use crate::fight_head::ordinal;
use crate::nav;
use crate::rail::shown_name;
use crate::theme::{self, size};
use crate::window::Message;

/// The page's insets (`.home{padding:18px 22px 30px}`, and `14px 12px
/// 24px` at 820 px and under) and the air between its parts (`gap:18px`).
const PAD: iced::Padding = iced::Padding {
    top: 18.0,
    right: 22.0,
    bottom: 30.0,
    left: 22.0,
};
/// Narrow, the insets are the same both sides: the page's scrollbar takes
/// a lane of its own beside them only while the page overflows
/// (`overflow:auto`), so a Home that fits is centred ([`page_of`]).
const PAD_NARROW: iced::Padding = iced::Padding {
    top: 14.0,
    right: 12.0,
    bottom: 24.0,
    left: 12.0,
};
const GAP: f32 = 18.0;
/// The title (`.home-top h2{font-family:Marcellus;font-size:28px}`) and
/// what stands beside it (`.home-top{gap:16px}`).
const TITLE_PX: f32 = 28.0;
const TOP_GAP: f32 = 16.0;
/// A scope chip (`.chip{height:26px;padding-inline:10px;font-size:14px;
/// gap:6px}`, `.chip .cd{8px}`) and the air between two (`.chips{gap:6px}`).
const CHIP_H: f32 = 26.0;
const CHIP_PAD_X: f32 = 10.0;
const CHIP_PX: f32 = 14.0;
const CHIP_DOT: f32 = 8.0;
const CHIP_GAP: f32 = 6.0;
/// The night's card (`.lastnight{padding:16px 18px;gap:18px 26px;
/// border-radius:10px}`, its columns `1.2fr` and `1fr`), its place in
/// Marcellus (`h3{22px}`), its caption (`.cap{13px 600}`) and its line of
/// words (`.sub{margin:2px 0 10px}`, the frame's 14 px).
const CARD_PAD: [f32; 2] = [16.0, 18.0];
const CARD_GAP_X: f32 = 26.0;
const CARD_GAP_Y: f32 = 18.0;
const CARD_RADIUS: f32 = 10.0;
const LEFT_PORTION: u16 = 12;
const RIGHT_PORTION: u16 = 10;
const PLACE_PX: f32 = 22.0;
const CAP_PX: f32 = 13.0;
const SUB_PX: f32 = 14.0;
const SUB_BELOW: f32 = 10.0;
/// A pull's tile (`.ptile{grid-template-columns:18px minmax(0,1fr) auto;
/// gap:10px;padding:7px 8px;border-radius:6px;font-size:15px}`, `.r{14px}`).
const TILE_MARK: f32 = 18.0;
const TILE_GAP: f32 = 10.0;
const TILE_PAD: [f32; 2] = [7.0, 8.0];
const TILE_RADIUS: f32 = 6.0;
const TILE_PX: f32 = 15.0;
const TILE_RIGHT_PX: f32 = 14.0;
/// A wipe's best % after its name, a space's width from it.
const TILE_TAIL_GAP: f32 = 4.0;
/// Tiles the night lists before it says how many more there were: a
/// progression night of thirty wipes is the chart's to show, not a list's.
pub(super) const MAX_TILES: usize = 10;
/// A panel (`.panel{padding:14px 16px 16px;border-radius:10px}`, `h3{margin:
/// 0 0 10px;font-size:15px;font-weight:600}`, `h3 small{13px}`).
const PANEL_PAD: iced::Padding = iced::Padding {
    top: 14.0,
    right: 16.0,
    bottom: 16.0,
    left: 16.0,
};
const PANEL_RADIUS: f32 = 10.0;
const PANEL_TITLE_PX: f32 = 15.0;
const PANEL_SMALL_PX: f32 = 13.0;
const PANEL_HEAD_BELOW: f32 = 10.0;
/// The least air between a panel's title and its small words, which the
/// prototype's `justify-content:space-between` pushes apart.
const PANEL_HEAD_GAP: f32 = 8.0;
/// A key's or a boss's row, washed under the pointer as a jump point: its
/// corners, a keycap's.
const ROW_RADIUS: f32 = 4.0;
/// The narrowest a panel is laid out at (`.hgrid{grid-template-columns:
/// repeat(auto-fit,minmax(330px,1fr))}`) — what a key's row needs for its
/// dungeon's name beside a par bar of its least width — and the most
/// columns, however wide: past three a dashboard stops being glanceable. At
/// 820 px and under there is one (`.hgrid{grid-template-columns:minmax(0,
/// 1fr)}`).
const MIN_COL: f32 = 330.0;
const MAX_COLS: usize = 3;
/// A key's row (`.krow{grid-template-columns:8px minmax(0,1fr) minmax(90px,
/// 1.1fr) 66px;gap:10px;height:30px;font-size:14.5px}`).
const KEY_H: f32 = 30.0;
const KEY_DOT: f32 = 8.0;
const KEY_GAP: f32 = 10.0;
const KEY_RESULT_W: f32 = 66.0;
const KEY_BAR_MIN: f32 = 90.0;
const KEY_BAR_SHARE: f32 = 1.1;
const KEY_PX: f32 = 14.5;
/// A boss's row (`.brow{gap:4px 12px;padding:7px 0}`, `.bn{15px}`, `.bs{
/// 14px}`) and its dots (`.pdot{10px;border:2px}`, `.pdots{gap:4px}`).
const BOSS_PAD_Y: f32 = 7.0;
const BOSS_GAP: f32 = 4.0;
const BOSS_GAP_X: f32 = 12.0;
const BOSS_PX: f32 = 15.0;
const BOSS_RIGHT_PX: f32 = 14.0;
const PDOT: f32 = 10.0;
const PDOT_EDGE: f32 = 2.0;
const PDOT_GAP: f32 = 4.0;
/// A legend (`.legend{gap:14px;font-size:13px;margin-top:8px}`, `i{9px}`,
/// `span{gap:6px}`).
const LEGEND_GAP: f32 = 14.0;
const LEGEND_PX: f32 = 13.0;
const LEGEND_DOT: f32 = 9.0;
const LEGEND_ABOVE: f32 = 8.0;
const LEGEND_INNER: f32 = 6.0;
/// A ring's edge in a legend (`box-shadow:inset 0 0 0 2px`).
const LEGEND_RING: f32 = 2.0;

/// How many columns of panels fit in `width`.
pub(crate) fn columns_for(width: f32, gap: f32) -> usize {
    if !width.is_finite() || width <= 0.0 {
        return 1;
    }
    // n columns need n*MIN_COL plus the gaps between them.
    let mut n = 1;
    while n < MAX_COLS && (n + 1) as f32 * MIN_COL + n as f32 * gap <= width {
        n += 1;
    }
    n
}

/// The panels a week shows: the prototype's three (keys, the raid, key
/// throughput), whatever the week holds, and a panel for each further raid.
pub(super) fn panel_count(panels: &Panels) -> usize {
    3 + panels.raids.len().saturating_sub(1)
}

/// The columns `count` panels are laid out in across `inner`: as many as
/// fit, but never more than there are panels — `repeat(auto-fit, …)`
/// collapses an empty track, so a short row is shared by the panels it has
/// rather than leaving a hole beside them.
pub(super) fn grid_columns(inner: f32, count: usize) -> usize {
    columns_for(inner, GAP).min(count).max(1)
}

/// The page, `width` wide.
pub(super) fn laid_out(
    meta: &Meta,
    panels: &Panels,
    chars: &[CharLine],
    width: f32,
) -> Element<'static, Message> {
    // `@container app (max-width: 820px)`: Home is the whole window's
    // width there (the rail is a drawer), so its own width is the test.
    let narrow = width <= theme::NARROW_WINDOW;
    let pad = if narrow { PAD_NARROW } else { PAD };
    let inner = (width - pad.left - pad.right).max(0.0);
    let mut page = column![top(meta, chars)].spacing(GAP);
    // What the reader is looking at, before any number: an empty screen for
    // three different reasons must not look like one screen. A store that
    // is off, or not heard from yet, has no week to show — the night card
    // says why, and no panel says "none" of what nobody has read. A store
    // that answered but lost something says so over the week it has.
    if meta.settled
        && let Some(line) = meta.state_line.clone()
    {
        page = page.push(text(line).size(size::MICRO).color(theme::INK_2));
    }
    page = page.push(last_night(meta, panels, chars, inner, narrow));
    if !meta.settled {
        return page_of(page, pad, narrow);
    }
    // The prototype's three, always and in its order — keys, the raid,
    // throughput — each saying so when its week is empty, so the dashboard
    // keeps its shape from one week to the next; a second raid of the week
    // (another difficulty) after them, rather than pushing throughput off
    // the first row.
    let cols = if narrow {
        1
    } else {
        grid_columns(inner, panel_count(panels))
    };
    let col_w = (inner - GAP * cols.saturating_sub(1) as f32) / cols as f32;
    let who = scope_name(meta, chars);
    let hide = meta.hide_realms;
    let mut raids = panels.raids.iter();
    let mut cards = vec![
        keys_panel(panels, who.as_deref(), hide, col_w),
        raid_panel(raids.next(), who.as_deref(), hide),
        trend_panel(panels, who.as_deref(), hide, col_w),
    ];
    cards.extend(raids.map(|r| raid_panel(Some(r), who.as_deref(), hide)));
    page = page.push(grid(cards, cols, GAP));
    // The one honest word about a stop the reader would otherwise read as
    // the whole week.
    if meta.stalled {
        page = page.push(
            text(format!(
                "Only the newest {} pulls of the week were read.",
                super::PAGE * super::MAX_PAGES
            ))
            .size(size::MICRO)
            .color(theme::INK_3_TEXT),
        );
    }
    page_of(page, pad, narrow)
}

/// The page's column in its insets, scrolling. Narrow, its bar has a lane
/// of its own, taken only while the page overflows: 12 px of gutter is too
/// little for a bar to float in beside the panels. Wide, it floats in the
/// 22 px gutter, clear of them.
fn page_of(
    page: iced::widget::Column<'static, Message>,
    pad: iced::Padding,
    narrow: bool,
) -> Element<'static, Message> {
    let page = scrollable(container(page).padding(pad).width(Length::Fill))
        .height(Length::Fill)
        .width(Length::Fill);
    if narrow {
        page.direction(scrollable::Direction::Vertical(
            scrollable::Scrollbar::new()
                .width(theme::pitch::SCROLL_LANE)
                .spacing(0.0),
        ))
        .into()
    } else {
        page.into()
    }
}

/// The scoped character's name, as drawn; `None` scoped to all of them.
fn scope_name(meta: &Meta, chars: &[CharLine]) -> Option<String> {
    let guid = meta.scope.as_deref()?;
    Some(chars.iter().find(|c| c.guid == guid).map_or_else(
        || "this character".to_string(),
        |c| shown_name(&c.name, meta.hide_realms),
    ))
}

/// The title — "You, this week", or the scoped character's — and the scope
/// chips: all characters, then each one, a dot in their class colour. A
/// chip scopes Home and is remembered as the scope it opens on.
fn top(meta: &Meta, chars: &[CharLine]) -> Element<'static, Message> {
    // The title names the character without their realm whatever the
    // option says — the chip beside it carries the realm when it is shown —
    // and wraps rather than runs off a narrow window's edge (the
    // prototype's `h2` wraps).
    let title = match scope_name(meta, chars) {
        Some(name) => format!("{}, this week", crate::view::display_name(&name)),
        None => "You, this week".to_string(),
    };
    let mut line = row![
        text(title)
            .size(TITLE_PX)
            .font(theme::TITLE)
            .color(theme::INK)
            .wrapping(text::Wrapping::WordOrGlyph)
    ]
    .spacing(TOP_GAP)
    .align_y(iced::Alignment::Center);
    let mut chips = row![scope_chip(
        super::ALL_CHARACTERS.to_string(),
        None,
        meta.scope.is_none(),
        meta.accent,
        None
    )]
    .spacing(CHIP_GAP)
    .align_y(iced::Alignment::Center);
    for c in chars {
        let on = meta.scope.as_deref() == Some(c.guid.as_str());
        chips = chips.push(scope_chip(
            shown_name(&c.name, meta.hide_realms),
            Some(c.class.map_or(theme::INK_3, theme::class_rgb)),
            on,
            meta.accent,
            Some(c.guid.clone()),
        ));
    }
    line = line.push(chips.wrap().vertical_spacing(CHIP_GAP));
    line.wrap().vertical_spacing(CHIP_GAP).into()
}

/// One scope chip (`.chip`): the character's dot and name, pressed — the
/// accent's edge over a wash of it — when it is the scope; under the
/// pointer its edge and its words brighten (`.chip:hover{color:var(--ink);
/// border-color:var(--edge)}`), a pressed chip's edge staying the accent's.
fn scope_chip(
    label: String,
    dot: Option<Color>,
    on: bool,
    accent: theme::Accent,
    scope: Option<String>,
) -> Element<'static, Message> {
    let mut face = row![].spacing(CHIP_GAP).align_y(iced::Alignment::Center);
    if let Some(color) = dot {
        face = face.push(nav::dot::<Message>(color, CHIP_DOT));
    }
    // The words take the chip's ink, which its state sets.
    face = face.push(text(label).size(CHIP_PX).wrapping(text::Wrapping::None));
    button(
        container(face)
            .height(Length::Fill)
            .align_y(iced::Alignment::Center),
    )
    .padding([0.0, CHIP_PAD_X])
    .height(Length::Fixed(CHIP_H))
    .on_press(Message::HomeCharacter(scope))
    .style(move |_: &Theme, status| {
        let lit = matches!(status, button::Status::Hovered | button::Status::Pressed);
        button::Style {
            background: on.then(|| theme::accent_wash(accent).into()),
            text_color: if on || lit { theme::INK } else { theme::INK_2 },
            border: Border {
                color: if on {
                    accent.base
                } else if lit {
                    theme::EDGE
                } else {
                    theme::LINE
                },
                width: 1.0,
                radius: (CHIP_H / 2.0).into(),
            },
            ..button::Style::default()
        }
    })
    .into()
}

/// The night card's surface (`.lastnight`): a panel's.
fn card_style(_: &Theme) -> container::Style {
    nav::surface_style(CARD_RADIUS)
}

/// "Sunday, September 27" — the year after it when it is not tonight's.
pub(super) fn long_date(day: i64, tonight: i64) -> String {
    let (y, m, d) = crate::rail::civil(day);
    let month = crate::rail::month_name(m, false);
    let weekday = crate::rail::weekday(day);
    if y == crate::rail::civil(tonight).0 {
        format!("{weekday}, {month} {d}")
    } else {
        format!("{weekday}, {month} {d}, {y}")
    }
}

/// What the night card's caption calls its night: tonight's is tonight's.
pub(super) fn night_cap(day: i64, tonight: i64) -> &'static str {
    if day == tonight {
        "Tonight you played"
    } else {
        "Last night you played"
    }
}

fn cap(words: &'static str) -> Element<'static, Message> {
    text(words)
        .size(CAP_PX)
        .font(theme::UI_SEMIBOLD)
        .color(theme::GOLD_DIM)
        .into()
}

/// The night (`.lastnight`): where and when, on whom, each pull as a tile
/// with its place in the role — and beside it (under it, narrow) how that
/// place moved across the night.
fn last_night(
    meta: &Meta,
    panels: &Panels,
    chars: &[CharLine],
    inner: f32,
    narrow: bool,
) -> Element<'static, Message> {
    let Some(n) = &panels.night else {
        return container(
            column![
                cap(EMPTY_CAP),
                nav::note::<Message>(empty_night(meta, panels, chars))
            ]
            .spacing(SUB_BELOW),
        )
        .padding(CARD_PAD)
        .width(Length::Fill)
        .style(card_style)
        .into();
    };
    let left = night_words(meta, n);
    // The chart is as wide as its column lets it be, up to its own cap.
    let content = inner - 2.0 * CARD_PAD[1];
    let right_w = if narrow {
        content
    } else {
        (content - CARD_GAP_X) * f32::from(RIGHT_PORTION) / f32::from(LEFT_PORTION + RIGHT_PORTION)
    };
    let right = column![
        cap("Your rank, pull by pull"),
        RankSlope::new(&n.pulls, n.who.color()).view(right_w.clamp(1.0, charts::SLOPE_MAX_W)),
        legend(vec![
            (legend_dot(n.who.color()), "kill or timed".to_string()),
            (
                legend_ring(n.who.color(), theme::SURFACE),
                "wipe or over time".to_string()
            ),
        ]),
    ];
    let body: Element<'static, Message> = if narrow {
        column![left, right].spacing(CARD_GAP_Y).into()
    } else {
        row![
            container(left).width(Length::FillPortion(LEFT_PORTION)),
            container(right).width(Length::FillPortion(RIGHT_PORTION)),
        ]
        .spacing(CARD_GAP_X)
        .into()
    };
    container(body)
        .padding(CARD_PAD)
        .width(Length::Fill)
        .style(card_style)
        .into()
}

/// The night card's left column: its caption, its place, the date and the
/// character, and the tiles.
fn night_words(meta: &Meta, n: &NightPanel) -> Element<'static, Message> {
    let who = n.who.class.map_or(theme::INK, theme::class_text);
    // The line wraps between its words, never inside the name: a row of
    // runs, each whole.
    let sub = row![
        text(format!("{}, on ", long_date(n.day, meta.tonight)))
            .size(SUB_PX)
            .color(theme::INK_2),
        text(shown_name(&n.who.name, meta.hide_realms))
            .size(SUB_PX)
            .font(theme::UI_SEMIBOLD)
            .color(who),
        text(". ").size(SUB_PX).color(theme::INK_2),
        text(format!("Rank is among your role, by {}.", n.measure))
            .size(SUB_PX)
            .color(theme::INK_2),
    ]
    .wrap();
    let mut col = column![
        cap(night_cap(n.day, meta.tonight)),
        text(n.place.clone())
            .size(PLACE_PX)
            .font(theme::TITLE)
            .color(theme::INK),
        container(sub).padding(iced::Padding {
            top: 2.0,
            bottom: SUB_BELOW,
            ..iced::Padding::ZERO
        }),
    ];
    // The newest of a long night: the chart beside holds every pull.
    let skip = n.pulls.len().saturating_sub(MAX_TILES);
    if skip > 0 {
        col = col.push(
            text(format!(
                "{} earlier that night, on the chart and the rail",
                nav::plural(skip, "pull")
            ))
            .size(nav::NOTE_PX)
            .color(theme::INK_3_TEXT),
        );
    }
    for p in n.pulls.iter().skip(skip) {
        col = col.push(tile(p));
    }
    col.into()
}

/// The night card's caption with no night to show: no "last night"
/// over words that say there was none, or that nobody has read it yet.
pub(super) const EMPTY_CAP: &str = "Your week";

/// Why the night card has no night: words for each reason, never an empty
/// frame — a store off or not heard from yet in its own words (the
/// screen's state line), before any "none".
fn empty_night(meta: &Meta, panels: &Panels, chars: &[CharLine]) -> String {
    if !meta.settled {
        return meta.state_line.clone().unwrap_or_default();
    }
    if panels.unowned {
        return "The store has not named a character of yours yet: list them in \
                history_characters, or install the wowdps addon."
            .to_string();
    }
    match scope_name(meta, chars) {
        Some(name) => format!("No pulls on {name} this week."),
        None => "No pulls stored this week.".to_string(),
    }
}

/// One pull of the night (`.ptile`): its outcome, its name — "at 56%" after
/// a wipe's — and its place in the role with the figure it was by: "17th
/// of 19, 149.3k". As wide as it says (the prototype's tile is an inline
/// button, its place right after its name); a name too long for the column
/// gives way before the place does. A press opens it.
fn tile(p: &NightPull) -> Element<'static, Message> {
    let place = ordinal(p.standing.place);
    let of = format!(
        " of {}, {}",
        p.standing.of,
        human(p.standing.value.round().max(0.0) as u64)
    );
    let mut name = ellipsis(p.name.clone())
        .size(TILE_PX)
        .color(theme::INK)
        .leaving(
            vec![
                (place.clone(), TILE_RIGHT_PX, theme::UI_MEDIUM),
                (of.clone(), TILE_RIGHT_PX, theme::UI),
            ],
            TILE_MARK + 2.0 * TILE_GAP + 2.0 * TILE_PAD[1],
        );
    if let Some(pct) = p.wipe_pct {
        name = name
            .tail(
                format!("at {pct}%"),
                TILE_PX,
                theme::INK_3_TEXT,
                TILE_TAIL_GAP,
            )
            .giving(crate::ellipsis::Give::Whole);
    }
    let right = row![
        text(place)
            .size(TILE_RIGHT_PX)
            .font(theme::UI_MEDIUM)
            .color(theme::INK)
            .wrapping(text::Wrapping::None),
        text(of)
            .size(TILE_RIGHT_PX)
            .color(theme::INK_2)
            .wrapping(text::Wrapping::None),
    ];
    let face = row![
        container(crate::rail::mark(p.mark)).center_x(Length::Fixed(TILE_MARK)),
        name,
        right,
    ]
    .spacing(TILE_GAP)
    .align_y(iced::Alignment::Center);
    jump(
        face,
        TILE_PAD,
        TILE_RADIUS,
        p.fight_id.clone(),
        Length::Shrink,
    )
}

/// A press anywhere on `face` opens the stored pull `fight_id`, washed
/// under the pointer as every row that answers it is.
fn jump(
    face: impl Into<Element<'static, Message>>,
    pad: [f32; 2],
    radius: f32,
    fight_id: String,
    width: Length,
) -> Element<'static, Message> {
    button(face)
        .padding(pad)
        .width(width)
        .on_press(Message::OpenStored(fight_id))
        .style(move |_: &Theme, status| button::Style {
            background: matches!(status, button::Status::Hovered | button::Status::Pressed)
                .then(|| theme::HOVER.into()),
            text_color: theme::INK,
            border: iced::border::rounded(radius),
            ..button::Style::default()
        })
        .into()
}

/// A panel (`.panel`): its heading and the small words beside it, over its
/// body, in the panel's insets. Its surface is not its own: [`grid`] lays
/// it under each row's panels as tall as the tallest of them, as the
/// prototype's grid stretches its cards.
fn panel(
    title: String,
    small: &'static str,
    body: impl Into<Element<'static, Message>>,
) -> Element<'static, Message> {
    let head = row![
        container(
            ellipsis(title)
                .size(PANEL_TITLE_PX)
                .font(theme::UI_SEMIBOLD)
                .color(theme::INK)
        )
        .width(Length::Fill),
        text(small)
            .size(PANEL_SMALL_PX)
            .color(theme::INK_3_TEXT)
            .wrapping(text::Wrapping::None),
    ]
    .spacing(PANEL_HEAD_GAP)
    .align_y(iced::Alignment::Center);
    container(
        column![
            container(head).padding(iced::Padding {
                bottom: PANEL_HEAD_BELOW,
                ..iced::Padding::ZERO
            }),
            body.into()
        ]
        .width(Length::Fill),
    )
    .padding(PANEL_PAD)
    .width(Length::Fill)
    .into()
}

/// A panel's surface (`.panel{background:var(--surface);border:1px solid
/// var(--line);border-radius:10px}`), as tall as its row.
fn panel_surface() -> Element<'static, Message> {
    container(Space::new())
        .width(Length::FillPortion(1))
        .height(Length::Fill)
        .style(|_: &Theme| nav::surface_style(PANEL_RADIUS))
        .into()
}

/// The empty words of a panel about `what`: "No keys this week.", "No
/// keys on Tranqster this week."
fn none_of(what: &str, who: Option<&str>) -> String {
    match who {
        Some(name) => format!("No {what} on {name} this week."),
        None => format!("No {what} this week."),
    }
}

/// "Keys this week": each key's time against its timers.
fn keys_panel(
    panels: &Panels,
    who: Option<&str>,
    hide_realms: bool,
    col_w: f32,
) -> Element<'static, Message> {
    let inner = (col_w - PANEL_PAD.left - PANEL_PAD.right).max(0.0);
    let body: Element<'static, Message> = if panels.keys.is_empty() {
        nav::note(none_of("keys", who))
    } else {
        let mut list = column![];
        for k in &panels.keys {
            list = list.push(key_row(k, inner, hide_realms));
        }
        column![
            list,
            legend_words("Ticks mark +3, +2 and the timer, left to right"),
        ]
        .into()
    };
    panel(
        "Keys this week".to_string(),
        "run time against the timer",
        body,
    )
}

/// The widths of a key row's name and bar in a panel `inner` wide: what the
/// dot, the result and the gaps leave, shared 1 : 1.1 with the bar never
/// under 90 px (`minmax(0,1fr) minmax(90px,1.1fr)`).
pub(super) fn key_widths(inner: f32) -> (f32, f32) {
    let free = (inner - KEY_DOT - KEY_RESULT_W - 3.0 * KEY_GAP).max(0.0);
    let bar = (free * KEY_BAR_SHARE / (1.0 + KEY_BAR_SHARE))
        .max(KEY_BAR_MIN)
        .min(free);
    (free - bar, bar)
}

/// One key (`.krow`): whose it was, its name, its run against its timers
/// and what it earned — "+1 31:39", "over 36:12". A press opens it.
fn key_row(k: &KeyRun, inner: f32, hide_realms: bool) -> Element<'static, Message> {
    let (name_w, bar_w) = key_widths(inner);
    let result = row![
        text(k.result())
            .size(KEY_PX)
            .font(theme::UI_SEMIBOLD)
            .color(if k.timed { theme::GOOD } else { theme::BAD })
            .wrapping(text::Wrapping::None),
        text(format!(" {}", duration(k.clock_ms)))
            .size(KEY_PX)
            .color(theme::INK)
            .wrapping(text::Wrapping::None),
    ];
    let face = row![
        nav::tip(
            nav::dot::<Message>(k.who.color(), KEY_DOT),
            shown_name(&k.who.name, hide_realms)
        ),
        container(ellipsis(k.name.clone()).size(KEY_PX).color(theme::INK))
            .width(Length::Fixed(name_w)),
        ParBar {
            clock_ms: k.clock_ms,
            pars: k.pars,
            timed: k.timed,
        }
        .view(bar_w),
        container(result)
            .width(Length::Fixed(KEY_RESULT_W))
            .align_x(iced::Alignment::End),
    ]
    .spacing(KEY_GAP)
    .height(Length::Fixed(KEY_H))
    .align_y(iced::Alignment::Center);
    jump(
        face,
        [0.0, 0.0],
        ROW_RADIUS,
        k.fight_id.clone(),
        Length::Fill,
    )
}

/// A raid at its difficulty: each boss with how the week went on it —
/// "Killed in 7:02" or "Best 2%", the pull count — and one dot per pull.
/// `None` is the panel with nothing in it, which says so.
fn raid_panel(
    r: Option<&RaidPanel>,
    who: Option<&str>,
    hide_realms: bool,
) -> Element<'static, Message> {
    let Some(r) = r else {
        return panel(
            "Raid".to_string(),
            "one dot per pull",
            nav::note(none_of("raid pulls", who)),
        );
    };
    let mut list = column![];
    for (i, b) in r.bosses.iter().enumerate() {
        // `.brow{border-top:1px solid var(--line)}`, the first's none.
        if i > 0 {
            list = list.push(nav::hairline::<Message>());
        }
        list = list.push(boss_row(b, hide_realms));
    }
    panel(
        r.title.clone(),
        "one dot per pull",
        column![
            list,
            legend(vec![
                (legend_dot(theme::GOOD), "kill".to_string()),
                (
                    legend_ring(theme::INK_3, Color::TRANSPARENT),
                    "wipe, ringed in the character's colour".to_string()
                ),
            ]),
        ],
    )
}

/// What a boss's row says of it: its fastest kill, else how close the
/// closest wipe came, else that it was never killed — and in which ink.
pub(super) fn boss_outcome(b: &BossLine) -> (String, Color) {
    match (b.best_kill_ms, b.best_pct) {
        (Some(ms), _) => (format!("Killed in {}", duration(ms)), theme::GOOD),
        (None, Some(pct)) => (format!("Best {pct}%"), theme::INK),
        // No kill and no OBSERVED health reading: never "100%", never "0%".
        (None, None) => ("No kill".to_string(), theme::INK),
    }
}

/// One boss (`.brow`): its name and outcome — a press opens its fastest
/// kill, else its newest pull — and under them a dot per pull, each a
/// press to that pull.
fn boss_row(b: &BossLine, hide_realms: bool) -> Element<'static, Message> {
    let (outcome, ink) = boss_outcome(b);
    let head = row![
        container(ellipsis(b.name.clone()).size(BOSS_PX).color(theme::INK)).width(Length::Fill),
        row![
            text(outcome)
                .size(BOSS_RIGHT_PX)
                .font(theme::UI_MEDIUM)
                .color(ink)
                .wrapping(text::Wrapping::None),
            text(format!(", {}", nav::plural(b.pulls.len(), "pull")))
                .size(BOSS_RIGHT_PX)
                .color(theme::INK_2)
                .wrapping(text::Wrapping::None),
        ],
    ]
    .spacing(BOSS_GAP_X)
    .align_y(iced::Alignment::Center);
    let mut dots = row![].spacing(PDOT_GAP).align_y(iced::Alignment::Center);
    for d in &b.pulls {
        dots = dots.push(pull_dot(d, hide_realms));
    }
    column![
        jump(
            head,
            [0.0, 0.0],
            ROW_RADIUS,
            b.fight_id.clone(),
            Length::Fill
        ),
        dots.wrap().vertical_spacing(PDOT_GAP),
    ]
    .spacing(BOSS_GAP)
    .padding([BOSS_PAD_Y, 0.0])
    .into()
}

/// A pull's dot (`.pdot`): filled green on a kill, a ring in whose colour
/// on a wipe. A press opens the pull.
fn pull_dot(d: &PullDot, hide_realms: bool) -> Element<'static, Message> {
    let (fill, edge) = if d.kill {
        (theme::GOOD, theme::GOOD)
    } else {
        (Color::TRANSPARENT, d.who.color())
    };
    let dot = container(Space::new())
        .width(Length::Fixed(PDOT))
        .height(Length::Fixed(PDOT))
        .style(move |_: &Theme| container::Style {
            background: Some(fill.into()),
            border: Border {
                color: edge,
                width: PDOT_EDGE,
                radius: (PDOT / 2.0).into(),
            },
            ..container::Style::default()
        });
    let words = format!(
        "{}, {}",
        shown_name(&d.who.name, hide_realms),
        if d.kill { "kill" } else { "wipe" }
    );
    mouse_area(nav::tip(dot, words))
        .on_press(Message::OpenStored(d.fight_id.clone()))
        .interaction(mouse::Interaction::Pointer)
        .into()
}

/// "Effective dps on keys": a dot per run in its character's colour, each
/// character's best ringed; a legend of whose dots are whose.
fn trend_panel(
    panels: &Panels,
    who: Option<&str>,
    hide_realms: bool,
    col_w: f32,
) -> Element<'static, Message> {
    let inner = (col_w - PANEL_PAD.left - PANEL_PAD.right).max(1.0);
    let body: Element<'static, Message> = if panels.trend.is_empty() {
        nav::note(if panels.keys.is_empty() {
            none_of("keys", who)
        } else {
            none_of("keys played as dps", who)
        })
    } else {
        let mut seen: Vec<&super::Char> = Vec::new();
        for p in &panels.trend {
            if !seen.iter().any(|c| c.guid == p.who.guid) {
                seen.push(&p.who);
            }
        }
        let mut items: Vec<(Element<'static, Message>, String)> = seen
            .iter()
            .map(|c| (legend_dot(c.color()), shown_name(&c.name, hide_realms)))
            .collect();
        items.push((
            legend_ring(theme::LEGENDARY, Color::TRANSPARENT),
            "personal best".to_string(),
        ));
        column![Trend::new(&panels.trend).view(inner), legend(items)].into()
    };
    panel("Effective dps on keys".to_string(), "each dot a run", body)
}

/// A legend (`.legend`): marks and their words, wrapping.
fn legend(items: Vec<(Element<'static, Message>, String)>) -> Element<'static, Message> {
    let mut line = row![].spacing(LEGEND_GAP).align_y(iced::Alignment::Center);
    for (mark, words) in items {
        line = line.push(
            row![
                mark,
                text(words)
                    .size(LEGEND_PX)
                    .color(theme::INK_2)
                    .wrapping(text::Wrapping::None)
            ]
            .spacing(LEGEND_INNER)
            .align_y(iced::Alignment::Center),
        );
    }
    legend_frame(line.wrap().vertical_spacing(LEGEND_INNER))
}

/// A legend of words alone (`.legend span` with no mark): flush with the
/// panel's edge, where a mark would have stood.
fn legend_words(words: &'static str) -> Element<'static, Message> {
    legend_frame(
        text(words)
            .size(LEGEND_PX)
            .color(theme::INK_2)
            .wrapping(text::Wrapping::Word),
    )
}

/// A legend's place under what it explains (`.legend{margin-top:8px}`).
fn legend_frame(content: impl Into<Element<'static, Message>>) -> Element<'static, Message> {
    container(content)
        .padding(iced::Padding {
            top: LEGEND_ABOVE,
            ..iced::Padding::ZERO
        })
        .into()
}

/// A legend's filled mark.
fn legend_dot(color: Color) -> Element<'static, Message> {
    nav::dot::<Message>(color, LEGEND_DOT)
}

/// A legend's ring: `edge` round a `fill`.
fn legend_ring(edge: Color, fill: Color) -> Element<'static, Message> {
    container(Space::new())
        .width(Length::Fixed(LEGEND_DOT))
        .height(Length::Fixed(LEGEND_DOT))
        .style(move |_: &Theme| container::Style {
            background: Some(fill.into()),
            border: Border {
                color: edge,
                width: LEGEND_RING,
                radius: (LEGEND_DOT / 2.0).into(),
            },
            ..container::Style::default()
        })
        .into()
}

/// Lay panels out in `cols` columns, padding the last row so a lone panel
/// keeps its column's width instead of stretching across the window. Every
/// panel of a row is as tall as the tallest (the prototype's `.hgrid` is a
/// CSS grid, its cards stretched): the panels are the row's base layer,
/// which sets its height, and their surfaces a second row of the same
/// shape laid under them, each as tall as the row. (Cards of `Fill` height
/// in a row inside the page's scrollable would be laid out against no
/// height at all.)
fn grid(
    panels: Vec<Element<'static, Message>>,
    cols: usize,
    gap: f32,
) -> Element<'static, Message> {
    let mut grid = column![].spacing(gap);
    let mut panels = panels.into_iter().peekable();
    while panels.peek().is_some() {
        let mut line = row![].spacing(gap).align_y(iced::Alignment::Start);
        let mut under = row![].spacing(gap).height(Length::Fill);
        for _ in 0..cols {
            match panels.next() {
                Some(p) => {
                    line = line.push(container(p).width(Length::FillPortion(1)));
                    under = under.push(panel_surface());
                }
                None => {
                    line = line.push(Space::new().width(Length::FillPortion(1)));
                    under = under.push(Space::new().width(Length::FillPortion(1)));
                }
            }
        }
        grid = grid.push(stack![line].push_under(under));
    }
    grid.into()
}
