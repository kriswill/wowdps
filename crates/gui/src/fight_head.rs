//! The fight header over the window's meter (the prototype's `.fhead`):
//! ONE title line and ONE stat line, where the meter used to wear a title,
//! the instance strip, a second title line with its ‹ › chips and a band
//! of stat cards — some 380 px of a 1000 px window, which left a 25-player
//! raid 14 rows.
//!
//! The title line: the encounter in Marcellus, then in secondary ink its
//! difficulty and size, the outcome badge and the duration, and at its end
//! the older / newer pull buttons (the pointer's `[` and `]`, and ← →). The
//! stat line: the view's raid figures as gold-dim label and ink value
//! pairs — full figures with their commas, the line being where the whole
//! number is the point — and after them the "you" chip: the owner's place
//! in the view, a click away from their row.
//!
//! The deaths come from the snapshot's raid timeline (R25, v35), which
//! answers for the whole fight on every view: "Deaths 6" on the Damage and
//! Enemies lines, "First 1:10" and "Battle rezzes 2" on Deaths, and the
//! chip's "died 5:45" — a stored pull's too, from the store's rebuild of it;
//! a store that kept the card alone sends none, and its lines say less
//! rather than borrow a count from another view. Only the group's deaths
//! count: an arena's other team dies in the timeline, flagged `enemy`, and a
//! self-rez is no battle rez. The owner is the row
//! the daemon marked `mine`. Still waiting: a live pull's best health for
//! "Wipe at N%" (the badge words it from [`Verdict::wipe_pct`], which only
//! a stored card fills).
//!
//! Window-only. The overlay keeps its strip and its own header, and
//! nothing here is a renderer it shares.

use iced::widget::{Row as Line, Space, button, column, container, row, text};
use iced::{Border, Color, Element, Font, Length, Theme};

use wowdps_model::fmt::duration;

use crate::ellipsis::ellipsis;
use crate::line_icons::LineIcon;
use crate::nav::{self, Badge};
use crate::theme::{self, pitch, size};
use crate::view::display_name;
use crate::window::{Gui, Message};
use wowdps_gui_logic::labels::Tone;

/// The you chip's spec disc, and the chip itself: the disc and the 3 px
/// above and below it (iced draws the 1 px border inside that) — the
/// prototype's `2px` padding and `1px` border around a 20 px disc.
const DISC: f32 = 20.0;
pub(crate) const CHIP_H: f32 = DISC + 2.0 * 3.0;
/// Inside the chip (`.youchip{padding:2px 10px 2px 3px}` and its 1 px
/// border): the disc tucked to its left edge, air after the words.
const CHIP_PAD: iced::Padding = iced::Padding {
    top: 3.0,
    right: 10.0,
    bottom: 3.0,
    left: 3.0,
};
/// Between the chip's pieces (`.youchip{gap:8px}`).
const CHIP_GAP: f32 = 8.0;
/// Gaps: the title line's pieces (`.ftitle{gap:12px}`), the meta's
/// (`.fmeta{gap:10px}`), the stat pairs' (`.stats{gap:4px 22px}`, `2px
/// 14px` narrow) and a pair's own (`.stat{gap:7px}`).
const TITLE_GAP: f32 = 12.0;
const META_GAP: f32 = 10.0;
const STATS_GAP: f32 = 22.0;
const STATS_GAP_NARROW: f32 = 14.0;
const STATS_ROW_GAP: f32 = 4.0;
const STATS_ROW_GAP_NARROW: f32 = 2.0;
const PAIR_GAP: f32 = 7.0;
/// Between the two lines (`.fhead{gap:7px}`).
const LINE_GAP: f32 = 7.0;
/// The title's line (`.ftitle h2{line-height:1.1}`): the title line is as
/// tall as its 30 px step buttons, not as a 27 px face's default leading.
const TITLE_LEADING: f32 = 1.1;
/// The header inside the fight's workspace, which runs edge to edge under
/// the top bar: the prototype's `.fhead{padding:13px 18px 9px}`.
pub(crate) const PAD: iced::Padding = iced::Padding {
    top: 13.0,
    right: 18.0,
    bottom: 9.0,
    left: 18.0,
};
/// The same under 820 px (`.fhead{padding:10px 12px 8px}`).
pub(crate) const PAD_NARROW: iced::Padding = iced::Padding {
    top: 10.0,
    right: 12.0,
    bottom: 8.0,
    left: 12.0,
};
/// Between the older and newer buttons (`.fnav{gap:2px}`), and the two
/// together: two targets and the gap between.
const STEPS_GAP: f32 = 2.0;
const STEPS_W: f32 = 2.0 * pitch::ICON_BUTTON + STEPS_GAP;

/// How wide the window is, as the header's own width says (the header runs
/// the window's width; the breakpoints are the window's,
/// `theme::NARROW_WINDOW` and `theme::TILE_WINDOW`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Fit {
    /// Under `theme::NARROW_WINDOW`: the title a step smaller, the header
    /// inset less, the stat line wrapping.
    Narrow,
    /// Under `theme::TILE_WINDOW`: the chip right after the figures.
    Tile,
    /// The chip at the stat line's far end.
    Wide,
}

impl Fit {
    fn of(width: f32) -> Self {
        if width < theme::NARROW_WINDOW {
            Fit::Narrow
        } else if width <= theme::TILE_WINDOW {
            Fit::Tile
        } else {
            Fit::Wide
        }
    }

    /// The header's inset at this width.
    fn pad(self) -> iced::Padding {
        match self {
            Fit::Narrow => PAD_NARROW,
            Fit::Tile | Fit::Wide => PAD,
        }
    }
}

/// Everything the header says, gathered as owned data: the layout runs
/// again at every width (`responsive`) and holds none of the window.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Head {
    /// The fight's name; `None` before there is a fight, when the line says
    /// it is waiting — in the quiet ink of a status, not as a title.
    pub title: Option<String>,
    /// "Heroic, 25 players"; empty off a boss pull.
    pub meta: String,
    pub badge: Option<Badge>,
    /// Empty before there is a fight to time.
    pub duration: String,
    /// How long a live pull has been silent (the game buffers its log).
    pub stale: Option<String>,
    /// There is a newer / an older pull on the rail to step to.
    pub newer: bool,
    pub older: bool,
    /// The stat line, when this header wears one and there is a fight to
    /// sum: "waiting for combat…" is no place for a confident zero.
    pub stats: Option<Stats>,
    /// What the line says with no fight to name: [`WAITING`] for combat,
    /// or [`READING`] while the store answers for a stored pull.
    pub waiting: &'static str,
    /// The rail is a drawer, and the line leads with its button
    /// (`.railbtn`, shown at 1180 px and under).
    pub rail_button: bool,
    /// How wide the docked rail beside the stage is: the header measures
    /// the stage, and the breakpoints are the window's.
    pub beside: f32,
    /// The pull's card is pinned: a star after the title, as on the rail.
    pub pinned: bool,
    /// The night a stored pull of an earlier night is from ("Mon, Sep 21"):
    /// the header is what says where on the rail the stage is once the
    /// drawer is shut. `None` for tonight's.
    pub night: Option<String>,
}

impl Head {
    pub(crate) fn of(state: &Gui, stats: bool) -> Self {
        let app = state.fight();
        let (newer, older) = state.pull_steps();
        let name = app.segment_name();
        let missing = state.stored.as_ref().filter(|s| s.missing);
        // A stored card knows how close a wipe came, which a live snapshot
        // does not carry yet — a stored pull's own, or the card the store
        // wrote for a pull of the log, paired as the rail pairs them.
        let card = state.stage_card();
        let wipe_pct = card.and_then(crate::home::wipe_pct);
        let tonight = state.tonight();
        let night = state
            .stored
            .as_ref()
            .and(card)
            .map(|c| crate::rail::night_of(c.start_local_ms))
            .filter(|day| *day != tonight)
            .map(|day| crate::rail::night_short(day, tonight));
        Head {
            duration: name
                .as_ref()
                .map_or_else(String::new, |_| duration(app.duration_ms())),
            stats: (stats && name.is_some()).then(|| stats_of(state)),
            title: name,
            meta: meta(app.segment_encounter()),
            badge: badge(Verdict {
                wipe_pct,
                ..Verdict::of(app)
            }),
            // The game buffers its log writes; say how far behind the file
            // is rather than let a live fight look frozen.
            stale: app
                .is_live()
                .then(|| state.stale_secs())
                .flatten()
                .map(|secs| format!("no events for {secs}s")),
            newer,
            older,
            waiting: match (&state.stored, missing) {
                (_, Some(_)) => GONE,
                (Some(_), None) => READING,
                (None, None) => WAITING,
            },
            rail_button: false,
            beside: 0.0,
            pinned: state.pin_target().is_some_and(|(_, pinned)| pinned),
            night,
        }
    }

    /// The header laid out at the width the window gives it — what a
    /// layout that is itself laid out by its width (the meter's split)
    /// builds from a `Head` it holds.
    pub(crate) fn element(self) -> Element<'static, Message> {
        container(
            iced::widget::responsive(move |bounds| self.layout(bounds.width))
                .height(Length::Shrink),
        )
        .width(Length::Fill)
        .into()
    }

    fn layout(&self, width: f32) -> Element<'static, Message> {
        let fit = Fit::of(width + self.beside);
        let mut lines = column![self.title_line(fit)].spacing(LINE_GAP);
        if let Some(stats) = &self.stats {
            lines = lines.push(stat_line(stats, fit));
        }
        container(lines)
            .padding(fit.pad())
            .width(Length::Fill)
            .into()
    }

    /// The title, what it was, and the steps to its neighbours. The title
    /// is what gives way, in a narrow window as in a wide one: it ends in
    /// "…" before the meta or a step button is pushed off the line
    /// (`.ftitle h2{min-width:0}` — the prototype's narrow frame keeps
    /// "Heroic, 25 players" whole beside "The Coiled …").
    fn title_line(&self, fit: Fit) -> Element<'static, Message> {
        let narrow = fit == Fit::Narrow;
        let (title_px, meta_px) = if narrow {
            (size::ENCOUNTER_NARROW, size::META_NARROW)
        } else {
            (size::ENCOUNTER, size::META)
        };
        // The meta's pieces in secondary ink, and the room the title leaves
        // for them: their words, measured when laid out, and their frames.
        let mut meta = row![].spacing(META_GAP).align_y(iced::Alignment::Center);
        let mut words: Vec<(String, f32, Font)> = Vec::new();
        let mut fixed = 0.0;
        // A pinned pull says so first, a shape as the rail's row does, in
        // its quiet ink: its card is one retention keeps. Narrow, the title
        // needs the room more — the rail's row and `p`'s word say it.
        if self.pinned && !narrow {
            words.push((crate::rail::PIN.to_string(), meta_px, theme::UI));
            meta = meta.push(nav::tip(
                text(crate::rail::PIN)
                    .size(meta_px)
                    .color(theme::INK_3)
                    .wrapping(text::Wrapping::None),
                crate::rail::PIN_TIP,
            ));
        }
        // An earlier night's pull says which night, before what it was.
        // Narrow, it gives way before the verdict does.
        if let Some(night) = self.night.as_ref().filter(|_| !narrow) {
            words.push((night.clone(), meta_px, theme::UI));
            meta = meta.push(quiet(night, meta_px));
        }
        if !self.meta.is_empty() {
            words.push((self.meta.clone(), meta_px, theme::UI));
            meta = meta.push(quiet(&self.meta, meta_px));
        }
        let mut pieces = words.len();
        if let Some(b) = &self.badge {
            words.push((nav::sentence(&b.word), size::MICRO, theme::UI_SEMIBOLD));
            if let Some(detail) = &b.detail {
                words.push((detail.clone(), size::MICRO, theme::UI));
            }
            fixed += nav::badge_extra(b);
            pieces += 1;
            meta = meta.push(nav::badge(b));
        }
        if !self.duration.is_empty() {
            words.push((self.duration.clone(), meta_px, theme::UI));
            pieces += 1;
            meta = meta.push(quiet(&self.duration, meta_px));
        }
        if let Some(stale) = &self.stale {
            words.push((stale.clone(), size::MICRO, theme::UI));
            pieces += 1;
            meta = meta.push(quiet(stale, size::MICRO));
        }
        // The line's own gaps are taken off before any piece is laid out;
        // the meta's are inside the meta.
        fixed += META_GAP * (pieces as f32 - 1.0).max(0.0) + STEPS_W;
        // ‹ is older and › newer, as ← and → and `[` and `]` are: the
        // pointer and the keys step the same way. (The prototype's `.fnav`
        // puts newer first; the window reads left as older, as its keys do
        // — a departure the decision record names.)
        let steps = row![
            nav::tip(
                nav::icon_button(
                    LineIcon::ChevronLeft,
                    self.older.then_some(Message::OlderPull),
                    None,
                ),
                OLDER_TIP,
            ),
            nav::tip(
                nav::icon_button(
                    LineIcon::ChevronRight,
                    self.newer.then_some(Message::NewerPull),
                    None,
                ),
                NEWER_TIP,
            ),
        ]
        .spacing(STEPS_GAP);
        // Before there is a fight the line says so as a status would, in
        // the quiet ink — not in the title's face, as though a fight were
        // called that.
        let title: Element<'static, Message> = match &self.title {
            Some(title) => ellipsis(title.clone())
                .size(title_px)
                .font(theme::TITLE)
                .color(theme::INK)
                .line_height(text::LineHeight::Relative(TITLE_LEADING))
                .leaving(words, fixed)
                .into(),
            None => container(quiet(self.waiting, meta_px))
                .height(Length::Fixed(pitch::ICON_BUTTON))
                .align_y(iced::Alignment::Center)
                .into(),
        };
        // Where the rail is a drawer, its button leads the line: the pulls
        // are a press away from the fight they list. (Laid out ahead of the
        // title, it is off the title's room before the title measures.)
        let mut line = row![].spacing(TITLE_GAP).align_y(iced::Alignment::Center);
        if self.rail_button {
            line = line.push(nav::tip(
                nav::icon_button(
                    LineIcon::List,
                    Some(Message::OpenRail),
                    Some(rail_button_id()),
                ),
                RAIL_TIP,
            ));
        }
        line.push(title)
            .push(meta)
            .push(Space::new().width(Length::Fill))
            .push(steps)
            .into()
    }
}

/// The rail's button on the title line, so a test can press it.
pub(crate) fn rail_button_id() -> iced::widget::Id {
    iced::widget::Id::new("rail-button")
}

/// One of the meta's words, in secondary ink on one line — in the face it
/// is measured in.
fn quiet(s: &str, px: f32) -> Element<'static, Message> {
    text(s.to_string())
        .size(px)
        .font(theme::UI)
        .color(theme::INK_2)
        .wrapping(text::Wrapping::None)
        .into()
}

/// The stat line: the pairs, then the chip — at the line's far end in a
/// wide window, right after the pairs in a tile (`.youchip{margin-left:0}`
/// under 1180 px), and in a narrow one after them with the whole line
/// wrapping as it must. The line is never shorter than the chip, so a view
/// without one (Enemies), or one whose answer is on its way, lays its tabs
/// and rows out where every other view does.
fn stat_line(stats: &Stats, fit: Fit) -> Element<'static, Message> {
    let value_px = if fit == Fit::Narrow {
        size::STAT_NARROW
    } else {
        size::STAT
    };
    let pairs: Vec<Element<'static, Message>> = stats
        .pairs
        .iter()
        .map(|p| {
            row![
                text(p.label.clone())
                    .size(size::LABEL)
                    .color(theme::GOLD_DIM)
                    .wrapping(text::Wrapping::None),
                text(p.value.clone())
                    .size(value_px)
                    .font(theme::UI_MEDIUM)
                    .color(theme::INK)
                    .wrapping(text::Wrapping::None),
            ]
            .spacing(PAIR_GAP)
            // The label sits on the value's baseline, near enough.
            .align_y(iced::Alignment::End)
            .into()
        })
        .collect();
    let chip = stats.you.as_ref().map(you_chip);
    let body: Element<'static, Message> = match fit {
        // The chip follows the figures, and the whole line wraps as it
        // must (`.stats{flex-wrap:wrap}`) rather than clip the chip.
        Fit::Narrow | Fit::Tile => {
            let (gap, rows_gap) = if fit == Fit::Narrow {
                (STATS_GAP_NARROW, STATS_ROW_GAP_NARROW)
            } else {
                (STATS_GAP, STATS_ROW_GAP)
            };
            let mut line = Line::with_children(pairs)
                .spacing(gap)
                .align_y(iced::Alignment::Center);
            if let Some(chip) = chip {
                line = line.push(chip);
            }
            line.wrap().vertical_spacing(rows_gap).into()
        }
        Fit::Wide => {
            let mut line = row![
                Line::with_children(pairs)
                    .spacing(STATS_GAP)
                    .align_y(iced::Alignment::End),
                Space::new().width(Length::Fill),
            ]
            .align_y(iced::Alignment::Center);
            if let Some(chip) = chip {
                line = line.push(chip);
            }
            line.into()
        }
    };
    // The chip's height, held by the line whether or not the chip is on it.
    // (A Shrink width, not a Fixed 0, which a row drops as void.)
    row![Space::new().height(CHIP_H), body]
        .align_y(iced::Alignment::Center)
        .into()
}

/// The chip (`.youchip`): a pill washed in the owner's class colour with
/// its edge a step stronger, their spec disc, their name as the owner's
/// text (`--you-text`, AA on the wash where a player's name colour is
/// not), and what the view says of them. A press selects their row — when
/// they have one here; a chip that speaks from another view is the same
/// pill, inert.
fn you_chip(you: &You) -> Element<'static, Message> {
    let raw = you.class.map_or(theme::INK_3, theme::class_rgb);
    let mut line = row![
        crate::compare::class_icon::<Message>(you.class, you.spec, None, DISC),
        text(you.name.clone())
            .size(size::CHIP)
            .font(theme::UI_SEMIBOLD)
            .color(you.class.map_or(theme::INK, theme::you_text))
            .wrapping(text::Wrapping::None),
        text(you.words.clone())
            .size(size::CHIP)
            .color(theme::INK)
            .wrapping(text::Wrapping::None),
    ]
    .spacing(CHIP_GAP)
    .align_y(iced::Alignment::Center);
    if let Some(figure) = &you.figure {
        line = line.push(
            text(figure.clone())
                .size(size::CHIP)
                .color(theme::INK_2)
                .wrapping(text::Wrapping::None),
        );
    }
    let pill = move |wash: f32| {
        let fill = Color { a: wash, ..raw };
        let edge = Color {
            a: theme::YOU_EDGE,
            ..raw
        };
        (fill, edge)
    };
    if !you.selectable {
        let (fill, edge) = pill(theme::YOU_WASH);
        return container(line)
            .padding(CHIP_PAD)
            .height(CHIP_H)
            .style(move |_: &Theme| container::Style {
                background: Some(fill.into()),
                border: Border {
                    color: edge,
                    width: 1.0,
                    radius: (CHIP_H / 2.0).into(),
                },
                ..container::Style::default()
            })
            .into();
    }
    let chip = button(line)
        .padding(CHIP_PAD)
        .height(CHIP_H)
        .on_press(Message::SelectOwner)
        .style(move |_: &Theme, status| {
            let (fill, edge) = pill(match status {
                button::Status::Hovered | button::Status::Pressed => theme::YOU_WASH_HOVER,
                _ => theme::YOU_WASH,
            });
            button::Style {
                background: Some(fill.into()),
                text_color: theme::INK,
                border: Border {
                    color: edge,
                    width: 1.0,
                    radius: (CHIP_H / 2.0).into(),
                },
                ..button::Style::default()
            }
        });
    nav::tip(chip, "Select your row")
}

// ---- the words ---------------------------------------------------------------
//
// What the header says is gui-logic's (`fight_head`); this GUI colours it.

pub(crate) use wowdps_gui_logic::fight_head::{
    GONE, NEWER_TIP, OLDER_TIP, RAIL_TIP, READING, Seen, Stats, Verdict, WAITING, You, meta,
    ordinal, outcome, pairs, player_chart, you,
};

/// The outcome badge (`.badge`), its word [`outcome`]'s: Live with its red
/// dot; a good word in green, a bad one in red. `None` when there is nothing
/// to say.
pub(crate) fn badge(v: Verdict) -> Option<Badge> {
    let (word, tone) = outcome(v)?;
    Some(match tone {
        Tone::Live => Badge::live(),
        Tone::Good => Badge::new(word, theme::GOOD),
        Tone::Bad | Tone::None => Badge::new(word, theme::BAD),
    })
}

/// The stat line for the view on screen — or, while its answer is on its
/// way (asked for, not in; a stored pull still loading), nothing: the
/// rows in hand are another view's or a placeholder's (`fight_head::
/// pending`). The line keeps its height.
fn stats_of(state: &Gui) -> Stats {
    let app = state.fight();
    if wowdps_gui_logic::fight_head::pending(app) {
        return Stats::default();
    }
    let rows = app.rows();
    let seen = state.seen.of(app).and_then(Seen::owner);
    let owner = state.owner_in(&rows);
    let raid = app.raid();
    // R25: when the owner died, as the raid timeline marks it.
    let died_at = owner
        .and_then(|i| rows.get(i))
        .zip(raid)
        .and_then(|(me, r)| r.deaths.iter().find(|d| d.guid == me.key))
        .map(|d| d.at_ms);
    let you = you(app.view, &rows, owner, seen, app.is_live(), died_at).map(|mut you| {
        if !state.cfg.hide_realms {
            return you;
        }
        you.name = display_name(&you.name).to_string();
        you
    });
    Stats {
        pairs: pairs(app.view, &rows, raid),
        you,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::window::testkit::{self as tk, apply, simulator, simulator_as};
    use wowdps_gui_logic::fight_head::is_named;
    use wowdps_model::fmt::commas;
    use wowdps_model::{Action, Encounter, Row, SegmentKind, Spec, View};
    use wowdps_proto::ClientState;

    /// The header over the window's fight, with its stat line or without.
    fn view(state: &Gui, stats: bool) -> Element<'static, Message> {
        Head::of(state, stats).element()
    }

    fn row(label: &str, amount: u64, spec: Option<Spec>) -> Row {
        Row {
            key: format!("Player-{label}"),
            label: format!("{label}-Realm-US"),
            amount,
            extra: amount / 10,
            per_sec: amount as f64 / 100.0,
            class: spec.map(Spec::class),
            spec,
            ..Row::default()
        }
    }

    #[test]
    fn the_meta_words_difficulty_and_size() {
        let e = |difficulty, group_size| {
            Some(Encounter {
                id: 3492,
                difficulty,
                group_size,
            })
        };
        assert_eq!(meta(e(15, 25)), "Heroic, 25 players");
        assert_eq!(meta(e(16, 20)), "Mythic, 20 players");
        assert_eq!(meta(e(8, 5)), "Mythic Keystone, 5 players");
        assert_eq!(meta(e(14, 1)), "Normal, 1 player");
        assert_eq!(meta(e(999, 10)), "10 players", "an unknown difficulty");
        assert_eq!(meta(e(15, 0)), "Heroic", "no size said");
        assert_eq!(meta(None), "", "trash, a Σ, an arena");
    }

    /// Every outcome worded as the prototype's badges are, in green or red.
    #[test]
    fn every_outcome_has_its_badge() {
        let word = |v: Verdict| badge(v).map(|b| (b.word, b.color == theme::GOOD, b.live));
        let fight = |success| Verdict {
            kind: Some(SegmentKind::Encounter),
            success,
            duration_ms: 422_040,
            ..Verdict::default()
        };
        assert_eq!(word(fight(Some(true))), Some(("Kill".into(), true, false)));
        assert_eq!(
            word(fight(Some(false))),
            Some(("Wipe".into(), false, false))
        );
        let close = Verdict {
            wipe_pct: Some(56),
            ..fight(Some(false))
        };
        assert_eq!(word(close), Some(("Wipe at 56%".into(), false, false)));
        let live = Verdict {
            live: true,
            ..fight(None)
        };
        assert_eq!(word(live), Some(("Live".into(), false, true)), "a red dot");
        let arena = |success| Verdict {
            arena: true,
            ..fight(success)
        };
        assert_eq!(word(arena(Some(true))), Some(("Win".into(), true, false)));
        assert_eq!(
            word(arena(Some(false))),
            Some(("Loss".into(), false, false))
        );
        let key = |success, pars_ms| Verdict {
            kind: Some(SegmentKind::Overall),
            success,
            pars_ms,
            duration_ms: 1_600_000,
            ..Verdict::default()
        };
        let pars = Some((2_040_000, 1_632_000, 1_224_000));
        assert_eq!(
            word(key(Some(true), pars)),
            Some(("Timed +2".into(), true, false))
        );
        assert_eq!(
            word(key(Some(true), None)),
            Some(("Timed".into(), true, false))
        );
        assert_eq!(
            word(key(Some(false), pars)),
            Some(("Over time".into(), false, false))
        );
        assert_eq!(word(fight(None)), None, "a pull the log cut off");
        assert_eq!(word(key(None, None)), None, "a raid visit's Σ");
        // Drawn, the badge keeps its words as they are.
        let mut ui = simulator(nav::badge::<()>(&badge(close).unwrap()));
        assert!(ui.find("Wipe at 56%").is_ok());
    }

    /// The stat line folds each view's rows into the prototype's pairs,
    /// full figures with their commas, OUR side only — and never a figure
    /// the snapshot does not carry: no Deaths on Damage or Enemies until
    /// the wire says how many died, whichever tabs the reader has visited.
    #[test]
    fn the_stat_line_folds_each_view_with_commas() {
        let state = tk::raid(25);
        let rows = state.rows();
        let total: u64 = rows.iter().map(|r| r.amount).sum();
        assert_eq!(total, 1_900_000_000);
        let rate: f64 = rows.iter().map(|r| r.per_sec).sum();
        let line = |view| -> Vec<(String, String)> {
            pairs(view, &rows, None)
                .into_iter()
                .map(|p| (p.label, p.value))
                .collect()
        };
        let raid = commas(rate.round() as u64);
        let is = |l: &str, v: &str| (l.to_string(), v.to_string());
        assert_eq!(
            line(View::Damage),
            vec![is("Raid dps", &raid), is("Damage", "1,900,000,000")]
        );
        // 25 × 50,000 overheal against 1.9 B healed.
        let over = 1_250_000.0 / 1_901_250_000.0 * 100.0;
        assert_eq!(
            line(View::Healing),
            vec![
                is("Raid hps", &raid),
                is("Healing", "1,900,000,000"),
                is("Overheal", &format!("{over:.1}%")),
            ]
        );
        assert_eq!(
            line(View::Taken),
            vec![
                is("Raid dtps", &raid),
                is("Taken", "1,900,000,000"),
                is("Absorbed", "1,250,000"),
            ]
        );
        assert_eq!(
            line(View::EnemyTaken),
            vec![
                is("Raid dtps", &raid),
                is("Damage to enemies", "1,900,000,000")
            ]
        );
        assert_eq!(line(View::Deaths), vec![is("Deaths", "1,900,000,000")]);
        assert_eq!(
            line(View::CrowdControl),
            vec![is("Crowd control", "1,900,000,000"), is("Players", "25")]
        );
        // R13: the enemy team is on the chart, not in the fold.
        let mut arena = rows.clone();
        for r in &mut arena[20..] {
            r.enemy = true;
        }
        let players = pairs(View::Interrupts, &arena, None);
        assert_eq!(players[1].value, "20");
        // Drawn over the meter, the whole figure is on screen.
        let (gui, _peer) = tk::gui_over(state);
        let mut ui = tk::wide(view(&gui, true));
        assert!(ui.find("Raid dps").is_ok());
        assert!(ui.find("1,900,000,000").is_ok());
        assert!(ui.find("Heroic, 25 players").is_ok());
        assert!(ui.find("7:02").is_ok());
        assert!(ui.find("Kill").is_ok());
    }

    /// The Damage line says the same whatever the reader visited first: a
    /// trip to the Deaths tab — its reply in or not yet — changes nothing
    /// on it. Its Deaths pair is the raid timeline's (v35), which every
    /// snapshot carries, never a count borrowed from the Deaths tab.
    #[test]
    fn the_damage_line_does_not_depend_on_the_tabs_visited() {
        let (state, mut mock) = tk::kill();
        let (mut gui, _peer) = tk::gui_over(state);
        let labels = |gui: &Gui| -> Vec<String> {
            Head::of(gui, true)
                .stats
                .expect("a fight's stat line")
                .pairs
                .into_iter()
                .map(|p| p.label)
                .collect()
        };
        let before = labels(&gui);
        // Asked for Deaths, its snapshot not in yet: what the window sees
        // is the old view's reply.
        let _ = gui.state.apply(Action::SetView(View::Deaths));
        gui.seen.observe(&gui.state, None);
        apply(&mut gui.state, &mut mock, Action::SetView(View::Deaths));
        gui.seen.observe(&gui.state, None);
        apply(&mut gui.state, &mut mock, Action::SetView(View::Damage));
        gui.seen.observe(&gui.state, None);
        assert_eq!(labels(&gui), before);
        assert!(before.iter().any(|l| l == "Deaths"), "{before:?}");
    }

    /// The chip places the owner among their own role on Damage, gives the
    /// rate elsewhere, the count with its noun on a count view, says died,
    /// survived or alive on Deaths — and is not there for an owner the
    /// chart does not hold and the window never saw in the fight.
    #[test]
    fn the_you_chip_says_where_the_owner_stands() {
        let rows = vec![
            row("Ace", 300, Some(Spec::Fire)),
            row("Bea", 250, Some(Spec::HolyPriest)),
            row("Cid", 200, Some(Spec::Demonology)),
            row("Dru", 150, Some(Spec::Guardian)),
            row("Eve", 1, Some(Spec::Havoc)),
        ];
        let words = |view, owner, seen: Option<&Row>| {
            you(view, &rows, owner, seen, false, None).map(|y| (y.words, y.figure))
        };
        assert_eq!(
            words(View::Damage, Some(2), None),
            Some(("2nd of 3 dps".into(), Some("2".into()))),
            "Cid is second of the three dps, the healer and tank aside"
        );
        assert_eq!(
            words(View::Damage, Some(1), None),
            Some(("1st of 1 healer".into(), Some("3".into())))
        );
        assert_eq!(
            words(View::Healing, Some(1), None),
            Some(("3 hps".into(), None))
        );
        assert_eq!(
            words(View::Taken, Some(3), None),
            Some(("2 dtps".into(), None))
        );
        // A count says what it counted.
        assert_eq!(
            words(View::Interrupts, Some(0), None),
            Some(("300 interrupts".into(), None))
        );
        assert_eq!(
            words(View::Dispels, Some(4), None),
            Some(("1 dispel".into(), None))
        );
        assert_eq!(
            words(View::CrowdControl, Some(3), None),
            Some(("150 crowd control".into(), None))
        );
        // An owner with no row on a count view, seen in the fight: none.
        assert_eq!(
            words(View::Interrupts, None, Some(&rows[2])),
            Some(("no interrupts".into(), None))
        );
        assert_eq!(
            words(View::CrowdControl, None, Some(&rows[2])),
            Some(("no crowd control".into(), None))
        );
        assert_eq!(
            words(View::EnemyTaken, Some(0), None),
            None,
            "enemies' rows"
        );
        assert_eq!(words(View::Damage, None, None), None, "not in the fight");
        assert_eq!(words(View::Dispels, None, None), None, "never seen here");
        // Deaths: a row is a death; no row is survival, when the window saw
        // the owner in this fight at all — and while it goes on, alive.
        let mut deaths = rows.clone();
        deaths[2].amount = 1;
        assert_eq!(
            you(View::Deaths, &deaths, Some(2), None, false, None).map(|y| y.words),
            Some("died".into())
        );
        deaths[2].amount = 2;
        assert_eq!(
            you(View::Deaths, &deaths, Some(2), None, false, None).map(|y| y.words),
            Some("died 2 times".into())
        );
        assert_eq!(
            words(View::Deaths, None, Some(&rows[2])),
            Some(("survived".into(), None))
        );
        assert_eq!(
            you(View::Deaths, &rows, None, Some(&rows[2]), true, None).map(|y| y.words),
            Some("alive".into()),
            "a pull still going has no survivors yet"
        );
        assert_eq!(words(View::Deaths, None, None), None, "never seen here");
        // The chip carries the owner's spec and name.
        let chip = you(View::Damage, &rows, Some(2), None, false, None).unwrap();
        assert_eq!(chip.spec, Some(Spec::Demonology));
        assert_eq!(chip.name, "Cid-Realm-US");
        // A press selects a row the chart holds; a chip that speaks from
        // another view has none to go to, and takes no press.
        assert!(chip.selectable);
        let survived = you(View::Deaths, &rows, None, Some(&rows[2]), false, None).unwrap();
        let none = you(View::Interrupts, &rows, None, Some(&rows[2]), false, None).unwrap();
        for inert in [&survived, &none] {
            assert!(!inert.selectable, "{}", inert.words);
            let mut ui = simulator(you_chip(inert));
            ui.click(inert.words.as_str()).unwrap();
            assert_eq!(ui.into_messages().count(), 0, "{}", inert.words);
        }
        let mut ui = simulator(you_chip(&chip));
        ui.click(chip.words.as_str()).unwrap();
        assert!(matches!(
            ui.into_messages().collect::<Vec<_>>().as_slice(),
            [Message::SelectOwner]
        ));
    }

    #[test]
    fn ordinals_read_as_places() {
        for (n, s) in [
            (1, "1st"),
            (2, "2nd"),
            (3, "3rd"),
            (4, "4th"),
            (11, "11th"),
            (12, "12th"),
            (13, "13th"),
            (17, "17th"),
            (21, "21st"),
            (22, "22nd"),
            (23, "23rd"),
            (101, "101st"),
            (111, "111th"),
        ] {
            assert_eq!(ordinal(n), s);
        }
    }

    #[test]
    fn a_configured_name_names_its_row_whole_or_by_its_name_half() {
        assert!(is_named("Tranqlock-Proudmoore-US", "Tranqlock"));
        assert!(is_named(
            "Tranqlock-Proudmoore-US",
            "tranqlock-proudmoore-us"
        ));
        assert!(is_named("Akanôs-Nebula-US", "AKANÔS"));
        assert!(!is_named("Tranqlocker-Proudmoore-US", "Tranqlock"));
        assert!(!is_named("Tranqlock-Proudmoore-US", "Proudmoore"));
    }

    /// What the other views said is kept for the fight it was said of: the
    /// owner seen on Damage lets Deaths say "survived". A new fight starts
    /// over.
    #[test]
    fn seen_carries_one_fight_s_owner_across_its_views() {
        let (mut state, mut mock) = tk::kill();
        let mut seen = Seen::default();
        let owner = state.rows().last().cloned().unwrap();
        seen.observe(&state, Some(&owner));
        let facts = seen.of(&state).expect("this fight's");
        assert_eq!(facts.owner(), Some(&owner));
        apply(&mut state, &mut mock, Action::SetView(View::Deaths));
        seen.observe(&state, None);
        let facts = seen.of(&state).unwrap();
        assert_eq!(facts.owner(), Some(&owner), "still seen");
        // Another fight: what was seen was of the last one.
        apply(&mut state, &mut mock, Action::OlderSegment);
        assert!(seen.of(&state).is_none());
        seen.observe(&state, None);
        let facts = seen.of(&state).unwrap();
        assert_eq!(facts.owner(), None);
    }

    /// The pull's players are its own: while the pull on screen loads, the
    /// rows in hand are no list of who fought in it — none, rather than the
    /// last pull's.
    #[test]
    fn a_loading_pull_lists_no_one_else_s_players() {
        let (mut state, mut mock) = tk::kill();
        let mut seen = Seen::default();
        seen.observe(&state, None);
        assert!(!seen.players(&state).is_empty(), "the kill's players");
        apply(&mut state, &mut mock, Action::OlderSegment);
        state.status = Some("loading The Next Pull…".to_string());
        assert!(wowdps_proto::is_loading_status(
            state.status.as_deref().unwrap()
        ));
        assert!(
            seen.players(&state).is_empty(),
            "loading: nobody's list yet"
        );
        state.status = None;
        assert!(!seen.players(&state).is_empty(), "loaded: its own rows");
    }

    /// The acceptance the header was built for, over a 25-player raid at
    /// the prototype's wide frame in the window's own fonts, its raid
    /// timeline in hand: the first row starts no more than 290 px down —
    /// the ribbon's 86 px (R25) included, where the prototype's own first
    /// row stands at about 287 px — and 18 rows show without a scroll.
    #[test]
    fn the_chrome_leaves_a_raid_its_rows() {
        let (mut gui, _peer) = tk::gui_over(tk::raided(25));
        gui.cfg.hide_realms = true;
        let size = iced::Size::new(1440.0, 900.0);
        let mut ui = simulator_as(crate::window::settings(), size, crate::view::view(&gui));
        let list = ui
            .find(crate::view::meter_list_id())
            .expect("the meter's rows")
            .bounds();
        assert!(list.y <= 290.0, "the first row starts {} px down", list.y);
        let rows = (list.height / theme::pitch::ROW).floor();
        assert!(rows >= 18.0, "{rows} rows show ({} px)", list.height);
        // The eighteenth row's name is whole inside the list's view.
        let eighteenth = ui.find("Raider17").expect("row 18").bounds();
        assert!(
            eighteenth.y + eighteenth.height <= list.y + list.height,
            "{eighteenth:?} in {list:?}"
        );
        // The total pins under the list, flush with the window's bottom.
        let total = ui.find(crate::view::meter_total_id()).unwrap().bounds();
        assert!((total.y - (list.y + list.height)).abs() < 1.0);
        assert!((total.y + total.height - size.height).abs() < 1.0);
        // The owner's chip is on the header, in their role's place.
        let head = Head::of(&gui, true);
        let you = head.stats.and_then(|s| s.you).expect("the owner's chip");
        assert_eq!(you.name, "Raider16");
        assert!(you.words.ends_with(" dps"), "{}", you.words);
        assert!(ui.find("Raider16").is_ok());
    }

    /// Every view lays its tabs and rows out where every other does: the
    /// stat line is as tall as the chip whether the chip is on it or not
    /// (Enemies wears none).
    #[test]
    fn the_stat_line_keeps_its_height_without_the_chip() {
        let (mut gui, _peer) = tk::gui_over(tk::raid(25));
        gui.owner_guid = Some("Player-1-16".to_string());
        let size = iced::Size::new(1440.0, 900.0);
        let top = |gui: &Gui| {
            let mut ui = simulator_as(crate::window::settings(), size, crate::view::view(gui));
            ui.find(crate::view::meter_list_id()).unwrap().bounds().y
        };
        let with_chip = top(&gui);
        assert!(Head::of(&gui, true).stats.unwrap().you.is_some());
        gui.state.view = View::EnemyTaken;
        assert!(Head::of(&gui, true).stats.unwrap().you.is_none());
        let without = top(&gui);
        assert!(
            (without - with_chip).abs() < 0.5,
            "{with_chip} vs {without}"
        );
    }

    /// No fight, no figures: the line says it is waiting, as a status in
    /// the quiet ink rather than a title a fight could be called, and
    /// wears no stat line.
    #[test]
    fn waiting_for_combat_sums_nothing() {
        let mut state = ClientState::new();
        state.screen = wowdps_model::Screen::Meter;
        let (gui, _peer) = tk::gui_over(state);
        let head = Head::of(&gui, true);
        assert_eq!(head.title, None, "no fight, no title");
        assert_eq!(head.stats, None);
        let mut ui = simulator_as(
            crate::window::settings(),
            iced::Size::new(1440.0, 200.0),
            view(&gui, true),
        );
        let words = ui.find(WAITING).expect("the status").bounds();
        assert!(
            words.height < size::ENCOUNTER,
            "set as the meta is, not as a title: {words:?}"
        );
    }

    /// A short list keeps its total right under its last row, not at the
    /// window's bottom edge 600 px below it.
    #[test]
    fn a_short_list_s_total_follows_its_last_row() {
        let (state, _mock) = tk::kill();
        let (gui, _peer) = tk::gui_over(state);
        let size = iced::Size::new(1440.0, 900.0);
        let mut ui = simulator_as(crate::window::settings(), size, crate::view::view(&gui));
        let list = ui.find(crate::view::meter_list_id()).unwrap().bounds();
        let total = ui.find(crate::view::meter_total_id()).unwrap().bounds();
        let rows = gui.state.rows().len() as f32;
        assert!(
            (list.height - rows * theme::pitch::ROW).abs() < 1.0,
            "the list is as tall as its rows: {list:?}"
        );
        assert!((total.y - (list.y + list.height)).abs() < 1.0, "{total:?}");
        assert!(total.y + total.height < size.height - 300.0);
    }

    /// A narrow window insets the header less (`.fhead{padding:10px 12px
    /// 8px}`), sets the title smaller, keeps the meta whole — the title is
    /// what ends in "…" — and wraps the stat line rather than clipping the
    /// chip; a tile keeps the chip right after the figures; a wide window
    /// puts it at the line's end.
    #[test]
    fn a_narrow_header_wraps_its_stat_line() {
        let (mut gui, _peer) = tk::gui_over(tk::raid(25));
        gui.owner_guid = Some("Player-1-16".to_string());
        // The header's own width in a window `window` wide.
        let at = |window: f32| {
            let mut ui = simulator_as(
                crate::window::settings(),
                iced::Size::new(window, 400.0),
                view(&gui, true),
            );
            let chip = ui.find("Raider16-Realm-US").expect("the chip").bounds();
            let pair = ui.find("Raid dps").unwrap().bounds();
            let value = ui.find("1,900,000,000").unwrap().bounds();
            let title = ui.find("The Coiled Altar").unwrap().bounds();
            let sized = ui.find("Heroic, 25 players").is_ok();
            let _ = ui.snapshot(&Theme::TokyoNight).unwrap();
            (chip, pair, value, title, sized)
        };
        let (chip, pair, _, title, sized) = at(1440.0);
        assert!(chip.y < pair.y + pair.height, "one line");
        assert!(chip.x > 1000.0, "the chip at the line's end: {chip:?}");
        assert!(sized);
        assert!((title.x - PAD.left).abs() < 0.5, "{title:?}");
        assert!((pair.x - PAD.left).abs() < 0.5, "{pair:?}");
        let wide_title = title.height;
        // A tile: the chip's name 22 px after the last figure, past its
        // disc, the chip's inset and the gap before the name.
        let (chip, pair, value, _, sized) = at(960.0);
        assert!(chip.y < pair.y + pair.height, "one line");
        let after = chip.x - (value.x + value.width);
        assert!(
            (after - (STATS_GAP + 3.0 + DISC + 8.0)).abs() < 2.0,
            "the chip right after the figures: {after}"
        );
        assert!(sized);
        let (chip, pair, _, title, sized) = at(460.0);
        assert!(chip.x + chip.width <= 460.0, "{chip:?}");
        assert!(
            chip.y > pair.y + pair.height,
            "the chip wrapped under the pairs"
        );
        assert!(title.height < wide_title, "a narrow title is set smaller");
        assert!(sized, "a narrow meta keeps the group's size");
        assert!((title.x - PAD_NARROW.left).abs() < 0.5, "{title:?}");
        assert!((pair.x - PAD_NARROW.left).abs() < 0.5, "{pair:?}");
        assert!(
            (title.y - PAD_NARROW.top).abs() < 3.0,
            "the narrow inset on top: {title:?}"
        );
    }

    /// The title line is as tall as its step buttons (`.ftitle h2{line-
    /// height:1.1}`), and the chip is the prototype's 26 px pill.
    #[test]
    fn the_title_line_and_the_chip_keep_the_prototype_s_heights() {
        let (mut gui, _peer) = tk::gui_over(tk::raid(25));
        gui.owner_guid = Some("Player-1-16".to_string());
        let mut ui = simulator_as(
            crate::window::settings(),
            iced::Size::new(1440.0, 200.0),
            view(&gui, true),
        );
        let title = ui.find("The Coiled Altar").unwrap().bounds();
        assert!(title.height <= pitch::ICON_BUTTON, "{title:?}");
        let pair = ui.find("Raid dps").unwrap().bounds();
        // The stat line starts under a 30 px title line and the 7 px gap.
        assert!(
            (pair.y - (PAD.top + pitch::ICON_BUTTON + LINE_GAP)).abs() < 6.0,
            "{pair:?}"
        );
        assert_eq!(CHIP_H, 26.0);
    }

    /// ‹ steps older and › newer, as ← → and `[` `]` do; under the pointer
    /// each lights its tooltip, drawn on the floating surface below it.
    #[test]
    fn the_step_buttons_step_the_way_the_keys_do() {
        let (state, _mock) = tk::kill();
        let (gui, _peer) = tk::gui_over(state);
        let head = Head::of(&gui, false);
        assert!(head.older && head.newer, "the kill has pulls either side");
        let size = iced::Size::new(1440.0, 200.0);
        // The first chevron is ‹, the second ›, at the line's end.
        let y = PAD.top + pitch::ICON_BUTTON / 2.0;
        let older = iced::Point::new(
            size.width - PAD.right - STEPS_W + pitch::ICON_BUTTON / 2.0,
            y,
        );
        let newer = iced::Point::new(size.width - PAD.right - pitch::ICON_BUTTON / 2.0, y);
        // Under the buttons, where a tooltip floats: the band from the
        // title line's foot to 40 px below it, across the steps' end of
        // the line (physical px, at the snapshots' scale of 2).
        let band = |at: iced::Point| {
            let (x0, x1) = ((at.x - 80.0) as u32 * 2, (at.x + 40.0) as u32 * 2);
            let y0 = ((PAD.top + pitch::ICON_BUTTON + 2.0) * 2.0) as u32;
            (x0, y0, x1, y0 + 80)
        };
        for (at, words, sent) in [
            (older, OLDER_TIP, Message::OlderPull),
            (newer, NEWER_TIP, Message::NewerPull),
        ] {
            let tipped = |pointer: Option<iced::Point>| {
                tk::pixels_at(view(&gui, false), size, &Theme::TokyoNight, pointer).count_in(
                    band(at),
                    theme::SURFACE,
                    2,
                )
            };
            assert_eq!(tipped(None), 0, "{words}: no tip without the pointer");
            assert!(tipped(Some(at)) > 100, "{words}: its tip is drawn");
            let mut ui = simulator_as(crate::window::settings(), size, view(&gui, false));
            ui.point_at(at);
            let _ = ui.simulate(iced_test::simulator::click());
            let got: Vec<Message> = ui.into_messages().collect();
            assert_eq!(got.len(), 1, "{words}: {got:?}");
            assert_eq!(
                std::mem::discriminant(&got[0]),
                std::mem::discriminant(&sent),
                "{words}: {got:?}"
            );
        }
        // At the end of the list the step is inert: no message, however
        // it is pressed.
        let (mut state, mut mock) = tk::kill();
        for _ in 0..64 {
            if state.segment_index() + 1 >= state.segment_count() {
                break;
            }
            apply(&mut state, &mut mock, Action::NewerSegment);
        }
        let (gui, _peer) = tk::gui_over(state);
        assert!(!Head::of(&gui, false).newer, "the newest pull");
        let mut ui = simulator_as(crate::window::settings(), size, view(&gui, false));
        ui.point_at(newer);
        let _ = ui.simulate(iced_test::simulator::click());
        assert_eq!(ui.into_messages().count(), 0, "nothing newer to step to");
    }

    /// Between asking for a view and its answer, the rows in hand are the
    /// last view's: the line says nothing rather than "Raid hps 0", the
    /// chip is not drawn from them, and the line keeps its height.
    #[test]
    fn a_view_on_its_way_says_nothing_yet() {
        let (mut gui, _peer) = tk::gui_over(tk::raid(25));
        gui.owner_guid = Some("Player-1-16".to_string());
        let size = iced::Size::new(1440.0, 900.0);
        let top = |gui: &Gui| {
            let mut ui = simulator_as(crate::window::settings(), size, crate::view::view(gui));
            ui.find(crate::view::meter_list_id()).unwrap().bounds().y
        };
        let answered = top(&gui);
        let _ = gui.state.apply(Action::SetView(View::Healing));
        let stats = Head::of(&gui, true).stats.expect("a fight's line");
        assert_eq!(stats, Stats::default(), "no pairs, no chip");
        // The header alone: what it says while the answer is on its way.
        let mut ui = simulator_as(crate::window::settings(), size, view(&gui, true));
        assert!(ui.find("Raid hps").is_err());
        assert!(ui.find("0").is_err(), "no confident zero");
        assert!(
            ui.find("Raider16-Realm-US").is_err(),
            "no chip from Damage's rows"
        );
        assert!((top(&gui) - answered).abs() < 0.5, "nothing jumps");
    }

    /// R25 (v35): the raid's deaths reach the window with the raid timeline,
    /// so the Damage and Enemies lines name them, and the Deaths line their
    /// count, the first one's time and the battle rezzes, as the
    /// prototype's `statLine()` does — without a timeline (a stored pull)
    /// the lines say less rather than borrow another view's count.
    #[test]
    fn the_stat_line_names_the_deaths_the_wire_carries() {
        let state = tk::raided(25);
        let rows = state.rows();
        let raid = state.raid();
        let line = |view| -> Vec<(String, String)> {
            pairs(view, &rows, raid)
                .into_iter()
                .map(|p| (p.label, p.value))
                .collect()
        };
        let labels = |view| -> Vec<String> { line(view).into_iter().map(|p| p.0).collect() };
        assert_eq!(labels(View::Damage), ["Raid dps", "Damage", "Deaths"]);
        assert_eq!(
            labels(View::EnemyTaken),
            ["Raid dtps", "Damage to enemies", "Deaths"]
        );
        assert_eq!(
            line(View::Deaths),
            [
                ("Deaths".to_string(), "6".to_string()),
                ("First".to_string(), "1:10".to_string()),
                ("Battle rezzes".to_string(), "2".to_string()),
            ]
        );
        let bare = |view| -> Vec<String> {
            pairs(view, &rows, None)
                .into_iter()
                .map(|p| p.label)
                .collect()
        };
        assert_eq!(bare(View::Damage), ["Raid dps", "Damage"]);
        assert_eq!(bare(View::Deaths), ["Deaths"]);
    }

    /// v35: the chip says when the owner died, from the timeline — and is
    /// the owner's because the daemon marked their row, whatever the
    /// config calls them.
    #[test]
    fn the_chip_says_when_the_owner_died() {
        let rows = vec![row("Ana", 1, None), row("Bo", 1, None)];
        let died = you(View::Deaths, &rows, Some(1), None, false, Some(345_500)).map(|y| y.words);
        assert_eq!(died.as_deref(), Some("died 5:45"));
        let (gui, _peer) = tk::gui_over(tk::raided(25));
        let stats = stats_of(&gui);
        let you = stats.you.expect("the owner's row is marked mine");
        assert_eq!(you.name, "Raider16-Realm-US");
        // On the Deaths view the chip finds the owner's death in the
        // timeline by their row's key, and says when.
        let (gui, _peer) = tk::gui_over(tk::raided_deaths(25));
        let you = stats_of(&gui).you.expect("the owner died");
        assert_eq!(you.name, "Raider16-Realm-US");
        assert_eq!(you.words, "died 5:45");
    }

    /// R13: an arena's other team dies in the timeline too, and is never
    /// counted among the group's deaths; a self-rez is no battle rez.
    #[test]
    fn the_stat_line_counts_the_groups_deaths_alone() {
        let state = tk::raided(25);
        let rows = state.rows();
        let mut raid = state.raid().cloned().expect("a timeline");
        let mut foe = raid.deaths[0].clone();
        foe.guid = "Player-2-X".into();
        foe.enemy = true;
        foe.at_ms = 10_000;
        raid.deaths.insert(0, foe);
        // Raider7's rez (at 4:25) becomes their own: an Ankh.
        let own = raid.deaths[3].guid.clone();
        if let Some(r) = raid.deaths[3].rez.as_mut() {
            r.by = own;
        }
        let line = |view| -> Vec<(String, String)> {
            pairs(view, &rows, Some(&raid))
                .into_iter()
                .map(|p| (p.label, p.value))
                .collect()
        };
        assert_eq!(
            line(View::Deaths),
            [
                ("Deaths".to_string(), "6".to_string()),
                ("First".to_string(), "1:10".to_string()),
                ("Battle rezzes".to_string(), "1".to_string()),
            ]
        );
        assert!(line(View::Damage).contains(&("Deaths".to_string(), "6".to_string())));
    }

    /// A stored kill wears the Kill badge: the card's outcome reaches the
    /// header through the stored pull's own `ClientState` (the reads
    /// themselves are gui-logic's `history` tests).
    #[test]
    fn a_stored_kill_reads_as_a_kill() {
        use wowdps_daemon::mock::MockDaemon;
        use wowdps_proto::DaemonMsg;
        use wowdps_proto::history::FightKind;

        let mut mock = MockDaemon::fixture().with_history();
        let card = mock
            .history()
            .cards()
            .iter()
            .find(|c| c.kind == FightKind::Encounter && c.success == Some(true))
            .cloned()
            .expect("a stored kill");
        let mut next = 10;
        let (mut s, mut asked) = crate::history::Stored::open(
            card.id.clone(),
            Some(card),
            View::Damage,
            None,
            None,
            &mut next,
        );
        for _ in 0..4 {
            let mut then = Vec::new();
            for m in std::mem::take(&mut asked) {
                for reply in mock.handle(m) {
                    if let DaemonMsg::Fight { req_id, fight } = reply {
                        then.extend(s.absorb(req_id, fight, &mut next));
                    }
                }
            }
            asked = then;
            if asked.is_empty() {
                break;
            }
        }
        let word = badge(Verdict::of(&s.state)).map(|b| b.word);
        assert_eq!(word.as_deref(), Some("Kill"), "the card's outcome");
    }
}
