//! Rendering. Nothing here mutates state.
//!
//! Layout mirrors the TUI: a segment-list screen and a meter screen whose
//! rows are class-colored bars; an open drilldown replaces the rows with the
//! by-spell / by-target panes.
//!
//! The renderers the overlay shares with the window (the recap rows, the
//! ability drill's breadcrumb, stat strip and target list, the team divider)
//! keep their names for the overlay's look, pixel for pixel; each has an
//! `_in` twin that takes a surface's [`Look`], which the window calls with
//! `Look::WINDOW`. The overlay's own rows (`overlay_row`, `overlay_drill_row`)
//! and what it borrows as is (`header_tag`, `rank_cell`, `hover_style`) draw
//! as they always have; the rest is the window's alone and draws with the
//! redesign's tokens directly.

use iced::widget::{Space, checkbox, column, container, mouse_area, row, scrollable, stack, text};
use iced::{Border, Color, Element, Font, Length, Theme};

use wowdps_model::fmt::{duration, human};
use wowdps_model::{Class, ListRow, Pane, Role, Row, Screen, SegmentKind, View};
use wowdps_proto::ClientState;

use crate::compare;
use crate::fold;
use crate::line_icons::LineIcon;
use crate::nav;
use crate::table;
use crate::theme::{self, Look, pitch, size};
use crate::window::{Gui, Message, RowHover};

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
/// The scale the window draws the renderers it shares with the overlay at
/// (`Look::type_scale`): the overlay passes its zoom, the window this —
/// its rows near the prototype's 14.5 px, their captions at 13.
const WS: f32 = Look::WINDOW.type_scale;

/// Bar color for players whose COMBATANT_INFO has not been seen yet.
const CLASSLESS: Color = Color::from_rgb(0.42, 0.44, 0.52);
/// R24: the hostile red an enemy row wears — its bar and its skull disc —
/// on the enemy view, where no row has a class.
pub(crate) const HOSTILE: Color = Color::from_rgb(0.80, 0.30, 0.32);

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
    // Home is window-local too, and sits under the talent viewer in the same
    // stack: `ClientState` keeps ticking below both, screen untouched.
    let content: Element<'_, Message> = match (&state.history, &state.home, app.screen) {
        // History sits above Home in the same window-local stack.
        (Some(h), _, _) => crate::history::screen(
            h,
            state.owner_guid.as_deref(),
            accent_of(state),
            state.cfg.density(),
            state.cfg.show_ranks,
            state.cfg.hide_realms,
        ),
        (None, Some(ui), _) => crate::home::screen(
            ui,
            &state.home_panels,
            &state.season,
            accent_of(state),
            state.cfg.density(),
            state.cfg.hide_realms,
        ),
        (None, None, Screen::List) => list_screen(app),
        (None, None, Screen::Meter) => meter_screen(state),
        (None, None, Screen::Compare) => compare_screen(state),
    };
    // The top bar runs edge to edge (`.top`); a screen sits in the window's
    // 10 px frame under it — bar a fight's workspace (the meter and the
    // comparison: `.stage`), whose tabs' and headings' hairlines, selected
    // rows and total run edge to edge too, each piece carrying the
    // prototype's own inset instead.
    let stage = state.history.is_none()
        && state.home.is_none()
        && matches!(app.screen, Screen::Meter | Screen::Compare);
    let frame = if stage {
        iced::Padding {
            top: 8.0,
            ..iced::Padding::ZERO
        }
    } else {
        iced::Padding {
            top: 8.0,
            right: 10.0,
            bottom: 10.0,
            left: 10.0,
        }
    };
    let body = column![
        chrome(state),
        container(content)
            .padding(frame)
            .width(Length::Fill)
            .height(Length::Fill),
    ]
    .width(Length::Fill)
    .height(Length::Fill);
    if state.shortcuts_open {
        stack![
            body,
            nav::shortcut_sheet(state.surface(), Message::ToggleShortcuts)
        ]
        .into()
    } else if state.options_open {
        stack![body, options_panel(&state.cfg, accent_of(state))].into()
    } else if state.picker_open {
        // The picker's menu, over whichever screen the picker was pressed
        // on: Home lists its own characters, the strip the window's memory
        // of them.
        let on_history = state.history.as_ref().is_some_and(|h| h.stored.is_none());
        let picks: Vec<nav::CharPick> = if state.home.is_some() && state.history.is_none() {
            state
                .home_panels
                .characters
                .iter()
                .filter(|c| !c.guid.is_empty())
                .map(crate::home::char_pick)
                .collect()
        } else {
            state
                .known_characters
                .iter()
                .map(crate::home::char_pick)
                .collect()
        };
        stack![
            body,
            nav::character_menu(
                nav::Menu {
                    chars: &picks,
                    selected: if on_history {
                        state.history.as_ref().and_then(|h| h.character.as_deref())
                    } else {
                        state.owner_guid.as_deref()
                    },
                    // Only History widens, and widening never moves the lock.
                    everyone: on_history,
                    hide_realms: state.cfg.hide_realms,
                    hover: state.picker_hover,
                    at_end: state.home.is_none() && state.history.is_none(),
                },
                Message::PickerHover,
                |guid| match guid {
                    Some(guid) => Message::HomeCharacter(Some(guid)),
                    None => Message::HistoryCharacter(None),
                },
                Message::TogglePicker,
                accent_of(state),
            )
        ]
        .into()
    } else {
        body.into()
    }
}

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

/// The top bar every screen wears (`.top`): the wordmark, the places —
/// Home, the fight list, the live pin, History — the locked character's
/// picker, the gear and the help button. The views belong to a fight, so
/// they are not on this bar: the meter and the comparison draw `view_tabs`
/// under the fight header, a stored fight under its summary cards.
fn chrome(state: &Gui) -> Element<'static, Message> {
    let app = &state.state;
    let history_open = state.history.is_some();
    let home_open = state.home.is_some() && !history_open;
    // Live is a red dot while a pull is in progress, and a hollow ring when
    // nothing is — told apart by shape as well as colour, never yellow.
    let anything_live = app.entries().iter().any(|e| e.row.live);
    let tabs: Vec<nav::Tab<Message>> = vec![
        nav::Tab {
            lead: nav::Lead::None,
            label: "Home",
            active: home_open,
            on_press: Some(Message::ToggleHome),
        },
        nav::Tab {
            lead: nav::Lead::None,
            label: "Fights",
            active: !home_open && !history_open && app.screen == Screen::List,
            on_press: Some(Message::GotoList),
        },
        nav::Tab {
            lead: if anything_live {
                nav::Lead::Dot(theme::BAD)
            } else {
                nav::Lead::Ring(theme::INK_3)
            },
            label: "Live",
            active: !home_open
                && !history_open
                && app.screen != Screen::List
                && app.following_live(),
            on_press: Some(Message::GotoLive),
        },
        nav::Tab {
            lead: nav::Lead::None,
            label: "History",
            active: history_open,
            on_press: Some(Message::HistoryOpen(crate::history::Scope::All)),
        },
    ];
    let accent = accent_of(state);
    // The locked character, pickable from any screen. Home's title carries
    // the same picker as its name, so the bar only shows it elsewhere —
    // two on one screen would be one too many.
    let picks: Option<(Vec<nav::CharPick>, Option<String>, bool)> =
        (!home_open && !history_open && !state.known_characters.is_empty()).then(|| {
            (
                state
                    .known_characters
                    .iter()
                    .map(crate::home::char_pick)
                    .collect(),
                state.owner_guid.clone(),
                state.cfg.hide_realms,
            )
        });
    // Only the layout knows the width, and the wordmark is the one thing a
    // narrow window drops (`.mark` is hidden under 820 px).
    let strip = iced::widget::responsive(move |bounds| {
        let mut strip = row![].spacing(6).align_y(iced::Alignment::Center);
        if bounds.width >= theme::NARROW {
            strip = strip.push(nav::wordmark::<Message>());
        }
        // The places sit on the bar's bottom edge, so the active one's
        // underline lands on the bar's hairline (`.place`).
        strip = strip.push(
            container(nav::tab_bar(tabs.clone(), accent, nav::Strip::Places))
                .width(Length::Fill)
                .height(Length::Fill)
                .align_y(iced::Alignment::End),
        );
        if let Some((picks, owner, hide_realms)) = &picks {
            strip = strip.push(nav::character_picker(
                picks,
                owner.as_deref(),
                false,
                *hide_realms,
                Message::TogglePicker,
                size::PLACE,
            ));
        }
        // The gear lives here, on every screen: its options (ranks, realm
        // names, the chrome) apply to every screen, so the switch is never
        // a screen away.
        strip
            .push(nav::gear(Message::ToggleOptions))
            .push(nav::help_glyph(Message::ToggleShortcuts))
            .height(Length::Fixed(pitch::TOP_BAR))
            .into()
    });
    // The bar (`.top`): the panel's surface, edge to edge, a hairline along
    // its bottom that the active place's underline covers.
    stack![
        container(Space::new())
            .width(Length::Fill)
            .height(Length::Fixed(pitch::TOP_BAR))
            .style(|_: &Theme| container::Style {
                background: Some(theme::SURFACE.into()),
                ..container::Style::default()
            }),
        container(nav::hairline::<Message>())
            .width(Length::Fill)
            .height(Length::Fixed(pitch::TOP_BAR))
            .align_y(iced::Alignment::End),
        container(strip)
            .padding([0, 10])
            .width(Length::Fill)
            .height(Length::Fixed(pitch::TOP_BAR)),
    ]
    .width(Length::Fill)
    .height(Length::Fixed(pitch::TOP_BAR))
    .into()
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

/// Does this row answer to `needle` (already folded — see [`crate::fold`])?
///
/// Substring, so "prot" finds Protection and "resto" finds Restoration
/// without an abbreviation table, and "protection" legitimately matches both
/// Protection Warrior and Protection Paladin — the class name is how a
/// reader narrows that, not a bug. A row whose class or spec R8 has not
/// inferred yet answers to neither term; it is not matched by everything,
/// and an empty filter still keeps it.
///
/// Every field goes through the SAME folding comparison, so an accented
/// class or spec name folds exactly as a player name does and the two paths
/// cannot drift.
fn row_matches(r: &Row, needle: &[char]) -> bool {
    fold::contains(&r.label, needle)
        || r.class.is_some_and(|c| fold::contains(c.name(), needle))
        || r.spec.is_some_and(|s| fold::contains(s.name(), needle))
        || r.spec
            .is_some_and(|s| fold::contains(s.role().name(), needle))
}

/// The same filter, keeping each row's position in the UNFILTERED list. The
/// index is both the rank the row displays and the one a click sends back to
/// `ClientState`, so it must survive filtering or a filtered click would
/// drill into the wrong player.
pub(crate) fn filtered_indexed(rows: Vec<Row>, filter: &str) -> Vec<(usize, Row)> {
    // Folded ONCE per call, not once per row: this runs on every snapshot at
    // 10 Hz over a whole raid's rows.
    let needle = fold::fold(filter);
    rows.into_iter()
        .enumerate()
        .filter(|(_, r)| needle.is_empty() || row_matches(r, &needle))
        .collect()
}

/// The rows as DRAWN: filtered, then sorted by the chosen column, each with
/// the index the daemon gave it — the index a click sends back and the
/// rank a row keeps. Ranks and shares are never recomputed: sorting by crit
/// asks a different question of the same chart, it does not make a new one.
pub(crate) fn ordered(
    rows: Vec<Row>,
    filter: &str,
    sort: Option<(table::Col, bool)>,
) -> Vec<(usize, Row)> {
    table::sorted(filtered_indexed(rows, filter), sort)
}

// ---- the segment list ------------------------------------------------------

fn list_screen(app: &ClientState) -> Element<'static, Message> {
    let source = match app.source.as_deref() {
        Some(name) => name.to_string(),
        None => "waiting for a combat log…".to_string(),
    };
    let header = row![
        text(source)
            .size(size::TITLE)
            .color(theme::INK)
            .font(theme::UI_SEMIBOLD),
        Space::new().width(Length::Fill),
        text("Encounters").size(size::LABEL).color(theme::GOLD_DIM),
    ]
    .align_y(iced::Alignment::Center)
    .spacing(8);

    let rows = app.list_rows();
    let selected = app.list_selection();
    let mut list = column![].spacing(2);
    if rows.is_empty() {
        list = list.push(
            text("no encounters indexed yet")
                .size(size::BODY)
                .color(theme::INK_2),
        );
    }
    for (i, r) in rows.iter().enumerate() {
        list = list.push(list_row(i, r, i == selected));
    }

    let mut screen = column![
        header,
        scrollable(scroll_clear(list))
            .height(Length::Fill)
            .width(Length::Fill),
    ]
    .spacing(8)
    .height(Length::Fill);
    if let Some(footer) = footer(app) {
        screen = screen.push(footer);
    }
    screen.into()
}

fn list_row(i: usize, r: &ListRow, selected: bool) -> Element<'static, Message> {
    let tag = if r.live {
        Some(nav::Badge::live())
    } else {
        let (word, color) = match (r.kind, r.success) {
            // R13: an arena match's outcome is the home team's, not a boss's.
            (SegmentKind::Encounter, Some(true)) if r.arena => ("WIN", theme::GOOD),
            (SegmentKind::Encounter, Some(false)) if r.arena => ("LOSS", theme::BAD),
            (SegmentKind::Encounter, Some(true)) => ("KILL", theme::GOOD),
            (SegmentKind::Encounter, Some(false)) => ("WIPE", theme::BAD),
            // R10: a completed key reads as timed/depleted.
            (SegmentKind::Overall, Some(true)) => ("TIMED", theme::GOOD),
            (SegmentKind::Overall, Some(false)) => ("OVER", theme::BAD),
            (SegmentKind::Encounter | SegmentKind::Overall, None) | (SegmentKind::Trash, _) => {
                ("", theme::INK_3)
            }
        };
        (!word.is_empty()).then(|| nav::Badge::new(word, color))
    };
    // A fight in ink, the visit's Σ in secondary ink — a summary, not a
    // highlight — and trash quieter still.
    let name_color = match r.kind {
        SegmentKind::Encounter => theme::INK,
        SegmentKind::Overall => theme::INK_2,
        SegmentKind::Trash => theme::INK_3,
    };
    // R10: the Overall header row wears a Σ so it can't be mistaken for a
    // fight with the instance's name.
    let name = match r.kind {
        SegmentKind::Overall => format!("Σ {}", r.name),
        _ => r.name.clone(),
    };
    let mut line = row![
        text(name).size(size::BODY).color(name_color),
        Space::new().width(Length::Fill),
    ]
    .spacing(10)
    .align_y(iced::Alignment::Center);
    if let Some(tag) = &tag {
        line = line.push(nav::badge(tag));
    }
    let line = line.push(
        text(duration(r.duration_ms))
            .size(size::SMALL)
            .color(theme::INK_2),
    );

    mouse_area(
        container(line)
            .padding([4, 8])
            .width(Length::Fill)
            .style(move |_: &Theme| row_style_in(&Look::WINDOW, selected)),
    )
    .on_press(Message::ListRow(i))
    .into()
}

// ---- the meter -------------------------------------------------------------

/// The meter: the fight header, the view tabs with the row filter at their
/// end, and the table — or, drilled, the drill under the header's title
/// line. Nothing else stands between the tabs and the first row.
fn meter_screen(state: &Gui) -> Element<'static, Message> {
    let app = &state.state;
    let drilled = app.drill.is_some();
    // The filter narrows the PLAYER list, so it belongs to that list: a
    // drill's panes are abilities and targets, where a player's name matches
    // nothing and would blank both panes. Drawn only where it applies —
    // `Gui::filter_visible` agrees, so `/` cannot focus a field that is not
    // on screen.
    let filter = (!drilled).then(|| {
        nav::filter_box(
            &state.filter,
            state.filter_focused,
            // It narrows whatever the rows are: on Enemies, the enemies.
            if app.view == View::EnemyTaken {
                "Filter enemies"
            } else {
                "Filter players"
            },
            Message::Filter,
            Message::Filter(String::new()),
            Message::FocusFilter,
            Message::FilterDone,
        )
    });
    let mut content = column![
        crate::fight_head::view(state, !drilled),
        view_tabs_with(accent_of(state), app.view, false, filter, STAGE_TABS),
    ];
    content = content.push(if drilled {
        // A drill keeps the window's frame: its panes are the inspector's
        // to lay out edge to edge, a step still to come.
        container(drill_body(state))
            .padding(STAGE_BODY)
            .height(Length::Fill)
            .into()
    } else {
        meter_table(MeterList::meter(state))
    });
    if let Some(footer) = footer(app) {
        content = content.push(container(footer).padding(STAGE_FOOTER));
    }
    content.height(Length::Fill).into()
}

/// A fight's workspace runs edge to edge (`.stage`), each piece in its own
/// inset: the view tabs' row (`.vtabs{padding:0 12px 0 10px}`), what sits
/// under them in the window's frame (a drill, the comparison) and the
/// status line.
const STAGE_TABS: iced::Padding = iced::Padding {
    top: 0.0,
    right: 12.0,
    bottom: 0.0,
    left: 10.0,
};
const STAGE_BODY: iced::Padding = iced::Padding {
    top: 8.0,
    right: 10.0,
    bottom: 10.0,
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

/// Header badge for the watched segment: LIVE while accumulating, else
/// success worded by kind — KILL/WIPE for fights, TIMED/OVER for a keyed
/// visit's overall (R10).
pub(crate) fn header_tag(app: &ClientState) -> (&'static str, Color) {
    if app.is_live() {
        return ("LIVE", YELLOW);
    }
    match verdict(app) {
        Some((word, true)) => (word, GREEN),
        Some((word, false)) => (word, RED),
        None => ("", DIM),
    }
}

/// A closed segment's outcome: its word, and whether it went well. `None`
/// while it has none (trash, an unfinished pull).
fn verdict(app: &ClientState) -> Option<(&'static str, bool)> {
    let overall = app.segment_kind() == Some(SegmentKind::Overall);
    let good = app.segment_success()?;
    Some(match (good, overall) {
        // R13: arena matches word the home team's outcome.
        (true, false) if app.segment_arena() => ("WIN", true),
        (false, false) if app.segment_arena() => ("LOSS", false),
        (true, false) => ("KILL", true),
        (false, false) => ("WIPE", false),
        (true, true) => ("TIMED", true),
        (false, true) => ("OVER", false),
    })
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

/// "Keanucleavês-Proudmoore-US" → "Keanucleavês". Character names cannot
/// contain '-', so everything from the first dash is realm noise.
pub(crate) fn display_name(label: &str) -> &str {
    label.split('-').next().unwrap_or(label)
}

/// `label` with a player's realm taken off wherever one is written: a whole
/// label ("Bearlysimpin-Proudmoore-US" → "Bearlysimpin"), or the source in
/// the parentheses a recap line or an ability wears ("Word of Glory
/// (Soundscape-Proudmoore-US)" → "Word of Glory (Soundscape)"). For the
/// panes whose rows are players and creatures alike — a drill's targets and
/// attackers, a recap — so, unlike [`display_name`], it touches only what
/// reads as a player's "Name-Realm-Region": a creature's hyphen ("Yogg-
/// Saron", "Blood-Queen Lana'thel") is part of its name, not a realm.
pub(crate) fn realmless(label: &str) -> String {
    if let Some(head) = label.strip_suffix(')')
        && let Some((what, who)) = head.rsplit_once(" (")
    {
        return match player_name(who) {
            Some(name) => format!("{what} ({name})"),
            None => label.to_string(),
        };
    }
    player_name(label).map_or_else(|| label.to_string(), str::to_string)
}

/// The name in a player label the log writes as "Name-Realm-Region" (the
/// region two capitals, no part of it holding a space), else `None`.
fn player_name(label: &str) -> Option<&str> {
    let mut parts = label.split('-');
    let name = parts.next().filter(|n| !n.is_empty() && !n.contains(' '))?;
    let rest: Vec<&str> = parts.collect();
    let region = rest.last()?;
    let shaped = rest.len() >= 2
        && region.len() == 2
        && region.chars().all(|c| c.is_ascii_uppercase())
        && rest.iter().all(|p| !p.is_empty() && !p.contains(' '));
    shaped.then_some(name)
}

/// `rows` as the window draws them in a pane of mixed players and
/// creatures: realms taken off every label when the option says so.
pub(crate) fn realmless_rows(rows: &[Row], hide_realms: bool) -> Vec<Row> {
    rows.iter()
        .map(|r| {
            if hide_realms {
                Row {
                    label: realmless(&r.label),
                    ..r.clone()
                }
            } else {
                r.clone()
            }
        })
        .collect()
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

/// A meter-shaped list — the live meter, or an enemy's attackers (R24) —
/// as owned data, so [`meter_table`] can lay it out at whatever size the
/// window gives it: the prototype keeps only the amount and the rate in a
/// narrow window, and the total row sits under a short list but pins to
/// the bottom of a long one — and only the layout knows either.
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
    /// are comparison picks. `None` for an attacker list, whose are not.
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
    /// The enemy drill's attackers rather than the meter.
    attackers: bool,
    /// A row's pitch, by the configured density.
    row_h: f32,
    /// The owner's row, by daemon index, and their class: it wears the
    /// "you" tag and its rank in their colour.
    owner: Option<(usize, Option<Class>)>,
    /// A filter narrows what is drawn: the total folds the drawn rows.
    filtered: bool,
}

impl MeterList {
    /// The live meter, as the window's filter, sort and options draw it.
    fn meter(state: &Gui) -> Self {
        let app = &state.state;
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
            slots: Some(all.iter().map(|r| app.compare_slot(&r.key)).collect()),
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
            attackers: false,
            row_h: state.cfg.density().row_h(),
            filtered: !state.filter.is_empty(),
        }
    }

    /// R24: the enemy drill's attacker list — the meter's row shape over
    /// the by-attacker rows (players, class and spec on them), a click
    /// descending into that attacker's abilities on the enemy.
    fn attackers(state: &Gui, rows: &[Row]) -> Self {
        let app = &state.state;
        let drawn = rows
            .iter()
            .cloned()
            .map(|r| {
                if state.cfg.hide_realms {
                    Row {
                        label: display_name(&r.label).to_string(),
                        ..r
                    }
                } else {
                    r
                }
            })
            .enumerate()
            .collect();
        Self {
            all: rows.to_vec(),
            drawn,
            slots: None,
            selected: app.drill.as_ref().map_or(0, |d| d.target_sel),
            hover: state.hover_in(Pane::Target),
            view: app.view,
            sort: None,
            show_ranks: state.cfg.show_ranks,
            split: None,
            enemies: false,
            attackers: true,
            row_h: state.cfg.density().row_h(),
            owner: None,
            filtered: false,
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
    let covered = state.talents.is_some() || state.home.is_some() || state.history.is_some();
    if covered || state.state.screen != Screen::Meter || state.state.drill.is_some() {
        return None;
    }
    MeterList::meter(state).extent(row)
}

/// The meter's table — the headings, the rows and the total — laid out at
/// the size it is given.
fn meter_table(list: MeterList) -> Element<'static, Message> {
    iced::widget::responsive(move |bounds| {
        // The live meter runs the window's width, edge to edge; an
        // attacker list sits in a drill's 10 px frame.
        let narrow = if list.attackers {
            bounds.width < theme::NARROW
        } else {
            bounds.width < theme::NARROW_WINDOW
        };
        meter_table_at(&list, narrow, bounds.height)
    })
    .into()
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
fn total_follows(rows_h: f32, height: f32) -> bool {
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
    // own lead (`.trow{padding-left:4px}` and a 30 px rank column); an
    // attacker list sits in a drill's frame.
    let row_lead = if l.attackers { 0.0 } else { ROW_LEAD };
    // Where the icon column starts (`.who2`): the heading over the names
    // and the total's label start there too, as the prototype's do.
    let who = row_lead + rank_w + ICON_X;
    let lead = row![
        Space::new().width(Length::Fixed((who - HEAD_PAD).max(0.0))),
        text(if l.enemies { "Enemy" } else { "Player" })
            .size(size::LABEL)
            .color(theme::GOLD_DIM)
            .wrapping(text::Wrapping::None),
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
            text(if l.attackers {
                "nothing landed yet"
            } else {
                "nothing to show for this view yet"
            })
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
        // an enemy row wears the skull disc, and is never a pick.
        let icon: Element<'static, Message> = match &l.slots {
            _ if l.enemies => compare::enemy_icon(None, 18.0),
            Some(slots) => mouse_area(compare::class_icon(
                r.class,
                r.spec,
                slots.get(i).copied().flatten(),
                18.0,
            ))
            .on_press(Message::CompareRow(i))
            .into(),
            None => compare::class_icon(r.class, r.spec, None, 18.0),
        };
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
            name_tags(r.spec.map(|s| s.role()), mine),
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
        // the hover's breath is for the other rows.
        let hovered = l.hover == Some(i);
        let selected = i == l.selected;
        let line = container(line.push(bar))
            .padding(iced::Padding {
                left: row_lead,
                ..iced::Padding::ZERO
            })
            .style(move |_: &Theme| {
                if selected {
                    row_style_in(&Look::WINDOW, true)
                } else {
                    hover_style_in(&Look::WINDOW, hovered)
                }
            });
        let (press, hover) = if l.attackers {
            (Message::AttackerRow(i), RowHover::Drill(Pane::Target, i))
        } else {
            (Message::MeterRow(i), RowHover::Meter(i))
        };
        list = list.push(
            mouse_area(line)
                .on_press(press)
                .on_enter(Message::HoverRow(Some(hover)))
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
    let rows: Element<'static, Message> = if l.attackers {
        scroller.into()
    } else {
        mouse_area(scroller)
            .on_right_press(Message::ClearCompare)
            .into()
    };
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
    // the list and its scrollbar lane; an attacker list's folds every
    // column, as a drill pane's does.
    let total = if l.attackers {
        scroll_clear(table::total(
            cols,
            grid,
            &ours,
            label,
            lead_pad,
            table::Fold::Full,
        ))
    } else {
        container(table::total(
            cols,
            grid,
            &ours,
            label,
            lead_pad,
            table::Fold::Meter {
                filtered: l.filtered,
            },
        ))
    };
    #[cfg(test)]
    let total = total.id(meter_total_id());
    column![
        // The list sits inside `scroll_clear`'s scrollbar gutter, so the
        // headings wear the same gutter to keep columns, and the total
        // (`Fold::Meter`) keeps it inside its full-width surface.
        scroll_clear(heads),
        nav::hairline::<Message>(),
        rows,
        total,
    ]
    .height(Length::Fill)
    .into()
}

/// What follows a name on its line (`.who2`): a tank's shield or a
/// healer's cross in faint ink (`.role`), then the owner's "you" tag —
/// `you` is `Some(their class)` on the owner's row. Each is a fixed width,
/// and the name leaves room for them all, so they stay beside it.
fn name_tags(
    role: Option<Role>,
    you: Option<Option<Class>>,
) -> Option<(Element<'static, Message>, f32)> {
    let glyph = match role {
        Some(Role::Tank) => Some(LineIcon::Shield),
        Some(Role::Healer) => Some(LineIcon::Cross),
        Some(Role::Dps) | None => None,
    };
    let mut tags = row![].spacing(TAG_GAP).align_y(iced::Alignment::Center);
    let mut widths = Vec::new();
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

/// A role glyph (`.role svg`), the "you" tag's width and the gap between
/// what follows a name.
const ROLE_GLYPH: f32 = 13.0;
const YOU_TAG_W: f32 = 26.0;
const TAG_GAP: f32 = 8.0;
/// The tag's line and corners (`.youtag{line-height:15px;border-radius:4px}`).
const YOU_TAG_LINE: f32 = 15.0;
const YOU_TAG_RADIUS: f32 = 4.0;

/// The owner's tag (`.youtag`): "you" at 11.5 px, 600, in the owner's text
/// colour (`--you-text`, AA on the selected row's RAISE where a player's
/// name colour is not), on a 1 px frame of their class colour.
fn you_tag(class: Option<Class>) -> Element<'static, Message> {
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

// ---- the comparison (R12) --------------------------------------------------

fn compare_screen(state: &Gui) -> Element<'static, Message> {
    let app = &state.state;
    let (hover, spell_hover, probe) = (
        state.compare_hover.clone(),
        state.spell_hover.clone(),
        state.graph_probe,
    );
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
        on_spell_hover: std::rc::Rc::new(Message::CompareSpellHover),
        spell_hover,
    };
    let mut screen = column![
        // The fight's title line: the pair has figures of its own.
        crate::fight_head::view(state, false),
        view_tabs_with(accent_of(state), app.view, false, None, STAGE_TABS),
        // R12: right-click anywhere else on the body clears the pair and
        // returns to the meter — pointer parity with Esc.
        container(
            mouse_area(compare::compare_body_in(
                &Look::WINDOW,
                app,
                WS,
                120.0,
                true,
                ctl
            ))
            .on_right_press(Message::ClearCompare)
        )
        .padding(STAGE_BODY)
        .height(Length::Fill),
    ];
    if let Some(footer) = footer(app) {
        screen = screen.push(container(footer).padding(STAGE_FOOTER));
    }
    screen.height(Length::Fill).into()
}

fn drill_body(state: &Gui) -> Element<'static, Message> {
    let app = &state.state;
    let Some(drill) = app.drill.as_ref() else {
        return meter_table(MeterList::meter(state));
    };
    // Who the drill is about: their name as the meter draws it, in their
    // class colour lifted to read as text. An enemy (R24) has no class, and
    // its hyphen is its name's.
    let hide_realms = state.cfg.hide_realms;
    let class = app
        .rows()
        .iter()
        .find(|r| r.key == drill.key)
        .and_then(|r| r.class);
    let who = match (hide_realms, class) {
        (false, _) => drill.label.clone(),
        (true, Some(_)) => display_name(&drill.label).to_string(),
        (true, None) => realmless(&drill.label),
    };
    let who_ink = class.map_or(theme::INK, theme::class_text);
    // v16: the second level — one ability, its stats and its own curve over
    // the player's ghosted one.
    if let Some((_, spell_label)) = app.drill_spell().cloned() {
        let spell_row = app.drill_spell_row();
        // A CC ability names its target ("Polymorph (Name-Realm-US)").
        let spell_label = if state.cfg.hide_realms {
            realmless(&spell_label)
        } else {
            spell_label
        };
        let mut body = column![spell_breadcrumb_in::<Message>(
            &Look::WINDOW,
            &who,
            Some(who_ink),
            &spell_label,
            spell_row.as_ref(),
            WS
        ),]
        .spacing(10);
        match &spell_row {
            Some(r) => body = body.push(spell_stats_in::<Message>(&Look::WINDOW, r, app.view, WS)),
            None => {
                body = body.push(text("No data yet").size(size::SMALL).color(theme::INK_2));
            }
        }
        // v17: who the ability landed on. The meter's filter is a player
        // filter and does not reach here — narrowing to one player and then
        // drilling into them must not empty the pane.
        let targets = realmless_rows(&app.spell_target_rows(), state.cfg.hide_realms);
        body = body
            .push(
                row![
                    // R24: on the enemy view the second level is the
                    // attacker's abilities on the enemy, not targets.
                    text(if app.view == View::EnemyTaken {
                        "Abilities"
                    } else {
                        "Targets"
                    })
                    .size(size::LABEL)
                    .color(theme::GOLD_DIM),
                    Space::new().width(Length::Fill),
                    text("Hits · total · share")
                        .size(size::LABEL)
                        .color(theme::GOLD_DIM),
                ]
                .padding([0, 8]),
            )
            .push(spell_target_list_in::<Message>(
                &Look::WINDOW,
                &targets,
                pitch::TARGET_ROW,
                WS,
            ));
        if let Some(t) = app.drill_timeline().filter(|t| !t.buckets.is_empty()) {
            let focus_color = spell_row
                .as_ref()
                .and_then(|r| school_color(r.school))
                .unwrap_or(Look::WINDOW.focus);
            let ctl = compare::GraphCtl {
                on_range: std::rc::Rc::new(Message::DrillRange),
                on_hover: std::rc::Rc::new(Message::CompareHover),
                hover: state.compare_hover.clone(),
                on_probe: std::rc::Rc::new(Message::GraphProbe),
                probe: state.graph_probe,
                on_spell: std::rc::Rc::new(Message::CompareSpell),
                on_spell_hover: std::rc::Rc::new(Message::CompareSpellHover),
                spell_hover: state.spell_hover.clone(),
            };
            let focus = app.spell_timeline().map(|ft| (ft, focus_color));
            let rate = rate_label(app.view);
            // Same height as the player drill's graph: consistent chart,
            // more room for the targets.
            body = body.push(compare::drill_graph_in(
                &Look::WINDOW,
                app,
                t,
                class,
                WS,
                110.0,
                rate,
                true,
                focus,
                ctl,
            ));
        }
        return body.into();
    }

    // The panes mix players and creatures (a heal's targets, a recap's
    // sources): the option takes realms off what reads as a player.
    let (by_spell, by_target) = app.breakdown();
    let (by_spell, by_target) = (
        realmless_rows(&by_spell, hide_realms),
        realmless_rows(&by_target, hide_realms),
    );
    let title = row![
        text(who)
            .size(size::TITLE)
            .color(who_ink)
            .font(theme::UI_SEMIBOLD),
        text(format!("— {}", window_view_name(app.view)))
            .size(size::BODY)
            .color(theme::INK_2),
    ]
    .spacing(8)
    .align_y(iced::Alignment::Center);

    // Deaths drill into the recap timeline + attacker totals (R9); Taken
    // (R17) into what hit the player and who swung it.
    let recap = app.view == View::Deaths;
    let (spell_title, target_title) = match app.view {
        View::Deaths => ("Death recap", "By attacker"),
        View::Taken | View::EnemyTaken => ("By ability", "By attacker"),
        _ => ("By spell", "By target"),
    };
    // What the pane's number means in this view, so the columns are as
    // self-describing as the meter's caption line.
    let caption = match app.view {
        View::Damage | View::Healing | View::Deaths => "total",
        View::Taken | View::EnemyTaken => "taken",
        View::Interrupts | View::CrowdControl | View::Dispels => "count",
    };
    // The spell pane is the throughput table and carries six columns, so
    // it takes the larger share; the target pane's three fit the rest.
    // R24: the enemy drill is ONE list — the attackers, sorted — and a
    // click on one descends into their abilities on this enemy.
    let enemy = app.view == View::EnemyTaken;
    let target_pane = || {
        container(drill_pane(
            target_title,
            caption,
            &by_target,
            false,
            drill.pane == Pane::Target,
            drill.target_sel,
            enemy.then_some(Message::AttackerRow as fn(usize) -> Message),
            Pane::Target,
            state.hover_in(Pane::Target),
            table::TARGETS,
            // R9: a death window's attackers are amounts, not the Deaths
            // meter's counts, so the headings word them as damage.
            if recap { View::Damage } else { app.view },
            None,
            None,
        ))
    };
    let panes: Element<'static, Message> = if enemy {
        // The attackers are PLAYERS, drawn exactly like the meter's rows:
        // rank, class icon, class-colored bar, the meter's columns.
        let _ = target_pane;
        meter_table(MeterList::attackers(state, &by_target))
    } else {
        row![
            container(drill_pane(
                spell_title,
                if recap { "Amount · hp" } else { caption },
                &by_spell,
                recap,
                drill.pane == Pane::Spell,
                drill.spell_sel,
                // v16: clicking a spell row descends into the ability.
                (!recap).then_some(Message::SpellRow as fn(usize) -> Message),
                Pane::Spell,
                state.hover_in(Pane::Spell),
                table::SPELLS,
                app.view,
                state.drill_sort,
                Some(Message::SortSpellsBy),
            ))
            .width(Length::FillPortion(3)),
            target_pane().width(Length::FillPortion(2)),
        ]
        .spacing(10)
        .height(Length::Fill)
        .into()
    };

    let mut body = column![title].spacing(6);
    // v28 (R9): the death navigator over a Deaths drill — every window
    // the player has, the described one lit; ← → step them.
    if recap {
        let (deaths, shown) = app.deaths();
        let dropped = app.drill_breakdown().map_or(0, |b| b.deaths_dropped);
        if let Some(chips) =
            crate::taken::death_chips(deaths, shown, dropped, accent_of(state), Message::PickDeath)
        {
            body = body.push(chips);
        }
    }
    // R17: the mitigation record over a Taken drill's panes — cards and
    // miss chips, where one sentence used to be.
    if app.view == View::Taken
        && let Some(m) = app.drill_mitigation()
    {
        let taken = app
            .rows()
            .iter()
            .find(|r| r.key == drill.key)
            .map_or(0, |r| r.amount);
        body = body.push(nav::stat_cards::<Message>(
            &crate::taken::mitigation_cards(m, taken, app.duration_ms()),
            state.cfg.density(),
        ));
        if let Some(chips) = crate::taken::miss_chips::<Message>(m) {
            body = body.push(container(chips).padding([0, 8]));
        }
    }
    // R21: the stack ledger is its own section, reached by a jump chip —
    // the matrix is tall, and stacked under the panes it starved them.
    let ledger = app
        .drill_stacks()
        .map(|(stacking, cells, base)| crate::taken::matrices(stacking, cells, base))
        .unwrap_or_default();
    if !ledger.is_empty() {
        body = body.push(nav::chip_row(
            vec![
                ("Breakdown".to_string(), Message::ShowStacks(false)),
                ("Stacks".to_string(), Message::ShowStacks(true)),
            ],
            Some(usize::from(state.stacks_open)),
            accent_of(state),
        ));
    }
    if state.stacks_open && !ledger.is_empty() {
        let dropped = app.drill_breakdown().map_or(0, |b| b.stacks_dropped);
        if let Some(el) = crate::taken::stack_matrix::<Message>(&ledger, dropped) {
            body = body.push(
                scrollable(scroll_clear(el))
                    .width(Length::Fill)
                    .height(Length::Fill),
            );
        }
    } else {
        body = body.push(panes);
    }
    // v14: the player's timeline under the panes — the comparison's graph
    // for one side (Damage view only; the daemon sends no timeline
    // otherwise). Drag zooms client-side, right-click zooms out, `g`
    // toggles the curve.
    if let Some(t) = app.drill_timeline().filter(|t| !t.buckets.is_empty()) {
        let ctl = compare::GraphCtl {
            on_range: std::rc::Rc::new(Message::DrillRange),
            on_hover: std::rc::Rc::new(Message::CompareHover),
            hover: state.compare_hover.clone(),
            on_probe: std::rc::Rc::new(Message::GraphProbe),
            probe: state.graph_probe,
            on_spell: std::rc::Rc::new(Message::CompareSpell),
            on_spell_hover: std::rc::Rc::new(Message::CompareSpellHover),
            spell_hover: state.spell_hover.clone(),
        };
        let rate = rate_label(app.view);
        body = body.push(compare::drill_graph_in(
            &Look::WINDOW,
            app,
            t,
            class,
            WS,
            110.0,
            rate,
            true,
            None,
            ctl,
        ));
    }
    body.into()
}

/// How a view words its per-second rate: `dps`, `hps`, or `dtps` for damage
/// taken (R17). Count views never show one; they read `dps` here only
/// because nothing asks them.
pub(crate) fn rate_label(view: View) -> &'static str {
    match view {
        View::Healing => "hps",
        View::Taken | View::EnemyTaken => "dtps",
        _ => "dps",
    }
}

/// R17: the drilled player's mitigation record as one line, when the view
/// is Taken and the daemon sent one. Shared by the window and the overlay.
pub(crate) fn drill_mitigation_line(app: &ClientState) -> Option<String> {
    if app.view != View::Taken {
        return None;
    }
    let drill = app.drill.as_ref()?;
    let m = app.drill_mitigation()?;
    let taken = app
        .rows()
        .iter()
        .find(|r| r.key == drill.key)
        .map_or(0, |r| r.amount);
    Some(mitigation_line(m, taken))
}

pub(crate) use wowdps_model::fmt::mitigation_line;

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
    // Which pane this is, and the row the pointer is over in it: a drill is
    // read with the mouse, and a list with no hover mark gives it nothing
    // back. Inert rows light up too — the highlight says "this is the line
    // you are reading", not "this is clickable".
    pane: Pane,
    hover: Option<usize>,
    // The pane's column set (the design study's throughput table on the
    // spell pane, a shorter one beside it), the view the headings word
    // themselves for, and the sort with its heading message — `None`
    // leaves the headings inert. A recap pane ignores all of this: its
    // rows are chronological and wear their own shape.
    cols: &'static [table::Col],
    view: View,
    sort: Option<(table::Col, bool)>,
    on_sort: Option<fn(table::Col) -> Message>,
) -> Element<'static, Message> {
    // A pane is as wide as the layout makes it — half a window, or half of
    // a narrow one — so its columns are chosen there: the least telling go
    // first ([`table::fit`]), and the names keep room to be read.
    let rows = rows.to_vec();
    iced::widget::responsive(move |bounds| {
        let cols = table::fit(cols, bounds.width - 10.0, DRILL_NAME_MIN);
        // A recap line keeps its amount and its health at every width;
        // the overkill beside them is the one that gives way.
        let fit = if bounds.width >= RECAP_OVER_MIN {
            RecapFit::Wide
        } else {
            RecapFit::Narrow
        };
        drill_pane_at(
            title, caption, &rows, recap, active, selected, click, pane, hover, &cols, view, sort,
            on_sort, fit,
        )
    })
    .into()
}

/// The room a drill row's name keeps before a column gives way: the
/// ability's icon, a dozen characters and the row's own padding.
const DRILL_NAME_MIN: f32 = 150.0;

/// The narrowest recap pane that shows a killing blow's "(N over)": its
/// three fixed cells at the window's scale, and a label still ~110 px
/// wide. A narrow window's pane (~250 px) drops the overkill instead of
/// letting the label shove the amount onto the health.
const RECAP_OVER_MIN: f32 = 340.0;

#[allow(clippy::too_many_arguments)]
fn drill_pane_at(
    title: &'static str,
    caption: &'static str,
    rows: &[Row],
    recap: bool,
    active: bool,
    selected: usize,
    click: Option<fn(usize) -> Message>,
    pane: Pane,
    hover: Option<usize>,
    cols: &[table::Col],
    view: View,
    sort: Option<(table::Col, bool)>,
    on_sort: Option<fn(table::Col) -> Message>,
    fit: RecapFit,
) -> Element<'static, Message> {
    let title_color = if active { theme::INK } else { theme::INK_2 };
    // Recap rows are chronological, not sorted, so the max is anywhere.
    let max = rows.iter().map(|r| r.amount).max().unwrap_or(1);
    // The meter's filter is a PLAYER filter and stops at the meter: these
    // rows are abilities and targets, and narrowing to a player before
    // drilling into them must not blank the panes. A sort reorders what is
    // drawn and keeps every row's index, exactly as the meter's does.
    let indexed: Vec<(usize, Row)> = rows.iter().cloned().enumerate().collect();
    let drawn = if recap {
        indexed
    } else {
        table::sorted(indexed, sort)
    };
    let mut list = column![];
    if drawn.is_empty() {
        list = list.push(text("—").size(size::SMALL).color(theme::INK_3));
    }
    for (i, r) in &drawn {
        let (i, r) = (*i, r);
        let el: Element<'static, Message> = if recap {
            recap_row_at(&Look::WINDOW, r, max, pitch::DRILL_ROW, WS, fit)
        } else {
            bar_row(
                r,
                max,
                active && i == selected,
                pitch::DRILL_ROW,
                Some(cols),
                1.0,
                None,
                None,
            )
        };
        let hovered = hover == Some(i);
        let el: Element<'static, Message> = container(el)
            .style(move |_: &Theme| hover_style_in(&Look::WINDOW, hovered))
            .into();
        let mut area = mouse_area(el)
            .on_enter(Message::HoverRow(Some(RowHover::Drill(pane, i))))
            .on_exit(Message::HoverRow(None));
        if let Some(f) = click {
            area = area.on_press(f(i));
        }
        list = list.push(area);
    }
    let heading: Element<'static, Message> = if recap {
        row![
            text(title)
                .size(size::BODY)
                .color(title_color)
                .font(theme::UI_MEDIUM),
            Space::new().width(Length::Fill),
            text(caption).size(size::LABEL).color(theme::GOLD_DIM),
        ]
        .padding([0, 8])
        .into()
    } else {
        let lead = row![
            Space::new().width(Length::Fixed(14.0)),
            text(title)
                .size(size::BODY)
                .color(title_color)
                .font(theme::UI_MEDIUM)
                .wrapping(text::Wrapping::None),
        ]
        .spacing(COL_GAP)
        .align_y(iced::Alignment::Center);
        table::heads(cols, table::Grid::Table, view, sort, on_sort, lead)
    };
    let mut pane_col = column![
        scroll_clear(heading),
        scrollable(scroll_clear(list))
            .height(Length::Fill)
            .width(Length::Fill),
    ]
    .spacing(4);
    // The pinned total row, so a per-ability number always has its
    // denominator on screen. The recap's rows are a story, not a sum.
    if !recap && !rows.is_empty() {
        pane_col = pane_col.push(scroll_clear(table::total::<Message>(
            cols,
            table::Grid::Table,
            rows,
            format!("Total · {}", rows.len()),
            14.0,
            table::Fold::Full,
        )));
    }
    pane_col.width(Length::Fill).into()
}

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

/// One class-colored bar with its labels on top. The bar's width is the row's
/// amount relative to `max`, the list's top amount ([`class_bar`]).
/// `compact` drops the secondary columns — drill
/// panes are half a window wide and clip anything more than name + amount.
/// Emits no messages, so it serves any frontend's message type. `scale`
/// multiplies the text sizes: the window renders at 1.0 and zooms through
/// iced's scale factor, but the overlay must zoom manually (iced_layershell
/// 0.19 does not scale pointer coordinates by a custom scale factor, which
/// breaks hit-testing).
#[allow(clippy::too_many_arguments)]
pub(crate) fn bar_row<M: 'static>(
    r: &Row,
    max: u64,
    selected: bool,
    height: f32,
    cols: Option<&[table::Col]>,
    scale: f32,
    rank: Option<usize>,
    // The class icon, drawn INSIDE the bar at its leading edge (the meter's
    // rows; a drill pane passes none). It arrives built so the caller can
    // give it its own click.
    icon: Option<Element<'static, M>>,
) -> Element<'static, M> {
    bar_row_tagged(
        r,
        max,
        selected,
        height,
        cols,
        table::Grid::Table,
        scale,
        rank,
        icon,
        None,
    )
}

/// [`bar_row`] with `tags` right after the name — the meter's role glyph
/// and "you" tag — and their width, which the name leaves room for: a long
/// name ends in "…" before a tag is pushed off its line.
#[allow(clippy::too_many_arguments)]
pub(crate) fn bar_row_tagged<M: 'static>(
    r: &Row,
    max: u64,
    selected: bool,
    height: f32,
    cols: Option<&[table::Col]>,
    // The widths and gap `cols` stand at: the live meter's own grid, or
    // every other table's.
    grid: table::Grid,
    scale: f32,
    rank: Option<usize>,
    icon: Option<Element<'static, M>>,
    tags: Option<(Element<'static, M>, f32)>,
) -> Element<'static, M> {
    let compact = cols.is_none();
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

    if compact {
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
    }

    // The window row: name and number columns over the bar, which runs
    // under the WHOLE row — name and number columns alike.
    let labels = container(labels)
        .padding(track_pad(scale))
        .width(Length::Fill)
        .height(Length::Fill);
    let metrics = table::cells::<M>(cols.unwrap_or(table::METER), grid, r, scale, false);
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

/// Gap between the meter row's columns, and between the caption headings
/// over them. One constant so the two cannot drift.
const COL_GAP: f32 = table::GAP;

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
pub(crate) enum RecapFit {
    /// The overlay's narrow panel: the source's realm stripped, the label
    /// laid out as it always was.
    Compact,
    /// The window: the label clipped to what is left of the line, so the
    /// fixed cells — overkill, amount, health — always keep their widths.
    Wide,
    /// The window in a narrow pane: [`RecapFit::Wide`] less the overkill,
    /// which the amount and the health outrank.
    Narrow,
}

/// [`recap_row_in`] fitted: see [`RecapFit`].
pub(crate) fn recap_row_at<M: 'static>(
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
    let sign = if r.gain { "+" } else { "" };
    let hp_txt =
        r.hp.map(|(cur, max_hp)| format!("{:.0}%", cur as f64 / max_hp.max(1) as f64 * 100.0))
            .unwrap_or_default();
    let amount = metric(
        format!("{sign}{}", human(r.amount)),
        12.0,
        if r.gain { look.good } else { look.hit },
        52.0,
    );
    let hp = metric(hp_txt, 11.0, look.dim, 40.0);
    let labels = if compact {
        // The overlay is narrow: strip realm suffixes from the attacker/
        // healer in parens, like the meter rows do for player names.
        let label = match r.label.split_once(" (") {
            Some((head, tail)) => {
                let who = tail.trim_end_matches(')');
                let short = who.split('-').next().unwrap_or(who);
                format!("{head} ({short})")
            }
            None => r.label.clone(),
        };
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
        card("crit", crit, Some(look.crit)),
        card("avg", avg, None),
    ]
    .spacing(6.0 * scale);
    if r.extra > 0 {
        let what = match view {
            View::Healing => "overheal",
            View::Taken | View::EnemyTaken => "absorbed",
            _ => "overkill",
        };
        line = line.push(card(what, human(r.extra), Some(look.bad)));
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
/// edge, at full strength when `lit` (the selection). One place, so a
/// History pull's bar and a meter row's cannot drift apart.
pub(crate) fn bar_ramp(color: Color, lit: bool) -> iced::Background {
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
fn row_style_in(look: &Look, selected: bool) -> container::Style {
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
/// keymap lives behind `?`, per screen, and the rows take the height.
fn footer(app: &ClientState) -> Option<Element<'static, Message>> {
    let status = app.status.as_deref().filter(|s| !s.trim().is_empty())?;
    Some(
        container(text(status.to_string()).size(size::SMALL).color(theme::BAD))
            .padding(iced::Padding {
                top: 6.0,
                ..iced::Padding::ZERO
            })
            .into(),
    )
}

/// The seven views as a tab strip of their own, under a fight's header:
/// they switch what the numbers on THIS fight mean, so they sit with the
/// numbers rather than on the front-door strip.
/// `stored`: a stored fight offers only the views the store writes (R24:
/// no ☠ tab on a card).
pub(crate) fn view_tabs(
    accent: theme::Accent,
    shown: View,
    stored: bool,
) -> Element<'static, Message> {
    view_tabs_with(accent, shown, stored, None, iced::Padding::ZERO)
}

/// [`view_tabs`] with `trailing` at the row's end — the meter's filter
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
        .filter(|v| !stored || v.is_stored())
        .map(|v| nav::Tab {
            lead: nav::Lead::Icon(LineIcon::of_view(v)),
            label: window_view_name(v),
            active: shown == v,
            on_press: Some(Message::PickView(v)),
        })
        .collect();
    nav::view_strip(tabs, accent, trailing, inset)
}

/// The window's views in the prototype's order (`VIEWS`): damage and
/// healing, then the two a raid reads next — what was taken and who died —
/// before the counts, and the enemies last. `View::ALL` (the TUI's, and the
/// overlay's cycle) keeps its own order.
pub(crate) const WINDOW_VIEWS: [View; 8] = [
    View::Damage,
    View::Healing,
    View::Taken,
    View::Deaths,
    View::Interrupts,
    View::CrowdControl,
    View::Dispels,
    View::EnemyTaken,
];

/// A view's name in the window, in the prototype's sentence case and its
/// words ("Crowd control", "Enemies"). The overlay keeps `view_name`'s.
pub(crate) fn window_view_name(v: View) -> &'static str {
    match v {
        View::Damage => "Damage",
        View::Healing => "Healing",
        View::Taken => "Taken",
        View::Deaths => "Deaths",
        View::Interrupts => "Interrupts",
        View::CrowdControl => "Crowd control",
        View::Dispels => "Dispels",
        View::EnemyTaken => "Enemies",
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
        assert!(ui.find("Kill").is_ok());
        assert!(ui.find("Wipe").is_ok());
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
        assert!(has(list_row(0, &live, true), "Live"));
        let mut win = list_entry(SegmentKind::Encounter, Some(true));
        win.arena = true;
        assert!(has(list_row(0, &win, false), "Win"));
        let mut loss = list_entry(SegmentKind::Encounter, Some(false));
        loss.arena = true;
        assert!(has(list_row(0, &loss, false), "Loss"));
        let timed = list_entry(SegmentKind::Overall, Some(true));
        assert!(has(list_row(0, &timed, false), "Timed"));
        assert!(has(list_row(0, &timed, false), "Σ Somewhere"));
        let over = list_entry(SegmentKind::Overall, Some(false));
        assert!(has(list_row(0, &over, false), "Over"));
        let open = list_entry(SegmentKind::Overall, None);
        assert!(has(list_row(0, &open, false), "1:23"));
        let trash = list_entry(SegmentKind::Trash, None);
        assert!(has(list_row(0, &trash, false), "Somewhere"));
        let _ = render(list_row(0, &trash, true));
        // A fight without a verdict yet, but no longer live (a cut log).
        let undecided = list_entry(SegmentKind::Encounter, None);
        let mut ui = simulator(list_row(0, &undecided, false));
        assert!(ui.find("Somewhere").is_ok());
        assert!(ui.find("Live").is_err());
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
        let mut ui = tk::wide(meter_screen(&gui));
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
    /// own fourth — with the overkill column gone, and totals itself.
    #[test]
    fn every_view_renders_with_its_own_captions() {
        for view in [
            View::Damage,
            View::Healing,
            View::Interrupts,
            View::CrowdControl,
            View::Dispels,
            View::Deaths,
            View::Taken,
        ] {
            let (mut state, mut mock) = tk::kill();
            apply(&mut state, &mut mock, Action::SetView(view));
            let rows = state.rows();
            let (gui, _peer) = tk::gui_over(state);
            let mut ui = tk::wide(meter_screen(&gui));
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
        // The panel is the window's, not the meter's: it is up over the
        // fight list too.
        gui.state.screen = Screen::List;
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
        let width = |role, you| name_tags(role, you).map(|(_, w)| w);
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
    }

    /// The fight's workspace runs edge to edge: the total the window's
    /// width, "Player" over the icon column as the prototype's head is
    /// (`.who2`), and a filter's total the drawn rows' — their count and
    /// sum, no share of the chart it is not.
    #[test]
    fn the_meter_runs_edge_to_edge_and_totals_what_it_draws() {
        let size = iced::Size::new(1440.0, 900.0);
        let (mut gui, _peer) = tk::gui_over(tk::raid(25));
        gui.cfg.hide_realms = true;
        let mut ui = tk::simulator_as(crate::window::settings(), size, view(&gui));
        let total = ui.find(meter_total_id()).unwrap().bounds();
        assert!(
            total.x.abs() < 0.5 && (total.width - size.width).abs() < 0.5,
            "{total:?}"
        );
        let head = ui.find("Player").unwrap().bounds();
        let icon_x = ROW_LEAD + RANK_W + 6.0 + ICON_X;
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
        let mut ui = simulator(meter_screen(&gui));
        assert!(ui.find("Live").is_ok());
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
    fn footer_carries_only_the_daemon_status() {
        let mut state = ClientState::new();
        // No status, no footer — not an empty line, not the gap above one:
        // the keymap lives behind `?` now, and the rows take the height.
        assert!(footer(&state).is_none());
        state.status = Some("  ".to_string());
        assert!(footer(&state).is_none(), "an empty status says nothing");
        state.status = Some("segment gone: the log rotated".to_string());
        assert!(has(
            footer(&state).expect("a status to show"),
            "segment gone: the log rotated"
        ));
        // The meter shows it under the table when there is one.
        let (mut state, _) = tk::kill();
        state.status = Some("daemon gone — reconnecting…".to_string());
        let (gui, _peer) = tk::gui_over(state);
        let mut ui = tk::wide(meter_screen(&gui));
        assert!(ui.find("daemon gone — reconnecting…").is_ok());
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
        // The probe is a BUCKET now — one instant, marked on every graph
        // sharing it — so the readout is that bucket's own value.
        let probed = state
            .drill_timeline()
            .map(|t| human(t.rolling_dps(15_000)[3] as u64))
            .unwrap();
        let (mut gui, _peer) = tk::gui_over(state);
        gui.graph_probe = Some(3);
        let mut ui = tk::wide(meter_screen(&gui));
        assert!(ui.find(label.as_str()).is_ok());
        assert!(ui.find("— Damage").is_ok());
        assert!(ui.find("By spell").is_ok());
        assert!(ui.find("By target").is_ok());
        // The throughput table: its headings and its pinned total.
        for head in ["Hits", "Avg", "Crit"] {
            assert!(ui.find(head).is_ok(), "{head} heading");
        }
        let total = format!("Total · {}", by_spell.len());
        assert!(
            ui.find(total.as_str()).is_ok(),
            "the spell pane's total row"
        );
        assert!(ui.find(by_spell[0].label.as_str()).is_ok());
        assert!(ui.find(by_target[0].label.as_str()).is_ok());
        let want = format!("dps: {probed}");
        assert!(ui.find(want.as_str()).is_ok(), "the probe readout: {want}");
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
            assert!(
                ui.find(format!("— {}", window_view_name(view)).as_str())
                    .is_ok()
            );
            assert!(ui.find("Count").is_ok());
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
        // The rate word follows the VIEW, so a Healing drill reads "hps" —
        // the value is that bucket's, the probe being an instant.
        let probed = state
            .drill_timeline()
            .filter(|t| !t.buckets.is_empty())
            .map(|t| human(t.rolling_dps(15_000)[2] as u64));
        let (mut gui, _peer) = tk::gui_over(state);
        gui.graph_probe = Some(2);
        let mut ui = simulator(meter_screen(&gui));
        assert!(ui.find("— Healing").is_ok());
        if let Some(v) = probed {
            let want = format!("hps: {v}");
            assert!(ui.find(want.as_str()).is_ok(), "{want}");
        }
        let _ = ui.snapshot(&Theme::TokyoNight).unwrap();
    }
    /// R21: the stack matrix is a section behind a chip, never stacked
    /// under the panes — and the chip only exists when a ledger does.
    #[test]
    fn the_stacks_section_replaces_the_panes_only_when_asked() {
        let (mut state, mut mock) = tk::taken_kill();
        apply(&mut state, &mut mock, Action::Open);
        let has_ledger = state.drill_stacks().is_some_and(|(s, _, _)| !s.is_empty());
        let (mut gui, _peer) = tk::gui_over(state);
        let mut ui = simulator(meter_screen(&gui));
        assert!(ui.find("By ability").is_ok());
        assert_eq!(
            ui.find("stacks").is_ok(),
            has_ledger,
            "the chip follows the ledger"
        );
        gui.stacks_open = true;
        let mut ui = simulator(meter_screen(&gui));
        if has_ledger {
            assert!(
                ui.find("By ability").is_err(),
                "the matrix took the panes\x27 place"
            );
        } else {
            assert!(
                ui.find("By ability").is_ok(),
                "nothing to show, so the panes stay"
            );
        }
        let _ = ui.snapshot(&Theme::TokyoNight).unwrap();
    }

    /// R17: the Taken view over `taken.txt` — the tank's row reads its
    /// absorbed part and dtps, and the drill words its panes as what hit
    /// him and who swung, with the mitigation record in one line under
    /// them.

    #[test]
    fn the_taken_drill_words_its_panes_and_the_mitigation_cards() {
        let (state, _mock) = tk::taken_kill();
        let top = state.rows().first().cloned().unwrap();
        assert_eq!(top.amount, 84_000, "Durgan's taken (fixture golden)");
        assert_eq!(top.extra, 12_000, "his partial absorbs");
        let (gui, _peer) = tk::gui_over(state);
        let mut ui = tk::wide(meter_screen(&gui));
        assert!(ui.find("— Taken").is_err(), "not drilled yet");
        assert!(ui.find("Absorbed").is_ok());
        assert!(ui.find("(Absorbed)").is_err(), "no parenthesised extra");
        assert!(ui.find("Per sec").is_ok());
        assert!(ui.find("12.0k").is_ok(), "the fourth column is absorbed");
        assert!(ui.find("1.4k").is_ok(), "84 000 over 60 s");
        let _ = ui.snapshot(&Theme::TokyoNight).unwrap();

        let (mut state, mut mock) = tk::taken_kill();
        apply(&mut state, &mut mock, Action::Open);
        assert!(state.drill.is_some());
        let m = *state
            .drill_mitigation()
            .expect("the daemon attaches the record to a Taken drill");
        assert_eq!((m.absorbed, m.blocked), (12_000, 18_000));
        assert_eq!(m.absorbed_full + m.blocked_full, 55_000);
        assert_eq!(m.misses(), 5);
        let (gui, _peer) = tk::gui_over(state);
        let mut ui = simulator(meter_screen(&gui));
        assert!(ui.find("— Taken").is_ok());
        assert!(ui.find("By ability").is_ok());
        assert!(ui.find("By attacker").is_ok());
        assert!(ui.find("Taken").is_ok(), "pane caption");
        assert!(ui.find("Cinder Lash").is_ok(), "an ability row");
        assert!(ui.find("Taken Test Boss").is_ok(), "an attacker row");
        // The record as cards and chips, where one sentence used to be.
        assert!(ui.find("Mitigated").is_ok(), "the mitigated card");
        assert!(ui.find("61%").is_ok(), "its value");
        assert!(ui.find("Absorbed").is_ok());
        assert!(
            ui.find("blocked 18.0k").is_ok(),
            "blocked rides under absorbed"
        );
        assert!(ui.find("Prevented").is_ok());
        assert!(ui.find("55,000").is_ok());
        assert!(ui.find("5 misses").is_ok(), "the chips");
        assert!(ui.find("dodge").is_ok());
        assert!(ui.find("parry").is_ok());
        assert!(ui.find("block").is_ok());
        assert!(ui.find("miss").is_ok());
        let _ = ui.snapshot(&Theme::TokyoNight).unwrap();
    }

    #[test]
    fn the_rate_label_follows_the_view() {
        assert_eq!(rate_label(View::Taken), "dtps");
        assert_eq!(rate_label(View::Healing), "hps");
        assert_eq!(rate_label(View::Damage), "dps");
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
        assert!(ui.find("Death recap").is_ok());
        assert!(ui.find("By attacker").is_ok());
        assert!(ui.find("Amount · hp").is_ok());
        assert!(ui.find("— Deaths").is_ok());
        if let Some(a) = attackers.first() {
            assert!(ui.find(a.label.as_str()).is_ok());
        }
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
        let mut ui = tk::wide(meter_screen(&gui));
        for r in &rows {
            let shown = meter_label(&r.label, true);
            assert!(
                ui.find(shown.as_str()).is_ok(),
                "{} drawn as {shown}",
                r.label
            );
        }
    }

    /// `hide_realms` reaches every pane of a drill, not just the meter: a
    /// healer's targets are players, and each one is drawn by name alone —
    /// the drill's panes, the ability drill's target list and the title.
    #[test]
    fn realms_are_hidden_in_the_drill_panes() {
        let (mut state, mut mock) = tk::kill();
        apply(&mut state, &mut mock, Action::SetView(View::Healing));
        apply(&mut state, &mut mock, Action::Open);
        let (_, targets) = state.breakdown();
        let realmed: Vec<String> = targets
            .iter()
            .map(|r| r.label.clone())
            .filter(|l| realmless(l) != *l)
            .collect();
        assert!(
            !realmed.is_empty(),
            "the healer healed players: {targets:?}"
        );
        let drilled = state.drill.clone().unwrap().label;

        let (mut gui, _peer) = tk::gui_over(state);
        gui.cfg.hide_realms = false;
        let mut ui = simulator(meter_screen(&gui));
        for l in &realmed {
            assert!(ui.find(l.as_str()).is_ok(), "the option off: {l} as logged");
        }
        gui.cfg.hide_realms = true;
        let mut ui = simulator(meter_screen(&gui));
        for l in &realmed {
            assert!(ui.find(l.as_str()).is_err(), "{l} still wears its realm");
            assert!(ui.find(realmless(l).as_str()).is_ok(), "{l} by name");
        }
        assert!(ui.find(drilled.as_str()).is_err(), "the title too");
        assert!(ui.find(display_name(&drilled)).is_ok());

        // One level deeper: who the top heal landed on.
        let mut state = std::mem::replace(&mut gui.state, ClientState::new());
        apply(&mut state, &mut mock, Action::Open);
        let landed: Vec<String> = state
            .spell_target_rows()
            .iter()
            .map(|r| r.label.clone())
            .filter(|l| realmless(l) != *l)
            .collect();
        assert!(!landed.is_empty(), "the heal landed on players");
        let (mut gui, _peer) = tk::gui_over(state);
        gui.cfg.hide_realms = true;
        let mut ui = simulator(meter_screen(&gui));
        for l in &landed {
            assert!(ui.find(l.as_str()).is_err(), "{l} still wears its realm");
            assert!(ui.find(realmless(l).as_str()).is_ok());
        }
    }

    /// The window's semantic colours: a kill is the good green and a wipe
    /// the bad red, LIVE a red dot and its word — and none of them yellow.
    #[test]
    fn the_window_words_outcomes_in_green_and_red() {
        let (live, _) = tk::live();
        let badge = crate::fight_head::outcome(&live).expect("a live pull says so");
        assert_eq!(badge.word, "Live");
        assert!(badge.live, "with its dot");
        assert_eq!(badge.color, theme::BAD);
        let (kill, _) = tk::kill();
        assert_eq!(
            crate::fight_head::outcome(&kill).map(|b| (b.word, b.color)),
            Some(("Kill".to_string(), theme::GOOD))
        );
        let (wipe, _) = tk::wipe();
        assert_eq!(
            crate::fight_head::outcome(&wipe).map(|b| (b.word, b.color)),
            Some(("Wipe".to_string(), theme::BAD))
        );
        assert_eq!(crate::fight_head::outcome(&ClientState::new()), None);
        // The list's LIVE row wears the dot too, and its word is findable.
        let mut entry = list_entry(SegmentKind::Encounter, None);
        entry.live = true;
        assert!(has(list_row(0, &entry, false), "Live"));
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
    fn the_ability_drill_shows_breadcrumb_stats_targets_and_focus_curve() {
        let (state, _) = tk::spell_drilled();
        let (_, spell_label) = state.drill_spell().cloned().unwrap();
        let spell_row = state.drill_spell_row().expect("the row behind the ability");
        let targets = state.spell_target_rows();
        assert!(!targets.is_empty());
        assert!(state.spell_timeline().is_some());
        let (mut gui, _peer) = tk::gui_over(state);
        gui.graph_probe = Some(1);
        let mut ui = simulator(meter_screen(&gui));
        assert!(ui.find(spell_label.as_str()).is_ok());
        assert!(ui.find("Targets").is_ok());
        assert!(ui.find("Hits · total · share").is_ok());
        for card in ["Total", "Share", "Hits", "Crit", "Avg"] {
            assert!(ui.find(card).is_ok(), "{card} card");
        }
        assert!(ui.find(human(spell_row.amount).as_str()).is_ok());
        assert!(ui.find(targets[0].label.as_str()).is_ok());
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
        // Drilled into one ability: the FOCUS curve is what the readout
        // reads, and it is still worded with the view's own rate.
        let probed = state
            .spell_timeline()
            .or_else(|| state.drill_timeline())
            .filter(|t| !t.buckets.is_empty())
            .map(|t| human(t.rolling_dps(15_000)[1] as u64));
        let (mut gui, _peer) = tk::gui_over(state);
        gui.graph_probe = Some(1);
        let mut ui = simulator(meter_screen(&gui));
        assert!(ui.find("Targets").is_ok());
        if let Some(v) = probed {
            let want = format!("hps: {v}");
            assert!(ui.find(want.as_str()).is_ok(), "healing rate word: {want}");
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
        let mut ui = simulator(drill_body(&gui));
        assert!(ui.find("no data yet").is_ok(), "no stats without the row");
        assert!(ui.find("Bolt").is_ok());
        assert!(ui.find("Ghost").is_ok());
        let mut plain = ClientState::new();
        plain.screen = Screen::Meter;
        let (gui, _peer) = tk::gui_over(plain);
        assert!(has(drill_body(&gui), "nothing to show for this view yet"));
    }

    // ---- the comparison ----------------------------------------------------------

    #[test]
    fn the_comparison_screen_names_both_players() {
        let (state, _) = tk::compared();
        let (a, b) = state.compare_sides().unwrap();
        let (a_name, b_name) = (a.total.label.clone(), b.total.label.clone());
        // One instant, both curves: the readout names each side, which is
        // the whole reason the time cursor is shared.
        let at = |t: &wowdps_model::Timeline| human(t.rolling_dps(15_000)[2] as u64);
        let (a_at, b_at) = (at(&a.timeline), at(&b.timeline));
        let (mut gui, _peer) = tk::gui_over(state);
        gui.graph_probe = Some(2);
        gui.compare_hover = Some("nothing hovered by that name".to_string());
        let mut ui = simulator(view(&gui));
        let short = |s: &str| s.split('-').next().unwrap().to_string();
        assert!(ui.find(short(&a_name).as_str()).is_ok());
        assert!(ui.find(short(&b_name).as_str()).is_ok());
        assert!(ui.find("The Ashen Warden").is_ok());
        // One reading per graph, each drawn under its own half of the row.
        for want in [
            format!("{} {a_at}", short(&a_name)),
            format!("{} {b_at}", short(&b_name)),
        ] {
            assert!(ui.find(want.as_str()).is_ok(), "{want}");
        }
        assert!(ui.find("0:02 · dps").is_ok(), "the instant, said once");
        let _ = ui.snapshot(&Theme::TokyoNight).unwrap();
    }

    /// A recap line at a narrow window's pane width: the label clips, and
    /// the amount and the health keep their own cells — side by side, inside
    /// the line, never printed over each other — with the overkill the one
    /// that gives way (it is drawn in the recap's red where it fits).
    #[test]
    fn a_narrow_recap_line_keeps_its_amount_and_health_apart() {
        let r = Row {
            label: "Coalesced Venom (Zul'jan the Unending)".to_string(),
            amount: 8_900,
            extra: 1_600,
            count: 1,
            hp: Some((0, 612_000)),
            ..row("x", 0, None)
        };
        let width = 250.0;
        let size = iced::Size::new(width, pitch::DRILL_ROW);
        let line = |fit| -> Element<'static, ()> {
            container(recap_row_at::<()>(
                &Look::WINDOW,
                &r,
                9_000,
                pitch::DRILL_ROW,
                WS,
                fit,
            ))
            .width(width)
            .into()
        };
        for fit in [RecapFit::Narrow, RecapFit::Wide] {
            let mut ui = tk::simulator_as(crate::window::settings(), size, line(fit));
            let label = ui.find(r.label.as_str()).unwrap().bounds();
            let amount = ui.find("8.9k").unwrap().bounds();
            let hp = ui.find("0%").unwrap().bounds();
            assert!(
                label.x + label.width <= amount.x + 0.5,
                "{fit:?}: the label runs into the amount: {label:?} {amount:?}"
            );
            assert!(
                amount.x + amount.width <= hp.x + 0.5,
                "{fit:?}: the amount is on the health: {amount:?} {hp:?}"
            );
            assert!(hp.x + hp.width <= width + 0.5, "{fit:?}: off the line");
            assert_eq!(
                ui.find("(1.6k over)").is_ok(),
                fit == RecapFit::Wide,
                "{fit:?}: the overkill"
            );
            if fit == RecapFit::Narrow {
                assert!(label.width > 90.0, "the label keeps room: {label:?}");
            }
        }
        // The overkill is the recap's red, not a dim aside.
        let px = tk::pixels(line(RecapFit::Wide), size, &theme::window_theme());
        assert!(px.count(theme::BAD, 2) > 0);
        // And the drill picks the fit by its pane's width.
        const { assert!(RECAP_OVER_MIN > 250.0 && RECAP_OVER_MIN < 440.0) };
    }

    /// The Deaths drill at the default 460 px window: every recap line has
    /// its amount and its health inside the pane, and the killing blow drops
    /// its overkill before either; at a wide window the overkill is back.
    #[test]
    fn the_narrow_deaths_drill_drops_the_overkill_first() {
        let (mut state, mut mock) = tk::kill();
        apply(&mut state, &mut mock, Action::SetView(View::Deaths));
        apply(&mut state, &mut mock, Action::Open);
        let (recap, _) = state.breakdown();
        let over = recap
            .iter()
            .find(|r| r.extra > 0 && !r.gain)
            .map(|r| format!("({} over)", human(r.extra)));
        let (gui, _peer) = tk::gui_over(state);
        let narrow = iced::Size::new(460.0, 860.0);
        let mut ui = tk::simulator_as(crate::window::settings(), narrow, meter_screen(&gui));
        assert!(ui.find("Death recap").is_ok());
        if let Some(over) = &over {
            assert!(ui.find(over.as_str()).is_err(), "{over} at 460 px");
            let mut wide = tk::simulator_as(
                crate::window::settings(),
                iced::Size::new(1440.0, 900.0),
                meter_screen(&gui),
            );
            assert!(wide.find(over.as_str()).is_ok(), "{over} at 1440 px");
        }
    }

    /// The comparison at the default 460 px window: each half-width table
    /// gives up crit, then the average, before it gives up the names — every
    /// ability is still named, as wide as it is in a wide window or the
    /// room a name keeps.
    #[test]
    fn a_narrow_comparison_keeps_its_ability_names() {
        let (mut state, mut mock) = tk::kill();
        tk::apply(&mut state, &mut mock, Action::PickCompare);
        tk::apply(&mut state, &mut mock, Action::Down);
        tk::apply(&mut state, &mut mock, Action::PickCompare);
        assert_eq!(state.screen, Screen::Compare);
        let (a, b) = state.compare_sides().unwrap();
        let labels: Vec<String> = a
            .spells
            .iter()
            .chain(&b.spells)
            .map(|r| r.label.clone())
            .collect();
        assert!(!labels.is_empty());
        let (gui, _peer) = tk::gui_over(state);
        let settings = crate::window::settings;
        let mut wide = tk::simulator_as(settings(), iced::Size::new(1440.0, 900.0), view(&gui));
        let mut narrow = tk::simulator_as(settings(), iced::Size::new(460.0, 860.0), view(&gui));
        for l in &labels {
            let whole = wide.find(l.as_str()).unwrap().bounds().width;
            let kept = narrow.find(l.as_str()).unwrap().bounds().width;
            assert!(
                kept >= whole.min(56.0),
                "{l}: {kept:.0} px of {whole:.0} at 460 px"
            );
        }
        // Wide, the table has every column; narrow, the names outrank crit.
        assert!(wide.find("Crit").is_ok());
        assert!(narrow.find("Crit").is_err());
        assert!(narrow.find("Hits").is_ok());
    }

    /// v29: a comparison opened from Taken is about what hit them — the
    /// table lists the abilities that landed, the header's rate is dtps, and
    /// each side's mitigation record sits under its table.
    #[test]
    fn a_taken_comparison_words_itself_as_damage_taken() {
        let (mut state, mut mock) = tk::taken_kill();
        tk::apply(&mut state, &mut mock, Action::PickCompare);
        tk::apply(&mut state, &mut mock, Action::Down);
        tk::apply(&mut state, &mut mock, Action::PickCompare);
        assert_eq!(state.screen, Screen::Compare);
        assert_eq!(state.compare_view(), View::Taken, "the snapshot's own view");
        let (a, _) = state.compare_sides().unwrap();
        let hit_by = a.spells[0].label.clone();
        let record = mitigation_line(
            a.mitigation.as_ref().expect("a Taken side carries one"),
            a.total.amount,
        );
        let (gui, _peer) = tk::gui_over(state);
        let mut ui = simulator(view(&gui));
        assert!(ui.find("Hit by").is_ok(), "not 'Spell'");
        assert!(ui.find(hit_by.as_str()).is_ok(), "an ability that LANDED");
        assert!(ui.find(record.as_str()).is_ok(), "R17's record per side");
        let _ = ui.snapshot(&Theme::TokyoNight).unwrap();

        // And switching back on the comparison re-asks for damage: the view
        // keys work here, and the wording follows the answer.
        let (mut state, mut mock) = tk::taken_kill();
        tk::apply(&mut state, &mut mock, Action::PickCompare);
        tk::apply(&mut state, &mut mock, Action::Down);
        tk::apply(&mut state, &mut mock, Action::PickCompare);
        tk::apply(&mut state, &mut mock, Action::SetView(View::Damage));
        assert_eq!(state.screen, Screen::Compare, "the pair survives");
        assert_eq!(state.compare_view(), View::Damage);
        let (a, _) = state.compare_sides().unwrap();
        assert!(a.mitigation.is_none());
        let (gui, _peer) = tk::gui_over(state);
        let mut ui = simulator(view(&gui));
        assert!(ui.find("Spell").is_ok());
        assert!(ui.find("Hit by").is_err());
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
            let mut ui = tk::wide(meter_screen(gui));
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
        let (state, _mock) = tk::drilled();
        let (gui, _peer) = tk::gui_over(state);
        let mut ui = tk::wide(meter_screen(&gui));
        assert!(ui.find(nav::filter_id()).is_err(), "no filter over a drill");
    }

    #[test]
    fn the_filter_narrows_rows_without_renumbering() {
        let (state, _) = tk::kill();
        let rows = state.rows();
        assert!(rows.len() > 1);
        let target = rows[1].label.clone();
        let (mut gui, _peer) = tk::gui_over(state);
        gui.filter = target.to_lowercase();
        let mut ui = simulator(meter_screen(&gui));
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
        let mut ui = simulator(bar_row::<()>(
            &r,
            185_370,
            true,
            24.0,
            Some(table::METER),
            1.0,
            Some(3),
            None,
        ));
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
        let mut ui = simulator(bar_row::<()>(
            &quiet, 185_370, false, 20.0, None, 1.0, None, None,
        ));
        assert!(ui.find("40").is_ok());
        assert!(ui.find("0").is_err(), "no rate cell in compact rows");
        let _ = ui.snapshot(&Theme::TokyoNight).unwrap();
        let _ = render(bar_row::<()>(
            &quiet,
            185_370,
            false,
            20.0,
            Some(table::METER),
            1.5,
            None,
            None,
        ));
        // Zero and full bars take their own branches.
        let _ = render(bar_row::<()>(
            &row("z", 0, None),
            10,
            false,
            20.0,
            Some(table::METER),
            1.0,
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
        let _ = render(bar_row::<()>(
            &r,
            10,
            false,
            20.0,
            Some(table::METER),
            1.0,
            Some(1),
            None,
        ));
    }
}
