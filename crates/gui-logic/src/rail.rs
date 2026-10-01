//! The pull rail's model (the prototype's `.rail`): ONE list of pulls,
//! tonight's log and every stored night. Nights by their local date
//! ("Tonight", "Saturday, Sep 26", a 06:00 cutover so a raid past midnight
//! is one night), the visits in each (an instance and its difficulty, your
//! keys, a delve) wearing a dot in the colour of the character who played
//! them, and the pulls newest first, a visit's Σ last. Tonight comes from
//! the daemon's segment list; the earlier nights from the history store's
//! pages (`history::Earlier`). A stored card that is ALSO a segment of the
//! tailed log — its id is the log's id and the row's start — is listed
//! once, as the log's, and lends that row what only the store knows.
//!
//! Moved from the iced window's `rail.rs` (plan 3.4, wave B); both GUIs
//! draw from it.

use std::collections::{HashMap, HashSet};

use wowdps_model::fmt::{duration, key_tier};
use wowdps_model::{Class, ListRow, SegmentId, SegmentKind, difficulty_name};
use wowdps_proto::ListEntry;
use wowdps_proto::history::{FightCard, FightKind, fight_id};

use crate::theme::Color;

/// A night starts at 06:00 local, the cutover the mcp's local buckets
/// default to: a raid that runs past midnight is one night, not two.
const CUTOVER_MS: i64 = 6 * 3_600_000;
const DAY_MS: i64 = 86_400_000;
/// A dungeon's visit that a key on the same map starts within this long of
/// is the key's zone-in (the minutes before CHALLENGE_MODE_START), not a
/// visit of its own: the key's card is the run.
pub const ZONE_IN_MS: i64 = 10 * 60_000;

/// Is the rail beside the stage, at a window this wide? At 1180 and under
/// it is a drawer (`@container app (max-width: 1180px)`).
pub fn docked(window: f32) -> bool {
    window > crate::theme::TILE_WINDOW
}

// ---- the model -----------------------------------------------------------------

/// Where a row leads: a segment of the tailed log, by the daemon's id for
/// it, or a stored fight, by the store's.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Pull {
    Log(SegmentId),
    Stored(String),
}

/// A row's lead glyph (`.oc`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mark {
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
pub enum KeyWord {
    Plus(u8),
    Over,
}

/// One pull on the rail.
#[derive(Debug, Clone, PartialEq)]
pub struct Line {
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
    pub start_ms: i64,
    /// Whose it was: what decides where the dot goes.
    pub owner: Option<Owner>,
}

impl Line {
    /// Does the rail draw this pull? Trash goes when the toggle says so —
    /// but never the pull on the stage (`at`), nor the row the drawer's
    /// keys are on (`cursor`), which keep their places.
    pub fn shown(&self, hide_trash: bool, at: Option<&Pull>, cursor: Option<&Pull>) -> bool {
        !(hide_trash && self.trash) || at == Some(&self.pull) || cursor == Some(&self.pull)
    }
}

/// A character a dot stands for: their class colour, and the name the
/// dot's tip says — colour alone cannot tell two alts of a class apart.
#[derive(Debug, Clone, PartialEq)]
pub struct Who {
    pub color: Color,
    pub name: String,
}

/// Whose a pull was: their guid (the tailed log's logger, when no card
/// names them, is ""), their name and their class.
#[derive(Debug, Clone, PartialEq)]
pub struct Owner {
    pub guid: String,
    pub name: String,
    pub class: Option<Class>,
}

/// A visit: an instance and its difficulty, your keys, a delve.
#[derive(Debug, Clone, PartialEq)]
pub struct Visit {
    pub title: String,
    /// The one character who played every pull in it, as their dot.
    pub dot: Option<Who>,
    /// Newest first; a whole visit's Σ last.
    pub lines: Vec<Line>,
}

/// A night: its visits, newest first.
#[derive(Debug, Clone, PartialEq)]
pub struct Night {
    /// Days since the epoch, of the local date the night began on.
    pub day: i64,
    pub label: String,
    /// It is tonight: the clock's night, or one with a pull still going.
    pub tonight: bool,
    pub visits: Vec<Visit>,
}

/// The rail, derived: every night the log and the store's pages hold.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Rail {
    pub nights: Vec<Night>,
}

/// What the rail is made from.
pub struct Sources<'a> {
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
pub fn night_of(local_ms: i64) -> i64 {
    (local_ms - CUTOVER_MS).div_euclid(DAY_MS)
}

/// The night it is now, at `now_utc_ms`, in the timezone the log writes
/// (`tz_min` east of UTC; the store's cards carry it). `None` reads UTC.
pub fn tonight(now_utc_ms: i64, tz_min: Option<i16>) -> i64 {
    night_of(now_utc_ms + i64::from(tz_min.unwrap_or(0)) * 60_000)
}

/// Days since the epoch as a civil date (Howard Hinnant's civil-from-days,
/// the inverse of days-from-civil — `home::parse_ymd`'s milliseconds over
/// [`DAY_MS`]): year, month 1–12, day 1–31.
pub fn civil(days: i64) -> (i64, u32, u32) {
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
pub fn month_name(m: u32, short: bool) -> &'static str {
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
pub fn weekday(day: i64) -> &'static str {
    // 1970-01-01 was a Thursday.
    WEEKDAYS
        .get((day + 4).rem_euclid(7) as usize)
        .copied()
        .unwrap_or_default()
}

/// A night's heading: "Tonight", else its weekday and date — "Saturday,
/// Sep 26" — with the year only when it is not tonight's.
pub fn night_label(day: i64, tonight: i64) -> String {
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
pub fn night_short(day: i64, tonight: i64) -> String {
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
pub fn is_raid(d: u32) -> bool {
    matches!(d, 3..=7 | 9 | 14..=17 | 33 | 151)
}
pub fn is_dungeon(d: u32) -> bool {
    matches!(d, 1 | 2 | 8 | 23 | 24)
}
pub const DELVE: u32 = 208;

/// A character's dot: their class colour as data, as a bar is, and their
/// name — none for a character whose class nobody saw.
fn dot_of(o: &Owner) -> Option<Who> {
    o.class.map(|class| Who {
        color: Color::of_class(class),
        name: o.name.clone(),
    })
}

impl Rail {
    /// The rail over `src`.
    pub fn build(src: &Sources) -> Self {
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
    pub fn lines(&self) -> impl Iterator<Item = &Line> {
        self.nights
            .iter()
            .flat_map(|n| n.visits.iter())
            .flat_map(|v| v.lines.iter())
    }

    /// The pull one row down the rail (`older`) or up it from `at`, over
    /// the rows `hide_trash` leaves. From a row it does not hold — a
    /// hidden one keeps its place — or from nowhere, the top. `None` at the
    /// end.
    pub fn step(&self, at: Option<&Pull>, older: bool, hide_trash: bool) -> Option<Pull> {
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
    pub fn earlier(&self) -> Option<usize> {
        self.nights.iter().position(|n| !n.tonight)
    }

    /// The night `pull` is listed under, by its place on the rail.
    pub fn night_of_pull(&self, pull: &Pull) -> Option<usize> {
        self.nights.iter().position(|n| {
            n.visits
                .iter()
                .any(|v| v.lines.iter().any(|l| l.pull == *pull))
        })
    }

    /// The row for `pull`, when the rail holds it.
    pub fn line(&self, pull: &Pull) -> Option<&Line> {
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
pub fn log_instances(entries: &[ListEntry], log_id: Option<u64>) -> Vec<LogInstance> {
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
pub struct LogInstance {
    pub log: u64,
    /// Its first pull's start to its last one's end, on the log's clock.
    pub lo: i64,
    pub hi: i64,
    /// "The Venomous Abyss".
    pub name: String,
}

impl LogInstance {
    /// `c` was pulled in this visit: its log's, begun within its span.
    pub fn holds(&self, c: &FightCard) -> bool {
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

/// What the rail says with nothing to list.
pub const EMPTY: &str =
    "No pulls yet. Pulls appear here as you fight, and earlier nights load from your history.";

/// Whether more of the store can be asked for, and whether it is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum More {
    /// The store is all in hand (or off): no button.
    None,
    /// "Show older nights".
    Offer,
    /// A page is on its way.
    Asking,
}

/// A pull's clock on its row: a pull under a second — a boss reset, a
/// pull the game called off at once — says so, where "0:00" would read as
/// a value that failed to arrive.
pub fn clock(ms: i64) -> String {
    if ms < 1000 {
        "<0:01".to_string()
    } else {
        duration(ms)
    }
}

/// The room the rail leaves under a row it scrolls to (the prototype's
/// `near()`: `pad = 40` past the bottom): the row shows with the rows after
/// it, not flush with the rail's last line.
pub const NEAR_PAD: f32 = 40.0;

/// Where the rail's list stands to show the row `[top, bottom]` (its
/// content's coordinates) from `offset`, in a viewport `height` tall over
/// content `content` tall: the least move that shows it with [`NEAR_PAD`]
/// under it — a row above the viewport comes to its top (`near()`).
pub fn near_offset(offset: f32, height: f32, content: f32, top: f32, bottom: f32) -> f32 {
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
pub fn open_offset(
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

/// The log, its visits and the store's cards the rail's tests build on, in
/// every crate that draws a rail.
#[doc(hidden)]
pub mod samples {
    use super::*;
    use wowdps_model::Encounter;
    use wowdps_proto::history::{CardPlayer, KeyInfo};

    /// Local midnight of a date, as the log's clock writes it (a local-time
    /// epoch: `home::parse_ymd`'s days, read as the log's own).
    pub fn day(ymd: &str) -> i64 {
        crate::home::parse_ymd(ymd).unwrap_or_default()
    }
    pub const H: i64 = 3_600_000;
    pub const LOG: u64 = 0xabc;

    /// The night of the 26th: its evening, and the small hours after it.
    pub fn night(ymd: &str) -> i64 {
        night_of(day(ymd) + 20 * H)
    }

    pub fn entry(id: u64, kind: SegmentKind, name: &str, start: i64, ms: i64) -> ListEntry {
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
    pub fn raid_night(start: i64) -> Vec<ListEntry> {
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

    pub fn card(id: &str, kind: FightKind, name: &str, start: i64, ms: i64) -> FightCard {
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
    pub fn owned(mut c: FightCard, who: &str, class: Class) -> FightCard {
        c.owner = Some(who.to_string());
        c.players = vec![CardPlayer {
            guid: who.to_string(),
            name: who.to_string(),
            class: Some(class),
            ..CardPlayer::default()
        }];
        c
    }

    pub fn in_map(mut c: FightCard, map_id: u32, difficulty: u32) -> FightCard {
        c.key = Some(KeyInfo {
            map_id,
            difficulty,
            ..KeyInfo::default()
        });
        c
    }

    pub fn build(entries: &[ListEntry], cards: &[FightCard], tonight: i64) -> Rail {
        Rail::build(&Sources {
            entries,
            log_id: Some(LOG),
            watched: None,
            cards,
            owner: Some((Some("Tranqlock-Proudmoore-US".to_string()), Class::Warlock)),
            tonight,
        })
    }

    pub fn names(v: &Visit) -> Vec<&str> {
        v.lines.iter().map(|l| l.name.as_str()).collect()
    }
}

#[cfg(test)]
mod tests;
