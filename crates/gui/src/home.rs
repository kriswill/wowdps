//! Home: a read-only dashboard over the daemon's history store.
//!
//! Home is window-local, exactly like the talent viewer — never a `Screen`
//! variant, so `ClientState` (and with it the TUI and the overlay) never
//! learns it exists. Everything on it is derived here, client-side, from the
//! `HistoryQuery::Fights` answers the daemon already serves; this slice adds
//! no query and no wire message.
//!
//! Two rules run through the whole module:
//!
//! - **A number we cannot derive is a dash, never a zero.** The store carries
//!   no Mythic+ rating and no raid boss roster, so the season score and the
//!   `/ 8` denominator the design mock showed are not here at all, and every
//!   `Option` that comes back empty renders as `—` with the reason beside it.
//! - **Paging is transport.** The reader never sees a page: the list grows as
//!   they scroll toward its end, one request in flight at a time, and stops
//!   asking the moment the cache holds everything the store matched.

use std::collections::BTreeMap;

use iced::widget::{Space, column, container, row, scrollable, text};
use iced::{Color, Element, Font, Length};

use wowdps_model::fmt::{duration, human};
use wowdps_model::{Class, Role, Spec};
use wowdps_proto::history::{CardPlayer, FightCard, FightKind};
use wowdps_proto::{ClientMsg, FightSort, HistoryAnswer, HistoryQuery};

use crate::config::Config;
use crate::nav::{self, Stat};
use crate::theme::{self, Density, size};

/// Cards per request. Small enough that the answer is one modest frame and
/// the first screenful arrives quickly; the daemon caps it anyway (§2).
pub(crate) const PAGE: u32 = 200;
/// Requests one burst of scrolling will make before it stops on its own.
/// Not a display cap: scrolling further asks again.
pub(crate) const MAX_PAGES: u32 = 5;
/// How close to the bottom of the list (in logical pixels) counts as "the
/// reader is asking for more".
pub(crate) const SCROLL_TRIGGER: f32 = 400.0;

/// The em dash every underivable number wears.
pub(crate) const DASH: &str = "—";

/// Which part of Home the reader is looking at. The overview truncates every
/// list to what fits a grid cell; focusing a section is how the rest of it is
/// reachable at all, which is why the chips are a focus and not a scroll —
/// there is more here than scrolling could reveal.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum Section {
    /// The overview grid: every panel, each truncated.
    #[default]
    Season,
    Keys,
    Raid,
    Me,
    Characters,
    Recent,
}

impl Section {
    pub(crate) fn name(self) -> &'static str {
        match self {
            Section::Season => "season",
            Section::Keys => "keys",
            Section::Raid => "raid",
            Section::Me => "me",
            Section::Characters => "characters",
            Section::Recent => "recent",
        }
    }
}

/// How many rows a panel shows in the overview grid. Focus a section to see
/// the whole list.
const OVERVIEW_KEYS: usize = 5;
const OVERVIEW_BOSSES: usize = 6;
const OVERVIEW_RECENT: usize = 12;

/// Window-local Home state.
#[derive(Debug, Default)]
pub(crate) struct Home {
    /// Cards accumulated across requests, newest first, deduped by id.
    pub cards: Vec<FightCard>,
    /// The in-flight `GetHistory` req_id. `Some` means a request is out and
    /// no second one may be sent — the rule that keeps a scroll gesture from
    /// flooding the history queue a closing pull needs.
    pub pending: Option<u32>,
    /// `total` from the last answer: how many cards matched before `limit`.
    pub total: Option<u32>,
    /// Requests made in the current burst.
    pub pages: u32,
    /// The last card id of the newest answer, for `after_id`.
    pub cursor: Option<String>,
    /// Which character the screen is scoped to; `None` = the owner the
    /// newest card names.
    pub character: Option<String>,
    /// Which section is in focus. `Season` is the overview grid.
    pub section: Section,
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

    /// The next request to send, or `None` when one is already out, the
    /// burst's ceiling is reached, or the list is complete. Owning this
    /// decision in one place is what keeps the "one in flight" rule true for
    /// the open gesture and the scroll gesture alike.
    pub(crate) fn next_request(&mut self, req_id: u32, season: &Season) -> Option<ClientMsg> {
        if self.pending.is_some() || self.complete() || self.pages >= MAX_PAGES {
            return None;
        }
        self.pending = Some(req_id);
        self.pages += 1;
        Some(ClientMsg::GetHistory {
            req_id,
            query: HistoryQuery::Fights {
                encounter: None,
                difficulty: None,
                // NOT the owner: the characters panel needs every character
                // the store has seen, not just the one we are scoped to.
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

    /// The reader scrolled: reopen the burst budget so the list keeps
    /// growing past `MAX_PAGES`. Transport, never a control.
    pub(crate) fn scrolled_to_end(&mut self) {
        self.pages = 0;
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
        self.cursor = cards.last().map(|c| c.id.clone());
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

    /// Which guid the screen is about.
    pub(crate) fn owner(&self) -> Option<&str> {
        self.character
            .as_deref()
            .or_else(|| self.cards.iter().find_map(|c| c.owner.as_deref()))
    }
}

/// A named UTC date range, from the config.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct Season {
    pub label: String,
    pub start_utc_ms: Option<i64>,
    pub end_utc_ms: Option<i64>,
}

impl Season {
    pub(crate) fn from_config(cfg: &Config) -> Self {
        Self {
            label: cfg.season_label.clone(),
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
    pub top: Vec<Stat>,
    pub keys: Vec<KeyLine>,
    pub raid: RaidPanel,
    pub me: MePanel,
    pub characters: Vec<CharLine>,
    pub recent: Vec<RecentLine>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct KeyLine {
    pub map_id: u32,
    pub name: String,
    pub best_level: Option<u32>,
    pub runs: u32,
    pub timed: u32,
    pub over: u32,
    pub fight_id: String,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct RaidPanel {
    pub instance: Option<String>,
    pub bosses: Vec<BossLine>,
    pub week_pulls: u32,
    pub week_kills: u32,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct BossLine {
    pub name: String,
    pub encounter: Option<u32>,
    pub difficulty_tag: &'static str,
    pub best_kill_ms: Option<i64>,
    /// R16's lowest boss health on a wipe. `None` = no health report was
    /// logged, which is not the same as "never scratched it".
    pub best_pct: Option<u16>,
    pub pulls: u32,
    pub fight_id: String,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct MePanel {
    pub name: String,
    pub class: Option<Class>,
    pub spec: Option<Spec>,
    pub role: Option<Role>,
    /// What the numbers below measure, worded for the subject's role.
    pub measure: &'static str,
    pub median: Option<f64>,
    pub best: Option<f64>,
    pub deaths_per_pull: Option<f64>,
    /// Oldest → newest, at most 12 points.
    pub spark: Vec<f64>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct CharLine {
    pub guid: String,
    pub name: String,
    pub class: Option<Class>,
    pub spec: Option<Spec>,
    pub fights: u32,
    pub last_utc_ms: i64,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct RecentLine {
    pub fight_id: String,
    pub name: String,
    pub tag: String,
    pub tag_color: Color,
    pub duration_ms: i64,
    pub pinned: bool,
    pub key_level: Option<u32>,
}

const WEEK_MS: i64 = 7 * 86_400_000;

/// Difficulty ids as the log writes them, worded the way a raider says them.
fn difficulty_tag(difficulty: Option<u32>) -> &'static str {
    match difficulty {
        Some(16) => "M",
        Some(15) => "H",
        Some(14) => "N",
        Some(17) => "LFR",
        Some(_) => "?",
        None => "",
    }
}

/// The tag a stored card wears in the recent list — the same words the meter
/// header uses, plus the keystone verdict for a key.
fn card_tag(c: &FightCard) -> (String, Color) {
    if c.aborted {
        return ("OPEN".to_string(), theme::DIM);
    }
    match (c.kind, c.success) {
        (FightKind::Key, Some(_)) => match c.pars_ms {
            // The key's own words ("TIMED +2", "OVER +0:26"), which carry
            // more than a bare success flag does.
            Some(pars) => (
                wowdps_model::fmt::key_tag(c.official_ms.unwrap_or(c.duration_ms), pars, c.success),
                if c.success == Some(true) {
                    theme::GREEN
                } else {
                    theme::RED
                },
            ),
            None if c.success == Some(true) => ("TIMED".to_string(), theme::GREEN),
            None => ("OVER".to_string(), theme::RED),
        },
        (_, Some(true)) => ("KILL".to_string(), theme::GREEN),
        (_, Some(false)) => ("WIPE".to_string(), theme::RED),
        (_, None) => (String::new(), theme::DIM),
    }
}

/// The measure a role is judged by, and its name. DPS is graded by
/// `effective_dps` (R19) — the one number that does not reward an
/// Augmentation Evoker's buffs twice.
fn measure_of(p: &CardPlayer, duration_ms: i64) -> (&'static str, f64) {
    match p.role() {
        Some(Role::Healer) => ("hps", p.hps),
        Some(Role::Tank) => ("dtps", p.dtps),
        _ => ("effective dps", p.effective_dps(duration_ms)),
    }
}

pub(crate) fn derive(
    cards: &[FightCard],
    owner: Option<&str>,
    season: &Season,
    // `history_characters` from the config: characters that are "me" even
    // when this season holds no card for them.
    configured: &[String],
) -> Panels {
    let in_season: Vec<&FightCard> = cards
        .iter()
        .filter(|c| season.contains(c.start_utc_ms))
        .collect();
    let newest = in_season.first().map(|c| c.start_utc_ms).unwrap_or(0);
    let week_start = newest - WEEK_MS;

    Panels {
        top: top_stats(&in_season, season, week_start),
        keys: key_lines(&in_season),
        raid: raid_panel(&in_season, week_start),
        me: me_panel(&in_season, owner),
        characters: character_lines(&in_season, configured),
        recent: recent_lines(&in_season),
    }
}

/// The headline row. There is deliberately no "season score" card: no card
/// in the store carries a Mythic+ rating, and a computed lookalike would be
/// a number the game never showed the user.
fn top_stats(cards: &[&FightCard], season: &Season, week_start: i64) -> Vec<Stat> {
    if cards.is_empty() {
        // Nothing matched: three dashes with the reason, not three zeros
        // that would read as "you did nothing this season".
        return vec![
            Stat::unknown("pulls", &season.label),
            Stat::unknown("kills", "no stored fights"),
            Stat::unknown("this week", "no stored fights"),
        ];
    }
    let pulls = cards.iter().filter(|c| !c.aborted).count();
    let week = cards
        .iter()
        .filter(|c| !c.aborted && c.start_utc_ms >= week_start)
        .count();
    let kills = cards
        .iter()
        .filter(|c| c.success == Some(true) && !c.aborted)
        .count();
    vec![
        Stat {
            label: "pulls".to_string(),
            value: pulls.to_string(),
            sub: Some(season.label.clone()),
            value_color: None,
            headline: true,
        },
        Stat {
            label: "kills".to_string(),
            value: kills.to_string(),
            sub: None,
            value_color: Some(theme::GREEN),
            headline: false,
        },
        Stat {
            label: "this week".to_string(),
            value: format!("{week} pulls"),
            // The last 7 days measured from the newest card, not from now:
            // a store read on Tuesday about last Saturday's raid should not
            // silently empty itself.
            sub: Some("7 days".to_string()),
            value_color: None,
            headline: false,
        },
    ]
}

/// A key card is named "Skyreach +15": the level is on the card AND in the
/// name, and `KeyLine` already carries it as `best_level`. Strip it here so
/// the panel says the level once — twice reads as a bug, and the shorter
/// name is what fits a grid column without wrapping.
pub(crate) fn dungeon_name(name: &str) -> &str {
    match name.rsplit_once(" +") {
        // Only when what follows is really a keystone level.
        Some((head, level)) if !level.is_empty() && level.bytes().all(|b| b.is_ascii_digit()) => {
            head
        }
        _ => name,
    }
}

fn key_lines(cards: &[&FightCard]) -> Vec<KeyLine> {
    let mut by_map: BTreeMap<u32, KeyLine> = BTreeMap::new();
    for c in cards
        .iter()
        .filter(|c| c.kind == FightKind::Key && !c.aborted)
    {
        let Some(key) = c.key.as_ref() else { continue };
        let line = by_map.entry(key.map_id).or_insert_with(|| KeyLine {
            map_id: key.map_id,
            name: dungeon_name(&c.name).to_string(),
            fight_id: c.id.clone(),
            ..KeyLine::default()
        });
        line.runs += 1;
        match c.success {
            Some(true) => line.timed += 1,
            Some(false) => line.over += 1,
            None => {}
        }
        // The best run is the highest timed level; an untimed run never
        // becomes the "best" one however high the key was.
        if c.success == Some(true) && key.level > line.best_level {
            line.best_level = key.level;
            line.fight_id = c.id.clone();
        }
    }
    let mut out: Vec<KeyLine> = by_map.into_values().collect();
    out.sort_by(|a, b| {
        b.best_level
            .cmp(&a.best_level)
            .then(b.runs.cmp(&a.runs))
            .then(a.name.cmp(&b.name))
    });
    // Every dungeon: the overview shows the first few, the focused section
    // shows the rest, and neither can show what derive threw away.
    out
}

/// Bosses seen, best kill per boss, and the week's pulls. There is no
/// "N / 8": no card knows how many bosses the raid has, so the denominator
/// would be invented. The panel says how many are down and how many were
/// seen, both of which the cards do know.
fn raid_panel(cards: &[&FightCard], week_start: i64) -> RaidPanel {
    let raids: Vec<&&FightCard> = cards
        .iter()
        .filter(|c| c.kind == FightKind::Encounter && !c.aborted)
        .collect();
    let mut by_boss: BTreeMap<(String, u32), BossLine> = BTreeMap::new();
    for c in &raids {
        let difficulty = c.encounter.map(|e| e.difficulty);
        let key = (c.name.clone(), difficulty.unwrap_or(0));
        let line = by_boss.entry(key).or_insert_with(|| BossLine {
            name: c.name.clone(),
            encounter: c.encounter.map(|e| e.id),
            difficulty_tag: difficulty_tag(difficulty),
            fight_id: c.id.clone(),
            ..BossLine::default()
        });
        line.pulls += 1;
        if c.success == Some(true) {
            if line.best_kill_ms.is_none_or(|best| c.duration_ms < best) {
                line.best_kill_ms = Some(c.duration_ms);
                line.fight_id = c.id.clone();
            }
        } else if let Some(pct) = c.best_pct {
            // The closest a wipe came. `None` on a card stays `None`: "no
            // health report" is not "the boss never took damage".
            line.best_pct = Some(line.best_pct.map_or(pct, |b| b.min(pct)));
        }
    }
    let mut bosses: Vec<BossLine> = by_boss.into_values().collect();
    bosses.sort_by(|a, b| b.pulls.cmp(&a.pulls).then(a.name.cmp(&b.name)));
    RaidPanel {
        instance: raids.first().map(|c| c.name.clone()),
        bosses,
        week_pulls: raids
            .iter()
            .filter(|c| c.start_utc_ms >= week_start)
            .count() as u32,
        week_kills: raids
            .iter()
            .filter(|c| c.start_utc_ms >= week_start && c.success == Some(true))
            .count() as u32,
    }
}

/// The median of an ALREADY SORTED slice: the middle value, or the mean of
/// the two middle ones when the count is even. The screen calls it a median,
/// so it has to be one — with an even number of pulls the upper-middle value
/// alone reads high, and a coach comparing nights would see a step that is
/// an artefact of the pull count.
pub(crate) fn median_of(sorted: &[f64]) -> Option<f64> {
    match sorted.len() {
        0 => None,
        n if n % 2 == 1 => sorted.get(n / 2).copied(),
        n => {
            let (lo, hi) = (sorted.get(n / 2 - 1)?, sorted.get(n / 2)?);
            Some((lo + hi) / 2.0)
        }
    }
}

fn me_panel(cards: &[&FightCard], owner: Option<&str>) -> MePanel {
    let Some(owner) = owner else {
        return MePanel::default();
    };
    let mine: Vec<(&&FightCard, &CardPlayer)> = cards
        .iter()
        .filter(|c| !c.aborted)
        .filter_map(|c| c.players.iter().find(|p| p.guid == owner).map(|p| (c, p)))
        .collect();
    let Some((newest_card, newest_me)) = mine.first() else {
        return MePanel::default();
    };
    let (measure, _) = measure_of(newest_me, newest_card.duration_ms);
    let mut values: Vec<f64> = mine
        .iter()
        .map(|(c, p)| measure_of(p, c.duration_ms).1)
        // A pull the subject sat out of, or one with no clock, is not a
        // zero performance — it is not a data point at all.
        .filter(|v| *v > 0.0)
        .collect();
    let pulls = mine.len() as f64;
    let deaths: u32 = mine.iter().map(|(_, p)| p.deaths).sum();
    // Oldest → newest, so the sparkline reads left to right in time.
    let mut spark: Vec<f64> = values.iter().rev().copied().collect();
    if spark.len() > 12 {
        spark.drain(..spark.len() - 12);
    }
    let best = values.iter().copied().fold(f64::NAN, f64::max);
    values.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let median = median_of(&values);
    MePanel {
        name: newest_me.name.clone(),
        class: newest_me.class,
        spec: newest_me.spec,
        role: newest_me.role(),
        measure,
        median,
        best: best.is_finite().then_some(best),
        deaths_per_pull: (pulls > 0.0).then(|| f64::from(deaths) / pulls),
        spark,
    }
}

/// Every character the store has seen as an owner, unioned with the ones
/// `history_characters` names (decisions §4). A card whose owner the daemon
/// could not resolve names nobody and contributes nothing — better a short
/// list than a list of guildmates presented as "you" — but a configured
/// character with no cards THIS SEASON is still one of yours, and says so
/// rather than being absent.
fn character_lines(cards: &[&FightCard], configured: &[String]) -> Vec<CharLine> {
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
        line.last_utc_ms = line.last_utc_ms.max(c.start_utc_ms);
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

fn recent_lines(cards: &[&FightCard]) -> Vec<RecentLine> {
    cards
        .iter()
        .map(|c| {
            let (tag, tag_color) = card_tag(c);
            RecentLine {
                fight_id: c.id.clone(),
                name: c.name.clone(),
                tag,
                tag_color,
                duration_ms: c.duration_ms,
                pinned: c.pinned,
                key_level: c.key.as_ref().and_then(|k| k.level),
            }
        })
        .collect()
}

// ---- the screen -------------------------------------------------------------

/// A number we have, or the dash for one we do not.
fn or_dash(v: Option<f64>) -> String {
    v.map_or_else(|| DASH.to_string(), |v| human(v as u64))
}

fn line<'a>(label: String, value: String, color: Color) -> Element<'a, crate::window::Message> {
    row![
        // Clipped, not wrapped: a long dungeon name in a narrow grid column
        // must not turn one row into two and misalign the panel.
        container(
            text(label)
                .size(size::MICRO)
                .color(Color::WHITE)
                .wrapping(iced::widget::text::Wrapping::None),
        )
        .clip(true),
        Space::new().width(Length::Fill),
        text(value)
            .size(size::MICRO)
            .color(color)
            .font(Font::MONOSPACE),
    ]
    .spacing(8)
    .into()
}

/// The facts the screen needs about `Home` itself, cheap to clone into the
/// `responsive` closure that lays the grid out (`Home` is not `Clone`, and
/// the closure is called again on every resize).
#[derive(Debug, Clone, Default)]
struct Meta {
    cards: usize,
    total: Option<u32>,
    answered: bool,
    stalled: bool,
    character: Option<String>,
    section: Section,
    state_line: Option<String>,
}

impl Meta {
    fn of(home: &Home) -> Self {
        Self {
            cards: home.cards.len(),
            total: home.total,
            answered: home.answered,
            stalled: !home.complete() && home.pending.is_none() && home.pages >= MAX_PAGES,
            character: home.character.clone(),
            section: home.section,
            state_line: state_line(home),
        }
    }
}

/// Narrowest a panel may be laid out at — 15 rem at the 16 px root the design
/// study assumes. Wider windows get more columns rather than one column of
/// rows with a hand's breadth of nothing between name and number.
const MIN_COL: f32 = 240.0;
/// Most columns, however wide the window: past three a dashboard stops being
/// glanceable and becomes a spreadsheet.
const MAX_COLS: usize = 3;

/// How many columns fit in `width`.
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

/// Lay panels out in `cols` columns, padding the last row so a lone panel
/// keeps its column's width instead of stretching across the window.
fn grid<M: 'static>(
    panels: Vec<Element<'static, M>>,
    cols: usize,
    gap: f32,
) -> Element<'static, M> {
    let mut grid = column![].spacing(gap);
    let mut panels = panels.into_iter().peekable();
    while panels.peek().is_some() {
        let mut line = row![].spacing(gap).align_y(iced::Alignment::Start);
        let mut used = 0;
        for _ in 0..cols {
            match panels.next() {
                Some(p) => {
                    line = line.push(container(p).width(Length::FillPortion(1)));
                    used += 1;
                }
                None => break,
            }
        }
        for _ in used..cols {
            line = line.push(Space::new().width(Length::FillPortion(1)));
        }
        grid = grid.push(line);
    }
    grid.into()
}

/// The sections the chips offer, in the order the grid lays them out. A
/// chip is only offered for a section that has something to show — a chip
/// that leads to an empty frame is the affordance-shaped hole this whole
/// mechanism exists to avoid.
pub(crate) fn sections(panels: &Panels) -> Vec<Section> {
    let mut out = vec![Section::Season];
    if !panels.keys.is_empty() {
        out.push(Section::Keys);
    }
    if !panels.raid.bosses.is_empty() {
        out.push(Section::Raid);
    }
    out.push(Section::Me);
    if !panels.characters.is_empty() {
        out.push(Section::Characters);
    }
    out.push(Section::Recent);
    out
}

/// The whole screen. The list at the bottom is a `scrollable` that asks for
/// more as it nears its end — there is no pager and no "load more".
pub(crate) fn screen(
    home: &Home,
    panels: &Panels,
    season: &Season,
    accent: theme::Accent,
    density: Density,
) -> Element<'static, crate::window::Message> {
    let meta = Meta::of(home);
    let panels = panels.clone();
    let season = season.clone();
    // The column count is a function of the width, which only the layout
    // knows; `responsive` is how a widget tree gets to ask.
    iced::widget::responsive(move |size| {
        laid_out(&meta, &panels, &season, accent, density, size.width)
    })
    .into()
}

fn laid_out(
    meta: &Meta,
    panels: &Panels,
    season: &Season,
    accent: theme::Accent,
    density: Density,
    width: f32,
) -> Element<'static, crate::window::Message> {
    use crate::window::Message;

    let mut head = column![
        nav::two_tone_title(
            if panels.me.name.is_empty() {
                "wowdps".to_string()
            } else {
                panels.me.name.clone()
            },
            season.label.clone(),
            None,
            accent,
            size::TITLE,
        ),
        nav::stat_cards(&panels.top, accent, density),
    ]
    .spacing(density.gap());

    // What the reader is looking at, before any number: an empty screen for
    // three different reasons must not look like one screen.
    if let Some(state) = meta.state_line.clone() {
        head = head.push(text(state).size(size::MICRO).color(theme::YELLOW));
    }

    // The chips FOCUS a section: the overview truncates every list, so this
    // is the only way to the rest of one. Pressing the active chip (or
    // "season") comes back to the overview, so a chip never becomes a
    // one-way door.
    let offered = sections(panels);
    let chips: Vec<(String, Message)> = offered
        .iter()
        .map(|s| {
            let to = if *s == meta.section {
                Section::Season
            } else {
                *s
            };
            (s.name().to_string(), Message::HomeSection(to))
        })
        .collect();
    let active = offered.iter().position(|s| *s == meta.section);
    head = head.push(nav::chip_row(chips, active, accent));

    // The overview shows every panel, each cut to what a grid cell holds; a
    // focused section shows that one panel whole, at full width. `full` is
    // that difference, and it is the only difference — same builders, so the
    // two views cannot drift.
    let focus = meta.section;
    let full = focus != Section::Season;
    let wanted = |s: Section| !full || focus == s;
    let mut cards: Vec<Element<'static, Message>> = Vec::new();

    if wanted(Section::Keys) && !panels.keys.is_empty() {
        cards.push(keys_panel(panels, accent, full));
    }
    if wanted(Section::Raid) && !panels.raid.bosses.is_empty() {
        cards.push(raid_card(panels, accent, full));
    }
    if wanted(Section::Me) {
        cards.push(me_card(panels, accent));
    }
    if wanted(Section::Characters) && !panels.characters.is_empty() {
        cards.push(characters_card(meta, panels, accent));
    }
    if wanted(Section::Recent) {
        cards.push(recent_card(meta, panels, accent, full));
    }
    // A focused section whose content went away between the click and the
    // next answer (a re-read after a stored fight, a narrowed season) still
    // says what happened rather than showing an empty frame.
    if cards.is_empty() {
        cards.push(nav::panel(
            focus.name(),
            None,
            text(if meta.answered {
                "nothing here this season"
            } else {
                "reading the history store…"
            })
            .size(size::MICRO)
            .color(theme::DIM),
            None,
            accent,
        ));
    }

    let gap = density.gap();
    let mut body = head;
    // A focused section is one panel with the whole width to itself; the
    // overview shares it out.
    let cols = if full { 1 } else { columns_for(width, gap) };
    body = body.push(grid(cards, cols, gap));
    // The one honest word about a stop the reader would otherwise read as
    // "still loading".
    if meta.stalled {
        body = body.push(
            text("scroll for more of the store")
                .size(size::TINY)
                .color(theme::DIM),
        );
    }

    container(
        scrollable(container(body).padding(density.pad()))
            .on_scroll(|v| Message::HomeScrolled(v.into()))
            .height(Length::Fill)
            .width(Length::Fill),
    )
    .height(Length::Fill)
    .into()
}

/// The whole of a list, or the head of it — the overview truncates so a grid
/// cell stays a glance; the focused section is where the rest lives.
fn head_of<T>(rows: &[T], full: bool, n: usize) -> &[T] {
    if full {
        rows
    } else {
        rows.get(..n.min(rows.len())).unwrap_or(rows)
    }
}

/// "showing 5 of 23", or nothing when that is the whole of it. A truncated
/// list must say it is truncated, or the overview reads as the whole story.
fn shown_of(len: usize, full: bool, n: usize) -> Option<String> {
    (!full && len > n).then(|| format!("{n} of {len}"))
}

fn keys_panel(
    panels: &Panels,
    accent: theme::Accent,
    full: bool,
) -> Element<'static, crate::window::Message> {
    let mut list = column![].spacing(2);
    for k in head_of(&panels.keys, full, OVERVIEW_KEYS) {
        let best = k
            .best_level
            .map_or_else(|| DASH.to_string(), |l| format!("+{l}"));
        list = list.push(line(
            k.name.clone(),
            format!("{best} · {} runs · {} timed", k.runs, k.timed),
            theme::DIM,
        ));
    }
    nav::panel(
        "mythic+",
        shown_of(panels.keys.len(), full, OVERVIEW_KEYS),
        list,
        None,
        accent,
    )
}

fn raid_card(
    panels: &Panels,
    accent: theme::Accent,
    full: bool,
) -> Element<'static, crate::window::Message> {
    let down = panels
        .raid
        .bosses
        .iter()
        .filter(|b| b.best_kill_ms.is_some())
        .count();
    let mut list = column![].spacing(2);
    for b in head_of(&panels.raid.bosses, full, OVERVIEW_BOSSES) {
        list = list.push(boss_line(b));
    }
    nav::panel(
        "raid",
        // The denominator a boss roster would give is not in any card, so
        // the caption counts what the cards know.
        Some(format!("{down} down · {} seen", panels.raid.bosses.len())),
        list,
        None,
        accent,
    )
}

fn me_card(panels: &Panels, accent: theme::Accent) -> Element<'static, crate::window::Message> {
    let me = &panels.me;
    let body = if me.name.is_empty() {
        column![
            text("no owner identified — set history_characters in the config")
                .size(size::MICRO)
                .color(theme::DIM),
        ]
    } else {
        column![
            line(
                format!("median {}", me.measure),
                or_dash(me.median),
                Color::WHITE
            ),
            line(
                format!("best {}", me.measure),
                or_dash(me.best),
                theme::GREEN
            ),
            line(
                "deaths / pull".to_string(),
                me.deaths_per_pull
                    .map_or_else(|| DASH.to_string(), |d| format!("{d:.1}")),
                theme::RED
            ),
        ]
        .spacing(2)
    };
    nav::panel(
        "me",
        (!me.name.is_empty()).then(|| format!("{} pulls", me.spark.len())),
        body,
        None,
        accent,
    )
}

fn characters_card(
    meta: &Meta,
    panels: &Panels,
    accent: theme::Accent,
) -> Element<'static, crate::window::Message> {
    use crate::window::Message;
    // A list, not chips: these are characters, and a name in its class color
    // with a fight count is the whole point. Clicking one scopes the "me"
    // panel to it; the scoped one is lit.
    let scoped = meta.character.as_deref();
    let mut list = column![].spacing(2);
    for c in &panels.characters {
        // A character the config names but this season has no card for: it
        // is still yours, and "0 fights" would read as a measurement rather
        // than as "nothing here yet".
        let seen = !c.guid.is_empty();
        let on = seen && Some(c.guid.as_str()) == scoped;
        let color = c
            .class
            .map_or(theme::DIM, |class| theme::accent(Some(class), c.spec).base);
        let row = row![
            text(c.name.clone())
                .size(size::MICRO)
                .color(if on { Color::WHITE } else { color }),
            Space::new().width(Length::Fill),
            text(if seen {
                format!("{} fights", c.fights)
            } else {
                "none this season".to_string()
            })
            .size(size::MICRO)
            .color(theme::DIM)
            .font(Font::MONOSPACE),
        ]
        .spacing(8);
        list = list.push(if seen {
            Element::from(
                iced::widget::mouse_area(row)
                    .on_press(Message::HomeCharacter(Some(c.guid.clone()))),
            )
        } else {
            // Nothing to scope to: no guid, no cards, no press.
            Element::from(row)
        });
    }
    nav::panel(
        "characters",
        (panels.characters.len() == 1).then(|| "alts appear as the store sees them".to_string()),
        list,
        scoped.map(|_| {
            (
                "show the newest character".to_string(),
                Message::HomeCharacter(None),
            )
        }),
        accent,
    )
}

fn recent_card(
    meta: &Meta,
    panels: &Panels,
    accent: theme::Accent,
    full: bool,
) -> Element<'static, crate::window::Message> {
    let mut recent = column![].spacing(2);
    if panels.recent.is_empty() {
        // Words, never an empty frame — and which words depends on whether
        // the store has answered yet.
        recent = recent.push(
            text(if meta.answered {
                "no stored fights yet"
            } else {
                "…"
            })
            .size(size::MICRO)
            .color(theme::DIM),
        );
    }
    for r in head_of(&panels.recent, full, OVERVIEW_RECENT) {
        let level = r.key_level.map_or_else(String::new, |l| format!(" +{l}"));
        recent = recent.push(
            row![
                text(if r.pinned { "★" } else { " " })
                    .size(size::TINY)
                    .color(theme::YELLOW),
                text(format!("{}{level}", r.name))
                    .size(size::MICRO)
                    .color(Color::WHITE),
                Space::new().width(Length::Fill),
                text(r.tag.clone())
                    .size(size::TINY)
                    .color(r.tag_color)
                    .font(Font::MONOSPACE),
                text(duration(r.duration_ms))
                    .size(size::MICRO)
                    .color(theme::DIM)
                    .font(Font::MONOSPACE),
            ]
            .spacing(6),
        );
    }
    nav::panel(
        "recent",
        // Truncated: say how much of the list is on screen. Whole: say how
        // much of the STORE is in hand, which is the §9 progress line.
        shown_of(panels.recent.len(), full, OVERVIEW_RECENT)
            .or_else(|| meta.total.map(|t| format!("{} of {t}", meta.cards))),
        recent,
        None,
        accent,
    )
}

/// One boss row. A kill shows its time; a wipe shows how close it came — in
/// the SAME column, at the same weight, because "best 81%" is an outcome as
/// much as "4:12" is, and the pull count is the caption either way.
fn boss_line(b: &BossLine) -> Element<'static, crate::window::Message> {
    let (headline, color) = match (b.best_kill_ms, b.best_pct) {
        (Some(ms), _) => (duration(ms), theme::GREEN),
        (None, Some(pct)) => (format!("{pct}%"), theme::YELLOW),
        // No kill and no health report: nothing honest to put in the
        // outcome column. Never "100%".
        (None, None) => (DASH.to_string(), theme::DIM),
    };
    row![
        text(format!("{} {}", b.name, b.difficulty_tag))
            .size(size::MICRO)
            .color(Color::WHITE),
        Space::new().width(Length::Fill),
        text(format!("{} pulls", b.pulls))
            .size(size::TINY)
            .color(theme::DIM)
            .font(Font::MONOSPACE),
        text(headline)
            .size(size::MICRO)
            .color(color)
            .font(Font::MONOSPACE)
            .width(Length::Fixed(52.0))
            .align_x(iced::Alignment::End),
    ]
    .spacing(8)
    .into()
}

/// The one line that tells a disabled store from a cold one from a degraded
/// one. `hub.rs` answers all three with an empty card list, so without this
/// the screen would show the same confident nothing for each.
fn state_line(home: &Home) -> Option<String> {
    if let Some(why) = home.disabled_reason.as_deref() {
        return Some(format!("the history store is off — {why}"));
    }
    if !home.answered {
        return Some("reading the history store…".to_string());
    }
    if home.dropped > 0 {
        return Some(format!(
            "{} request(s) the daemon dropped — this is not the whole story",
            home.dropped
        ));
    }
    None
}

/// Is this scroll position asking for more? Deliberately NOT
/// `Viewport::relative_offset`, which divides by `content - viewport` and so
/// hands back a non-finite number for content shorter than its viewport.
/// Where a scroll gesture left the list. The three numbers `wants_more`
/// needs, lifted out of iced's `Viewport` — which has no public constructor,
/// so a message carrying one could never be built in a test.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct ScrollAt {
    pub content_h: f32,
    pub view_h: f32,
    pub offset_y: f32,
}

impl From<scrollable::Viewport> for ScrollAt {
    fn from(v: scrollable::Viewport) -> Self {
        Self {
            content_h: v.content_bounds().height,
            view_h: v.bounds().height,
            offset_y: v.absolute_offset().y,
        }
    }
}

pub(crate) fn wants_more(content_h: f32, view_h: f32, offset_y: f32) -> bool {
    // Nothing to scroll: the reader cannot be asking for more. (This is
    // also why the trigger is computed here rather than taken from
    // `Viewport::relative_offset`, which divides by exactly this
    // difference and hands back a non-finite number for a short list.)
    if content_h <= view_h {
        return false;
    }
    content_h - view_h - offset_y <= SCROLL_TRIGGER
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::window::testkit::simulator;
    use wowdps_daemon::mock::MockDaemon;

    fn cards_from_fixture() -> Vec<FightCard> {
        MockDaemon::fixture()
            .with_history()
            .history()
            .cards()
            .to_vec()
    }

    #[test]
    fn parse_ymd_round_trips_and_rejects_nonsense() {
        assert_eq!(parse_ymd("1970-01-01"), Some(0));
        assert_eq!(parse_ymd("2000-03-01"), Some(951_868_800_000));
        assert_eq!(parse_ymd("2026-08-12"), Some(1_786_492_800_000));
        // 2024 is a leap year, 2026 is not.
        assert!(parse_ymd("2024-02-29").is_some());
        assert_eq!(parse_ymd("2026-02-29"), None);
        for bad in [
            "2026-13-01",
            "2026-02-30",
            "today",
            "",
            "2026-1-1",
            "2026-01-01-01",
        ] {
            assert_eq!(parse_ymd(bad), None, "{bad} parsed");
        }
    }

    #[test]
    fn a_season_is_half_open() {
        let s = Season {
            label: "s3".to_string(),
            start_utc_ms: parse_ymd("2026-01-01"),
            end_utc_ms: parse_ymd("2026-02-01"),
        };
        assert!(s.contains(parse_ymd("2026-01-01").unwrap()));
        assert!(s.contains(parse_ymd("2026-01-31").unwrap()));
        assert!(!s.contains(parse_ymd("2026-02-01").unwrap()));
        assert!(!s.contains(parse_ymd("2025-12-31").unwrap()));
        assert!(Season::default().contains(0), "an unset season is all time");
    }

    #[test]
    fn derive_over_the_fixture_store() {
        let cards = cards_from_fixture();
        assert!(!cards.is_empty(), "the fixture stores fights");
        let owner = cards.iter().find_map(|c| c.owner.clone());
        let panels = derive(&cards, owner.as_deref(), &Season::default(), &[]);
        assert!(!panels.recent.is_empty());
        assert!(!panels.top.is_empty());
        assert_eq!(
            panels,
            derive(&cards, owner.as_deref(), &Season::default(), &[]),
            "the derivation is pure"
        );
        if let Some(owner) = owner {
            let me = cards
                .iter()
                .flat_map(|c| &c.players)
                .find(|p| p.guid == owner)
                .map(|p| p.name.clone());
            assert_eq!(
                Some(panels.me.name.clone()),
                me,
                "the me panel is a real player"
            );
        }
    }

    #[test]
    fn unknowable_numbers_render_as_em_dash() {
        let panels = derive(&[], None, &Season::default(), &[]);
        // No season score card exists at all — the store has no rating, and
        // a stand-in would be a lie rather than a gap.
        assert!(
            !panels.top.iter().any(|s| s.label.contains("score")),
            "a score card was reintroduced: nothing in the store can fill it"
        );
        for s in &panels.top {
            assert_eq!(s.value, DASH, "{s:?} invented a number for an empty store");
        }
        assert_eq!(panels.me.median, None);
        assert_eq!(panels.me.deaths_per_pull, None);
    }

    #[test]
    fn an_empty_store_derives_empty_panels_without_panicking() {
        let panels = derive(&[], None, &Season::default(), &[]);
        assert!(panels.recent.is_empty());
        assert!(panels.keys.is_empty());
        assert!(panels.raid.bosses.is_empty());
        assert!(panels.characters.is_empty());
        assert_eq!(panels.me, MePanel::default());
    }

    fn card_with(owner: Option<&str>, players: Vec<CardPlayer>) -> FightCard {
        FightCard {
            id: "f1".to_string(),
            name: "Boss".to_string(),
            duration_ms: 60_000,
            success: Some(true),
            owner: owner.map(str::to_string),
            players,
            ..FightCard::default()
        }
    }

    fn player(guid: &str, spec: Spec, dps: f64, hps: f64, dtps: f64) -> CardPlayer {
        CardPlayer {
            guid: guid.to_string(),
            name: guid.to_string(),
            class: Some(spec.class()),
            spec: Some(spec),
            damage: (dps * 60.0) as u64,
            dps,
            hps,
            dtps,
            ..CardPlayer::default()
        }
    }

    #[test]
    fn a_card_with_no_owner_contributes_no_character() {
        let cards = vec![card_with(
            None,
            vec![player("G-a", Spec::Fire, 100.0, 0.0, 0.0)],
        )];
        assert!(
            derive(&cards, None, &Season::default(), &[])
                .characters
                .is_empty()
        );
        let named = vec![card_with(
            Some("G-a"),
            vec![player("G-a", Spec::Fire, 100.0, 0.0, 0.0)],
        )];
        assert_eq!(
            derive(&named, None, &Season::default(), &[])
                .characters
                .len(),
            1
        );
    }

    #[test]
    fn a_configured_character_appears_even_with_no_cards_this_season() {
        let cards = vec![card_with(
            Some("G-a"),
            vec![player("G-a", Spec::Fire, 100.0, 0.0, 0.0)],
        )];
        let configured = vec!["G-a".to_string(), "Alt-Nebula-US".to_string()];
        let chars = derive(&cards, None, &Season::default(), &configured).characters;
        assert_eq!(chars.len(), 2, "{chars:?}");
        let alt = chars.iter().find(|c| c.name == "Alt-Nebula-US").unwrap();
        assert_eq!(alt.fights, 0);
        assert!(alt.guid.is_empty(), "nothing to scope to, and it says so");
        // The logged one is not duplicated by its own config entry.
        assert_eq!(chars.iter().filter(|c| c.name == "G-a").count(), 1);
        // Matching is by name, case-insensitively — the config is hand-typed.
        let shouty = vec!["g-A".to_string()];
        assert_eq!(
            derive(&cards, None, &Season::default(), &shouty)
                .characters
                .len(),
            1
        );
    }

    #[test]
    fn the_median_is_a_median_for_an_even_count() {
        assert_eq!(median_of(&[]), None);
        assert_eq!(median_of(&[7.0]), Some(7.0));
        assert_eq!(median_of(&[1.0, 3.0]), Some(2.0));
        assert_eq!(median_of(&[1.0, 2.0, 3.0]), Some(2.0));
        assert_eq!(median_of(&[1.0, 2.0, 3.0, 100.0]), Some(2.5));
        // Two pulls, one good and one bad: the median is between them, not
        // the better of the two.
        let cards = vec![
            FightCard {
                id: "f1".to_string(),
                start_utc_ms: 2,
                duration_ms: 60_000,
                owner: Some("G-me".to_string()),
                players: vec![player("G-me", Spec::Fire, 300.0, 0.0, 0.0)],
                ..FightCard::default()
            },
            FightCard {
                id: "f2".to_string(),
                start_utc_ms: 1,
                duration_ms: 60_000,
                owner: Some("G-me".to_string()),
                players: vec![player("G-me", Spec::Fire, 100.0, 0.0, 0.0)],
                ..FightCard::default()
            },
        ];
        let me = derive(&cards, Some("G-me"), &Season::default(), &[]).me;
        assert_eq!(me.median, Some(200.0));
        assert_eq!(me.best, Some(300.0));
    }

    #[test]
    fn a_key_says_its_level_once() {
        assert_eq!(dungeon_name("Skyreach +15"), "Skyreach");
        assert_eq!(dungeon_name("Algeth'ar Academy +2"), "Algeth'ar Academy");
        // Not a keystone suffix: leave the name alone.
        assert_eq!(dungeon_name("The Ashen Warden"), "The Ashen Warden");
        assert_eq!(dungeon_name("Halls of Valor +"), "Halls of Valor +");
        assert_eq!(dungeon_name("Weird +x"), "Weird +x");
    }

    #[test]
    fn role_measure_follows_the_subject() {
        let cases = [
            (Spec::ProtectionWarrior, "dtps"),
            (Spec::HolyPriest, "hps"),
            (Spec::Fire, "effective dps"),
        ];
        for (spec, expect) in cases {
            let p = player("G-me", spec, 100.0, 200.0, 300.0);
            let cards = vec![card_with(Some("G-me"), vec![p])];
            let panels = derive(&cards, Some("G-me"), &Season::default(), &[]);
            assert_eq!(panels.me.measure, expect, "{spec:?}");
        }
    }

    #[test]
    fn a_stale_answer_is_dropped_and_pages_dedupe() {
        let cards = cards_from_fixture();
        let mut home = Home::new();
        let req = home.next_request(1, &Season::default());
        assert!(req.is_some());
        assert_eq!(
            home.next_request(2, &Season::default()),
            None,
            "one request in flight at a time"
        );
        let answer = HistoryAnswer::Fights {
            cards: cards.clone(),
            total: cards.len() as u32,
        };
        home.absorb(99, &answer);
        assert!(home.cards.is_empty(), "an unknown req_id lands nowhere");
        home.absorb(1, &answer);
        assert_eq!(home.cards.len(), cards.len());
        home.absorb(1, &answer);
        assert_eq!(home.cards.len(), cards.len(), "a second fold dedupes");
        assert!(home.complete());
        assert_eq!(
            home.next_request(3, &Season::default()),
            None,
            "a complete list stops asking"
        );
    }

    #[test]
    fn a_burst_stops_at_its_ceiling_and_a_scroll_reopens_it() {
        let mut home = Home::new();
        for i in 0..MAX_PAGES {
            assert!(home.next_request(i, &Season::default()).is_some());
            home.absorb(
                i,
                &HistoryAnswer::Fights {
                    cards: Vec::new(),
                    total: u32::MAX,
                },
            );
        }
        assert_eq!(home.next_request(99, &Season::default()), None);
        home.scrolled_to_end();
        assert!(home.next_request(99, &Season::default()).is_some());
    }

    #[test]
    fn the_screen_renders_loading_empty_and_populated() {
        let accent = theme::NEUTRAL;
        let season = Season {
            label: "season 3".to_string(),
            ..Season::default()
        };
        // Loading: the reader is told the store is being read, and nothing
        // is asserted as a number yet.
        let loading = Home::new();
        let mut ui = simulator(screen(
            &loading,
            &Panels::default(),
            &season,
            accent,
            Density::Comfortable,
        ));
        assert!(ui.find("reading the history store…").is_ok());
        assert!(
            ui.find("no stored fights yet").is_err(),
            "not empty, unread"
        );
        let _ = ui.snapshot(&iced::Theme::TokyoNight).unwrap();

        // Answered and empty: the opposite words, and no invented numbers.
        let mut empty = Home::new();
        empty.answered = true;
        let mut ui = simulator(screen(
            &empty,
            &Panels::default(),
            &season,
            accent,
            Density::Comfortable,
        ));
        assert!(ui.find("no stored fights yet").is_ok());
        assert!(ui.find("reading the history store…").is_err());
        assert!(ui.find("season 3").is_ok(), "the season is named");
        let _ = ui.snapshot(&iced::Theme::TokyoNight).unwrap();

        // Off, and degraded: three different empty screens, as decisions §3
        // requires — never the same confident nothing.
        let mut off = Home::new();
        off.answered = true;
        off.disabled_reason = Some("history_enabled = false".to_string());
        let mut ui = simulator(screen(
            &off,
            &Panels::default(),
            &season,
            accent,
            Density::Comfortable,
        ));
        assert!(
            ui.find("the history store is off — history_enabled = false")
                .is_ok()
        );

        let mut degraded = Home::new();
        degraded.answered = true;
        degraded.dropped = 3;
        let mut ui = simulator(screen(
            &degraded,
            &Panels::default(),
            &season,
            accent,
            Density::Comfortable,
        ));
        assert!(
            ui.find("3 request(s) the daemon dropped — this is not the whole story")
                .is_ok()
        );

        // Populated: the panels' real content is on screen.
        let cards = cards_from_fixture();
        let owner = cards.iter().find_map(|c| c.owner.clone());
        let mut full = Home::new();
        full.absorb_for_test(cards.clone());
        let panels = derive(&cards, owner.as_deref(), &season, &[]);
        let mut ui = simulator(screen(
            &full,
            &panels,
            &season,
            theme::accent(Some(Class::Mage), None),
            Density::Comfortable,
        ));
        assert!(ui.find("recent").is_ok());
        assert!(
            ui.find(panels.recent[0].name.as_str()).is_ok(),
            "the newest stored fight is listed"
        );
        assert!(
            ui.find(format!("{} of {}", cards.len(), cards.len()).as_str())
                .is_ok(),
            "the caption counts what is held against what matched"
        );
        assert!(ui.find("reading the history store…").is_err());
        let _ = ui.snapshot(&iced::Theme::TokyoNight).unwrap();
    }

    /// A section that lost its content between the click and the next
    /// answer must say so, not show an empty frame.
    #[test]
    fn a_focused_section_with_nothing_in_it_says_so() {
        let mut home = Home::new();
        home.answered = true;
        home.section = Section::Keys;
        let mut ui = simulator(laid_out(
            &Meta::of(&home),
            &Panels::default(),
            &Season::default(),
            theme::NEUTRAL,
            Density::Comfortable,
            900.0,
        ));
        assert!(ui.find("nothing here this season").is_ok());

        let mut unread = Home::new();
        unread.section = Section::Keys;
        let mut ui = simulator(laid_out(
            &Meta::of(&unread),
            &Panels::default(),
            &Season::default(),
            theme::NEUTRAL,
            Density::Comfortable,
            900.0,
        ));
        assert!(ui.find("reading the history store…").is_ok());
    }

    /// The overview truncates; focusing the section is the only way to the
    /// rest, so the two must actually differ.
    #[test]
    fn a_focused_section_shows_more_than_the_overview() {
        let recent: Vec<RecentLine> = (0..OVERVIEW_RECENT + 7)
            .map(|i| RecentLine {
                fight_id: format!("f{i}"),
                name: format!("Pull {i}"),
                tag: "KILL".to_string(),
                tag_color: theme::GREEN,
                duration_ms: 60_000,
                pinned: false,
                key_level: None,
            })
            .collect();
        let panels = Panels {
            recent: recent.clone(),
            ..Panels::default()
        };
        let mut home = Home::new();
        home.answered = true;
        let overview = simulator(recent_card(
            &Meta::of(&home),
            &panels,
            theme::NEUTRAL,
            false,
        ));
        let mut overview = overview;
        let last = recent.last().unwrap().name.clone();
        assert!(overview.find(last.as_str()).is_err(), "the overview cuts");
        assert!(
            overview
                .find(format!("{OVERVIEW_RECENT} of {}", recent.len()).as_str())
                .is_ok(),
            "and says that it cut"
        );
        let mut focused = simulator(recent_card(&Meta::of(&home), &panels, theme::NEUTRAL, true));
        assert!(focused.find(last.as_str()).is_ok(), "the section shows all");
    }

    /// The grid is the whole point of the responsive layout: a wide window
    /// gets columns, a narrow one gets one.
    #[test]
    fn the_panel_grid_follows_the_width() {
        assert_eq!(
            columns_for(460.0, 8.0),
            1,
            "the default window is one column"
        );
        assert_eq!(columns_for(1396.0, 8.0), 3, "a tiled window is three");
        assert_eq!(columns_for(700.0, 8.0), 2);
        // Degenerate widths must not divide by anything.
        assert_eq!(columns_for(0.0, 8.0), 1);
        assert_eq!(columns_for(f32::NAN, 8.0), 1);
        assert_eq!(
            columns_for(f32::INFINITY, 8.0),
            1,
            "a nonsense width is one column, not MAX_COLS of nothing"
        );
    }

    #[test]
    fn a_wide_home_still_shows_every_panel() {
        let cards = cards_from_fixture();
        let owner = cards.iter().find_map(|c| c.owner.clone());
        let season = Season::default();
        let panels = derive(&cards, owner.as_deref(), &season, &[]);
        let mut full = Home::new();
        full.absorb_for_test(cards);
        let el = laid_out(
            &Meta::of(&full),
            &panels,
            &season,
            theme::NEUTRAL,
            Density::Comfortable,
            1396.0,
        );
        let mut ui = simulator(el);
        for section in sections(&panels) {
            assert!(
                ui.find(section.name()).is_ok(),
                "{section:?} is missing its chip"
            );
        }
        let _ = ui.snapshot(&iced::Theme::TokyoNight).unwrap();
    }

    /// A wipe's "how close" must sit where a kill's time sits, in the same
    /// column, or it reads as a formatting bug.
    #[test]
    fn a_no_kill_boss_keeps_the_outcome_column() {
        let kill = BossLine {
            name: "Ulgrax".to_string(),
            difficulty_tag: "M",
            best_kill_ms: Some(252_000),
            pulls: 9,
            ..BossLine::default()
        };
        let wipe = BossLine {
            name: "Verkath".to_string(),
            difficulty_tag: "M",
            best_pct: Some(81),
            pulls: 23,
            ..BossLine::default()
        };
        let unknown = BossLine {
            name: "Nobody".to_string(),
            pulls: 2,
            ..BossLine::default()
        };
        let mut ui = simulator(boss_line(&kill));
        assert!(ui.find("4:12").is_ok());
        assert!(ui.find("9 pulls").is_ok());
        let mut ui = simulator(boss_line(&wipe));
        assert!(ui.find("81%").is_ok(), "the outcome, not a sentence");
        assert!(ui.find("23 pulls").is_ok());
        let mut ui = simulator(boss_line(&unknown));
        assert!(ui.find(DASH).is_ok(), "nothing logged is a dash, not 100%");
    }

    impl Home {
        /// Fold cards in without a round trip, for render tests.
        fn absorb_for_test(&mut self, cards: Vec<FightCard>) {
            let total = cards.len() as u32;
            self.pending = Some(0);
            self.absorb(0, &HistoryAnswer::Fights { cards, total });
        }
    }
}
