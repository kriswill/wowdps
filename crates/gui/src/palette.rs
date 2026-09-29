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

use wowdps_model::{Action, Class, Row, Spec, View};

use crate::ellipsis::ellipsis;
use crate::fold;
use crate::line_icons::{LineIcon, line_icon};
use crate::nav;
use crate::rail::{KeyWord, Mark, Pull, Rail};
use crate::theme::{self, size};
use crate::window::Message;

/// Pulls offered before anything is typed: the rail's newest, a live one
/// among them — the pulls a reader most often jumps back to.
pub(crate) const RECENT: usize = 4;
/// Pulls a query lists at most: the rail can hold hundreds, and a few more
/// letters narrow them.
pub(crate) const MAX_PULLS: usize = 20;

/// The palette's state: what is typed, and the selection the keys move —
/// the one line lit, and what Enter runs. The pointer lights nothing (the
/// prototype's `.pal-it` has no hover): a second wash under a resting
/// pointer would leave the reader asking which of two lines Enter meant.
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct Palette {
    pub query: String,
    /// Index into [`listed`]'s answer for the query.
    pub sel: usize,
}

impl Palette {
    /// A new query: the selection goes back to the top of what it lists.
    pub(crate) fn typed(&mut self, query: String) {
        self.query = query;
        self.sel = 0;
    }

    /// One step down (`down`) or up a list `len` long, stopping at its ends.
    pub(crate) fn step(&mut self, down: bool, len: usize) {
        self.clamp(len);
        self.sel = if down {
            (self.sel + 1).min(len.saturating_sub(1))
        } else {
            self.sel.saturating_sub(1)
        };
    }

    /// The list changed under the card (a live pull arrived, the players
    /// refilled after a view's answer, a page of the rail landed): the
    /// selection stays on a line it lists — the last, when it shrank past
    /// it — so Enter always runs something drawn (the prototype's `S.pi =
    /// min(S.pi, len - 1)` on every render).
    pub(crate) fn clamp(&mut self, len: usize) {
        self.sel = self.sel.min(len.saturating_sub(1));
    }
}

/// The groups, in the order the card lists them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Group {
    Pulls,
    Players,
    Views,
    Screens,
}

impl Group {
    pub(crate) fn name(self) -> &'static str {
        match self {
            Group::Pulls => "Pulls",
            Group::Players => "Players in this pull",
            Group::Views => "Views",
            Group::Screens => "Screens",
        }
    }
}

/// What running an item does.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Run {
    /// Put the pull on the stage.
    Pull(Pull),
    /// Select the player on the pull's meter (by key, as a drill names
    /// them), on a view they have a row on.
    Player {
        key: String,
        label: String,
    },
    View(View),
    /// Home (`~`), the live pull (`m`), the earlier nights on the rail
    /// (`H`), the `?` sheet, the talent viewer (`t`).
    Home,
    Live,
    Earlier,
    Sheet,
    Talents,
    /// Home, scoped to one character's guid (`None`: all of them) — what
    /// its chips do, for a reader on the keys.
    HomeScope(Option<String>),
}

/// One line of the card.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Item {
    pub group: Group,
    pub title: String,
    /// The quieter words at its right: a pull's night and verdict, a
    /// player's spec. Searched with the title.
    pub sub: String,
    /// The key that does the same, on a keycap at its right.
    pub key: Option<&'static str>,
    /// A player's disc: their spec icon, else their class's.
    pub disc: Option<(Option<Class>, Option<Spec>)>,
    /// A pull the empty query offers ([`RECENT`]).
    pub recent: bool,
    pub run: Run,
}

/// Every item the palette could list: the rail's pulls, the players of the
/// pull on the stage, the views and the screens — Home's scopes among them,
/// all characters and each of `chars` (the characters the window knows you
/// play). `players` are that pull's rows — our side's, whatever view they
/// were read on.
pub(crate) fn items(
    rail: &Rail,
    players: &[Row],
    chars: &[crate::home::CharLine],
    hide_realms: bool,
) -> Vec<Item> {
    let shown = |name: &str| crate::rail::shown_name(name, hide_realms);
    let mut out = Vec::new();
    let mut recent = 0;
    for night in &rail.nights {
        for visit in &night.visits {
            for line in &visit.lines {
                // What the rail itself says at the row's right, as words.
                let verdict = match (line.mark, line.key, line.best_pct) {
                    (Mark::Live, ..) => Some("live".to_string()),
                    (Mark::Sum, ..) => Some("whole visit".to_string()),
                    (Mark::Good, Some(KeyWord::Plus(n)), _) => Some(format!("+{n}")),
                    (Mark::Bad, Some(KeyWord::Over), _) => Some("over".to_string()),
                    (Mark::Good, ..) if visit.title == "Arena" => Some("win".to_string()),
                    (Mark::Bad, ..) if visit.title == "Arena" => Some("loss".to_string()),
                    (Mark::Good, ..) => Some("kill".to_string()),
                    (Mark::Bad, _, Some(pct)) => Some(format!("wipe at {pct}%")),
                    (Mark::Bad, ..) => Some("wipe".to_string()),
                    (Mark::Dash, ..) => None,
                };
                let sub = match verdict {
                    Some(v) => format!("{}, {v}", night.label),
                    None => night.label.clone(),
                };
                // The newest few worth jumping back to: a live pull, a
                // boss, a key — not the trash between them, nor a visit's Σ.
                let offered = line.mark == Mark::Live || (!line.trash && line.mark != Mark::Sum);
                let is_recent = offered && recent < RECENT;
                recent += usize::from(is_recent);
                out.push(Item {
                    group: Group::Pulls,
                    // A visit's Σ goes by its visit: "The Venomous Abyss,
                    // Heroic", whole.
                    title: if line.mark == Mark::Sum {
                        visit.title.clone()
                    } else {
                        line.name.clone()
                    },
                    sub,
                    key: None,
                    disc: None,
                    recent: is_recent,
                    run: Run::Pull(line.pull.clone()),
                });
            }
        }
    }
    for r in players.iter().filter(|r| !r.enemy) {
        let sub = match (r.spec, r.class) {
            (Some(s), Some(c)) => format!("{} {}", s.name(), c.name()),
            (None, Some(c)) => c.name().to_string(),
            _ => String::new(),
        };
        out.push(Item {
            group: Group::Players,
            title: shown(&r.label),
            sub,
            key: None,
            disc: Some((r.class, r.spec)),
            recent: false,
            run: Run::Player {
                key: r.key.clone(),
                label: r.label.clone(),
            },
        });
    }
    for v in crate::view::WINDOW_VIEWS {
        out.push(Item {
            group: Group::Views,
            title: crate::view::window_view_name(v).to_string(),
            sub: String::new(),
            key: crate::keys::key_for(Action::SetView(v)),
            disc: None,
            recent: false,
            run: Run::View(v),
        });
    }
    for (title, key, run) in [
        ("Home", "~", Run::Home),
        ("Live pull", "m", Run::Live),
        ("Earlier nights", "H", Run::Earlier),
        ("Keyboard shortcuts", "?", Run::Sheet),
        ("Talents", "t", Run::Talents),
    ] {
        out.push(Item {
            group: Group::Screens,
            title: title.to_string(),
            sub: String::new(),
            key: Some(key),
            disc: None,
            recent: false,
            run,
        });
    }
    // Home's scope chips, for the keys: all characters, then each one.
    out.push(Item {
        group: Group::Screens,
        title: format!("Home: {}", crate::home::ALL_CHARACTERS),
        sub: String::new(),
        key: None,
        disc: None,
        recent: false,
        run: Run::HomeScope(None),
    });
    for c in chars.iter().filter(|c| !c.guid.is_empty()) {
        out.push(Item {
            group: Group::Screens,
            title: format!("Home: {}", shown(&c.name)),
            sub: String::new(),
            key: None,
            disc: Some((c.class, c.spec)),
            recent: false,
            run: Run::HomeScope(Some(c.guid.clone())),
        });
    }
    out
}

/// What the card lists for `query`: before anything is typed, the recent
/// pulls and every other item but Home's scopes (the prototype's card,
/// which has none — they wait for a word, a name or "home"); then every
/// item whose title and words together contain it, accent-folded and case
/// aside — at most [`MAX_PULLS`] pulls.
pub(crate) fn listed(items: Vec<Item>, query: &str) -> Vec<Item> {
    let needle = fold::fold(query.trim());
    let mut pulls = 0;
    items
        .into_iter()
        .filter(|i| {
            if i.group == Group::Pulls {
                let keep = if needle.is_empty() {
                    i.recent
                } else {
                    pulls < MAX_PULLS && matches(i, &needle)
                };
                pulls += usize::from(keep);
                return keep;
            }
            if needle.is_empty() {
                return !matches!(i.run, Run::HomeScope(_));
            }
            matches(i, &needle)
        })
        .collect()
}

/// Does `item` answer to the folded `needle`? Its title and its words are
/// one text, a space between (the prototype's `fold(t + ' ' + s)`), so a
/// query may run from one into the other: "akanos devourer".
fn matches(item: &Item, needle: &[char]) -> bool {
    if item.sub.is_empty() {
        fold::contains(&item.title, needle)
    } else {
        fold::contains(&format!("{} {}", item.title, item.sub), needle)
    }
}

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
