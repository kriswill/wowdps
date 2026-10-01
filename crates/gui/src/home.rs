//! Home (the prototype's `.home`): the reader's week, over the daemon's
//! history store.
//!
//! Home is window-local, exactly like the talent viewer — never a `Screen`
//! variant, so `ClientState` (and with it the TUI and the overlay) never
//! learns it exists. Everything on it is derived here, client-side, from
//! the `HistoryQuery::Fights` answers the daemon already serves: each
//! card's "me" is the scoped character's row on it, or — scoped to all of
//! them — the row of the card's owner.
//!
//! It opens on the question a reader asks after a raid, "how did last night
//! go for me": the night's pulls with their rank in the role, and how that
//! rank moved across the night. Under it, the week: keys against their
//! timers, raid progress with one dot per pull, and key throughput across
//! characters. Scope is a chip row — all characters, or one — never a lock:
//! the config's `character` is only the scope Home opens on.
//!
//! Two rules run through the whole module:
//!
//! - **A number we cannot derive is not drawn.** No card carries a raid's
//!   boss roster or a Mythic+ rating, so there is no "N / 8" and no score;
//!   a wipe nobody saw the boss's health on says "No kill", never "0%".
//! - **Paging is transport.** The reader never sees a page: Home asks for
//!   the store's newest cards, one request in flight at a time, until the
//!   week is in hand.

use std::collections::BTreeMap;

use iced::Element;

use wowdps_model::fmt::key_tier;
use wowdps_model::{Class, Role, Spec, difficulty_name};
use wowdps_proto::history::{CardPlayer, FightCard, FightKind};
use wowdps_proto::{ClientMsg, FightSort, HistoryAnswer, HistoryQuery};

use crate::config::Config;
use crate::nav;
use crate::rail::{self, Mark};
use crate::theme;

mod charts;
mod panels;

/// Cards per request: the history pages', which Home reads too.
pub(crate) use crate::history::PAGE;

/// Requests one opening of Home makes before it stops: a week of stored
/// trash in the thousands is not read to the end, and the screen says so.
pub(crate) const MAX_PAGES: u32 = 10;

/// A week, measured back from the store's newest card rather than from
/// now: Home opened on Tuesday about Saturday's raid should not empty
/// itself.
pub(crate) const WEEK_MS: i64 = 7 * 86_400_000;

/// Home's widest scope, as its first chip (and the palette) says it.
pub(crate) const ALL_CHARACTERS: &str = "All characters";

/// Window-local Home state.
#[derive(Debug, Default)]
pub(crate) struct Home {
    /// Cards accumulated across requests, newest first, deduped by id.
    pub cards: Vec<FightCard>,
    /// The in-flight `GetHistory` req_id. `Some` means a request is out and
    /// no second one may be sent — the rule that keeps Home from flooding
    /// the history queue a closing pull needs.
    pub pending: Option<u32>,
    /// `total` from the last answer: how many cards matched before `limit`.
    pub total: Option<u32>,
    /// Requests made since Home opened.
    pub pages: u32,
    /// The last card id of the newest answer, for `after_id`.
    pub cursor: Option<String>,
    /// Whose week the screen shows: one character's guid, or `None` for
    /// every character the store names as yours.
    pub scope: Option<String>,
    /// At least one answer landed — what tells "still loading" from "the
    /// store is empty".
    pub answered: bool,
    /// From `Status`: the store is off, and why. An empty answer means
    /// something quite different when it is set.
    pub disabled_reason: Option<String>,
    /// From `Status`: writes or reads the daemon dropped. A dashboard over a
    /// store that lost something must say so rather than imply completeness.
    pub dropped: u32,
}

impl Home {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// Everything the store matched is in hand.
    pub(crate) fn complete(&self) -> bool {
        self.total.is_some_and(|t| self.cards.len() as u32 >= t)
    }

    /// The week is in hand: the oldest card held began more than a week
    /// before the newest, so no page after it can hold one of the week's.
    pub(crate) fn week_read(&self) -> bool {
        let newest = self.cards.iter().map(|c| c.start_utc_ms).max();
        let oldest = self.cards.iter().map(|c| c.start_utc_ms).min();
        newest
            .zip(oldest)
            .is_some_and(|(n, o)| o < n.saturating_sub(WEEK_MS))
    }

    /// The reads stopped before the week was whole: [`MAX_PAGES`] asked
    /// and the store still holding more of it.
    pub(crate) fn stalled(&self) -> bool {
        self.pending.is_none() && self.pages >= MAX_PAGES && !self.complete() && !self.week_read()
    }

    /// The next request to send, or `None` when one is already out, the
    /// week is in hand, the store is, or the reads hit their ceiling.
    /// Owning this decision in one place is what keeps the "one in flight"
    /// rule true however many answers land.
    pub(crate) fn next_request(&mut self, req_id: u32, season: &Season) -> Option<ClientMsg> {
        if self.pending.is_some() || self.complete() || self.week_read() || self.pages >= MAX_PAGES
        {
            return None;
        }
        self.pending = Some(req_id);
        self.pages += 1;
        Some(ClientMsg::GetHistory {
            req_id,
            query: HistoryQuery::Fights {
                encounter: None,
                difficulty: None,
                // NOT the scope: the chips offer every character the store
                // has seen you play, and a chip changes nothing asked.
                guid: None,
                since_utc_ms: season.start_utc_ms,
                kind: None,
                sort: FightSort::Newest,
                limit: PAGE,
                after_id: self.cursor.clone(),
                role: None,
            },
        })
    }

    /// Fold one answer in. An answer whose id is not the one outstanding is
    /// a stale reply for a request we no longer care about, and is dropped.
    pub(crate) fn absorb(&mut self, req_id: u32, answer: &HistoryAnswer) {
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
            if !self.cards.iter().any(|have| have.id == c.id) {
                self.cards.push(c.clone());
            }
        }
        // The daemon sorts newest first and pages from a cursor, but a
        // re-request after `HistoryChanged` can interleave; sort once here
        // so the screen never depends on arrival order.
        self.cards
            .sort_by_key(|c| std::cmp::Reverse(c.start_utc_ms));
        // An empty tail is not the top of the list: keep the cursor so the
        // next request asks for what follows the last card we actually
        // hold, instead of re-requesting page one and dropping it as a
        // duplicate.
        if let Some(last) = cards.last() {
            self.cursor = Some(last.id.clone());
        }
    }

    /// A fight was stored: start the list over so the new pull is on it.
    pub(crate) fn reset(&mut self) {
        self.cards.clear();
        self.cursor = None;
        self.pages = 0;
        self.total = None;
        // A reply to the superseded request must not append to the fresh
        // list, so the id in flight is forgotten too.
        self.pending = None;
    }
}

/// A UTC date range, from the config: Home reads nothing from before the
/// season's start.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct Season {
    pub start_utc_ms: Option<i64>,
    pub end_utc_ms: Option<i64>,
}

impl Season {
    pub(crate) fn from_config(cfg: &Config) -> Self {
        Self {
            start_utc_ms: cfg.season_start.as_deref().and_then(parse_ymd),
            end_utc_ms: cfg.season_end.as_deref().and_then(parse_ymd),
        }
    }

    /// Half-open: the end date is the first day NOT in the season.
    pub(crate) fn contains(&self, start_utc_ms: i64) -> bool {
        self.start_utc_ms.is_none_or(|s| start_utc_ms >= s)
            && self.end_utc_ms.is_none_or(|e| start_utc_ms < e)
    }
}

/// "YYYY-MM-DD" → milliseconds since the Unix epoch, UTC midnight. `None`
/// for anything that is not exactly that shape or is not a real date.
/// Days-from-civil (Howard Hinnant), integer only — the dependency policy
/// keeps chrono out and a date is ten characters of arithmetic.
pub(crate) fn parse_ymd(s: &str) -> Option<i64> {
    let mut parts = s.split('-');
    let y: i64 = parts.next()?.parse().ok()?;
    let m_str = parts.next()?;
    let d_str = parts.next()?;
    if parts.next().is_some() || m_str.len() != 2 || d_str.len() != 2 {
        return None;
    }
    let m: i64 = m_str.parse().ok()?;
    let d: i64 = d_str.parse().ok()?;
    if !(1..=12).contains(&m) || d < 1 || d > days_in_month(y, m) {
        return None;
    }
    // Days from 1970-01-01, shifting the year to start in March so the leap
    // day is the last day of the "year" and needs no special case.
    let y_shift = y - i64::from(m <= 2);
    let era = if y_shift >= 0 { y_shift } else { y_shift - 399 } / 400;
    let yoe = y_shift - era * 400;
    let doy = (153 * (m + if m > 2 { -3 } else { 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    Some(days * 86_400_000)
}

fn days_in_month(y: i64, m: i64) -> i64 {
    match m {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if (y % 4 == 0 && y % 100 != 0) || y % 400 == 0 => 29,
        2 => 28,
        _ => 0,
    }
}

// ---- the derivation ---------------------------------------------------------

/// The whole screen, derived. Pure over the cards — this, not the widget
/// tree, is what the tests hold.
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct Panels {
    /// Every character the cards in hand name as yours, and the configured
    /// ones: what the window remembers for the scope chips and the picker.
    pub characters: Vec<CharLine>,
    /// Whose window this is by the store: the owner of its newest card,
    /// whatever the scope — what the chrome and the "you" follow.
    pub owner: Option<Char>,
    /// "Last night you played": the scope's newest night.
    pub night: Option<NightPanel>,
    /// The week's keys, newest first.
    pub keys: Vec<KeyRun>,
    /// The week's raids, one panel per instance and difficulty, newest
    /// first.
    pub raids: Vec<RaidPanel>,
    /// The week's key runs as throughput points, oldest first.
    pub trend: Vec<TrendPoint>,
    /// The week holds pulls, but no card names whose they were: nothing
    /// can be said about "you" until the store knows who that is.
    pub unowned: bool,
}

/// A character as Home draws them: a dot, a chip, a name.
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct Char {
    pub guid: String,
    pub name: String,
    pub class: Option<Class>,
    pub spec: Option<Spec>,
}

impl Char {
    fn of(p: &CardPlayer) -> Self {
        Self {
            guid: p.guid.clone(),
            name: p.name.clone(),
            class: p.class,
            spec: p.spec,
        }
    }

    /// Their colour as data — a dot, a curve — `Class::rgb` as it is.
    pub(crate) fn color(&self) -> iced::Color {
        self.class.map_or(theme::INK_3, theme::class_rgb)
    }
}

/// Where a player stood on one card among their ROLE — a healer against
/// healers — by the measure that role is read by: the fight header's
/// `Place`, over a stored card. Every player of the role counts, as on the
/// meter: Home ranks, it does not grade (the coach's floors are the mcp's).
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Standing {
    /// 1-based.
    pub place: usize,
    pub of: usize,
    /// Their measure: effective dps, or a healer's healing per second.
    pub value: f64,
}

impl Standing {
    /// How high in the role, 1 at the top and 0 at the bottom — a rank
    /// among 19 and one among 5 on one scale. Alone in the role is the top.
    pub(crate) fn percentile(self) -> f32 {
        if self.of > 1 {
            1.0 - (self.place.saturating_sub(1)) as f32 / (self.of - 1) as f32
        } else {
            1.0
        }
    }
}

/// The measure a role is read by, and its words: a healer's healing per
/// second, everyone else's effective damage per second (R19) — the one
/// number that does not reward an Augmentation Evoker's buffs twice. A
/// tank is ranked among tanks by it, as the meter's Damage view ranks them.
pub(crate) fn measure_of(p: &CardPlayer, duration_ms: i64) -> (&'static str, f64) {
    match p.role() {
        Some(Role::Healer) => ("healing per second", p.hps),
        _ => ("effective dps", p.effective_dps(duration_ms)),
    }
}

/// `guid`'s standing on `card` among our side's players of their role (all
/// of our side when their spec, and so their role, is unknown); `None`
/// when they are not on it.
pub(crate) fn standing(card: &FightCard, guid: &str) -> Option<Standing> {
    let me = card.players.iter().find(|p| p.guid == guid && !p.enemy)?;
    let role = me.role();
    let (_, mine) = measure_of(me, card.duration_ms);
    let peers: Vec<f64> = card
        .players
        .iter()
        .filter(|p| !p.enemy && (role.is_none() || p.role() == role))
        .map(|p| measure_of(p, card.duration_ms).1)
        .collect();
    Some(Standing {
        place: peers.iter().filter(|v| **v > mine).count() + 1,
        of: peers.len(),
        value: mine,
    })
}

/// The night Home leads with.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct NightPanel {
    /// The night, as the rail counts them ([`rail::night_of`]).
    pub day: i64,
    /// Where it was played: "The Venomous Abyss, Heroic", "Mythic+ keys".
    pub place: String,
    /// Who played it.
    pub who: Char,
    /// What the rank is by, in words: "effective dps".
    pub measure: &'static str,
    /// Oldest first, as the night went.
    pub pulls: Vec<NightPull>,
}

/// One pull of the night: a tile, and a dot on the rank chart.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct NightPull {
    pub fight_id: String,
    pub name: String,
    /// ✓ a kill or a timed key, ✕ a wipe or a key over time, a dash for a
    /// run with no verdict.
    pub mark: Mark,
    /// A wipe's lowest observed boss health ([`wipe_pct`]).
    pub wipe_pct: Option<u16>,
    pub standing: Standing,
}

/// One key of the week, against its timers.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct KeyRun {
    pub fight_id: String,
    /// "Kings' Rest +14".
    pub name: String,
    pub who: Char,
    /// The key clock (CHALLENGE_MODE_END's own, when the card has it).
    pub clock_ms: i64,
    /// The dungeon's (par, +2, +3) timers.
    pub pars: (i64, i64, i64),
    pub timed: bool,
    /// The chests it earned, 1 to 3, when timed.
    pub tier: u8,
}

impl KeyRun {
    /// What the row says of it: "+2", or "over".
    pub(crate) fn result(&self) -> String {
        if self.timed {
            format!("+{}", self.tier)
        } else {
            "over".to_string()
        }
    }
}

/// One raid at one difficulty, as the week played it.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct RaidPanel {
    /// "The Venomous Abyss, Heroic".
    pub title: String,
    /// In the raid's order, as far as the encounter ids tell it.
    pub bosses: Vec<BossLine>,
    /// Its newest pull's start, for the order of the panels.
    newest: i64,
}

/// One boss of a raid panel.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct BossLine {
    pub name: String,
    pub encounter: u32,
    /// Where the row leads: the fastest kill, else the newest pull.
    pub fight_id: String,
    pub best_kill_ms: Option<i64>,
    /// The lowest boss health a wipe was SEEN at ([`observed_pct`]).
    pub best_pct: Option<u16>,
    /// One per pull, oldest first.
    pub pulls: Vec<PullDot>,
}

/// A pull's dot: filled on a kill, a ring on a wipe, in whose colour.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct PullDot {
    pub fight_id: String,
    pub kill: bool,
    pub who: Char,
}

/// A key run's point on the throughput chart.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct TrendPoint {
    pub fight_id: String,
    pub who: Char,
    /// The night it was run on.
    pub day: i64,
    /// Effective damage per second.
    pub value: f64,
    /// Its character's best of the week: ringed in legendary orange.
    pub best: bool,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct CharLine {
    pub guid: String,
    pub name: String,
    pub class: Option<Class>,
    pub spec: Option<Spec>,
    pub fights: u32,
    pub last_utc_ms: i64,
    /// Their newest card's start on the log's clock: the night the
    /// picker's menu says they last played. 0 when no card says.
    pub last_local_ms: i64,
}

/// How close a WIPE came — its observed best boss health — for the words
/// that say it; `None` for anything that is not a boss wipe, or a wipe
/// whose health nobody saw ([`observed_pct`]).
pub(crate) fn wipe_pct(c: &FightCard) -> Option<u16> {
    (c.kind == FightKind::Encounter && c.success == Some(false) && !c.aborted)
        .then(|| observed_pct(c))
        .flatten()
}

/// A key card is named "Skyreach +15": the level is on the card AND in the
/// name. Strip it where the level is said apart from the name, so it is
/// said once — twice reads as a bug.
pub(crate) fn dungeon_name(name: &str) -> &str {
    match name.rsplit_once(" +") {
        // Only when what follows is really a keystone level.
        Some((head, level)) if !level.is_empty() && level.bytes().all(|b| b.is_ascii_digit()) => {
            head
        }
        _ => name,
    }
}

/// R16's `best_pct` as an OBSERVATION, not a number. On a kill 0 is the
/// truth — the boss reached zero. On anything else 0 cannot be: a boss at 0 %
/// is a kill by definition, so a 0 on a wipe means R16 never saw a health
/// reading, and reporting it as "0 %" tells the reader they nearly had a
/// pull nobody won. The card arguably should carry `None` there, but that is
/// a daemon-side change that would need every stored card rewritten, so the
/// client reads it as unknown.
fn observed_pct(c: &FightCard) -> Option<u16> {
    let pct = c.best_pct?;
    (pct > 0 || c.success == Some(true)).then_some(pct)
}

/// The card's "me": the scoped character's row on it, or — scoped to all
/// of them — its owner's. A card with neither is nobody's pull.
fn subject<'a>(c: &'a FightCard, scope: Option<&str>) -> Option<&'a CardPlayer> {
    let guid = scope.or(c.owner.as_deref())?;
    c.players.iter().find(|p| p.guid == guid && !p.enemy)
}

/// A card as a lead glyph: ✓ a kill, a win or a timed key, ✕ a wipe, a loss
/// or a key over time, a dash without a verdict — the rail's own.
fn mark_of(c: &FightCard) -> Mark {
    match (c.kind, c.success) {
        (FightKind::Overall | FightKind::Trash, _) => Mark::Dash,
        (_, Some(true)) => Mark::Good,
        (_, Some(false)) => Mark::Bad,
        (_, None) => Mark::Dash,
    }
}

/// A dungeon visit's card with no key: a run the night's list shows (a key
/// on its map begun within minutes of it was that key's zone-in, and is
/// the key's card's).
fn dungeon_run(c: &FightCard, cards: &[&FightCard]) -> bool {
    let Some(k) = c.key.as_ref().filter(|k| rail::is_dungeon(k.difficulty)) else {
        return false;
    };
    c.kind == FightKind::Overall
        && !cards.iter().any(|o| {
            o.kind == FightKind::Key
                && o.log == c.log
                && o.key.as_ref().is_some_and(|ok| ok.map_id == k.map_id)
                && (0..=rail::ZONE_IN_MS).contains(&(o.start_local_ms - c.start_local_ms))
        })
}

/// A pull the night's list shows: a boss, a key, an arena match, a dungeon
/// run — never trash, nor a raid visit's Σ over the bosses it lists.
fn night_pull(c: &FightCard, cards: &[&FightCard]) -> bool {
    match c.kind {
        FightKind::Encounter | FightKind::Key | FightKind::Arena => true,
        FightKind::Overall => dungeon_run(c, cards),
        FightKind::Trash => false,
    }
}

/// A raid boss pull's difficulty, when it is one.
fn raid_difficulty(c: &FightCard) -> Option<u32> {
    let d = c
        .encounter
        .filter(|_| c.kind == FightKind::Encounter)?
        .difficulty;
    rail::is_raid(d).then_some(d)
}

/// The instance a raid boss pull was in, the first of: the raid visit's Σ
/// card of the same log that began before it that night (at its difficulty
/// where one is, the zone's own word for it where none is); a Σ of any
/// night on the map the pull's card names; the tailed log's own visit the
/// pull was in, as the rail titles it — the daemon stores a visit's Σ only
/// once the visit closes, so a raid night still going has none; and last,
/// the instance another pull of the same boss was filed under by those
/// same three.
fn instance_of(c: &FightCard, known: &Known) -> Option<String> {
    let cards = known.cards;
    let in_log = |c: &FightCard| -> Option<String> {
        known
            .log
            .iter()
            .find(|v| v.holds(c))
            .map(|v| v.name.clone())
    };
    let on_map = |c: &FightCard| -> Option<String> {
        let map = c.key.as_ref().map(|k| k.map_id).filter(|m| *m != 0)?;
        cards
            .iter()
            .filter(|o| {
                o.kind == FightKind::Overall
                    && o.key
                        .as_ref()
                        .is_some_and(|k| k.map_id == map && rail::is_raid(k.difficulty))
            })
            .max_by_key(|o| o.start_local_ms)
            .map(|o| o.name.clone())
    };
    let sigma = |c: &FightCard| -> Option<String> {
        let d = raid_difficulty(c)?;
        let visits: Vec<(&FightCard, u32)> = cards
            .iter()
            .filter(|o| {
                o.kind == FightKind::Overall
                    && o.log == c.log
                    && o.start_local_ms <= c.start_local_ms
                    && rail::night_of(o.start_local_ms) == rail::night_of(c.start_local_ms)
            })
            .filter_map(|o| Some((*o, o.key.as_ref()?.difficulty)))
            .filter(|(_, zone)| rail::is_raid(*zone))
            .collect();
        let latest = |same: bool| {
            visits
                .iter()
                .filter(|(_, zone)| !same || *zone == d)
                .max_by_key(|(o, _)| o.start_local_ms)
                .map(|(o, _)| o.name.clone())
        };
        latest(true).or_else(|| latest(false))
    };
    sigma(c)
        .or_else(|| on_map(c))
        .or_else(|| in_log(c))
        .or_else(|| {
            let id = c.encounter?.id;
            cards
                .iter()
                .filter(|o| o.encounter.is_some_and(|e| e.id == id))
                .find_map(|o| sigma(o).or_else(|| on_map(o)).or_else(|| in_log(o)))
        })
}

/// What names a pull's place: the cards in hand, and the tailed log's
/// instance visits ([`rail::log_instances`]).
#[derive(Clone, Copy)]
struct Known<'a> {
    cards: &'a [&'a FightCard],
    log: &'a [rail::LogInstance],
}

/// Where a pull was played, as the rail titles its visit: a raid and its
/// difficulty, "Mythic+ keys", "Dungeons", "Arena", "Delve". A raid whose
/// instance nothing names is said by its difficulty alone ("Heroic raid"),
/// never under a placeholder that reads like a name.
fn place_of(c: &FightCard, known: &Known) -> String {
    match c.kind {
        FightKind::Key => "Mythic+ keys".to_string(),
        FightKind::Overall => "Dungeons".to_string(),
        FightKind::Arena => "Arena".to_string(),
        FightKind::Trash => "Open world".to_string(),
        FightKind::Encounter => match c.encounter.map(|e| e.difficulty) {
            Some(d) if rail::is_raid(d) => match (instance_of(c, known), difficulty_name(d)) {
                (Some(instance), Some(name)) => format!("{instance}, {name}"),
                (Some(instance), None) => instance,
                (None, Some(name)) => format!("{name} raid"),
                (None, None) => "Raid".to_string(),
            },
            Some(rail::DELVE) => "Delve".to_string(),
            Some(d) if rail::is_dungeon(d) => "Dungeons".to_string(),
            _ => "Encounters".to_string(),
        },
    }
}

/// A night of several places, said once each in the order they were
/// played: "A", "A and B", "A, B and more". A dungeon run on a keys night
/// is one of the keys.
fn places(titles: Vec<String>) -> String {
    let keys = titles.iter().any(|t| t == "Mythic+ keys");
    let mut seen: Vec<String> = Vec::new();
    for t in titles {
        let t = if keys && t == "Dungeons" {
            "Mythic+ keys".to_string()
        } else {
            t
        };
        if !seen.contains(&t) {
            seen.push(t);
        }
    }
    match seen.as_slice() {
        [] => String::new(),
        [a] => a.clone(),
        [a, b] => format!("{a} and {b}"),
        [a, b, ..] => format!("{a}, {b} and more"),
    }
}

pub(crate) fn derive(
    cards: &[FightCard],
    scope: Option<&str>,
    season: &Season,
    // `history_characters` from the config: characters that are "me" even
    // when the week holds no card for them.
    configured: &[String],
    // The tailed log's instance visits: where its raid pulls were, when no
    // card says yet.
    log: &[rail::LogInstance],
) -> Panels {
    let in_season: Vec<&FightCard> = cards
        .iter()
        .filter(|c| season.contains(c.start_utc_ms))
        .collect();
    // The week ends at the store's newest card, whoever's it was: every
    // scope is a slice of the one week.
    let newest = in_season.iter().map(|c| c.start_utc_ms).max().unwrap_or(0);
    let week: Vec<&FightCard> = in_season
        .iter()
        .copied()
        .filter(|c| !c.aborted && c.start_utc_ms >= newest.saturating_sub(WEEK_MS))
        .collect();
    // Newest first, each with its "me".
    let mut mine: Vec<(&FightCard, &CardPlayer)> = week
        .iter()
        .filter_map(|c| subject(c, scope).map(|p| (*c, p)))
        .collect();
    mine.sort_by_key(|(c, _)| std::cmp::Reverse(c.start_utc_ms));
    let owner = in_season
        .iter()
        .filter(|c| c.owner.is_some())
        .max_by_key(|c| c.start_utc_ms)
        .and_then(|c| subject(c, None))
        .map(Char::of);
    let characters = character_lines(&in_season, configured);
    let known = Known {
        cards: &in_season,
        log,
    };
    Panels {
        characters,
        owner,
        night: night_panel(&mine, &known),
        keys: key_runs(&mine),
        raids: raid_panels(&mine, &known),
        trend: trend_points(&mine),
        unowned: mine.is_empty() && scope.is_none() && week.iter().any(|c| c.owner.is_none()),
    }
}

/// The scope's newest night: its character's pulls that night, oldest
/// first, each with its standing in the role.
fn night_panel(mine: &[(&FightCard, &CardPlayer)], known: &Known) -> Option<NightPanel> {
    let all = known.cards;
    let (newest, me) = mine.iter().find(|(c, _)| night_pull(c, all)).copied()?;
    let day = rail::night_of(newest.start_local_ms);
    let who = Char::of(me);
    let mut pulls: Vec<(&FightCard, NightPull)> = mine
        .iter()
        .filter(|(c, p)| {
            p.guid == who.guid && rail::night_of(c.start_local_ms) == day && night_pull(c, all)
        })
        .filter_map(|(c, p)| {
            Some((
                *c,
                NightPull {
                    fight_id: c.id.clone(),
                    name: c.name.clone(),
                    mark: mark_of(c),
                    wipe_pct: wipe_pct(c),
                    standing: standing(c, &p.guid)?,
                },
            ))
        })
        .collect();
    pulls.sort_by_key(|(c, _)| c.start_local_ms);
    let place = places(pulls.iter().map(|(c, _)| place_of(c, known)).collect());
    Some(NightPanel {
        day,
        place,
        measure: measure_of(me, newest.duration_ms).0,
        who,
        pulls: pulls.into_iter().map(|(_, p)| p).collect(),
    })
}

/// The week's keys that carry their timers, newest first.
fn key_runs(mine: &[(&FightCard, &CardPlayer)]) -> Vec<KeyRun> {
    mine.iter()
        .filter(|(c, _)| c.kind == FightKind::Key)
        .filter_map(|(c, p)| {
            let pars = c.pars_ms?;
            let clock_ms = c.official_ms.unwrap_or(c.duration_ms);
            // The game's verdict when the card has one; the clock's else.
            let timed = c.success.unwrap_or(clock_ms <= pars.0);
            Some(KeyRun {
                fight_id: c.id.clone(),
                name: key_name(c),
                who: Char::of(p),
                clock_ms,
                pars,
                timed,
                tier: key_tier(clock_ms, pars).max(1),
            })
        })
        .collect()
}

/// A key's name with its level said once: "Kings' Rest +14".
fn key_name(c: &FightCard) -> String {
    match c.key.as_ref().and_then(|k| k.level) {
        Some(level) => format!("{} +{level}", dungeon_name(&c.name)),
        None => c.name.clone(),
    }
}

/// The week's raid pulls, one panel per instance and difficulty (newest
/// first), each boss with every pull as a dot.
fn raid_panels(mine: &[(&FightCard, &CardPlayer)], known: &Known) -> Vec<RaidPanel> {
    let mut panels: Vec<RaidPanel> = Vec::new();
    // Oldest first, so each boss's dots run the way the week did.
    for (c, p) in mine.iter().rev() {
        let (Some(_), Some(e)) = (raid_difficulty(c), c.encounter) else {
            continue;
        };
        let title = place_of(c, known);
        let at = match panels.iter().position(|r| r.title == title) {
            Some(at) => at,
            None => {
                panels.push(RaidPanel {
                    title,
                    bosses: Vec::new(),
                    newest: 0,
                });
                panels.len() - 1
            }
        };
        let Some(panel) = panels.get_mut(at) else {
            continue;
        };
        panel.newest = panel.newest.max(c.start_utc_ms);
        let boss = match panel.bosses.iter().position(|b| b.encounter == e.id) {
            Some(i) => i,
            None => {
                panel.bosses.push(BossLine {
                    name: c.name.clone(),
                    encounter: e.id,
                    fight_id: c.id.clone(),
                    best_kill_ms: None,
                    best_pct: None,
                    pulls: Vec::new(),
                });
                panel.bosses.len() - 1
            }
        };
        let Some(b) = panel.bosses.get_mut(boss) else {
            continue;
        };
        let kill = c.success == Some(true);
        b.pulls.push(PullDot {
            fight_id: c.id.clone(),
            kill,
            who: Char::of(p),
        });
        if kill {
            if b.best_kill_ms.is_none_or(|best| c.duration_ms < best) {
                b.best_kill_ms = Some(c.duration_ms);
                b.fight_id = c.id.clone();
            }
        } else {
            if let Some(pct) = observed_pct(c) {
                // The closest a wipe came. An unobserved pull stays out of
                // it: "no health report" is not "the boss never took
                // damage".
                b.best_pct = Some(b.best_pct.map_or(pct, |have| have.min(pct)));
            }
            // Unkilled, the row leads to the newest pull.
            if b.best_kill_ms.is_none() {
                b.fight_id = c.id.clone();
            }
        }
    }
    for p in &mut panels {
        p.bosses.sort_by_key(|b| b.encounter);
    }
    panels.sort_by_key(|p| std::cmp::Reverse(p.newest));
    panels
}

/// The week's key runs as throughput points, oldest first — the runs of a
/// damage dealer (or an unknown spec): a healer's or a tank's effective
/// dps is not what they are judged by, and would drag the scale down to
/// it. Each character's best of the week is marked.
fn trend_points(mine: &[(&FightCard, &CardPlayer)]) -> Vec<TrendPoint> {
    let mut points: Vec<TrendPoint> = mine
        .iter()
        .rev()
        .filter(|(c, p)| c.kind == FightKind::Key && matches!(p.role(), Some(Role::Dps) | None))
        .map(|(c, p)| TrendPoint {
            fight_id: c.id.clone(),
            who: Char::of(p),
            day: rail::night_of(c.start_local_ms),
            value: p.effective_dps(c.duration_ms),
            best: false,
        })
        .filter(|t| t.value > 0.0)
        .collect();
    let mut best: BTreeMap<String, usize> = BTreeMap::new();
    for (i, t) in points.iter().enumerate() {
        let at = best.entry(t.who.guid.clone()).or_insert(i);
        if points.get(*at).is_some_and(|b| t.value > b.value) {
            *at = i;
        }
    }
    for i in best.into_values() {
        if let Some(t) = points.get_mut(i) {
            t.best = true;
        }
    }
    points
}

/// Every character the store has seen as an owner, unioned with the ones
/// `history_characters` names (decisions §4). A card whose owner the daemon
/// could not resolve names nobody and contributes nothing — better a short
/// list than a list of guildmates presented as "you" — but a configured
/// character with no cards in hand is still one of yours, and says so
/// rather than being absent.
pub(crate) fn character_lines(cards: &[&FightCard], configured: &[String]) -> Vec<CharLine> {
    let mut by_guid: BTreeMap<String, CharLine> = BTreeMap::new();
    for c in cards {
        let Some(owner) = c.owner.as_deref() else {
            continue;
        };
        let Some(p) = c.players.iter().find(|p| p.guid == owner) else {
            continue;
        };
        let line = by_guid
            .entry(owner.to_string())
            .or_insert_with(|| CharLine {
                guid: owner.to_string(),
                name: p.name.clone(),
                class: p.class,
                spec: p.spec,
                ..CharLine::default()
            });
        line.fights += 1;
        if c.start_utc_ms >= line.last_utc_ms {
            line.last_utc_ms = c.start_utc_ms;
            line.last_local_ms = c.start_local_ms;
        }
    }
    let mut out: Vec<CharLine> = by_guid.into_values().collect();
    // The config names characters, not guids; the store knows guids. Name is
    // the only join there is, and it is the same "Name-Realm" on both sides.
    for name in configured {
        if !out.iter().any(|c| c.name.eq_ignore_ascii_case(name)) {
            out.push(CharLine {
                name: name.clone(),
                ..CharLine::default()
            });
        }
    }
    out.sort_by(|a, b| b.fights.cmp(&a.fights).then(a.name.cmp(&b.name)));
    out
}

/// A character line as the picker wears it.
pub(crate) fn char_pick(c: &CharLine) -> nav::CharPick {
    nav::CharPick {
        guid: c.guid.clone(),
        name: c.name.clone(),
        class: c.class,
        spec: c.spec,
        fights: c.fights,
        last_local_ms: (c.last_local_ms != 0).then_some(c.last_local_ms),
    }
}

// ---- the screen -------------------------------------------------------------

/// The facts the screen needs besides the panels, cheap to clone into the
/// `responsive` closure that lays it out (`Home` is not `Clone`, and the
/// closure is called again on every resize).
#[derive(Debug, Clone)]
pub(crate) struct Meta {
    /// The store is on and has answered: there is a week to show. Off, or
    /// not heard from yet, the screen says which instead of drawing panels
    /// that say "none" of what nobody read.
    pub settled: bool,
    pub stalled: bool,
    /// Whose week: a guid, or `None` for every character.
    pub scope: Option<String>,
    /// The line that tells a disabled store from a cold one from a
    /// degraded one ([`state_line`]).
    pub state_line: Option<String>,
    pub hide_realms: bool,
    /// The night it is now, which words how long ago the lead night was.
    pub tonight: i64,
    /// A pressed chip's edge.
    pub accent: theme::Accent,
}

impl Meta {
    pub(crate) fn of(home: &Home, hide_realms: bool, tonight: i64, accent: theme::Accent) -> Self {
        Self {
            settled: home.answered && home.disabled_reason.is_none(),
            stalled: home.stalled(),
            scope: home.scope.clone(),
            state_line: state_line(home),
            hide_realms,
            tonight,
            accent,
        }
    }
}

/// The whole screen: the title and its scope chips, the night, and the
/// week's panels in as many columns as the width holds. `chars` are the
/// characters the window knows you play — from this week's cards and every
/// page of the store it has read — which the chips offer, by name.
pub(crate) fn screen(
    home: &Home,
    panels: &Panels,
    chars: &[CharLine],
    accent: theme::Accent,
    hide_realms: bool,
    tonight: i64,
) -> Element<'static, crate::window::Message> {
    let meta = Meta::of(home, hide_realms, tonight, accent);
    let panels = panels.clone();
    let mut chars: Vec<CharLine> = chars
        .iter()
        .filter(|c| !c.guid.is_empty())
        .cloned()
        .collect();
    // The chips stand still: by name, not by how much each was played.
    chars.sort_by_key(|c| crate::fold::fold(&c.name));
    // The layout is a function of the width, which only the layout knows;
    // `responsive` is how a widget tree gets to ask.
    iced::widget::responsive(move |size| panels::laid_out(&meta, &panels, &chars, size.width))
        .into()
}

/// The one line that tells a disabled store from a cold one from a
/// degraded one. `hub.rs` answers all three with an empty card list, so
/// without this the screen would show the same confident nothing for each.
fn state_line(home: &Home) -> Option<String> {
    if let Some(why) = home.disabled_reason.as_deref() {
        return Some(format!("The history store is off: {why}."));
    }
    if !home.answered {
        return Some("Your week is on its way from the history store.".to_string());
    }
    if home.dropped > 0 {
        return Some(format!(
            "The daemon dropped {}: this is not the whole week.",
            nav::plural(home.dropped as usize, "request")
        ));
    }
    None
}

#[cfg(test)]
mod tests;
