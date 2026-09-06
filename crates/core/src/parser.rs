//! WoW advanced combat log line parser.
//!
//! Layout is verified against the WowCoach.gg machine-readable spec
//! (`format_version: 22`, `verified_against_patch: "12.0+"`), cross-checked against
//! warcraft.wiki.gg. Where the two disagree the spec wins — see `design-core.md`.
//!
//! Field indices below are into the comma-split of the text *after* the timestamp:
//! `0` is the event name, `1..=8` the base unit block. There is **no** `hideCaster`
//! field in the file format (that is the in-game API shape only).

use std::borrow::Cow;

use wowdps_model::{GearItem, MissKind, TalentPick};

/// Number of fields in the advanced-combat-logging block. The wiki says 17; that is
/// wrong for current retail — two always-zero fields sit between `absorb` and
/// `power_type`.
const ADVANCED_LEN: usize = 19;

const FLAG_TYPE_PLAYER: u32 = 0x0000_0400;
// Only `is_pet_or_guardian` reads these, and that method is contract-mandated
// public API rather than something this binary calls — see its comment below.
#[allow(dead_code)]
const FLAG_TYPE_PET: u32 = 0x0000_1000;
#[allow(dead_code)]
const FLAG_TYPE_GUARDIAN: u32 = 0x0000_2000;

/// A parsed log line. `ts_ms` is monotonic within a file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogLine {
    pub ts_ms: i64,
    pub event: Event,
    /// Pet ownership learned from the advanced block, when it carries one. Additive to
    /// the contract (the contract mandates advanced-field `ownerGUID` attribution but
    /// `Event` has no slot for it). Callers that only care about `event` can ignore it.
    pub owner_hint: Option<OwnerHint>,
    /// The advanced block's health report for the unit it describes (post-event).
    /// Like `owner_hint`: additive, ignorable, and carried on *any* advanced line —
    /// including ones whose `event` is `Other` (e.g. SWING_DAMAGE_LANDED), which is
    /// how the meter back-fills HP for events whose own block describes the source.
    pub hp_hint: Option<HpHint>,
}

impl LogLine {
    pub fn new(ts_ms: i64, event: Event) -> Self {
        Self {
            ts_ms,
            event,
            owner_hint: None,
            hp_hint: None,
        }
    }
}

/// "`unit_guid` is owned by `owner_guid`", as reported by the advanced block.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OwnerHint {
    pub unit_guid: String,
    pub owner_guid: String,
}

/// "`unit_guid` is at `current`/`max` health", as reported by the advanced block.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HpHint {
    pub unit_guid: String,
    pub current: u64,
    pub max: u64,
    /// The described unit's own unit flags — the source's or the
    /// destination's, whichever the block's guid names (0 when neither) —
    /// so a consumer can tell a hostile NPC from a friendly guardian (R16).
    pub flags: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Unit {
    pub guid: String,
    pub name: String,
    pub flags: u32,
}

impl Unit {
    /// A GUID of all zeros means "no unit". Real logs emit such lines carrying PLAYER
    /// flags anyway, so the nil check must come BEFORE the flag test or the meter grows
    /// a phantom player row.
    fn is_nil(&self) -> bool {
        self.guid.is_empty() || self.guid == ZERO_GUID
    }

    pub fn is_player(&self) -> bool {
        !self.is_nil() && (self.flags & FLAG_TYPE_PLAYER != 0 || self.guid.starts_with("Player-"))
    }

    /// Flag bits only — never the GUID prefix. Player-summoned units are not always
    /// `Pet-` GUIDs (an Efflorescence totem is a `Creature-`), and conversely ownership
    /// is established by `SPELL_SUMMON`/`ownerGUID`, not by this predicate.
    // Required by CONTRACT.md's `impl Unit`, but nothing calls it: pet damage is
    // attributed through `SPELL_SUMMON`/`ownerGUID` in `meter.rs`, and the TUI
    // never inspects units at all. Kept as contract surface, exercised by the
    // tests below; the allow is what a binary crate needs for unused pub items.
    #[allow(dead_code)]
    pub fn is_pet_or_guardian(&self) -> bool {
        !self.is_nil() && self.flags & (FLAG_TYPE_PET | FLAG_TYPE_GUARDIAN) != 0
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Spell {
    pub id: u32,
    pub name: String,
    pub school: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuraType {
    Buff,
    Debuff,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    Version {
        log_version: u32,
        advanced: bool,
        /// BUILD_VERSION as (major, minor, patch); zeros when absent.
        build: (u16, u16, u16),
        /// PROJECT_ID (1 = retail); 0 when absent.
        project_id: u8,
    },
    EncounterStart {
        id: u32,
        name: String,
        difficulty: u32,
        group_size: u32,
    },
    EncounterEnd {
        id: u32,
        name: String,
        success: bool,
    },
    CombatantInfo {
        guid: String,
        /// currentSpecID (field 25 on real retail lines); the meter maps it to a class.
        spec_id: Option<u32>,
        /// Field 2 ("faction"). R13: inside an arena this is the player's
        /// SIDE (0/1) — the index ARENA_MATCH_END's winner is judged
        /// against. (ARENA_MATCH_START's trailing teamID field is a dead
        /// constant 0 in real logs and useless for the verdict.)
        faction: u32,
        /// The line's talent bracket `[(nodeId,entryId,rank),…]`. Empty when
        /// the bracket is absent or unbalanced — never a parse failure.
        talents: Vec<TalentPick>,
        /// The line's gear bracket `[(itemId,ilvl,(enchants),(bonusIds),(gems)),…]`,
        /// in the log's inventory-slot order. Empty on absence, like `talents`.
        gear: Vec<GearItem>,
    },
    /// R10: the player moved zones. A nonzero `difficulty` marks instanced
    /// content (dungeon, keystone, raid, delve); the open world logs 0.
    ZoneChange {
        map_id: u32,
        name: String,
        difficulty: u32,
    },
    /// R13: an arena match began (gates opening). The home side is NOT here
    /// (the line's trailing teamID is always 0) — it comes from the match's
    /// own COMBATANT_INFO lines (`faction`) crossed with unit flags.
    ArenaMatchStart {
        map_id: u32,
        match_type: String,
    },
    /// R13: the arena match resolved.
    ArenaMatchEnd {
        winning_team: u32,
    },
    /// R10: a keystone was activated inside the current instance.
    /// `challenge_id` is the MapChallengeMode row — the key for the
    /// generated par-timer table.
    ChallengeModeStart {
        map_id: u32,
        challenge_id: u32,
        key_level: u32,
    },
    /// R10: the keystone run resolved. The game also fires a zeroed reset
    /// form on entry, before any `ChallengeModeStart` — the meter ignores
    /// ends for visits that never keyed. `total_ms` is the official run
    /// time from the game's own timer, death penalties included.
    ChallengeModeEnd {
        map_id: u32,
        success: bool,
        total_ms: i64,
    },
    Damage {
        src: Unit,
        dst: Unit,
        spell: Option<Spell>,
        amount: u64,
        overkill: i64,
        absorbed: u64,
        /// R17: the partially blocked part; the log's `amount` is post-block.
        blocked: u64,
        critical: bool,
        periodic: bool,
    },
    /// R17: a swing or spell that did not land (`*_MISSED`). `prevented` is
    /// the BLOCK amount or the ABSORB `amountMissed`, else 0 — damage the
    /// miss stopped outright, never damage taken.
    Missed {
        src: Unit,
        dst: Unit,
        /// `None` for a melee swing.
        spell: Option<Spell>,
        kind: MissKind,
        off_hand: bool,
        prevented: u64,
    },
    Heal {
        src: Unit,
        dst: Unit,
        spell: Spell,
        /// Total healing *including* overheal (the canonical log value).
        amount: u64,
        overheal: u64,
        absorbed: u64,
        critical: bool,
    },
    Absorbed {
        src: Unit,
        dst: Unit,
        absorber: Unit,
        /// The damage spell that got absorbed; `None` when the hit was a melee swing.
        spell: Option<Spell>,
        /// The shield spell doing the absorbing (always present).
        absorb_spell: Spell,
        amount: u64,
    },
    Interrupt {
        src: Unit,
        dst: Unit,
        spell: Spell,
        interrupted_spell: Spell,
    },
    AuraApplied {
        src: Unit,
        dst: Unit,
        spell: Spell,
        aura_type: AuraType,
    },
    /// R12/v13: the aura coming off again — what turns a marker into a span.
    /// Only mark durations read these; they never open or extend a segment.
    AuraRemoved {
        src: Unit,
        dst: Unit,
        spell: Spell,
        aura_type: AuraType,
    },
    Dispel {
        src: Unit,
        dst: Unit,
        spell: Spell,
        dispelled_spell: Spell,
    },
    /// R12: a cast that actually went off. The meter uses these for one
    /// thing only — telling a trinket the player *used* from one that fired
    /// on its own — so no cast ever opens or extends a segment.
    Cast {
        src: Unit,
        spell: Spell,
    },
    Summon {
        owner: Unit,
        pet: Unit,
    },
    Death {
        unit: Unit,
    },
    /// Recognised as a log line but not modelled. Never an error.
    Other,
}

const ZERO_GUID: &str = "0000000000000000";

const GUID_PREFIXES: [&str; 7] = [
    "Player-",
    "Pet-",
    "Creature-",
    "Vehicle-",
    "GameObject-",
    "BattlePet-",
    "Vignette-",
];

/// True for anything that can occupy the advanced block's `info_guid` slot. Every field
/// that sits there when advanced logging is OFF is an integer or a bare keyword
/// (`BUFF`, `Falling`, a miss type), so this probe cannot false-positive.
pub(crate) fn is_guid(s: &str) -> bool {
    s == ZERO_GUID || GUID_PREFIXES.iter().any(|p| s.starts_with(p))
}

/// Quote-aware CSV split. `None` on an unterminated quote (a truncated line).
/// Byte offset of the timestamp/CSV separator and its width: a tab (1) or the
/// first doubled space (2), whichever comes first. `None` when neither is
/// present, which is not a combat-log line.
fn separator(line: &str) -> Option<(usize, usize)> {
    let b = line.as_bytes();
    for (i, &c) in b.iter().enumerate() {
        if c == b'\t' {
            return Some((i, 1));
        }
        if c == b' ' && b.get(i + 1) == Some(&b' ') {
            return Some((i, 2));
        }
    }
    None
}

/// Split the event CSV, borrowing from `s` rather than copying it.
///
/// Building a `String` per field cost 261 ms of the 315 ms `parse_line` spent
/// on an 86 MB segment — 8.9M fields, 28.5 to the line. Borrowing instead
/// takes 95 ms and, on that log, allocates for *no* field at all.
///
/// The scan walks bytes, not chars: `,` and `"` are ASCII, so a byte scan can
/// never land inside a multi-byte UTF-8 sequence, and every boundary it
/// reports is a char boundary. Quotes are stripped, not preserved, exactly as
/// the char-by-char version did — a field quoted only at its ends borrows the
/// slice between them, and only the pathological rest build a `String`.
fn split_csv(s: &str) -> Option<Vec<Cow<'_, str>>> {
    // Advanced-log lines run ~28 fields; starting there costs one allocation
    // instead of the five doublings a growing Vec needs to reach it.
    let mut out = Vec::with_capacity(32);
    let mut start = 0usize;
    let mut in_quotes = false;
    let mut quotes = 0u32;

    for (i, &b) in s.as_bytes().iter().enumerate() {
        match b {
            b'"' => {
                in_quotes = !in_quotes;
                quotes += 1;
            }
            b',' if !in_quotes => {
                out.push(csv_field(s.get(start..i)?, quotes));
                start = i + 1;
                quotes = 0;
            }
            _ => {}
        }
    }
    if in_quotes {
        return None;
    }
    out.push(csv_field(s.get(start..)?, quotes));
    Some(out)
}

/// One field with its quotes removed, borrowed whenever that costs nothing.
fn csv_field(raw: &str, quotes: u32) -> Cow<'_, str> {
    if quotes == 0 {
        return Cow::Borrowed(raw);
    }
    if quotes == 2 {
        // `"Name-Realm"`: the only quoted shape the log actually emits. Both
        // of the field's two quotes are accounted for by the prefix and the
        // suffix, so what is left between them cannot hold another — no need
        // to scan it again to find out.
        if let Some(inner) = raw.strip_prefix('"').and_then(|r| r.strip_suffix('"')) {
            return Cow::Borrowed(inner);
        }
    }
    Cow::Owned(raw.replace('"', ""))
}

/// Days since the Unix epoch. Howard Hinnant's civil-date algorithm.
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// `M/D/YYYY HH:MM:SS.mmm-7` (retail) or `M/D HH:MM:SS.mmm` (legacy).
///
/// The timezone offset is deliberately ignored: it is constant within a file, and only
/// monotonic deltas are required. When the year is absent it defaults to a fixed value
/// — again, only deltas matter.
pub(crate) fn parse_timestamp(s: &str) -> Option<i64> {
    let (date, time) = s.trim().split_once(' ')?;

    let mut dp = date.split('/');
    let month: i64 = dp.next()?.parse().ok()?;
    let day: i64 = dp.next()?.parse().ok()?;
    let year: i64 = match dp.next() {
        Some(y) => y.parse().ok()?,
        None => 2000,
    };
    if dp.next().is_some() || !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return None;
    }

    // Trim the trailing timezone offset ("-7", "+2", "-04:00").
    let time = match time.find(['+', '-']) {
        Some(i) => time.get(..i)?,
        None => time,
    };
    let mut tp = time.split(':');
    let hour: i64 = tp.next()?.parse().ok()?;
    let min: i64 = tp.next()?.parse().ok()?;
    let secs = tp.next()?;
    let (sec, millis) = match secs.split_once('.') {
        Some((s, frac)) => {
            let digits: String = frac
                .chars()
                .filter(|c| c.is_ascii_digit())
                .take(3)
                .collect();
            if digits.is_empty() {
                return None;
            }
            let scale = 10_i64.pow(3 - digits.len() as u32);
            (s.parse::<i64>().ok()?, digits.parse::<i64>().ok()? * scale)
        }
        None => (secs.parse::<i64>().ok()?, 0),
    };
    if hour > 23 || min > 59 || sec > 60 {
        return None;
    }

    Some(
        days_from_civil(year, month, day) * 86_400_000
            + hour * 3_600_000
            + min * 60_000
            + sec * 1_000
            + millis,
    )
}

/// `"12.0.0"` → `(12, 0, 0)`; anything unparseable → zeros (a missing
/// build is data, never a parse failure).
fn parse_build(s: &str) -> (u16, u16, u16) {
    let mut it = s.trim().split('.').map(|p| p.parse::<u16>().unwrap_or(0));
    let mut next = || it.next().unwrap_or(0);
    (next(), next(), next())
}

/// The line's timezone offset in minutes east of UTC (`-7` → -420,
/// `+05:30` → 330), or `None` for legacy `M/D` timestamps that carry no
/// year and no offset. Read once per log by the history store: `ts_ms` is
/// a local-time epoch, and this is what turns it into UTC.
pub fn tz_offset_min(line: &str) -> Option<i16> {
    let line = line.trim_end_matches(['\n', '\r']);
    let (idx, _) = separator(line)?;
    let (date, time) = line.get(..idx)?.trim().split_once(' ')?;
    if date.split('/').count() < 3 {
        return None;
    }
    let i = time.find(['+', '-'])?;
    let sign: i16 = if time.as_bytes().get(i) == Some(&b'-') {
        -1
    } else {
        1
    };
    let off = time.get(i + 1..)?;
    let (h, m) = match off.split_once(':') {
        Some((h, m)) => (h.parse::<i16>().ok()?, m.parse::<i16>().ok()?),
        None => (off.parse::<i16>().ok()?, 0),
    };
    if h > 14 || m > 59 {
        return None;
    }
    Some(sign * (h * 60 + m))
}

fn parse_u32(s: &str) -> u32 {
    if let Some(hex) = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
        u32::from_str_radix(hex, 16).unwrap_or(0)
    } else {
        s.parse().unwrap_or(0)
    }
}

fn parse_u64(s: &str) -> u64 {
    s.parse().unwrap_or(0)
}

fn parse_i64(s: &str) -> i64 {
    s.parse().unwrap_or(0)
}

/// Combat-log booleans are `1` / `nil`.
fn truthy(s: &str) -> bool {
    !matches!(s, "nil" | "0" | "")
}

fn get<'a>(f: &'a [Cow<'_, str>], i: usize) -> Option<&'a str> {
    f.get(i).map(AsRef::as_ref)
}

/// Unit block: `guid, name, flags, raidFlags` at `i`.
fn unit_at(f: &[Cow<'_, str>], i: usize) -> Unit {
    let name = get(f, i + 1).unwrap_or_default();
    Unit {
        guid: get(f, i).unwrap_or_default().to_string(),
        // The literal `nil` stands in for "no unit"; don't surface it as a display name.
        name: if name == "nil" {
            String::new()
        } else {
            name.to_string()
        },
        flags: parse_u32(get(f, i + 2).unwrap_or_default()),
    }
}

/// Spell block: `id, name, school` at `i`.
fn spell_at(f: &[Cow<'_, str>], i: usize) -> Spell {
    Spell {
        id: parse_u32(get(f, i).unwrap_or_default()),
        name: get(f, i + 1).unwrap_or_default().to_string(),
        school: parse_u32(get(f, i + 2).unwrap_or_default()),
    }
}

fn aura_type(s: &str) -> AuraType {
    if s.eq_ignore_ascii_case("BUFF") {
        AuraType::Buff
    } else {
        AuraType::Debuff
    }
}

/// Events that restate damage already logged elsewhere. Counting them double-counts.
fn is_duplicate_event(ev: &str) -> bool {
    ev.ends_with("_SUPPORT")          // Augmentation Evoker: same hit, logged twice
        || ev == "SWING_DAMAGE_LANDED" // same swing as SWING_DAMAGE, target's view
        || ev == "DAMAGE_SPLIT"        // defensive mechanic, not offensive damage
        || ev == "SPELL_HEAL_ABSORBED"
}

pub(crate) fn is_damage_event(ev: &str) -> bool {
    matches!(
        ev,
        "SWING_DAMAGE"
            | "SPELL_DAMAGE"
            | "SPELL_PERIODIC_DAMAGE"
            | "RANGE_DAMAGE"
            | "SPELL_BUILDING_DAMAGE"
            | "DAMAGE_SHIELD"
            | "ENVIRONMENTAL_DAMAGE"
    )
}

/// R17: the miss families. Never combat for the scanner (nothing here opens
/// a segment); the meter records them into an already-open one.
pub(crate) fn is_missed_event(ev: &str) -> bool {
    matches!(
        ev,
        "SWING_MISSED"
            | "SPELL_MISSED"
            | "SPELL_PERIODIC_MISSED"
            | "RANGE_MISSED"
            | "DAMAGE_SHIELD_MISSED"
    )
}

/// Parse one log line. `None` for blank or malformed lines; unknown events yield
/// `Event::Other`. Never panics.
pub fn parse_line(line: &str) -> Option<LogLine> {
    let line = line.trim_end_matches(['\n', '\r']);
    if line.trim().is_empty() {
        return None;
    }

    // Timestamp and event CSV are separated by two spaces (a tab on some
    // clients). Finding both with str::find meant two generic pattern
    // searches over the line to keep only the earlier hit; one byte scan
    // stopping at the first of either is the same answer for less work, and
    // no tie is possible because a byte cannot be both a tab and a space.
    let (idx, skip) = separator(line)?;
    let ts_ms = parse_timestamp(line.get(..idx)?)?;

    let rest = line.get(idx + skip..)?.trim_start();
    if rest.is_empty() {
        return None;
    }
    // COMBATANT_INFO is the one event whose brackets a comma split shreds
    // (split_csv is quote-aware, not bracket-aware), so it is scanned raw —
    // which also skips building the ~460-field Vec these lines would cost.
    if let Some(body) = rest.strip_prefix("COMBATANT_INFO,") {
        return Some(LogLine::new(ts_ms, parse_combatant_info(body)));
    }
    let f = split_csv(rest)?;
    if f.first().is_none_or(|e| e.is_empty()) {
        return None;
    }

    Some(parse_event(&f, ts_ms))
}

/// COMBATANT_INFO, from the text after `"COMBATANT_INFO,"`. Fields 1 (guid),
/// 2 (faction) and 25 (currentSpecID) are scalars preceding the first `[`;
/// then come the talent bracket, a PvP/stats tuple (skipped), the gear
/// bracket, and the auras bracket (ignored). Anything malformed degrades to
/// empty vectors or `None` — this event never fails a line.
fn parse_combatant_info(body: &str) -> Event {
    let first = body.find('[');
    let scalars = body.get(..first.unwrap_or(body.len())).unwrap_or_default();
    let mut f = scalars.split(',');
    let guid = f.next().unwrap_or_default().to_string();
    let faction = parse_u32(f.next().unwrap_or_default());
    // Line field 25 = body index 24; two next() calls consumed 0 and 1.
    let spec_id = f.nth(22).and_then(|v| v.parse().ok());

    let mut talents = Vec::new();
    let mut gear = Vec::new();
    if let Some(open) = first
        && let Some(t_end) = bracket_end(body, open)
    {
        talents = parse_talent_bracket(body.get(open + 1..t_end).unwrap_or_default());
        if let Some(g_open) = body
            .get(t_end + 1..)
            .and_then(|s| s.find('['))
            .map(|i| i + t_end + 1)
            && let Some(g_end) = bracket_end(body, g_open)
        {
            gear = parse_gear_bracket(body.get(g_open + 1..g_end).unwrap_or_default());
        }
    }
    Event::CombatantInfo {
        guid,
        spec_id,
        faction,
        talents,
        gear,
    }
}

/// Index of the `]` balancing the `[` at `open`, or `None` when the bracket
/// never closes. Parens don't nest brackets and bracket contents carry no
/// quotes, so a bare depth counter is exact.
fn bracket_end(s: &str, open: usize) -> Option<usize> {
    let mut depth = 0usize;
    for (i, &b) in s.as_bytes().iter().enumerate().skip(open) {
        match b {
            b'[' => depth += 1,
            b']' => {
                depth -= 1;
                if depth == 0 {
                    return Some(i);
                }
            }
            _ => {}
        }
    }
    None
}

/// Interiors of the depth-0 `(...)` groups of `s`. A stray `)` ends the walk
/// rather than guessing at what the rest means.
fn paren_groups(s: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut depth = 0usize;
    let mut start = 0usize;
    for (i, &b) in s.as_bytes().iter().enumerate() {
        match b {
            b'(' => {
                if depth == 0 {
                    start = i + 1;
                }
                depth += 1;
            }
            b')' => {
                if depth == 0 {
                    return out;
                }
                depth -= 1;
                if depth == 0
                    && let Some(g) = s.get(start..i)
                {
                    out.push(g);
                }
            }
            _ => {}
        }
    }
    out
}

/// Split `s` on the commas at paren depth 0, so a gear tuple's nested
/// `(enchants)`/`(bonusIds)`/`(gems)` lists survive as single elements.
fn split_top(s: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut depth = 0usize;
    let mut start = 0usize;
    for (i, &b) in s.as_bytes().iter().enumerate() {
        match b {
            b'(' => depth += 1,
            b')' => depth = depth.saturating_sub(1),
            b',' if depth == 0 => {
                if let Some(p) = s.get(start..i) {
                    out.push(p);
                }
                start = i + 1;
            }
            _ => {}
        }
    }
    if let Some(p) = s.get(start..) {
        out.push(p);
    }
    out
}

/// `(a,b,c)` → the numbers inside; `()` → empty. Non-numbers are dropped.
fn u32_list(s: &str) -> Vec<u32> {
    s.trim()
        .strip_prefix('(')
        .and_then(|r| r.strip_suffix(')'))
        .map(|inner| {
            inner
                .split(',')
                .filter_map(|v| v.trim().parse().ok())
                .collect()
        })
        .unwrap_or_default()
}

/// `[(nodeId,entryId,rank),…]` interior → picks; malformed tuples drop out
/// individually.
fn parse_talent_bracket(inner: &str) -> Vec<TalentPick> {
    paren_groups(inner)
        .into_iter()
        .filter_map(|g| {
            let mut p = g.split(',');
            Some(TalentPick {
                node_id: p.next()?.trim().parse().ok()?,
                entry_id: p.next()?.trim().parse().ok()?,
                rank: p.next()?.trim().parse().ok()?,
            })
        })
        .collect()
}

/// `[(itemId,ilvl,(enchants),(bonusIds),(gems)),…]` interior → items, in the
/// log's slot order. Trailing elements a future patch appends are ignored.
/// The array is positional (slot = index), so a malformed item becomes an
/// EMPTY slot (`item_id: 0`) rather than dropping out — dropping it would
/// shift every later item into the wrong slot.
fn parse_gear_bracket(inner: &str) -> Vec<GearItem> {
    paren_groups(inner)
        .into_iter()
        .map(|g| {
            let parts = split_top(g);
            let id = parts.first().and_then(|s| s.trim().parse().ok());
            let ilvl = parts.get(1).and_then(|s| s.trim().parse().ok());
            match (id, ilvl) {
                (Some(item_id), Some(ilvl)) => GearItem {
                    item_id,
                    ilvl,
                    enchants: parts.get(2).map(|s| u32_list(s)).unwrap_or_default(),
                    bonus_ids: parts.get(3).map(|s| u32_list(s)).unwrap_or_default(),
                    gems: parts.get(4).map(|s| u32_list(s)).unwrap_or_default(),
                },
                _ => GearItem::default(),
            }
        })
        .collect()
}

fn parse_event(f: &[Cow<'_, str>], ts_ms: i64) -> LogLine {
    let ev = f.first().map_or("", AsRef::as_ref);
    let plain = |event| LogLine::new(ts_ms, event);

    // Metadata events carry no base unit block.
    match ev {
        "COMBAT_LOG_VERSION" => {
            return plain(Event::Version {
                log_version: parse_u32(get(f, 1).unwrap_or_default()),
                advanced: truthy(get(f, 3).unwrap_or_default()),
                build: parse_build(get(f, 5).unwrap_or_default()),
                project_id: get(f, 7).unwrap_or_default().parse().unwrap_or(0),
            });
        }
        "ENCOUNTER_START" => {
            return plain(Event::EncounterStart {
                id: parse_u32(get(f, 1).unwrap_or_default()),
                name: get(f, 2).unwrap_or_default().to_string(),
                difficulty: parse_u32(get(f, 3).unwrap_or_default()),
                group_size: parse_u32(get(f, 4).unwrap_or_default()),
            });
        }
        "ENCOUNTER_END" => {
            return plain(Event::EncounterEnd {
                id: parse_u32(get(f, 1).unwrap_or_default()),
                name: get(f, 2).unwrap_or_default().to_string(),
                // Offset 5; the trailing duration_ms at 6 is optional.
                success: truthy(get(f, 5).unwrap_or_default()),
            });
        }
        "ZONE_CHANGE" => {
            return plain(Event::ZoneChange {
                map_id: parse_u32(get(f, 1).unwrap_or_default()),
                name: get(f, 2).unwrap_or_default().to_string(),
                difficulty: parse_u32(get(f, 3).unwrap_or_default()),
            });
        }
        // ARENA_MATCH_START,mapID,unk,matchType,teamID (teamID: dead, always 0)
        "ARENA_MATCH_START" => {
            return plain(Event::ArenaMatchStart {
                map_id: parse_u32(get(f, 1).unwrap_or_default()),
                match_type: get(f, 3).unwrap_or_default().to_string(),
            });
        }
        // ARENA_MATCH_END,winningTeam,matchDurationSecs,newRating1,newRating2
        "ARENA_MATCH_END" => {
            return plain(Event::ArenaMatchEnd {
                winning_team: parse_u32(get(f, 1).unwrap_or_default()),
            });
        }
        // CHALLENGE_MODE_START,"Name",mapID,challengeID,level,[affixes]
        "CHALLENGE_MODE_START" => {
            return plain(Event::ChallengeModeStart {
                map_id: parse_u32(get(f, 2).unwrap_or_default()),
                challenge_id: parse_u32(get(f, 3).unwrap_or_default()),
                key_level: parse_u32(get(f, 4).unwrap_or_default()),
            });
        }
        // CHALLENGE_MODE_END,mapID,success,level,totalMs,...
        "CHALLENGE_MODE_END" => {
            return plain(Event::ChallengeModeEnd {
                map_id: parse_u32(get(f, 1).unwrap_or_default()),
                success: truthy(get(f, 2).unwrap_or_default()),
                total_ms: parse_u32(get(f, 4).unwrap_or_default()) as i64,
            });
        }
        // COMBATANT_INFO never reaches here — parse_line routes it to
        // parse_combatant_info before the comma split.
        _ => {}
    }

    if f.len() < 9 {
        return plain(Event::Other);
    }

    // SPELL_ABSORBED has no advanced block and genuinely variable arity (19 vs 22
    // fields), so it is the one event indexed from the END. The relative offsets below
    // are identical in both forms — verified against real 19- and 22-field lines.
    if ev == "SPELL_ABSORBED" {
        let n = f.len();
        if n < 19 {
            return plain(Event::Other);
        }
        return plain(Event::Absorbed {
            src: unit_at(f, 1),
            dst: unit_at(f, 5),
            absorber: unit_at(f, n - 10),
            // The damage-spell block is present only in the longer form.
            spell: (n >= 22).then(|| spell_at(f, 9)),
            absorb_spell: spell_at(f, n - 6),
            amount: parse_u64(get(f, n - 3).unwrap_or_default()),
        });
    }

    // Locate the advanced block, then index the suffix FORWARD from it. Indexing from
    // the end is unsafe: SWING_DAMAGE omits `is_off_hand` on main-hand swings.
    let prefix_len = if ev.starts_with("SPELL_")
        || ev.starts_with("RANGE_")
        || ev.starts_with("DAMAGE_SHIELD")
    {
        3
    } else {
        0
    };
    let adv_start = 9 + prefix_len;
    let advanced = f.get(adv_start).is_some_and(|s| is_guid(s));
    let suffix = adv_start + if advanced { ADVANCED_LEN } else { 0 };

    // The advanced block describes the SOURCE on SWING_DAMAGE but the TARGET on
    // SPELL_*/RANGE_* events. Keying the hint on info_guid is correct either way.
    let owner_hint = if advanced {
        let info = get(f, adv_start).unwrap_or_default();
        let owner = get(f, adv_start + 1).unwrap_or_default();
        (owner != ZERO_GUID && is_guid(owner)).then(|| OwnerHint {
            unit_guid: info.to_string(),
            owner_guid: owner.to_string(),
        })
    } else {
        None
    };
    let hp_hint = if advanced {
        let info = get(f, adv_start).unwrap_or_default();
        let current = parse_u64(get(f, adv_start + 2).unwrap_or_default());
        let max = parse_u64(get(f, adv_start + 3).unwrap_or_default());
        // Field 1 is the source guid (flags at 3), field 5 the destination
        // (flags at 7); the block names one of them.
        let flags = if get(f, 1) == Some(info) {
            parse_u32(get(f, 3).unwrap_or_default())
        } else if get(f, 5) == Some(info) {
            parse_u32(get(f, 7).unwrap_or_default())
        } else {
            0
        };
        (info != ZERO_GUID && is_guid(info) && max > 0).then(|| HpHint {
            unit_guid: info.to_string(),
            current,
            max,
            flags,
        })
    } else {
        None
    };
    let with_hint = |event| LogLine {
        ts_ms,
        event,
        owner_hint: owner_hint.clone(),
        hp_hint: hp_hint.clone(),
    };

    // Double-logged damage is never counted, but its advanced block still
    // carries a fresh HP report — SWING_DAMAGE_LANDED is the target's view of
    // a swing, exactly what back-fills the recap entry its SWING_DAMAGE twin
    // opened with no HP.
    if is_duplicate_event(ev) {
        return with_hint(Event::Other);
    }

    let spell = (prefix_len == 3).then(|| spell_at(f, 9));

    if is_damage_event(ev) {
        // ENVIRONMENTAL_DAMAGE prepends envType to the suffix.
        let s = suffix + usize::from(ev == "ENVIRONMENTAL_DAMAGE");
        let Some(amount) = get(f, s) else {
            return with_hint(Event::Other);
        };
        if f.len() <= s + 7 {
            return with_hint(Event::Other);
        }
        // R17: ENVIRONMENTAL_DAMAGE has no spell block; its envType ("Falling",
        // "Lava" …) becomes the ability label so Taken never reads "Melee".
        let spell = if ev == "ENVIRONMENTAL_DAMAGE" {
            Some(Spell {
                id: 0,
                name: get(f, suffix).unwrap_or_default().to_string(),
                school: parse_u32(get(f, s + 3).unwrap_or_default()),
            })
        } else {
            spell
        };
        return with_hint(Event::Damage {
            src: unit_at(f, 1),
            dst: unit_at(f, 5),
            spell,
            // suffix[0] is base_amount (post-mitigation, canonical);
            // suffix[1] is raw_amount (pre-mitigation, diagnostics only).
            amount: parse_u64(amount),
            overkill: parse_i64(get(f, s + 2).unwrap_or_default()),
            absorbed: parse_u64(get(f, s + 6).unwrap_or_default()),
            blocked: parse_u64(get(f, s + 5).unwrap_or_default()),
            critical: truthy(get(f, s + 7).unwrap_or_default()),
            periodic: ev.contains("_PERIODIC_"),
        });
    }

    if is_missed_event(ev) {
        // R17: no advanced block; the tail is `missType, isOffHand[, amount
        // [, unmitigated, critical]]` and SPELL_* lines trail an `ST` / `AOE`
        // token — so index FORWARD from missType, never from the end.
        let m = suffix;
        let Some(kind) = get(f, m).and_then(MissKind::parse) else {
            return with_hint(Event::Other);
        };
        let prevented = match kind {
            MissKind::Block | MissKind::Absorb => parse_u64(get(f, m + 2).unwrap_or_default()),
            _ => 0,
        };
        return with_hint(Event::Missed {
            src: unit_at(f, 1),
            dst: unit_at(f, 5),
            spell,
            kind,
            off_hand: truthy(get(f, m + 1).unwrap_or_default()),
            prevented,
        });
    }

    match ev {
        "SPELL_HEAL" | "SPELL_PERIODIC_HEAL" => {
            // With the advanced block the suffix is 5 fields led by `healed_to_hp`,
            // which is NOT the heal amount (it is zero when a heal is fully converted
            // to a shield, e.g. Death Strike). Without it, `amount` leads.
            let h = suffix + usize::from(f.len() >= suffix + 5);
            if f.len() <= h + 3 {
                return with_hint(Event::Other);
            }
            with_hint(Event::Heal {
                src: unit_at(f, 1),
                dst: unit_at(f, 5),
                spell: spell.unwrap_or_default(),
                amount: parse_u64(get(f, h).unwrap_or_default()),
                overheal: parse_u64(get(f, h + 1).unwrap_or_default()),
                absorbed: parse_u64(get(f, h + 2).unwrap_or_default()),
                critical: truthy(get(f, h + 3).unwrap_or_default()),
            })
        }
        "SPELL_INTERRUPT" => {
            if f.len() <= suffix + 2 {
                return with_hint(Event::Other);
            }
            with_hint(Event::Interrupt {
                src: unit_at(f, 1),
                dst: unit_at(f, 5),
                spell: spell.unwrap_or_default(),
                interrupted_spell: spell_at(f, suffix),
            })
        }
        "SPELL_DISPEL" | "SPELL_STOLEN" => {
            if f.len() <= suffix + 2 {
                return with_hint(Event::Other);
            }
            with_hint(Event::Dispel {
                src: unit_at(f, 1),
                dst: unit_at(f, 5),
                spell: spell.unwrap_or_default(),
                dispelled_spell: spell_at(f, suffix),
            })
        }
        "SPELL_AURA_APPLIED" => {
            let Some(kind) = get(f, suffix) else {
                return with_hint(Event::Other);
            };
            with_hint(Event::AuraApplied {
                src: unit_at(f, 1),
                dst: unit_at(f, 5),
                spell: spell.unwrap_or_default(),
                // The optional trailing absorb amount sits AFTER this; never read it
                // as a stack count.
                aura_type: aura_type(kind),
            })
        }
        "SPELL_AURA_REMOVED" => {
            let Some(kind) = get(f, suffix) else {
                return with_hint(Event::Other);
            };
            with_hint(Event::AuraRemoved {
                src: unit_at(f, 1),
                dst: unit_at(f, 5),
                spell: spell.unwrap_or_default(),
                aura_type: aura_type(kind),
            })
        }
        "SPELL_CAST_SUCCESS" => with_hint(Event::Cast {
            src: unit_at(f, 1),
            spell: spell.unwrap_or_default(),
        }),
        "SPELL_SUMMON" => with_hint(Event::Summon {
            owner: unit_at(f, 1),
            pet: unit_at(f, 5),
        }),
        "UNIT_DIED" => with_hint(Event::Death {
            unit: unit_at(f, 5),
        }),
        _ => with_hint(Event::Other),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TS: &str = "7/27/2026 21:03:11.472-4";

    /// The 19-field advanced block, parameterised by the unit it describes and its owner.
    fn adv(info: &str, owner: &str) -> String {
        format!(
            "{info},{owner},125000,180000,4200,0,8500,0,0,0,3,95,100,0,1234.56,-987.65,2222,3.14,639"
        )
    }

    fn line(body: &str) -> String {
        format!("{TS}  {body}")
    }

    fn parse(body: &str) -> Event {
        parse_line(&line(body)).expect("should parse").event
    }

    const PLAYER: &str = r#"Player-1168-0A234B,"Thrall-Ragnaros",0x511,0x0"#;
    const HEALER: &str = r#"Player-1168-0B999C,"Moira-Ragnaros",0x512,0x0"#;
    const BOSS: &str = r#"Creature-0-4232-2662-31585-214502-0001,"Ulgrax the Devourer",0xa48,0x0"#;
    const BOSS_GUID: &str = "Creature-0-4232-2662-31585-214502-0001";
    // Real logs use 0x80000000 (not 0x0) for "no raid marker" — that is > i32::MAX.
    const NIL_UNIT: &str = "0000000000000000,nil,0x80000000,0x80000000";

    // ---- timestamps -------------------------------------------------------

    #[test]
    fn parses_retail_timestamp_with_tz_offset() {
        let a = parse_line(&line("SPELL_CAST_SUCCESS,x")).unwrap();
        let b = parse_line("7/27/2026 21:03:12.472-4  SPELL_CAST_SUCCESS,x").unwrap();
        assert_eq!(b.ts_ms - a.ts_ms, 1000, "one second apart");
    }

    #[test]
    fn parses_legacy_timestamp_without_year_or_tz() {
        let a = parse_line("7/27 21:03:11.000  SPELL_CAST_SUCCESS,x").unwrap();
        let b = parse_line("7/27 21:03:11.250  SPELL_CAST_SUCCESS,x").unwrap();
        assert_eq!(b.ts_ms - a.ts_ms, 250);
    }

    #[test]
    fn timestamp_is_monotonic_across_midnight() {
        let a = parse_line("7/27/2026 23:59:59.000-4  SPELL_CAST_SUCCESS,x").unwrap();
        let b = parse_line("7/28/2026 00:00:01.000-4  SPELL_CAST_SUCCESS,x").unwrap();
        assert_eq!(b.ts_ms - a.ts_ms, 2000);
    }

    #[test]
    fn accepts_tab_separator() {
        assert!(parse_line("7/27/2026 21:03:11.472-4\tSPELL_CAST_SUCCESS,x").is_some());
    }

    // ---- metadata ---------------------------------------------------------

    #[test]
    fn build_and_project_default_to_zero_when_absent() {
        let e = parse("COMBAT_LOG_VERSION,20,ADVANCED_LOG_ENABLED,1");
        assert_eq!(
            e,
            Event::Version {
                log_version: 20,
                advanced: true,
                build: (0, 0, 0),
                project_id: 0,
            }
        );
    }

    #[test]
    fn tz_offset_reads_the_timestamp_suffix() {
        let line = |ts: &str| format!("{ts}  SPELL_CAST_SUCCESS,x");
        assert_eq!(tz_offset_min(&line("7/27/2026 20:05:00.000-4")), Some(-240));
        assert_eq!(tz_offset_min(&line("7/27/2026 20:05:00.000+2")), Some(120));
        assert_eq!(
            tz_offset_min(&line("7/27/2026 20:05:00.000+05:30")),
            Some(330)
        );
        assert_eq!(
            tz_offset_min(&line("7/27/2026 20:05:00.000-04:00")),
            Some(-240)
        );
        // Legacy M/D lines carry neither a year nor an offset.
        assert_eq!(tz_offset_min(&line("7/27 20:05:00.000")), None);
        assert_eq!(tz_offset_min(&line("7/27/2026 20:05:00.000")), None);
        assert_eq!(tz_offset_min(&line("7/27/2026 20:05:00.000-99")), None);
        assert_eq!(tz_offset_min("garbage"), None);
    }

    #[test]
    fn parses_combat_log_version() {
        let e =
            parse("COMBAT_LOG_VERSION,22,ADVANCED_LOG_ENABLED,1,BUILD_VERSION,12.0.0,PROJECT_ID,1");
        assert_eq!(
            e,
            Event::Version {
                log_version: 22,
                advanced: true,
                build: (12, 0, 0),
                project_id: 1,
            }
        );
    }

    #[test]
    fn parses_encounter_start() {
        let e = parse(r#"ENCOUNTER_START,2917,"Ulgrax the Devourer",14,20,2657"#);
        assert_eq!(
            e,
            Event::EncounterStart {
                id: 2917,
                name: "Ulgrax the Devourer".into(),
                difficulty: 14,
                group_size: 20,
            }
        );
    }

    #[test]
    fn parses_encounter_end_kill_and_wipe() {
        let kill = parse(r#"ENCOUNTER_END,2917,"Ulgrax the Devourer",14,20,1,183000"#);
        assert_eq!(
            kill,
            Event::EncounterEnd {
                id: 2917,
                name: "Ulgrax the Devourer".into(),
                success: true
            }
        );
        // Trailing duration_ms is optional and absent here.
        let wipe = parse(r#"ENCOUNTER_END,2917,"Ulgrax the Devourer",14,20,0"#);
        assert_eq!(
            wipe,
            Event::EncounterEnd {
                id: 2917,
                name: "Ulgrax the Devourer".into(),
                success: false
            }
        );
    }

    #[test]
    fn parses_arena_match_start_and_end() {
        // Real retail lines, verified against a live log.
        let start = parse("ARENA_MATCH_START,1552,0,Skirmish,0");
        assert_eq!(
            start,
            Event::ArenaMatchStart {
                map_id: 1552,
                match_type: "Skirmish".into(),
            }
        );
        let end = parse("ARENA_MATCH_END,1,61,0,0");
        assert_eq!(end, Event::ArenaMatchEnd { winning_team: 1 });
    }

    #[test]
    fn parses_combatant_info_guid() {
        // A short line (no spec field) parses with None; the brackets still yield
        // their picks (first bracket) and gear (next bracket — empty here).
        let e = parse("COMBATANT_INFO,Player-1168-0A234B,0,7549,3591,[(1,2,3),(4,5,6)],[],(0,0)");
        assert_eq!(
            e,
            Event::CombatantInfo {
                guid: "Player-1168-0A234B".into(),
                spec_id: None,
                faction: 0,
                talents: vec![
                    TalentPick {
                        node_id: 1,
                        entry_id: 2,
                        rank: 3
                    },
                    TalentPick {
                        node_id: 4,
                        entry_id: 5,
                        rank: 6
                    },
                ],
                gear: vec![],
            }
        );
    }

    #[test]
    fn parses_combatant_info_spec_id_from_a_real_shaped_line() {
        // Real retail line shape (build 12.0.7): 24 stat fields after the guid, then
        // currentSpecID at field 25, then the talent brackets. Spec 70 = Ret Paladin.
        let e = parse(
            "COMBATANT_INFO,Player-5-0E9E6142,1,2129,217,26548,664,0,0,0,0,968,968,968,221,0,668,668,668,0,1062,73,73,73,2361,70,[(81523,102493,1)],[],(0,0)",
        );
        assert_eq!(
            e,
            Event::CombatantInfo {
                guid: "Player-5-0E9E6142".into(),
                spec_id: Some(70),
                faction: 1,
                talents: vec![TalentPick {
                    node_id: 81523,
                    entry_id: 102493,
                    rank: 1
                }],
                gear: vec![],
            }
        );
    }

    #[test]
    fn combatant_info_gear_bracket_yields_items_and_skips_the_pvp_tuple() {
        // Fixture shape: talents, then the (0,0) PvP/stats tuple, then gear with
        // nested enchant/bonus/gem lists, then the aura bracket (ignored). The
        // rank-0 pick (a granted node) survives as written.
        let e = parse(
            "COMBATANT_INFO,Player-1168-0A1B2C01,0,12480,3140,980,6420,0,0,0,3120,3120,3120,410,220,4870,4870,4870,190,3960,5210,5210,5210,0,0,71,[(91024,124871,1),(91025,124872,1),(91026,124873,0)],(0,0),[(212446,639,(),(6652,10356),()),(212449,639,(),(6652),(213743))],[(Player-1168-0A1B2C02,17,Player-1168-0A1B2C01,1126)]",
        );
        assert_eq!(
            e,
            Event::CombatantInfo {
                guid: "Player-1168-0A1B2C01".into(),
                spec_id: Some(71),
                faction: 0,
                talents: vec![
                    TalentPick {
                        node_id: 91024,
                        entry_id: 124871,
                        rank: 1
                    },
                    TalentPick {
                        node_id: 91025,
                        entry_id: 124872,
                        rank: 1
                    },
                    TalentPick {
                        node_id: 91026,
                        entry_id: 124873,
                        rank: 0
                    },
                ],
                gear: vec![
                    GearItem {
                        item_id: 212446,
                        ilvl: 639,
                        enchants: vec![],
                        bonus_ids: vec![6652, 10356],
                        gems: vec![],
                    },
                    GearItem {
                        item_id: 212449,
                        ilvl: 639,
                        enchants: vec![],
                        bonus_ids: vec![6652],
                        gems: vec![213743],
                    },
                ],
            }
        );
    }

    #[test]
    fn combatant_info_malformed_gear_item_holds_its_slot() {
        // The gear array is positional: a corrupt tuple must become an empty
        // slot, never vanish and shift every later item into the wrong slot.
        let e = parse(
            "COMBATANT_INFO,Player-1168-0A1B2C01,0,1,1,1,1,0,0,0,1,1,1,1,1,1,1,1,1,1,1,1,1,0,0,71,[],(0,0),[(212446,639,(),(),()),(garbage),(212449,639,(),(),())],[]",
        );
        let Event::CombatantInfo { gear, .. } = e else {
            panic!("not COMBATANT_INFO: {e:?}");
        };
        assert_eq!(gear.len(), 3);
        assert_eq!(gear[0].item_id, 212446);
        assert_eq!(
            gear[1],
            GearItem::default(),
            "the corrupt tuple is an empty slot"
        );
        assert_eq!(gear[2].item_id, 212449, "the item after it keeps its slot");
    }

    #[test]
    fn combatant_info_unbalanced_bracket_degrades_to_empty_vectors() {
        // A truncated line (mid-write tail read) must never fail: scalars parse,
        // the unbalanced bracket yields nothing.
        let e = parse(
            "COMBATANT_INFO,Player-5-0E9E6142,1,2129,217,26548,664,0,0,0,0,968,968,968,221,0,668,668,668,0,1062,73,73,73,2361,70,[(81523,102493,1),(8152",
        );
        assert_eq!(
            e,
            Event::CombatantInfo {
                guid: "Player-5-0E9E6142".into(),
                spec_id: Some(70),
                faction: 1,
                talents: vec![],
                gear: vec![],
            }
        );
    }

    // ---- damage -----------------------------------------------------------

    #[test]
    fn advanced_lines_carry_an_hp_hint_even_when_the_event_is_dropped() {
        // SPELL_DAMAGE: the block describes the target, post-hit.
        let l = parse_line(&line(&format!(
            "SPELL_DAMAGE,{PLAYER},{BOSS},133,\"Fireball\",0x4,{},12345,13000,-1,4,0,0,250,1,nil,nil,ST",
            adv(BOSS_GUID, "0000000000000000")
        )))
        .unwrap();
        assert_eq!(
            l.hp_hint,
            Some(HpHint {
                unit_guid: BOSS_GUID.into(),
                current: 125_000,
                max: 180_000,
                flags: 0xa48,
            })
        );

        // SWING_DAMAGE_LANDED is never counted (Event::Other) but still
        // reports the target's HP — the back-fill path for melee hits.
        let l = parse_line(&line(&format!(
            "SWING_DAMAGE_LANDED,{PLAYER},{BOSS},{},9000,9000,-1,1,0,0,0,nil,nil,nil",
            adv(BOSS_GUID, "0000000000000000")
        )))
        .unwrap();
        assert_eq!(l.event, Event::Other);
        assert!(l.hp_hint.is_some());

        // Non-advanced lines carry none.
        let l = parse_line(&line(&format!(
            "SPELL_DAMAGE,{PLAYER},{BOSS},133,\"Fireball\",0x4,5000,5200,-1,4,0,0,0,1,nil,nil"
        )))
        .unwrap();
        assert_eq!(l.hp_hint, None);
    }

    #[test]
    fn parses_advanced_spell_damage() {
        let e = parse(&format!(
            "SPELL_DAMAGE,{PLAYER},{BOSS},133,\"Fireball\",0x4,{},12345,13000,-1,4,0,0,250,1,nil,nil,ST",
            adv(BOSS_GUID, "0000000000000000")
        ));
        let Event::Damage {
            src,
            spell,
            amount,
            overkill,
            absorbed,
            critical,
            periodic,
            ..
        } = e
        else {
            panic!("expected Damage, got {e:?}")
        };
        assert_eq!(src.name, "Thrall-Ragnaros");
        assert!(src.is_player());
        assert_eq!(spell.unwrap().name, "Fireball");
        // amount is base_amount (offset 31), NOT raw_amount (offset 32).
        assert_eq!(amount, 12345);
        assert_eq!(overkill, -1, "-1 means not a killing blow; meter clamps");
        assert_eq!(absorbed, 250);
        assert!(critical);
        assert!(!periodic);
    }

    #[test]
    fn parses_killing_blow_overkill() {
        let e = parse(&format!(
            "SPELL_DAMAGE,{PLAYER},{BOSS},133,\"Fireball\",0x4,{},9000,9000,3500,4,0,0,0,nil,nil,nil,ST",
            adv(BOSS_GUID, "0000000000000000")
        ));
        let Event::Damage { overkill, .. } = e else {
            panic!()
        };
        assert_eq!(overkill, 3500);
    }

    #[test]
    fn periodic_damage_is_flagged() {
        let e = parse(&format!(
            "SPELL_PERIODIC_DAMAGE,{PLAYER},{BOSS},172,\"Corruption\",0x20,{},800,800,-1,32,0,0,0,nil,nil,nil,ST",
            adv(BOSS_GUID, "0000000000000000")
        ));
        let Event::Damage {
            amount, periodic, ..
        } = e
        else {
            panic!()
        };
        assert_eq!(amount, 800);
        assert!(periodic);
    }

    /// Advanced logging OFF: the GUID probe at the advanced slot must return false.
    #[test]
    fn parses_damage_without_advanced_block() {
        let e = parse(&format!(
            "SPELL_DAMAGE,{PLAYER},{BOSS},133,\"Fireball\",0x4,5000,5200,-1,4,0,0,0,1,nil,nil"
        ));
        let Event::Damage {
            amount, critical, ..
        } = e
        else {
            panic!("got {e:?}")
        };
        assert_eq!(amount, 5000);
        assert!(critical);
    }

    /// A comma inside a quoted spell name must not split the field.
    #[test]
    fn handles_comma_inside_quoted_name() {
        let e = parse(&format!(
            "SPELL_DAMAGE,{PLAYER},{BOSS},999001,\"Blessing of Might, Greater\",0x4,5000,5200,-1,4,0,0,0,nil,nil,nil"
        ));
        let Event::Damage { spell, amount, .. } = e else {
            panic!()
        };
        assert_eq!(spell.unwrap().name, "Blessing of Might, Greater");
        assert_eq!(amount, 5000, "amount must survive the embedded comma");
    }

    // ---- the SWING optional-trailing-field trap ---------------------------

    /// Main-hand swings OMIT `is_off_hand` entirely (38 fields); off-hand swings have it
    /// (39). This pair is the regression test proving suffix-from-end would be wrong.
    #[test]
    fn swing_damage_main_hand_and_off_hand_both_parse() {
        let main = parse(&format!(
            "SWING_DAMAGE,{PLAYER},{BOSS},{},2500,2500,-1,1,0,0,0,nil,nil,nil",
            adv("Player-1168-0A234B", "0000000000000000")
        ));
        let Event::Damage { amount, spell, .. } = main else {
            panic!()
        };
        assert_eq!(amount, 2500);
        assert!(spell.is_none(), "swings have no spell prefix");

        let off = parse(&format!(
            "SWING_DAMAGE,{PLAYER},{BOSS},{},1200,1200,-1,1,0,0,0,nil,nil,nil,1",
            adv("Player-1168-0A234B", "0000000000000000")
        ));
        let Event::Damage { amount, .. } = off else {
            panic!()
        };
        assert_eq!(amount, 1200);
    }

    // ---- double-count traps (R1) ------------------------------------------

    #[test]
    fn swing_damage_landed_is_other() {
        let e = parse(&format!(
            "SWING_DAMAGE_LANDED,{PLAYER},{BOSS},{},2500,2500,-1,1,0,0,0,nil,nil,nil",
            adv(BOSS_GUID, "0000000000000000")
        ));
        assert_eq!(e, Event::Other, "duplicate of SWING_DAMAGE");
    }

    #[test]
    fn support_events_are_other() {
        for ev in [
            "SPELL_DAMAGE_SUPPORT",
            "SPELL_PERIODIC_DAMAGE_SUPPORT",
            "RANGE_DAMAGE_SUPPORT",
            "SPELL_HEAL_SUPPORT",
        ] {
            let e = parse(&format!(
                "{ev},{PLAYER},{BOSS},133,\"Fireball\",0x4,{},12345,13000,-1,4,0,0,0,1,nil,nil,Player-1168-0AEVOK",
                adv(BOSS_GUID, "0000000000000000")
            ));
            assert_eq!(e, Event::Other, "{ev} duplicates the underlying hit");
        }
    }

    #[test]
    fn damage_split_is_other() {
        let e = parse(&format!(
            "DAMAGE_SPLIT,{PLAYER},{BOSS},133,\"Fireball\",0x4,{},100,100,-1,4,0,0,0,nil,nil,nil",
            adv(BOSS_GUID, "0000000000000000")
        ));
        assert_eq!(e, Event::Other);
    }

    // ---- healing ----------------------------------------------------------

    /// The heal amount is suffix[1] (`amount`), not suffix[0] (`healed_to_hp`).
    #[test]
    fn parses_advanced_spell_heal() {
        let e = parse(&format!(
            "SPELL_HEAL,{HEALER},{PLAYER},2061,\"Flash Heal\",0x2,{},140000,20000,5000,0,1",
            adv("Player-1168-0A234B", "0000000000000000")
        ));
        let Event::Heal {
            src,
            dst,
            amount,
            overheal,
            critical,
            ..
        } = e
        else {
            panic!("{e:?}")
        };
        assert_eq!(src.name, "Moira-Ragnaros");
        assert_eq!(dst.name, "Thrall-Ragnaros");
        assert_eq!(amount, 20000, "canonical amount includes overheal");
        assert_eq!(overheal, 5000);
        assert!(critical);
    }

    #[test]
    fn parses_full_overheal() {
        let e = parse(&format!(
            "SPELL_PERIODIC_HEAL,{HEALER},{PLAYER},139,\"Renew\",0x2,{},180000,8000,8000,0,nil",
            adv("Player-1168-0A234B", "0000000000000000")
        ));
        let Event::Heal {
            amount, overheal, ..
        } = e
        else {
            panic!()
        };
        assert_eq!((amount, overheal), (8000, 8000));
    }

    // ---- SPELL_ABSORBED, both arities -------------------------------------

    #[test]
    fn parses_self_shield_absorb_19_fields() {
        let e = parse(&format!(
            "SPELL_ABSORBED,{BOSS},{PLAYER},{PLAYER},17,\"Power Word: Shield\",0x2,3000,9000,nil"
        ));
        let Event::Absorbed {
            absorber,
            spell,
            absorb_spell,
            amount,
            ..
        } = e
        else {
            panic!("{e:?}")
        };
        assert_eq!(absorber.name, "Thrall-Ragnaros");
        assert!(spell.is_none(), "no damage spell on the 19-field form");
        assert_eq!(absorb_spell.name, "Power Word: Shield");
        assert_eq!(amount, 3000);
    }

    #[test]
    fn parses_shield_on_other_absorb_22_fields() {
        let e = parse(&format!(
            "SPELL_ABSORBED,{BOSS},{PLAYER},468731,\"Devouring Bite\",0x1,{HEALER},17,\"Power Word: Shield\",0x2,4500,12000,nil"
        ));
        let Event::Absorbed {
            src,
            dst,
            absorber,
            spell,
            absorb_spell,
            amount,
        } = e
        else {
            panic!("{e:?}")
        };
        assert_eq!(src.name, "Ulgrax the Devourer");
        assert_eq!(dst.name, "Thrall-Ragnaros");
        assert_eq!(
            absorber.name, "Moira-Ragnaros",
            "credit goes to the shield caster"
        );
        assert_eq!(spell.unwrap().name, "Devouring Bite");
        assert_eq!(absorb_spell.name, "Power Word: Shield");
        assert_eq!(amount, 4500);
    }

    // ---- interrupt / dispel / aura ----------------------------------------

    #[test]
    fn parses_interrupt() {
        let e = parse(&format!(
            "SPELL_INTERRUPT,{PLAYER},{BOSS},57994,\"Wind Shear\",0x8,468999,\"Digestive Acid\",0x8"
        ));
        let Event::Interrupt {
            spell,
            interrupted_spell,
            ..
        } = e
        else {
            panic!("{e:?}")
        };
        assert_eq!(spell.name, "Wind Shear");
        assert_eq!(interrupted_spell.name, "Digestive Acid");
    }

    #[test]
    fn parses_dispel() {
        let e = parse(&format!(
            "SPELL_DISPEL,{HEALER},{PLAYER},527,\"Purify\",0x2,468888,\"Carnivorous Contest\",0x20,DEBUFF"
        ));
        let Event::Dispel {
            spell,
            dispelled_spell,
            ..
        } = e
        else {
            panic!("{e:?}")
        };
        assert_eq!(spell.name, "Purify");
        assert_eq!(dispelled_spell.name, "Carnivorous Contest");
    }

    #[test]
    fn parses_aura_applied_debuff() {
        let e = parse(&format!(
            "SPELL_AURA_APPLIED,{PLAYER},{BOSS},118,\"Polymorph\",0x40,DEBUFF"
        ));
        let Event::AuraApplied {
            spell, aura_type, ..
        } = e
        else {
            panic!("{e:?}")
        };
        assert_eq!(spell.id, 118);
        assert_eq!(aura_type, AuraType::Debuff);
    }

    /// The optional trailing absorb amount must not be mistaken for the aura type.
    #[test]
    fn parses_aura_applied_buff_with_trailing_amount() {
        let e = parse(&format!(
            "SPELL_AURA_APPLIED,{HEALER},{PLAYER},17,\"Power Word: Shield\",0x2,BUFF,45000"
        ));
        let Event::AuraApplied { aura_type, .. } = e else {
            panic!("{e:?}")
        };
        assert_eq!(aura_type, AuraType::Buff);
    }

    /// Real logs emit SPELL_AURA_APPLIED at 13, 14 AND 15 fields. aura_type is always
    /// idx12; trailing optionals are ignored and width is never gated on.
    #[test]
    fn aura_applied_tolerates_13_14_and_15_field_widths() {
        for tail in ["DEBUFF", "DEBUFF,45000", "DEBUFF,0,0"] {
            let e = parse(&format!(
                "SPELL_AURA_APPLIED,{PLAYER},{BOSS},118,\"Polymorph\",0x40,{tail}"
            ));
            let Event::AuraApplied {
                aura_type, spell, ..
            } = e
            else {
                panic!("{tail}: {e:?}")
            };
            assert_eq!(aura_type, AuraType::Debuff, "tail {tail:?}");
            assert_eq!(spell.id, 118);
        }
    }

    /// 36 real lines carry a nil sourceGUID with PLAYER flags set. Classifying those as
    /// players grows a phantom row on the meter.
    #[test]
    fn nil_guid_with_player_flags_is_not_a_player() {
        let e = parse(&format!(
            "SPELL_DAMAGE,0000000000000000,nil,0x514,0x80000000,{PLAYER},1249797,\"Shattered Sky\",0x20,3000,3000,-1,32,0,0,0,nil,nil,nil"
        ));
        let Event::Damage { src, .. } = e else {
            panic!("{e:?}")
        };
        assert_eq!(src.flags & 0x400, 0x400, "the PLAYER flag really is set");
        assert!(!src.is_player(), "but a nil GUID is still nobody");
        assert!(!src.is_pet_or_guardian());
    }

    // ---- summon / death / environmental -----------------------------------

    #[test]
    fn parses_summon() {
        let e = parse(
            r#"SPELL_SUMMON,Player-1168-0C777D,"Gul-Ragnaros",0x511,0x0,Pet-0-4232-2662-31585-165189-0100AB,"Felhunter",0x1114,0x0,691,"Summon Felhunter",0x20"#,
        );
        let Event::Summon { owner, pet } = e else {
            panic!("{e:?}")
        };
        assert_eq!(owner.guid, "Player-1168-0C777D");
        assert_eq!(pet.name, "Felhunter");
        assert!(pet.is_pet_or_guardian());
    }

    /// A pet's SWING advanced block describes the SOURCE, so it carries the owner GUID.
    #[test]
    fn swing_advanced_block_yields_owner_hint() {
        let l = parse_line(&line(&format!(
            "SWING_DAMAGE,Pet-0-4232-2662-31585-165189-0200CD,\"Bloodfang\",0x1114,0x0,{BOSS},{},1500,1500,-1,1,0,0,0,nil,nil,nil",
            adv("Pet-0-4232-2662-31585-165189-0200CD", "Player-1168-0D555E")
        )))
        .unwrap();
        assert_eq!(
            l.owner_hint,
            Some(OwnerHint {
                unit_guid: "Pet-0-4232-2662-31585-165189-0200CD".into(),
                owner_guid: "Player-1168-0D555E".into(),
            })
        );
    }

    /// A zero owner GUID is "not a pet" and must not produce a hint.
    #[test]
    fn zero_owner_guid_yields_no_hint() {
        let l = parse_line(&line(&format!(
            "SPELL_DAMAGE,{PLAYER},{BOSS},133,\"Fireball\",0x4,{},1,1,-1,4,0,0,0,nil,nil,nil,ST",
            adv(BOSS_GUID, "0000000000000000")
        )))
        .unwrap();
        assert_eq!(l.owner_hint, None);
    }

    #[test]
    fn parses_unit_died_with_nil_source() {
        let e = parse(&format!("UNIT_DIED,{NIL_UNIT},{PLAYER}"));
        let Event::Death { unit } = e else {
            panic!("{e:?}")
        };
        assert_eq!(unit.name, "Thrall-Ragnaros");
        assert!(unit.is_player());
    }

    /// Verbatim real-log shape: 10 fields, a single trailing `0` (not 11 as docs imply).
    #[test]
    fn parses_real_unit_died_shape() {
        let e = parse(
            r#"UNIT_DIED,0000000000000000,nil,0x80000000,0x80000000,Player-5-0BC007E0,"Dawgoneefour-Proudmoore-US",0x2114,0x80000000,0"#,
        );
        let Event::Death { unit } = e else {
            panic!("{e:?}")
        };
        assert_eq!(unit.name, "Dawgoneefour-Proudmoore-US");
        assert_eq!(unit.flags, 0x2114);
    }

    // ---- real-log corrections (validator, verified against build 12.0.7) ----

    /// `0x80000000` exceeds i32::MAX and appears as the raid-flag on nearly every real
    /// line; unit flags reach five hex digits (`0x10a48`, a vehicle).
    #[test]
    fn flags_parse_as_u32_including_high_bit_and_five_digits() {
        let e = parse(
            r#"UNIT_DIED,0000000000000000,nil,0x80000000,0x80000000,Vehicle-0-3881-2913-77155-240391-0000682736,"L'ura",0x10a48,0x80000000,0"#,
        );
        let Event::Death { unit } = e else {
            panic!("{e:?}")
        };
        assert_eq!(unit.flags, 0x10a48);
        assert_eq!(unit.name, "L'ura", "apostrophes are legal in names");
    }

    /// Schools are inconsistently formatted within a single line: `0x1` hex here,
    /// bare decimal `106` there.
    #[test]
    fn school_accepts_hex_and_bare_decimal() {
        let e = parse(&format!(
            "SPELL_INTERRUPT,{PLAYER},{BOSS},57994,\"Wind Shear\",0x1,468999,\"Digestive Acid\",106"
        ));
        let Event::Interrupt {
            spell,
            interrupted_spell,
            ..
        } = e
        else {
            panic!("{e:?}")
        };
        assert_eq!(spell.school, 1, "0x-prefixed hex");
        assert_eq!(
            interrupted_spell.school, 106,
            "bare decimal on the same line"
        );
    }

    /// A player-summoned unit is not always a `Pet-` GUID — an Efflorescence totem is a
    /// `Creature-` with NPC flags. Ownership comes from the summon, not the GUID shape.
    #[test]
    fn summon_can_target_a_creature_guid() {
        let e = parse(
            r#"SPELL_SUMMON,Player-3676-0EC8A6B9,"Knothot-Area52-US",0x514,0x80000000,Creature-0-3881-2913-77155-47649-00006827C1,"Efflorescence",0xa28,0x80000000,145205,"Efflorescence",0x8"#,
        );
        let Event::Summon { owner, pet } = e else {
            panic!("{e:?}")
        };
        assert_eq!(owner.guid, "Player-3676-0EC8A6B9");
        assert!(pet.guid.starts_with("Creature-"));
        assert!(
            !pet.is_pet_or_guardian(),
            "0xa28 has neither Pet nor Guardian bit — attribution must come from the \
             summon event, not this predicate"
        );
    }

    #[test]
    fn pet_detection_uses_flag_bits_not_guid_prefix() {
        // Guardian bit set on a Creature- GUID.
        let guardian = Unit {
            guid: "Creature-0-1".into(),
            name: "Ebon Gargoyle".into(),
            flags: 0x2114,
        };
        assert!(guardian.is_pet_or_guardian());
        // Pet bit set.
        let pet = Unit {
            guid: "Pet-0-1".into(),
            name: "Felhunter".into(),
            flags: 0x1114,
        };
        assert!(pet.is_pet_or_guardian());
        // A Pet- GUID with no type bits must NOT be classified by its prefix.
        let bare = Unit {
            guid: "Pet-0-1".into(),
            name: "x".into(),
            flags: 0x0,
        };
        assert!(!bare.is_pet_or_guardian());
    }

    #[test]
    fn names_carry_realm_suffixes_apostrophes_and_non_ascii() {
        for (raw, want) in [
            (r#""Fidèle-Tichondrius-US""#, "Fidèle-Tichondrius-US"),
            (r#""Swegbert-Kil'jaeden-US""#, "Swegbert-Kil'jaeden-US"),
            (r#""L'ura""#, "L'ura"),
        ] {
            let e = parse(&format!(
                "SPELL_SUMMON,Player-1-A,{raw},0x511,0x80000000,Pet-0-1,\"P\",0x1114,0x80000000,691,\"S\",0x20"
            ));
            let Event::Summon { owner, .. } = e else {
                panic!("{e:?}")
            };
            assert_eq!(owner.name, want);
        }
    }

    /// envType sits at offset 28, AFTER the advanced block — not at 9 as the wiki says.
    #[test]
    fn parses_environmental_damage() {
        let e = parse(&format!(
            "ENVIRONMENTAL_DAMAGE,{NIL_UNIT},{PLAYER},{},Falling,4000,4000,-1,1,0,0,0,nil,nil,nil",
            adv("Player-1168-0A234B", "0000000000000000")
        ));
        let Event::Damage { src, amount, .. } = e else {
            panic!("{e:?}")
        };
        assert_eq!(
            amount, 4000,
            "envType must be skipped, not read as the amount"
        );
        assert!(!src.is_player(), "null source belongs to nobody");
    }

    // ---- R17: *_MISSED and the partial-mitigation fields ------------------

    fn missed(e: Event) -> (Option<Spell>, MissKind, bool, u64) {
        let Event::Missed {
            spell,
            kind,
            off_hand,
            prevented,
            ..
        } = e
        else {
            panic!("expected Missed, got {e:?}")
        };
        (spell, kind, off_hand, prevented)
    }

    /// SWING_MISSED is 11 fields bare, 12 with a BLOCK amount, 14 with the
    /// ABSORB tail (`amountMissed, unmitigated, critical`).
    #[test]
    fn swing_missed_parses_all_three_widths() {
        let e = parse(&format!("SWING_MISSED,{BOSS},{PLAYER},DODGE,nil"));
        let Event::Missed { src, dst, .. } = &e else {
            panic!("{e:?}")
        };
        assert_eq!(src.name, "Ulgrax the Devourer");
        assert_eq!(dst.guid, "Player-1168-0A234B");
        assert_eq!(missed(e), (None, MissKind::Dodge, false, 0));

        let e = parse(&format!("SWING_MISSED,{BOSS},{PLAYER},BLOCK,nil,60693"));
        assert_eq!(missed(e), (None, MissKind::Block, false, 60693));

        let e = parse(&format!(
            "SWING_MISSED,{BOSS},{PLAYER},ABSORB,nil,12345,15000,nil"
        ));
        assert_eq!(
            missed(e),
            (None, MissKind::Absorb, false, 12345),
            "amountMissed, not unmitigated"
        );
        let e = parse(&format!(
            "SWING_MISSED,{BOSS},{PLAYER},ABSORB,nil,12345,15000,1"
        ));
        assert_eq!(
            missed(e),
            (None, MissKind::Absorb, false, 12345),
            "the critical flag is dropped, not misread"
        );
    }

    /// SPELL_MISSED / SPELL_PERIODIC_MISSED always trail an `ST` / `AOE`
    /// token: 15 / 16 / 18 fields. Indexing from the end would read it as
    /// the amount.
    #[test]
    fn spell_missed_parses_with_the_st_and_aoe_trailer() {
        let e = parse(&format!(
            "SPELL_MISSED,{BOSS},{PLAYER},1449,\"Smash\",1,PARRY,nil,ST"
        ));
        let (spell, kind, off, prevented) = missed(e);
        assert_eq!(spell.as_ref().map(|s| s.name.as_str()), Some("Smash"));
        assert_eq!(spell.map(|s| s.id), Some(1449));
        assert_eq!((kind, off, prevented), (MissKind::Parry, false, 0));

        let e = parse(&format!(
            "SPELL_MISSED,{BOSS},{PLAYER},1449,\"Smash\",1,BLOCK,nil,700,AOE"
        ));
        assert_eq!(missed(e).1, MissKind::Block);
        assert_eq!(
            missed(parse(&format!(
                "SPELL_MISSED,{BOSS},{PLAYER},1449,\"Smash\",1,BLOCK,nil,700,AOE"
            )))
            .3,
            700
        );

        let e = parse(&format!(
            "SPELL_MISSED,{BOSS},{PLAYER},1449,\"Smash\",1,ABSORB,nil,300,340,1,ST"
        ));
        assert_eq!(
            missed(e),
            (
                Some(Spell {
                    id: 1449,
                    name: "Smash".into(),
                    school: 1
                }),
                MissKind::Absorb,
                false,
                300
            )
        );

        let e = parse(&format!(
            "SPELL_PERIODIC_MISSED,{BOSS},{PLAYER},372120,\"Hollow Rot\",0x20,IMMUNE,nil,ST"
        ));
        let (spell, kind, _, _) = missed(e);
        assert_eq!(
            (spell.map(|s| s.school), kind),
            (Some(0x20), MissKind::Immune)
        );
    }

    /// RANGE_MISSED carries the same tail with NO trailer: 14 / 15 / 17.
    #[test]
    fn range_missed_has_no_trailer() {
        let e = parse(&format!(
            "RANGE_MISSED,{PLAYER},{BOSS},75,\"Auto Shot\",1,MISS,nil"
        ));
        assert_eq!(missed(e).1, MissKind::Miss);
        let e = parse(&format!(
            "RANGE_MISSED,{PLAYER},{BOSS},75,\"Auto Shot\",1,BLOCK,nil,500"
        ));
        assert_eq!(
            missed(e),
            (
                Some(Spell {
                    id: 75,
                    name: "Auto Shot".into(),
                    school: 1
                }),
                MissKind::Block,
                false,
                500
            )
        );
        let e = parse(&format!(
            "RANGE_MISSED,{PLAYER},{BOSS},75,\"Auto Shot\",1,ABSORB,nil,900,950,nil"
        ));
        assert_eq!(missed(e).3, 900);
    }

    #[test]
    fn missed_survives_a_quoted_comma_before_the_miss_type() {
        let e = parse(&format!(
            "SWING_MISSED,Creature-0-4232-2662-31585-226403-0001,\"Nek'zali, the Soulcoiler\",0xa48,0x0,{PLAYER},PARRY,nil"
        ));
        let Event::Missed { src, kind, .. } = e else {
            panic!("{e:?}")
        };
        assert_eq!(src.name, "Nek'zali, the Soulcoiler");
        assert_eq!(kind, MissKind::Parry);
    }

    #[test]
    fn missed_off_hand_reads_nil_and_one() {
        let e = parse(&format!("SWING_MISSED,{BOSS},{PLAYER},MISS,nil"));
        assert!(!missed(e).2);
        let e = parse(&format!("SWING_MISSED,{BOSS},{PLAYER},MISS,1"));
        assert!(missed(e).2);
    }

    #[test]
    fn every_observed_miss_type_parses_and_unknown_is_other() {
        for (token, kind) in [
            ("DODGE", MissKind::Dodge),
            ("PARRY", MissKind::Parry),
            ("BLOCK", MissKind::Block),
            ("MISS", MissKind::Miss),
            ("ABSORB", MissKind::Absorb),
            ("IMMUNE", MissKind::Immune),
            ("DEFLECT", MissKind::Deflect),
            ("EVADE", MissKind::Evade),
            ("REFLECT", MissKind::Reflect),
            ("RESIST", MissKind::Resist),
        ] {
            let e = parse(&format!("SWING_MISSED,{BOSS},{PLAYER},{token},nil"));
            assert_eq!(missed(e).1, kind, "{token}");
        }
        let e = parse(&format!("SWING_MISSED,{BOSS},{PLAYER},FROBNICATE,nil"));
        assert_eq!(
            e,
            Event::Other,
            "an unknown missType is Other, never an error"
        );
        let e = parse(&format!("SWING_MISSED,{BOSS},{PLAYER}"));
        assert_eq!(e, Event::Other, "a truncated line is Other");
    }

    #[test]
    fn damage_shield_missed_parses_like_spell_missed() {
        let e = parse(&format!(
            "DAMAGE_SHIELD_MISSED,{PLAYER},{BOSS},7294,\"Retribution Aura\",2,EVADE,nil,ST"
        ));
        let (spell, kind, _, _) = missed(e);
        assert_eq!(spell.map(|s| s.name), Some("Retribution Aura".into()));
        assert_eq!(kind, MissKind::Evade);
    }

    /// `blocked` is at suffix offset +5 on every damage family; a partial
    /// block reads `…,-1,1,0,60693,5355,nil` → blocked 60693, absorbed 5355.
    #[test]
    fn blocked_parses_on_swing_and_spell_damage() {
        let e = parse(&format!(
            "SWING_DAMAGE,{BOSS},{PLAYER},{},64000,124000,-1,1,0,60693,5355,nil,nil,nil",
            adv(BOSS_GUID, "0000000000000000")
        ));
        let Event::Damage {
            amount,
            blocked,
            absorbed,
            critical,
            ..
        } = e
        else {
            panic!("{e:?}")
        };
        assert_eq!(
            (amount, blocked, absorbed, critical),
            (64000, 60693, 5355, false)
        );

        let e = parse(&format!(
            "SPELL_DAMAGE,{BOSS},{PLAYER},1449,\"Smash\",1,{},30000,35000,-1,1,0,4000,1000,1,nil,nil,ST",
            adv("Player-1168-0A234B", "0000000000000000")
        ));
        let Event::Damage {
            amount,
            blocked,
            absorbed,
            critical,
            ..
        } = e
        else {
            panic!("{e:?}")
        };
        assert_eq!(
            (amount, blocked, absorbed, critical),
            (30000, 4000, 1000, true)
        );

        // And without the advanced block the offsets still hold.
        let e = parse(&format!(
            "SWING_DAMAGE,{BOSS},{PLAYER},2500,2500,-1,1,0,700,0,nil,nil,nil"
        ));
        let Event::Damage { blocked, .. } = e else {
            panic!("{e:?}")
        };
        assert_eq!(blocked, 700);
    }

    /// R17: the envType becomes a synthetic spell (id 0) so the Taken drill
    /// reads "Falling", never "Melee".
    #[test]
    fn environmental_damage_is_labelled_by_its_env_type() {
        let e = parse(&format!(
            "ENVIRONMENTAL_DAMAGE,{NIL_UNIT},{PLAYER},{},Lava,4000,4000,-1,4,0,0,0,nil,nil,nil",
            adv("Player-1168-0A234B", "0000000000000000")
        ));
        let Event::Damage { spell, amount, .. } = e else {
            panic!("{e:?}")
        };
        assert_eq!(
            spell,
            Some(Spell {
                id: 0,
                name: "Lava".into(),
                school: 4
            })
        );
        assert_eq!(amount, 4000);
    }

    // ---- unit flags -------------------------------------------------------

    #[test]
    fn unit_classification() {
        let player = Unit {
            guid: "Player-1-A".into(),
            name: "P".into(),
            flags: 0x511,
        };
        assert!(player.is_player());
        assert!(!player.is_pet_or_guardian());

        let pet = Unit {
            guid: "Pet-0-1".into(),
            name: "Felhunter".into(),
            flags: 0x1114,
        };
        assert!(pet.is_pet_or_guardian());
        assert!(!pet.is_player());

        let boss = Unit {
            guid: "Creature-0-1".into(),
            name: "B".into(),
            flags: 0xa48,
        };
        assert!(!boss.is_player());
        assert!(!boss.is_pet_or_guardian());
    }

    // ---- negative control: never panic, never poison the stream ------------

    #[test]
    fn malformed_lines_return_none_without_panicking() {
        let bad = [
            "",
            "   ",
            "\n",
            "7/27/2026 21:03:42.000-4",
            "SPELL_DAMAGE,Player-1,\"x\",0x0,0x0",
            "not/a/timestamp here  SPELL_DAMAGE,a,b",
            "7/27/2026 21:03:42.000-4  ",
            "//  ,,,,",
            "7/27/2026 :: .-4  SPELL_DAMAGE,a",
        ];
        for b in bad {
            assert!(parse_line(b).is_none(), "expected None for {b:?}");
        }
    }

    #[test]
    fn unterminated_quote_is_malformed() {
        assert!(parse_line(&line("SPELL_DAMAGE,Player-1,\"unterminated,0x0")).is_none());
    }

    #[test]
    fn truncated_damage_line_is_not_a_panic() {
        // Real logs get cut mid-write when the game crashes.
        let l = parse_line(&line(&format!(
            "SPELL_DAMAGE,{PLAYER},{BOSS},133,\"Fireball\",0x4,{},12345",
            adv(BOSS_GUID, "0000000000000000")
        )));
        assert!(matches!(
            l,
            None | Some(LogLine {
                event: Event::Other,
                ..
            })
        ));
    }

    /// Whole-file smoke test against a real combat log. Skipped unless
    /// `WOWDPS_REAL_LOG` points at one, so the shared gate stays hermetic.
    ///
    /// Run: `WOWDPS_REAL_LOG=<path> cargo test real_log -- --nocapture`
    #[test]
    fn real_log_parses_without_loss() {
        let Ok(path) = std::env::var("WOWDPS_REAL_LOG") else {
            return;
        };
        let text = std::fs::read_to_string(&path).expect("readable log");

        let (mut total, mut none, mut other, mut dmg, mut heal, mut absorb) = (0, 0, 0, 0, 0, 0);
        let (mut dmg_sum, mut heal_sum) = (0u64, 0u64);
        let (mut cinfo, mut cinfo_talented, mut cinfo_geared) = (0, 0, 0);
        for raw in text.lines() {
            if raw.trim().is_empty() {
                continue;
            }
            total += 1;
            match parse_line(raw) {
                None => {
                    if none < 5 {
                        eprintln!("UNPARSED: {raw}");
                    }
                    none += 1;
                }
                Some(l) => match l.event {
                    Event::Other => other += 1,
                    Event::Damage { amount, .. } => {
                        dmg += 1;
                        dmg_sum += amount;
                    }
                    Event::Heal {
                        amount, overheal, ..
                    } => {
                        heal += 1;
                        heal_sum += amount.saturating_sub(overheal);
                    }
                    Event::Absorbed { .. } => absorb += 1,
                    Event::CombatantInfo {
                        ref talents,
                        ref gear,
                        ..
                    } => {
                        cinfo += 1;
                        // Real max-level players run ~50-80 picks; the gear
                        // dump is the standard 15-18 slot inventory with sane
                        // item levels. A drifted bracket position would land
                        // way outside these bands.
                        if talents.len() >= 30 {
                            cinfo_talented += 1;
                        }
                        if (10..=20).contains(&gear.len()) && gear.iter().all(|g| g.ilvl < 2000) {
                            cinfo_geared += 1;
                        }
                    }
                    _ => {}
                },
            }
        }
        eprintln!(
            "lines={total} unparsed={none} other={other} damage={dmg} heal={heal} \
             absorbed={absorb} dmg_total={dmg_sum} effective_heal={heal_sum} \
             combatant_info={cinfo} talented={cinfo_talented} geared={cinfo_geared}"
        );
        assert_eq!(none, 0, "every line of a real log must parse");
        assert!(dmg > 0 && heal > 0 && absorb > 0, "expected real events");
        // The bracket scan against real 461-508-field lines: every
        // COMBATANT_INFO must yield a plausible full build and gear dump.
        assert!(cinfo > 0, "expected COMBATANT_INFO lines in a real log");
        assert_eq!(cinfo_talented, cinfo, "talent bracket drifted?");
        assert_eq!(cinfo_geared, cinfo, "gear bracket drifted?");
    }

    #[test]
    fn unknown_event_is_other_not_none() {
        let e = parse(&format!(
            "SPELL_CAST_START,{PLAYER},{BOSS},133,\"Fireball\",0x4"
        ));
        assert_eq!(e, Event::Other);
        let e = parse("SOME_FUTURE_EVENT,1,2,3");
        assert_eq!(e, Event::Other);
    }
}
