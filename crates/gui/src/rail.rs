//! The pull rail (the prototype's `.rail`): ONE list of pulls, tonight's
//! log and every stored night, where the window used to keep a fight list
//! for the log and a History screen for the store — two lists with two
//! shapes, and four controls that moved between pulls, each over a
//! different scope.
//!
//! Nights by their local date ("Tonight", "Saturday, Sep 26"), the visits
//! in each (an instance and its difficulty, your keys, a delve) wearing a
//! dot in the colour of the character who played them, and the pulls newest
//! first: an outcome glyph (✓ kill or timed, ✕ wipe or over time, a dash for
//! trash, Σ for a whole visit, a red dot while live), the name, and at the
//! right a wipe's best %, a key's +N or "over", a dot where one visit holds
//! several characters, and the duration. Tonight comes from the daemon's
//! segment list; the earlier nights from the history store's pages
//! (`history::Earlier`). A stored card that is ALSO a segment of the tailed
//! log — its id is the log's id and the row's start — is listed once, as
//! the log's, and lends that row what only the store knows (a wipe's best
//! health, whose pull it was).
//!
//! Above 1180 px the rail stands at the window's left, 236 px wide; at 1180
//! and under it is a 280 px drawer over a scrim, opened from the fight
//! header's list button (or `H`) and closed by the scrim, Esc, Enter or a
//! pick. `[` and `]` walk it, stored nights included — the drawer left open
//! on the row they reach — and while it is open j, k and the arrows walk a
//! highlight over its rows for Enter to open. Window-only.

use std::collections::{HashMap, HashSet};

use iced::advanced::widget::operation::{self, Operation, Outcome};
use iced::widget::canvas::{self, Canvas, Path, Stroke};
use iced::widget::{
    Space, button, column, container, mouse_area, opaque, row, scrollable, stack, text,
};
use iced::{Color, Element, Length, Point, Rectangle, Renderer, Size, Theme, mouse};

use wowdps_model::fmt::{duration, key_tier};
use wowdps_model::{Class, ListRow, SegmentId, SegmentKind, difficulty_name};
use wowdps_proto::ListEntry;
use wowdps_proto::history::{FightCard, FightKind, fight_id};

use crate::ellipsis::ellipsis;
use crate::line_icons::{LineIcon, line_icon};
use crate::nav;
use crate::theme::{self, size};
use crate::window::Message;

/// The rail beside the stage (`.body{grid-template-columns:236px …}`) and
/// the drawer it becomes (`.rail{width:280px}` under 1180 px).
pub(crate) const RAIL_W: f32 = 236.0;
pub(crate) const DRAWER_W: f32 = 280.0;
/// A pull's row (`.pull{padding:4px 12px 4px 14px;line-height:20px}`), and
/// its lead glyph's box and the glyph in it (`.oc`, `.oc svg`).
const PULL_H: f32 = 28.0;
const PULL_PAD: iced::Padding = iced::Padding {
    top: 4.0,
    right: 12.0,
    bottom: 4.0,
    left: 14.0,
};
const PULL_GAP: f32 = 8.0;
const MARK_BOX: f32 = 16.0;
const MARK_ICON: f32 = 14.0;
/// A pull's name (14.5 px; trash 13.5 in the faint ink), what sits at its
/// right (`.rt{gap:7px;font-size:13.5px}`), a character's dot (7 px).
const NAME_PX: f32 = 14.5;
const TRASH_PX: f32 = 13.5;
const RIGHT_PX: f32 = 13.5;
const RIGHT_GAP: f32 = 7.0;
const DOT: f32 = 7.0;
/// The current pull's edge (`box-shadow:inset 2px 0 0 var(--accent)`),
/// and the ring on the row the drawer's keys are on.
const EDGE_W: f32 = 2.0;
const CURSOR_RING: f32 = 1.0;
const CURSOR_RADIUS: f32 = 3.0;
/// The head (`.rail-head{gap:6px;padding:10px 8px 8px 14px}`, `h2{15px
/// 600}`) and the trash toggle (`.toggle{13px;padding:2px 6px;
/// border-radius:4px}`), pressed in the raise and the second ink
/// (`.toggle[aria-pressed=true]`) — its words and its width as they were.
const HEAD_PAD: iced::Padding = iced::Padding {
    top: 10.0,
    right: 8.0,
    bottom: 8.0,
    left: 14.0,
};
const HEAD_GAP: f32 = 6.0;
const HEAD_PX: f32 = 15.0;
const TOGGLE_PX: f32 = 13.0;
const TOGGLE_PAD: [f32; 2] = [2.0, 6.0];
const TOGGLE_RADIUS: f32 = 4.0;
/// A night (`.night{padding-top:12px}`, `h3{13px 600;padding:0 14px 2px}`)
/// and a visit (`.visit{padding:6px 14px 3px;gap:7px;font-size:13px}`).
const NIGHT_PAD: iced::Padding = iced::Padding {
    top: 12.0,
    right: 14.0,
    bottom: 2.0,
    left: 14.0,
};
const LABEL_PX: f32 = 13.0;
const VISIT_PAD: iced::Padding = iced::Padding {
    top: 6.0,
    right: 14.0,
    bottom: 3.0,
    left: 14.0,
};
const VISIT_GAP: f32 = 7.0;
/// The trash dash (`.oc .dash{width:6px;height:1.5px;border-radius:1px}`).
const DASH_W: f32 = 6.0;
const DASH_H: f32 = 1.5;
const DASH_RADIUS: f32 = 1.0;
/// "Show older nights" (`.rail-more{margin:14px 14px 0;padding:6px 10px;
/// border:1px dashed;border-radius:6px;font-size:13.5px}`) — its dash and
/// gap, near a browser's for a 1 px dashed edge — and the list's foot
/// (`.rail-scroll{padding-bottom:14px}`).
const MORE_MARGIN: f32 = 14.0;
const MORE_PAD: [f32; 2] = [6.0, 10.0];
const MORE_RADIUS: f32 = 6.0;
const MORE_EDGE: f32 = 1.0;
const MORE_DASH: [f32; 2] = [4.0, 3.0];
const FOOT: f32 = 14.0;
/// The list's scrollbar (`::-webkit-scrollbar{width:10px}`, its thumb
/// `border:2px solid transparent;background-clip:padding-box`): a 10 px
/// lane with a 6 px thumb down its middle.
const SCROLL_LANE: f32 = 10.0;
const SCROLL_THUMB: f32 = 6.0;
/// The pin's mark on a row — in the row's 14 px left gutter, 2 px clear of
/// the current row's 2 px edge, in the quiet ink: the one gold at the
/// rail's edge is the current row's — and what it says under the pointer.
pub(crate) const PIN: &str = "★";
const PIN_PX: f32 = 11.0;
const PIN_INSET: f32 = 4.0;
pub(crate) const PIN_TIP: &str = "Pinned: retention keeps it (p)";
/// The drawer's shadow (`box-shadow:20px 0 50px rgba(0,0,0,.5)`).
const DRAWER_SHADOW: iced::Shadow = iced::Shadow {
    color: Color::from_rgba(0.0, 0.0, 0.0, 0.5),
    offset: iced::Vector::new(20.0, 0.0),
    blur_radius: 50.0,
};

/// A night starts at 06:00 local, the cutover the mcp's local buckets
/// default to: a raid that runs past midnight is one night, not two.
const CUTOVER_MS: i64 = 6 * 3_600_000;
const DAY_MS: i64 = 86_400_000;
/// A dungeon's visit that a key on the same map starts within this long of
/// is the key's zone-in (the minutes before CHALLENGE_MODE_START), not a
/// visit of its own: the key's card is the run.
pub(crate) const ZONE_IN_MS: i64 = 10 * 60_000;

/// Is the rail beside the stage, at a window this wide? At 1180 and under
/// it is a drawer (`@container app (max-width: 1180px)`).
pub(crate) fn docked(window: f32) -> bool {
    window > theme::TILE_WINDOW
}

// ---- the model -----------------------------------------------------------------

/// Where a row leads: a segment of the tailed log, by the daemon's id for
/// it, or a stored fight, by the store's.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Pull {
    Log(SegmentId),
    Stored(String),
}

/// A row's lead glyph (`.oc`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Mark {
    /// ✓: a kill, a win, a timed key.
    Good,
    /// ✕: a wipe, a loss, a key over time.
    Bad,
    /// A dash: trash, and anything without a verdict.
    Dash,
    /// Σ: a whole visit.
    Sum,
    /// A red dot: still going.
    Live,
}

/// What a key's row says at its right: the chests it earned, or over.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum KeyWord {
    Plus(u8),
    Over,
}

/// One pull on the rail.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Line {
    pub pull: Pull,
    pub name: String,
    pub mark: Mark,
    pub trash: bool,
    /// A wipe's best boss health, when something saw it.
    pub best_pct: Option<u16>,
    pub key: Option<KeyWord>,
    /// Whose pull it was, as a dot in their class colour — only in a visit
    /// several characters share; one character's visit wears it once.
    pub dot: Option<Who>,
    /// Its card is pinned: retention keeps it.
    pub pinned: bool,
    pub duration_ms: i64,
    /// Its local start, for the order.
    start_ms: i64,
    /// Whose it was: what decides where the dot goes.
    owner: Option<Owner>,
}

/// A character a dot stands for: their class colour, and the name the
/// dot's tip says — colour alone cannot tell two alts of a class apart.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Who {
    pub color: Color,
    pub name: String,
}

/// Whose a pull was: their guid (the tailed log's logger, when no card
/// names them, is ""), their name and their class.
#[derive(Debug, Clone, PartialEq)]
struct Owner {
    guid: String,
    name: String,
    class: Option<Class>,
}

/// A visit: an instance and its difficulty, your keys, a delve.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Visit {
    pub title: String,
    /// The one character who played every pull in it, as their dot.
    pub dot: Option<Who>,
    /// Newest first; a whole visit's Σ last.
    pub lines: Vec<Line>,
}

/// A night: its visits, newest first.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Night {
    /// Days since the epoch, of the local date the night began on.
    pub day: i64,
    pub label: String,
    /// It is tonight: the clock's night, or one with a pull still going.
    pub tonight: bool,
    pub visits: Vec<Visit>,
}

/// The rail, derived: every night the log and the store's pages hold.
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct Rail {
    pub nights: Vec<Night>,
}

/// What the rail is made from.
pub(crate) struct Sources<'a> {
    /// The tailed log's segments, oldest first.
    pub entries: &'a [ListEntry],
    /// The log's identity, which with a row's start names its stored card.
    pub log_id: Option<u64>,
    /// The segment the stage is watching, as its snapshot has it now:
    /// fresher than its list row, which the daemon re-sends only when the
    /// list changes shape (a pull that ended says so here first).
    pub watched: Option<(SegmentId, ListRow)>,
    /// The store's cards in hand, newest first.
    pub cards: &'a [FightCard],
    /// The window's owner's name, when known, and class: whose the log's
    /// pulls are when no card of theirs says.
    pub owner: Option<(Option<String>, Class)>,
    /// The night it is now ([`tonight`]).
    pub tonight: i64,
}

/// The night a local timestamp belongs to: its date, the small hours
/// counted with the evening before.
pub(crate) fn night_of(local_ms: i64) -> i64 {
    (local_ms - CUTOVER_MS).div_euclid(DAY_MS)
}

/// The night it is now, at `now_utc_ms`, in the timezone the log writes
/// (`tz_min` east of UTC; the store's cards carry it). `None` reads UTC.
pub(crate) fn tonight(now_utc_ms: i64, tz_min: Option<i16>) -> i64 {
    night_of(now_utc_ms + i64::from(tz_min.unwrap_or(0)) * 60_000)
}

/// Days since the epoch as a civil date (Howard Hinnant's civil-from-days,
/// the inverse of days-from-civil — `home::parse_ymd`'s milliseconds over
/// [`DAY_MS`]): year, month 1–12, day 1–31.
pub(crate) fn civil(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (yoe + era * 400 + i64::from(m <= 2), m as u32, d as u32)
}

const WEEKDAYS: [&str; 7] = [
    "Sunday",
    "Monday",
    "Tuesday",
    "Wednesday",
    "Thursday",
    "Friday",
    "Saturday",
];
const MONTHS: [&str; 12] = [
    "January",
    "February",
    "March",
    "April",
    "May",
    "June",
    "July",
    "August",
    "September",
    "October",
    "November",
    "December",
];

/// A month (1–12) by its name, "September" — or its first three letters,
/// "Sep", `short`: the one table the rail's headings and Home's dates
/// read.
pub(crate) fn month_name(m: u32, short: bool) -> &'static str {
    let name = MONTHS
        .get((m as usize).saturating_sub(1))
        .copied()
        .unwrap_or_default();
    if short {
        name.get(..3).unwrap_or(name)
    } else {
        name
    }
}

/// The weekday of a night: "Saturday".
pub(crate) fn weekday(day: i64) -> &'static str {
    // 1970-01-01 was a Thursday.
    WEEKDAYS
        .get((day + 4).rem_euclid(7) as usize)
        .copied()
        .unwrap_or_default()
}

/// A night's heading: "Tonight", else its weekday and date — "Saturday,
/// Sep 26" — with the year only when it is not tonight's.
pub(crate) fn night_label(day: i64, tonight: i64) -> String {
    if day == tonight {
        return "Tonight".to_string();
    }
    let (y, m, d) = civil(day);
    let weekday = weekday(day);
    let month = month_name(m, true);
    if y == civil(tonight).0 {
        format!("{weekday}, {month} {d}")
    } else {
        format!("{weekday}, {month} {d}, {y}")
    }
}

/// A night as the fight header names it beside a pull of it: "Mon, Sep 21"
/// — the year only when it is not tonight's.
pub(crate) fn night_short(day: i64, tonight: i64) -> String {
    let (y, m, d) = civil(day);
    let weekday = weekday(day).get(..3).unwrap_or_default();
    let month = month_name(m, true);
    if y == civil(tonight).0 {
        format!("{weekday}, {month} {d}")
    } else {
        format!("{weekday}, {month} {d}, {y}")
    }
}

/// The difficulty ids a raid is run at (the log's own; legacy sizes, the
/// four current ones, timewalking) and a party's (normal, heroic, mythic,
/// a keystone, timewalking); a delve's is 208.
pub(crate) fn is_raid(d: u32) -> bool {
    matches!(d, 3..=7 | 9 | 14..=17 | 33 | 151)
}
pub(crate) fn is_dungeon(d: u32) -> bool {
    matches!(d, 1 | 2 | 8 | 23 | 24)
}
pub(crate) const DELVE: u32 = 208;

/// A character's dot: their class colour as data, as a bar is, and their
/// name — none for a character whose class nobody saw.
fn dot_of(o: &Owner) -> Option<Who> {
    o.class.map(|class| Who {
        color: theme::class_rgb(class),
        name: o.name.clone(),
    })
}

impl Rail {
    /// The rail over `src`.
    pub(crate) fn build(src: &Sources) -> Self {
        let rows: Vec<(SegmentId, ListRow)> = src
            .entries
            .iter()
            .map(|e| match &src.watched {
                Some((id, fresh)) if *id == e.id => (e.id, fresh.clone()),
                _ => (e.id, e.row.clone()),
            })
            .collect();
        // The cards that are the log's own segments, by id: listed once, as
        // the log's, lending their rows what the store knows.
        let log_ids: HashMap<SegmentId, String> = match src.log_id {
            Some(log) => rows
                .iter()
                .map(|(id, r)| {
                    (
                        *id,
                        fight_id(log, r.start_ms, r.kind == SegmentKind::Overall),
                    )
                })
                .collect(),
            None => HashMap::new(),
        };
        let in_log: HashSet<&str> = log_ids.values().map(String::as_str).collect();
        let cards_by_id: HashMap<&str, &FightCard> =
            src.cards.iter().map(|c| (c.id.as_str(), c)).collect();

        let (mut instances, world) = log_visits(&rows, &log_ids, &cards_by_id, src.owner.as_ref());
        let mut stored: Vec<&FightCard> = Vec::new();
        for c in src.cards.iter().filter(|c| !in_log.contains(c.id.as_str())) {
            // A stored pull of the tailed log that its list does not hold
            // (a copy of the log cut short) was pulled in the log's visit
            // it followed: listed there, not as a visit of its own.
            match joins_log(c, src.log_id, &instances, &world).and_then(|i| instances.get_mut(i)) {
                Some(v) => v.lines.push(card_line(c, false)),
                None => stored.push(c),
            }
        }
        let names = Names::of(src.cards, &instances);
        let mut visits: Vec<(i64, Visit)> = instances
            .into_iter()
            .chain(world)
            .map(LogVisit::into_visit)
            .collect();
        visits.extend(stored_visits(&stored, &names));

        let mut nights: Vec<Night> = Vec::new();
        for (day, visit) in visits {
            match nights.iter_mut().find(|n| n.day == day) {
                Some(n) => n.visits.push(visit),
                None => nights.push(Night {
                    day,
                    label: String::new(),
                    tonight: false,
                    visits: vec![visit],
                }),
            }
        }
        nights.sort_by_key(|n| std::cmp::Reverse(n.day));
        for n in &mut nights {
            n.visits.sort_by_key(|v| std::cmp::Reverse(newest(v)));
            // A night with a pull still going is tonight, whatever the
            // clock the store's timezone reads.
            let live = n
                .visits
                .iter()
                .any(|v| v.lines.iter().any(|l| l.mark == Mark::Live));
            n.tonight = live || n.day == src.tonight;
            n.label = night_label(if live { src.tonight } else { n.day }, src.tonight);
        }
        Rail { nights }
    }

    /// Every row, top to bottom.
    pub(crate) fn lines(&self) -> impl Iterator<Item = &Line> {
        self.nights
            .iter()
            .flat_map(|n| n.visits.iter())
            .flat_map(|v| v.lines.iter())
    }

    /// The pull one row down the rail (`older`) or up it from `at`, over
    /// the rows `hide_trash` leaves. From a row it does not hold — a
    /// hidden one keeps its place — or from nowhere, the top. `None` at the
    /// end.
    pub(crate) fn step(&self, at: Option<&Pull>, older: bool, hide_trash: bool) -> Option<Pull> {
        let lines: Vec<&Line> = self.lines().collect();
        let shown = |l: &Line| !(hide_trash && l.trash);
        let Some(i) = at.and_then(|p| lines.iter().position(|l| l.pull == *p)) else {
            return lines.iter().find(|l| shown(l)).map(|l| l.pull.clone());
        };
        let found = if older {
            lines.iter().skip(i + 1).find(|l| shown(l))
        } else {
            lines.iter().take(i).rev().find(|l| shown(l))
        };
        found.map(|l| l.pull.clone())
    }

    /// The first night that is not tonight: where `H` opens the rail.
    pub(crate) fn earlier(&self) -> Option<usize> {
        self.nights.iter().position(|n| !n.tonight)
    }

    /// The night `pull` is listed under, by its place on the rail.
    pub(crate) fn night_of_pull(&self, pull: &Pull) -> Option<usize> {
        self.nights.iter().position(|n| {
            n.visits
                .iter()
                .any(|v| v.lines.iter().any(|l| l.pull == *pull))
        })
    }

    /// The row for `pull`, when the rail holds it.
    #[cfg(test)]
    pub(crate) fn line(&self, pull: &Pull) -> Option<&Line> {
        self.lines().find(|l| l.pull == *pull)
    }
}

/// The start the visit's order goes by: its newest pull's.
fn newest(v: &Visit) -> i64 {
    v.lines.iter().map(|l| l.start_ms).max().unwrap_or(i64::MIN)
}

/// A visit of the tailed log, before it is laid out: its title, its rows,
/// the span of the log's clock it covers (its first pull's start to its
/// last one's end), and — an instance visit's — the map and difficulty it
/// was at, as far as its Σ card and its pulls say.
struct LogVisit {
    title: String,
    lines: Vec<Line>,
    lo: i64,
    hi: i64,
    map: Option<u32>,
    difficulty: Option<u32>,
}

impl LogVisit {
    /// The visit laid out, with the night it began on.
    fn into_visit(self) -> (i64, Visit) {
        let day = self
            .lines
            .iter()
            .map(|l| l.start_ms)
            .min()
            .map_or(0, night_of);
        (day, visit(self.title, self.lines))
    }
}

/// The instance visit of the log a stored card of the log's own belongs
/// to, when its list does not hold the card: the last thing the log began
/// before the card — nothing zoned in between — on the card's night, an
/// instance visit at the card's map (or, where either says none, its
/// difficulty).
fn joins_log(
    c: &FightCard,
    log: Option<u64>,
    instances: &[LogVisit],
    world: &[LogVisit],
) -> Option<usize> {
    if log != Some(c.log) || !matches!(c.kind, FightKind::Encounter | FightKind::Trash) {
        return None;
    }
    let at = c.start_local_ms;
    let latest_world = world.iter().map(|w| w.lo).filter(|lo| *lo <= at).max();
    let (i, v) = instances
        .iter()
        .enumerate()
        .filter(|(_, v)| v.lo <= at)
        .max_by_key(|(_, v)| v.lo)?;
    if latest_world.is_some_and(|w| w > v.lo) || night_of(v.lo) != night_of(at) {
        return None;
    }
    let difficulty = map_of(c)
        .map(|(_, d)| d)
        .or(c.encounter.map(|e| e.difficulty));
    let same = match (v.map, map_of(c)) {
        (Some(m), Some((cm, cd))) => m == cm && v.difficulty.is_none_or(|d| d == cd),
        _ => v.difficulty.is_some() && v.difficulty == difficulty,
    };
    same.then_some(i)
}

/// The tailed log's visits: each instance visit (its members and its Σ),
/// and the runs of segments between them out in the world — an arena's
/// matches a run of their own. A segment the daemon files under no visit
/// but that fell inside one's span — trash between two of its bosses — is
/// that visit's, as the reader lived it.
fn log_visits(
    rows: &[(SegmentId, ListRow)],
    log_ids: &HashMap<SegmentId, String>,
    cards: &HashMap<&str, &FightCard>,
    owner: Option<&(Option<String>, Class)>,
) -> (Vec<LogVisit>, Vec<LogVisit>) {
    let entries: Vec<ListEntry> = rows
        .iter()
        .map(|(id, row)| ListEntry {
            id: *id,
            row: row.clone(),
        })
        .collect();
    let card_of = |id: SegmentId| {
        log_ids
            .get(&id)
            .and_then(|f| cards.get(f.as_str()))
            .copied()
    };
    // The log is one logger's: a card of it that names its owner says who;
    // else the window's owner.
    let logger = rows
        .iter()
        .filter_map(|(id, _)| card_of(*id))
        .find_map(card_owner)
        .filter(|o| o.class.is_some())
        .or_else(|| {
            owner.map(|(name, class)| Owner {
                guid: String::new(),
                name: name.clone().unwrap_or_else(|| YOU.to_string()),
                class: Some(*class),
            })
        });
    let line = |pos: usize| -> Option<Line> {
        let (id, r) = rows.get(pos)?;
        let card = card_of(*id);
        Some(Line {
            pull: Pull::Log(*id),
            // A trash segment is trash, whatever the engine last named it.
            name: match r.kind {
                SegmentKind::Overall => "Whole visit".to_string(),
                SegmentKind::Trash => "Trash".to_string(),
                SegmentKind::Encounter => r.name.clone(),
            },
            mark: row_mark(r),
            trash: r.kind == SegmentKind::Trash,
            best_pct: card.and_then(crate::home::wipe_pct),
            key: r.pars_ms.zip(r.success).map(|(pars, timed)| match timed {
                true => KeyWord::Plus(key_tier(r.duration_ms, pars)),
                false => KeyWord::Over,
            }),
            dot: None,
            pinned: card.is_some_and(|c| c.pinned),
            duration_ms: r.duration_ms,
            start_ms: r.start_ms,
            owner: logger.clone(),
        })
    };
    let blocks = crate::timeline::blocks(&entries);
    // Every instance visit, and the span of the log's clock it covers: its
    // first pull's start to its last one's end.
    let mut visits: Vec<LogVisit> = Vec::new();
    for block in blocks.iter().filter(|b| b.ordinal.is_some()) {
        let overall = block.overall.and_then(|p| rows.get(p)).map(|(_, r)| r);
        let sigma_card = block
            .overall
            .and_then(|p| rows.get(p))
            .and_then(|(id, _)| card_of(*id));
        let difficulty_id = block
            .members
            .iter()
            .filter_map(|&p| rows.get(p))
            .find_map(|(_, r)| r.encounter.map(|e| e.difficulty))
            .or(sigma_card.and_then(map_of).map(|(_, d)| d));
        let difficulty = difficulty_id.and_then(difficulty_name);
        let title = match (overall, difficulty) {
            // A key's name carries its level; its difficulty is the key.
            (Some(o), _) if o.pars_ms.is_some() => o.name.clone(),
            (Some(o), Some(d)) => format!("{}, {d}", o.name),
            (Some(o), None) => o.name.clone(),
            (None, Some(d)) => format!("Instance, {d}"),
            (None, None) => "Instance".to_string(),
        };
        let spans = block
            .members
            .iter()
            .chain(block.overall.iter())
            .filter_map(|&p| rows.get(p))
            .map(|(_, r)| (r.start_ms, r.start_ms + r.duration_ms.max(0)));
        let (lo, hi) = spans.fold((i64::MAX, i64::MIN), |(lo, hi), (s, e)| {
            (lo.min(s), hi.max(e))
        });
        let mut lines: Vec<Line> = block.members.iter().filter_map(|&p| line(p)).collect();
        lines.extend(block.overall.and_then(line));
        visits.push(LogVisit {
            title,
            lines,
            lo,
            hi,
            map: sigma_card.and_then(map_of).map(|(m, _)| m),
            difficulty: difficulty_id,
        });
    }
    let mut out: Vec<LogVisit> = Vec::new();
    // Out in the world, consecutive segments are one visit — an arena's
    // matches one of their own, called what they were.
    let mut world: Vec<Line> = Vec::new();
    let mut arena = false;
    let flush = |world: &mut Vec<Line>, arena: bool, out: &mut Vec<LogVisit>| {
        if world.is_empty() {
            return;
        }
        let lines = std::mem::take(world);
        let lo = lines.iter().map(|l| l.start_ms).min().unwrap_or(0);
        let hi = lines
            .iter()
            .map(|l| l.start_ms + l.duration_ms.max(0))
            .max()
            .unwrap_or(lo);
        let title = if arena { "Arena" } else { "Open world" };
        out.push(LogVisit {
            title: title.to_string(),
            lines,
            lo,
            hi,
            map: None,
            difficulty: None,
        });
    };
    for block in blocks.iter() {
        if block.ordinal.is_some() {
            flush(&mut world, arena, &mut out);
            continue;
        }
        for &p in &block.members {
            let Some(l) = line(p) else {
                continue;
            };
            match visits
                .iter_mut()
                .find(|v| (v.lo..v.hi).contains(&l.start_ms))
            {
                Some(v) => v.lines.push(l),
                None => {
                    let is_arena = rows.get(p).is_some_and(|(_, r)| r.arena);
                    if is_arena != arena {
                        flush(&mut world, arena, &mut out);
                        arena = is_arena;
                    }
                    world.push(l);
                }
            }
        }
    }
    flush(&mut world, arena, &mut out);
    (visits, out)
}

/// The tailed log's instance visits as the rail titles them — each one's
/// instance, by its Σ row's name, and the span of the log's clock it
/// covers — for Home, whose raid pulls are named by their visit's Σ CARD,
/// which the daemon stores only once the visit closes: on a raid night
/// still going, the log's own list is what knows where the pulls were. A
/// key's visit is left out (its Σ is named for the key, not a place).
pub(crate) fn log_instances(entries: &[ListEntry], log_id: Option<u64>) -> Vec<LogInstance> {
    let Some(log) = log_id else {
        return Vec::new();
    };
    crate::timeline::blocks(entries)
        .iter()
        .filter(|b| b.ordinal.is_some())
        .filter_map(|b| {
            let sigma = &entries.get(b.overall?)?.row;
            if sigma.pars_ms.is_some() || sigma.name.is_empty() {
                return None;
            }
            let (lo, hi) = b
                .members
                .iter()
                .chain(b.overall.iter())
                .filter_map(|&p| entries.get(p))
                .map(|e| (e.row.start_ms, e.row.start_ms + e.row.duration_ms.max(0)))
                .fold((i64::MAX, i64::MIN), |(lo, hi), (s, e)| {
                    (lo.min(s), hi.max(e))
                });
            Some(LogInstance {
                log,
                lo,
                hi,
                name: sigma.name.clone(),
            })
        })
        .collect()
}

/// An instance visit of the tailed log ([`log_instances`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LogInstance {
    pub log: u64,
    /// Its first pull's start to its last one's end, on the log's clock.
    pub lo: i64,
    pub hi: i64,
    /// "The Venomous Abyss".
    pub name: String,
}

impl LogInstance {
    /// `c` was pulled in this visit: its log's, begun within its span.
    pub(crate) fn holds(&self, c: &FightCard) -> bool {
        c.log == self.log && (self.lo..=self.hi).contains(&c.start_local_ms)
    }
}

/// A log row's glyph: a visit's Σ is one whether or not the visit goes on.
fn row_mark(r: &ListRow) -> Mark {
    match (r.kind, r.live, r.success) {
        (SegmentKind::Overall, _, _) => Mark::Sum,
        (_, true, _) => Mark::Live,
        (SegmentKind::Trash, _, _) => Mark::Dash,
        (SegmentKind::Encounter, _, Some(true)) => Mark::Good,
        (SegmentKind::Encounter, _, Some(false)) => Mark::Bad,
        (SegmentKind::Encounter, _, None) => Mark::Dash,
    }
}

/// Who a dot names when the window knows the owner's class but not yet
/// their name.
const YOU: &str = "You";

/// A card's owner, their name and their class, when the card names one.
fn card_owner(c: &FightCard) -> Option<Owner> {
    let guid = c.owner.as_ref()?;
    let player = c.players.iter().find(|p| p.guid == *guid);
    Some(Owner {
        guid: guid.clone(),
        name: player.map_or_else(|| guid.clone(), |p| p.name.clone()),
        class: player.and_then(|p| p.class),
    })
}

/// A stored card as a row: `sum` for a raid visit's Σ.
fn card_line(c: &FightCard, sum: bool) -> Line {
    let mark = match (sum, c.kind, c.success) {
        (true, _, _) => Mark::Sum,
        (_, FightKind::Trash, _) | (_, FightKind::Overall, _) => Mark::Dash,
        (_, _, _) if c.aborted => Mark::Dash,
        (_, _, Some(true)) => Mark::Good,
        (_, _, Some(false)) => Mark::Bad,
        (_, _, None) => Mark::Dash,
    };
    let clock = match c.kind {
        FightKind::Key => c.official_ms.unwrap_or(c.duration_ms),
        _ => c.duration_ms,
    };
    let key = match (c.kind, c.pars_ms, c.success) {
        (FightKind::Key, Some(pars), Some(true)) => Some(KeyWord::Plus(key_tier(clock, pars))),
        (FightKind::Key, _, Some(false)) => Some(KeyWord::Over),
        _ => None,
    };
    Line {
        pull: Pull::Stored(c.id.clone()),
        name: if sum {
            "Whole visit".to_string()
        } else {
            c.name.clone()
        },
        mark,
        trash: c.kind == FightKind::Trash,
        best_pct: crate::home::wipe_pct(c),
        key,
        dot: None,
        pinned: c.pinned,
        duration_ms: clock,
        start_ms: c.start_local_ms,
        owner: card_owner(c),
    }
}

/// The map and difficulty a card was in, when it says.
fn map_of(c: &FightCard) -> Option<(u32, u32)> {
    c.key.as_ref().map(|k| (k.map_id, k.difficulty))
}

/// What names a raid pull no visit of the store claims: the instances the
/// window has seen a Σ card of, by map (a card's name, from any night);
/// and the log's instance visits — their night, difficulty and title.
struct Names {
    instances: HashMap<u32, String>,
    log: Vec<(i64, Option<u32>, Option<u32>, String)>,
}

impl Names {
    fn of(cards: &[FightCard], log: &[LogVisit]) -> Self {
        let mut instances = HashMap::new();
        for c in cards.iter().filter(|c| c.kind == FightKind::Overall) {
            if let Some((map, _)) = map_of(c) {
                instances.entry(map).or_insert_with(|| c.name.clone());
            }
        }
        let log = log
            .iter()
            .map(|v| (night_of(v.lo), v.map, v.difficulty, v.title.clone()))
            .collect();
        Names { instances, log }
    }

    /// A title for a raid boss pull at `difficulty` (on `map` when its card
    /// says) on night `day` that no stored visit claims: a visit of the
    /// log that night at that difficulty (and map) — the same instance, as
    /// the reader lived the night — else the instance its map names, else
    /// the kind of content it was.
    fn raid(&self, day: i64, map: Option<u32>, difficulty: u32) -> String {
        let same_map = |m: Option<u32>| match (m, map) {
            (Some(a), Some(b)) => a == b,
            _ => true,
        };
        if let Some((.., title)) = self
            .log
            .iter()
            .find(|(d, m, diff, _)| *d == day && *diff == Some(difficulty) && same_map(*m))
        {
            return title.clone();
        }
        let name = difficulty_name(difficulty);
        match (map.and_then(|m| self.instances.get(&m)), name) {
            (Some(instance), Some(d)) => format!("{instance}, {d}"),
            (Some(instance), None) => instance.clone(),
            (None, Some(d)) => format!("Raid, {d}"),
            (None, None) => "Raid".to_string(),
        }
    }
}

/// The store's visits, each with its night. Per night: every raid visit
/// (its Σ card, the pulls in it), the night's keys and dungeon runs as one
/// visit, and what no visit claims under the kind of content it was — a
/// raid's pulls under the instance's name wherever the window knows it.
fn stored_visits(cards: &[&FightCard], names: &Names) -> Vec<(i64, Visit)> {
    let mut by_night: Vec<(i64, Vec<&FightCard>)> = Vec::new();
    for c in cards {
        let day = night_of(c.start_local_ms);
        match by_night.iter_mut().find(|(d, _)| *d == day) {
            Some((_, cs)) => cs.push(c),
            None => by_night.push((day, vec![c])),
        }
    }
    let mut out = Vec::new();
    for (day, mut cards) in by_night {
        cards.sort_by_key(|c| c.start_local_ms);
        let dungeon = |c: &FightCard| map_of(c).is_some_and(|(_, d)| is_dungeon(d));
        // A dungeon visit a key on its map starts right after is the key's
        // zone-in.
        let zone_in = |o: &FightCard| {
            cards.iter().any(|k| {
                k.kind == FightKind::Key
                    && k.log == o.log
                    && map_of(k).map(|m| m.0) == map_of(o).map(|m| m.0)
                    && (0..=ZONE_IN_MS).contains(&(k.start_local_ms - o.start_local_ms))
            })
        };
        let raids: Vec<&FightCard> = cards
            .iter()
            .copied()
            .filter(|c| c.kind == FightKind::Overall && !dungeon(c))
            .collect();
        let mut members: Vec<Vec<Line>> = vec![Vec::new(); raids.len()];
        let mut keys: Vec<Line> = Vec::new();
        let mut loose: Vec<(String, Vec<Line>)> = Vec::new();
        for c in cards.iter().copied() {
            match c.kind {
                FightKind::Key => keys.push(card_line(c, false)),
                FightKind::Overall if dungeon(c) => {
                    if !zone_in(c) {
                        keys.push(card_line(c, false));
                    }
                }
                FightKind::Overall => {}
                FightKind::Encounter | FightKind::Arena | FightKind::Trash => {
                    // The latest raid visit of the same log begun before it,
                    // on its map when both say theirs. A card with no visit
                    // (an arena match, the open world's pulls) was in none:
                    // only a raid boss the store filed without its visit
                    // joins a Σ, at the Σ's difficulty. A Σ with no visit
                    // of its own (a store from before they were kept) says
                    // nothing to go by.
                    let home = raids.iter().rposition(|o| {
                        o.log == c.log
                            && c.kind != FightKind::Arena
                            && o.start_local_ms <= c.start_local_ms
                            && match (map_of(o), map_of(c)) {
                                (Some(a), Some(b)) => a.0 == b.0,
                                (Some(a), None) => c
                                    .encounter
                                    .is_some_and(|e| is_raid(e.difficulty) && e.difficulty == a.1),
                                (None, _) => true,
                            }
                    });
                    match home.and_then(|i| members.get_mut(i)) {
                        Some(lines) => lines.push(card_line(c, false)),
                        None => {
                            let title = loose_title(c, day, names);
                            match loose.iter_mut().find(|(t, _)| *t == title) {
                                Some((_, lines)) => lines.push(card_line(c, false)),
                                None => loose.push((title, vec![card_line(c, false)])),
                            }
                        }
                    }
                }
            }
        }
        for (o, mut lines) in raids.iter().zip(members) {
            let title = match map_of(o).and_then(|(_, d)| difficulty_name(d)) {
                Some(d) => format!("{}, {d}", o.name),
                None => o.name.clone(),
            };
            // A raid pull no Σ claimed, named for this visit's instance and
            // difficulty, is this visit's: the same raid that night.
            if let Some(at) = loose.iter().position(|(t, _)| *t == title) {
                lines.extend(loose.remove(at).1);
            }
            lines.push(card_line(o, true));
            out.push((day, visit(title, lines)));
        }
        if !keys.is_empty() {
            let any_key = cards.iter().any(|c| c.kind == FightKind::Key);
            let title = if any_key { "Mythic+ keys" } else { "Dungeons" };
            out.push((day, visit(title.to_string(), keys)));
        }
        for (title, lines) in loose {
            out.push((day, visit(title, lines)));
        }
    }
    out
}

/// What a pull no visit claims was: an arena match, a delve, a raid boss
/// under its instance's name where the window knows it ([`Names::raid`]),
/// a dungeon boss, or trash out in the world.
fn loose_title(c: &FightCard, day: i64, names: &Names) -> String {
    let d = c.encounter.map(|e| e.difficulty);
    match (c.kind, d) {
        (FightKind::Arena, _) => "Arena".to_string(),
        (FightKind::Trash, _) => "Open world".to_string(),
        (_, Some(DELVE)) => "Delve".to_string(),
        (_, Some(d)) if is_raid(d) => names.raid(day, map_of(c).map(|(m, _)| m), d),
        (_, Some(d)) if is_dungeon(d) => "Dungeons".to_string(),
        _ => "Encounters".to_string(),
    }
}

/// A visit of `lines`: newest first with any Σ last, and the dots placed —
/// one on the visit when one character played all of it, one per row when
/// several did, none when nobody says.
fn visit(title: String, mut lines: Vec<Line>) -> Visit {
    lines.sort_by_key(|l| (l.mark == Mark::Sum, std::cmp::Reverse(l.start_ms)));
    let owners: Vec<&Owner> = lines.iter().filter_map(|l| l.owner.as_ref()).collect();
    let one = owners
        .first()
        .filter(|first| owners.iter().all(|o| o.guid == first.guid))
        .copied()
        .cloned();
    let dot = match one {
        Some(o) => dot_of(&o),
        None => {
            for l in &mut lines {
                l.dot = l.owner.as_ref().and_then(dot_of);
            }
            None
        }
    };
    Visit { title, dot, lines }
}

// ---- drawing -----------------------------------------------------------------------

/// Whether more of the store can be asked for, and whether it is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum More {
    /// The store is all in hand (or off): no button.
    None,
    /// "Show older nights".
    Offer,
    /// A page is on its way.
    Asking,
}

/// The rail's scrollable, the current pull's row, the first earlier
/// night's heading and the drawer's scrim, for the window's operations and
/// its tests.
pub(crate) fn scroll_id() -> iced::widget::Id {
    iced::widget::Id::new("rail")
}
pub(crate) fn current_id() -> iced::widget::Id {
    iced::widget::Id::new("rail-current")
}
pub(crate) fn earlier_id() -> iced::widget::Id {
    iced::widget::Id::new("rail-earlier")
}
pub(crate) fn scrim_id() -> iced::widget::Id {
    iced::widget::Id::new("rail-scrim")
}
/// The drawer's keyboard highlight, and the heading of the night the pull
/// on the stage is listed under.
pub(crate) fn cursor_id() -> iced::widget::Id {
    iced::widget::Id::new("rail-cursor")
}
pub(crate) fn current_night_id() -> iced::widget::Id {
    iced::widget::Id::new("rail-current-night")
}

/// What the rail draws beside its model.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Shown {
    /// The pull on the stage.
    pub at: Option<Pull>,
    /// The row the drawer's keys are on (j, k, the arrows; Enter opens it).
    pub cursor: Option<Pull>,
    pub hide_trash: bool,
    pub more: More,
    pub accent: theme::Accent,
    /// Names in the dots' tips go without their realm, as every name does.
    pub hide_realms: bool,
}

impl Shown {
    /// Does the rail draw `l`? Trash goes when the toggle says so — but
    /// never the pull on the stage, nor the row the drawer's keys are on,
    /// which keep their places.
    fn shows(&self, l: &Line) -> bool {
        !(self.hide_trash && l.trash)
            || self.at.as_ref() == Some(&l.pull)
            || self.cursor.as_ref() == Some(&l.pull)
    }
}

/// What the rail says with nothing to list.
pub(crate) const EMPTY: &str =
    "No pulls yet. Pulls appear here as you fight, and earlier nights load from your history.";

/// The rail, `width` wide (`.rail`): its head — "Pulls" and the trash
/// toggle — over the nights, on the panel's surface.
pub(crate) fn panel(rail: &Rail, shown: &Shown, width: f32) -> Element<'static, Message> {
    let on = shown.hide_trash;
    // Pressed, the toggle is raised and its words in the second ink
    // (`.toggle[aria-pressed=true]`): the same words at the same width, so
    // nothing on the head moves when it is pressed.
    let toggle = button(text("Hide trash").size(TOGGLE_PX))
        .padding(TOGGLE_PAD)
        .on_press(Message::HideTrash)
        .style(move |_: &Theme, _| button::Style {
            background: on.then(|| theme::RAISE.into()),
            text_color: if on { theme::INK_2 } else { theme::INK_3_TEXT },
            border: iced::border::rounded(TOGGLE_RADIUS),
            ..button::Style::default()
        });
    let head = container(
        row![
            text("Pulls")
                .size(HEAD_PX)
                .font(theme::UI_SEMIBOLD)
                .color(theme::INK),
            Space::new().width(Length::Fill),
            toggle,
        ]
        .spacing(HEAD_GAP)
        .align_y(iced::Alignment::Center),
    )
    .padding(HEAD_PAD)
    .width(Length::Fill);
    let earlier = rail.earlier();
    let mut list = column![];
    if rail.nights.is_empty() {
        list = list
            .push(container(text(EMPTY).size(size::SMALL).color(theme::INK_2)).padding(NIGHT_PAD));
    }
    for (i, n) in rail.nights.iter().enumerate() {
        // The visits the toggle leaves anything of: a night whose every
        // pull it hides is no heading over nothing.
        let visits: Vec<(&Visit, Vec<&Line>)> = n
            .visits
            .iter()
            .map(|v| (v, v.lines.iter().filter(|l| shown.shows(l)).collect()))
            .filter(|(_, lines): &(&Visit, Vec<&Line>)| !lines.is_empty())
            .collect();
        if visits.is_empty() {
            continue;
        }
        let heading = container(
            text(n.label.clone())
                .size(LABEL_PX)
                .font(theme::UI_SEMIBOLD)
                .color(theme::GOLD_DIM),
        )
        .padding(NIGHT_PAD)
        .width(Length::Fill);
        let heading = if earlier == Some(i) {
            heading.id(earlier_id())
        } else {
            heading
        };
        // The night the pull on the stage is under: where the drawer
        // opening on it may stand its heading at the top.
        let holds_current = shown.at.as_ref().is_some_and(|at| {
            visits
                .iter()
                .any(|(_, ls)| ls.iter().any(|l| l.pull == *at))
        });
        list = list.push(if holds_current {
            container(heading)
                .id(current_night_id())
                .width(Length::Fill)
        } else {
            heading
        });
        for (v, lines) in visits {
            list = list.push(visit_line(v, shown.hide_realms));
            for l in lines {
                list = list.push(pull_line(
                    l,
                    shown.at.as_ref() == Some(&l.pull),
                    shown.cursor.as_ref() == Some(&l.pull),
                    shown.accent,
                    shown.hide_realms,
                ));
            }
        }
    }
    match shown.more {
        More::None => {}
        More::Offer => list = list.push(more_button(true)),
        More::Asking => list = list.push(more_button(false)),
    }
    let list = list.push(Space::new().height(Length::Fixed(FOOT)));
    container(
        column![
            head,
            nav::hairline::<Message>(),
            scrollable(crate::view::scroll_clear(list.width(Length::Fill)))
                .id(scroll_id())
                // The rail's own thumb: 6 px down the middle of its 10 px
                // lane (the prototype's padding-box thumb), rail-local so
                // every other list keeps its own.
                .direction(scrollable::Direction::Vertical(
                    scrollable::Scrollbar::new()
                        .width(SCROLL_LANE)
                        .scroller_width(SCROLL_THUMB),
                ))
                .height(Length::Fill)
                .width(Length::Fill),
        ]
        .height(Length::Fill),
    )
    .width(Length::Fixed(width))
    .height(Length::Fill)
    .style(|_: &Theme| container::Style {
        background: Some(theme::SURFACE.into()),
        ..container::Style::default()
    })
    .into()
}

/// The rail over `stage` as a drawer (`.app.rail-open .rail`): the stage
/// under a scrim that closes it, the rail at the left with its shadow.
/// The scrim holds the pointer (the arrow, `Idle`, is an interaction a
/// stack stops at), so the rows under it light for no one; the rail is
/// opaque to it, so a press on a heading, a visit's line or the empty foot
/// of a short list is the rail's, and never reaches the scrim under it.
pub(crate) fn drawer(
    stage: Element<'static, Message>,
    rail: Element<'static, Message>,
) -> Element<'static, Message> {
    let scrim = mouse_area(
        container(Space::new())
            .id(scrim_id())
            .width(Length::Fill)
            .height(Length::Fill)
            .style(|_: &Theme| container::Style {
                background: Some(theme::RAIL_SCRIM.into()),
                ..container::Style::default()
            }),
    )
    .interaction(iced::mouse::Interaction::Idle)
    .on_press(Message::CloseRail);
    let rail = opaque(
        container(rail)
            .height(Length::Fill)
            .style(|_: &Theme| container::Style {
                background: Some(theme::SURFACE.into()),
                shadow: DRAWER_SHADOW,
                ..container::Style::default()
            }),
    );
    stack![stage, scrim, rail]
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}

/// A name as the window draws it — the rail's tips, Home, the palette,
/// the inspector: without its realm when the option says so.
pub(crate) fn shown_name(name: &str, hide_realms: bool) -> String {
    if hide_realms {
        crate::view::display_name(name).to_string()
    } else {
        name.to_string()
    }
}

/// A character's dot, saying under the pointer whose it is.
fn dot(who: &Who, hide_realms: bool) -> Element<'static, Message> {
    nav::tip(
        nav::dot::<Message>(who.color, DOT),
        shown_name(&who.name, hide_realms),
    )
}

/// A visit's line (`.visit`): the character's dot and what it was.
fn visit_line(v: &Visit, hide_realms: bool) -> Element<'static, Message> {
    let mut line = row![].spacing(VISIT_GAP).align_y(iced::Alignment::Center);
    if let Some(who) = &v.dot {
        line = line.push(dot(who, hide_realms));
    }
    line = line.push(
        ellipsis(v.title.clone())
            .size(LABEL_PX)
            .color(theme::INK_3_TEXT),
    );
    container(line)
        .padding(VISIT_PAD)
        .width(Length::Fill)
        .into()
}

/// A pull's name, where the line lets it be: a key's level ("+14", the one
/// thing between two runs of a dungeon) is kept whole after its dungeon's
/// name, which is what gives way to "…".
fn pull_name(name: &str, px: f32, ink: Color) -> Element<'static, Message> {
    let dungeon = crate::home::dungeon_name(name);
    let level = name.get(dungeon.len()..).unwrap_or_default();
    if level.is_empty() {
        return ellipsis(name.to_string()).size(px).color(ink).into();
    }
    container(
        row![
            ellipsis(dungeon.to_string())
                .size(px)
                .color(ink)
                .leaving(vec![(level.to_string(), px, theme::UI)], 0.0),
            text(level.to_string())
                .size(px)
                .color(ink)
                .wrapping(text::Wrapping::None),
        ]
        .align_y(iced::Alignment::Center),
    )
    .width(Length::Fill)
    .into()
}

/// A pull's clock on its row: a pull under a second — a boss reset, a
/// pull the game called off at once — says so, where "0:00" would read as
/// a value that failed to arrive.
pub(crate) fn clock(ms: i64) -> String {
    if ms < 1000 {
        "<0:01".to_string()
    } else {
        duration(ms)
    }
}

/// A pull's row (`.pull`): its glyph, its name, and at its right what it
/// came to — lit and edged in the accent when it is on the stage, ringed
/// in it when the drawer's keys are on another row (`keyed`).
fn pull_line(
    l: &Line,
    current: bool,
    keyed: bool,
    accent: theme::Accent,
    hide_realms: bool,
) -> Element<'static, Message> {
    let (px, ink) = match (l.trash, l.mark) {
        (true, _) => (TRASH_PX, theme::INK_3_TEXT),
        (_, Mark::Sum) => (NAME_PX, theme::INK_2),
        _ => (NAME_PX, theme::INK),
    };
    let quiet = |s: String| {
        text(s)
            .size(RIGHT_PX)
            .color(theme::INK_2)
            .wrapping(text::Wrapping::None)
    };
    let mut right = row![].spacing(RIGHT_GAP).align_y(iced::Alignment::Center);
    if let Some(pct) = l.best_pct {
        right = right.push(quiet(format!("{pct}%")));
    }
    match l.key {
        Some(KeyWord::Plus(n)) => {
            right = right.push(
                text(format!("+{n}"))
                    .size(RIGHT_PX)
                    .font(theme::UI_SEMIBOLD)
                    .color(theme::GOOD),
            );
        }
        Some(KeyWord::Over) => {
            right = right.push(
                text("over")
                    .size(RIGHT_PX)
                    .font(theme::UI_SEMIBOLD)
                    .color(theme::BAD),
            );
        }
        None => {}
    }
    if let Some(who) = &l.dot {
        right = right.push(dot(who, hide_realms));
    }
    right = right.push(quiet(clock(l.duration_ms)));
    let body = container(
        row![mark(l.mark), pull_name(&l.name, px, ink), right]
            .spacing(PULL_GAP)
            .align_y(iced::Alignment::Center),
    )
    .padding(PULL_PAD)
    .height(Length::Fixed(PULL_H))
    .width(Length::Fill)
    .align_y(iced::Alignment::Center);
    let edge = container(Space::new())
        .width(Length::Fixed(EDGE_W))
        .height(Length::Fill)
        .style(move |_: &Theme| container::Style {
            background: current.then(|| accent.base.into()),
            ..container::Style::default()
        });
    // A pinned card's star stands in the row's left gutter, before its
    // glyph: a shape that takes nothing from the name, where the name and
    // its right cluster already share 236 px (the store behind the shots
    // pins 117 of its 802 cards) — in the quiet ink, so the one gold at the
    // rail's edge is the current row's.
    let mut layers = stack![body, edge];
    if l.pinned {
        layers = layers.push(
            container(nav::tip(
                text(PIN)
                    .size(PIN_PX)
                    .color(theme::INK_3)
                    .wrapping(text::Wrapping::None),
                PIN_TIP,
            ))
            .padding(iced::Padding {
                left: PIN_INSET,
                ..iced::Padding::ZERO
            })
            .height(Length::Fill)
            .align_y(iced::Alignment::Center),
        );
    }
    let face = container(layers).width(Length::Fill);
    let face = if current { face.id(current_id()) } else { face };
    let face = if keyed {
        container(face).id(cursor_id()).width(Length::Fill)
    } else {
        face
    };
    button(face)
        .padding(0)
        .width(Length::Fill)
        .on_press(Message::Pull(l.pull.clone()))
        .style(move |_: &Theme, status| button::Style {
            background: if current {
                Some(theme::RAISE.into())
            } else {
                (keyed || matches!(status, button::Status::Hovered | button::Status::Pressed))
                    .then(|| theme::HOVER.into())
            },
            text_color: theme::INK,
            // The keys' row, once they leave the pull on the stage (whose
            // row is lit already): a focus ring, the accent's other use.
            border: if keyed && !current {
                iced::Border {
                    color: accent.base,
                    width: CURSOR_RING,
                    radius: CURSOR_RADIUS.into(),
                }
            } else {
                iced::Border::default()
            },
            ..button::Style::default()
        })
        .into()
}

/// A row's lead glyph in its 16 px box.
pub(crate) fn mark(m: Mark) -> Element<'static, Message> {
    let glyph: Element<'static, Message> = match m {
        Mark::Good => line_icon(LineIcon::Check, MARK_ICON, theme::GOOD),
        Mark::Bad => line_icon(LineIcon::Close, MARK_ICON, theme::BAD),
        Mark::Live => nav::dot(theme::BAD, size::DOT),
        Mark::Sum => text("Σ").size(NAME_PX).color(theme::INK_3_TEXT).into(),
        Mark::Dash => container(Space::new())
            .width(Length::Fixed(DASH_W))
            .height(Length::Fixed(DASH_H))
            .style(|_: &Theme| container::Style {
                background: Some(theme::INK_3.into()),
                border: iced::border::rounded(DASH_RADIUS),
                ..container::Style::default()
            })
            .into(),
    };
    container(glyph).center(Length::Fixed(MARK_BOX)).into()
}

/// "Show older nights" (`.rail-more`), or the word that a page is coming:
/// framed in a dashed edge, which is what says "more to load" rather than
/// "a button like the rest". iced's borders are solid, so the dashes are a
/// canvas under the button, as large as it is.
fn more_button(offer: bool) -> Element<'static, Message> {
    let words = if offer {
        "Show older nights"
    } else {
        "Reading the history store…"
    };
    let face = button(
        text(words)
            .size(RIGHT_PX)
            .width(Length::Fill)
            .align_x(iced::Alignment::Center),
    )
    .padding(MORE_PAD)
    .width(Length::Fill)
    .on_press_maybe(offer.then_some(Message::OlderNights))
    .style(|_: &Theme, status| button::Style {
        text_color: match status {
            button::Status::Hovered | button::Status::Pressed => theme::INK_2,
            _ => theme::INK_3_TEXT,
        },
        ..button::Style::default()
    });
    let edge = Canvas::new(DashedEdge)
        .width(Length::Fill)
        .height(Length::Fill);
    container(stack![face].push_under(edge))
        .padding(iced::Padding {
            top: MORE_MARGIN,
            right: MORE_MARGIN,
            bottom: 0.0,
            left: MORE_MARGIN,
        })
        .width(Length::Fill)
        .into()
}

/// The dashed edge of "Show older nights" (`border:1px dashed var(--edge);
/// border-radius:6px`), drawn inside its bounds.
struct DashedEdge;

impl<M> canvas::Program<M> for DashedEdge {
    type State = ();

    fn draw(
        &self,
        _state: &(),
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<canvas::Geometry> {
        let mut frame = canvas::Frame::new(renderer, bounds.size());
        // The stroke centred on a line half its width in, so all of it is
        // inside the bounds, as a CSS border is.
        let half = MORE_EDGE / 2.0;
        let frame_path = Path::rounded_rectangle(
            Point::new(half, half),
            Size::new(bounds.width - MORE_EDGE, bounds.height - MORE_EDGE),
            MORE_RADIUS.into(),
        );
        frame.stroke(
            &frame_path,
            Stroke {
                line_dash: canvas::LineDash {
                    segments: &MORE_DASH,
                    offset: 0,
                },
                ..Stroke::default()
                    .with_color(theme::EDGE)
                    .with_width(MORE_EDGE)
            },
        );
        vec![frame.into_geometry()]
    }
}

// ---- operations ------------------------------------------------------------------

/// Scrolls the rail so the earlier nights' heading stands at its top — `H`.
/// Nothing when the rail holds no earlier night, or is not drawn.
#[derive(Default)]
pub(crate) struct ToEarlier {
    content: Option<Rectangle>,
    found: Option<Rectangle>,
}

impl Operation for ToEarlier {
    fn traverse(&mut self, operate: &mut dyn FnMut(&mut dyn Operation)) {
        operate(self);
    }

    fn container(&mut self, id: Option<&iced::widget::Id>, bounds: Rectangle) {
        if id == Some(&earlier_id()) {
            self.found = Some(bounds);
        }
    }

    fn scrollable(
        &mut self,
        id: Option<&iced::widget::Id>,
        _bounds: Rectangle,
        content: Rectangle,
        _translation: iced::Vector,
        _state: &mut dyn operation::Scrollable,
    ) {
        if id == Some(&scroll_id()) {
            self.content = Some(content);
        }
    }

    fn finish(&self) -> Outcome<()> {
        match (self.content, self.found) {
            (Some(content), Some(found)) => Outcome::Chain(Box::new(ScrollTo {
                y: found.y - content.y,
            })),
            _ => Outcome::None,
        }
    }
}

/// The room the rail leaves under a row it scrolls to (the prototype's
/// `near()`: `pad = 40` past the bottom): the row shows with the rows after
/// it, not flush with the rail's last line.
pub(crate) const NEAR_PAD: f32 = 40.0;

/// Where the rail's list stands to show the row `[top, bottom]` (its
/// content's coordinates) from `offset`, in a viewport `height` tall over
/// content `content` tall: the least move that shows it with [`NEAR_PAD`]
/// under it — a row above the viewport comes to its top (`near()`).
pub(crate) fn near_offset(offset: f32, height: f32, content: f32, top: f32, bottom: f32) -> f32 {
    let to = if top < offset {
        top
    } else if bottom > offset + height - NEAR_PAD {
        bottom - height + NEAR_PAD
    } else {
        offset
    };
    to.clamp(0.0, (content - height).max(0.0))
}

/// Where the drawer opening on the row `[top, bottom]` stands it: where it
/// is, when the row is in sight with its [`NEAR_PAD`]; else its night's
/// heading (at `heading`) at the top, when the heading and the row fit
/// under it together — the visit's other pulls and its Σ in view; else the
/// row in the middle.
pub(crate) fn open_offset(
    offset: f32,
    height: f32,
    content: f32,
    (top, bottom): (f32, f32),
    heading: Option<f32>,
) -> f32 {
    let to = if top >= offset && bottom <= offset + height - NEAR_PAD {
        offset
    } else {
        match heading {
            Some(h) if h <= top && bottom - h + NEAR_PAD <= height => h,
            _ => (top + bottom) / 2.0 - height / 2.0,
        }
    };
    to.clamp(0.0, (content - height).max(0.0))
}

/// Scrolls the rail to a row: [`near_offset`] after a step, or
/// [`open_offset`] as the drawer opens on it. Nothing when the row is not
/// drawn.
pub(crate) struct Reveal {
    row: iced::widget::Id,
    open: bool,
    viewport: Option<(Rectangle, Rectangle, iced::Vector)>,
    found: Option<Rectangle>,
    heading: Option<Rectangle>,
}

impl Reveal {
    /// The least scroll that shows the row `row` with the room under it.
    pub(crate) fn near(row: iced::widget::Id) -> Self {
        Reveal {
            row,
            open: false,
            viewport: None,
            found: None,
            heading: None,
        }
    }

    /// The drawer opening on the pull on the stage.
    pub(crate) fn open() -> Self {
        Reveal {
            open: true,
            ..Reveal::near(current_id())
        }
    }
}

impl Operation for Reveal {
    fn traverse(&mut self, operate: &mut dyn FnMut(&mut dyn Operation)) {
        operate(self);
    }

    fn container(&mut self, id: Option<&iced::widget::Id>, bounds: Rectangle) {
        if id == Some(&self.row) {
            self.found = Some(bounds);
        } else if self.open && id == Some(&current_night_id()) {
            self.heading = Some(bounds);
        }
    }

    fn scrollable(
        &mut self,
        id: Option<&iced::widget::Id>,
        bounds: Rectangle,
        content: Rectangle,
        translation: iced::Vector,
        _state: &mut dyn operation::Scrollable,
    ) {
        if id == Some(&scroll_id()) {
            self.viewport = Some((bounds, content, translation));
        }
    }

    fn finish(&self) -> Outcome<()> {
        let (Some((bounds, content, at)), Some(row)) = (self.viewport, self.found) else {
            return Outcome::None;
        };
        let (top, bottom) = (row.y - content.y, row.y + row.height - content.y);
        let to = if self.open {
            open_offset(
                at.y,
                bounds.height,
                content.height,
                (top, bottom),
                self.heading.map(|h| h.y - content.y),
            )
        } else {
            near_offset(at.y, bounds.height, content.height, top, bottom)
        };
        if (to - at.y).abs() < 0.5 {
            return Outcome::None;
        }
        Outcome::Chain(Box::new(ScrollTo { y: to }))
    }
}

/// Puts the rail's content `y` at the top of its viewport.
struct ScrollTo {
    y: f32,
}

impl Operation for ScrollTo {
    fn traverse(&mut self, operate: &mut dyn FnMut(&mut dyn Operation)) {
        operate(self);
    }

    fn scrollable(
        &mut self,
        id: Option<&iced::widget::Id>,
        _bounds: Rectangle,
        _content: Rectangle,
        _translation: iced::Vector,
        state: &mut dyn operation::Scrollable,
    ) {
        if id == Some(&scroll_id()) {
            state.scroll_to(iced::widget::operation::AbsoluteOffset {
                x: None,
                y: Some(self.y.max(0.0)),
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::window::testkit::simulator;
    use wowdps_model::Encounter;
    use wowdps_proto::history::{CardPlayer, KeyInfo};

    /// Local midnight of a date, as the log's clock writes it (a local-time
    /// epoch: `home::parse_ymd`'s days, read as the log's own).
    fn day(ymd: &str) -> i64 {
        crate::home::parse_ymd(ymd).unwrap()
    }
    const H: i64 = 3_600_000;
    const LOG: u64 = 0xabc;

    /// The night of the 26th: its evening, and the small hours after it.
    fn night(ymd: &str) -> i64 {
        night_of(day(ymd) + 20 * H)
    }

    fn entry(id: u64, kind: SegmentKind, name: &str, start: i64, ms: i64) -> ListEntry {
        ListEntry {
            id: SegmentId(id),
            row: ListRow {
                kind,
                name: name.to_string(),
                start_ms: start,
                success: None,
                duration_ms: ms,
                live: false,
                instance: None,
                pars_ms: None,
                arena: false,
                encounter: None,
            },
        }
    }

    /// A raid visit of the log, oldest first as the daemon lists it: its Σ,
    /// a kill, trash, a wipe — and the live trash after them.
    fn raid_night(start: i64) -> Vec<ListEntry> {
        let boss = |id, name: &str, at, ok| {
            let mut e = entry(id, SegmentKind::Encounter, name, start + at, 7 * 60_000);
            e.row.instance = Some(0);
            e.row.success = Some(ok);
            e.row.encounter = Some(Encounter {
                id: 3429,
                difficulty: 15,
                group_size: 25,
            });
            e
        };
        let mut sum = entry(
            1,
            SegmentKind::Overall,
            "The Venomous Abyss",
            start,
            31 * 60_000,
        );
        sum.row.instance = Some(0);
        let mut trash = entry(3, SegmentKind::Trash, "Trash", start + 10 * 60_000, 60_000);
        trash.row.instance = Some(0);
        let mut live = entry(
            5,
            SegmentKind::Trash,
            "Trash",
            start + 30 * 60_000,
            11 * 60_000,
        );
        live.row.instance = Some(0);
        live.row.live = true;
        vec![
            sum,
            boss(2, "The Coiled Altar", 60_000, true),
            trash,
            boss(4, "Ula'tek", 15 * 60_000, false),
            live,
        ]
    }

    fn card(id: &str, kind: FightKind, name: &str, start: i64, ms: i64) -> FightCard {
        FightCard {
            id: id.to_string(),
            log: 7,
            kind,
            name: name.to_string(),
            start_local_ms: start,
            start_utc_ms: start + 7 * H,
            duration_ms: ms,
            ..FightCard::default()
        }
    }

    /// `c` played by `who`, a `class`.
    fn owned(mut c: FightCard, who: &str, class: Class) -> FightCard {
        c.owner = Some(who.to_string());
        c.players = vec![CardPlayer {
            guid: who.to_string(),
            name: who.to_string(),
            class: Some(class),
            ..CardPlayer::default()
        }];
        c
    }

    fn in_map(mut c: FightCard, map_id: u32, difficulty: u32) -> FightCard {
        c.key = Some(KeyInfo {
            map_id,
            difficulty,
            ..KeyInfo::default()
        });
        c
    }

    fn build(entries: &[ListEntry], cards: &[FightCard], tonight: i64) -> Rail {
        Rail::build(&Sources {
            entries,
            log_id: Some(LOG),
            watched: None,
            cards,
            owner: Some((Some("Tranqlock-Proudmoore-US".to_string()), Class::Warlock)),
            tonight,
        })
    }

    fn names(v: &Visit) -> Vec<&str> {
        v.lines.iter().map(|l| l.name.as_str()).collect()
    }

    /// A night is a LOCAL date — the log's clock, which is the reader's —
    /// and a raid past midnight is one night: the small hours count with
    /// the evening before (the mcp's 06:00 cutover). "Tonight" is the one
    /// it is now, in the timezone the log was written in; the rest are
    /// named by weekday and date, the year only when it is not this one.
    #[test]
    fn nights_are_local_dates_with_the_small_hours_counted_with_the_evening() {
        let evening = day("2026-09-26") + 20 * H;
        let small_hours = day("2026-09-27") + 2 * H;
        let morning = day("2026-09-27") + 7 * H;
        assert_eq!(night_of(evening), night_of(small_hours), "one raid night");
        assert_ne!(night_of(small_hours), night_of(morning));
        assert_eq!(
            night_of(day("2026-09-26")),
            night_of(day("2026-09-25") + 20 * H)
        );
        // 04:00 UTC on the 28th is 21:00 on the 27th seven hours west: the
        // 27th's night, where UTC would call it the 28th's small hours
        // and so the 27th's too — and at 14:00 UTC it is the 28th in both.
        let now = day("2026-09-28") + 4 * H;
        assert_eq!(tonight(now, Some(-420)), night("2026-09-27"));
        assert_eq!(tonight(now + 10 * H, Some(-420)), night("2026-09-28"));
        assert_eq!(tonight(now + 10 * H, None), night("2026-09-28"));
        let tonight = night("2026-09-27");
        assert_eq!(night_label(tonight, tonight), "Tonight");
        assert_eq!(
            night_label(night("2026-09-26"), tonight),
            "Saturday, Sep 26"
        );
        assert_eq!(night_label(night("2026-09-25"), tonight), "Friday, Sep 25");
        assert_eq!(
            night_label(night("2025-12-31"), tonight),
            "Wednesday, Dec 31, 2025"
        );
        assert_eq!(civil(night("2026-09-26")), (2026, 9, 26));
        // Pulls on either side of midnight land under one heading.
        let cards = vec![
            card("a", FightKind::Encounter, "Late", small_hours, 60_000),
            card("b", FightKind::Encounter, "Early", evening, 60_000),
            card("c", FightKind::Encounter, "Next day", morning, 60_000),
        ];
        let rail = build(&[], &cards, tonight);
        let labels: Vec<&str> = rail.nights.iter().map(|n| n.label.as_str()).collect();
        assert_eq!(labels, ["Tonight", "Saturday, Sep 26"]);
        let heads: Vec<Vec<&str>> = rail
            .nights
            .iter()
            .map(|n| n.visits.iter().flat_map(names).collect())
            .collect();
        assert_eq!(heads, [vec!["Next day"], vec!["Late", "Early"]]);
    }

    /// Tonight's log is a visit of its instance and difficulty, its pulls
    /// newest first — the live one on top with its red dot — the visit's Σ
    /// last, "Whole visit" (a Σ still while the visit goes on), and trash
    /// the quiet dash, called trash whatever the engine named it. A pull
    /// still going makes its night "Tonight" whatever the clock.
    #[test]
    fn the_log_lists_its_pulls_newest_first_and_its_sum_last() {
        let start = day("2026-09-27") + 19 * H;
        let mut entries = raid_night(start);
        entries[0].row.live = true;
        entries[4].row.name = "Ula'tek".to_string();
        let rail = build(&entries, &[], night("2020-01-01"));
        assert_eq!(rail.nights.len(), 1);
        let n = &rail.nights[0];
        assert_eq!(n.label, "Tonight", "a live pull's night");
        let v = &n.visits[0];
        assert_eq!(v.title, "The Venomous Abyss, Heroic");
        assert_eq!(
            names(v),
            [
                "Trash",
                "Ula'tek",
                "Trash",
                "The Coiled Altar",
                "Whole visit"
            ]
        );
        let marks: Vec<Mark> = v.lines.iter().map(|l| l.mark).collect();
        assert_eq!(
            marks,
            [Mark::Live, Mark::Bad, Mark::Dash, Mark::Good, Mark::Sum]
        );
        assert!(v.lines[0].trash && !v.lines[1].trash);
        // The log is the owner's: the visit wears their dot, the rows none.
        assert_eq!(
            v.dot,
            Some(Who {
                color: theme::class_rgb(Class::Warlock),
                name: "Tranqlock-Proudmoore-US".to_string(),
            }),
            "the window's owner: their colour, and their name on its tip"
        );
        assert!(v.lines.iter().all(|l| l.dot.is_none()));
        // Out in the world, a stray pull is a visit of its own kind; one
        // the daemon filed under no visit but inside a visit's span is the
        // visit's.
        let mut world = raid_night(start);
        world.push(entry(
            6,
            SegmentKind::Trash,
            "Trash",
            start + 50 * 60_000,
            30_000,
        ));
        world.insert(
            3,
            entry(7, SegmentKind::Trash, "Trash", start + 12 * 60_000, 60_000),
        );
        // An arena match out in the world is an arena's, not the world's.
        let mut arena = entry(
            8,
            SegmentKind::Encounter,
            "Nagrand Arena",
            start + 55 * 60_000,
            180_000,
        );
        arena.row.arena = true;
        arena.row.success = Some(true);
        world.push(arena);
        let rail = build(&world, &[], night("2026-09-27"));
        let titles: Vec<&str> = rail.nights[0]
            .visits
            .iter()
            .map(|v| v.title.as_str())
            .collect();
        assert_eq!(
            titles,
            ["Arena", "Open world", "The Venomous Abyss, Heroic"]
        );
        let raid = &rail.nights[0].visits[2];
        assert_eq!(raid.lines.len(), 6, "the stray inside it joined it");
        assert_eq!(raid.lines[2].pull, Pull::Log(SegmentId(7)));
    }

    /// A stored card that is also a segment of the tailed log — its id is
    /// the log's id and the row's start (a Σ's with its mark) — is listed
    /// once, as the log's, and lends the row what only the store knows: a
    /// wipe's best health, whose pull it was. Another log's cards stand
    /// apart.
    #[test]
    fn a_card_of_the_log_s_own_pull_is_listed_once() {
        let start = day("2026-09-27") + 19 * H;
        let entries = raid_night(start);
        let row = |i: usize| &entries[i].row;
        let mut wipe = card(
            &fight_id(LOG, row(3).start_ms, false),
            FightKind::Encounter,
            "Ula'tek",
            row(3).start_ms,
            row(3).duration_ms,
        );
        wipe.success = Some(false);
        wipe.best_pct = Some(56);
        let wipe = owned(wipe, "Player-1-9", Class::Warlock);
        let sum = card(
            &fight_id(LOG, row(0).start_ms, true),
            FightKind::Overall,
            "The Venomous Abyss",
            row(0).start_ms,
            row(0).duration_ms,
        );
        let elsewhere = card(
            "other-1",
            FightKind::Encounter,
            "Ula'tek",
            start - 24 * H,
            60_000,
        );
        let wipe_id = wipe.id.clone();
        let cards = vec![wipe, sum, elsewhere];
        let rail = build(&entries, &cards, night("2026-09-27"));
        let log: Vec<&Line> = rail.nights[0].visits[0].lines.iter().collect();
        assert_eq!(log.len(), 5, "the log's own, each once");
        assert!(
            rail.nights[0].visits.len() == 1,
            "no stored visit beside it"
        );
        let ula = log.iter().find(|l| l.name == "Ula'tek").unwrap();
        assert_eq!(ula.pull, Pull::Log(SegmentId(4)));
        assert_eq!(ula.best_pct, Some(56), "the card's best health");
        assert_eq!(rail.nights.len(), 2, "another log's night");
        assert_eq!(
            rail.nights[1].visits[0].lines[0].pull,
            Pull::Stored("other-1".to_string())
        );
        // With no log id nothing can be matched: the same cards stand apart,
        // the wipe's card a stored pull beside the log's own row for it.
        let rail = Rail::build(&Sources {
            entries: &entries,
            log_id: None,
            watched: None,
            cards: &cards,
            owner: None,
            tonight: night("2026-09-27"),
        });
        assert_eq!(rail.lines().count(), 8, "five of the log's, three cards");
        assert!(rail.line(&Pull::Log(SegmentId(4))).is_some());
        assert!(
            rail.line(&Pull::Stored(wipe_id.clone()))
                .is_some_and(|l| l.best_pct == Some(56)),
            "the wipe's card, on its own"
        );
        assert_eq!(
            rail.line(&Pull::Log(SegmentId(4))).and_then(|l| l.best_pct),
            None,
            "lending nothing to a row it is not paired with"
        );
    }

    /// The store's night: a raid visit (its pulls on its map, newest first,
    /// its Σ last), the night's keys and dungeon runs as one visit — a
    /// key's zone-in left out, the key being the run — with "+N" on a
    /// timed key and "over" on a depleted one, and a delve's boss under
    /// "Delve". Several characters' pulls wear a dot each; one character's
    /// visit wears it once.
    #[test]
    fn a_stored_night_is_its_raid_its_keys_and_its_delve() {
        let d = day("2026-09-26");
        let raid = in_map(
            card(
                "r-s",
                FightKind::Overall,
                "The Venomous Abyss",
                d + 19 * H,
                30 * 60_000,
            ),
            3004,
            15,
        );
        let boss = |id: &str, at: i64, ok: bool, pct: Option<u16>| {
            let mut c = card(
                id,
                FightKind::Encounter,
                "The Lost Explorers",
                d + at,
                400_000,
            );
            c.success = Some(ok);
            c.best_pct = pct;
            c.encounter = Some(Encounter {
                id: 3497,
                difficulty: 15,
                group_size: 20,
            });
            owned(c, "Player-1-2", Class::DeathKnight)
        };
        let key = |id: &str, name: &str, at: i64, ok: bool, who: &str, class| {
            let mut c = card(id, FightKind::Key, name, d + at, 1_600_000);
            c.success = Some(ok);
            c.official_ms = Some(1_600_000);
            c.pars_ms = Some((1_800_000, 1_440_000, 1_080_000));
            owned(in_map(c, 2825, 23), who, class)
        };
        let stub = in_map(
            card(
                "z-s",
                FightKind::Overall,
                "Den of Nalorakk",
                d + 13 * H,
                60_000,
            ),
            2825,
            23,
        );
        let pools = owned(
            in_map(
                card(
                    "p-s",
                    FightKind::Overall,
                    "Ruby Life Pools",
                    d + 16 * H,
                    1_338_000,
                ),
                2521,
                8,
            ),
            "Player-1-3",
            Class::DemonHunter,
        );
        let mut delve = card("dv", FightKind::Encounter, "Drakta", d + 11 * H, 69_000);
        delve.success = Some(true);
        delve.encounter = Some(Encounter {
            id: 3535,
            difficulty: 208,
            group_size: 2,
        });
        // An arena match in the same log after the raid: it was in no
        // visit (the store keeps none for it), so it joins none.
        let mut arena = card(
            "ar",
            FightKind::Arena,
            "Nagrand Arena",
            d + 19 * H + 40 * 60_000,
            180_000,
        );
        arena.success = Some(true);
        let cards = vec![
            owned(raid, "Player-1-2", Class::DeathKnight),
            boss("b1", 19 * H + 60_000, false, Some(97)),
            in_map(boss("b2", 19 * H + 20 * 60_000, false, Some(2)), 3004, 15),
            key(
                "k1",
                "Den of Nalorakk +14",
                13 * H + 90_000,
                true,
                "Player-1-3",
                Class::DemonHunter,
            ),
            key(
                "k2",
                "Kings' Rest +14",
                14 * H,
                false,
                "Player-1-3",
                Class::DemonHunter,
            ),
            key(
                "k3",
                "Altar of Fangs +14",
                17 * H,
                true,
                "Player-1-4",
                Class::Warlock,
            ),
            stub,
            pools,
            delve,
            arena,
        ];
        let rail = build(&[], &cards, night("2026-09-27"));
        assert_eq!(rail.nights.len(), 1);
        let n = &rail.nights[0];
        assert_eq!(n.label, "Saturday, Sep 26");
        let titles: Vec<&str> = n.visits.iter().map(|v| v.title.as_str()).collect();
        assert_eq!(
            titles,
            [
                "Arena",
                "The Venomous Abyss, Heroic",
                "Mythic+ keys",
                "Delve"
            ]
        );
        let raid = &n.visits[1];
        assert_eq!(
            names(raid),
            ["The Lost Explorers", "The Lost Explorers", "Whole visit"]
        );
        assert_eq!(raid.lines[0].best_pct, Some(2), "newest first");
        assert_eq!(raid.lines[2].mark, Mark::Sum);
        assert_eq!(
            raid.dot.as_ref().map(|w| w.color),
            Some(theme::class_rgb(Class::DeathKnight))
        );
        let keys = &n.visits[2];
        assert_eq!(
            names(keys),
            [
                "Altar of Fangs +14",
                "Ruby Life Pools",
                "Kings' Rest +14",
                "Den of Nalorakk +14"
            ],
            "the zone-in is the key's, not a run"
        );
        let words: Vec<Option<KeyWord>> = keys.lines.iter().map(|l| l.key).collect();
        assert_eq!(
            words,
            [
                Some(KeyWord::Plus(1)),
                None,
                Some(KeyWord::Over),
                Some(KeyWord::Plus(1))
            ]
        );
        assert_eq!(keys.lines[1].mark, Mark::Dash, "a run with no verdict");
        assert_eq!(keys.dot, None, "two characters played the keys");
        assert_eq!(
            keys.lines[0].dot,
            Some(Who {
                color: theme::class_rgb(Class::Warlock),
                name: "Player-1-4".to_string(),
            }),
            "a card's owner by the name the card gives them"
        );
        assert_eq!(
            keys.lines[2].dot.as_ref().map(|w| w.color),
            Some(theme::class_rgb(Class::DemonHunter))
        );
        assert_eq!(n.visits[3].lines[0].mark, Mark::Good);
        assert_eq!(n.visits[3].dot, None, "nobody said whose");
    }

    /// A stored pull of the tailed log that its list does not hold — a copy
    /// of the log cut short — is listed in the log's visit it followed, not
    /// as a visit of its own. A raid pull no visit claims is named for its
    /// instance: after the log's visit that night at its difficulty, else
    /// after any Σ card of its map in hand, and "Raid, Heroic" only when
    /// nothing names it.
    #[test]
    fn a_raid_pull_no_visit_claims_is_named_for_its_instance() {
        let start = day("2026-09-27") + 19 * H;
        let entries = raid_night(start);
        let boss = |id: &str, log: u64, at: i64, map: Option<u32>| {
            let mut c = card(id, FightKind::Encounter, "Ula'tek", at, 400_000);
            c.log = log;
            c.success = Some(false);
            c.encounter = Some(Encounter {
                id: 3429,
                difficulty: 15,
                group_size: 25,
            });
            match map {
                Some(m) => in_map(c, m, 15),
                None => c,
            }
        };
        let late = boss("late", LOG, start + 45 * 60_000, Some(3004));
        let other = boss("other", 9, start + 50 * 60_000, None);
        let rail = build(&entries, &[late, other], night("2026-09-27"));
        assert_eq!(rail.nights.len(), 1);
        let titles: Vec<&str> = rail.nights[0]
            .visits
            .iter()
            .map(|v| v.title.as_str())
            .collect();
        assert_eq!(
            titles,
            ["The Venomous Abyss, Heroic", "The Venomous Abyss, Heroic"],
            "no \"Raid, Heroic\" where the night names the instance"
        );
        let log = rail.nights[0]
            .visits
            .iter()
            .find(|v| v.lines.iter().any(|l| l.pull == Pull::Log(SegmentId(1))))
            .expect("the log's visit");
        assert_eq!(
            log.lines.first().map(|l| &l.pull),
            Some(&Pull::Stored("late".to_string())),
            "the log's own card, newest, in the log's visit"
        );
        assert_eq!(log.lines.last().map(|l| l.mark), Some(Mark::Sum));
        // Another night: named by a Σ card of its map from any night…
        let earlier = day("2026-09-20") + 20 * H;
        let sigma = in_map(
            card(
                "sigma",
                FightKind::Overall,
                "The Venomous Abyss",
                day("2026-09-26") + 19 * H,
                60_000,
            ),
            3004,
            15,
        );
        let mapped = boss("mapped", 5, earlier, Some(3004));
        let bare = boss("bare", 6, earlier - 3 * 24 * H, None);
        let rail = build(&[], &[sigma, mapped, bare], night("2026-09-27"));
        let title_of = |id: &str| {
            rail.nights
                .iter()
                .flat_map(|n| n.visits.iter())
                .find(|v| {
                    v.lines
                        .iter()
                        .any(|l| l.pull == Pull::Stored(id.to_string()))
                })
                .map(|v| v.title.clone())
        };
        assert_eq!(
            title_of("mapped").as_deref(),
            Some("The Venomous Abyss, Heroic")
        );
        // …and only with nothing to name it, by the kind of content.
        assert_eq!(title_of("bare").as_deref(), Some("Raid, Heroic"));
    }

    /// `[` walks down the rail, `]` up it, across nights and visits, over
    /// the rows the trash toggle leaves; from a row it hides, the nearest
    /// shown one; from nowhere, the top; past either end, nothing.
    #[test]
    fn the_steps_walk_the_rail_over_what_it_shows() {
        let start = day("2026-09-27") + 19 * H;
        let cards = vec![card(
            "old",
            FightKind::Encounter,
            "Ula'tek",
            start - 24 * H,
            60_000,
        )];
        let rail = build(&raid_night(start), &cards, night("2026-09-27"));
        let order: Vec<Pull> = rail.lines().map(|l| l.pull.clone()).collect();
        let log = |i| Pull::Log(SegmentId(i));
        assert_eq!(
            order,
            [
                log(5),
                log(4),
                log(3),
                log(2),
                log(1),
                Pull::Stored("old".to_string())
            ]
        );
        assert_eq!(rail.step(None, true, false), Some(log(5)), "the top");
        assert_eq!(rail.step(Some(&log(4)), true, false), Some(log(3)));
        assert_eq!(rail.step(Some(&log(4)), false, false), Some(log(5)));
        assert_eq!(rail.step(Some(&log(5)), false, false), None, "the top");
        assert_eq!(
            rail.step(Some(&log(1)), true, false),
            Some(Pull::Stored("old".to_string())),
            "into the store's nights"
        );
        assert_eq!(
            rail.step(Some(&Pull::Stored("old".to_string())), true, false),
            None
        );
        // Trash hidden: over it, and from it.
        assert_eq!(rail.step(Some(&log(4)), true, true), Some(log(2)));
        assert_eq!(rail.step(None, true, true), Some(log(4)), "the top shown");
        assert_eq!(rail.step(Some(&log(3)), true, true), Some(log(2)));
        assert_eq!(rail.step(Some(&log(3)), false, true), Some(log(4)));
        // Where H opens it: the first night that is not tonight.
        assert_eq!(rail.earlier(), Some(1));
        assert!(rail.line(&log(2)).is_some_and(|l| l.mark == Mark::Good));
    }

    /// The rail draws its nights, its visits and their pulls: the current
    /// pull lit, trash left out when the toggle says so (but the pull on
    /// the stage kept), what a row says at its right, and the button for
    /// older nights — or the word that they are coming.
    #[test]
    fn the_panel_draws_what_the_rail_holds() {
        let start = day("2026-09-27") + 19 * H;
        let mut wipe = card(
            "w",
            FightKind::Encounter,
            "The Lost Explorers",
            start - 24 * H,
            454_000,
        );
        wipe.success = Some(false);
        wipe.best_pct = Some(97);
        let rail = build(&raid_night(start), &[wipe], night("2026-09-27"));
        let shown = |at: Option<Pull>, hide_trash, more| Shown {
            at,
            cursor: None,
            hide_trash,
            more,
            accent: theme::GOLD_ACCENT,
            hide_realms: false,
        };
        let mut ui = simulator(panel(&rail, &shown(None, false, More::Offer), RAIL_W));
        for words in [
            "Pulls",
            "Hide trash",
            "Tonight",
            "Saturday, Sep 26",
            "The Venomous Abyss, Heroic",
            "The Coiled Altar",
            "Whole visit",
            "Trash",
            "97%",
            "7:34",
            "Show older nights",
        ] {
            assert!(ui.find(words).is_ok(), "{words}");
        }
        ui.click("The Coiled Altar").unwrap();
        ui.click("Hide trash").unwrap();
        ui.click("Show older nights").unwrap();
        let sent: Vec<Message> = ui.into_messages().collect();
        assert!(matches!(
            sent.as_slice(),
            [
                Message::Pull(Pull::Log(SegmentId(2))),
                Message::HideTrash,
                Message::OlderNights
            ]
        ));
        // Trash hidden: gone, but for the pull on the stage.
        let mut ui = simulator(panel(
            &rail,
            &shown(Some(Pull::Log(SegmentId(3))), true, More::Asking),
            RAIL_W,
        ));
        assert!(ui.find(current_id()).is_ok(), "the pull on the stage");
        assert!(ui.find("Reading the history store…").is_ok());
        assert!(ui.find("Show older nights").is_err());
        let mut ui = simulator(panel(&rail, &shown(None, true, More::None), RAIL_W));
        assert!(ui.find("Trash").is_err(), "hidden");
        assert!(ui.find(current_id()).is_err());
        assert!(ui.find("Reading the history store…").is_err());
        // An empty rail says why it is empty.
        let mut ui = simulator(panel(
            &Rail::default(),
            &shown(None, false, More::None),
            RAIL_W,
        ));
        assert!(ui.find(EMPTY).is_ok());
        // The drawer's keys ring their row, and a pull under a second says
        // so rather than a clock that reads as missing.
        let mut ui = simulator(panel(
            &rail,
            &Shown {
                cursor: Some(Pull::Log(SegmentId(2))),
                ..shown(None, false, More::None)
            },
            RAIL_W,
        ));
        assert!(ui.find(cursor_id()).is_ok(), "the keys' row");
        assert_eq!(clock(0), "<0:01");
        assert_eq!(clock(999), "<0:01");
        assert_eq!(clock(1_000), "0:01");
    }

    /// A step shows its row with the prototype's 40 px under it, the least
    /// scroll that does; the drawer opening on a row it must scroll to
    /// stands its night's heading at the top when both fit, else centres
    /// it; a row in sight stays where it is either way; and neither
    /// scrolls past the list's ends.
    #[test]
    fn the_rail_scrolls_to_a_row_with_room_under_it() {
        let (h, content) = (500.0, 2_000.0);
        // In sight with its room: no move.
        assert_eq!(near_offset(0.0, h, content, 100.0, 128.0), 0.0);
        // Past the bottom: 40 px under it.
        assert_eq!(near_offset(0.0, h, content, 480.0, 508.0), 48.0);
        // Above the top: at the top.
        assert_eq!(near_offset(300.0, h, content, 200.0, 228.0), 200.0);
        // Near the end: no further than the list goes.
        assert_eq!(near_offset(0.0, h, content, 1_972.0, 2_000.0), 1_500.0);
        // Opening: its heading at the top when both fit…
        assert_eq!(
            open_offset(0.0, h, content, (900.0, 928.0), Some(700.0)),
            700.0
        );
        // …else the row in the middle…
        assert_eq!(
            open_offset(0.0, h, content, (900.0, 928.0), Some(100.0)),
            664.0
        );
        assert_eq!(open_offset(0.0, h, content, (900.0, 928.0), None), 664.0);
        // …and a row already in sight stays put.
        assert_eq!(
            open_offset(0.0, h, content, (100.0, 128.0), Some(12.0)),
            0.0
        );
        assert_eq!(
            open_offset(0.0, h, 300.0, (200.0, 228.0), Some(0.0)),
            0.0,
            "a short list never scrolls"
        );
    }
}
