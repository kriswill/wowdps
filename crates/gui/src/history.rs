//! Layout F of the design study: the History screen — a browser over the
//! store's cards, scoped to one boss, one dungeon or everything, with the
//! owner's own number beside each pull; and the stored fight it opens,
//! rendered with the same table the live meter uses (`GetFight` answers in
//! the live snapshot's shape, so nothing about the meter is learned twice).
//! Window-local like Home: `ClientState` never learns it exists.

use iced::widget::{Space, column, container, mouse_area, row, scrollable, text};
use iced::{Color, Element, Font, Length, Theme};

use wowdps_model::fmt::{commas, duration, human, view_name};
use wowdps_model::{Row, View};
use wowdps_proto::history::{FightCard, FightKind};
use wowdps_proto::{ClientMsg, FightSort, HistoryAnswer, HistoryQuery, StoredFight};

use crate::home::{self, DASH};
use crate::nav;
use crate::table;
use crate::theme::{self, DIM, Density, GREEN, YELLOW, size};
use crate::window::Message;

/// What the list is about. `matches` is the client-side twin of the query
/// filter, so a card that arrived under one scope never shows under another.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) enum Scope {
    #[default]
    All,
    Encounter {
        id: u32,
        difficulty: Option<u32>,
        name: String,
    },
    Key {
        map_id: u32,
        name: String,
    },
}

impl Scope {
    pub(crate) fn title(&self) -> String {
        match self {
            Scope::All => "every fight".to_string(),
            Scope::Encounter { name, .. } | Scope::Key { name, .. } => name.clone(),
        }
    }

    pub(crate) fn matches(&self, c: &FightCard) -> bool {
        match self {
            Scope::All => true,
            Scope::Encounter { id, difficulty, .. } => {
                c.kind == FightKind::Encounter
                    && c.encounter.is_some_and(|e| {
                        e.id == *id && difficulty.is_none_or(|d| e.difficulty == d)
                    })
            }
            Scope::Key { map_id, .. } => {
                c.kind == FightKind::Key && c.key.as_ref().is_some_and(|k| k.map_id == *map_id)
            }
        }
    }
}

/// The opened stored fight: what was asked for and what came back.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Stored {
    pub fight_id: String,
    pub view: View,
    /// The drilled player's guid, when drilled.
    pub drill: Option<String>,
    /// The in-flight `GetFight` req_id.
    pub pending: Option<u32>,
    /// The daemon's answer; `Some(None)` is "unknown or evicted".
    pub fight: Option<Option<StoredFight>>,
    /// The selected row of the meter (or of the by-spell pane, drilled).
    pub sel: usize,
}

/// Window-local History state.
#[derive(Debug, Default)]
pub(crate) struct History {
    pub scope: Scope,
    /// Cards in scope, newest first, deduped by id.
    pub cards: Vec<FightCard>,
    /// The one `GetHistory` in flight — the same one-in-flight rule Home
    /// keeps, for the same reason: reads must never evict a `Store`.
    pub pending: Option<u32>,
    pub total: Option<u32>,
    pub pages: u32,
    pub cursor: Option<String>,
    pub answered: bool,
    pub sel: usize,
    pub stored: Option<Stored>,
    /// The character the list is scoped to (a guid); `None` = everyone,
    /// with the owner's number where the owner was on the pull.
    pub character: Option<String>,
    /// Every character the unscoped list has seen you play — remembered
    /// across a character scope, since a scoped answer names only one.
    pub characters: Vec<home::CharLine>,
    /// `history_characters` from the config: names that are "me" even on
    /// cards whose owner the daemon never resolved.
    pub configured: Vec<String>,
}

impl History {
    pub(crate) fn new(scope: Scope) -> Self {
        Self {
            scope,
            ..Self::default()
        }
    }

    pub(crate) fn complete(&self) -> bool {
        self.total.is_some_and(|t| self.cards.len() as u32 >= t)
    }

    /// The next page to ask for, or `None` while one is out, the burst is
    /// spent, or the list is complete.
    pub(crate) fn next_request(&mut self, req_id: u32) -> Option<ClientMsg> {
        if self.pending.is_some() || self.complete() || self.pages >= home::MAX_PAGES {
            return None;
        }
        self.pending = Some(req_id);
        self.pages += 1;
        let (encounter, difficulty, kind) = match &self.scope {
            Scope::All => (None, None, None),
            Scope::Encounter { id, difficulty, .. } => {
                (Some(*id), *difficulty, Some(FightKind::Encounter))
            }
            Scope::Key { .. } => (None, None, Some(FightKind::Key)),
        };
        Some(ClientMsg::GetHistory {
            req_id,
            query: HistoryQuery::Fights {
                encounter,
                difficulty,
                guid: self.character.clone(),
                since_utc_ms: None,
                kind,
                sort: FightSort::Newest,
                limit: home::PAGE,
                after_id: self.cursor.clone(),
                role: None,
            },
        })
    }

    pub(crate) fn scrolled_to_end(&mut self) {
        self.pages = 0;
    }

    /// Fold an answer in: a page of cards, or a pin flip.
    pub(crate) fn absorb(&mut self, req_id: u32, answer: &HistoryAnswer) {
        if let HistoryAnswer::Pinned { fight_id, pinned } = answer {
            if let Some(c) = self.cards.iter_mut().find(|c| c.id == *fight_id) {
                c.pinned = *pinned;
            }
            return;
        }
        if self.pending != Some(req_id) {
            return;
        }
        self.pending = None;
        let HistoryAnswer::Fights { cards, total } = answer else {
            return;
        };
        self.answered = true;
        self.total = Some(*total);
        for c in cards {
            if self.scope.matches(c) && !self.cards.iter().any(|have| have.id == c.id) {
                self.cards.push(c.clone());
            }
        }
        self.cards
            .sort_by_key(|c| std::cmp::Reverse(c.start_utc_ms));
        if self.character.is_none() {
            let all: Vec<&FightCard> = self.cards.iter().collect();
            let mut seen = home::character_lines(&all, &[]);
            // A configured name resolves to a guid through any card that
            // lists the player — the only join between the two.
            for name in &self.configured {
                if seen.iter().any(|c| c.name.eq_ignore_ascii_case(name)) {
                    continue;
                }
                if let Some(p) = all
                    .iter()
                    .flat_map(|c| c.players.iter())
                    .find(|p| p.name.eq_ignore_ascii_case(name))
                {
                    seen.push(home::CharLine {
                        guid: p.guid.clone(),
                        name: p.name.clone(),
                        class: p.class,
                        spec: p.spec,
                        ..home::CharLine::default()
                    });
                }
            }
            for c in seen {
                if let Some(have) = self.characters.iter_mut().find(|h| h.guid == c.guid) {
                    *have = c;
                } else {
                    self.characters.push(c);
                }
            }
        }
        if let Some(last) = cards.last() {
            self.cursor = Some(last.id.clone());
        }
        self.sel = self.sel.min(self.cards.len().saturating_sub(1));
    }

    /// Scope the list to one character (or everyone) and start it over.
    pub(crate) fn set_character(&mut self, guid: Option<String>) {
        self.character = guid;
        self.reset();
        self.sel = 0;
    }

    /// The store changed: start the list over.
    pub(crate) fn reset(&mut self) {
        self.cards.clear();
        self.cursor = None;
        self.pages = 0;
        self.total = None;
        self.pending = None;
    }

    /// Open a stored fight (on the Damage view, undrilled) and ask for it.
    pub(crate) fn open(&mut self, fight_id: String, req_id: u32) -> ClientMsg {
        let mut s = Stored {
            fight_id,
            view: View::Damage,
            drill: None,
            pending: None,
            fight: None,
            sel: 0,
        };
        let msg = s.request(req_id);
        self.stored = Some(s);
        msg
    }

    /// `Some(msg)` when the reader asked for something the open fight has
    /// not been fetched for yet.
    pub(crate) fn refetch(&mut self, req_id: u32) -> Option<ClientMsg> {
        self.stored.as_mut().map(|s| s.request(req_id))
    }

    pub(crate) fn absorb_fight(&mut self, req_id: u32, fight: Option<StoredFight>) {
        if let Some(s) = self.stored.as_mut()
            && s.pending == Some(req_id)
        {
            s.pending = None;
            s.fight = Some(fight);
            let len = s.rows().len();
            s.sel = s.sel.min(len.saturating_sub(1));
        }
    }

    /// One level up: drill → stored fight → the list. `false` when the
    /// list itself is showing, so the caller closes the screen.
    pub(crate) fn back(&mut self) -> bool {
        match self.stored.as_mut() {
            Some(s) if s.drill.is_some() => {
                s.drill = None;
                s.sel = 0;
                true
            }
            Some(_) => {
                self.stored = None;
                true
            }
            None => false,
        }
    }

    /// The fight id the selection names, for pinning and opening.
    pub(crate) fn selected_id(&self) -> Option<&str> {
        self.cards.get(self.sel).map(|c| c.id.as_str())
    }
}

impl Stored {
    fn request(&mut self, req_id: u32) -> ClientMsg {
        self.pending = Some(req_id);
        ClientMsg::GetFight {
            req_id,
            fight_id: self.fight_id.clone(),
            view: self.view,
            drill: self.drill.clone(),
            death: None,
            boss: None,
        }
    }

    pub(crate) fn rows(&self) -> Vec<Row> {
        match &self.fight {
            Some(Some(f)) => f.rows.clone(),
            _ => Vec::new(),
        }
    }

    /// Switch the view: the drill follows the player, as the live meter's
    /// does. Returns whether anything changed (so the caller refetches).
    pub(crate) fn set_view(&mut self, view: View) -> bool {
        if self.view == view {
            return false;
        }
        self.view = view;
        self.sel = 0;
        true
    }

    /// Drill into the selected row. `false` when there is nothing to open.
    pub(crate) fn drill_selected(&mut self) -> bool {
        if self.drill.is_some() {
            return false;
        }
        let Some(row) = self.rows().get(self.sel).cloned() else {
            return false;
        };
        self.drill = Some(row.key);
        self.sel = 0;
        true
    }
}

/// One pull on the list, derived. `ordinal` counts the scope's pulls from
/// the oldest, so "#14" means the same thing on every visit.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Line {
    pub fight_id: String,
    pub ordinal: u32,
    pub name: String,
    pub tag: String,
    pub tag_color: Color,
    pub duration_ms: i64,
    pub pinned: bool,
    /// The owner's number on this pull, by their role's measure.
    pub measure: Option<(&'static str, f64)>,
}

/// The list, newest first, with the owner's measure where they were on
/// the pull. `owner` is the guid Home resolved; `None` leaves the measure
/// column empty rather than borrowing anyone else's number.
pub(crate) fn derive(cards: &[FightCard], owner: Option<&str>) -> Vec<Line> {
    let mut oldest_first: Vec<&FightCard> = cards.iter().collect();
    oldest_first.sort_by_key(|c| c.start_utc_ms);
    let mut lines: Vec<Line> = oldest_first
        .iter()
        .enumerate()
        .map(|(i, c)| {
            let (tag, tag_color) = home::card_tag(c);
            let measure = owner.and_then(|guid| {
                c.players
                    .iter()
                    .find(|p| p.guid == guid)
                    .map(|p| home::measure_of(p, c.duration_ms))
            });
            Line {
                fight_id: c.id.clone(),
                ordinal: i as u32 + 1,
                name: c.name.clone(),
                tag,
                tag_color,
                duration_ms: c.duration_ms,
                pinned: c.pinned,
                measure,
            }
        })
        .collect();
    lines.reverse();
    lines
}

/// The cards over the list: pulls, kills, the best kill, the owner's best
/// and median, pins. Every number is a fold over `lines`.
pub(crate) fn stats(lines: &[Line]) -> Vec<nav::Stat> {
    let pulls = lines.len();
    let kills: Vec<&Line> = lines
        .iter()
        .filter(|l| l.tag == "KILL" || l.tag.starts_with("TIMED"))
        .collect();
    // A zero-length "kill" is a card the log closed before it started; it
    // is not the fastest kill.
    let best_kill = kills
        .iter()
        .map(|l| l.duration_ms)
        .filter(|ms| *ms >= 1_000)
        .min();
    let mut values: Vec<f64> = lines
        .iter()
        .filter_map(|l| l.measure.map(|(_, v)| v))
        .collect();
    values.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let label = lines
        .iter()
        .find_map(|l| l.measure.map(|(m, _)| m))
        .unwrap_or("your measure");
    let best = values.last().copied();
    let median = values.get(values.len() / 2).copied();
    let pinned = lines.iter().filter(|l| l.pinned).count();
    vec![
        nav::Stat {
            label: format!("best {label}"),
            value: best.map_or_else(|| DASH.to_string(), |v| commas(v as u64)),
            sub: Some(format!(
                "median {}",
                median.map_or_else(|| DASH.to_string(), |v| commas(v as u64))
            )),
            value_color: None,
            headline: true,
        },
        nav::Stat {
            label: "pulls".to_string(),
            value: pulls.to_string(),
            sub: Some(format!("{} kills", kills.len())),
            value_color: None,
            headline: false,
        },
        nav::Stat {
            label: "best kill".to_string(),
            value: best_kill.map_or_else(|| DASH.to_string(), duration),
            sub: None,
            value_color: Some(GREEN),
            headline: false,
        },
        nav::Stat {
            label: "pinned".to_string(),
            value: pinned.to_string(),
            sub: Some("kept from retention".to_string()),
            value_color: Some(YELLOW),
            headline: false,
        },
    ]
}

/// The screen: the stored fight when one is open, else the list.
pub(crate) fn screen(
    h: &History,
    owner: Option<&str>,
    accent: theme::Accent,
    density: Density,
    hide_realms: bool,
) -> Element<'static, Message> {
    if let Some(s) = &h.stored {
        return stored_screen(s, accent, density);
    }
    // The owner Home resolved, else whoever the newest card names — the
    // same rule Home itself uses.
    let owner = h
        .character
        .as_deref()
        .or(owner)
        .or_else(|| h.cards.iter().find_map(|c| c.owner.as_deref()));
    let lines = derive(&h.cards, owner);
    // The character filter sits where Home's does: the title's name IS the
    // picker, and this is the one screen whose menu offers "everyone".
    let picks: Vec<nav::CharPick> = h.characters.iter().map(home::char_pick).collect();
    let title = row![
        nav::character_picker(
            &picks,
            h.character.as_deref(),
            true,
            hide_realms,
            Message::TogglePicker,
            accent,
            size::TITLE,
        ),
        text("·").size(size::TITLE * 0.8).color(theme::DIM),
        text(h.scope.title())
            .size(size::TITLE * 0.8)
            .color(theme::DIM),
        text("· history").size(size::TITLE * 0.8).color(theme::DIM),
    ]
    .spacing(8)
    .align_y(iced::Alignment::Center);
    let mut head = column![title].spacing(6);
    // Scope chips: everything, then the bosses and dungeons the cards in
    // hand name — a browser's own contents are its navigation.
    let mut chips: Vec<(String, Message)> =
        vec![("all".to_string(), Message::HistoryOpen(Scope::All))];
    let mut active = (h.scope == Scope::All).then_some(0);
    for c in &h.cards {
        let scope = match (c.kind, c.encounter, &c.key) {
            (FightKind::Encounter, Some(e), _) => Scope::Encounter {
                id: e.id,
                difficulty: Some(e.difficulty),
                name: c.name.clone(),
            },
            (FightKind::Key, _, Some(k)) => Scope::Key {
                map_id: k.map_id,
                name: home::dungeon_name(&c.name).to_string(),
            },
            _ => continue,
        };
        if chips.len() >= 9 {
            break;
        }
        let label = match &scope {
            Scope::Encounter {
                name, difficulty, ..
            } => {
                format!("{name} {}", difficulty_letter(*difficulty))
            }
            Scope::Key { name, .. } => name.clone(),
            Scope::All => continue,
        };
        if chips.iter().any(|(l, _)| *l == label) {
            continue;
        }
        if scope == h.scope {
            active = Some(chips.len());
        }
        chips.push((label, Message::HistoryOpen(scope)));
    }
    if chips.len() > 1 {
        head = head.push(chip_strip(nav::chip_row(chips, active, accent)));
    }
    head = head.push(nav::stat_cards::<Message>(&stats(&lines), accent, density));

    let mut list = column![].spacing(2);
    if lines.is_empty() {
        list = list.push(
            text(if h.answered {
                "no stored fights in this scope"
            } else {
                "reading the history store…"
            })
            .size(size::SMALL)
            .color(DIM),
        );
    }
    let max = lines
        .iter()
        .filter_map(|l| l.measure.map(|(_, v)| v))
        .fold(0.0f64, f64::max);
    for (i, l) in lines.iter().enumerate() {
        list = list.push(
            mouse_area(pull_row(l, i == h.sel, max, accent, h.character.is_none()))
                .on_press(Message::HistoryRow(i)),
        );
    }
    let count = match h.total {
        Some(t) => format!("{} of {t}", h.cards.len()),
        None => String::new(),
    };
    let heads = row![
        text("#")
            .size(size::TINY)
            .color(DIM)
            .font(Font::MONOSPACE)
            .width(Length::Fixed(36.0)),
        text("pull").size(size::TINY).color(DIM).width(Length::Fill),
        text(count)
            .size(size::TINY)
            .color(DIM)
            .font(Font::MONOSPACE),
        text("outcome")
            .size(size::TINY)
            .color(DIM)
            .font(Font::MONOSPACE)
            .width(Length::Fixed(92.0))
            .align_x(iced::Alignment::End),
        text("time")
            .size(size::TINY)
            .color(DIM)
            .font(Font::MONOSPACE)
            .width(Length::Fixed(52.0))
            .align_x(iced::Alignment::End),
        text("you")
            .size(size::TINY)
            .color(DIM)
            .font(Font::MONOSPACE)
            .width(Length::Fixed(64.0))
            .align_x(iced::Alignment::End),
    ]
    .spacing(8)
    .padding([0, 8]);
    column![
        head,
        heads,
        scrollable(crate::view::scroll_clear(list))
            .on_scroll(|v| Message::HistoryScrolled(v.into()))
            .height(Length::Fill)
            .width(Length::Fill),
        text("enter opens the pull · p pins it · esc back")
            .size(size::TINY)
            .color(DIM),
    ]
    .spacing(6)
    .height(Length::Fill)
    .into()
}

fn difficulty_letter(d: Option<u32>) -> &'static str {
    match d {
        Some(16) => "M",
        Some(15) => "H",
        Some(14) => "N",
        Some(17) => "LFR",
        _ => "",
    }
}

/// The unscoped list's bar: nobody's class, so the neutral blue, ramping
/// from transparent at the tail to blue at the leading edge (full on the
/// selected pull).
fn everyone_fill(selected: bool) -> iced::Background {
    let blue = theme::NEUTRAL.base;
    let head = if selected { 1.0 } else { 0.7 };
    iced::Background::Gradient(
        iced::gradient::Linear::new(std::f32::consts::FRAC_PI_2)
            .add_stop(0.0, Color { a: 0.0, ..blue })
            .add_stop(1.0, Color { a: head, ..blue })
            .into(),
    )
}

/// A pull row: the name over a thin bar, so the row is taller than a text
/// line by the bar and its gap.
const ROW_H: f32 = 28.0;
use crate::view::BAR_H;

fn pull_row(
    l: &Line,
    selected: bool,
    max: f64,
    accent: theme::Accent,
    // The list is widened to everyone: the bar is nobody's class color.
    everyone: bool,
) -> Element<'static, Message> {
    // The owner's number as a NARROW bar UNDER the name against the scope's
    // best, so comparing pulls is visual before it is numeric — and the name
    // sits on the panel in its own ink, never on the class color arguing
    // with it.
    let fill = l
        .measure
        .map(|(_, v)| {
            if max > 0.0 {
                (v / max * 100.0).round() as u16
            } else {
                0
            }
        })
        .unwrap_or(0)
        .clamp(0, 100);
    let bar: Element<'static, Message> = if fill == 0 {
        Space::new().width(Length::Fill).height(Length::Fill).into()
    } else {
        row![
            container(Space::new())
                .width(Length::FillPortion(fill))
                .height(Length::Fill)
                // The selected pull's bar is the accent at full strength; the
                // rest sit back, so the selection needs no frame. Widened to
                // everyone the bar is a transparent-to-blue ramp instead —
                // a class color here would read as one character's number.
                .style(move |_: &Theme| container::Style {
                    background: Some(if everyone {
                        everyone_fill(selected)
                    } else if selected {
                        theme::accent_fill(accent)
                    } else {
                        theme::accent_fill(theme::Accent {
                            base: Color {
                                a: 0.55,
                                ..accent.base
                            },
                            lift: Color {
                                a: 0.3,
                                ..accent.lift
                            },
                            ..accent
                        })
                    }),
                    border: iced::border::rounded(2),
                    ..container::Style::default()
                }),
            Space::new().width(Length::FillPortion(100 - fill.max(1))),
        ]
        .into()
    };
    let label = row![
        text(format!("#{}", l.ordinal))
            .size(size::MICRO)
            .color(DIM)
            .font(Font::MONOSPACE)
            .width(Length::Fixed(36.0)),
        text(if l.pinned { "★ " } else { "" })
            .size(size::MICRO)
            .color(YELLOW),
        container(
            text(l.name.clone())
                .size(size::BODY)
                .color(crate::view::name_ink(selected))
                .wrapping(iced::widget::text::Wrapping::None)
        )
        .clip(true)
        .width(Length::Fill),
    ]
    .spacing(8)
    .align_y(iced::Alignment::Center);
    let track = container(
        column![
            container(label).padding([0, 8]).height(Length::Fill),
            container(bar)
                .height(Length::Fixed(BAR_H))
                .width(Length::Fill)
                .style(|_: &Theme| container::Style {
                    background: Some(Color::from_rgba(1.0, 1.0, 1.0, 0.04).into()),
                    border: iced::border::rounded(2),
                    ..container::Style::default()
                }),
        ]
        .spacing(2),
    )
    .clip(true)
    .width(Length::Fill)
    .height(Length::Fill);
    let cell = |s: String, color: Color, w: f32| {
        text(s)
            .size(size::MICRO)
            .color(color)
            .font(Font::MONOSPACE)
            .width(Length::Fixed(w))
            .align_x(iced::Alignment::End)
    };
    container(
        row![
            track,
            cell(l.tag.clone(), l.tag_color, 92.0),
            cell(duration(l.duration_ms), Color::WHITE, 52.0),
            cell(
                l.measure
                    .map_or_else(|| DASH.to_string(), |(_, v)| human(v as u64)),
                Color::WHITE,
                64.0
            ),
        ]
        .spacing(8)
        .padding([0, 8])
        .align_y(iced::Alignment::Center),
    )
    .height(ROW_H)
    .width(Length::Fill)
    .style(move |_: &Theme| container::Style {
        background: selected.then(|| Color::from_rgba(1.0, 1.0, 1.0, 0.04).into()),
        border: iced::border::rounded(3),
        ..container::Style::default()
    })
    .into()
}

fn stored_screen(s: &Stored, accent: theme::Accent, density: Density) -> Element<'static, Message> {
    let fight = match &s.fight {
        None => {
            return column![
                text("reading the stored fight…")
                    .size(size::SMALL)
                    .color(DIM)
            ]
            .height(Length::Fill)
            .into();
        }
        Some(None) => {
            return column![
                text("this fight is no longer in the store")
                    .size(size::SMALL)
                    .color(DIM),
                text("esc back").size(size::TINY).color(DIM),
            ]
            .spacing(6)
            .height(Length::Fill)
            .into();
        }
        Some(Some(f)) => f,
    };
    let card = &fight.card;
    let (tag, tag_color) = home::card_tag(card);
    let title = row![
        nav::two_tone_title::<Message>(
            card.name.clone(),
            format!("· {}", view_name(s.view)),
            (!tag.is_empty()).then_some((tag, tag_color)),
            accent,
            size::TITLE,
        ),
        Space::new().width(Length::Fill),
        text("stored fight").size(size::TINY).color(DIM),
        text(duration(card.duration_ms))
            .size(size::HEAD)
            .font(Font::MONOSPACE),
    ]
    .spacing(10)
    .align_y(iced::Alignment::Center);
    let mut body = column![title].spacing(6).height(Length::Fill);
    body = body.push(crate::view::view_tabs(accent, density, s.view));
    let rows = &fight.rows;
    match (&s.drill, &fight.breakdown) {
        (Some(guid), Some(b)) => {
            let who = rows
                .iter()
                .find(|r| r.key == *guid)
                .map_or_else(|| guid.clone(), |r| r.label.clone());
            body = body.push(text(who).size(size::HEAD));
            let pane = |title: &'static str, rows: &[Row], cols: &'static [table::Col]| {
                let max = rows.iter().map(|r| r.amount).max().unwrap_or(1);
                let mut list = column![].spacing(2);
                for r in rows {
                    list = list.push(crate::view::bar_row::<Message>(
                        r,
                        max,
                        false,
                        22.0,
                        Some(cols),
                        1.0,
                        None,
                        None,
                    ));
                }
                let lead = row![
                    Space::new().width(Length::Fixed(14.0)),
                    text(title).size(size::SMALL),
                ]
                .spacing(table::GAP);
                let mut col = column![
                    crate::view::scroll_clear(table::heads::<Message>(
                        cols, s.view, None, None, lead
                    )),
                    scrollable(crate::view::scroll_clear(list)).height(Length::Fill),
                ]
                .spacing(4);
                if !rows.is_empty() {
                    col = col.push(crate::view::scroll_clear(table::total::<Message>(
                        cols,
                        rows,
                        format!("total · {}", rows.len()),
                        14.0,
                    )));
                }
                col
            };
            body = body.push(
                row![
                    container(pane("by spell", &b.by_spell, table::SPELLS))
                        .width(Length::FillPortion(3)),
                    container(pane("by target", &b.by_target, table::TARGETS))
                        .width(Length::FillPortion(2)),
                ]
                .spacing(10)
                .height(Length::Fill),
            );
        }
        (Some(_), None) => {
            body = body.push(
                text(format!(
                    "no breakdown stored for this player (tier {})",
                    fight.tier
                ))
                .size(size::SMALL)
                .color(DIM),
            );
        }
        (None, _) => {
            body = body.push(nav::stat_cards::<Message>(
                &stored_stats(rows, s.view),
                accent,
                density,
            ));
            let lead = row![
                Space::new().width(Length::Fixed(14.0)),
                text("player").size(size::TINY).color(DIM),
            ]
            .spacing(table::GAP);
            body = body.push(table::heads::<Message>(
                table::METER,
                s.view,
                None,
                None,
                lead,
            ));
            let max = rows.iter().map(|r| r.amount).max().unwrap_or(1);
            let mut list = column![].spacing(2);
            if rows.is_empty() {
                list = list.push(
                    text(format!(
                        "no rows stored for this view (tier {})",
                        fight.tier
                    ))
                    .size(size::SMALL)
                    .color(DIM),
                );
            }
            for (i, r) in rows.iter().enumerate() {
                let el = row![
                    text((i + 1).to_string())
                        .size(size::SMALL)
                        .font(Font::MONOSPACE)
                        .width(Length::Fixed(20.0))
                        .align_x(iced::Alignment::End),
                    crate::view::bar_row::<Message>(
                        r,
                        max,
                        i == s.sel,
                        24.0,
                        Some(table::METER),
                        1.0,
                        None,
                        Some(crate::compare::class_icon::<Message>(
                            r.class, r.spec, None, 18.0,
                        )),
                    ),
                ]
                .spacing(6)
                .align_y(iced::Alignment::Center);
                list = list.push(mouse_area(el).on_press(Message::StoredRow(i)));
            }
            body = body.push(
                scrollable(crate::view::scroll_clear(list))
                    .height(Length::Fill)
                    .width(Length::Fill),
            );
            let ours: Vec<Row> = rows.iter().filter(|r| !r.enemy).cloned().collect();
            body = body.push(crate::view::scroll_clear(table::total::<Message>(
                table::METER,
                &ours,
                format!("total · {} players", ours.len()),
                14.0 + 20.0 + table::GAP,
            )));
        }
    }
    body.into()
}

fn stored_stats(rows: &[Row], view: View) -> Vec<nav::Stat> {
    let ours: Vec<&Row> = rows.iter().filter(|r| !r.enemy).collect();
    let rate = match view {
        View::Healing => "hps",
        View::Taken => "dtps",
        _ => "dps",
    };
    let counted = matches!(
        view,
        View::Interrupts | View::CrowdControl | View::Dispels | View::Deaths
    );
    let total: u64 = ours.iter().map(|r| r.amount).sum();
    let raid: f64 = ours.iter().map(|r| r.per_sec).sum();
    let mut cards = Vec::new();
    if !counted {
        cards.push(nav::Stat {
            label: format!("raid {rate}"),
            value: commas(raid as u64),
            sub: None,
            value_color: None,
            headline: true,
        });
    }
    cards.push(nav::Stat {
        label: if counted {
            view_name(view).to_lowercase()
        } else {
            "total".to_string()
        },
        value: if counted {
            total.to_string()
        } else {
            commas(total)
        },
        sub: Some(format!("{} players", ours.len())),
        value_color: None,
        headline: counted,
    });
    cards
}

/// A chip row that scrolls sideways rather than wrapping: a long boss name
/// at the end must not fold into three lines and push the cards down.
fn chip_strip(chips: Element<'static, Message>) -> Element<'static, Message> {
    scrollable(chips)
        .direction(scrollable::Direction::Horizontal(
            scrollable::Scrollbar::new().width(2).scroller_width(2),
        ))
        .width(Length::Fill)
        .into()
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::window::testkit::simulator;
    use wowdps_daemon::mock::MockDaemon;

    fn cards() -> Vec<FightCard> {
        let mock = MockDaemon::fixture().with_history();
        let mut cards = mock.history().cards().to_vec();
        cards.sort_by_key(|c| std::cmp::Reverse(c.start_utc_ms));
        cards
    }

    #[test]
    fn the_list_numbers_pulls_from_the_oldest_and_carries_the_owner() {
        let cards = cards();
        assert!(cards.len() >= 2, "the fixture stores more than one fight");
        let owner = cards.iter().find_map(|c| c.owner.as_deref());
        let lines = derive(&cards, owner);
        assert_eq!(lines.len(), cards.len());
        assert_eq!(lines[0].fight_id, cards[0].id, "newest first");
        assert_eq!(lines.last().unwrap().ordinal, 1, "the oldest is pull #1");
        assert_eq!(lines[0].ordinal as usize, cards.len());
        if owner.is_some() {
            assert!(
                lines.iter().any(|l| l.measure.is_some()),
                "the owner\x27s number rides the pulls they were on"
            );
        }
        // No owner: no borrowed number.
        assert!(derive(&cards, None).iter().all(|l| l.measure.is_none()));
        let s = stats(&lines);
        assert_eq!(s[1].label, "pulls");
        assert_eq!(s[1].value, cards.len().to_string());
        assert!(s[0].headline);
    }

    #[test]
    fn scopes_match_their_own_cards_only() {
        let cards = cards();
        let boss = cards
            .iter()
            .find(|c| c.kind == FightKind::Encounter && c.encounter.is_some())
            .expect("an encounter card");
        let e = boss.encounter.unwrap();
        let scope = Scope::Encounter {
            id: e.id,
            difficulty: Some(e.difficulty),
            name: boss.name.clone(),
        };
        assert!(scope.matches(boss));
        assert!(
            cards
                .iter()
                .filter(|c| c.kind != FightKind::Encounter)
                .all(|c| !scope.matches(c))
        );
        assert!(Scope::All.matches(boss));
        assert_eq!(scope.title(), boss.name);
        let mut h = History::new(scope);
        let req = h.next_request(7).unwrap();
        assert!(matches!(
            req,
            ClientMsg::GetHistory {
                req_id: 7,
                query: HistoryQuery::Fights {
                    encounter: Some(_),
                    kind: Some(FightKind::Encounter),
                    ..
                }
            }
        ));
        assert!(h.next_request(8).is_none(), "one in flight");
        h.absorb(
            7,
            &HistoryAnswer::Fights {
                cards: cards.clone(),
                total: cards.len() as u32,
            },
        );
        assert!(
            h.cards.iter().all(|c| h.scope.matches(c)),
            "off-scope cards are dropped"
        );
        assert!(!h.cards.is_empty());
        // A pin flip lands on the card it names.
        let id = h.cards[0].id.clone();
        let was = h.cards[0].pinned;
        h.absorb(
            99,
            &HistoryAnswer::Pinned {
                fight_id: id,
                pinned: !was,
            },
        );
        assert_eq!(h.cards[0].pinned, !was);
    }

    #[test]
    fn the_screens_render_in_every_state() {
        let cards = cards();
        let mut h = History::new(Scope::All);
        let mut ui = simulator(screen(
            &h,
            None,
            theme::NEUTRAL,
            Density::Comfortable,
            false,
        ));
        assert!(ui.find("reading the history store…").is_ok());
        h.answered = true;
        let mut ui = simulator(screen(
            &h,
            None,
            theme::NEUTRAL,
            Density::Comfortable,
            false,
        ));
        assert!(ui.find("no stored fights in this scope").is_ok());
        h.cards = cards.clone();
        h.total = Some(cards.len() as u32);
        let mut ui = simulator(screen(
            &h,
            None,
            theme::NEUTRAL,
            Density::Comfortable,
            false,
        ));
        assert!(ui.find("every fight").is_ok());
        assert!(ui.find("pulls").is_ok());
        assert!(ui.find(cards[0].name.as_str()).is_ok());
        ui.click(cards[0].name.as_str()).unwrap();
        assert!(matches!(
            ui.into_messages().next(),
            Some(Message::HistoryRow(0))
        ));
        // The stored fight: waiting, gone, and answered.
        let msg = h.open(cards[0].id.clone(), 3);
        assert!(matches!(msg, ClientMsg::GetFight { req_id: 3, .. }));
        let mut ui = simulator(screen(
            &h,
            None,
            theme::NEUTRAL,
            Density::Comfortable,
            false,
        ));
        assert!(ui.find("reading the stored fight…").is_ok());
        h.absorb_fight(3, None);
        let mut ui = simulator(screen(
            &h,
            None,
            theme::NEUTRAL,
            Density::Comfortable,
            false,
        ));
        assert!(ui.find("this fight is no longer in the store").is_ok());
        let mock = MockDaemon::fixture().with_history();
        let fight = mock
            .history()
            .stored_fight(&cards[0].id, View::Damage, None, None)
            .expect("the fixture fight is stored");
        h.stored.as_mut().unwrap().pending = Some(4);
        h.absorb_fight(4, Some(fight.clone()));
        let mut ui = simulator(screen(
            &h,
            None,
            theme::NEUTRAL,
            Density::Comfortable,
            false,
        ));
        assert!(ui.find("stored fight").is_ok());
        assert!(ui.find("· Damage").is_ok());
        if let Some(top) = fight.rows.first() {
            assert!(ui.find(top.label.as_str()).is_ok());
        }
        // Drilled: the panes, or the honest "no breakdown" line.
        assert!(h.stored.as_mut().unwrap().drill_selected());
        assert!(h.back(), "back closes the drill first");
        assert!(h.stored.as_ref().unwrap().drill.is_none());
        assert!(h.back(), "then the fight");
        assert!(h.stored.is_none());
        assert!(!h.back(), "then there is nothing left to close");
    }
}
