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
//! Waiting on the Wire step, which carries the raid's marks (deaths, rezzes)
//! and `Row.mine` to the window — each a follow-up that step owns, pinned
//! by an ignored test here (`the_stat_line_names_the_deaths_once_the_wire_
//! carries_them`):
//! - the Deaths pair on the Damage and Enemies lines ("Deaths 6");
//! - Deaths' "First m:ss" and "Battle rezzes n";
//! - the time in the chip's "died m:ss" (today "died");
//! - a live pull's best health for "Wipe at N%" (the badge words it from
//!   [`Verdict::wipe_pct`], which nothing on the live meter fills yet).
//!
//! A meter snapshot carries its own view's rows only, and a count of deaths
//! that appeared once the reader had visited the Deaths tab would make one
//! view's line change with where the reader had been — so until then the
//! lines say less rather than something that moves.
//!
//! Window-only. The overlay keeps its strip and its own header, and
//! nothing here is a renderer it shares.

use iced::widget::{Row as Line, Space, button, column, container, row, text, tooltip};
use iced::{Border, Color, Element, Font, Length, Theme};

use wowdps_model::fmt::{commas, duration, key_tier};
use wowdps_model::{
    Class, Encounter, Role, Row, SegmentId, SegmentKind, Spec, View, difficulty_name,
};
use wowdps_proto::ClientState;

use crate::ellipsis::ellipsis;
use crate::line_icons::LineIcon;
use crate::nav::{self, Badge};
use crate::theme::{self, pitch, size};
use crate::view::{display_name, rate_label, window_view_name};
use crate::window::{Gui, Message};

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
/// A tooltip (`.tip{padding:5px 8px;border-radius:6px}`), and how far under
/// what it names it floats.
const TIP_PAD: [u16; 2] = [5, 8];
const TIP_RADIUS: f32 = 6.0;
const TIP_GAP: f32 = 4.0;
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
/// the top bar: the prototype's `.fhead{padding:13px 18px 9px}`, less the
/// 8 px the window keeps under the bar.
pub(crate) const PAD: iced::Padding = iced::Padding {
    top: 5.0,
    right: 18.0,
    bottom: 9.0,
    left: 18.0,
};
/// The same under 820 px (`.fhead{padding:10px 12px 8px}`), less the 8.
pub(crate) const PAD_NARROW: iced::Padding = iced::Padding {
    top: 2.0,
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

/// The header, as the meter wears it (`stats`: with the stat line) or as a
/// drill and the comparison do — the title line alone, their own figures
/// under it. Laid out at the width the window gives it: a narrow window
/// insets it less, sets the title smaller and lets the stat line wrap.
pub(crate) fn view(state: &Gui, stats: bool) -> Element<'static, Message> {
    let head = Head::of(state, stats);
    container(
        iced::widget::responsive(move |bounds| head.layout(bounds.width)).height(Length::Shrink),
    )
    .width(Length::Fill)
    .into()
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
    /// There is a newer / an older pull to step to.
    pub newer: bool,
    pub older: bool,
    /// The stat line, when this header wears one and there is a fight to
    /// sum: "waiting for combat…" is no place for a confident zero.
    pub stats: Option<Stats>,
}

/// What the title line says before there is a fight.
pub(crate) const WAITING: &str = "waiting for combat…";

/// The stat line: the view's figures, and the owner's chip. Both empty
/// while the view's answer is on its way ([`Stats::pending`]).
#[derive(Debug, Clone, PartialEq, Default)]
pub(crate) struct Stats {
    pub pairs: Vec<Pair>,
    pub you: Option<You>,
}

/// One label and its value.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Pair {
    pub label: String,
    pub value: String,
}

/// The "you" chip: the owner as the chart knows them, and what the view
/// says of them.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct You {
    pub name: String,
    pub class: Option<Class>,
    pub spec: Option<Spec>,
    /// "17th of 19 dps", "149,258 hps", "3 interrupts", "died", "survived".
    pub words: String,
    /// The figure after the words, in secondary ink: the rate beside a
    /// place.
    pub figure: Option<String>,
    /// The owner has a row on this view for a press to select. On Deaths
    /// ("survived") and a count view ("no interrupts") the chip speaks from
    /// what another view said (`Seen`), and there is no row to go to: no
    /// press, no tooltip, no hand.
    pub selectable: bool,
}

impl Head {
    pub(crate) fn of(state: &Gui, stats: bool) -> Self {
        let app = &state.state;
        let (at, count) = (app.segment_index(), app.segment_count());
        let name = app.segment_name();
        Head {
            duration: name
                .as_ref()
                .map_or_else(String::new, |_| duration(app.duration_ms())),
            stats: (stats && name.is_some()).then(|| Stats::of(state)),
            title: name,
            meta: meta(app.segment_encounter()),
            badge: outcome(app),
            // The game buffers its log writes; say how far behind the file
            // is rather than let a live fight look frozen.
            stale: app
                .is_live()
                .then(|| state.stale_secs())
                .flatten()
                .map(|secs| format!("no events for {secs}s")),
            newer: at + 1 < count,
            older: count > 0 && at > 0,
        }
    }

    fn layout(&self, width: f32) -> Element<'static, Message> {
        let fit = Fit::of(width);
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
            tip(
                nav::icon_button(
                    LineIcon::ChevronLeft,
                    self.older.then_some(Message::OlderPull),
                    None,
                ),
                OLDER_TIP,
            ),
            tip(
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
            None => container(quiet(WAITING, meta_px))
                .height(Length::Fixed(pitch::ICON_BUTTON))
                .align_y(iced::Alignment::Center)
                .into(),
        };
        row![title, meta, Space::new().width(Length::Fill), steps]
            .spacing(TITLE_GAP)
            .align_y(iced::Alignment::Center)
            .into()
    }
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

/// The step buttons' tooltips: what they do, and the key that does it.
pub(crate) const OLDER_TIP: &str = "Older pull ( [ )";
pub(crate) const NEWER_TIP: &str = "Newer pull ( ] )";

/// `content` with its tooltip (`.tip`): 13 px on the floating surface,
/// under it.
fn tip(
    content: impl Into<Element<'static, Message>>,
    words: &'static str,
) -> Element<'static, Message> {
    tooltip(
        content,
        container(text(words).size(size::MICRO).color(theme::INK))
            .padding(TIP_PAD)
            .style(|_: &Theme| nav::floating_style(TIP_RADIUS)),
        tooltip::Position::Bottom,
    )
    .gap(TIP_GAP)
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
    tip(chip, "Select your row")
}

// ---- the words ---------------------------------------------------------------

/// "Heroic, 25 players": the encounter's difficulty as the game names it
/// and how many were in it. Empty off a boss pull — trash, a visit's Σ, an
/// arena — which has no ENCOUNTER_START to say it.
pub(crate) fn meta(encounter: Option<Encounter>) -> String {
    let Some(e) = encounter else {
        return String::new();
    };
    let size = (e.group_size > 0).then(|| nav::plural(e.group_size as usize, "player"));
    match (difficulty_name(e.difficulty), size) {
        (Some(d), Some(s)) => format!("{d}, {s}"),
        (Some(d), None) => d.to_string(),
        (None, Some(s)) => s,
        (None, None) => String::new(),
    }
}

/// What an outcome is worded from: the watched segment's verdict, and what
/// kind of segment it is.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub(crate) struct Verdict {
    pub live: bool,
    pub kind: Option<SegmentKind>,
    pub success: Option<bool>,
    /// R13: an arena match, won or lost.
    pub arena: bool,
    /// R10: a keyed visit's (par, +2, +3) timers.
    pub pars_ms: Option<(i64, i64, i64)>,
    pub duration_ms: i64,
    /// R16: how close a wipe came, when something said. The live meter's
    /// snapshot does not carry it yet — a stored card does — so nothing
    /// fills it here until the Wire step brings a pull's best health.
    pub wipe_pct: Option<u16>,
}

impl Verdict {
    fn of(app: &ClientState) -> Self {
        Verdict {
            live: app.is_live(),
            kind: app.segment_kind(),
            success: app.segment_success(),
            arena: app.segment_arena(),
            pars_ms: app.segment_pars_ms(),
            duration_ms: app.duration_ms(),
            wipe_pct: None,
        }
    }
}

/// The watched segment's outcome badge ([`badge`]).
pub(crate) fn outcome(app: &ClientState) -> Option<Badge> {
    badge(Verdict::of(app))
}

/// The outcome badge (`.badge`): Live with its red dot; Kill, Win and
/// "Timed +2" (the key's upgrade, R10) in green; "Wipe at 56%", Loss and
/// "Over time" in red. `None` when there is nothing to say: trash, a
/// raid visit's Σ, a pull the log cut off before its end.
pub(crate) fn badge(v: Verdict) -> Option<Badge> {
    if v.live {
        return Some(Badge::live());
    }
    let good = v.success?;
    let word = match (v.kind, good) {
        (Some(SegmentKind::Overall), true) => match v.pars_ms {
            Some(pars) => format!("Timed +{}", key_tier(v.duration_ms, pars)),
            None => "Timed".to_string(),
        },
        (Some(SegmentKind::Overall), false) => "Over time".to_string(),
        (_, true) if v.arena => "Win".to_string(),
        (_, false) if v.arena => "Loss".to_string(),
        (_, true) => "Kill".to_string(),
        (_, false) => match v.wipe_pct {
            Some(pct) => format!("Wipe at {pct}%"),
            None => "Wipe".to_string(),
        },
    };
    Some(Badge::new(
        word,
        if good { theme::GOOD } else { theme::BAD },
    ))
}

impl Stats {
    /// The line for the view on screen — or, while its answer is on its
    /// way (asked for, not in; a stored pull still loading), nothing: the
    /// rows in hand are another view's or a placeholder's, and a "Raid hps
    /// 0" or a "survived" drawn from them would be the confident false
    /// figure the line never shows. The line keeps its height.
    fn of(state: &Gui) -> Self {
        let app = &state.state;
        if Self::pending(app) {
            return Stats::default();
        }
        let rows = app.rows();
        let seen = state.seen.of(app).and_then(|s| s.owner.as_ref());
        let owner = state.owner_in(&rows);
        let you = you(app.view, &rows, owner, seen, app.is_live()).map(|mut you| {
            if !state.cfg.hide_realms {
                return you;
            }
            you.name = display_name(&you.name).to_string();
            you
        });
        Stats {
            pairs: pairs(app.view, &rows),
            you,
        }
    }

    /// The view's answer is not in yet.
    fn pending(app: &ClientState) -> bool {
        let loading = app
            .status
            .as_deref()
            .is_some_and(wowdps_proto::is_loading_status);
        !app.view_answered() || loading
    }
}

/// The stat line's figures for `view`, folded from its rows — OUR side's:
/// an arena's enemy team (R13) is on the chart, not in the fold. The
/// prototype also names the deaths on Damage and Enemies, and the first
/// one's time and the battle rezzes on Deaths: those are the Wire step's
/// (the module's follow-ups), since a snapshot carries its own view's
/// rows alone.
pub(crate) fn pairs(view: View, rows: &[Row]) -> Vec<Pair> {
    let ours: Vec<&Row> = rows.iter().filter(|r| !r.enemy).collect();
    let total: u64 = ours.iter().map(|r| r.amount).sum();
    let extra: u64 = ours.iter().map(|r| r.extra).sum();
    let rate: f64 = ours.iter().map(|r| r.per_sec).sum();
    let pair = |label: &str, value: String| Pair {
        label: label.to_string(),
        value,
    };
    let raid = pair(
        &format!("Raid {}", rate_label(view)),
        commas(rate.round() as u64),
    );
    match view {
        View::Damage => vec![raid, pair("Damage", commas(total))],
        View::Healing => {
            let mut line = vec![raid, pair("Healing", commas(total))];
            let fold = Row {
                amount: total,
                extra,
                ..Row::default()
            };
            if total + extra > 0 {
                line.push(pair(
                    "Overheal",
                    format!("{:.1}%", crate::table::overheal_pct(&fold)),
                ));
            }
            line
        }
        View::Taken => vec![
            raid,
            pair("Taken", commas(total)),
            pair("Absorbed", commas(extra)),
        ],
        // "Raid dtps": what the enemies took, a second — the prototype's
        // words beside "Damage to enemies".
        View::EnemyTaken => vec![raid, pair("Damage to enemies", commas(total))],
        View::Deaths => vec![pair("Deaths", commas(total))],
        View::Interrupts | View::CrowdControl | View::Dispels => vec![
            pair(window_view_name(view), commas(total)),
            pair("Players", ours.len().to_string()),
        ],
    }
}

/// What the chip says of the owner on `view`, their row being `owner` of
/// `rows`: on Damage their place among their own ROLE — a healer against
/// healers, what the history store grades by — and their rate after it;
/// on Healing and Taken the rate; on a count view the count and its noun
/// ("3 interrupts"), or "no interrupts" when the window saw them in this
/// fight (`seen`, their row on another view) and they have no row here;
/// on Deaths "died", or — seen, and not among the dead — "survived" once
/// the fight is over and "alive" while it is `live`. When they died waits
/// for the raid marks on the wire (the prototype's "died m:ss"). `None` is
/// no chip: the owner is not known to be in this fight, or — on the
/// Enemies view — the rows are the enemies.
pub(crate) fn you(
    view: View,
    rows: &[Row],
    owner: Option<usize>,
    seen: Option<&Row>,
    live: bool,
) -> Option<You> {
    if view == View::EnemyTaken {
        return None;
    }
    let me = owner.and_then(|i| rows.get(i));
    // Selectable when it names a row on this chart; a chip made from what
    // another view said has none to go to.
    let chip = |r: &Row, words: String, figure: Option<String>| You {
        name: r.label.clone(),
        class: r.class,
        spec: r.spec,
        words,
        figure,
        selectable: me.is_some(),
    };
    match (view, me) {
        (View::Deaths, Some(me)) => Some(chip(
            me,
            match me.amount {
                0 | 1 => "died".to_string(),
                n => format!("died {n} times"),
            },
            None,
        )),
        (View::Deaths, None) => seen.map(|me| {
            let words = if live { "alive" } else { "survived" };
            chip(me, words.to_string(), None)
        }),
        (View::Interrupts | View::CrowdControl | View::Dispels, None) => {
            seen.map(|me| chip(me, format!("no {}", count_noun(view, 0)), None))
        }
        (_, None) => None,
        (View::Damage, Some(me)) => {
            let role = me.spec.map(Spec::role);
            // Everyone on our side who plays the owner's role; with no
            // spec known, everyone on our side.
            let peers: Vec<&Row> = rows
                .iter()
                .filter(|r| !r.enemy && (role.is_none() || r.spec.map(Spec::role) == role))
                .collect();
            let place = peers.iter().filter(|r| r.amount > me.amount).count() + 1;
            let words = match role_noun(role, peers.len()) {
                Some(noun) => format!("{} of {} {noun}", ordinal(place), peers.len()),
                None => format!("{} of {}", ordinal(place), peers.len()),
            };
            Some(chip(me, words, Some(commas(me.per_sec.round() as u64))))
        }
        (View::Healing | View::Taken, Some(me)) => Some(chip(
            me,
            format!("{} {}", commas(me.per_sec.round() as u64), rate_label(view)),
            None,
        )),
        (_, Some(me)) => Some(chip(
            me,
            format!("{} {}", commas(me.amount), count_noun(view, me.amount)),
            None,
        )),
    }
}

/// What a count view counts, agreeing with `n`: "interrupt(s)",
/// "dispel(s)" — and "crowd control", which has no plural.
fn count_noun(view: View, n: u64) -> &'static str {
    match (view, n) {
        (View::Interrupts, 1) => "interrupt",
        (View::Interrupts, _) => "interrupts",
        (View::Dispels, 1) => "dispel",
        (View::Dispels, _) => "dispels",
        _ => "crowd control",
    }
}

/// A role as the chip counts `n` of its members: "dps", "healers",
/// "tanks" — "healer" and "tank" when there is one.
fn role_noun(role: Option<Role>, n: usize) -> Option<&'static str> {
    role.map(|r| match (r, n) {
        (Role::Dps, _) => "dps",
        (Role::Healer, 1) => "healer",
        (Role::Healer, _) => "healers",
        (Role::Tank, 1) => "tank",
        (Role::Tank, _) => "tanks",
    })
}

/// 1st, 2nd, 3rd, 4th … 11th, 12th, 13th … 21st, 22nd.
pub(crate) fn ordinal(n: usize) -> String {
    let suffix = match (n % 10, n % 100) {
        (_, 11..=13) => "th",
        (1, _) => "st",
        (2, _) => "nd",
        (3, _) => "rd",
        _ => "th",
    };
    format!("{n}{suffix}")
}

/// A row label names the owner by one of their configured names: the
/// "Name-Realm" whole, or a bare "Name" matching its name half — the way
/// the daemon reads `history_characters`. Case aside.
pub(crate) fn is_named(label: &str, name: &str) -> bool {
    let (label, name) = (label.to_lowercase(), name.to_lowercase());
    label == name
        || label
            .strip_prefix(&name)
            .is_some_and(|rest| rest.starts_with('-'))
}

// ---- what other views said -------------------------------------------------

/// What the window has seen of the watched fight on views other than the
/// one on screen: the owner's row, as it last stood — what lets the Deaths
/// view say "survived" and a count view "no interrupts" rather than
/// nothing, since a snapshot carries its own view's rows only. Held for one
/// fight, begun again with the next; never taken from a loading
/// placeholder.
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct Seen {
    fight: Option<SegmentId>,
    /// The owner's row as it last stood, on any view.
    owner: Option<Row>,
}

impl Seen {
    /// Take in what is on screen now; `owner` is the owner's row there.
    pub(crate) fn observe(&mut self, app: &ClientState, owner: Option<&Row>) {
        let Some(fight) = watched(app) else {
            return;
        };
        if self.fight != Some(fight) {
            *self = Seen {
                fight: Some(fight),
                ..Seen::default()
            };
        }
        let loading = app
            .status
            .as_deref()
            .is_some_and(wowdps_proto::is_loading_status);
        if loading {
            return;
        }
        if let Some(me) = owner {
            self.owner = Some(me.clone());
        }
    }

    /// What was seen, when it is of the fight on screen.
    fn of(&self, app: &ClientState) -> Option<&Seen> {
        (self.fight.is_some() && watched(app) == self.fight).then_some(self)
    }
}

/// The fight on screen, by the daemon's id for it: `None` before a
/// snapshot describes one.
fn watched(app: &ClientState) -> Option<SegmentId> {
    app.segment_name()?;
    app.entries().get(app.segment_index()).map(|e| e.id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::window::testkit::{self as tk, apply, simulator, simulator_as};
    use wowdps_model::Action;

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
            pairs(view, &rows)
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
        let players = pairs(View::Interrupts, &arena);
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
    /// trip to the Deaths tab — its reply in or not yet — adds no Deaths
    /// pair to it, and never a false "Deaths 0".
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
        assert!(!before.iter().any(|l| l == "Deaths"), "{before:?}");
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
            you(view, &rows, owner, seen, false).map(|y| (y.words, y.figure))
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
            you(View::Deaths, &deaths, Some(2), None, false).map(|y| y.words),
            Some("died".into())
        );
        deaths[2].amount = 2;
        assert_eq!(
            you(View::Deaths, &deaths, Some(2), None, false).map(|y| y.words),
            Some("died 2 times".into())
        );
        assert_eq!(
            words(View::Deaths, None, Some(&rows[2])),
            Some(("survived".into(), None))
        );
        assert_eq!(
            you(View::Deaths, &rows, None, Some(&rows[2]), true).map(|y| y.words),
            Some("alive".into()),
            "a pull still going has no survivors yet"
        );
        assert_eq!(words(View::Deaths, None, None), None, "never seen here");
        // The chip carries the owner's spec and name.
        let chip = you(View::Damage, &rows, Some(2), None, false).unwrap();
        assert_eq!(chip.spec, Some(Spec::Demonology));
        assert_eq!(chip.name, "Cid-Realm-US");
        // A press selects a row the chart holds; a chip that speaks from
        // another view has none to go to, and takes no press.
        assert!(chip.selectable);
        let survived = you(View::Deaths, &rows, None, Some(&rows[2]), false).unwrap();
        let none = you(View::Interrupts, &rows, None, Some(&rows[2]), false).unwrap();
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
        assert_eq!(facts.owner.as_ref(), Some(&owner));
        apply(&mut state, &mut mock, Action::SetView(View::Deaths));
        seen.observe(&state, None);
        let facts = seen.of(&state).unwrap();
        assert_eq!(facts.owner.as_ref(), Some(&owner), "still seen");
        // Another fight: what was seen was of the last one.
        apply(&mut state, &mut mock, Action::OlderSegment);
        assert!(seen.of(&state).is_none());
        seen.observe(&state, None);
        let facts = seen.of(&state).unwrap();
        assert_eq!(facts.owner, None);
    }

    /// The acceptance the header was built for, over a 25-player raid at
    /// the prototype's wide frame in the window's own fonts: the first row
    /// starts no more than 230 px down, and 19 rows show without a scroll.
    #[test]
    fn the_chrome_leaves_a_raid_its_rows() {
        let (mut gui, _peer) = tk::gui_over(tk::raid(25));
        gui.cfg.hide_realms = true;
        gui.owner_guid = Some("Player-1-16".to_string());
        let size = iced::Size::new(1440.0, 900.0);
        let mut ui = simulator_as(crate::window::settings(), size, crate::view::view(&gui));
        let list = ui
            .find(crate::view::meter_list_id())
            .expect("the meter's rows")
            .bounds();
        assert!(list.y <= 230.0, "the first row starts {} px down", list.y);
        let rows = (list.height / theme::pitch::ROW).floor();
        assert!(rows >= 19.0, "{rows} rows show ({} px)", list.height);
        // The nineteenth row's name is whole inside the list's view.
        let nineteenth = ui.find("Raider18").expect("row 19").bounds();
        assert!(
            nineteenth.y + nineteenth.height <= list.y + list.height,
            "{nineteenth:?} in {list:?}"
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

    /// The Wire step's follow-ups, held here so they cannot be forgotten:
    /// once the raid's death marks reach the window, the Damage and Enemies
    /// lines name the deaths, the Deaths line their first time and the
    /// battle rezzes, as the prototype's `statLine()` does. Ignored until
    /// then — `pairs` has nothing to count them from.
    #[test]
    #[ignore = "the Wire step: raid death marks on the wire (see the module docs)"]
    fn the_stat_line_names_the_deaths_once_the_wire_carries_them() {
        let rows = tk::raid(25).rows();
        let labels =
            |view| -> Vec<String> { pairs(view, &rows).into_iter().map(|p| p.label).collect() };
        assert_eq!(labels(View::Damage), ["Raid dps", "Damage", "Deaths"]);
        assert_eq!(
            labels(View::EnemyTaken),
            ["Raid dtps", "Damage to enemies", "Deaths"]
        );
        assert_eq!(labels(View::Deaths), ["Deaths", "First", "Battle rezzes"]);
    }
}
