//! Rendering. Nothing here mutates state.
//!
//! The window is its top bar over a body: the pull rail beside (or, 1180 px
//! and under, over) either Home or a pull's workspace — the fight header,
//! the ribbon, the view tabs, the meter and the inspector beside it. A pull
//! is drawn from the stage's `ClientState` (`Gui::fight`), the tailed log's
//! or a stored pull's own, so one set of renderers draws both.
//!
//! The renderers the overlay shares with the window (the recap rows, the
//! ability drill's breadcrumb, stat strip and target list, the team divider,
//! the row styles) keep their names for the overlay's look, pixel for pixel;
//! each has an `_in` twin that takes a surface's [`Look`]. The window draws
//! its drill, recap and comparison in the inspector now, so of these it
//! calls only the team divider's and the row styles' twins, with
//! `Look::WINDOW`; the rest draw the overlay alone. The overlay's own rows
//! (`overlay_row`, `overlay_drill_row`) and what it borrows as is
//! (`header_tag`, `rank_cell`, `hover_style`) draw as they always have; the
//! rest is the window's alone and draws with the redesign's tokens directly.

use iced::widget::{Space, checkbox, column, container, mouse_area, row, scrollable, stack, text};
use iced::{Border, Color, Element, Font, Length, Theme};

use wowdps_model::fmt::human;
use wowdps_model::{Class, Role, Row, Screen, View};
use wowdps_proto::ClientState;

use crate::compare;

use crate::line_icons::LineIcon;
use crate::nav;
use crate::rail;
use crate::table;
use crate::theme::{self, DensityPitch, Look, pitch, size};
use crate::window::{Gui, Message, RowHover};
use wowdps_gui_logic::{drill, labels};

/// A right-lane wrapper for anything inside a `scrollable`: the scrollbar
/// paints OVER the content's right edge, and without this the last column
/// (a %, an amount) sits under it.
pub(crate) fn scroll_clear<'a, M: 'a>(
    content: impl Into<Element<'a, M>>,
) -> iced::widget::Container<'a, M> {
    container(content).padding(iced::Padding {
        top: 0.0,
        right: pitch::SCROLL_LANE,
        bottom: 0.0,
        left: 0.0,
    })
}

// The palette lives in `theme` now; re-exported here because every renderer
// in the crate — the overlay above all — names it through `view::`.
pub(crate) use crate::theme::{DIM, GREEN, RED, YELLOW};

/// Bar color for players whose COMBATANT_INFO has not been seen yet.
pub(crate) const CLASSLESS: Color = Color::from_rgb(0.42, 0.44, 0.52);
/// R24: the hostile red an enemy row wears — its bar and its skull disc —
/// on the enemy view, where no row has a class.
pub(crate) const HOSTILE: Color = Color::from_rgb(0.80, 0.30, 0.32);

pub fn view(state: &Gui) -> Element<'_, Message> {
    // The talent viewer replaces the whole screen while open (`t` / Esc);
    // the ClientState machine underneath keeps running untouched.
    if let Some(ui) = &state.talents {
        return container(crate::talents::screen(ui).map(Message::Talents))
            .padding(10)
            .width(Length::Fill)
            .height(Length::Fill)
            .into();
    }
    // Whether the rail stands beside the stage or is a drawer over it is
    // the window's width, which only the layout knows.
    let body = iced::widget::responsive(move |size| body(state, size.width));
    let page = column![crate::top_bar::Bar::of(state).element(), body]
        .width(Length::Fill)
        .height(Length::Fill);
    if let Some(p) = &state.palette {
        stack![
            page,
            crate::palette::overlay(p, &state.palette_items(), accent_of(state))
        ]
        .into()
    } else if state.shortcuts_open {
        stack![
            page,
            nav::shortcut_sheet(
                state.surface(),
                &state.inert_keys(),
                Message::ToggleShortcuts
            )
        ]
        .into()
    } else if state.options_open {
        stack![page, options_panel(&state.cfg, accent_of(state))].into()
    } else if state.picker_open {
        // The picker's menu, hung from the top bar: every character the
        // window knows you play (Home's answers and the rail's pages), the
        // one Home is scoped to — or opens on — lit. A pick is Home's
        // scope.
        let picks: Vec<nav::CharPick> = state
            .known_characters
            .iter()
            .map(crate::home::char_pick)
            .collect();
        let scope = match &state.home {
            Some(ui) => ui.scope.as_deref(),
            None => state.cfg.character.as_deref(),
        };
        stack![
            page,
            nav::character_menu(
                nav::Menu {
                    chars: &picks,
                    selected: scope,
                    hide_realms: state.cfg.hide_realms,
                    hover: state.picker_hover,
                    at_end: Some(crate::top_bar::PICKER_END),
                    tonight: state.tonight(),
                },
                Message::PickerHover,
                Message::PickerFollow,
                |guid| Message::HomeCharacter(Some(guid)),
                Message::TogglePicker,
                accent_of(state),
            )
        ]
        .into()
    } else {
        page.into()
    }
}

/// Everything under the top bar, `width` wide: the rail and what it is
/// beside — Home in the window's frame, or a pull's workspace edge to edge
/// (`.stage`), whose tabs' and headings' hairlines, selected rows and total
/// run to its edges, each piece carrying the prototype's own inset.
fn body(state: &Gui, width: f32) -> Element<'static, Message> {
    let docked = rail::docked(width);
    let content: Element<'static, Message> = match &state.home {
        // Home pads itself: its insets are the window's breakpoint's.
        Some(ui) => crate::home::screen(
            ui,
            &state.home_panels,
            &state.known_characters,
            accent_of(state),
            state.cfg.hide_realms,
            state.tonight(),
        ),
        // The stage measures itself; the breakpoints are the window's, so
        // it is told what the docked rail takes.
        None => stage(state, if docked { rail::RAIL_W + 1.0 } else { 0.0 }),
    };
    let rail = |w: f32| rail::panel(&state.rail(), &state.rail_shown(), w);
    if docked {
        // `.rail{border-right:1px solid var(--line)}`.
        row![rail(rail::RAIL_W), nav::vrule::<Message>(), content]
            .height(Length::Fill)
            .into()
    } else if state.rail_open {
        rail::drawer(content, rail(rail::DRAWER_W))
    } else {
        content
    }
}

/// A pull's workspace: the meter's, or — the store not answering for the
/// stored pull the rail listed — the fight header (its rail button and its
/// steps, so the pointer can still leave) over the word that it is gone.
fn stage(state: &Gui, beside: f32) -> Element<'static, Message> {
    if state.stored.as_ref().is_some_and(|s| s.missing) {
        let head = crate::fight_head::Head {
            rail_button: beside == 0.0,
            beside,
            ..crate::fight_head::Head::of(state, false)
        };
        return column![head.element(), gone(state.history_disabled.as_deref())]
            .width(Length::Fill)
            .height(Length::Fill)
            .into();
    }
    meter_screen(state, beside)
}

/// What stands on the stage for a stored pull the store did not answer for
/// (`.empty`), asked twice: retention took it after the rail listed it —
/// or, when the daemon says the store is off, that.
fn gone(disabled: Option<&str>) -> Element<'static, Message> {
    let (title, words) = match disabled {
        Some(why) => (
            "The history store is off".to_string(),
            format!(
                "The daemon says: {why}. Tonight's pulls of the log are still on the rail, \
                 and m is the live one."
            ),
        ),
        None => (
            "This pull is gone from the history store".to_string(),
            "The store answered twice that it no longer holds it: retention may have \
             removed it after the rail listed it. Pick another pull, or press m for the \
             live one."
                .to_string(),
        ),
    };
    container(
        column![
            text(title)
                .size(size::TITLE)
                .font(theme::UI_SEMIBOLD)
                .color(theme::INK),
            text(words).size(size::BODY).color(theme::INK_2),
        ]
        .spacing(GONE_GAP)
        .max_width(GONE_W),
    )
    .padding(GONE_PAD)
    .width(Length::Fill)
    .height(Length::Fill)
    .into()
}

/// `.empty{max-width:52ch}`, near enough at the reading size; its inset
/// and the air between its two lines.
const GONE_W: f32 = 420.0;
const GONE_PAD: [f32; 2] = [28.0, 20.0];
const GONE_GAP: f32 = 6.0;

/// The chrome accent: the OWNER's, resolved once and held (`Gui::accent`).
/// It says whose window this is, not what the cursor is on — the design
/// study's §2a — and it deliberately ignores the selection: rows resort on
/// every 10 Hz snapshot, so a selection-derived accent re-tinted the whole
/// window whenever rank 1 changed class, with no user action at all.
fn accent_of(state: &Gui) -> theme::Accent {
    state.accent
}

#[cfg(test)]
pub(crate) fn accent_for_test(state: &Gui) -> theme::Accent {
    accent_of(state)
}

/// Accent-folded, case-insensitive substring over what a row IS: its label,
/// its class, its spec and its role — so "akanos" finds `Akanôs`. Ranks and percentages are NOT recomputed — a filtered
/// row keeps the rank and share it holds in the whole chart, which is the
/// entire point of filtering one player out of it.
/// Indices dropped: only the tests want the rows on their own, since every
/// drawn list needs the original index a click sends back.
#[cfg(test)]
pub(crate) fn filtered(rows: Vec<Row>, filter: &str) -> Vec<Row> {
    filtered_indexed(rows, filter)
        .into_iter()
        .map(|(_, r)| r)
        .collect()
}

#[cfg(test)]
pub(crate) use wowdps_gui_logic::table::filtered_indexed;
pub(crate) use wowdps_gui_logic::table::ordered;

// ---- the meter -------------------------------------------------------------

/// The fight's workspace (`.stage`): the fight header, the view tabs with
/// the row filter at their end, and under them master and detail — the
/// meter, and beside it the inspector on the selected player (or the
/// pair). At 820 px and under the meter is alone and the inspector is
/// pushed over the whole stage (`.insp{position:absolute;inset:0}`) while
/// the keys are in it; a click on a row pushes it there. The pull is the
/// stage's (`Gui::fight`): the tailed log's or a stored one, drawn alike.
/// `beside` is what the docked rail takes of the window: the stage
/// measures itself, and every breakpoint here is the window's.
fn meter_screen(state: &Gui, beside: f32) -> Element<'static, Message> {
    let app = state.fight();
    let head = crate::fight_head::Head {
        rail_button: beside == 0.0,
        beside,
        ..crate::fight_head::Head::of(state, true)
    };
    let accent = accent_of(state);
    let stored = state.stored.is_some();
    let view = app.view;
    let (filter, focused) = (state.filter.clone(), state.filter_focused);
    // It narrows whatever the rows are: on Enemies, the enemies.
    let placeholder = if view == View::EnemyTaken {
        "Filter enemies"
    } else {
        "Filter players"
    };
    let list = MeterList::meter(state);
    // R25 (v35): the pull's signature under the header, and on the Deaths
    // view the deaths in the order they happened in the meter's place —
    // both from the raid timeline, the live daemon's or the store's rebuild.
    let ribbon = crate::ribbon::Ribbon::of(state);
    let deaths = crate::deaths::Table::of(state);
    let insp = crate::inspector::Insp::of(state);
    let pushed = app.inspecting();
    let status = app.status.clone();
    let toast = state.toast.as_ref().map(|(words, _)| toast_card(words));
    let stage = iced::widget::responsive(move |bounds| {
        let window = bounds.width + beside;
        let fit = crate::inspector::Fit::of(window);
        let mut stage = column![];
        if fit == crate::inspector::Fit::Narrow && pushed {
            stage = stage.push(insp.view(bounds.width, fit, true));
        } else {
            let filter = nav::filter_box(
                &filter,
                focused,
                placeholder,
                Message::Filter,
                Message::Filter(String::new()),
                Message::FocusFilter,
                Message::FilterDone,
            );
            // The meter's columns narrow with the WINDOW (`@container app
            // (max-width: 820px)`), not with the room the inspector leaves.
            let narrow = fit == crate::inspector::Fit::Narrow;
            let table = |push: bool| match &deaths {
                Some(t) => t.clone().view(narrow),
                None => meter_table(MeterList {
                    narrow,
                    push,
                    ..list.clone()
                }),
            };
            let body: Element<'static, Message> = match crate::inspector::beside(window) {
                Some(w) => row![
                    container(table(false)).width(Length::Fill),
                    // `.meter{border-right:1px solid var(--line)}`.
                    nav::vrule::<Message>(),
                    insp.view(w, fit, false),
                ]
                .height(Length::Fill)
                .into(),
                None => table(true),
            };
            stage = stage.push(head.clone().element());
            // The stage keeps its children in place with or without the
            // ribbon (iced matches a column's children by position): the
            // tabs' filter and the list's scroll keep their state when a
            // timeline arrives or goes.
            stage = stage.push(match &ribbon {
                Some(r) => r.clone().view(narrow),
                None => Space::new().height(Length::Fixed(0.0)).into(),
            });
            stage = stage
                .push(view_tabs_with(
                    accent,
                    view,
                    stored,
                    Some(filter),
                    STAGE_TABS,
                ))
                .push(body);
        }
        if let Some(footer) = footer_of(status.as_deref()) {
            stage = stage.push(container(footer).padding(STAGE_FOOTER));
        }
        stage.height(Length::Fill).into()
    });
    match toast {
        Some(card) => stack![stage, card].into(),
        None => stage.into(),
    }
}

/// A passing word over the stage (`.toast{position:absolute;left:50%;
/// bottom:18px;transform:translateX(-50%);background:var(--raise);border:
/// 1px solid var(--edge);border-radius:8px;padding:8px 14px;font-size:14px;
/// box-shadow:0 12px 30px rgba(0,0,0,.5);max-width:calc(100% - 32px)}`),
/// centred at the stage's foot. It takes no pointer: the stage under it
/// keeps its hover and its clicks.
fn toast_card(words: &str) -> Element<'static, Message> {
    let card = container(text(words.to_string()).size(TOAST_PX).color(theme::INK))
        .padding([TOAST_PAD_Y, TOAST_PAD_X])
        .style(|_: &Theme| container::Style {
            background: Some(theme::RAISE.into()),
            border: iced::Border {
                color: theme::EDGE,
                width: 1.0,
                radius: TOAST_RADIUS.into(),
            },
            shadow: TOAST_SHADOW,
            ..container::Style::default()
        });
    container(card)
        .width(Length::Fill)
        .height(Length::Fill)
        .align_x(iced::Alignment::Center)
        .align_y(iced::Alignment::End)
        .padding(iced::Padding {
            top: 0.0,
            right: TOAST_SIDE,
            bottom: TOAST_BOTTOM,
            left: TOAST_SIDE,
        })
        .into()
}

/// The toast's words, its insets, its corners, its distance from the
/// stage's foot and its least distance from either side, and its shadow.
const TOAST_PX: f32 = 14.0;
const TOAST_PAD_Y: f32 = 8.0;
const TOAST_PAD_X: f32 = 14.0;
const TOAST_RADIUS: f32 = 8.0;
const TOAST_BOTTOM: f32 = 18.0;
const TOAST_SIDE: f32 = 16.0;
const TOAST_SHADOW: iced::Shadow = iced::Shadow {
    color: Color::from_rgba(0.0, 0.0, 0.0, 0.5),
    offset: iced::Vector::new(0.0, 12.0),
    blur_radius: 30.0,
};

/// A fight's workspace runs edge to edge (`.stage`), each piece in its own
/// inset: the view tabs' row (`.vtabs{padding:0 12px 0 10px}`) and the
/// status line.
const STAGE_TABS: iced::Padding = iced::Padding {
    top: 0.0,
    right: 12.0,
    bottom: 0.0,
    left: 10.0,
};
const STAGE_FOOTER: iced::Padding = iced::Padding {
    top: 0.0,
    right: 10.0,
    bottom: 6.0,
    left: 10.0,
};

/// The ⚙ dropdown: durable presentation toggles, saved to the config as
/// they change and honoured by every list in the window — the live meter,
/// a stored fight, the comparison — and the chrome's colour.
fn options_panel(cfg: &crate::config::Config, accent: theme::Accent) -> Element<'static, Message> {
    // The chrome is a choice between two, so it is two chips side by side,
    // the pressed one lit — the prototype's "Game gold / Your class".
    let current = cfg.chrome();
    let chrome_chip = |label: &str, chrome: theme::Chrome| {
        mouse_area(nav::chip(label.to_string(), current == chrome, accent))
            .on_press(Message::SetChrome(chrome))
    };
    let panel = container(
        column![
            text("Options")
                .size(size::LABEL)
                .color(theme::GOLD_DIM)
                .font(theme::UI_SEMIBOLD),
            checkbox(cfg.show_ranks)
                .label("Row ranks")
                .on_toggle(Message::SetShowRanks)
                .size(16)
                .text_size(size::BODY),
            checkbox(cfg.hide_realms)
                .label("Hide realm names")
                .on_toggle(Message::SetHideRealms)
                .size(16)
                .text_size(size::BODY),
            text("Chrome").size(size::LABEL).color(theme::GOLD_DIM),
            row![
                chrome_chip("Game gold", theme::Chrome::Gold),
                chrome_chip("Your class", theme::Chrome::Class),
            ]
            .spacing(6),
        ]
        .spacing(8),
    )
    .padding(10)
    .style(|_: &Theme| nav::floating_style(8.0));
    // Anchored under the bar's gear (left of the help button), over
    // whichever screen is up; the wrapper itself is inert, but the panel
    // swallows presses so rows underneath don't fire through it, and the
    // pointer wandering off the panel dismisses it.
    container(
        mouse_area(panel)
            .on_press(Message::Noop)
            .on_exit(Message::CloseOptions),
    )
    .width(Length::Fill)
    .align_x(iced::Alignment::End)
    .padding(iced::Padding {
        top: pitch::TOP_BAR,
        right: 40.0,
        bottom: 0.0,
        left: 0.0,
    })
    .into()
}

/// Header badge for the watched segment (gui-logic's word, the overlay's
/// colour for its tone).
pub(crate) fn header_tag(app: &ClientState) -> (&'static str, Color) {
    let (word, tone) = labels::header_tag(app);
    let colour = match tone {
        labels::Tone::Live => YELLOW,
        labels::Tone::Good => GREEN,
        labels::Tone::Bad => RED,
        labels::Tone::None => DIM,
    };
    (word, colour)
}

/// A meter row's label with its realm off. R24: the enemy view's rows are
/// creatures, whose hyphen is their name's ("Yogg-Saron", "Blood-Queen
/// Lana'thel"), so only what reads as a player's "Name-Realm-Region" loses
/// anything there ([`realmless`]); every other view's rows are players.
fn meter_label(label: &str, enemies: bool) -> String {
    if enemies {
        realmless(label)
    } else {
        display_name(label).to_string()
    }
}

pub(crate) use wowdps_gui_logic::labels::{display_name, realmless, realmless_rows};

pub(crate) use wowdps_gui_logic::table::enemy_split;

/// R13: the line between the teams in a PvP chart. Message-generic like
/// `compare::class_icon`, so both surfaces can use it.
pub(crate) fn team_divider<M: 'static>(size: f32) -> Element<'static, M> {
    team_divider_in(&Look::OVERLAY, size)
}

/// [`team_divider`] in a surface's own [`Look`] — its bad-news red.
pub(crate) fn team_divider_in<M: 'static>(look: &Look, size: f32) -> Element<'static, M> {
    let red = look.bad;
    let line = move || {
        iced::widget::container(iced::widget::Space::new())
            .width(Length::Fill)
            .height(1)
            .style(move |_| iced::widget::container::Style {
                background: Some(iced::Background::Color(Color { a: 0.4, ..red })),
                ..Default::default()
            })
    };
    iced::widget::row![line(), text("enemy team").size(size).color(red), line(),]
        .spacing(8)
        .align_y(iced::Alignment::Center)
        .into()
}

/// Rank column width (window): two digits of the window's tabular figures
/// at `size::SMALL`, with air — the prototype's 26 px `.rk` less its gap.
const RANK_W: f32 = 22.0;
/// The same under 820 px, where the prototype's amount-and-rate grid
/// narrows its rank column by 4 (`.v-num4, .v-enemy{--cols:26px …}`): the
/// rank ends at 30 and the disc starts at 41 (the prototype's 30, 42).
const RANK_W_NARROW: f32 = RANK_W - 4.0;

/// The rank label drawn on a bar's left edge, ahead of the name: the row's
/// 1-based sort position, dim so the name still leads. Message-generic so
/// the overlay's rows can use it too (scaled).
pub(crate) fn rank_cell<M: 'static>(rank: usize, size: f32, width: f32) -> Element<'static, M> {
    text(rank.to_string())
        .size(size)
        .color(Color::WHITE)
        .font(Font::MONOSPACE)
        .width(Length::Fixed(width))
        .align_x(iced::Alignment::End)
        .into()
}

/// The window's rank column, `width` wide: the row's place in its tabular
/// figures, in the faint ink's text grade (`theme::INK_3_TEXT`) — the
/// prototype's `.rk` is INK_3, under AA on the selected row, and a rank is
/// read — so the name still leads; on the owner's row in the owner's text
/// colour, at 600 (`.trow.me .rk`: `--you-text`).
fn window_rank(rank: usize, width: f32, owner: Option<Color>) -> Element<'static, Message> {
    text(rank.to_string())
        .size(size::SMALL)
        .color(owner.unwrap_or(theme::INK_3_TEXT))
        .font(if owner.is_some() {
            theme::UI_SEMIBOLD
        } else {
            theme::UI
        })
        .width(Length::Fixed(width))
        .align_x(iced::Alignment::End)
        .into()
}

/// The meter list's scrollable, so the "you" chip can bring the owner's
/// row into view.
pub(crate) fn meter_list_id() -> iced::widget::Id {
    iced::widget::Id::new("meter-list")
}

/// The total row's id, so a test can find where it sits.
#[cfg(test)]
pub(crate) fn meter_total_id() -> iced::widget::Id {
    iced::widget::Id::new("meter-total")
}

/// The live meter as owned data, so [`meter_table`] can lay it out at
/// whatever size the window gives it: the prototype keeps only the amount
/// and the rate in a narrow window, and the total row sits under a short
/// list but pins to the bottom of a long one — and only the layout knows
/// either.
#[derive(Debug, Clone)]
struct MeterList {
    /// Every row in the daemon's order: ranks, the bar's scale and the
    /// total are the whole chart's.
    all: Vec<Row>,
    /// What is drawn, in the drawn order, each with the index it arrived
    /// with — the index a click sends back and the rank a row keeps —
    /// realms already off the labels when the option says so.
    drawn: Vec<(usize, Row)>,
    /// Each row's comparison slot, by daemon index: the meter's class icons
    /// are comparison picks. `None` for an attacker list, whose are not,
    /// and for a stored pull, which keeps no comparison.
    slots: Option<Vec<Option<usize>>>,
    selected: usize,
    hover: Option<usize>,
    view: View,
    sort: Option<(table::Col, bool)>,
    show_ranks: bool,
    /// R13: where the enemy team's block starts, in the daemon's order.
    split: Option<usize>,
    /// R24: the enemy view's meter, whose rows wear the skull disc.
    enemies: bool,
    /// The window is narrow (820 px and under): two columns, the amount
    /// and the rate — set by the layout, which knows the window's width.
    narrow: bool,
    /// A click on a row pushes the inspector over the meter as well as
    /// selecting (a narrow window, where the inspector is not beside it).
    push: bool,
    /// A row's pitch, by the configured density.
    row_h: f32,
    /// The owner's row, by daemon index, and their class: it wears the
    /// "you" tag and its rank in their colour.
    owner: Option<(usize, Option<Class>)>,
    /// The keys are in the inspector: the selection steps back to the
    /// hover's weight.
    keys_away: bool,
    /// A comparison on the live pull holds the meter's rows as they stood
    /// when the pair formed (its cursor carries none): the heading says so.
    paused: bool,
    /// A filter narrows what is drawn: the total folds the drawn rows.
    filtered: bool,
}

impl MeterList {
    /// The live meter, as the window's filter, sort and options draw it.
    fn meter(state: &Gui) -> Self {
        let app = state.fight();
        let all = app.rows();
        let enemies = app.view == View::EnemyTaken;
        let hide_realms = state.cfg.hide_realms;
        // The sort by a column this view's table has; another view's
        // choice is the daemon's order here (`Gui::meter_sort`).
        let sort = state.meter_sort();
        // Filtering and sorting change what is DRAWN and in what order,
        // never what a row's numbers mean: the scale, the ranks, the shares
        // and the click targets all stay the whole chart's. Only the total
        // follows the filter — it is the drawn rows' fold, as the
        // prototype's is. The filter matches the full label; the realm
        // comes off the drawn copy only.
        let drawn = ordered(all.clone(), &state.filter, sort)
            .into_iter()
            .map(|(i, r)| {
                let mut shown = if hide_realms {
                    Row {
                        label: meter_label(&r.label, enemies),
                        ..r
                    }
                } else {
                    r
                };
                // R24: the hostile tint, on the drawn copy only (`bar_color`).
                shown.enemy |= enemies;
                (i, shown)
            })
            .collect();
        Self {
            // A stored pull keeps no comparison to pick for: its icons are
            // plain, and a press on one selects the row like the rest of it.
            slots: state
                .stored
                .is_none()
                .then(|| all.iter().map(|r| app.compare_slot(&r.key)).collect()),
            // R13: a sort interleaves the teams, so the divider only makes
            // sense in the daemon's grouped order.
            split: sort.is_none().then(|| enemy_split(&all)).flatten(),
            owner: state
                .owner_in(&all)
                .map(|i| (i, all.get(i).and_then(|r| r.class))),
            all,
            drawn,
            selected: app.row_sel,
            hover: state.hover_meter(),
            view: app.view,
            sort,
            show_ranks: state.cfg.show_ranks,
            enemies,
            narrow: false,
            push: false,
            row_h: state.cfg.density().row_h(),
            filtered: !state.filter.is_empty(),
            keys_away: app.inspecting(),
            paused: app.screen == Screen::Compare && app.is_live(),
        }
    }

    /// How tall the rows stand, all of them: the pitch of each, the team
    /// divider, the words an empty list says instead.
    fn rows_h(&self) -> f32 {
        let divider = self
            .split
            .filter(|s| self.drawn.iter().any(|(i, _)| i >= s))
            .map_or(0.0, |_| DIVIDER_H);
        let empty = if self.drawn.is_empty() { EMPTY_H } else { 0.0 };
        self.drawn.len() as f32 * self.row_h + divider + empty
    }

    /// Where the daemon row `row` stands in the list's content, top and
    /// bottom: its place in the drawn order at the row pitch, below the
    /// team divider when it is past it. `None` when it is not drawn.
    fn extent(&self, row: usize) -> Option<(f32, f32)> {
        let at = self.drawn.iter().position(|(i, _)| *i == row)?;
        let divider = self
            .split
            .filter(|s| row >= *s && self.drawn.iter().any(|(i, _)| i >= s))
            .map_or(0.0, |_| DIVIDER_H);
        let top = at as f32 * self.row_h + divider;
        Some((top, top + self.row_h))
    }
}

/// Where the live meter's row `row` (the daemon's index) stands in its
/// list's content, top and bottom — what keeps a stepped selection in
/// sight. `None` when the meter's list is not on screen or the row is not
/// drawn.
pub(crate) fn meter_row_extent(state: &Gui, row: usize) -> Option<(f32, f32)> {
    let covered = state.talents.is_some() || state.home.is_some();
    // The meter stands beside the inspector (and under a comparison);
    // only a narrow window's pushed inspector hides it, and a scroll of a
    // list not drawn is nothing.
    if covered || state.fight().screen == Screen::List {
        return None;
    }
    // R25: on the Deaths view the list is the deaths, in the order they
    // happened: the row's player stands where their recapped death does.
    if let Some(table) = crate::deaths::Table::of(state) {
        let key = state.fight().rows().get(row)?.key.clone();
        return table.extent_of(&key);
    }
    MeterList::meter(state).extent(row)
}

/// The meter's table — the headings, the rows and the total — laid out at
/// the height it is given (the width is the window's business: `narrow`).
fn meter_table(list: MeterList) -> Element<'static, Message> {
    iced::widget::responsive(move |bounds| meter_table_at(&list, list.narrow, bounds.height)).into()
}

/// The heading line over the meter (`.thead{padding:8px … 6px}`): one line
/// of gold-dim 13.5 px and its padding.
const HEADS_PAD: iced::Padding = iced::Padding {
    top: 8.0,
    right: 0.0,
    bottom: 6.0,
    left: 0.0,
};
const HEADS_H: f32 = 8.0 + 6.0 + size::LABEL * 1.3;
/// The room between the heading over the names and the note after it.
const PAUSED_GAP: f32 = 10.0;
/// The team divider's line (R13), and the words an empty list shows, as
/// tall as they stand with a little to spare: a list is taken for short
/// only when it certainly is.
const DIVIDER_H: f32 = 20.0;
const EMPTY_H: f32 = 24.0;

/// Does a list of `rows_h` fit under the headings with the total row
/// straight after it, in `height`? Then the total sits under the last row;
/// otherwise it pins to the bottom and the rows scroll above it. iced has
/// no sticky positioning, so the layout's height is measured instead
/// (`responsive`).
pub(crate) fn total_follows(rows_h: f32, height: f32) -> bool {
    HEADS_H + 1.0 + rows_h + pitch::TOTAL <= height
}

fn meter_table_at(l: &MeterList, narrow: bool, height: f32) -> Element<'static, Message> {
    let cols = table::meter_set(l.view, narrow);
    // The prototype's own grid for the view, narrowed as it narrows.
    let grid = table::Grid::Meter {
        view: l.view,
        narrow,
    };
    // Under 820 px the amount-and-rate grid's rank column narrows too
    // (`.v-num4, .v-enemy{--cols:26px …}`); a count keeps its 30.
    let rank_cell = if cols == table::METER_NARROW {
        RANK_W_NARROW
    } else {
        RANK_W
    };
    // The heading over the names starts where the names do: "Player", or
    // "Enemy" over the enemy view's creatures. The rest are the numbers',
    // and sort.
    let rank_w = if l.show_ranks { rank_cell + 6.0 } else { 0.0 };
    // The live meter runs edge to edge, its rows inset by the prototype's
    // own lead (`.trow{padding-left:4px}` and a 30 px rank column).
    let row_lead = ROW_LEAD;
    // Where the icon column starts (`.who2`): the heading over the names
    // and the total's label start there too, as the prototype's do.
    let who = row_lead + rank_w + ICON_X;
    let mut heading = row![
        text(if l.enemies { "Enemy" } else { "Player" })
            .size(size::LABEL)
            .color(theme::GOLD_DIM)
            .wrapping(text::Wrapping::None),
    ]
    .spacing(PAUSED_GAP);
    if l.paused {
        // The pull goes on while the pair is up, and these rows do not:
        // said where the eye starts down the list.
        heading = heading.push(
            text("paused while comparing")
                .size(size::LABEL)
                .color(theme::INK_3_TEXT)
                .wrapping(text::Wrapping::None),
        );
    }
    let lead = row![
        Space::new().width(Length::Fixed((who - HEAD_PAD).max(0.0))),
        heading,
    ];
    let heads = container(table::heads(
        cols,
        grid,
        l.view,
        l.sort,
        Some(Message::SortBy),
        lead,
    ))
    .padding(HEADS_PAD);
    let max = l.all.iter().map(|r| r.amount).max().unwrap_or(1);
    let mut list = column![];
    if l.drawn.is_empty() {
        list = list.push(
            text("nothing to show for this view yet")
                .size(size::BODY)
                .color(theme::INK_2),
        );
    }
    let mut divided = false;
    for (i, r) in &l.drawn {
        let i = *i;
        // R13: the teams are grouped; mark where the enemy block starts —
        // before the first enemy row STILL DRAWN, because a filter that hid
        // the row at the boundary must not also hide the boundary.
        if !divided && l.split.is_some_and(|s| i >= s) {
            divided = true;
            list = list.push(team_divider_in(&Look::WINDOW, size::MICRO));
        }
        // R12: the class icon is the pick target, the rest of the row still
        // drills — two different questions, two different hit areas. R24:
        // an enemy row wears the skull disc, and is never a pick. The pin
        // is said after the name ("A"), never as a ring: the selection is
        // the pair's other half, and the raised row already says so.
        let icon: Element<'static, Message> = match &l.slots {
            _ if l.enemies => compare::enemy_icon(None, 18.0),
            Some(_) => mouse_area(compare::class_icon(r.class, r.spec, None, 18.0))
                .on_press(Message::CompareRow(i))
                .into(),
            None => compare::class_icon(r.class, r.spec, None, 18.0),
        };
        // R12: the pair's halves by their slot — the pin "A", the one it
        // is compared with "B".
        let slot = l.slots.as_ref().and_then(|s| s.get(i).copied().flatten());
        let mine = l.owner.filter(|(o, _)| *o == i).map(|(_, class)| class);
        let bar = container(bar_row_tagged(
            r,
            max,
            i == l.selected,
            l.row_h,
            Some(cols),
            grid,
            1.0,
            None,
            Some(icon),
            name_tags(r.spec.map(|s| s.role()), mine, slot),
        ));
        // The rank sits OUTSIDE the bar, far left, the way a raid roster
        // numbers its slots; the icon rides the bar's leading edge. The
        // hover mark and the click cover the WHOLE line, rank included.
        let mut line = row![].spacing(6).align_y(iced::Alignment::Center);
        if l.show_ranks {
            line = line.push(window_rank(
                i + 1,
                rank_cell,
                mine.map(|class| class.map_or(theme::INK, theme::you_text)),
            ));
        }
        // The selection raises the WHOLE line, rank included (`.trow.sel`);
        // the hover's breath is for the other rows. While the keys are in
        // the inspector the selection steps back to the hover's weight, so
        // the list that answers j/k is the one that looks it.
        let hovered = l.hover == Some(i);
        let selected = i == l.selected;
        let quiet = l.keys_away;
        let line = container(line.push(bar))
            .padding(iced::Padding {
                left: row_lead,
                ..iced::Padding::ZERO
            })
            .style(move |_: &Theme| {
                if selected && quiet {
                    hover_style_in(&Look::WINDOW, true)
                } else if selected {
                    row_style_in(&Look::WINDOW, true)
                } else {
                    hover_style_in(&Look::WINDOW, hovered)
                }
            });
        // A click selects the row, and the inspector follows it; in a
        // narrow window, where the inspector is not beside the meter, the
        // click pushes it too.
        let press = if l.push {
            Message::PushRow(i)
        } else {
            Message::MeterRow(i)
        };
        list = list.push(
            mouse_area(line)
                .on_press(press)
                .on_enter(Message::HoverRow(Some(RowHover::Meter(i))))
                .on_exit(Message::HoverRow(None)),
        );
    }
    // A short list takes its own height and the total follows its last
    // row; a long one takes what is left and scrolls, the total pinned
    // under it.
    let follows = total_follows(l.rows_h(), height);
    let scroller = scrollable(scroll_clear(list))
        .id(meter_list_id())
        .height(if follows {
            Length::Shrink
        } else {
            Length::Fill
        })
        .width(Length::Fill);
    // R12: right-click clears a lone half-pick (the badged icon) without
    // touching the drill or the selection. The row areas only claim left
    // presses, so the right press reaches this wrapper.
    let rows: Element<'static, Message> = mouse_area(scroller)
        .on_right_press(Message::ClearCompare)
        .into();
    // The total: the same columns, so a per-player number always has its
    // denominator on screen. Enemy rows (R13) are left out — the fold is
    // OUR team's — and so is whatever a filter hides: narrowed, the total
    // is of the rows drawn, and their count is the one it names. Its label
    // starts where the heading over the names does.
    let ours: Vec<Row> = if l.filtered {
        l.drawn
            .iter()
            .filter_map(|(i, _)| l.all.get(*i))
            .filter(|r| !r.enemy)
            .cloned()
            .collect()
    } else {
        l.all.iter().filter(|r| !r.enemy).cloned().collect()
    };
    let label = match (l.enemies, ours.len()) {
        (true, 1) => "Total, 1 enemy".to_string(),
        (true, n) => format!("Total, {n} enemies"),
        (false, n) => format!("Total, {}", nav::plural(n, "player")),
    };
    let lead_pad = (who - TOTAL_LEAD).max(0.0);
    // The live meter's total is the prototype's, the stage's width under
    // the list and its scrollbar lane.
    let total = container(table::total(cols, grid, &ours, label, lead_pad, l.filtered));
    #[cfg(test)]
    let total = total.id(meter_total_id());
    column![
        // The list sits inside `scroll_clear`'s scrollbar gutter, so the
        // headings wear the same gutter to keep columns, and the total
        // keeps it inside its full-width surface.
        scroll_clear(heads),
        nav::hairline::<Message>(),
        rows,
        total,
    ]
    .height(Length::Fill)
    .into()
}

/// What follows a name on its line (`.who2`): the comparison's halves — a
/// gold-dim "A" on the pinned player (`.trow.pin .nm::after{content:"  A";
/// color:var(--gold-dim);font-size:12px;font-weight:600}`) and a "B" on the
/// one it is compared with, the legend's two names, each saying so under
/// the pointer — then a tank's shield or a healer's cross in faint ink
/// (`.role`), then the owner's "you" tag — `you` is `Some(their class)` on
/// the owner's row. Each is a fixed width, and the name leaves room for
/// them all, so they stay beside it.
pub(crate) fn name_tags(
    role: Option<Role>,
    you: Option<Option<Class>>,
    slot: Option<usize>,
) -> Option<(Element<'static, Message>, f32)> {
    let glyph = match role {
        Some(Role::Tank) => Some(LineIcon::Shield),
        Some(Role::Healer) => Some(LineIcon::Cross),
        Some(Role::Dps) | None => None,
    };
    let mut tags = row![].spacing(TAG_GAP).align_y(iced::Alignment::Center);
    let mut widths = Vec::new();
    let half = match slot {
        Some(0) => Some(("A", "Pinned for comparison (v to stop)")),
        Some(1) => Some(("B", "Compared with the pinned player")),
        _ => None,
    };
    if let Some((letter, tip)) = half {
        tags = tags.push(crate::nav::tip(
            text(letter)
                .size(PIN_PX)
                .font(theme::UI_SEMIBOLD)
                .color(theme::GOLD_DIM)
                .width(Length::Fixed(PIN_W))
                .wrapping(text::Wrapping::None),
            tip,
        ));
        widths.push(PIN_W);
    }
    if let Some(glyph) = glyph {
        tags = tags.push(crate::line_icons::line_icon(
            glyph,
            ROLE_GLYPH,
            theme::INK_3,
        ));
        widths.push(ROLE_GLYPH);
    }
    if let Some(class) = you {
        tags = tags.push(you_tag(class));
        widths.push(YOU_TAG_W);
    }
    (!widths.is_empty()).then(|| {
        let gaps = TAG_GAP * (widths.len() - 1) as f32;
        (tags.into(), widths.iter().sum::<f32>() + gaps)
    })
}

/// The pair's "A" and "B": their size and the width each is given.
const PIN_PX: f32 = 12.0;
const PIN_W: f32 = 9.0;

/// A role glyph (`.role svg`), the "you" tag's width and the gap between
/// what follows a name.
const ROLE_GLYPH: f32 = 13.0;
pub(crate) const YOU_TAG_W: f32 = 26.0;
const TAG_GAP: f32 = 8.0;
/// The tag's line and corners (`.youtag{line-height:15px;border-radius:4px}`).
const YOU_TAG_LINE: f32 = 15.0;
const YOU_TAG_RADIUS: f32 = 4.0;

/// The owner's tag (`.youtag`): "you" at 11.5 px, 600, in the owner's text
/// colour (`--you-text`, AA on the selected row's RAISE where a player's
/// name colour is not), on a 1 px frame of their class colour.
pub(crate) fn you_tag(class: Option<Class>) -> Element<'static, Message> {
    let raw = class.map_or(theme::INK_3, theme::class_rgb);
    container(
        text("you")
            .size(size::YOU_TAG)
            .font(theme::UI_SEMIBOLD)
            .color(class.map_or(theme::INK_2, theme::you_text))
            .line_height(text::LineHeight::Absolute(YOU_TAG_LINE.into()))
            .wrapping(text::Wrapping::None),
    )
    .width(Length::Fixed(YOU_TAG_W))
    .align_x(iced::Alignment::Center)
    .style(move |_: &Theme| container::Style {
        border: Border {
            color: Color {
                a: theme::YOU_TAG_EDGE,
                ..raw
            },
            width: 1.0,
            radius: YOU_TAG_RADIUS.into(),
        },
        ..container::Style::default()
    })
    .into()
}

/// Where a meter row's class icon starts inside its bar: the track's
/// lead-in.
const ICON_X: f32 = 5.0;
/// The live meter's rows, inset from the stage's left edge: the
/// prototype's `.trow{padding-left:4px}` and 30 px rank column, less the
/// 22 px rank cell and its 6 px gap here — so the rank ends at 34 and the
/// disc starts at 45, where the prototype's do (34, 46).
const ROW_LEAD: f32 = 12.0;
/// The heading line's own inset (`table::heads`' padding).
const HEAD_PAD: f32 = 8.0;
/// Where the total row's label would start with no lead-in: its padding
/// and the gap after the lead-in.
const TOTAL_LEAD: f32 = 8.0 + table::GAP;

pub(crate) use wowdps_gui_logic::drill::{drill_mitigation_line, school_name};
pub(crate) use wowdps_gui_logic::labels::rate_label;
pub(crate) use wowdps_model::fmt::mitigation_line;

/// The live meter's heading line as its table draws it in a wide window
/// (`table::meter_set`, on the prototype's grid) — for the tests that
/// read the headings on their own.
#[cfg(test)]
fn meter_captions(
    app: &ClientState,
    sort: Option<(table::Col, bool)>,
) -> Element<'static, Message> {
    let lead = row![Space::new().width(Length::Fill)];
    table::heads(
        table::meter_set(app.view, false),
        table::Grid::Meter {
            view: app.view,
            narrow: false,
        },
        app.view,
        sort,
        Some(Message::SortBy),
        lead,
    )
}

/// One class-colored bar with its labels on top. The bar's width is the
/// row's amount relative to `max`, the list's top amount ([`class_bar`]).
/// `cols` of `None` is compact: the amount alone. `tags` follow the name —
/// the meter's role glyph and "you" tag — with their width, which the name
/// leaves room for: a long name ends in "…" before a tag is pushed off its
/// line. Emits no messages, so it serves any frontend's message type.
#[allow(clippy::too_many_arguments)]
pub(crate) fn bar_row_tagged<M: 'static>(
    r: &Row,
    max: u64,
    selected: bool,
    height: f32,
    cols: Option<&[table::Col]>,
    // The widths and gap `cols` stand at: the live meter's own grid, or an
    // inspector list's.
    grid: table::Grid,
    scale: f32,
    rank: Option<usize>,
    icon: Option<Element<'static, M>>,
    tags: Option<(Element<'static, M>, f32)>,
) -> Element<'static, M> {
    let bar = class_bar(r, max, selected);

    let mut labels = row![].spacing(10.0 * scale);
    // The rank rides on the bar itself, ahead of the name, so the bar can
    // hug the class icon.
    if let Some(rank) = rank {
        labels = labels.push(rank_cell(rank, 11.0 * scale, RANK_W * scale));
    }
    if let Some(icon) = icon {
        labels = labels.push(icon);
    }
    // v9: only by-spell drill rows carry a spell id; meter rows are players
    // (id 0), so this never fires for them.
    if let Some(h) = crate::spell_icons::handle(r.spell_id) {
        labels = labels.push(
            iced::widget::image(h)
                .width(Length::Fixed(17.0 * scale))
                .height(Length::Fixed(17.0 * scale)),
        );
    }
    // Fill + NoWrap inside a clipping container: NoWrap alone keeps the text
    // on one line but iced still PAINTS the overflow, which is how a long
    // "Spell (Pet Name)" label used to run under the number columns.
    let name = crate::ellipsis::ellipsis(r.label.clone())
        .size(size::NAME * scale)
        .color(name_ink(selected))
        .font(if selected {
            theme::UI_MEDIUM
        } else {
            theme::UI
        });
    let labels = match tags {
        // The name as wide as it reads, the tags right after it, and the
        // rest of the track empty. The row's gaps are taken off before any
        // piece is laid out, so the name leaves room for the tags alone.
        Some((tags, width)) => labels
            .push(name.leaving(Vec::new(), width))
            .push(tags)
            .push(Space::new().width(Length::Fill)),
        None => labels.push(container(name).clip(true).width(Length::Fill)),
    }
    .align_y(iced::Alignment::Center)
    .height(Length::Fill);

    let Some(cols) = cols else {
        // Half a window wide: there is no room for a separate amount column,
        // so the drill panes keep the older shape — the amount beside the
        // name, over the bar.
        let (primary, _, _) = Look::WINDOW.metrics;
        let labels = labels
            .push(
                text(human(r.amount))
                    .size(size::NUM * scale)
                    .color(primary)
                    .font(theme::UI_MEDIUM),
            )
            .padding([0.0, 8.0 * scale]);
        return under_bar(&Look::WINDOW, bar, labels, height, scale, selected);
    };

    // The window row: name and number columns over the bar, which runs
    // under the WHOLE row — name and number columns alike.
    let labels = container(labels)
        .padding(track_pad(scale))
        .width(Length::Fill)
        .height(Length::Fill);
    let metrics = table::cells::<M>(cols, grid, r, scale, false);
    let content = row![labels, metrics]
        .spacing(grid.gap() * scale)
        // No left padding: the bar abuts the margin (or the rank cell)
        // so the fill reads from the row's very edge.
        .padding(iced::Padding {
            top: 0.0,
            right: 8.0 * scale,
            bottom: 0.0,
            left: 0.0,
        })
        .align_y(iced::Alignment::Center);
    under_bar(&Look::WINDOW, bar, content, height, scale, selected)
}

/// Every bar in every list is this shape: a NARROW bar UNDER the row's
/// text, the text on the panel in its own ink. A fill behind the text put
/// every name and number on its class color and made them fight it.
pub(crate) const BAR_H: f32 = 3.0;

/// A row laid out as [`BAR_H`] says: `content` over `bar`, clipped to
/// `height`, selected or not in `look`'s own fill. `scale` is the
/// overlay's manual zoom.
fn under_bar<M: 'static>(
    look: &Look,
    bar: Element<'static, M>,
    content: impl Into<Element<'static, M>>,
    height: f32,
    scale: f32,
    selected: bool,
) -> Element<'static, M> {
    container(
        column![
            container(content).height(Length::Fill).width(Length::Fill),
            container(bar)
                .height(Length::Fixed(BAR_H * scale))
                .width(Length::Fill)
                .style(|_: &Theme| container::Style {
                    background: Some(theme::TRACK.into()),
                    border: iced::border::rounded(2),
                    ..container::Style::default()
                }),
        ]
        .spacing(1.0 * scale),
    )
    .clip(true)
    .height(height)
    .width(Length::Fill)
    .style({
        let look = *look;
        move |_: &Theme| row_style_in(&look, selected)
    })
    .into()
}

/// Inside the bar's track: the name starts where the caption's "player"
/// heading does.
fn track_pad(scale: f32) -> iced::Padding {
    iced::Padding {
        top: 0.0,
        right: 8.0 * scale,
        bottom: 0.0,
        // Tight against the bar's edge: the icon sits here now, and it
        // wants the fill's color behind it, not a gutter.
        left: 5.0 * scale,
    }
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
    let bar = class_bar(r, max, false);

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
    let (primary, secondary, tertiary) = Look::OVERLAY.metrics;

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

    under_bar(&Look::OVERLAY, bar, labels, height, scale, false)
}

/// The overlay drill's grid: its column widths and its caret's (gui-logic).
pub(crate) use wowdps_gui_logic::drill::{OVERLAY_CARET_W, OVERLAY_DRILL_COLS};

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
    recap_row_in(&Look::OVERLAY, r, max, height, scale, compact)
}

/// [`recap_row`] in a surface's own [`Look`]: the window's heals green and
/// hits red are the tokens', its numbers tabular. Realms are the caller's:
/// the window strips them from the label before it gets here when the
/// option says so (`compact` strips them for the overlay's narrow panel).
pub(crate) fn recap_row_in<M: 'static>(
    look: &Look,
    r: &Row,
    max: u64,
    height: f32,
    scale: f32,
    compact: bool,
) -> Element<'static, M> {
    let fit = if compact {
        RecapFit::Compact
    } else {
        RecapFit::Wide
    };
    recap_row_at(look, r, max, height, scale, fit)
}

/// How a recap line lays its words out.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RecapFit {
    /// The overlay's narrow panel: the source's realm stripped, the label
    /// laid out as it always was.
    Compact,
    /// The window: the label clipped to what is left of the line, so the
    /// fixed cells — overkill, amount, health — always keep their widths.
    Wide,
}

/// [`recap_row_in`] fitted: see [`RecapFit`].
fn recap_row_at<M: 'static>(
    look: &Look,
    r: &Row,
    max: u64,
    height: f32,
    scale: f32,
    fit: RecapFit,
) -> Element<'static, M> {
    let compact = fit == RecapFit::Compact;
    let (color, alpha) = if r.gain {
        (look.good, 0.30)
    } else if r.extra > 0 {
        (look.bad, 0.55)
    } else {
        (look.bad, 0.30)
    };
    let fill = drill::whole_pct(r.amount, max);
    let event_bar = part_bar(Color { a: alpha, ..color }, fill);

    // The HP strip: a faint track with the remaining-health fraction lit.
    let hp_strip: Element<'static, M> = match r.hp {
        Some((cur, max_hp)) => {
            let pct = drill::whole_pct(cur, max_hp);
            container(part_bar(
                Color {
                    a: 0.55,
                    ..look.health
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
            .font(look.num)
            .width(Length::Fixed(width * scale))
            .align_x(iced::Alignment::End)
    };
    let amount = metric(
        drill::recap_amount(r),
        12.0,
        if r.gain { look.good } else { look.hit },
        52.0,
    );
    let hp = metric(drill::recap_hp(r), 11.0, look.dim, 40.0);
    let labels = if compact {
        // The overlay is narrow: strip realm suffixes from the attacker/
        // healer in parens, like the meter rows do for player names.
        let label = drill::compact_recap_label(&r.label);
        row![text(label).size(12.0 * scale)]
            .spacing(4)
            .padding([0, 8])
            .align_y(iced::Alignment::Center)
            .height(Length::Fill)
            .push(Space::new().width(Length::Fill))
            .push(amount)
            .push(hp)
    } else {
        // The label is the flexible part, ellipsised in its Fill container:
        // unclipped, a long "Spell (Source)" shoved the fixed cells past the
        // line's end, and the amount landed on the health.
        let mut labels = row![
            container(crate::ellipsis::ellipsis(r.label.clone()).size(13.0 * scale))
                .clip(true)
                .width(Length::Fill)
        ]
        .spacing(4)
        .padding([0, 8])
        .align_y(iced::Alignment::Center)
        .height(Length::Fill);
        // A killing blow's overkill, in the recap's bad-news red (`.over`).
        if fit == RecapFit::Wide && r.extra > 0 && !r.gain {
            labels = labels.push(metric(
                format!("({} over)", human(r.extra)),
                11.0,
                look.bad,
                72.0,
            ));
        }
        labels.push(amount).push(hp)
    };

    let look = *look;
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
    .style(move |_: &Theme| row_style_in(&look, false))
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
    overlay_drill_line(r, Lead::Flat, &r.label, 0.0, max, height, scale, count_only)
}

/// R26: what leads a line of the overlay's drill. A flat drill leads with
/// nothing, exactly as it always drew; a tree drill keeps a column for the
/// fold caret and one for the ability's art on EVERY line, so a group's
/// name and a lone row's start together and a melee row, which has no art,
/// does not jump left of its siblings.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Lead {
    Flat,
    /// A row of a tree drill: both columns, the caret's left blank.
    Row,
    /// A group of a tree drill, its caret open or shut.
    Group {
        open: bool,
    },
}

/// R26: one line of the overlay drill's ability tree — `overlay_drill_row`
/// with the line's own words, `indent` px further in at scale 1 (a group's
/// rows sit under its name) and its `lead`. `Lead::Flat` with no indent
/// and the row's own label draws exactly what `overlay_drill_row` always
/// drew.
#[allow(clippy::too_many_arguments)]
pub(crate) fn overlay_drill_line<M: 'static>(
    r: &Row,
    lead: Lead,
    label: &str,
    indent: f32,
    max: u64,
    height: f32,
    scale: f32,
    count_only: bool,
) -> Element<'static, M> {
    let bar = class_bar(r, max, false);
    let metric = |s: String, size: f32, color: Color, width: f32| {
        text(s)
            .size(size * scale)
            .color(color)
            .font(Font::MONOSPACE)
            .width(Length::Fixed(width * scale))
            .align_x(iced::Alignment::End)
    };
    let (w_hits, w_crit, w_total) = OVERLAY_DRILL_COLS;
    let mut labels = row![].spacing(4).padding(iced::Padding {
        top: 0.0,
        right: 8.0,
        bottom: 0.0,
        left: 8.0 + indent * scale,
    });
    // R26: a group's caret leads its line, in the quiet ink the ability
    // drill's own "▸" wears; a tree drill's other lines keep its column.
    let caret = match lead {
        Lead::Flat => None,
        Lead::Row => Some(""),
        Lead::Group { open: true } => Some("▾"),
        Lead::Group { open: false } => Some("▸"),
    };
    if let Some(c) = caret {
        labels = labels.push(
            text(c)
                .size(11.0 * scale)
                .color(Look::OVERLAY.faint)
                .width(Length::Fixed(OVERLAY_CARET_W * scale)),
        );
    }
    // v9: by-spell rows carry their spell id — the ability's own art leads
    // the label when the spell-icon cache knows it.
    if let Some(h) = crate::spell_icons::handle(r.spell_id) {
        labels = labels.push(
            iced::widget::image(h)
                .width(Length::Fixed(12.0 * scale))
                .height(Length::Fixed(12.0 * scale)),
        );
    } else if lead != Lead::Flat {
        labels = labels.push(Space::new().width(Length::Fixed(12.0 * scale)));
    }
    // Fill + NoWrap inside a clipping container: without the clip, iced
    // paints the one-line overflow straight under the hits/crit/total
    // columns (see `bar_row_tagged`).
    let mut labels = labels
        .push(
            container(
                text(label.to_string())
                    .size(12.0 * scale)
                    .wrapping(text::Wrapping::None),
            )
            .clip(true)
            .width(Length::Fill),
        )
        .align_y(iced::Alignment::Center)
        .height(Length::Fill);
    let (primary, secondary, _) = Look::OVERLAY.metrics;
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

    under_bar(&Look::OVERLAY, bar, labels, height, scale, false)
}

/// v15: the colour for a school bitmask (gui-logic's table, as iced's colour).
pub(crate) fn school_color(mask: u32) -> Option<Color> {
    wowdps_gui_logic::theme::school_color(mask).map(|x| Color::from_rgba(x.r, x.g, x.b, x.a))
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
    // "Keanucleavês-Proudmoore-US" → "Keanucleavês": the overlay's crumb
    // is the name alone.
    let name = player.split('-').next().unwrap_or(player);
    spell_breadcrumb_in(&Look::OVERLAY, name, None, spell_label, spell_row, scale)
}

/// [`spell_breadcrumb`] in a surface's own [`Look`]. `player` is drawn as
/// given — the caller has taken the realm off when it should — in
/// `player_ink` when the surface names people in their colours (the
/// window: the player's class, lifted), else in the look's focus (the
/// overlay's yellow). The ability's name wears its school's colour only
/// where the look says so ([`Look::school_names`]); the school tag always
/// does.
pub(crate) fn spell_breadcrumb_in<M: 'static>(
    look: &Look,
    player: &str,
    player_ink: Option<Color>,
    spell_label: &str,
    spell_row: Option<&Row>,
    scale: f32,
) -> Element<'static, M> {
    let color = if look.school_names {
        spell_row
            .and_then(|r| school_color(r.school))
            .unwrap_or(look.ink)
    } else {
        look.ink
    };
    let mut line = row![
        text(player.to_string())
            .size(13.0 * scale)
            .color(player_ink.unwrap_or(look.focus)),
        text("▸").size(11.0 * scale).color(look.faint),
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
        let sc = sc.unwrap_or(look.dim);
        line = line.push(
            container(text(name).size(look.caption * scale).color(sc))
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

/// v16: the ability drill's stat strip — the numbers its by-spell row
/// already carried but the table never showed, each in its own card:
/// total, share of the player, hits, crit rate, average hit, the school,
/// and the view's `extra` (overkill/overheal) when there is any.
pub(crate) fn spell_stats<M: 'static>(r: &Row, view: View, scale: f32) -> Element<'static, M> {
    spell_stats_in(&Look::OVERLAY, r, view, scale)
}

/// [`spell_stats`] in a surface's own [`Look`]: the window's crit rate is
/// plain ink and its cards the token panels, where the overlay's crit is
/// yellow.
pub(crate) fn spell_stats_in<M: 'static>(
    look: &Look,
    r: &Row,
    view: View,
    scale: f32,
) -> Element<'static, M> {
    let (fill, edge) = look.card;
    // FillPortion: the cards SHARE the panel's width instead of demanding
    // their own — six of them always fit, at any zoom, with no scrollbar.
    let card = |label: &'static str, value: String, accent: Option<Color>| {
        container(
            column![
                text(value)
                    .size(13.0 * scale)
                    .color(accent.unwrap_or(look.ink))
                    .font(look.num),
                text(look.word(label))
                    .size(look.caption * scale)
                    .color(look.label),
            ]
            .spacing(2)
            .width(Length::Fill)
            .align_x(iced::Alignment::Center),
        )
        .width(Length::FillPortion(1))
        .padding([5.0 * scale, 4.0 * scale])
        .style(move |_: &Theme| container::Style {
            background: Some(fill.into()),
            border: Border {
                color: edge,
                width: 1.0,
                radius: 5.into(),
            },
            ..container::Style::default()
        })
    };
    let mut line = row![].spacing(6.0 * scale);
    for c in drill::stat_cards(r, view) {
        let accent = match c.tone {
            drill::StatTone::Plain => None,
            drill::StatTone::Crit => Some(look.crit),
            drill::StatTone::Bad => Some(look.bad),
        };
        line = line.push(card(c.label, c.value, accent));
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
    spell_target_list_in(&Look::OVERLAY, rows, height, scale)
}

/// [`spell_target_list`] in a surface's own [`Look`]. The labels are drawn
/// as given: the window strips realms from them first when it should.
pub(crate) fn spell_target_list_in<M: 'static>(
    look: &Look,
    rows: &[Row],
    height: f32,
    scale: f32,
) -> Element<'static, M> {
    let mut list = column![].spacing(2);
    if rows.is_empty() {
        list = list.push(text("no data yet").size(12.0 * scale).color(look.dim));
    }
    let max = rows.first().map_or(1, |r| r.amount.max(1));
    for r in rows {
        list = list.push(spell_target_row(look, r, max, height, scale));
    }
    scrollable(scroll_clear(list))
        .height(Length::Fill)
        .width(Length::Fill)
        .into()
}

/// One target row: name over a school-tinted bar, hits · amount · share.
fn spell_target_row<M: 'static>(
    look: &Look,
    r: &Row,
    max: u64,
    height: f32,
    scale: f32,
) -> Element<'static, M> {
    let bar = class_bar(r, max, false);
    let metric = |s: String, size: f32, color: Color, width: f32| {
        text(s)
            .size(size * scale)
            .color(color)
            .font(look.num)
            .width(Length::Fixed(width * scale))
            .align_x(iced::Alignment::End)
    };
    let (primary, secondary, tertiary) = look.metrics;
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
    under_bar(look, bar, labels, height, scale, false)
}

/// The color a row's bar wears: its spell school (v15, drill rows), else the
/// player's class, else the classless gray.
fn bar_color(r: &Row) -> Color {
    if let Some(c) = school_color(r.school) {
        return c;
    }
    // R24: a classless row flagged `enemy` at render time is an enemy-view
    // row (the meter sets the flag on its drawn copy, never on the model's
    // row, which stays team-less by the ruling).
    if r.enemy && r.class.is_none() {
        return HOSTILE;
    }
    match r.class {
        Some(c) => {
            let (cr, cg, cb) = c.rgb();
            Color::from_rgb8(cr, cg, cb)
        }
        None => CLASSLESS,
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
/// `lit` is the selection: the same bar at full strength.
fn class_bar<M: 'static>(r: &Row, max: u64, lit: bool) -> Element<'static, M> {
    let color = bar_color(r);

    let fill = (r.amount as f64 / max.max(1) as f64 * 100.0)
        .clamp(0.0, 100.0)
        .round() as u16;
    if fill >= 100 {
        bar_fill(color, lit).width(Length::Fill).into()
    } else if fill == 0 {
        Space::new().width(Length::Fill).height(Length::Fill).into()
    } else {
        row![
            bar_fill(color, lit).width(Length::FillPortion(fill)),
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
/// The selected row's bar is the same ramp at full strength — the selection
/// mark is a brighter bar and a brighter name, not a frame.
fn bar_fill<M: 'static>(color: Color, lit: bool) -> iced::widget::Container<'static, M> {
    container(Space::new().width(Length::Fill).height(Length::Fill)).style(move |_: &Theme| {
        container::Style {
            background: Some(bar_ramp(color, lit)),
            border: iced::border::rounded(2),
            ..container::Style::default()
        }
    })
}

/// Every bar's ramp: `color` dim at the tail and saturated at the leading
/// edge, at full strength when `lit` (the selection).
fn bar_ramp(color: Color, lit: bool) -> iced::Background {
    let (tail, head) = if lit { (0.55, 1.0) } else { (0.16, 0.55) };
    iced::Background::Gradient(
        iced::gradient::Linear::new(iced::Radians(std::f32::consts::FRAC_PI_2))
            .add_stop(0.0, Color { a: tail, ..color })
            .add_stop(1.0, Color { a: head, ..color })
            .into(),
    )
}

/// The pointer's own mark on a row: no border, so a hover can sit on the
/// selected row without arguing with it. Every list that answers the mouse
/// wears this one — the drill's panes and the comparison's two spell
/// tables — so "the thing under the cursor" looks the same everywhere. The
/// overlay's look; the window's is [`hover_style_in`] with its own.
pub(crate) fn hover_style(hovered: bool) -> container::Style {
    hover_style_in(&Look::OVERLAY, hovered)
}

/// [`hover_style`] in a surface's own [`Look`]: the window's is the
/// prototype's blue-tinted `--hover`, the overlay's a white wash.
pub(crate) fn hover_style_in(look: &Look, hovered: bool) -> container::Style {
    let wash = look.hover;
    container::Style {
        background: hovered.then(|| wash.into()),
        border: iced::border::rounded(3),
        ..container::Style::default()
    }
}

/// A row's container, selected or not: the selection is the surface's
/// fill and NO frame — with the lit bar and the bright name (`class_bar`,
/// [`name_ink`]). Every caller names its surface's look; only the window's
/// rows are ever drawn selected.
pub(crate) fn row_style_in(look: &Look, selected: bool) -> container::Style {
    let fill = look.select;
    container::Style {
        background: selected.then(|| fill.into()),
        border: iced::border::rounded(3),
        ..container::Style::default()
    }
}

/// A row's name in the window: parchment, and full white on the selected
/// row — the selection is "a lit bar and a bright name", never a frame.
pub(crate) fn name_ink(selected: bool) -> Color {
    if selected { Color::WHITE } else { theme::INK }
}

// ---- shared chrome ---------------------------------------------------------

/// The footer: the daemon's status line when it has something to say, and
/// NOTHING otherwise — not an empty line, not the gap above one: the
/// keymap lives behind `?`, per screen, and the rows take the height. From
/// the status line itself (the state's `status`), which a layout drawn by
/// its width (the fight's stage) holds instead of the state.
fn footer_of(status: Option<&str>) -> Option<Element<'static, Message>> {
    let status = status.filter(|s| !s.trim().is_empty())?;
    Some(
        container(text(status.to_string()).size(size::SMALL).color(theme::BAD))
            .padding(iced::Padding {
                top: 6.0,
                ..iced::Padding::ZERO
            })
            .into(),
    )
}

/// The views as a tab strip of their own, under a fight's header: they
/// switch what the numbers on THIS fight mean, so they sit with the numbers
/// rather than on the front-door strip. `stored`: a stored pull has only
/// the views the store writes (R24: no enemies on a card), and one it
/// lacks is there, disabled, saying why — the strip keeps its shape from
/// pull to pull. With `trailing` at the row's end — the meter's filter
/// (`.vtabs .filter`), which the always-on filter row gave way to — and
/// the row inside `inset`, over a hairline the full width: a fight's
/// workspace runs edge to edge.
pub(crate) fn view_tabs_with(
    accent: theme::Accent,
    shown: View,
    stored: bool,
    trailing: Option<Element<'static, Message>>,
    inset: iced::Padding,
) -> Element<'static, Message> {
    let tabs: Vec<nav::Tab<Message>> = WINDOW_VIEWS
        .into_iter()
        .map(|v| {
            let kept = !stored || v.is_stored();
            nav::Tab {
                lead: nav::Lead::Icon(LineIcon::of_view(v)),
                label: window_view_name(v),
                active: shown == v,
                on_press: kept.then_some(Message::PickView(v)),
                tip: (!kept).then_some(NOT_STORED),
            }
        })
        .collect();
    nav::view_strip(tabs, accent, trailing, inset)
}

/// What a view a stored pull lacks says under the pointer.
pub(crate) const NOT_STORED: &str = "The history store keeps no enemy damage";

pub(crate) use wowdps_gui_logic::labels::{WINDOW_VIEWS, window_view_name};
#[cfg(test)]
mod tests {
    use super::*;
    use crate::window::testkit::{self as tk, apply, render, simulator};
    use std::time::Duration;
    use wowdps_model::SegmentKind;
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

    /// The defect this guards: a mid-luminance bar (Hunter green, Monk jade)
    /// long enough to run under the numbers used to leave dps and % as DIM
    /// grey on a lit gradient. Every long bar's dimmest metric must clear the
    /// large-text bar against what is actually painted under it.
    /// The layout claim the ink heuristic could never make: at EVERY bar
    /// length the fill stops before the numbers start, so the numeric
    /// columns are always on the panel and their ink never depends on the
    /// row's color at all.

    #[test]
    fn selected_rows_get_a_wash_and_no_frame() {
        let on = row_style_in(&Look::WINDOW, true);
        // The window's selection is the prototype's raised navy, not a
        // grey wash; its hover the blue-tinted breath.
        assert_eq!(on.background, Some(theme::RAISE.into()));
        assert_eq!(
            on.border.width, 0.0,
            "the selection is the lit bar, not a frame"
        );
        let off = row_style_in(&Look::WINDOW, false);
        assert!(off.background.is_none());
        assert_eq!(off.border.width, 0.0);
        assert_eq!(name_ink(true), Color::WHITE);
        assert_eq!(name_ink(false), theme::INK, "parchment, a step under white");
        assert_eq!(
            hover_style_in(&Look::WINDOW, true).background,
            Some(theme::HOVER.into())
        );
        // The overlay's hover is the white wash it has always drawn.
        assert_eq!(
            hover_style(true).background,
            Some(Color::from_rgba(1.0, 1.0, 1.0, 0.07).into())
        );
        assert!(hover_style(false).background.is_none());
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

    // ---- the meter -------------------------------------------------------------

    /// Sorting reorders what is drawn and nothing else: the ranks, the
    /// shares and the click indexes are the daemon's, and the cycle always
    /// returns to its order.
    #[test]
    fn sorting_reorders_rows_without_renumbering() {
        let (state, _mock) = tk::kill();
        let rows = state.rows();
        assert!(rows.len() > 2);
        let by_crit = ordered(rows.clone(), "", Some((table::Col::Crit, true)));
        let crits: Vec<f64> = by_crit.iter().map(|(_, r)| r.crit_pct()).collect();
        assert!(crits.windows(2).all(|w| w[0] >= w[1]), "descending by crit");
        for (i, r) in &by_crit {
            assert_eq!(&rows[*i], r, "every row keeps the index it was given");
        }
        let asc = ordered(rows.clone(), "", Some((table::Col::Amount, false)));
        let amounts: Vec<u64> = asc.iter().map(|(_, r)| r.amount).collect();
        assert!(
            amounts.windows(2).all(|w| w[0] <= w[1]),
            "ascending by amount"
        );
        let plain = ordered(rows.clone(), "", None);
        assert!(
            plain.iter().enumerate().all(|(pos, (i, _))| pos == *i),
            "no sort is the daemon's order"
        );
    }

    /// The heading of the sorted column is gold and wears its arrow after
    /// the label (a line icon: the window's faces have no arrow glyph); an
    /// unsorted heading lights gold under the pointer; every numeric
    /// heading is a click target. The live meter's crit is `CritFine`.
    #[test]
    fn the_sorted_heading_wears_its_marker() {
        let (state, _mock) = tk::kill();
        let size = iced::Size::new(640.0, 40.0);
        let mut ui = simulator(meter_captions(&state, Some((table::Col::Rate, true))));
        let rate = ui.find("Per sec").unwrap().bounds();
        let share = ui.find("Share").unwrap().bounds();
        ui.click("Per sec").unwrap();
        let sent: Vec<Message> = ui.into_messages().collect();
        assert!(matches!(
            sent.as_slice(),
            [Message::SortBy(table::Col::Rate)]
        ));
        // Gold pixels just after a label (physical px, at scale 2).
        let after = |b: iced::Rectangle| {
            let x = ((b.x + b.width) * 2.0) as u32;
            (
                x,
                (b.y * 2.0) as u32,
                x + 32,
                ((b.y + b.height) * 2.0) as u32,
            )
        };
        let inside = |b: iced::Rectangle| {
            (
                (b.x * 2.0) as u32,
                (b.y * 2.0) as u32,
                ((b.x + b.width) * 2.0) as u32,
                ((b.y + b.height) * 2.0) as u32,
            )
        };
        let gold = |sort, at: Option<iced::Point>, region| {
            tk::pixels_at(meter_captions(&state, sort), size, &Theme::TokyoNight, at).count_in(
                region,
                theme::GOLD,
                24,
            )
        };
        let desc = Some((table::Col::Rate, true));
        assert!(
            gold(desc, None, after(rate)) > 4,
            "the arrow follows the label"
        );
        assert_eq!(gold(None, None, after(rate)), 0, "no arrow unsorted");
        assert_eq!(gold(None, None, inside(share)), 0, "gold-dim at rest");
        let over = Some(iced::Point::new(share.center_x(), share.center_y()));
        assert!(
            gold(None, over, inside(share)) > 4,
            "gold under the pointer"
        );
        let mut ui = simulator(meter_captions(&state, Some((table::Col::CritFine, false))));
        assert!(ui.find("Crit").is_ok());
        assert_eq!(
            table::sort_of(table::Col::CritFine, Some((table::Col::CritFine, false))),
            Some(false),
            "ascending"
        );
    }

    /// The meter wears ONE fight header — the title, its meta and badge,
    /// the stat line — and none of what it replaced: no view name after
    /// the title, no stat cards, no position counter.
    #[test]
    fn the_meter_wears_one_fight_header() {
        let (state, _mock) = tk::kill();
        let rows = state.rows();
        let (gui, _peer) = tk::gui_over(state);
        let mut ui = tk::wide(meter_screen(&gui, 0.0));
        assert!(ui.find("The Ashen Warden").is_ok());
        assert!(ui.find("Kill").is_ok());
        assert!(ui.find("Raid dps").is_ok());
        let damage: u64 = rows.iter().filter(|r| !r.enemy).map(|r| r.amount).sum();
        let damage = wowdps_model::fmt::commas(damage);
        assert!(
            ui.find(damage.as_str()).is_ok(),
            "the whole figure, commas and all"
        );
        for gone in ["· Damage", "Your dps", "Rank in role", "Total"] {
            assert!(ui.find(gone).is_err(), "{gone:?} left the header");
        }
        let _ = ui.snapshot(&Theme::TokyoNight).unwrap();
    }

    /// Every view's table heads its columns in the prototype's words — the
    /// names under "Player", then Amount, Per sec, Share and the view's
    /// own fourth — with the overkill column gone, and totals itself. The
    /// Deaths view's is the deaths in order (R25, `deaths`), below.
    #[test]
    fn every_view_renders_with_its_own_captions() {
        for view in [
            View::Damage,
            View::Healing,
            View::Interrupts,
            View::CrowdControl,
            View::Dispels,
            View::Taken,
        ] {
            let (mut state, mut mock) = tk::kill();
            apply(&mut state, &mut mock, Action::SetView(view));
            let rows = state.rows();
            let (gui, _peer) = tk::gui_over(state);
            let mut ui = tk::wide(meter_screen(&gui, 0.0));
            assert!(ui.find("The Ashen Warden").is_ok());
            let total = format!(
                "Total, {}",
                nav::plural(rows.iter().filter(|r| !r.enemy).count(), "player")
            );
            assert!(
                ui.find(total.as_str()).is_ok(),
                "{view:?} has its total row"
            );
            assert!(ui.find("Kill").is_ok());
            assert!(ui.find("Player").is_ok(), "{view:?} heads its names");
            let (fourth, rate) = match view {
                View::Damage => ("Crit", Some("Per sec")),
                View::Healing => ("Overheal", Some("Per sec")),
                View::Taken => ("Absorbed", Some("Per sec")),
                _ => ("Count", None),
            };
            assert!(ui.find(fourth).is_ok(), "{view:?} reads by {fourth}");
            assert!(ui.find("Share").is_ok());
            for gone in ["(Overkill)", "(Overheal)", "(Absorbed)"] {
                assert!(ui.find(gone).is_err(), "{view:?}: {gone} left the meter");
            }
            match rate {
                Some(rate) => assert!(ui.find(rate).is_ok(), "{view:?} rate heading"),
                None => assert!(ui.find("Per sec").is_err(), "a count has no rate"),
            }
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

    /// The Deaths view without a raid timeline (a store that kept a pull's
    /// card alone, an older daemon) keeps the count table: its heads, the
    /// players ranked by their deaths, and a total counting them.
    #[test]
    fn the_deaths_count_table_stands_without_a_timeline() {
        let state = tk::raid_deaths_bare(25);
        assert!(state.raid().is_none());
        let rows = state.rows();
        assert_eq!(rows.len(), 6, "one row per player who died");
        let (gui, _peer) = tk::gui_over(state);
        assert!(crate::deaths::Table::of(&gui).is_none(), "no chronology");
        let mut ui = tk::wide(meter_screen(&gui, 0.0));
        for head in ["Player", "Count", "Share"] {
            assert!(ui.find(head).is_ok(), "{head}");
        }
        assert!(ui.find("Total, 6 players").is_ok(), "its total");
        assert!(ui.find(rows[0].label.as_str()).is_ok(), "its top row");
        assert!(ui.find("1").is_ok(), "ranked");
    }

    #[test]
    fn ranks_are_optional_and_the_options_panel_overlays_every_screen() {
        let (state, _) = tk::kill();
        let (mut gui, _peer) = tk::gui_over(state);
        gui.cfg.show_ranks = false;
        {
            let mut ui = simulator(view(&gui));
            assert!(ui.find("#").is_err(), "no rank column");
            assert!(ui.find("Options").is_err());
        }
        gui.options_open = true;
        {
            let mut ui = simulator(view(&gui));
            assert!(ui.find("Options").is_ok());
        }
        // The panel is the window's, not the meter's: it is up over Home
        // too.
        gui.home = Some(crate::home::Home::new());
        let mut ui = simulator(view(&gui));
        assert!(ui.find("Options").is_ok());
        let _ = ui.snapshot(&Theme::TokyoNight).unwrap();
        let _ = render(options_panel(&gui.cfg, theme::GOLD_ACCENT));
    }

    #[test]
    fn a_meter_without_data_waits() {
        let mut state = ClientState::new();
        state.screen = Screen::Meter;
        let (gui, _peer) = tk::gui_over(state);
        let mut ui = simulator(view(&gui));
        assert!(ui.find("waiting for combat…").is_ok());
        assert!(ui.find("nothing to show for this view yet").is_ok());
        // No fight, so no clock to read — and no figures to sum: no
        // confident zero under a title that says nothing has happened.
        assert!(ui.find("0:00").is_err());
        assert!(ui.find("Raid dps").is_err());
        assert!(ui.find("Damage").is_ok(), "the tabs are still there");
    }

    /// The owner's row wears the "you" tag on its own line — and only the
    /// owner's — beside a tank's shield or a healer's cross, each a fixed
    /// width the name leaves room for.
    #[test]
    fn the_owner_s_row_wears_its_tag() {
        let size = iced::Size::new(1440.0, 900.0);
        let (mut gui, _peer) = tk::gui_over(tk::raid(25));
        gui.cfg.hide_realms = true;
        let at = |gui: &Gui| {
            let mut ui = tk::simulator_as(crate::window::settings(), size, view(gui));
            let tag = ui.find("you").map(|t| t.bounds());
            // The seventeenth row's line (the chip names them first).
            let list = ui.find(meter_list_id()).unwrap().bounds();
            (tag, list.y + 16.5 * pitch::ROW)
        };
        let (tag, _) = at(&gui);
        assert!(tag.is_err(), "no owner, no tag");
        gui.owner_guid = Some("Player-1-16".to_string());
        let (tag, line) = at(&gui);
        let tag = tag.expect("the owner's tag");
        // Over the bar under the words, a little above the pitch's middle.
        assert!(
            (tag.center_y() - line).abs() < 6.0,
            "on the owner's line: {tag:?} {line}"
        );
        assert!(
            tag.x > ROW_LEAD + RANK_W + 6.0 + ICON_X + 18.0,
            "after the name"
        );
        // What follows a name, and the room it takes.
        let width = |role, you| name_tags(role, you, None).map(|(_, w)| w);
        assert_eq!(width(Some(Role::Tank), None), Some(ROLE_GLYPH));
        assert_eq!(width(Some(Role::Healer), None), Some(ROLE_GLYPH));
        assert_eq!(width(Some(Role::Dps), None), None);
        assert_eq!(width(None, None), None);
        assert_eq!(
            width(Some(Role::Dps), Some(Some(Class::Warlock))),
            Some(YOU_TAG_W)
        );
        assert_eq!(
            width(Some(Role::Tank), Some(None)),
            Some(ROLE_GLYPH + TAG_GAP + YOU_TAG_W)
        );
        // The comparison's half leads them, the name leaving it room too.
        assert_eq!(
            name_tags(Some(Role::Dps), Some(None), Some(0)).map(|(_, w)| w),
            Some(PIN_W + TAG_GAP + YOU_TAG_W)
        );
    }

    /// The fight's workspace runs edge to edge: the meter's total from the
    /// window's left edge to the inspector's hairline, "Player" over the
    /// icon column as the prototype's head is (`.who2`), and a filter's
    /// total the drawn rows' — their count and sum, no share of the chart
    /// it is not.
    #[test]
    fn the_meter_runs_edge_to_edge_and_totals_what_it_draws() {
        let size = iced::Size::new(1440.0, 900.0);
        let (mut gui, _peer) = tk::gui_over(tk::raid(25));
        gui.cfg.hide_realms = true;
        let mut ui = tk::simulator_as(crate::window::settings(), size, view(&gui));
        // The stage starts at the docked rail's edge (its width and its
        // hairline); the inspector takes its share of the WINDOW's width.
        let stage = rail::RAIL_W + 1.0;
        let total = ui.find(meter_total_id()).unwrap().bounds();
        let meter_w = size.width - stage - crate::inspector::beside(size.width).unwrap() - 1.0;
        assert!(
            (total.x - stage).abs() < 0.5 && (total.width - meter_w).abs() < 0.5,
            "{total:?}"
        );
        let head = ui.find("Player").unwrap().bounds();
        let icon_x = stage + ROW_LEAD + RANK_W + 6.0 + ICON_X;
        assert!((head.x - icon_x).abs() < 0.5, "{head:?} vs {icon_x}");
        let label = ui.find("Total, 25 players").unwrap().bounds();
        assert!((label.x - icon_x).abs() < 0.5, "{label:?}");
        assert!(ui.find("100%").is_ok());
        drop(ui);
        // "Raider1" and "Raider10" … "Raider19": eleven drawn.
        gui.filter = "Raider1".to_string();
        let drawn: u64 = gui
            .state
            .rows()
            .iter()
            .filter(|r| r.label.starts_with("Raider1"))
            .map(|r| r.amount)
            .sum();
        let mut ui = tk::simulator_as(crate::window::settings(), size, view(&gui));
        assert!(ui.find("Total, 11 players").is_ok());
        assert!(ui.find(table::figure(drawn).as_str()).is_ok(), "their sum");
        assert!(ui.find("100%").is_err(), "no share while filtered");
    }

    #[test]
    fn a_live_fight_reports_how_far_behind_the_log_is() {
        let (state, _) = tk::live();
        let (mut gui, _peer) = tk::gui_over(state);
        gui.set_last_snapshot_at(Some(std::time::Instant::now() - Duration::from_secs(9)));
        let mut ui = simulator(meter_screen(&gui, 0.0));
        assert!(ui.find("Live").is_ok());
        assert!(ui.find("no events for 9s").is_ok());
        // A closed fight never shows the notice, however old the data.
        let (state, _) = tk::kill();
        let (mut gui, _peer) = tk::gui_over(state);
        gui.set_last_snapshot_at(Some(std::time::Instant::now() - Duration::from_secs(9)));
        assert!(
            simulator(meter_screen(&gui, 0.0))
                .find("no events for 9s")
                .is_err()
        );
    }

    #[test]
    fn footer_carries_only_the_daemon_status() {
        let mut state = ClientState::new();
        // No status, no footer — not an empty line, not the gap above one:
        // the keymap lives behind `?` now, and the rows take the height.
        assert!(footer_of(state.status.as_deref()).is_none());
        state.status = Some("  ".to_string());
        assert!(
            footer_of(state.status.as_deref()).is_none(),
            "an empty status says nothing"
        );
        state.status = Some("segment gone: the log rotated".to_string());
        assert!(has(
            footer_of(state.status.as_deref()).expect("a status to show"),
            "segment gone: the log rotated"
        ));
        // The meter shows it under the table when there is one.
        let (mut state, _) = tk::kill();
        state.status = Some("daemon gone — reconnecting…".to_string());
        let (gui, _peer) = tk::gui_over(state);
        let mut ui = tk::wide(meter_screen(&gui, 0.0));
        assert!(ui.find("daemon gone — reconnecting…").is_ok());
    }

    /// R17: the Taken meter over `taken.txt` — the tank's row reads its
    /// absorbed part in the fourth column and its dtps, with no
    /// parenthesised extra. (What hit him is the inspector's:
    /// `inspector::tests`.)
    #[test]
    fn the_taken_meter_reads_absorbed_and_dtps() {
        let (state, _mock) = tk::taken_kill();
        let top = state.rows().first().cloned().unwrap();
        assert_eq!(top.amount, 84_000, "Durgan's taken (fixture golden)");
        assert_eq!(top.extra, 12_000, "his partial absorbs");
        let (gui, _peer) = tk::gui_over(state);
        let mut ui = tk::wide(meter_screen(&gui, 0.0));
        assert!(ui.find("Absorbed").is_ok());
        assert!(ui.find("(Absorbed)").is_err(), "no parenthesised extra");
        assert!(ui.find("Per sec").is_ok());
        assert!(ui.find("12.0k").is_ok(), "the fourth column is absorbed");
        assert!(ui.find("1.4k").is_ok(), "84 000 over 60 s");
        let _ = ui.snapshot(&Theme::TokyoNight).unwrap();
    }

    /// Only what reads as a player's "Name-Realm-Region" loses its realm: a
    /// creature's hyphen is its name's, and a pet's parentheses are not a
    /// realm either.
    #[test]
    fn realmless_touches_only_player_labels() {
        assert_eq!(realmless("Bearlysimpin-Proudmoore-US"), "Bearlysimpin");
        assert_eq!(realmless("Yourhonour-Area52-US"), "Yourhonour");
        assert_eq!(
            realmless("Word of Glory (Soundscape-Proudmoore-US)"),
            "Word of Glory (Soundscape)"
        );
        for kept in [
            "Yogg-Saron",
            "Blood-Queen Lana'thel",
            "Shadow Bolt (Magus of the Dead)",
            "Polymorph (Fizzle the Mad)",
            "Melee",
            "Mind-numbing Poison",
            "Ana-Realm",
        ] {
            assert_eq!(realmless(kept), kept);
        }
        let rows = vec![row("Thraxx-Nebula-US", 1, None), row("Yogg-Saron", 1, None)];
        let shown: Vec<String> = realmless_rows(&rows, true)
            .into_iter()
            .map(|r| r.label)
            .collect();
        assert_eq!(shown, vec!["Thraxx", "Yogg-Saron"]);
        assert_eq!(
            realmless_rows(&rows, false),
            rows,
            "the option off: as given"
        );
    }

    /// With realms hidden, the enemy view keeps a creature's hyphen — its
    /// name's, not a realm — while an enemy PLAYER (an arena's) still loses
    /// the realm; every other view's rows are players.
    #[test]
    fn hidden_realms_leave_an_enemy_s_hyphen_alone() {
        assert_eq!(meter_label("Yogg-Saron", true), "Yogg-Saron");
        assert_eq!(
            meter_label("Blood-Queen Lana'thel", true),
            "Blood-Queen Lana'thel"
        );
        assert_eq!(meter_label("Durgan-Nebula-US", true), "Durgan");
        assert_eq!(meter_label("Durgan-Nebula-US", false), "Durgan");
        // The enemy view's meter over the fixture draws its rows by these.
        let (mut state, mut mock) = tk::kill();
        apply(&mut state, &mut mock, Action::SetView(View::EnemyTaken));
        let rows = state.rows();
        assert!(!rows.is_empty(), "the kill has enemies");
        let (mut gui, _peer) = tk::gui_over(state);
        gui.cfg.hide_realms = true;
        let mut ui = tk::wide(meter_screen(&gui, 0.0));
        for r in &rows {
            let shown = meter_label(&r.label, true);
            assert!(
                ui.find(shown.as_str()).is_ok(),
                "{} drawn as {shown}",
                r.label
            );
        }
    }

    /// The window's semantic colours: a kill is the good green and a wipe
    /// the bad red, LIVE a red dot and its word — and none of them yellow.
    #[test]
    fn the_window_words_outcomes_in_green_and_red() {
        let (live, _) = tk::live();
        let badge = crate::fight_head::badge(crate::fight_head::Verdict::of(&live))
            .expect("a live pull says so");
        assert_eq!(badge.word, "Live");
        assert!(badge.live, "with its dot");
        assert_eq!(badge.color, theme::BAD);
        let (kill, _) = tk::kill();
        assert_eq!(
            crate::fight_head::badge(crate::fight_head::Verdict::of(&kill))
                .map(|b| (b.word, b.color)),
            Some(("Kill".to_string(), theme::GOOD))
        );
        let (wipe, _) = tk::wipe();
        assert_eq!(
            crate::fight_head::badge(crate::fight_head::Verdict::of(&wipe))
                .map(|b| (b.word, b.color)),
            Some(("Wipe".to_string(), theme::BAD))
        );
        assert_eq!(
            crate::fight_head::badge(crate::fight_head::Verdict::of(&ClientState::new())),
            None
        );
        // The overlay keeps its words and its yellow.
        assert_eq!(header_tag(&live), ("LIVE", YELLOW));
        assert_eq!(header_tag(&kill), ("KILL", GREEN));
    }

    /// Crit in the window's ability strip is plain ink; the overlay's
    /// stays yellow. Both draw.
    #[test]
    fn the_windows_ability_strip_is_not_yellow() {
        let r = Row {
            spell_id: 0,
            ..row("Pyroblast", 9_000, None)
        };
        assert_eq!(Look::WINDOW.crit, theme::INK);
        // What is DRAWN, not only which constant was picked: the window's
        // strip has no yellow pixel, where the overlay's crit is yellow.
        let size = iced::Size::new(480.0, 60.0);
        let drawn = |look: &Look| {
            tk::pixels(
                spell_stats_in::<()>(look, &r, View::Damage, 1.0),
                size,
                &Theme::TokyoNight,
            )
        };
        assert_eq!(drawn(&Look::WINDOW).count(YELLOW, 8), 0, "no yellow");
        assert!(
            drawn(&Look::OVERLAY).count(YELLOW, 8) > 0,
            "the overlay's crit"
        );
        // A recap's hit is the tokens' bad-news red in the window, and the
        // overlay's plain white.
        let hit = |look: &Look| {
            tk::pixels(
                recap_row_in::<()>(look, &r, 9_000, 28.0, 1.0, false),
                iced::Size::new(480.0, 28.0),
                &Theme::TokyoNight,
            )
        };
        assert!(hit(&Look::WINDOW).count(theme::BAD, 2) > 0, "a red amount");
        assert_eq!(hit(&Look::OVERLAY).count(theme::BAD, 2), 0);
        for look in [&Look::WINDOW, &Look::OVERLAY] {
            let mut ui = simulator(spell_stats_in::<()>(look, &r, View::Damage, 1.0));
            assert!(ui.find(look.word("crit").as_str()).is_ok());
            assert!(ui.find("40%").is_ok());
            let _ = ui.snapshot(&Theme::TokyoNight).unwrap();
            let _ = render(spell_breadcrumb_in::<()>(
                look,
                "Thraxx",
                None,
                "Pyroblast",
                Some(&r),
                1.0,
            ));
            let _ = render(recap_row_in::<()>(look, &r, 9_000, 20.0, 1.0, false));
            let _ = render(spell_target_list_in::<()>(
                look,
                std::slice::from_ref(&r),
                20.0,
                1.0,
            ));
            let _ = render(team_divider_in::<()>(look, 11.0));
        }
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
        // The new chrome layers: the sheet over a screen, and Home.
        gui.talents = None;
        gui.shortcuts_open = true;
        let _ = render(view(&gui));
        gui.shortcuts_open = false;
        gui.home = Some(crate::home::Home::new());
        let _ = render(view(&gui));
    }

    /// The filter rides the view tabs' row, at its end — no row of its own
    /// between the tabs and the table — widens while it has focus, and is
    /// not drawn over a drill, whose panes are not players.
    #[test]
    fn the_filter_rides_the_tab_row_and_widens_with_focus() {
        let (state, _mock) = tk::kill();
        let (mut gui, _peer) = tk::gui_over(state);
        let field = |gui: &Gui| {
            let mut ui = tk::wide(meter_screen(gui, 0.0));
            let field = ui.find(nav::filter_id()).map(|t| t.bounds()).ok();
            let tab = ui.find("Healing").expect("the view tabs").bounds();
            let heading = ui.find("Player").expect("the table's heads").bounds();
            (field, tab, heading)
        };
        let (at_rest, tab, heading) = field(&gui);
        let at_rest = at_rest.expect("the filter is on the meter");
        assert!(
            at_rest.y < tab.center_y() && tab.center_y() < at_rest.y + at_rest.height,
            "on the tabs' line: {at_rest:?} by {tab:?}"
        );
        assert!(at_rest.x > 1000.0, "at the row's end: {at_rest:?}");
        assert!(
            heading.y - (tab.y + tab.height) < 30.0,
            "the heads follow the tabs: {heading:?}"
        );
        gui.filter_focused = true;
        let (focused, _, _) = field(&gui);
        let focused = focused.unwrap();
        assert_eq!(
            focused.width - at_rest.width,
            nav::FILTER_W_FOCUSED - nav::FILTER_W
        );
        // It grows leftwards: its end stays at the row's end.
        assert!((focused.x + focused.width - (at_rest.x + at_rest.width)).abs() < 1.0);
        // The meter stands beside the inspector, filter and all; only a
        // narrow window's pushed inspector covers it.
        let (state, _mock) = tk::drilled();
        let (mut gui, _peer) = tk::gui_over(state);
        let mut ui = tk::wide(meter_screen(&gui, 0.0));
        assert!(ui.find(nav::filter_id()).is_ok(), "the meter's, beside");
        gui.state.inspect();
        let narrow = iced::Size::new(460.0, 860.0);
        let mut ui = tk::simulator_as(crate::window::settings(), narrow, meter_screen(&gui, 0.0));
        assert!(ui.find(nav::filter_id()).is_err(), "pushed over the meter");
    }

    #[test]
    fn the_filter_narrows_rows_without_renumbering() {
        let (state, _) = tk::kill();
        let rows = state.rows();
        assert!(rows.len() > 1);
        let target = rows[1].label.clone();
        let (mut gui, _peer) = tk::gui_over(state);
        gui.filter = target.to_lowercase();
        let mut ui = simulator(meter_screen(&gui, 0.0));
        assert!(ui.find(target.as_str()).is_ok());
        assert!(
            ui.find(rows[0].label.as_str()).is_err(),
            "the top row is filtered out"
        );
        // Its rank is still the one it holds in the whole chart.
        assert!(ui.find("2").is_ok());
        let _ = ui.snapshot(&Theme::TokyoNight).unwrap();
    }

    /// The filter searches what a row IS, not only what it is called: the
    /// fixture's Warrior, Hunter and Discipline Priest answer to their
    /// class, their spec and their role.
    #[test]
    fn the_filter_matches_class_spec_and_role() {
        let (state, _) = tk::kill();
        let rows = state.rows();
        let labels = |needle: &str| -> Vec<String> {
            filtered(rows.clone(), needle)
                .into_iter()
                .map(|r| r.label)
                .collect()
        };
        // Class.
        assert_eq!(labels("warrior"), vec!["Thraxx-Nebula-US".to_string()]);
        assert_eq!(labels("PRIEST"), vec!["Mírelle-Nebula-US".to_string()]);
        assert!(labels("warlock").is_empty(), "nobody here is a Warlock");
        // Spec, including the free abbreviation a substring gives.
        assert_eq!(labels("discipline"), vec!["Mírelle-Nebula-US".to_string()]);
        assert_eq!(labels("marksman"), vec!["Kael'thar-Nebula-US".to_string()]);
        // Role.
        assert_eq!(labels("healer"), vec!["Mírelle-Nebula-US".to_string()]);
        assert_eq!(labels("tank").len(), 0, "the fixture fields no tank");
        assert_eq!(labels("dps").len(), 2, "the two damage dealers");
        // And the label still works, on a fragment of a name.
        assert_eq!(labels("thraxx"), vec!["Thraxx-Nebula-US".to_string()]);
    }

    /// A spec name shared by two classes matches both — "Protection" is
    /// genuinely two specs, and the class name is how a reader narrows it.
    #[test]
    fn a_spec_term_matches_across_classes() {
        let mut warrior = row("Tank One", 100, Some(Class::Warrior));
        warrior.spec = Some(Spec::ProtectionWarrior);
        let mut paladin = row("Tank Two", 90, Some(Class::Paladin));
        paladin.spec = Some(Spec::ProtectionPaladin);
        let mut mage = row("Caster", 80, Some(Class::Mage));
        mage.spec = Some(Spec::Fire);
        let rows = vec![warrior, paladin, mage];
        let names = |needle: &str| -> Vec<String> {
            filtered(rows.clone(), needle)
                .into_iter()
                .map(|r| r.label)
                .collect()
        };
        assert_eq!(names("protection").len(), 2);
        // "prot" gets there too, with no abbreviation table.
        assert_eq!(names("prot").len(), 2);
        // The class name disambiguates.
        assert_eq!(names("paladin"), vec!["Tank Two".to_string()]);
        // And the role both share.
        assert_eq!(names("tank").len(), 2);
        assert_eq!(names("fire"), vec!["Caster".to_string()]);
    }

    /// R8 has not inferred a class for this row yet. It must answer to no
    /// class or spec term — not to all of them — and must still be there
    /// when nothing is being filtered.
    /// WoW names are full of accents and keyboards mostly are not. Both of
    /// these are real rows from the user's store.
    #[test]
    fn the_filter_folds_accents_on_both_sides() {
        let mut akanos = row("Akanôs-Tichondrius-US", 100, Some(Class::Warlock));
        akanos.spec = Some(Spec::Affliction);
        let mut fidele = row("Fidèle-Tichondrius-US", 90, Some(Class::Paladin));
        fidele.spec = Some(Spec::HolyPaladin);
        let plain = row("Thraxx-Nebula-US", 80, Some(Class::Warrior));
        let rows = vec![akanos, fidele, plain];
        let names = |needle: &str| -> Vec<String> {
            filtered(rows.clone(), needle)
                .into_iter()
                .map(|r| r.label)
                .collect()
        };
        assert_eq!(names("akanos"), vec!["Akanôs-Tichondrius-US".to_string()]);
        assert_eq!(names("fidele"), vec!["Fidèle-Tichondrius-US".to_string()]);
        // The accented spelling finds it too — typing the name correctly is
        // not a mistake.
        assert_eq!(names("akanôs"), vec!["Akanôs-Tichondrius-US".to_string()]);
        assert_eq!(names("Fidèle"), vec!["Fidèle-Tichondrius-US".to_string()]);
        // The unaccented row is unaffected either way.
        assert_eq!(names("thraxx"), vec!["Thraxx-Nebula-US".to_string()]);
        assert_eq!(names("").len(), 3);
        // Class and spec terms fold through the same function: an accented
        // term still finds the plain name behind it.
        assert_eq!(names("wärlock"), vec!["Akanôs-Tichondrius-US".to_string()]);
        assert_eq!(names("hóly"), vec!["Fidèle-Tichondrius-US".to_string()]);
        assert_eq!(
            names("afflictión"),
            vec!["Akanôs-Tichondrius-US".to_string()]
        );
        // A Cyrillic term matches no Latin row, and is not transliterated
        // into one.
        assert!(names("дракон").is_empty());
    }

    /// Folding must not disturb the standing rule.
    #[test]
    fn a_folded_filter_keeps_the_original_ranks() {
        let mut top = row("Thraxx-Nebula-US", 100, Some(Class::Warrior));
        top.pct = 60.0;
        let mut second = row("Akanôs-Tichondrius-US", 50, Some(Class::Warlock));
        second.pct = 30.0;
        let third = row("Kael'thar-Nebula-US", 20, Some(Class::Hunter));
        let rows = vec![top, second, third];
        let kept = filtered_indexed(rows.clone(), "akanos");
        assert_eq!(kept.len(), 1);
        assert_eq!(kept[0].0, 1, "still rank 2 of the whole chart");
        assert_eq!(kept[0].1.pct, 30.0, "and still its own share");
    }

    #[test]
    fn an_unknown_class_row_matches_no_class_term() {
        let known = row("Zephyra", 100, Some(Class::Mage));
        let unknown = row("Unseen", 50, None);
        let rows = vec![known, unknown];
        assert_eq!(filtered(rows.clone(), "").len(), 2, "empty keeps it");
        assert_eq!(
            filtered(rows.clone(), "mage")
                .into_iter()
                .map(|r| r.label)
                .collect::<Vec<_>>(),
            vec!["Zephyra".to_string()]
        );
        assert!(filtered(rows.clone(), "healer").is_empty());
        // It still answers to its own name.
        assert_eq!(filtered(rows, "unseen").len(), 1);
    }

    /// The standing rule, restated over the new terms: matching more things
    /// must not renumber any of them.
    #[test]
    fn class_and_role_filters_keep_the_original_ranks() {
        let (state, _) = tk::kill();
        let rows = state.rows();
        let healer_at = rows
            .iter()
            .position(|r| {
                r.spec
                    .is_some_and(|s| s.role() == wowdps_model::Role::Healer)
            })
            .expect("the fixture has a healer");
        let kept = filtered_indexed(rows.clone(), "healer");
        assert_eq!(kept.len(), 1);
        assert_eq!(
            kept[0].0, healer_at,
            "the row keeps the index it holds in the whole chart"
        );
        assert_eq!(kept[0].1.pct, rows[healer_at].pct, "and its share");
    }

    #[test]
    fn an_empty_filter_is_the_identity() {
        let (state, _) = tk::kill();
        let rows = state.rows();
        assert_eq!(filtered(rows.clone(), ""), rows);
        // And the filter is case-insensitive over the label, nothing else.
        let one = filtered(rows.clone(), &rows[0].label.to_uppercase());
        assert_eq!(one.len(), 1);
        assert!(filtered(rows, "no such player").is_empty());
    }

    #[test]
    fn filtering_keeps_every_row_at_its_original_index() {
        let (state, _) = tk::kill();
        let rows = state.rows();
        let kept = filtered_indexed(rows.clone(), &rows[1].label);
        assert_eq!(kept.len(), 1);
        assert_eq!(kept[0].0, 1, "a click still names row 1 to the daemon");
    }

    // ---- rows ----------------------------------------------------------------------

    #[test]
    fn bar_rows_lay_the_metrics_in_columns() {
        let mut r = row("Thraxx-Nebula-US", 185_370, Some(Class::Warrior));
        r.extra = 5_200;
        r.per_sec = 3_089.5;
        r.pct = 50.83;
        let meter = table::Grid::Meter {
            view: View::Damage,
            narrow: false,
        };
        let mut ui = simulator(bar_row_tagged::<()>(
            &r,
            185_370,
            true,
            24.0,
            Some(table::METER_DAMAGE),
            meter,
            1.0,
            Some(3),
            None,
            None,
        ));
        assert!(ui.find("Thraxx-Nebula-US").is_ok());
        assert!(ui.find("185.4k").is_ok());
        assert!(ui.find("3.1k").is_ok());
        assert!(ui.find("50.8%").is_ok());
        assert!(ui.find("3").is_ok(), "the rank");
        let _ = ui.snapshot(&Theme::TokyoNight).unwrap();

        // Compact: the amount only; a sub-1/s rate and no extra go blank.
        let mut quiet = row("Pet", 40, Some(Class::Hunter));
        quiet.per_sec = 0.5;
        let mut ui = simulator(bar_row_tagged::<()>(
            &quiet, 185_370, false, 20.0, None, meter, 1.0, None, None, None,
        ));
        assert!(ui.find("40").is_ok());
        assert!(ui.find("0").is_err(), "no rate cell in compact rows");
        let _ = ui.snapshot(&Theme::TokyoNight).unwrap();
        let _ = render(bar_row_tagged::<()>(
            &quiet,
            185_370,
            false,
            20.0,
            Some(table::METER_DAMAGE),
            meter,
            1.5,
            None,
            None,
            None,
        ));
        // Zero and full bars take their own branches.
        let _ = render(bar_row_tagged::<()>(
            &row("z", 0, None),
            10,
            false,
            20.0,
            Some(table::METER_DAMAGE),
            meter,
            1.0,
            None,
            None,
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
        assert!(has(spell_stats::<()>(&r, View::Taken, 1.0), "absorbed"));

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
        let mut ui = simulator(meter_captions(&state, None));
        // No row has an overheal to show, so the column is not drawn.
        assert!(ui.find("(Overheal)").is_err());
        assert!(ui.find("Per sec").is_ok());
        assert!(ui.find("#").is_err(), "a rank explains itself");
        assert!(ui.find("player").is_err(), "and so does a name");
        state.view = View::Dispels;
        let mut ui = simulator(meter_captions(&state, None));
        assert!(ui.find("Count").is_ok());
        assert!(ui.find("#").is_err());
        let _ = render(rank_cell::<()>(7, 11.0, RANK_W));
        let cleared: Element<'static, ()> = scroll_clear(text("x")).into();
        let _ = render(cleared);
        // A class with a spec: the icon takes the drawn-disc fallback here.
        let mut r = row("Spec", 10, Some(Class::Mage));
        r.spec = Some(Spec::Fire);
        let _ = render(bar_row_tagged::<()>(
            &r,
            10,
            false,
            20.0,
            Some(table::METER_DAMAGE),
            table::Grid::Meter {
                view: View::Damage,
                narrow: false,
            },
            1.0,
            Some(1),
            None,
            None,
        ));
    }
}
