//! Encounter segmentation and per-player aggregation.
//!
//! Accounting follows CONTRACT.md rulings R1-R6.

use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::Arc;

use crate::parser::{AuraType, Event, HpHint, LogLine, Spell, Unit};
use wowdps_model::{
    Encounter, Healed, ItemKind, Loadout, Mark, MarkKind, MissKind, Mitigation, RoleSpellKind,
    ShieldRow, Support, Timeline,
};

/// R17: Brewmaster Stagger's self-sourced periodic tick — the staggered
/// portion of an earlier hit re-dealt to the monk. Already Taken on the hit
/// it came from, so it is excluded from the Taken view and tallied apart.
const STAGGER_TICK: u32 = 124255;

/// R17: the attacker label a nil-source damage event (falling, lava, an
/// ENVIRONMENTAL_DAMAGE line) earns on the Taken drill.
const ENVIRONMENT: &str = "Environment";

/// A new Trash segment starts after this much combat silence.
/// Shared with the index scanner, which mirrors this rule byte-cheaply.
pub(crate) const TRASH_GAP_MS: i64 = 60_000;

/// Self-absorb effects that are not healing (R2).
pub(crate) const NON_HEALING_ABSORBS: [u32; 4] = [114556, 31850, 31230, 115069];

/// Loss-of-control effects counted by the CrowdControl view. Exactness is not gated.
pub(crate) const CC_SPELLS: &[u32] = &[
    118, 28271, 28272, 61305, 61721, 61780, // Polymorph family
    51514, 210873, 211004, 211010, // Hex family
    3355, 187650, // Freezing Trap
    115078, // Paralysis
    853,    // Hammer of Justice
    20066,  // Repentance
    2094,   // Blind
    408, 1833, // Kidney Shot, Cheap Shot
    6770, // Sap
    5782, 118699, 5484, 8122, 5246, // Fears
    6789, // Mortal Coil
    339, 102359, 64695, 117526, // Roots (incl. Binding Shot)
    122, 33395, 82691, 157997, // Frost Nova / Ring of Frost
    30283, 179057, 211881, 217832, 207685, // Demon Hunter / Warlock
    119381, 108194, 221562, 207167, // Leg Sweep / Asphyxiate / Blinding Sleet
    33786,  // Cyclone
    5211, 99, 22570, // Mighty Bash / Incap Roar / Maim
    132168, 46968, 107570, // Shockwave / Storm Bolt
    31661,  // Dragon's Breath
    197214, // Sundering
    710,    // Banish
];

pub use wowdps_model::{Class, Row, SegmentKind, Spec, View};

/// The log's "no unit" guid — an ENVIRONMENTAL_DAMAGE source, a UNIT_DIED
/// source. Real lines carry PLAYER flags on it anyway (see `Segment::is_player`).
fn nil_guid(guid: &str) -> bool {
    guid.is_empty() || guid == "0000000000000000"
}

/// R18: a unit a role span may land on — a player, or a pet (folded onto
/// its owner at read time, so an external on a pet is the owner's received).
fn span_target(dst: &Unit) -> bool {
    dst.is_player() || dst.guid.starts_with("Pet-")
}

/// Damage sources that count toward naming a pull: the group's own output.
pub(crate) fn is_friendly_source(guid: &str) -> bool {
    guid.starts_with("Player-") || guid.starts_with("Pet-")
}

/// Damage targets that count toward naming a pull: NPC enemies (and training
/// dummies, which is exactly what you want a dummy segment named after).
pub(crate) fn is_hostile_target(guid: &str) -> bool {
    guid.starts_with("Creature-") || guid.starts_with("Vehicle-")
}

/// The display name a Trash segment earns from its enemy tally: most-hit
/// enemy first, ties broken alphabetically so replay and scan agree, `+N`
/// counting the other distinct enemies. `None` until any enemy was hit.
pub(crate) fn trash_name(enemies: &HashMap<String, u64>) -> Option<String> {
    let (top, _) = enemies
        .iter()
        .max_by(|a, b| a.1.cmp(b.1).then(b.0.cmp(a.0)))?;
    let others = enemies.len() - 1;
    Some(if others > 0 {
        format!("{top} +{others}")
    } else {
        top.clone()
    })
}

/// R13: an arena match's display name — "{zone} ({match type})". The zone
/// comes from the preceding difficulty-0 ZONE_CHANGE; a log begun mid-match
/// falls back to the bare "Arena".
pub(crate) fn arena_name(zone: Option<&str>, match_type: &str) -> String {
    let zone = zone.filter(|z| !z.is_empty()).unwrap_or("Arena");
    if match_type.is_empty() {
        zone.to_string()
    } else {
        format!("{zone} ({match_type})")
    }
}

#[derive(Debug, Clone, Default)]
struct Tally {
    amount: u64,
    extra: u64,
    /// Contributing events; `crits` counts the ones flagged critical.
    count: u64,
    crits: u64,
}

impl Tally {
    fn add(&mut self, amount: u64, extra: u64, crit: bool) {
        self.amount += amount;
        self.extra += extra;
        self.count += 1;
        self.crits += crit as u64;
    }

    fn merge(&mut self, other: &Tally) {
        self.amount += other.amount;
        self.extra += other.extra;
        self.count += other.count;
        self.crits += other.crits;
    }
}

/// One entry in a player's death recap (R9): a damage hit on them, or a
/// health gain (heal / consumed absorb), with their health right after it
/// when a line reported it.
#[derive(Debug, Clone)]
struct RecapEntry {
    ts: i64,
    spell: String,
    /// The attacker for damage, the caster for gains.
    src: String,
    amount: u64,
    /// Overkill for damage, overheal for heals.
    extra: u64,
    crit: bool,
    gain: bool,
    hp: Option<(u64, u64)>,
}

/// Recap ring capacity per player — a few seconds of raid combat. Bounded so
/// the meter never becomes an event store (R9).
const RECAP_CAP: usize = 32;

/// R12: the timeline grid. One second is fine enough to see a burst window
/// and coarse enough that an hour of trash costs 3600 u64s per actor; the
/// renderer smooths it into whatever window it wants.
const BUCKET_MS: i64 = 1_000;

/// R12: bucket ceiling per actor (~6 hours). A log with a corrupt clock can
/// name a timestamp far in the future; that must cost a clamp, not a
/// multi-gigabyte allocation.
const MAX_BUCKETS: usize = 21_600;

/// R12: item markers kept per player. A fight has a few dozen; the cap is
/// what stops a pathological log from turning the segment into an event
/// store, exactly like `RECAP_CAP`.
const MARK_CAP: usize = 256;

/// R18: role spans kept per target. Its own list beside `MARK_CAP`, so a
/// tank's Shield Blocks can never evict a trinket proc; inherits R12's
/// newest-dropped rule. The uncapped `uptime` rollup is the gated measure
/// once a long key wraps this.
const SPAN_CAP: usize = 256;

/// R12: an item buff landing this soon after the player cast that same spell
/// is the cast's own aura, not an independent proc.
const USE_AURA_MS: i64 = 2_000;

/// R12: the same proc re-applying inside this window is one proc (trinkets
/// refresh their own buff on every stack).
const PROC_GAP_MS: i64 = 500;

#[derive(Debug, Clone, Default)]
struct ViewStats {
    total: Tally,
    /// Keyed by display label. First-seen wins for id and school: same-name
    /// ranks share art and school.
    by_spell: HashMap<String, SpellSlot>,
    by_target: HashMap<String, Tally>,
}

/// One spell's tallies inside a view: the id behind the label (0 when the
/// label has none — Melee, "Death"), v15 the school bitmask (0 unknown),
/// the total, and v17 per-TARGET tallies so the ability drill can answer
/// "who ate this".
#[derive(Debug, Clone, Default)]
struct SpellSlot {
    id: u32,
    school: u32,
    tally: Tally,
    targets: HashMap<String, Tally>,
}

#[derive(Debug, Clone, Default)]
struct ActorStats {
    views: Vec<ViewStats>,
}

/// R10: one instance visit — a contiguous stay in instanced content (zoning
/// out suspends it, re-entering the same map+difficulty resumes it; every
/// keystone start opens a fresh visit, so a key's clock begins with the key,
/// not at the door). Segments recorded while zoned in carry the visit's
/// ordinal, and the visit's Overall merges them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Visit {
    pub map_id: u32,
    pub difficulty: u32,
    pub name: String,
    pub key_level: Option<u32>,
    /// A CHALLENGE_MODE_START was seen: gates CHALLENGE_MODE_END (the game
    /// fires a zeroed reset END on entry, before any START).
    pub keyed: bool,
    pub start_ms: i64,
    /// `None` while the visit is in progress (including suspended).
    pub end_ms: Option<i64>,
    /// Keystone runs: CHALLENGE_MODE_END's success flag.
    pub completed: Option<bool>,
    /// Keystone runs: CHALLENGE_MODE_END's totalMs — the game's own run
    /// time, death penalties included. `None` until the END fires.
    pub official_ms: Option<i64>,
    /// Keystone runs: the dungeon's (par, +2, +3) timers from the generated
    /// MapChallengeMode table; `None` when the challengeID is unknown.
    pub pars_ms: Option<(i64, i64, i64)>,
}

/// The in-game key timer starts when the activation countdown ends, ~10s
/// after the CHALLENGE_MODE_START line.
pub(crate) const KEY_COUNTDOWN_MS: i64 = 10_000;

impl Visit {
    /// "Skyreach +10" for keys, the zone name otherwise.
    pub fn display_name(&self) -> String {
        match self.key_level {
            Some(l) => format!("{} +{l}", self.name),
            None => self.name.clone(),
        }
    }

    /// R10: a keyed visit's clock is the key timer, not combat time — the
    /// official totalMs once the END fires (exact, penalties included);
    /// until then wall clock from the end of the activation countdown.
    /// `None` for unkeyed visits.
    pub fn key_clock(&self, now_ms: i64) -> Option<i64> {
        if !self.keyed {
            return None;
        }
        Some(self.official_ms.unwrap_or_else(|| {
            (self.end_ms.unwrap_or(now_ms) - self.start_ms - KEY_COUNTDOWN_MS).max(0)
        }))
    }

    /// R10: the outcome shown as a segment's `success`. For a keyed visit
    /// this is the TIMED verdict, not the END's success flag (which only
    /// means "completed" — the game fires it 1 even in overtime): the
    /// official time against par once the END fired, and a live/abandoned
    /// run already past par is depleted — OVER shows the moment the timer
    /// elapses. `None` while genuinely unresolved; the END flag is the
    /// fallback when the dungeon's par is unknown.
    pub fn verdict(&self, now_ms: i64) -> Option<bool> {
        if !self.keyed {
            return self.completed;
        }
        if self.completed == Some(false) {
            return Some(false);
        }
        let Some((par, _, _)) = self.pars_ms else {
            return self.completed;
        };
        match self.official_ms {
            Some(o) => Some(o <= par),
            None => (self.key_clock(now_ms).unwrap_or(0) > par).then_some(false),
        }
    }
}

/// R12: one player's time-resolved damage tallies — per display label, the
/// spell id and a sparse bucket list (see [`Segment::spell_series`]).
type SpellSeries = HashMap<String, (u32, Vec<(u32, Tally)>)>;

/// R16: the unit-flags reaction bit a boss-health report must carry.
/// Friendly `Creature-` guardians (totems, treants) report health on the
/// same lines and must never be mistaken for the boss.
const REACTION_HOSTILE: u32 = 0x40;

/// The creature id inside a `Creature-`/`Vehicle-` guid (its sixth dash
/// field) — what tells one spawn of an add from the next. A guid without
/// it is its own id.
fn npc_id(guid: &str) -> &str {
    guid.split('-').nth(5).unwrap_or(guid)
}

/// R16: one hostile NPC's health as observed inside an open Encounter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct BossHp {
    /// `(current, max)` of the lowest-fraction report.
    low: (u64, u64),
    /// The largest max health it reported — what ranks it against the
    /// other NPCs of the fight.
    peak_max: u64,
}

#[derive(Debug, Clone)]
pub struct Segment {
    pub kind: SegmentKind,
    pub name: String,
    pub start_ms: i64,
    /// `None` while live.
    pub end_ms: Option<i64>,
    pub success: Option<bool>,
    /// R10: ordinal of the instance visit this segment was recorded in.
    pub visit: Option<u32>,
    /// R13: an arena match — `success` means WIN/LOSS, not KILL/WIPE.
    pub arena: bool,
    /// R13: post-match arena tail. Never worth a list row, live or closed
    /// (R11), and the live cursor skips it — the leftover pets deciding
    /// nothing must not steal the meter from the finished match.
    pub noise: bool,
    /// ENCOUNTER_START identity (id, difficulty, group size); `None` on
    /// Trash, Overall and arena segments.
    pub encounter: Option<Encounter>,
    /// The game build from the log's COMBAT_LOG_VERSION line (R6 seed, so
    /// lazy loads carry it); zeros before any version line.
    pub build: (u16, u16, u16),
    /// PROJECT_ID from the same line (1 = retail); 0 before any.
    pub project_id: u8,
    /// The log format version from the same line; 0 before any.
    pub log_version: u32,
    /// R10, Overall segments only: the merged member combat time — an
    /// unkeyed visit's `duration_ms`.
    overall_ms: i64,
    /// R10, Overall segments only: the visit was a keystone run, so its
    /// clock is the key timer (see `Visit::key_clock`), not `overall_ms`.
    key: bool,
    /// R10, keyed Overall segments only: CHALLENGE_MODE_END's totalMs.
    official_ms: Option<i64>,
    /// R16: per hostile NPC (by guid) reporting health while this Encounter
    /// was open: the lowest-fraction report and its largest max health. The
    /// boss is picked at read time (`best_pct`). Empty off raid bosses
    /// (Trash, arena, Overall) and before any report.
    boss_hp: HashMap<String, BossHp>,

    /// Stats keyed by the RAW acting GUID. Ownership is resolved at read time so that
    /// a pet which acted before its SPELL_SUMMON still lands on its owner's row.
    actors: HashMap<String, ActorStats>,
    /// R17: per-player mitigation split, keyed by the RAW destination guid
    /// exactly like `actors` — folded onto owners at read time
    /// (`mitigation()`), so a pet hit or dodged before its SPELL_SUMMON still
    /// lands on its owner once the summon is known.
    mitigation: HashMap<String, Mitigation>,
    /// R19: per-player support ledger (given as the supporter, received on
    /// own hits and heals), keyed by the RAW guid like `mitigation` — a
    /// buffed pet's received folds onto its owner at read time.
    support: HashMap<String, Support>,
    /// R19: per RAW supporter guid, per RAW buffed-source guid, the shares
    /// that supporter contributed to that unit's hits and heals. Both keys
    /// raw, both folded onto owners at read time (`support_targets()`), so
    /// a pet buffed before its SPELL_SUMMON still names its owner once the
    /// summon is known — the rule `support` and `mitigation` follow.
    support_targets: HashMap<String, HashMap<String, SupportTarget>>,
    /// R2 amendment: effective healing landed on a unit, from any source,
    /// keyed by the RAW destination guid and folded like `mitigation`.
    healed: HashMap<String, Healed>,
    /// R2 amendment: the absorber-credited R3 total per RAW absorber guid —
    /// the `absorbed` half of the healing split. Written beside the Healing
    /// record, so it is always a subset of the absorber's Healing row.
    absorbed_credit: HashMap<String, u64>,
    owners: HashMap<String, String>,
    names: HashMap<String, String>,
    flags: HashMap<String, u32>,
    classes: HashMap<String, Class>,
    specs: HashMap<String, Spec>,
    /// COMBATANT_INFO talent + gear loadouts, keyed by player GUID. Same
    /// lifecycle as `classes`/`specs`: authoritative, carried into later
    /// segments via `Segment::new` seeding, latest line wins per field.
    /// `Arc`, not values: the map is cloned into every new segment and every
    /// Overall merge on the seed/lazy-load hot path — sharing makes those
    /// refcount bumps instead of ~2×65 nested-Vec allocations per player
    /// (`Arc` over `Rc` because the loader ships whole `Meter`s across
    /// threads).
    loadouts: HashMap<String, Arc<Loadout>>,
    last_ms: i64,
    /// Damage-event counts against each hostile unit, Details-style: a Trash
    /// segment is named after the enemy it fought most.
    enemies: HashMap<String, u64>,
    /// R11: a player damaged another player here (duels, world PvP;
    /// self-damage excluded) — meaningful combat with no hostile NPC.
    pvp: bool,
    /// R9: per-player ring of recent damage and gains, snapshotted on death.
    recent: HashMap<String, VecDeque<RecapEntry>>,
    /// R9: each player's latest death recap.
    recaps: HashMap<String, Vec<RecapEntry>>,
    /// R9: player GUIDs in first-death order.
    death_order: Vec<String>,
    /// R12: damage on a `BUCKET_MS` grid anchored at `start_ms`, keyed by the
    /// RAW acting guid like `actors` — ownership is resolved at read time, so
    /// a pet that acted before its SPELL_SUMMON still folds into its owner's
    /// curve.
    series: HashMap<String, Vec<u64>>,
    /// v14: effective healing (R2 amounts) on the same grid — what the
    /// Healing drilldown's graph draws. Kept apart from `series` so neither
    /// curve pollutes the other; marks are shared (a trinket is a trinket).
    heal_series: HashMap<String, Vec<u64>>,
    /// R12: the damage tallies time-resolved — per RAW acting guid, per
    /// display label, the spell id and a SPARSE bucket list (only buckets the
    /// spell actually hit in, in feed order). Rides the same `record` call as
    /// `by_spell`, so a window over the whole segment reproduces `breakdown`
    /// exactly; what it buys is `compare_spells` answering an arbitrary
    /// time range without a re-parse.
    spell_series: HashMap<String, SpellSeries>,
    /// R12: item markers per player guid. Stored at ABSOLUTE ms and made
    /// relative in `timeline()`, so an Overall (whose members start at
    /// different times) can merge them without rebasing anything.
    marks: HashMap<String, Vec<AbsMark>>,
    /// R12: when each player last cast each item spell, so the buff that
    /// follows an on-use trinket is not also counted as a proc.
    item_casts: HashMap<(String, u32), i64>,
    /// R18: role spans per RAW target guid (a buff on a pet folds onto its
    /// owner at read time like `mitigation`), absolute ms, capped at
    /// `SPAN_CAP` newest-dropped. Display only — `uptime` and `am` below are
    /// the measures, and they never wrap.
    spans: HashMap<String, Vec<AbsSpan>>,
    /// R18: the span still running per (raw target, spell, raw caster) —
    /// at most one, a re-apply or refresh by the same caster while open
    /// being a no-op; two casters of one spell on one target are two keys,
    /// each closed by its own removal (a shared key would read the second
    /// apply as a refresh and the second removal as an orphan, fabricating
    /// a segment-start span). Independent of the capped list so a removal
    /// after the list wrapped still credits `uptime`. Read-time close for
    /// whatever is still here at the end.
    open_spans: HashMap<SpanKey, OpenSpan>,
    /// R18: the keys whose segment-start rule has fired — the rule opens at
    /// most one span per key per segment, so a second orphaned refresh or
    /// removal of the same key is dropped rather than growing another
    /// `[start, ts]` span.
    retro_fired: HashSet<SpanKey>,
    /// R18: the uncapped rollup — per raw target, per (spell, raw caster):
    /// count and total ms of CLOSED spans. Open ones join at read time
    /// (`rollup`), so lazy = full holds without a mutate-on-close.
    uptime: HashMap<String, HashMap<(u32, String), Uptime>>,
    /// R18: every `ActiveMitigation` interval per raw target, absolute ms,
    /// `(at, end)` with `end == None` while the aura is still on — uncapped,
    /// so the union is exact whatever the capped list dropped. The union
    /// itself is computed at read time (`am_uptime_ms`: sort + sweep), never
    /// incrementally: a retroactive open at `start_ms` lands under groups
    /// that already closed, which an incremental busy counter double-counts.
    am: HashMap<String, Vec<(i64, Option<i64>)>>,
    /// R17/R18: damage taken (`amount + absorbed`, stagger ticks excluded
    /// exactly like the Taken row) on the R12 grid, keyed by the RAW
    /// destination guid and folded at read time (`taken_timeline`).
    taken_series: HashMap<String, Vec<u64>>,
    /// R20: the shield still open per (raw target, spell, raw absorber) —
    /// the span key, because a shield aura's caster IS the absorber the
    /// log's SPELL_ABSORBED names (census: 0 mismatches). At most one per
    /// key; an apply while open closes the old one first. Read-time fold
    /// for whatever is still here at the end (`shields`), never a
    /// mutate-on-close, so lazy = full.
    open_shields: HashMap<SpanKey, OpenShield>,
    /// R20: the CLOSED shields rolled up per raw absorber, per spell id.
    /// Folded onto owners at read time like `absorbed_credit`, so a pet's
    /// shield is its owner's row and an NPC's is nobody's.
    shields: HashMap<String, HashMap<u32, ShieldCell>>,
}

/// R20: a shield that has not seen its removal yet — `remaining` is the
/// running balance the log's REFRESH / REMOVED trailers report, `applied`
/// and `wasted` the ledger's two sides. `applied_known` is false for a
/// shield first seen by its absorb (or applied without a trailer);
/// `remaining_known` turns true on the first trailer; `waste_known` when
/// a refresh-down or a removal fixed the waste.
#[derive(Debug, Clone, PartialEq, Eq)]
struct OpenShield {
    label: String,
    applied: u64,
    applied_known: bool,
    consumed: u64,
    remaining: u64,
    remaining_known: bool,
    wasted: u64,
    waste_known: bool,
    /// A removal trailer BELOW the balance of a known shield: the row is
    /// inconsistent (`applied < consumed + wasted`) and closes as unknown.
    shrunk: bool,
}

/// R20: one (absorber, spell) cell of closed shields — a `ShieldRow` plus
/// whether ANY of them had a known waste, which is what makes
/// `absorb_wasted` `Some`: a cell of only unknown-waste shields is not a
/// 0 waste.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
struct ShieldCell {
    label: String,
    applied: u64,
    consumed: u64,
    wasted: u64,
    count: u32,
    unknown: u32,
    waste_known: bool,
}

impl ShieldCell {
    fn merge(&mut self, other: &ShieldCell) {
        if self.label.is_empty() {
            self.label = other.label.clone();
        }
        self.applied += other.applied;
        self.consumed += other.consumed;
        self.wasted += other.wasted;
        self.count += other.count;
        self.unknown += other.unknown;
        self.waste_known |= other.waste_known;
    }

    fn row(&self, spell_id: u32) -> ShieldRow {
        ShieldRow {
            spell_id,
            label: self.label.clone(),
            applied: self.applied,
            consumed: self.consumed,
            wasted: self.wasted,
            count: self.count,
            unknown: self.unknown,
        }
    }
}

/// R18: a role span before it is rebased onto a segment's start.
#[derive(Debug, Clone, PartialEq, Eq)]
struct AbsSpan {
    at_ms: i64,
    kind: MarkKind,
    label: String,
    spell_id: u32,
    /// The caster's raw guid.
    src: String,
    /// `None` while the aura is still on: the close is computed at read
    /// time against the segment's clock (`close_ms`).
    dur_ms: Option<i64>,
}

/// R18: a span that has not seen its removal yet.
#[derive(Debug, Clone, PartialEq, Eq)]
struct OpenSpan {
    at_ms: i64,
    kind: MarkKind,
    label: String,
    src: String,
}

/// R18: one (target, spell, caster) cell of the uptime rollup.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
struct Uptime {
    count: u32,
    total_ms: i64,
    /// The kind and the log's name, carried here because the capped span
    /// list may no longer hold any span of this cell.
    kind: Option<MarkKind>,
    label: String,
}

/// R18: what identifies one running span — (raw target, spell, raw caster).
type SpanKey = (String, u32, String);

/// R18: the total length of the union of `[at, end)` intervals — sorted by
/// start, then swept, merging whatever overlaps or touches. Empty and
/// inverted intervals contribute nothing.
fn union_ms(intervals: &mut [(i64, i64)]) -> i64 {
    intervals.sort_unstable();
    let mut total = 0;
    let mut cur: Option<(i64, i64)> = None;
    for &(at, end) in intervals.iter() {
        if end <= at {
            continue;
        }
        match cur {
            Some((_, ref mut e)) if at <= *e => *e = (*e).max(end),
            _ => {
                if let Some((s, e)) = cur {
                    total += e - s;
                }
                cur = Some((at, end));
            }
        }
    }
    if let Some((s, e)) = cur {
        total += e - s;
    }
    total
}

/// R18: one row of `Segment::uptime` — a (spell, caster) cell of the
/// player's rollup, open spans included through the read-time close.
/// Step 4b moved the type to the model as `UptimeCell` (the store and the
/// wire carry it); the old name stays as an alias for `tests/spans.rs`.
pub use wowdps_model::UptimeCell as UptimeRow;

/// R18: the role table's kind as the mark kind the wire carries.
fn mark_kind_of(kind: RoleSpellKind) -> MarkKind {
    match kind {
        RoleSpellKind::ActiveMitigation => MarkKind::ActiveMitigation,
        RoleSpellKind::Defensive => MarkKind::Defensive,
        RoleSpellKind::External => MarkKind::External,
        RoleSpellKind::SupportBuff => MarkKind::SupportBuff,
        RoleSpellKind::Cooldown => MarkKind::Cooldown,
    }
}

/// R19: one supporter's shares on one buffed player — the damage and
/// healing halves, and how many support lines carried them.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct SupportTarget {
    damage: u64,
    healing: u64,
    lines: u64,
}

/// R12: a [`Mark`] before it is rebased onto a segment's start.
#[derive(Debug, Clone, PartialEq, Eq)]
struct AbsMark {
    at_ms: i64,
    kind: MarkKind,
    label: String,
    spell_id: u32,
    /// v13: aura applied → removed, filled in when the removal arrives.
    dur_ms: Option<i64>,
}

impl Segment {
    fn new(kind: SegmentKind, name: String, start_ms: i64, seed: &Meter) -> Self {
        Self {
            kind,
            name,
            start_ms,
            end_ms: None,
            success: None,
            visit: seed.zoned_in.then_some(seed.current_visit).flatten(),
            arena: false,
            noise: false,
            encounter: None,
            build: seed.build,
            project_id: seed.project_id,
            log_version: seed.log_version,
            overall_ms: 0,
            key: false,
            official_ms: None,
            boss_hp: HashMap::new(),
            actors: HashMap::new(),
            mitigation: HashMap::new(),
            support: HashMap::new(),
            support_targets: HashMap::new(),
            healed: HashMap::new(),
            absorbed_credit: HashMap::new(),
            // Seed with what the meter already knows so a pet summoned in an earlier
            // segment still resolves here.
            owners: seed.owners.clone(),
            names: seed.names.clone(),
            flags: seed.flags.clone(),
            classes: seed.classes.clone(),
            specs: seed.specs.clone(),
            loadouts: seed.loadouts.clone(),
            last_ms: start_ms,
            enemies: HashMap::new(),
            pvp: false,
            recent: HashMap::new(),
            recaps: HashMap::new(),
            death_order: Vec::new(),
            series: HashMap::new(),
            heal_series: HashMap::new(),
            spell_series: HashMap::new(),
            marks: HashMap::new(),
            item_casts: HashMap::new(),
            spans: HashMap::new(),
            open_spans: HashMap::new(),
            retro_fired: HashSet::new(),
            uptime: HashMap::new(),
            am: HashMap::new(),
            taken_series: HashMap::new(),
            open_shields: HashMap::new(),
            shields: HashMap::new(),
        }
    }

    /// R18: the clock an aura still on at the end is closed against, at
    /// read time — a closed Encounter's end, a Trash segment's last combat
    /// line (R7's end, the same clock as its `duration_ms`), and for a live
    /// segment the newest combat line. An Overall never holds an open span:
    /// `absorb` closes each member's against the member's own clock.
    fn close_ms(&self) -> i64 {
        match self.kind {
            SegmentKind::Trash => self.last_ms,
            SegmentKind::Encounter | SegmentKind::Overall => self.end_ms.unwrap_or(self.last_ms),
        }
    }

    /// R7. Encounters run ENCOUNTER_START..ENCOUNTER_END exactly — the idle head and
    /// tail around the pull are part of the fight. Trash segments instead measure
    /// active combat, first..last combat event: they are closed by whatever happens
    /// next (a pull, a logger restart, a 60s lull), so open..close would charge them
    /// for arbitrary idle time and deflate DPS. `now_ms` is therefore unused for Trash.
    pub fn duration_ms(&self, now_ms: i64) -> i64 {
        let end = match self.kind {
            SegmentKind::Encounter => self.end_ms.unwrap_or(now_ms),
            SegmentKind::Trash => self.last_ms,
            // R10: an Overall's clock is the sum of its members' durations —
            // except a keystone run, whose clock is the key timer: the
            // official totalMs once the END fired, wall clock from the end
            // of the activation countdown until then.
            SegmentKind::Overall => {
                if !self.key {
                    return self.overall_ms;
                }
                return self.official_ms.unwrap_or_else(|| {
                    (self.end_ms.unwrap_or(now_ms) - self.start_ms - KEY_COUNTDOWN_MS).max(0)
                });
            }
        };
        (end - self.start_ms).max(0)
    }

    /// R16: keep, per hostile NPC, the lowest-fraction report and the
    /// largest max health it ever reported.
    fn note_boss_hp(&mut self, guid: &str, current: u64, max: u64) {
        if max == 0 {
            return;
        }
        match self.boss_hp.get_mut(guid) {
            Some(seen) => {
                // Compare fractions without floats: cur/max < c/m ⇔ cur·m < c·max.
                let (c, m) = seen.low;
                if (current as u128) * (m as u128) < (c as u128) * (max as u128) {
                    seen.low = (current, max);
                }
                seen.peak_max = seen.peak_max.max(max);
            }
            None => {
                self.boss_hp.insert(
                    guid.to_string(),
                    BossHp {
                        low: (current, max),
                        peak_max: max,
                    },
                );
            }
        }
    }

    /// R16, for readers and diagnostics: every hostile NPC that reported
    /// health while this Encounter was open — `(guid, lowest (current, max),
    /// largest max seen)` — the raw material `best_pct` grades from.
    pub fn boss_health(&self) -> Vec<(String, (u64, u64), u64)> {
        let mut v: Vec<(String, (u64, u64), u64)> = self
            .boss_hp
            .iter()
            .map(|(g, b)| (g.clone(), b.low, b.peak_max))
            .collect();
        v.sort_by(|a, b| b.2.cmp(&a.2).then(a.0.cmp(&b.0)));
        v
    }

    /// R16: how low the boss got, as a whole percent rounded down (0 on a
    /// kill, 100 at a pull that never scratched it). The boss is the hostile
    /// NPC with the largest max health seen while this Encounter was open;
    /// every NPC with at least half that much is a boss too (councils) —
    /// provided its creature id spawned once (an add pack is never a
    /// council). The answer is the lowest fraction among the bosses STILL
    /// STANDING — a member that is down (under 0.1 %: the game parks a boss
    /// it will not let die yet at 1 HP) is progress made, not the pull's
    /// grade; only when every boss is down is the pull 0. Adds and friendly
    /// guardians dying never count. `None` off raid bosses and when no hostile health
    /// report was seen.
    pub fn best_pct(&self) -> Option<u16> {
        if self.kind != SegmentKind::Encounter || self.arena {
            return None;
        }
        // ENCOUNTER_END says the bosses died: 0 by definition, whatever the
        // last health report said (a scripted death lands no 0/max report).
        if self.success == Some(true) {
            return Some(0);
        }
        let top = self.boss_hp.values().map(|b| b.peak_max).max()?;
        // A council member is one creature: an NPC id that spawned more than
        // once (an add pack, however large each add is) is never a boss. If
        // that leaves nothing — a boss re-spawned under a new guid — fall
        // back to everything big enough.
        let mut instances: HashMap<&str, u32> = HashMap::new();
        for guid in self.boss_hp.keys() {
            *instances.entry(npc_id(guid)).or_insert(0) += 1;
        }
        let big = |b: &BossHp| b.peak_max.saturating_mul(2) >= top;
        let unique = |guid: &String| instances.get(npc_id(guid)).copied() == Some(1);
        let strict: Vec<&BossHp> = self
            .boss_hp
            .iter()
            .filter(|(g, b)| big(b) && unique(g))
            .map(|(_, b)| b)
            .collect();
        let bosses: Vec<&BossHp> = if strict.is_empty() {
            self.boss_hp.values().filter(|b| big(b)).collect()
        } else {
            strict
        };
        // "Down": the game parks a boss that must not die yet at 1 HP, so
        // anything under 0.1 % is down, not a survivor at 0 %.
        let Some((current, max)) = bosses
            .iter()
            .map(|b| b.low)
            .filter(|&(c, m)| (c as u128) * 1000 >= m as u128)
            .min_by(|(c1, m1), (c2, m2)| {
                ((*c1 as u128) * (*m2 as u128)).cmp(&((*c2 as u128) * (*m1 as u128)))
            })
        else {
            // Every boss reached 0: the kill.
            return Some(0);
        };
        Some(((current as u128 * 100) / max.max(1) as u128).min(100) as u16)
    }

    /// R11: whether this segment earns a place in history once closed — the
    /// group damaged an enemy (the same tally that names pulls) or a player
    /// died. The combat log records the whole neighborhood, so world Trash
    /// can consist entirely of NPC-vs-NPC noise or out-of-combat
    /// topping-off heals: those stay on the live meter while open, but are
    /// not worth a list row afterwards. Encounters always count.
    pub fn counts(&self) -> bool {
        // R13: the post-match arena tail never counts, whatever it tallied.
        if self.noise {
            return false;
        }
        self.kind != SegmentKind::Trash
            || !self.enemies.is_empty()
            || self.pvp
            || !self.death_order.is_empty()
    }

    /// R10: merge another segment's counters into this one (Overall
    /// aggregation). Tallies sum; identity maps union with `other` winning;
    /// each player's latest death recap wins; the member's R7 duration is
    /// added to the Overall clock.
    pub fn absorb(&mut self, other: &Segment) {
        for (actor, stats) in &other.actors {
            let dst = self
                .actors
                .entry(actor.clone())
                .or_insert_with(|| ActorStats {
                    views: vec![ViewStats::default(); View::COUNT],
                });
            for (i, vs) in stats.views.iter().enumerate() {
                let Some(d) = dst.views.get_mut(i) else {
                    continue;
                };
                d.total.merge(&vs.total);
                for (k, s) in &vs.by_spell {
                    let slot = d.by_spell.entry(k.clone()).or_default();
                    if slot.id == 0 {
                        slot.id = s.id;
                    }
                    if slot.school == 0 {
                        slot.school = s.school;
                    }
                    slot.tally.merge(&s.tally);
                    for (target, t) in &s.targets {
                        slot.targets.entry(target.clone()).or_default().merge(t);
                    }
                }
                for (k, t) in &vs.by_target {
                    d.by_target.entry(k.clone()).or_default().merge(t);
                }
            }
        }
        // R17: raw-keyed like `actors`, so the Overall folds pets exactly as
        // its members do.
        for (guid, m) in &other.mitigation {
            self.mitigation.entry(guid.clone()).or_default().merge(m);
        }
        // R19 / R2 amendment: the same raw keying, the same fold.
        for (guid, sup) in &other.support {
            self.support.entry(guid.clone()).or_default().merge(sup);
        }
        for (supporter, targets) in &other.support_targets {
            let mine = self.support_targets.entry(supporter.clone()).or_default();
            for (name, t) in targets {
                let slot = mine.entry(name.clone()).or_default();
                slot.damage += t.damage;
                slot.healing += t.healing;
                slot.lines += t.lines;
            }
        }
        for (guid, h) in &other.healed {
            self.healed.entry(guid.clone()).or_default().merge(h);
        }
        for (guid, a) in &other.absorbed_credit {
            *self.absorbed_credit.entry(guid.clone()).or_default() += a;
        }
        for (k, v) in &other.owners {
            self.owners.insert(k.clone(), v.clone());
        }
        for (k, v) in &other.names {
            self.names.insert(k.clone(), v.clone());
        }
        for (k, v) in &other.flags {
            self.flags.insert(k.clone(), *v);
        }
        for (k, v) in &other.classes {
            self.classes.insert(k.clone(), *v);
        }
        for (k, v) in &other.specs {
            self.specs.insert(k.clone(), *v);
        }
        for (k, v) in &other.loadouts {
            self.loadouts.insert(k.clone(), Arc::clone(v));
        }
        for (k, v) in &other.enemies {
            *self.enemies.entry(k.clone()).or_insert(0) += v;
        }
        for g in &other.death_order {
            if !self.death_order.contains(g) {
                self.death_order.push(g.clone());
            }
        }
        for (g, recap) in &other.recaps {
            self.recaps.insert(g.clone(), recap.clone());
        }
        // R12. Members are merged oldest-first from the visit's first member,
        // so `other` never starts before `self` and the shift is >= 0.
        let shift = ((other.start_ms - self.start_ms).max(0) / BUCKET_MS) as usize;
        for (src, dst_map) in [
            (&other.series, &mut self.series),
            (&other.heal_series, &mut self.heal_series),
            (&other.taken_series, &mut self.taken_series),
        ] {
            for (actor, series) in src {
                let dst = dst_map.entry(actor.clone()).or_default();
                let end = shift + series.len();
                if dst.len() < end.min(MAX_BUCKETS) {
                    dst.resize(end.min(MAX_BUCKETS), 0);
                }
                for (i, v) in series.iter().enumerate() {
                    if let Some(slot) = dst.get_mut(shift + i) {
                        *slot += v;
                    }
                }
            }
        }
        for (actor, per_spell) in &other.spell_series {
            let dst = self.spell_series.entry(actor.clone()).or_default();
            for (spell, (id, slices)) in per_spell {
                let (did, dslices) = dst.entry(spell.clone()).or_default();
                if *did == 0 {
                    *did = *id;
                }
                // Appended, buckets rebased by the same shift; slices may
                // repeat a bucket across members — the range query sums, so
                // order and duplication cost nothing.
                for (b, t) in slices {
                    let nb = (*b as usize + shift).min(MAX_BUCKETS - 1) as u32;
                    dslices.push((nb, t.clone()));
                }
            }
        }
        for (player, marks) in &other.marks {
            let dst = self.marks.entry(player.clone()).or_default();
            for m in marks {
                if dst.len() >= MARK_CAP {
                    break;
                }
                if !dst.contains(m) {
                    dst.push(m.clone());
                }
            }
        }
        // R18: a member's spans join absolute like marks, under their own
        // cap; whatever the member still had open is closed here against
        // the MEMBER's clock (the read-time close it would answer itself),
        // so an Overall never carries an open span and its rollup, union
        // and list all agree with Σ members. The dedupe is keyed on
        // (at, spell, caster) under the target the map already keys — a
        // span's identity, not its bytes — so the same span absorbed twice
        // is one span whatever its close read as. Members are disjoint in
        // time and each opens its retro spans at its OWN `start_ms`, so
        // two members never legitimately hold the same identity.
        let member_close = other.close_ms();
        for (target, spans) in &other.spans {
            let dst = self.spans.entry(target.clone()).or_default();
            for s in spans {
                if dst.len() >= SPAN_CAP {
                    break;
                }
                let mut s = s.clone();
                if s.dur_ms.is_none() {
                    s.dur_ms = Some((member_close - s.at_ms).max(0));
                }
                let same =
                    |d: &AbsSpan| d.at_ms == s.at_ms && d.spell_id == s.spell_id && d.src == s.src;
                if !dst.iter().any(same) {
                    dst.push(s);
                }
            }
        }
        for (target, cells) in other.rollup() {
            let mine = self.uptime.entry(target).or_default();
            for (key, cell) in cells {
                let slot = mine.entry(key).or_default();
                slot.count += cell.count;
                slot.total_ms += cell.total_ms;
                if slot.kind.is_none() {
                    slot.kind = cell.kind;
                    slot.label = cell.label;
                }
            }
        }
        // R18: the member's AM intervals join absolute (they are durations
        // on the wall clock, like spans — no bucket shift), each open one
        // closed and every end clamped against the MEMBER's clock, so the
        // Overall's union over disjoint members is Σ member unions and an
        // Overall never holds an open interval.
        for (target, intervals) in &other.am {
            let dst = self.am.entry(target.clone()).or_default();
            dst.extend(
                intervals
                    .iter()
                    .map(|&(at, end)| (at, Some(end.unwrap_or(member_close).min(member_close)))),
            );
        }
        // R20: the member's closed cells sum per raw absorber per spell,
        // and whatever it still had open folds the way its OWN read would
        // (`shield_cells`: consumed + count + unknown, no applied, no
        // wasted) — so an Overall never holds an open shield and its rows,
        // `absorb_wasted` and `shields_unknown` are exactly Σ members'.
        for (absorber, cells) in other.shield_cells() {
            let mine = self.shields.entry(absorber).or_default();
            for (spell, cell) in cells {
                mine.entry(spell).or_default().merge(&cell);
            }
        }
        self.last_ms = self.last_ms.max(other.last_ms);
        self.overall_ms += other.duration_ms(other.last_ms);
    }

    /// R18: the uptime rollup as of now — the closed cells plus every span
    /// still open, closed against `close_ms`. Per raw target, per (spell,
    /// raw caster). Pure and deterministic: the same lines give the same
    /// answer whether replayed lazily or live, which is why the open ones
    /// are folded here and never written back.
    fn rollup(&self) -> HashMap<String, HashMap<(u32, String), Uptime>> {
        let mut out = self.uptime.clone();
        let now = self.close_ms();
        for ((target, spell, _), o) in &self.open_spans {
            let slot = out
                .entry(target.clone())
                .or_default()
                .entry((*spell, o.src.clone()))
                .or_default();
            slot.count += 1;
            slot.total_ms += (now - o.at_ms).max(0);
            if slot.kind.is_none() {
                slot.kind = Some(o.kind);
                slot.label = o.label.clone();
            }
        }
        out
    }

    /// Timestamp of the last combat event recorded here — the deterministic
    /// "now" the Overall merge uses for an open member's duration (R10).
    pub fn last_combat_ms(&self) -> i64 {
        self.last_ms
    }

    /// R20: whether `guid` is a unit the group controls as far as this
    /// segment knows NOW — a player or pet by guid, or any unit whose owner
    /// a SPELL_SUMMON or advanced block already named (a Monk's Celestial,
    /// a Warlock's demon, a `Creature-` guardian). The shield AURA gate:
    /// an aura is admitted or dropped at feed time, so an uncontrolled
    /// caster's shield never opens a key (a boss's own bubble is not a
    /// row waiting for its owner). The ABSORB path is not gated — it
    /// mirrors `absorbed_credit`, raw-keyed and folded at read, so the
    /// identity Σ consumed = `absorbed_healing` holds for any guid asked.
    fn controlled(&self, guid: &str) -> bool {
        is_friendly_source(guid) || self.owners.contains_key(guid)
    }

    /// Walk the ownership chain to the controlling unit. Bounded against cycles.
    fn resolve_owner<'a>(&'a self, guid: &'a str) -> &'a str {
        let mut cur = guid;
        for _ in 0..8 {
            match self.owners.get(cur) {
                Some(next) if next != cur => cur = next.as_str(),
                _ => break,
            }
        }
        cur
    }

    /// Nil GUIDs are rejected before the flag test: real logs carry PLAYER flags on
    /// nil-source lines, which would otherwise become a phantom meter row.
    fn is_player(&self, guid: &str) -> bool {
        if guid.is_empty() || guid == "0000000000000000" {
            return false;
        }
        self.flags.get(guid).is_some_and(|f| f & 0x0000_0400 != 0) || guid.starts_with("Player-")
    }

    fn label_for(&self, guid: &str) -> String {
        self.names
            .get(guid)
            .cloned()
            .unwrap_or_else(|| guid.to_string())
    }

    fn stats(&self, actor: &str, view: View) -> Option<&ViewStats> {
        self.actors.get(actor)?.views.get(view.index())
    }

    fn finish_rows(&self, mut rows: Vec<Row>, view: View) -> Vec<Row> {
        let total: u64 = rows.iter().map(|r| r.amount).sum();
        let secs = self.duration_ms(self.last_ms) as f64 / 1000.0;
        for r in &mut rows {
            r.pct = if total > 0 {
                r.amount as f64 / total as f64 * 100.0
            } else {
                0.0
            };
            r.per_sec = if view.is_rate() && secs > 0.0 {
                r.amount as f64 / secs
            } else {
                0.0
            };
        }
        // R13: the friendly team leads, the enemy team follows — a renderer
        // splits the chart at the first `enemy` row. Within a team descending
        // by amount; ties broken by label so ordering is deterministic.
        // Breakdown rows are never `enemy`, so their order is untouched.
        rows.sort_by(|a, b| {
            a.enemy
                .cmp(&b.enemy)
                .then_with(|| b.amount.cmp(&a.amount))
                .then_with(|| a.label.cmp(&b.label))
        });
        rows
    }

    /// Rows for a view, sorted desc by amount. `pct` is of the view total.
    pub fn rows(&self, view: View) -> Vec<Row> {
        let mut merged: HashMap<&str, Tally> = HashMap::new();
        for actor in self.actors.keys() {
            let owner = self.resolve_owner(actor);
            if !self.is_player(owner) {
                continue;
            }
            let Some(st) = self.stats(actor, view) else {
                continue;
            };
            // R17: Taken lists on `count > 0` — a player who was only dodged
            // has a row with nothing but misses on it.
            let empty = if view == View::Taken {
                st.total.count == 0
            } else {
                st.total.amount == 0 && st.total.extra == 0
            };
            if empty {
                continue;
            }
            merged.entry(owner).or_default().merge(&st.total);
        }

        let rows = merged
            .into_iter()
            .map(|(guid, t)| Row {
                key: guid.to_string(),
                label: self.label_for(guid),
                amount: t.amount,
                extra: t.extra,
                count: t.count,
                crits: t.crits,
                per_sec: 0.0,
                pct: 0.0,
                class: self.classes.get(guid).copied(),
                spec: self.specs.get(guid).copied(),
                hp: None,
                gain: false,
                spell_id: 0,
                // R13: reaction bit 0x40 — the hostile side of an arena.
                // Gated on `arena`: in the open world hostile-flagged players
                // (war mode, duels) must not split the chart into teams.
                // Segment-local flags, so lazy loads agree.
                enemy: self.arena && self.flags.get(guid).is_some_and(|f| f & 0x40 != 0),
                school: 0,
            })
            .collect();
        let mut rows = self.finish_rows(rows, view);
        // R9: deaths list in death order, not by count — the meter question
        // there is "who went down first", not "who died most".
        if view == View::Deaths {
            let order = |r: &Row| {
                self.death_order
                    .iter()
                    .position(|g| g == &r.key)
                    .unwrap_or(usize::MAX)
            };
            rows.sort_by_key(order);
        }
        rows
    }

    /// Drilldown for one player: (by-spell rows, by-target rows). For Deaths
    /// the panes are the death recap instead (R9): the ordered event timeline
    /// and the attacker totals behind it.
    pub fn breakdown(&self, player_guid: &str, view: View) -> (Vec<Row>, Vec<Row>) {
        if view == View::Deaths {
            return self.death_breakdown(player_guid);
        }
        let mut spells: HashMap<String, (String, u32, u32, Tally)> = HashMap::new();
        let mut targets: HashMap<String, Tally> = HashMap::new();

        for actor in self.actors.keys() {
            if self.resolve_owner(actor) != player_guid {
                continue;
            }
            let Some(st) = self.stats(actor, view) else {
                continue;
            };

            // R5: a pet's spells stay visible as "{spell} ({petName})" here, while the
            // meter row above remains merged under the owner. Keyed by pet NAME, not
            // guid: swarm specs (Army of the Dead, Wild Imps) summon dozens of
            // same-named instances per fight, and a row per instance buries the
            // drill under thirty identical "Shadow Bolt (Magus of the Dead)" lines.
            let pet_name = (actor != player_guid).then(|| self.label_for(actor));
            for (spell, s) in &st.by_spell {
                let (key, label) = match &pet_name {
                    Some(pet) => (format!("{spell}\u{0}{pet}"), format!("{spell} ({pet})")),
                    None => (spell.clone(), spell.clone()),
                };
                let e = spells
                    .entry(key)
                    .or_insert_with(|| (label, 0, 0, Tally::default()));
                if e.1 == 0 {
                    e.1 = s.id;
                }
                if e.2 == 0 {
                    e.2 = s.school;
                }
                e.3.merge(&s.tally);
            }
            for (target, t) in &st.by_target {
                targets.entry(target.clone()).or_default().merge(t);
            }
        }

        let class = self.classes.get(player_guid).copied();
        let spec = self.specs.get(player_guid).copied();
        let to_rows = |m: Vec<(String, String, u32, u32, Tally)>| -> Vec<Row> {
            m.into_iter()
                .map(|(key, label, spell_id, school, t)| Row {
                    key,
                    label,
                    amount: t.amount,
                    extra: t.extra,
                    count: t.count,
                    crits: t.crits,
                    per_sec: 0.0,
                    pct: 0.0,
                    class,
                    spec,
                    hp: None,
                    gain: false,
                    spell_id,
                    enemy: false,
                    school,
                })
                .collect()
        };

        let spell_rows = to_rows(
            spells
                .into_iter()
                .map(|(k, (l, id, school, t))| (k, l, id, school, t))
                .collect(),
        );
        let target_rows = to_rows(
            targets
                .into_iter()
                .map(|(k, t)| (k.clone(), k, 0, 0, t))
                .collect(),
        );
        (
            self.finish_rows(spell_rows, view),
            self.finish_rows(target_rows, view),
        )
    }

    /// R17: one player's mitigation split over this segment. Pets fold onto
    /// their owner at read time exactly like `rows` (a pet hit before its
    /// SPELL_SUMMON still lands here). `None` when nothing was ever swung at
    /// them or their pets.
    pub fn mitigation(&self, player_guid: &str) -> Option<Mitigation> {
        let mut out: Option<Mitigation> = None;
        for (guid, m) in &self.mitigation {
            if self.resolve_owner(guid) != player_guid {
                continue;
            }
            out.get_or_insert_with(Mitigation::default).merge(m);
        }
        out
    }

    /// R17: the mitigation record for a RAW destination guid, created on
    /// first touch. Write-side only; readers fold through `mitigation()`.
    fn mitigation_mut(&mut self, dst_guid: &str) -> &mut Mitigation {
        self.mitigation.entry(dst_guid.to_string()).or_default()
    }

    /// R19: one player's support ledger over this segment — given as the
    /// supporter, received on their own (and their pets') hits and heals.
    /// Folds onto owners like `mitigation`; `None` when no support line
    /// named them or their pets on either side — but `Some` of all zeros
    /// when the only line was a fully-overhealed heal share (its amount,
    /// `amount − overheal`, is 0; the line still names both parties).
    /// Answers for a supporter with no meter row at all (a guid the log
    /// only ever trails with).
    pub fn support(&self, player_guid: &str) -> Option<Support> {
        let mut out: Option<Support> = None;
        for (guid, sup) in &self.support {
            if self.resolve_owner(guid) != player_guid {
                continue;
            }
            out.get_or_insert_with(Support::default).merge(sup);
        }
        out
    }

    /// R19 (step 3b): every player `support()` answers for — the
    /// supporters AND the buffed sources, raw guids folded onto owners and
    /// non-players dropped — as Damage-shaped rows: `key` = the player's
    /// guid, `label` = their name (the guid itself for a supporter the log
    /// only ever trails with — the store's roster gap: such a player has no
    /// meter row anywhere, yet Σ effective over a card must equal Σ damage),
    /// `amount` = given damage shares, `extra` = given healing shares,
    /// `count` = the support lines they trailed. Sorted like
    /// `rows(Damage)`; empty on a fight without support.
    pub fn supporters(&self) -> Vec<Row> {
        let mut merged: HashMap<&str, (Support, u64)> = HashMap::new();
        for (guid, sup) in &self.support {
            let owner = self.resolve_owner(guid);
            if !self.is_player(owner) {
                continue;
            }
            merged.entry(owner).or_default().0.merge(sup);
        }
        for (supporter, targets) in &self.support_targets {
            let owner = self.resolve_owner(supporter);
            if let Some(slot) = merged.get_mut(owner) {
                slot.1 += targets.values().map(|t| t.lines).sum::<u64>();
            }
        }
        let rows = merged
            .into_iter()
            .map(|(owner, (sup, lines))| Row {
                class: self.classes.get(owner).copied(),
                spec: self.specs.get(owner).copied(),
                key: owner.to_string(),
                label: self.label_for(owner),
                amount: sup.given_damage,
                extra: sup.given_healing,
                count: lines,
                crits: 0,
                per_sec: 0.0,
                pct: 0.0,
                hp: None,
                gain: false,
                spell_id: 0,
                enemy: false,
                school: 0,
            })
            .collect();
        self.finish_rows(rows, View::Damage)
    }

    /// R19: whom a supporter's shares landed on — one row per buffed
    /// player: `key` = that player's guid (the buffed unit's raw guid
    /// walked to its owner, so a pet's shares are its owner's row and a
    /// pet buffed before its summon still lands there), `label` = the
    /// owner's name, `amount` = the damage shares, `extra` = the healing
    /// shares, `count` = support lines. `per_sec` is the damage share
    /// over the segment's duration and `pct` its share of the supporter's
    /// given damage, so the rows read like a Damage drill; sorted by
    /// amount desc, ties by label. Empty when the guid (or its pets)
    /// never supported anyone.
    pub fn support_targets(&self, player_guid: &str) -> Vec<Row> {
        let mut merged: HashMap<&str, SupportTarget> = HashMap::new();
        for (supporter, targets) in &self.support_targets {
            if self.resolve_owner(supporter) != player_guid {
                continue;
            }
            for (src, t) in targets {
                let slot = merged.entry(self.resolve_owner(src)).or_default();
                slot.damage += t.damage;
                slot.healing += t.healing;
                slot.lines += t.lines;
            }
        }
        let rows = merged
            .into_iter()
            .map(|(owner, t)| Row {
                class: self.classes.get(owner).copied(),
                spec: self.specs.get(owner).copied(),
                key: owner.to_string(),
                label: self.label_for(owner),
                amount: t.damage,
                extra: t.healing,
                count: t.lines,
                crits: 0,
                per_sec: 0.0,
                pct: 0.0,
                hp: None,
                gain: false,
                spell_id: 0,
                enemy: false,
                school: 0,
            })
            .collect();
        self.finish_rows(rows, View::Damage)
    }

    /// R2 amendment: effective healing that landed on the player (and
    /// their pets) from any source, with the self-cast subset. Folds like
    /// `mitigation`; `None` when nothing ever healed them.
    pub fn healed(&self, player_guid: &str) -> Option<Healed> {
        let mut out: Option<Healed> = None;
        for (guid, h) in &self.healed {
            if self.resolve_owner(guid) != player_guid {
                continue;
            }
            out.get_or_insert_with(Healed::default).merge(h);
        }
        out
    }

    /// R2 amendment: the absorb half of the player's Healing row — every
    /// SPELL_ABSORBED credited to them (or their pets) as the absorber,
    /// `NON_HEALING_ABSORBS` excluded exactly as the row excludes them.
    /// Never more than the row's amount.
    pub fn absorbed_healing(&self, player_guid: &str) -> u64 {
        self.absorbed_credit
            .iter()
            .filter(|(guid, _)| self.resolve_owner(guid) == player_guid)
            .map(|(_, a)| a)
            .sum()
    }

    /// R19: the one damage number for everyone — the player's R1 damage
    /// (pets folded, exactly the Damage row's amount) minus the shares a
    /// supporter accounts for, plus the shares they gave
    /// (`wowdps_model::effective`). Derived here, never stored.
    pub fn effective(&self, player_guid: &str) -> u64 {
        let damage: u64 = self
            .actors
            .iter()
            .filter(|(actor, _)| self.resolve_owner(actor) == player_guid)
            .filter_map(|(_, st)| st.views.get(View::Damage.index()))
            .map(|v| v.total.amount)
            .sum();
        let sup = self.support(player_guid).unwrap_or_default();
        wowdps_model::effective(damage, sup.received_damage, sup.given_damage)
    }

    /// R9: a fresh health report for a unit. Back-fills the newest recap entry
    /// still missing HP — SWING_DAMAGE describes its source, and
    /// SPELL_ABSORBED has no advanced block, so their entries get HP from the
    /// next line describing the victim (its LANDED twin / the paired damage
    /// line), gated to ~the same instant so a stale report can't lie.
    fn note_hp(&mut self, h: &HpHint, ts: i64) {
        if let Some(ring) = self.recent.get_mut(&h.unit_guid)
            && let Some(last) = ring.back_mut()
            && last.hp.is_none()
            && ts - last.ts <= 1_000
        {
            last.hp = Some((h.current, h.max));
        }
    }

    /// R9: append to a player's recap ring, evicting the oldest at capacity.
    fn recap_push(&mut self, guid: &str, entry: RecapEntry) {
        let ring = self.recent.entry(guid.to_string()).or_default();
        if ring.len() == RECAP_CAP {
            ring.pop_front();
        }
        ring.push_back(entry);
    }

    /// R9: the Deaths drilldown. The by-spell pane is the recap — newest
    /// first, so the killing blow leads — and the by-target pane totals the
    /// attackers behind it.
    fn death_breakdown(&self, guid: &str) -> (Vec<Row>, Vec<Row>) {
        let Some(recap) = self.recaps.get(guid) else {
            return (Vec::new(), Vec::new());
        };
        let class = self.classes.get(guid).copied();
        let spec = self.specs.get(guid).copied();
        let total: u64 = recap.iter().filter(|e| !e.gain).map(|e| e.amount).sum();
        // Source-less damage (boss auras, environment) logs a nil unit: the
        // spell alone is the whole story then, for the attacker pane too.
        let events = recap
            .iter()
            .rev()
            .enumerate()
            .map(|(i, e)| Row {
                key: format!("{i}"),
                label: if e.src.is_empty() {
                    e.spell.clone()
                } else {
                    format!("{} ({})", e.spell, e.src)
                },
                amount: e.amount,
                extra: e.extra,
                count: 1,
                crits: e.crit as u64,
                per_sec: 0.0,
                pct: if e.gain || total == 0 {
                    0.0
                } else {
                    e.amount as f64 / total as f64 * 100.0
                },
                class,
                spec,
                hp: e.hp,
                gain: e.gain,
                spell_id: 0,
                enemy: false,
                school: 0,
            })
            .collect();

        let mut attackers: HashMap<String, Tally> = HashMap::new();
        for e in recap.iter().filter(|e| !e.gain) {
            let attacker = if e.src.is_empty() { &e.spell } else { &e.src };
            attackers
                .entry(attacker.clone())
                .or_default()
                .add(e.amount, e.extra, e.crit);
        }
        let attacker_rows = attackers
            .into_iter()
            .map(|(name, t)| Row {
                key: name.clone(),
                label: name,
                amount: t.amount,
                extra: t.extra,
                count: t.count,
                crits: t.crits,
                per_sec: 0.0,
                pct: 0.0,
                class,
                spec,
                hp: None,
                gain: false,
                spell_id: 0,
                enemy: false,
                school: 0,
            })
            .collect();
        (events, self.finish_rows(attacker_rows, View::Deaths))
    }

    /// R12: what a comparison graph draws for one player — their damage on a
    /// fixed grid plus the item markers laid over it. Pets fold into their
    /// owner, exactly as they do in `rows`/`breakdown`.
    ///
    /// Times are relative to this segment's `start_ms`. On an Overall (R10)
    /// that is the visit's first member, and the curve therefore spans the
    /// visit's wall clock — including the gaps between pulls, which is what
    /// makes a per-visit graph readable at all.
    pub fn timeline(&self, player_guid: &str) -> Timeline {
        self.timeline_of(&self.series, player_guid)
    }

    /// The player's COMBATANT_INFO loadout as known to THIS segment — talents
    /// and gear from the latest line at or before it (seeded across segments
    /// like `classes`/`specs`). `None` for players whose info never fired.
    pub fn loadout(&self, player_guid: &str) -> Option<&Loadout> {
        self.loadouts.get(player_guid).map(Arc::as_ref)
    }

    /// v14: the healing counterpart — effective healing (R2) on the same
    /// grid, same pet folding, same item markers. What the Healing
    /// drilldown's graph draws.
    pub fn heal_timeline(&self, player_guid: &str) -> Timeline {
        self.timeline_of(&self.heal_series, player_guid)
    }

    /// v16: one ability's damage on the same R12 grid — the drilled spell's
    /// own curve. `spell_key` is the by-spell row's `key` ("spell" or
    /// "spell\0petName"), so client and meter agree on identity by
    /// construction; the pet arm sums same-named pet instances exactly like
    /// the breakdown row does. Marks are the player's, same as `timeline`.
    pub fn spell_timeline(&self, player_guid: &str, spell_key: &str) -> Timeline {
        let (want_spell, want_pet) = match spell_key.split_once('\u{0}') {
            Some((s, p)) => (s, Some(p)),
            None => (spell_key, None),
        };
        let mut buckets: Vec<u64> = Vec::new();
        for (actor, per_spell) in &self.spell_series {
            if self.resolve_owner(actor) != player_guid {
                continue;
            }
            let pet = (actor.as_str() != player_guid).then(|| self.label_for(actor));
            if pet.as_deref() != want_pet {
                continue;
            }
            let Some((_, slices)) = per_spell.get(want_spell) else {
                continue;
            };
            for (b, t) in slices {
                let i = *b as usize;
                if i >= MAX_BUCKETS {
                    continue;
                }
                if buckets.len() <= i {
                    buckets.resize(i + 1, 0);
                }
                if let Some(slot) = buckets.get_mut(i) {
                    *slot += t.amount;
                }
            }
        }
        Timeline {
            bucket_ms: BUCKET_MS as u32,
            buckets,
            marks: self.marks_for(player_guid),
        }
    }

    /// v17: who a drilled ability landed on — per-target rows for one spell
    /// of one player, keyed like [`Self::spell_timeline`]. Sorted desc;
    /// `pct` is of the SPELL's own total, and every row wears the spell's
    /// school so its bars tint like the ability's.
    pub fn spell_targets(&self, player_guid: &str, spell_key: &str, view: View) -> Vec<Row> {
        let (want_spell, want_pet) = match spell_key.split_once('\u{0}') {
            Some((s, p)) => (s, Some(p)),
            None => (spell_key, None),
        };
        let mut acc: HashMap<String, Tally> = HashMap::new();
        let mut school = 0u32;
        for actor in self.actors.keys() {
            if self.resolve_owner(actor) != player_guid {
                continue;
            }
            let pet = (actor.as_str() != player_guid).then(|| self.label_for(actor));
            if pet.as_deref() != want_pet {
                continue;
            }
            let Some(slot) = self
                .stats(actor, view)
                .and_then(|st| st.by_spell.get(want_spell))
            else {
                continue;
            };
            if school == 0 {
                school = slot.school;
            }
            for (target, t) in &slot.targets {
                acc.entry(target.clone()).or_default().merge(t);
            }
        }
        let total: u64 = acc.values().map(|t| t.amount).sum();
        let class = self.classes.get(player_guid).copied();
        let spec = self.specs.get(player_guid).copied();
        let mut rows: Vec<Row> = acc
            .into_iter()
            .map(|(target, t)| Row {
                key: target.clone(),
                label: target,
                amount: t.amount,
                extra: t.extra,
                count: t.count,
                crits: t.crits,
                per_sec: 0.0,
                pct: if total > 0 {
                    t.amount as f64 / total as f64 * 100.0
                } else {
                    0.0
                },
                class,
                spec,
                hp: None,
                gain: false,
                spell_id: 0,
                enemy: false,
                school,
            })
            .collect();
        rows.sort_by(|a, b| b.amount.cmp(&a.amount).then_with(|| a.label.cmp(&b.label)));
        rows
    }

    fn timeline_of(&self, map: &HashMap<String, Vec<u64>>, player_guid: &str) -> Timeline {
        let mut buckets: Vec<u64> = Vec::new();
        for (actor, series) in map {
            if self.resolve_owner(actor) != player_guid {
                continue;
            }
            if buckets.len() < series.len() {
                buckets.resize(series.len(), 0);
            }
            for (slot, v) in buckets.iter_mut().zip(series) {
                *slot += v;
            }
        }
        Timeline {
            bucket_ms: BUCKET_MS as u32,
            buckets,
            marks: self.marks_for(player_guid),
        }
    }

    /// The player's item markers (R12) and role spans (R18) merged, rebased
    /// onto the segment's start and sorted by time — shared by every
    /// timeline flavor. The close is computed HERE, kind-branched: an item
    /// mark still open reads 0 (a proc that never dropped is not a span; no
    /// R12 golden moves), a role span still open reads `close_ms − at`.
    fn marks_for(&self, player_guid: &str) -> Vec<Mark> {
        let mut marks: Vec<Mark> = self
            .marks
            .get(player_guid)
            .into_iter()
            .flatten()
            .map(|m| Mark {
                at_ms: m.at_ms - self.start_ms,
                kind: m.kind,
                label: m.label.clone(),
                spell_id: m.spell_id,
                dur_ms: m.dur_ms.unwrap_or(0),
                src: String::new(),
            })
            .collect();
        marks.extend(self.spans(player_guid));
        // Stable: items before spans on a tie, so two replays agree.
        marks.sort_by_key(|m| m.at_ms);
        marks
    }

    /// R18: the player's role spans only — every `ActiveMitigation` /
    /// `Defensive` / `External` / `SupportBuff` / `Cooldown` buff that
    /// landed on them (or their pets, folded like `mitigation`), rebased
    /// onto the segment's start, each with its caster, an open one closed
    /// at read time against the segment's clock. Bounded by `SPAN_CAP` per
    /// target; the measures below are not.
    pub fn spans(&self, player_guid: &str) -> Vec<Mark> {
        let close = self.close_ms();
        let mut out: Vec<Mark> = self
            .spans
            .iter()
            .filter(|(target, _)| self.resolve_owner(target) == player_guid)
            .flat_map(|(_, list)| list)
            .map(|s| Mark {
                at_ms: s.at_ms - self.start_ms,
                kind: s.kind,
                label: s.label.clone(),
                spell_id: s.spell_id,
                dur_ms: s.dur_ms.unwrap_or_else(|| (close - s.at_ms).max(0)),
                src: s.src.clone(),
            })
            .collect();
        out.sort_by(|a, b| (a.at_ms, a.spell_id, &a.src).cmp(&(b.at_ms, b.spell_id, &b.src)));
        out
    }

    /// R18: the uncapped rollup for one player as target — per (spell,
    /// caster): how many spans and their total ms, spans still open
    /// included through the read-time close. Pets fold onto the owner.
    /// Sorted by (kind, spell, caster) so two replays compare equal.
    pub fn uptime(&self, player_guid: &str) -> Vec<UptimeRow> {
        let mut cells: HashMap<(u32, String), Uptime> = HashMap::new();
        for (target, per) in self.rollup() {
            if self.resolve_owner(&target) != player_guid {
                continue;
            }
            for (key, cell) in per {
                let slot = cells.entry(key).or_default();
                slot.count += cell.count;
                slot.total_ms += cell.total_ms;
                if slot.kind.is_none() {
                    slot.kind = cell.kind;
                    slot.label = cell.label;
                }
            }
        }
        let mut rows: Vec<UptimeRow> = cells
            .into_iter()
            .filter_map(|((spell_id, src), cell)| {
                Some(UptimeRow {
                    spell_id,
                    label: cell.label,
                    kind: cell.kind?,
                    src,
                    count: cell.count,
                    total_ms: cell.total_ms,
                })
            })
            .collect();
        rows.sort_by(|a, b| {
            (a.kind.code(), a.spell_id, &a.src).cmp(&(b.kind.code(), b.spell_id, &b.src))
        });
        rows
    }

    /// R18: the per-millisecond UNION of `ActiveMitigation` spans on the
    /// player (pets folded) — overlapping buffs count once, so this never
    /// exceeds the segment's `duration_ms` and `am_uptime_pct` = this /
    /// `duration_ms`. Computed here from the uncapped interval list (exact
    /// whatever the capped span list dropped): an interval still open ends
    /// at `close_ms`, and EVERY end is clamped to `close_ms` — on Trash a
    /// removal in the 60 s idle tail closes its span past the R7 clock (the
    /// span and the rollup keep that truth), but the headline may never
    /// read over the segment on any kind. An Overall's intervals were
    /// already closed and clamped per member by `absorb`, and its own
    /// clock is not a member's, so it takes them as they are.
    pub fn am_uptime_ms(&self, player_guid: &str) -> i64 {
        let close = self.close_ms();
        let clamp = self.kind != SegmentKind::Overall;
        let mut intervals: Vec<(i64, i64)> = self
            .am
            .iter()
            .filter(|(target, _)| self.resolve_owner(target) == player_guid)
            .flat_map(|(_, list)| list.iter())
            .map(|&(at, end)| {
                let end = end.unwrap_or(close);
                (at, if clamp { end.min(close) } else { end })
            })
            .collect();
        union_ms(&mut intervals)
    }

    /// R18: `External` spans the player CAST (their pets folded), across
    /// every target — (count, total ms). A self-cast external is both given
    /// and received, which keeps Σ given = Σ received per segment exact.
    pub fn externals_given(&self, player_guid: &str) -> (u32, i64) {
        let mut out = (0, 0);
        for per in self.rollup().into_values() {
            for ((_, src), cell) in per {
                if cell.kind == Some(MarkKind::External) && self.resolve_owner(&src) == player_guid
                {
                    out.0 += cell.count;
                    out.1 += cell.total_ms;
                }
            }
        }
        out
    }

    /// R18: `External` spans that landed ON the player or their pets —
    /// (count, total ms), from any caster.
    pub fn externals_received(&self, player_guid: &str) -> (u32, i64) {
        let mut out = (0, 0);
        for (target, per) in self.rollup() {
            if self.resolve_owner(&target) != player_guid {
                continue;
            }
            for cell in per.into_values() {
                if cell.kind == Some(MarkKind::External) {
                    out.0 += cell.count;
                    out.1 += cell.total_ms;
                }
            }
        }
        out
    }

    /// R18: the `SupportBuff` spans the player gave, per (target owner
    /// guid, spell) — total ms, from the rollup. Σ over the rows is the
    /// supporter's total. Sorted by (target, spell).
    pub fn support_uptime(&self, player_guid: &str) -> Vec<(String, u32, i64)> {
        let mut per: HashMap<(String, u32), i64> = HashMap::new();
        for (target, cells) in self.rollup() {
            let owner = self.resolve_owner(&target).to_string();
            for ((spell, src), cell) in cells {
                if cell.kind == Some(MarkKind::SupportBuff)
                    && self.resolve_owner(&src) == player_guid
                {
                    *per.entry((owner.clone(), spell)).or_default() += cell.total_ms;
                }
            }
        }
        let mut rows: Vec<(String, u32, i64)> =
            per.into_iter().map(|((t, s), ms)| (t, s, ms)).collect();
        rows.sort();
        rows
    }

    /// R17/R18: damage taken on the R12 grid — `amount + absorbed` per
    /// bucket on the destination, stagger ticks excluded like the Taken
    /// row, pets folded — with the player's marks and spans.
    pub fn taken_timeline(&self, player_guid: &str) -> Timeline {
        self.timeline_of(&self.taken_series, player_guid)
    }

    /// R20: the ledger as of now — the closed cells plus every shield
    /// still open, folded with its `consumed` and `count` only (`unknown`
    /// += 1; applied, wasted and its waste flag dropped: the size and the
    /// waste of a shield that never closed are not observable). Per raw
    /// absorber, per spell. Pure and deterministic, like `rollup`: the
    /// same lines give the same answer lazily or live, which is why the
    /// open ones are folded here and never written back.
    fn shield_cells(&self) -> HashMap<String, HashMap<u32, ShieldCell>> {
        let mut out = self.shields.clone();
        for ((_, spell, absorber), o) in &self.open_shields {
            let cell = out
                .entry(absorber.clone())
                .or_default()
                .entry(*spell)
                .or_default();
            if cell.label.is_empty() {
                cell.label = o.label.clone();
            }
            cell.consumed += o.consumed;
            cell.count += 1;
            cell.unknown += 1;
        }
        out
    }

    /// R20: the player's shield ledger as the ABSORBER (pets folded) —
    /// one row per shield spell, open shields folded with `consumed` and
    /// `count` only; sorted by consumed desc, then spell id. Σ `consumed`
    /// over the rows = `absorbed_healing` exactly: every absorb that
    /// credits healing enters exactly one ledger key.
    pub fn shields(&self, player_guid: &str) -> Vec<ShieldRow> {
        let mut per: HashMap<u32, ShieldCell> = HashMap::new();
        for (absorber, cells) in self.shield_cells() {
            if self.resolve_owner(&absorber) != player_guid {
                continue;
            }
            for (spell, cell) in cells {
                per.entry(spell).or_default().merge(&cell);
            }
        }
        let mut rows: Vec<ShieldRow> = per.iter().map(|(id, c)| c.row(*id)).collect();
        rows.sort_by(|a, b| {
            b.consumed
                .cmp(&a.consumed)
                .then(a.spell_id.cmp(&b.spell_id))
        });
        rows
    }

    /// R20: Σ `wasted` over the player's CLOSED shields with a KNOWN waste
    /// (a removal trailer, a removal on a known-applied shield, or a
    /// refresh-down); `None` when none had one — a non-shielder, or only
    /// open / unknown ones — never a 0 that would claim perfect efficiency.
    pub fn absorb_wasted(&self, player_guid: &str) -> Option<u64> {
        let mut out: Option<u64> = None;
        for (absorber, cells) in &self.shields {
            if self.resolve_owner(absorber) != player_guid {
                continue;
            }
            for cell in cells.values() {
                if cell.waste_known {
                    *out.get_or_insert(0) += cell.wasted;
                }
            }
        }
        out
    }

    /// R20: Σ `unknown` over the player's rows — the shields whose APPLIED
    /// size was never seen (first seen by an absorb — the pre-pull shield —,
    /// applied without a trailer, or still open at the segment's end) or
    /// that shrank. A convenience over `shields()`; a caller wanting both
    /// reads the rows once (the store's extract does).
    pub fn shields_unknown(&self, player_guid: &str) -> u32 {
        self.shields(player_guid).iter().map(|r| r.unknown).sum()
    }

    /// R20: a shield aura landed on `target` from `absorber`. An open
    /// shield of the same key closes first with `wasted = remaining` when
    /// that is known (a double APPLIED without a REMOVED); the new one
    /// opens with `applied = remaining = a` when the trailer is there,
    /// unknown-applied otherwise.
    fn shield_apply(&mut self, target: &str, spell: &Spell, absorber: &str, absorb: Option<u64>) {
        let key: SpanKey = (target.to_string(), spell.id, absorber.to_string());
        if let Some(mut old) = self.open_shields.remove(&key) {
            if old.remaining_known {
                old.wasted += old.remaining;
                old.waste_known = true;
            }
            self.close_shield(absorber, spell.id, old);
        }
        let a = absorb.unwrap_or(0);
        self.open_shields.insert(
            key,
            OpenShield {
                label: spell.name.clone(),
                applied: a,
                applied_known: absorb.is_some(),
                consumed: 0,
                remaining: a,
                remaining_known: absorb.is_some(),
                wasted: 0,
                waste_known: false,
                shrunk: false,
            },
        );
    }

    /// R20: a refresh's trailer is the shield's NEW RUNNING TOTAL, never a
    /// delta: above the balance it is more shield applied, below it the
    /// difference was overwritten — waste. With no open key, or no
    /// trailer, nothing (an orphan refresh is not evidence of a shield;
    /// the absorb that follows will open one).
    fn shield_refresh(&mut self, target: &str, spell: &Spell, absorber: &str, absorb: Option<u64>) {
        let key: SpanKey = (target.to_string(), spell.id, absorber.to_string());
        let (Some(r), Some(o)) = (absorb, self.open_shields.get_mut(&key)) else {
            return;
        };
        if o.remaining_known {
            if r > o.remaining {
                o.applied += r - o.remaining;
            } else if r < o.remaining {
                o.wasted += o.remaining - r;
                o.waste_known = true;
            }
        }
        o.remaining = r;
        o.remaining_known = true;
    }

    /// R20: `amount` was soaked by the shield of (target, spell, absorber).
    /// On an open key `consumed += amount`; an over-absorb (more than the
    /// balance) raises `applied` by the excess when the size was known —
    /// Frost Shield, Soul Leech and Reversion under-report their size, and
    /// `applied = consumed + wasted` must hold by construction — and the
    /// balance is 0. With no open key it opens an unknown-applied shield
    /// with `consumed = amount`: the pre-pull shield, or a spell outside
    /// the table — an un-generated build never loses healing.
    fn shield_absorb(&mut self, target: &str, spell: &Spell, absorber: &str, amount: u64) {
        let key: SpanKey = (target.to_string(), spell.id, absorber.to_string());
        let Some(o) = self.open_shields.get_mut(&key) else {
            self.open_shields.insert(
                key,
                OpenShield {
                    label: spell.name.clone(),
                    applied: 0,
                    applied_known: false,
                    consumed: amount,
                    remaining: 0,
                    remaining_known: false,
                    wasted: 0,
                    waste_known: false,
                    shrunk: false,
                },
            );
            return;
        };
        o.consumed += amount;
        if o.remaining_known {
            if amount > o.remaining {
                if o.applied_known {
                    o.applied += amount - o.remaining;
                }
                o.remaining = 0;
            } else {
                o.remaining -= amount;
            }
        }
    }

    /// R20: the shield came off. Its trailer is what REMAINED and is
    /// authoritative for the waste even when the size was never seen;
    /// without one the balance is the waste when known, else the waste
    /// stays unknown. With no open key: a no-op — a removal is not
    /// evidence of a shield.
    ///
    /// A trailer that disagrees with the running balance of a KNOWN
    /// shield: ABOVE it is the over-absorb rule again — the shield grew
    /// with no REFRESH line (Soul Leech, Yu'lon's Grace, Frost Shield and
    /// other stacking shields; a real log removes a Soul Leech applied 843
    /// with 3 171 remaining) and `applied` rises by the difference, so
    /// `applied = consumed + wasted` holds by construction. BELOW it the
    /// shield shrank unobserved (First In, Last Out): `wasted` is the
    /// trailer, `applied` is left where the log put it, and the shield
    /// closes as `unknown` — the row is visibly inconsistent (`applied <
    /// consumed + wasted`), never quietly perfect. Raise-only keeps the
    /// symmetry with the absorb rule: no transition ever lowers `applied`.
    /// On the fixture every trailer equals the balance (`check.awk`'s B3
    /// self-check), so this changes no golden.
    fn shield_remove(&mut self, target: &str, spell: &Spell, absorber: &str, absorb: Option<u64>) {
        let key: SpanKey = (target.to_string(), spell.id, absorber.to_string());
        let Some(mut o) = self.open_shields.remove(&key) else {
            return;
        };
        if let Some(w) = absorb {
            o.wasted += w;
            o.waste_known = true;
            if o.applied_known && o.remaining_known {
                if w > o.remaining {
                    o.applied += w - o.remaining;
                } else if w < o.remaining {
                    o.shrunk = true;
                }
            }
        } else if o.remaining_known {
            o.wasted += o.remaining;
            o.waste_known = true;
        }
        self.close_shield(absorber, spell.id, o);
    }

    /// R20: a closed shield joins its (absorber, spell) cell: `applied`
    /// only when known, `wasted` only when known, `unknown` when the size
    /// never was — or when the shield shrank (its `applied` still counts;
    /// the flag is what marks the row inconsistent).
    fn close_shield(&mut self, absorber: &str, spell_id: u32, o: OpenShield) {
        let cell = self
            .shields
            .entry(absorber.to_string())
            .or_default()
            .entry(spell_id)
            .or_default();
        if cell.label.is_empty() {
            cell.label = o.label;
        }
        cell.count += 1;
        cell.consumed += o.consumed;
        if o.applied_known {
            cell.applied += o.applied;
        }
        if !o.applied_known || o.shrunk {
            cell.unknown += 1;
        }
        if o.waste_known {
            cell.wasted += o.wasted;
            cell.waste_known = true;
        }
    }

    /// R18: a role buff landed on (or refreshed on) `target`. A span already
    /// open for (target, spell, caster) makes this a no-op — a re-apply by
    /// the same caster while on is a refresh; another caster's apply of the
    /// same spell is its own span. `retro` (a refresh, or a removal, with
    /// no open span) opens at the SEGMENT'S START: the buff predated the
    /// segment and this line is the only evidence of it, so its caster is
    /// the line's — and it fires at most ONCE per key per segment; a later
    /// orphan of the same key is dropped, since a second `[start, ts]` span
    /// could only be a fabrication. Bypasses every item dedupe rule
    /// (`USE_AURA_MS`, `PROC_GAP_MS` are trinket semantics); the capped
    /// list may drop it, the measures never do.
    fn note_span(
        &mut self,
        target: &str,
        spell: &Spell,
        kind: MarkKind,
        src: &str,
        ts: i64,
        retro: bool,
    ) {
        let key: SpanKey = (target.to_string(), spell.id, src.to_string());
        if self.open_spans.contains_key(&key) {
            return;
        }
        if retro && !self.retro_fired.insert(key.clone()) {
            return;
        }
        let at = if retro { self.start_ms } else { ts };
        self.open_spans.insert(
            key,
            OpenSpan {
                at_ms: at,
                kind,
                label: spell.name.clone(),
                src: src.to_string(),
            },
        );
        if kind == MarkKind::ActiveMitigation {
            self.am
                .entry(target.to_string())
                .or_default()
                .push((at, None));
        }
        let list = self.spans.entry(target.to_string()).or_default();
        if list.len() >= SPAN_CAP {
            return;
        }
        list.push(AbsSpan {
            at_ms: at,
            kind,
            label: spell.name.clone(),
            spell_id: spell.id,
            src: src.to_string(),
            dur_ms: None,
        });
    }

    /// R18: the role buff came off `target`: close the open span of that
    /// (spell, caster), crediting the rollup and the AM interval. With none
    /// open the segment-start rule applies — opened at `start_ms` with this
    /// line's caster, closed at once (once per key; a repeat is dropped).
    /// The rollup cell is credited under the OPENING line's caster: the
    /// removal's `src` only selects which open span closes (the same guid
    /// by construction of the key) and never re-labels the cell.
    fn close_span(&mut self, target: &str, spell: &Spell, kind: MarkKind, src: &str, ts: i64) {
        let key: SpanKey = (target.to_string(), spell.id, src.to_string());
        if !self.open_spans.contains_key(&key) {
            self.note_span(target, spell, kind, src, ts, true);
        }
        let Some(open) = self.open_spans.remove(&key) else {
            return;
        };
        let dur = (ts - open.at_ms).max(0);
        let cell = self
            .uptime
            .entry(target.to_string())
            .or_default()
            .entry((spell.id, open.src.clone()))
            .or_default();
        cell.count += 1;
        cell.total_ms += dur;
        if cell.kind.is_none() {
            cell.kind = Some(open.kind);
            cell.label = open.label.clone();
        }
        // The AM interval this span opened is the newest still-open one
        // that began at its `at_ms`; two open intervals with the same start
        // are interchangeable for a union, so which of them closes is moot.
        if open.kind == MarkKind::ActiveMitigation
            && let Some(list) = self.am.get_mut(target)
            && let Some(iv) = list
                .iter_mut()
                .rev()
                .find(|(at, end)| *at == open.at_ms && end.is_none())
        {
            iv.1 = Some(ts.max(open.at_ms));
        }
        if let Some(list) = self.spans.get_mut(target)
            && let Some(s) = list
                .iter_mut()
                .rev()
                .find(|s| s.spell_id == spell.id && s.src == open.src && s.dur_ms.is_none())
        {
            s.dur_ms = Some(dur);
        }
    }

    /// R17/R18: add taken damage to the victim's curve at `ts`.
    fn bucket_taken(&mut self, victim: &str, ts: i64, amount: u64) {
        Self::bucket_into(self.start_ms, &mut self.taken_series, victim, ts, amount);
    }

    /// R12: add `amount` to an actor's damage curve at `ts`.
    fn bucket(&mut self, actor: &str, ts: i64, amount: u64) {
        Self::bucket_into(self.start_ms, &mut self.series, actor, ts, amount);
    }

    /// v14: the same, onto the actor's effective-healing curve.
    fn bucket_heal(&mut self, actor: &str, ts: i64, amount: u64) {
        Self::bucket_into(self.start_ms, &mut self.heal_series, actor, ts, amount);
    }

    fn bucket_into(
        start_ms: i64,
        map: &mut HashMap<String, Vec<u64>>,
        actor: &str,
        ts: i64,
        amount: u64,
    ) {
        let i = (ts - start_ms).max(0) / BUCKET_MS;
        // A clock that jumps forward costs a clamp, never an allocation.
        let Ok(i) = usize::try_from(i) else { return };
        if i >= MAX_BUCKETS {
            return;
        }
        let series = map.entry(actor.to_string()).or_default();
        if series.len() <= i {
            series.resize(i + 1, 0);
        }
        if let Some(slot) = series.get_mut(i) {
            *slot += amount;
        }
    }

    /// R12: the same event `record` just tallied, appended to the actor's
    /// sparse per-spell series so `compare_spells` can answer a time window
    /// without a re-parse. Merges into the last slice when the event lands in
    /// the same bucket (feed order is time order in practice); the query sums
    /// by range test, so even an out-of-order clock only costs a spare slice.
    #[allow(clippy::too_many_arguments)]
    fn spell_bucket(
        &mut self,
        actor: &str,
        spell: &str,
        spell_id: u32,
        ts: i64,
        amount: u64,
        extra: u64,
        crit: bool,
    ) {
        let i = (ts - self.start_ms).max(0) / BUCKET_MS;
        let Ok(i) = usize::try_from(i) else { return };
        if i >= MAX_BUCKETS {
            return;
        }
        let i = i as u32;
        let per_spell = self.spell_series.entry(actor.to_string()).or_default();
        let (id, slices) = per_spell.entry(spell.to_string()).or_default();
        if *id == 0 {
            *id = spell_id;
        }
        match slices.last_mut() {
            Some((b, t)) if *b == i => t.add(amount, extra, crit),
            _ => {
                let mut t = Tally::default();
                t.add(amount, extra, crit);
                slices.push((i, t));
            }
        }
    }

    /// R12: the per-spell comparison table over a time window — `range` in ms
    /// relative to `start_ms` (half-open, `lo..hi`), `None` for the whole
    /// segment, in which case it agrees with `breakdown` exactly. The Row
    /// returned alongside is the player's windowed total, so the compare
    /// header can wear the window's own damage and DPS. Pets fold into their
    /// owner under the same "{spell} ({pet})" labels `breakdown` writes.
    pub fn compare_spells(&self, player_guid: &str, range: Option<(i64, i64)>) -> (Row, Vec<Row>) {
        let in_range = |bucket: u32| match range {
            None => true,
            Some((lo, hi)) => {
                let b = bucket as i64 * BUCKET_MS;
                b + BUCKET_MS > lo && b < hi
            }
        };
        let mut spells: HashMap<String, (String, u32, Tally)> = HashMap::new();
        let mut total = Tally::default();
        for (actor, per_spell) in &self.spell_series {
            if self.resolve_owner(actor) != player_guid {
                continue;
            }
            // R5: pets keep their "{spell} ({pet})" label, keyed by NAME so
            // swarm summons share one row — same fold as `breakdown`.
            let pet_name = (actor != player_guid).then(|| self.label_for(actor));
            for (spell, (id, slices)) in per_spell {
                let mut t = Tally::default();
                for (b, s) in slices {
                    if in_range(*b) {
                        t.merge(s);
                    }
                }
                if t.count == 0 {
                    continue;
                }
                total.merge(&t);
                let (key, label) = match &pet_name {
                    Some(pet) => (format!("{spell}\u{0}{pet}"), format!("{spell} ({pet})")),
                    None => (spell.clone(), spell.clone()),
                };
                let e = spells
                    .entry(key)
                    .or_insert_with(|| (label, 0, Tally::default()));
                if e.1 == 0 {
                    e.1 = *id;
                }
                e.2.merge(&t);
            }
        }

        let class = self.classes.get(player_guid).copied();
        let spec = self.specs.get(player_guid).copied();
        let row = |key: String, label: String, spell_id: u32, t: &Tally| Row {
            key,
            label,
            amount: t.amount,
            extra: t.extra,
            count: t.count,
            crits: t.crits,
            per_sec: 0.0,
            pct: 0.0,
            class,
            spec,
            hp: None,
            gain: false,
            spell_id,
            enemy: false,
            // The sparse per-spell series carries no school; the compare
            // table draws no bars, so nothing reads this.
            school: 0,
        };

        let mut rows: Vec<Row> = spells
            .into_iter()
            .map(|(k, (l, id, t))| row(k, l, id, &t))
            .collect();
        let view_total: u64 = rows.iter().map(|r| r.amount).sum();
        let secs = match range {
            Some((lo, hi)) => (hi - lo).max(0) as f64 / 1000.0,
            None => self.duration_ms(self.last_ms) as f64 / 1000.0,
        };
        for r in &mut rows {
            r.pct = if view_total > 0 {
                r.amount as f64 / view_total as f64 * 100.0
            } else {
                0.0
            };
        }
        rows.sort_by(|a, b| b.amount.cmp(&a.amount).then_with(|| a.label.cmp(&b.label)));

        let mut total_row = row(
            player_guid.to_string(),
            self.label_for(player_guid),
            0,
            &total,
        );
        total_row.per_sec = if secs > 0.0 {
            total.amount as f64 / secs
        } else {
            0.0
        };
        (total_row, rows)
    }

    /// R12: record an item marker for a player, if the spell came from an
    /// item at all. `cast` distinguishes "the player pressed it" from "it
    /// landed on them"; the return says whether a mark was actually added.
    ///
    /// `class_spells` wins: the generated item table follows trinket trigger
    /// chains and so also claims some ordinary class spells (a trinket that
    /// procs a free Fireball lists Fireball), which must never surface as a
    /// trinket marker.
    fn note_mark(&mut self, player: &str, spell: &Spell, ts: i64, cast: bool) -> bool {
        // R18: externals (the Bloodlust family, Power Infusion — a priest
        // spell the class-spells veto below would eat) now come from the
        // role table and open a SPAN at the call site, before this runs.
        if crate::class_spells::resolve(spell.id).is_some() {
            return false;
        }
        let Some(item) = crate::item_spells::item_kind(spell.id) else {
            return false;
        };
        let kind = match (item, cast) {
            (ItemKind::Trinket, true) => MarkKind::TrinketUse,
            (ItemKind::Trinket, false) => MarkKind::TrinketProc,
            // Consumables only count when the player actually used one; a
            // flask's buff re-applying on a reload is not a consumable event.
            (_, true) => MarkKind::Consumable,
            (_, false) => return false,
        };
        if cast {
            self.item_casts.insert((player.to_string(), spell.id), ts);
        } else {
            // The buff an on-use trinket applies to itself is that use, not
            // a second, independent proc.
            if let Some(&cast_ms) = self.item_casts.get(&(player.to_string(), spell.id))
                && ts - cast_ms <= USE_AURA_MS
            {
                return false;
            }
        }
        let list = self.marks.entry(player.to_string()).or_default();
        // Trinkets refresh their own buff as they stack; one proc, one bar.
        if let Some(last) = list.iter().rev().find(|m| m.label == spell.name)
            && ts - last.at_ms <= PROC_GAP_MS
        {
            return false;
        }
        // v13: a re-application while the aura is still ON (no removal seen
        // yet) is a refresh, not a new event — the open span keeps running.
        if !cast
            && list
                .iter()
                .rev()
                .any(|m| m.spell_id == spell.id && m.dur_ms.is_none())
        {
            return false;
        }
        if list.len() >= MARK_CAP {
            return false;
        }
        list.push(AbsMark {
            at_ms: ts,
            kind,
            label: spell.name.clone(),
            spell_id: spell.id,
            dur_ms: None,
        });
        true
    }

    /// v13: the aura behind a marker came off — close the player's open span
    /// for that spell, turning the mark into a duration.
    fn close_mark(&mut self, player: &str, spell_id: u32, ts: i64) {
        if let Some(list) = self.marks.get_mut(player)
            && let Some(m) = list
                .iter_mut()
                .rev()
                .find(|m| m.spell_id == spell_id && m.dur_ms.is_none())
            && ts >= m.at_ms
        {
            m.dur_ms = Some(ts - m.at_ms);
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn record(
        &mut self,
        actor: &str,
        view: View,
        spell: &str,
        spell_id: u32,
        school: u32,
        target: &str,
        amount: u64,
        extra: u64,
        crit: bool,
    ) {
        let stats = self
            .actors
            .entry(actor.to_string())
            .or_insert_with(|| ActorStats {
                views: vec![ViewStats::default(); View::COUNT],
            });
        let Some(v) = stats.views.get_mut(view.index()) else {
            return;
        };
        v.total.add(amount, extra, crit);
        let slot = v.by_spell.entry(spell.to_string()).or_default();
        if slot.id == 0 {
            slot.id = spell_id;
        }
        if slot.school == 0 {
            slot.school = school;
        }
        slot.tally.add(amount, extra, crit);
        if !target.is_empty() {
            // v17: the same event, keyed spell×target, so the ability drill
            // can answer "who ate this" without any re-parse.
            slot.targets
                .entry(target.to_string())
                .or_default()
                .add(amount, extra, crit);
            v.by_target
                .entry(target.to_string())
                .or_default()
                .add(amount, extra, crit);
        }
    }
}

#[derive(Debug, Default)]
pub struct Meter {
    segments: Vec<Segment>,
    owners: HashMap<String, String>,
    names: HashMap<String, String>,
    flags: HashMap<String, u32>,
    classes: HashMap<String, Class>,
    specs: HashMap<String, Spec>,
    loadouts: HashMap<String, Arc<Loadout>>,
    last_combat_ms: Option<i64>,
    /// The latest COMBAT_LOG_VERSION line's build / project / format
    /// version, seeded into every segment opened after it.
    build: (u16, u16, u16),
    project_id: u8,
    log_version: u32,
    /// R10: every instance visit seen, in file order (ordinals index here).
    visits: Vec<Visit>,
    /// The visit currently in progress (open or suspended).
    current_visit: Option<u32>,
    /// Physically inside the current visit's instance right now — false
    /// while suspended, so outside combat doesn't join the visit.
    zoned_in: bool,
    /// R13: name of the last zone entered, ANY difficulty — arenas zone in
    /// with difficulty 0, so the visit table never learns their names.
    last_zone: Option<String>,
    /// R13: an arena match's segment is open, so ARENA_MATCH_END has
    /// something to verdict. False outside one — a stray END closes nothing.
    in_arena: bool,
    /// R13: the home side (0/1), resolved inside the match: the faction of
    /// the first friendly-flagged player to land a damage event. `None`
    /// until resolved; an END before resolution closes with no verdict.
    arena_home: Option<u32>,
    /// R13: guid → COMBATANT_INFO faction, collected only while a match is
    /// open (the game re-fires the infos right after ARENA_MATCH_START).
    /// Match-local, so a lazy load of the slice reproduces the verdict.
    arena_factions: HashMap<String, u32>,
    /// R13: the match ended but we are still standing in the arena — the
    /// pet/DoT tail before the teleport out is NOISE: it records into a
    /// segment that never surfaces. Cleared by any ZONE_CHANGE, the next
    /// ARENA_MATCH_START, or a version seam. ARENA_MATCH_END lines are seed
    /// lines so lazy loads of the tail reproduce this.
    arena_over: bool,
}

impl Meter {
    pub fn new() -> Self {
        Self::default()
    }

    /// Remember a unit's display name and flags, globally and in the open segment.
    /// Called before segment creation so a freshly opened segment inherits it.
    fn learn(&mut self, u: &Unit) {
        if u.guid.is_empty() || u.guid == "0000000000000000" {
            return;
        }
        // Every event relearns units it has already seen, so insert only on a
        // real change: the lookup still hashes, but the clone-and-allocate
        // pair behind it is skipped for the whole run after the first sighting.
        if !u.name.is_empty() && self.names.get(&u.guid).is_none_or(|n| *n != u.name) {
            self.names.insert(u.guid.clone(), u.name.clone());
        }
        if u.flags != 0 && self.flags.get(&u.guid) != Some(&u.flags) {
            self.flags.insert(u.guid.clone(), u.flags);
        }
        if let Some(s) = self.segments.last_mut() {
            if !u.name.is_empty() && s.names.get(&u.guid).is_none_or(|n| *n != u.name) {
                s.names.insert(u.guid.clone(), u.name.clone());
            }
            if u.flags != 0 && s.flags.get(&u.guid) != Some(&u.flags) {
                s.flags.insert(u.guid.clone(), u.flags);
            }
        }
    }

    /// R8: outside instanced content COMBATANT_INFO never fires, so class/spec
    /// is inferred from class-identifying casts. Evidence lands in the OPEN
    /// segment only — a closed segment's byte range excludes lines after its
    /// end, so writing there would break lazy/full parity — and never in the
    /// meter-level maps that seed future segments, which the lazy path
    /// reconstructs from COMBATANT_INFO seed lines alone. Must never open a
    /// segment or touch `last_combat_ms`: the index scanner mirrors
    /// segmentation and knows nothing about inference. On recording paths call
    /// this AFTER `record`, so a gap-split has already moved the open segment
    /// to the one this line belongs to.
    fn infer(&mut self, unit: &Unit, spell: &Spell) {
        if !unit.is_player() {
            return;
        }
        let Some(s) = self.segments.last_mut() else {
            return;
        };
        if s.end_ms.is_some() {
            return;
        }
        let has_spec = s.specs.contains_key(&unit.guid);
        if has_spec && s.classes.contains_key(&unit.guid) {
            return;
        }
        let Some((class, spec)) = crate::class_spells::resolve(spell.id) else {
            return;
        };
        let class = *s.classes.entry(unit.guid.clone()).or_insert(class);
        // A spec-unique cast may still refine a player whose class is already
        // known, but never against that class.
        if !has_spec
            && let Some(spec) = spec
            && spec.class() == class
        {
            s.specs.insert(unit.guid.clone(), spec);
        }
    }

    fn note_owner(&mut self, unit: &str, owner: &str) {
        if unit.is_empty() || owner.is_empty() || unit == owner {
            return;
        }
        // Same as `learn`: a pet's owner is restated on every one of its
        // events, and only the first statement is news.
        if self.owners.get(unit).is_none_or(|o| o != owner) {
            self.owners.insert(unit.to_string(), owner.to_string());
        }
        if let Some(s) = self.segments.last_mut()
            && s.owners.get(unit).is_none_or(|o| o != owner)
        {
            s.owners.insert(unit.to_string(), owner.to_string());
        }
    }

    /// R10: a zone change is a hard location break — the open Trash segment
    /// closes (encounters only close by ENCOUNTER_END).
    fn close_trash(&mut self, ts: i64) {
        if let Some(s) = self.segments.last()
            && s.end_ms.is_none()
            && s.kind == SegmentKind::Trash
        {
            self.close(ts, None);
        }
    }

    /// R10: the current visit (if any) ends here.
    fn close_visit(&mut self, ts: i64) {
        if let Some(i) = self.current_visit.take()
            && let Some(v) = self.visits.get_mut(i as usize)
            && v.end_ms.is_none()
        {
            v.end_ms = Some(ts);
        }
        self.zoned_in = false;
    }

    fn close(&mut self, ts: i64, success: Option<bool>) {
        if let Some(s) = self.segments.last_mut()
            && s.end_ms.is_none()
        {
            s.end_ms = Some(ts);
            // Deliberately does NOT advance last_ms: that field is "timestamp of the
            // last combat event", and R7 measures Trash duration from it. Bumping it
            // to the close time would charge the segment for the idle gap.
            if success.is_some() {
                s.success = success;
            }
        }
    }

    /// Ensure there is a live segment to record into, opening a Trash segment when
    /// combat happens outside an encounter or after a >60s lull.
    fn ensure_combat(&mut self, ts: i64) {
        let need_new = match self.segments.last() {
            None => true,
            Some(s) if s.end_ms.is_some() => true,
            Some(s) => {
                s.kind == SegmentKind::Trash
                    && self
                        .last_combat_ms
                        .is_some_and(|last| ts - last > TRASH_GAP_MS)
            }
        };
        if need_new {
            let close_at = self.last_combat_ms.unwrap_or(ts);
            self.close(close_at, None);
            let mut seg = Segment::new(SegmentKind::Trash, "Trash".to_string(), ts, self);
            // R13: combat inside a decided arena is the leftover tail.
            seg.noise = self.arena_over;
            self.segments.push(seg);
        }
        self.last_combat_ms = Some(ts);
        if let Some(s) = self.segments.last_mut() {
            s.last_ms = s.last_ms.max(ts);
        }
    }

    /// R17: the segment a NON-combat line at `ts` may write into — a
    /// `*_MISSED` line or a `NON_HEALING_ABSORBS` `SPELL_ABSORBED`. Neither
    /// is combat to the scanner, so neither may open, extend or split a
    /// segment; but "the open segment" is not enough either: a Trash
    /// segment stays open until the NEXT recordable line applies the
    /// `TRASH_GAP_MS` split, so this mirrors `ensure_combat`'s predicate
    /// WITHOUT acting on it — a line that would have split the segment is
    /// dropped, never credited to the stale pull. Lazy/full parity holds
    /// because the scanner ends the stale slice at the splitting line
    /// (`Index::ensure_combat` closes at the new line's offset), so a lazy
    /// replay of that slice sees the same lines with the same
    /// `last_combat_ms` and skips them the same way.
    fn open_segment_for_passive(&mut self, ts: i64) -> Option<&mut Segment> {
        let last = self.last_combat_ms;
        self.segments
            .last_mut()
            .filter(|s| s.end_ms.is_none())
            .filter(|s| {
                s.kind != SegmentKind::Trash || !last.is_some_and(|l| ts - l > TRASH_GAP_MS)
            })
    }

    /// Give a live Trash segment its Details-style name: the enemy hit most,
    /// plus `+N` for the other distinct enemies in the pull. Counts damage
    /// *events* from players/pets into creatures — cheap enough to run per
    /// line, and the index scanner can mirror it without parsing amounts.
    fn name_trash(&mut self, src_guid: &str, dst_guid: &str, dst_name: &str) {
        if !is_friendly_source(src_guid) || !is_hostile_target(dst_guid) {
            return;
        }
        let Some(s) = self.segments.last_mut() else {
            return;
        };
        if s.kind != SegmentKind::Trash {
            return;
        }
        *s.enemies.entry(dst_name.to_string()).or_insert(0) += 1;
        if let Some(name) = trash_name(&s.enemies) {
            s.name = name;
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn record(
        &mut self,
        ts: i64,
        actor: &str,
        view: View,
        spell: &str,
        spell_id: u32,
        school: u32,
        target: &str,
        amount: u64,
        extra: u64,
        crit: bool,
    ) {
        self.ensure_combat(ts);
        if let Some(s) = self.segments.last_mut() {
            s.record(
                actor, view, spell, spell_id, school, target, amount, extra, crit,
            );
            // R12: the damage curve rides the same lookup the tallies already
            // did, so the timeline costs one vector index per damage event.
            if view == View::Damage {
                s.bucket(actor, ts, amount);
                s.spell_bucket(actor, spell, spell_id, ts, amount, extra, crit);
            } else if view == View::Healing {
                // v14: effective healing gets its own curve — the Healing
                // drilldown's graph. No spell series: only the comparison
                // (damage-only) windows its tables by time.
                s.bucket_heal(actor, ts, amount);
            }
        }
    }

    pub fn feed(&mut self, line: LogLine) {
        let ts = line.ts_ms;
        if let Some(h) = &line.owner_hint {
            let (unit, owner) = (h.unit_guid.clone(), h.owner_guid.clone());
            self.note_owner(&unit, &owner);
        }
        // R9: health reports only ever back-fill an existing recap entry —
        // they must not open a segment or count as combat.
        if let Some(h) = &line.hp_hint
            && let Some(s) = self.segments.last_mut()
        {
            s.note_hp(h, ts);
            // R16: a hostile NPC's own health report inside an open boss
            // fight is the boss-health observation progression is graded on.
            if s.kind == SegmentKind::Encounter
                && !s.arena
                && s.end_ms.is_none()
                && is_hostile_target(&h.unit_guid)
                && h.flags & REACTION_HOSTILE != 0
            {
                s.note_boss_hp(&h.unit_guid, h.current, h.max);
            }
        }

        match &line.event {
            // R6: the logger restarted; accumulated state across the seam is
            // wrong. The visit is only SUSPENDED, not closed: a mid-run
            // /reload writes a version line with the key still in progress,
            // and the ZONE_CHANGE the game re-fires right after resumes it —
            // a seam somewhere else closes it at the next ZONE_CHANGE.
            Event::Version {
                log_version,
                build,
                project_id,
                ..
            } => {
                self.close(ts, None);
                self.build = *build;
                self.project_id = *project_id;
                self.log_version = *log_version;
                self.zoned_in = false;
                self.owners.clear();
                self.last_combat_ms = None;
                // R13: the seam closed the match's segment; its END (if the
                // logger even survives to see it) must not verdict whatever
                // opens next. Keeping this state only while a segment is
                // open also keeps it out of the scanner's checkpoints.
                self.in_arena = false;
                self.arena_home = None;
                self.arena_factions.clear();
                self.arena_over = false;
            }
            Event::EncounterStart {
                id,
                name,
                difficulty,
                group_size,
            } => {
                self.close(ts, None);
                let mut seg = Segment::new(SegmentKind::Encounter, name.clone(), ts, self);
                seg.encounter = Some(Encounter {
                    id: *id,
                    difficulty: *difficulty,
                    group_size: *group_size,
                });
                self.segments.push(seg);
                self.last_combat_ms = Some(ts);
            }
            // R4: close exactly here, no DoT-tail grace window.
            Event::EncounterEnd { success, .. } => self.close(ts, Some(*success)),

            // R13: an arena match is an encounter in every way that matters —
            // hard boundaries, a name, an outcome. Encounter kind means the
            // trash-gap rule can't split a slow dampening game and R7 clocks
            // the match START..END.
            Event::ArenaMatchStart { match_type, .. } => {
                self.close(ts, None);
                let name = arena_name(self.last_zone.as_deref(), match_type);
                let mut seg = Segment::new(SegmentKind::Encounter, name, ts, self);
                seg.arena = true;
                self.segments.push(seg);
                self.last_combat_ms = Some(ts);
                self.in_arena = true;
                self.arena_home = None;
                self.arena_factions.clear();
                self.arena_over = false;
            }
            // R13: win iff the winning side is the home side. A stray END
            // with no START behind it (log began mid-match) closes nothing,
            // and an END before the home side resolved closes verdict-less.
            Event::ArenaMatchEnd { winning_team } => {
                if self.in_arena {
                    self.in_arena = false;
                    let verdict = self.arena_home.take().map(|h| *winning_team == h);
                    self.arena_factions.clear();
                    self.close(ts, verdict);
                }
                // Unconditional — even an END whose START predates the log
                // leaves us standing in a decided arena (R13 noise until the
                // teleport out).
                self.arena_over = true;
            }

            // R12: a cast is evidence about ITEMS only. It never opens or
            // extends a segment (scanner lockstep), and it is deliberately
            // not a class-inference source — R8's sources are fixed, and
            // widening them here would silently move fixture expectations.
            // R18: through the passive gate, like every mark and span call
            // site — a cast after a segment's end (or past the trash gap)
            // lands nowhere. Before R18 this reached `segments.last_mut()`
            // unguarded, so a use after ENCOUNTER_END marked the closed
            // pull past its end; the gate closes that R12 hole too.
            Event::Cast { src, spell } => {
                if src.is_player()
                    && let Some(s) = self.open_segment_for_passive(ts)
                {
                    let guid = src.guid.clone();
                    s.note_mark(&guid, spell, ts, true);
                }
            }

            Event::Summon { owner, pet } => {
                self.learn(owner);
                self.learn(pet);
                let (p, o) = (pet.guid.clone(), owner.guid.clone());
                self.note_owner(&p, &o);
            }

            // R1: the absorbed portion still counts as damage done.
            Event::Damage {
                src,
                dst,
                spell,
                amount,
                overkill,
                absorbed,
                blocked,
                critical,
                ..
            } => {
                self.learn(src);
                self.learn(dst);
                let label = spell
                    .as_ref()
                    .map_or("Melee", |s| s.name.as_str())
                    .to_string();
                let (guid, target) = (src.guid.clone(), dst.name.clone());
                let dst_guid = dst.guid.clone();
                let spell_id = spell.as_ref().map_or(0, |s| s.id);
                // v15: a swing has no spell block — it is Physical (1).
                let school = spell.as_ref().map_or(1, |s| s.school);
                self.record(
                    ts,
                    &guid,
                    View::Damage,
                    &label,
                    spell_id,
                    school,
                    &target,
                    amount + absorbed,
                    (*overkill).max(0) as u64,
                    *critical,
                );
                // R17: the same event lands a second time, on its VICTIM, when
                // that is a player or pet — straight into the segment the
                // Damage record just opened or extended (never `Meter::record`:
                // that would be a second `ensure_combat` for one line). Same
                // amount convention as R1 (`amount + absorbed`; the log's
                // amount is already post-block), absorbed in `extra`, keyed by
                // the ATTACKER's name like every other view's by_target.
                if is_friendly_source(&dst_guid)
                    && let Some(s) = self.segments.last_mut()
                {
                    let stagger_tick = spell_id == STAGGER_TICK && guid == dst_guid;
                    if stagger_tick {
                        // The staggered portion was Taken in full on the hit
                        // it came from (its `absorbed`); the tick re-deals
                        // it. Tallied apart, at the amount Taken would have
                        // carried, so Σ dealt = Σ Taken + Σ ticked exactly.
                        s.mitigation_mut(&dst_guid).stagger_ticked += amount + absorbed;
                    } else {
                        let attacker = if nil_guid(&guid) {
                            ENVIRONMENT
                        } else {
                            src.name.as_str()
                        };
                        s.record(
                            &dst_guid,
                            View::Taken,
                            &label,
                            spell_id,
                            school,
                            attacker,
                            amount + absorbed,
                            *absorbed,
                            *critical,
                        );
                        let m = s.mitigation_mut(&dst_guid);
                        m.absorbed += absorbed;
                        m.blocked += blocked;
                        // R18: the taken series, same amount, same grid.
                        s.bucket_taken(&dst_guid, ts, amount + absorbed);
                    }
                }
                self.name_trash(&guid, &dst_guid, &target);
                // R13: the first friendly-flagged player to land a damage
                // event names the home side (all friendlies share one, so
                // which one resolves it cannot change the answer).
                if self.in_arena
                    && self.arena_home.is_none()
                    && guid.starts_with("Player-")
                    && src.flags & 0x10 != 0
                    && let Some(f) = self.arena_factions.get(&guid)
                {
                    self.arena_home = Some(*f);
                }
                // R11: duels and world PvP are meaningful combat even with
                // no hostile NPC in sight; self-damage is not.
                if is_friendly_source(&guid)
                    && dst_guid.starts_with("Player-")
                    && guid != dst_guid
                    && let Some(s) = self.segments.last_mut()
                {
                    s.pvp = true;
                }
                if let Some(sp) = spell {
                    self.infer(src, sp);
                }
                // R9: victims remember the hit. `amount` alone — the absorbed
                // part never touched their health and shows as its own gain
                // entry. The hit's own advanced block describes the victim
                // only on SPELL_*/RANGE_* lines; a swing's HP back-fills from
                // its LANDED twin via `note_hp`.
                if dst.is_player()
                    && *amount > 0
                    && let Some(s) = self.segments.last_mut()
                {
                    let hp = line
                        .hp_hint
                        .as_ref()
                        .filter(|h| h.unit_guid == dst.guid)
                        .map(|h| (h.current, h.max));
                    s.recap_push(
                        &dst.guid,
                        RecapEntry {
                            ts,
                            spell: spell
                                .as_ref()
                                .map_or("Melee", |sp| sp.name.as_str())
                                .to_string(),
                            src: src.name.clone(),
                            amount: *amount,
                            extra: (*overkill).max(0) as u64,
                            crit: *critical,
                            gain: false,
                            hp,
                        },
                    );
                }
            }

            // R2: rows carry effective healing, with overheal in `extra`.
            Event::Heal {
                src,
                dst,
                spell,
                amount,
                overheal,
                critical,
                ..
            } => {
                self.learn(src);
                self.learn(dst);
                if NON_HEALING_ABSORBS.contains(&spell.id) {
                    // Not healing, but still class evidence (a shield names its
                    // caster's class); never records, so it can't gap-split.
                    self.infer(src, spell);
                    return;
                }
                let (guid, label, target) =
                    (src.guid.clone(), spell.name.clone(), dst.name.clone());
                let effective = amount.saturating_sub(*overheal);
                self.record(
                    ts,
                    &guid,
                    View::Healing,
                    &label,
                    spell.id,
                    spell.school,
                    &target,
                    effective,
                    *overheal,
                    *critical,
                );
                self.infer(src, spell);
                // R2 amendment: the same effective amount lands on the
                // VICTIM's side as healing received — from any source, an
                // NPC's included — into the segment `record` just chose.
                if is_friendly_source(&dst.guid)
                    && let Some(s) = self.segments.last_mut()
                {
                    let h = s.healed.entry(dst.guid.clone()).or_default();
                    h.received += effective;
                    if src.guid == dst.guid {
                        h.self_healed += effective;
                    }
                }
                // R9: gains land in the recap too — a fully-overhealed potion
                // (amount 0, overheal in extra) is still worth seeing.
                if dst.is_player()
                    && let Some(s) = self.segments.last_mut()
                {
                    let hp = line
                        .hp_hint
                        .as_ref()
                        .filter(|h| h.unit_guid == dst.guid)
                        .map(|h| (h.current, h.max));
                    s.recap_push(
                        &dst.guid,
                        RecapEntry {
                            ts,
                            spell: spell.name.clone(),
                            src: src.name.clone(),
                            amount: effective,
                            extra: *overheal,
                            crit: *critical,
                            gain: true,
                            hp,
                        },
                    );
                }
            }

            // R2/R3: absorbs are healing credited to the shield's caster.
            Event::Absorbed {
                dst,
                absorber,
                absorb_spell,
                amount,
                ..
            } => {
                self.learn(absorber);
                self.learn(dst);
                if NON_HEALING_ABSORBS.contains(&absorb_spell.id) {
                    self.infer(absorber, absorb_spell);
                    // R17: what Stagger (or cheat-death) soaked on the victim.
                    // A subset of the paired damage line's `absorbed` (R3's
                    // premise), so reported and never added to `mitigated`.
                    // Into the OPEN, non-stale segment only: this line is not
                    // combat to the scanner and must not open, extend or split
                    // one. The game logs it just BEFORE the hit it shields, so
                    // the line that precedes a pull's first hit (after an
                    // ENCOUNTER_END or a >60 s lull) is dropped: the pull's
                    // slice starts at the hit, and a lazy load could never
                    // see it — attributing it forward would break parity.
                    if is_friendly_source(&dst.guid)
                        && let Some(s) = self.open_segment_for_passive(ts)
                    {
                        s.mitigation_mut(&dst.guid).stagger += amount;
                    }
                    return;
                }
                let (guid, label, target) = (
                    absorber.guid.clone(),
                    absorb_spell.name.clone(),
                    dst.name.clone(),
                );
                // An absorb's crit flag is unknowable from SPELL_ABSORBED:
                // counted, never a crit.
                self.record(
                    ts,
                    &guid,
                    View::Healing,
                    &label,
                    absorb_spell.id,
                    absorb_spell.school,
                    &target,
                    *amount,
                    0,
                    false,
                );
                // R2 amendment: the absorb half of the absorber's Healing
                // row, counted where the row was — after the exclusion
                // above, into the segment `record` just chose.
                if let Some(s) = self.segments.last_mut() {
                    *s.absorbed_credit.entry(guid.clone()).or_default() += amount;
                    // R20: the same segment the credit went to, AFTER
                    // `record` (an absorb is combat to the scanner and may
                    // have just opened the segment), so Σ rows.consumed =
                    // `absorbed_healing` per absorber exactly — which is why
                    // this is keyed and gated EXACTLY like the credit above:
                    // the raw absorber guid, whatever it is (a Monk's
                    // Celestial guardian absorbs as a `Creature-` whose
                    // owner the fold resolves; a real log credits half a
                    // healer's absorbs that way). Table or not: a shield the
                    // auras never named opens unknown-applied here.
                    s.shield_absorb(&dst.guid, absorb_spell, &guid, *amount);
                }
                self.infer(absorber, absorb_spell);
                // R9: a consumed shield is a gain the victim's recap shows.
                // SPELL_ABSORBED has no advanced block; HP back-fills from
                // the paired damage line right behind it.
                if dst.is_player()
                    && let Some(s) = self.segments.last_mut()
                {
                    s.recap_push(
                        &dst.guid,
                        RecapEntry {
                            ts,
                            spell: absorb_spell.name.clone(),
                            src: absorber.name.clone(),
                            amount: *amount,
                            extra: 0,
                            crit: false,
                            gain: true,
                            hp: None,
                        },
                    );
                }
            }

            Event::Interrupt {
                src,
                dst,
                spell,
                interrupted_spell,
            } => {
                self.learn(src);
                self.learn(dst);
                // The drill answers "what did the kick stop", not "which kick":
                // the interrupted cast leads, the interrupt ability in parens.
                let label = format!("{} ({})", interrupted_spell.name, spell.name);
                let (guid, target) = (src.guid.clone(), dst.name.clone());
                self.record(
                    ts,
                    &guid,
                    View::Interrupts,
                    &label,
                    interrupted_spell.id,
                    interrupted_spell.school,
                    &target,
                    1,
                    0,
                    false,
                );
                self.infer(src, spell);
            }

            Event::Dispel {
                src, dst, spell, ..
            } => {
                self.learn(src);
                self.learn(dst);
                let (guid, label, target) =
                    (src.guid.clone(), spell.name.clone(), dst.name.clone());
                self.record(
                    ts,
                    &guid,
                    View::Dispels,
                    &label,
                    spell.id,
                    spell.school,
                    &target,
                    1,
                    0,
                    false,
                );
                self.infer(src, spell);
            }

            Event::AuraApplied {
                src,
                dst,
                spell,
                aura_type,
                absorb,
            } => {
                self.learn(src);
                self.learn(dst);
                // R20: a Buff in the absorb-spell table from a caster the
                // group controls (`controlled`) opens a shield on its target
                // — through the passive gate, beside (never instead of) the
                // span and mark paths below. Never on the trailer alone:
                // Feast of Souls and every `BUFF,0,0` carry one.
                if *aura_type == AuraType::Buff
                    && crate::absorb_spells::is_absorb_spell(spell.id)
                    && let Some(s) = self.open_segment_for_passive(ts)
                    && s.controlled(&src.guid)
                {
                    s.shield_apply(&dst.guid, spell, &src.guid, *absorb);
                }
                if *aura_type == AuraType::Debuff && CC_SPELLS.contains(&spell.id) {
                    // Like the interrupt drill: what got locked down leads, so
                    // the by-spell pane reads "Polymorph (Fizzle the Mad)".
                    let label = format!("{} ({})", spell.name, dst.name);
                    let (guid, target) = (src.guid.clone(), dst.name.clone());
                    self.record(
                        ts,
                        &guid,
                        View::CrowdControl,
                        &label,
                        spell.id,
                        spell.school,
                        &target,
                        1,
                        0,
                        false,
                    );
                }
                // R18 first: a Buff in the role table opens a span on its
                // target with the caster — consulted BEFORE the class-spells
                // veto (Power Infusion is a priest spell) and bypassing the
                // item dedupe. Otherwise R12: a buff landing on a player with
                // no cast behind it is a proc. Both through the passive gate:
                // neither opens or extends a segment, and an aura after a
                // segment's end lands nowhere (before R18 the mark path
                // reached the closed segment unguarded — the R12 hole this
                // closes).
                if *aura_type == AuraType::Buff && span_target(dst) {
                    let role = crate::role_spells::role_kind(spell.id);
                    if let Some(s) = self.open_segment_for_passive(ts) {
                        let guid = dst.guid.clone();
                        match role {
                            Some(kind) => {
                                s.note_span(&guid, spell, mark_kind_of(kind), &src.guid, ts, false);
                            }
                            None if dst.is_player() => {
                                s.note_mark(&guid, spell, ts, false);
                            }
                            None => {}
                        }
                    }
                }
                // After the possible record: a CC aura is combat and may have
                // just gap-split; any other aura never records in either the
                // meter or the scanner, so inferring from it here is safe.
                self.infer(src, spell);
            }

            // R18: a refresh matters only to role spans — while one is open
            // it is a no-op; with none open it is the "buff predated the
            // segment" signal and opens one at the segment's start. Never an
            // item mark, never an R8 signal, never opens or extends a segment.
            Event::AuraRefresh {
                src,
                dst,
                spell,
                aura_type,
                absorb,
            } => {
                self.learn(src);
                self.learn(dst);
                // R20: the trailer is the shield's new running total.
                if *aura_type == AuraType::Buff
                    && crate::absorb_spells::is_absorb_spell(spell.id)
                    && let Some(s) = self.open_segment_for_passive(ts)
                    && s.controlled(&src.guid)
                {
                    s.shield_refresh(&dst.guid, spell, &src.guid, *absorb);
                }
                if *aura_type == AuraType::Buff
                    && span_target(dst)
                    && let Some(kind) = crate::role_spells::role_kind(spell.id)
                    && let Some(s) = self.open_segment_for_passive(ts)
                {
                    let guid = dst.guid.clone();
                    s.note_span(&guid, spell, mark_kind_of(kind), &src.guid, ts, true);
                }
            }

            // v13: the buff coming off closes the player's open marker span.
            // Like AuraApplied's marker path, this never opens or extends a
            // segment (scanner lockstep).
            Event::AuraRemoved {
                src,
                dst,
                spell,
                aura_type,
                absorb,
            } => {
                self.learn(src);
                self.learn(dst);
                // R20: the trailer is what remained — the waste.
                if *aura_type == AuraType::Buff
                    && crate::absorb_spells::is_absorb_spell(spell.id)
                    && let Some(s) = self.open_segment_for_passive(ts)
                    && s.controlled(&src.guid)
                {
                    s.shield_remove(&dst.guid, spell, &src.guid, *absorb);
                }
                // R18: a role buff closes its span (segment-start rule when
                // none is open); anything else closes an item mark. Through
                // the passive gate, like the apply.
                if *aura_type == AuraType::Buff && span_target(dst) {
                    let role = crate::role_spells::role_kind(spell.id);
                    if let Some(s) = self.open_segment_for_passive(ts) {
                        let guid = dst.guid.clone();
                        match role {
                            Some(kind) => {
                                s.close_span(&guid, spell, mark_kind_of(kind), &src.guid, ts);
                            }
                            None if dst.is_player() => s.close_mark(&guid, spell.id, ts),
                            None => {}
                        }
                    }
                }
            }

            Event::Death { unit } => {
                self.learn(unit);
                if unit.is_player() {
                    let guid = unit.guid.clone();
                    self.record(ts, &guid, View::Deaths, "Death", 0, 0, "", 1, 0, false);
                    // R9: freeze the ring as this death's recap (latest death
                    // wins) and remember who went down when. Draining the
                    // ring starts the next life's recap clean.
                    if let Some(s) = self.segments.last_mut() {
                        let recap = s
                            .recent
                            .remove(&guid)
                            .map(|r| r.into_iter().collect())
                            .unwrap_or_default();
                        s.recaps.insert(guid.clone(), recap);
                        if !s.death_order.contains(&guid) {
                            s.death_order.push(guid.clone());
                        }
                    }
                }
            }

            Event::CombatantInfo {
                guid,
                spec_id,
                faction,
                talents,
                gear,
            } => {
                // R13: inside a match the faction field is the player's SIDE.
                if self.in_arena && !guid.is_empty() {
                    self.arena_factions.insert(guid.clone(), *faction);
                }
                // Authoritative: overwrites anything R8 inference guessed, and
                // (unlike inference) persists into future segments via seeding.
                if let Some(spec) = spec_id.and_then(Spec::from_id)
                    && !guid.is_empty()
                {
                    let class = spec.class();
                    self.classes.insert(guid.clone(), class);
                    self.specs.insert(guid.clone(), spec);
                    if let Some(s) = self.segments.last_mut() {
                        s.classes.insert(guid.clone(), class);
                        s.specs.insert(guid.clone(), spec);
                    }
                }
                // A bracket that parsed empty carries no information (absent,
                // or truncated by a mid-write read) — it must not wipe what an
                // earlier, intact line established. PER FIELD: a line cut
                // inside the gear bracket still has full talents, and taking
                // its word on gear would erase the player's real equipment.
                if !guid.is_empty() && (!talents.is_empty() || !gear.is_empty()) {
                    let prev = self.loadouts.get(guid.as_str());
                    let loadout = Arc::new(Loadout {
                        spec_id: *spec_id,
                        talents: if talents.is_empty() {
                            prev.map(|p| p.talents.clone()).unwrap_or_default()
                        } else {
                            talents.clone()
                        },
                        gear: if gear.is_empty() {
                            prev.map(|p| p.gear.clone()).unwrap_or_default()
                        } else {
                            gear.clone()
                        },
                    });
                    self.loadouts.insert(guid.clone(), Arc::clone(&loadout));
                    if let Some(s) = self.segments.last_mut() {
                        s.loadouts.insert(guid.clone(), loadout);
                    }
                }
            }

            // R10: visit tracking. Every zone change closes the open Trash
            // segment; a nonzero difficulty means instanced content.
            Event::ZoneChange {
                map_id,
                name,
                difficulty,
            } => {
                self.close_trash(ts);
                // R13: any teleport ends the dead-arena window.
                self.arena_over = false;
                // R13: remembered at every difficulty — arena zones log 0.
                if !name.is_empty() {
                    self.last_zone = Some(name.clone());
                }
                if *difficulty == 0 {
                    // Leaving suspends the visit: it resumes on re-entry, and
                    // outside combat records with no visit.
                    self.zoned_in = false;
                } else {
                    // A keyed visit resumes on the map alone: the game
                    // re-fires ZONE_CHANGE mid-run (reloads, reconnects)
                    // with the keystone difficulty, which differs from the
                    // difficulty stamped at the door — that must not split
                    // the run or its END gets orphaned.
                    let same = self.current_visit.is_some_and(|i| {
                        self.visits.get(i as usize).is_some_and(|v| {
                            v.map_id == *map_id && (v.keyed || v.difficulty == *difficulty)
                        })
                    });
                    if same {
                        self.zoned_in = true;
                    } else {
                        self.close_visit(ts);
                        self.visits.push(Visit {
                            map_id: *map_id,
                            difficulty: *difficulty,
                            name: name.clone(),
                            key_level: None,
                            keyed: false,
                            start_ms: ts,
                            end_ms: None,
                            completed: None,
                            official_ms: None,
                            pars_ms: None,
                        });
                        self.current_visit = Some(self.visits.len() as u32 - 1);
                        self.zoned_in = true;
                    }
                }
            }

            // R10: a keystone activated — the dungeon resets and the key's
            // clock starts here, not at the door, so every START is a visit
            // boundary: whatever happened since zoning in (readiness heals,
            // an earlier key) stays behind in the closed visit, and the
            // fresh keyed visit IS the run.
            Event::ChallengeModeStart {
                map_id,
                challenge_id,
                key_level,
            } => {
                let Some(i) = self.current_visit else {
                    return;
                };
                let (difficulty, name) = {
                    let Some(v) = self.visits.get(i as usize) else {
                        return;
                    };
                    if v.map_id != *map_id {
                        return;
                    }
                    (v.difficulty, v.name.clone())
                };
                self.close_trash(ts);
                self.close_visit(ts);
                self.visits.push(Visit {
                    map_id: *map_id,
                    difficulty,
                    name,
                    key_level: Some(*key_level),
                    keyed: true,
                    start_ms: ts,
                    end_ms: None,
                    completed: None,
                    official_ms: None,
                    pars_ms: crate::keystone_timers::pars_ms(*challenge_id),
                });
                self.current_visit = Some(self.visits.len() as u32 - 1);
                self.zoned_in = true;
            }

            // R10: only a keyed visit's END counts — the zeroed reset the
            // game fires on entry precedes any START and is ignored.
            Event::ChallengeModeEnd {
                map_id,
                success,
                total_ms,
            } => {
                if let Some(i) = self.current_visit
                    && let Some(v) = self.visits.get_mut(i as usize)
                    && v.map_id == *map_id
                    && v.keyed
                {
                    v.completed = Some(*success);
                    v.official_ms = (*total_ms > 0).then_some(*total_ms);
                }
            }

            // R17: a hit that did not land is count 1 / amount 0 on the
            // victim's Taken row and its drill rows, and its kind (plus a
            // BLOCK's amount or an ABSORB's amountMissed — damage prevented
            // outright) goes to the mitigation record. Written into the OPEN,
            // non-stale segment only, mirroring R16: the scanner ignores
            // `*_MISSED`, so a miss must never open, extend or split a
            // segment — no `ensure_combat`, no `last_ms` — or lazy/full
            // parity breaks; and a miss past the trash gap belongs to no pull.
            Event::Missed {
                src,
                dst,
                spell,
                kind,
                prevented,
                ..
            } => {
                self.learn(src);
                self.learn(dst);
                if !is_friendly_source(&dst.guid) {
                    return;
                }
                let Some(s) = self.open_segment_for_passive(ts) else {
                    return;
                };
                let label = spell.as_ref().map_or("Melee", |sp| sp.name.as_str());
                let attacker = if nil_guid(&src.guid) {
                    ENVIRONMENT
                } else {
                    src.name.as_str()
                };
                s.record(
                    &dst.guid,
                    View::Taken,
                    label,
                    spell.as_ref().map_or(0, |sp| sp.id),
                    spell.as_ref().map_or(1, |sp| sp.school),
                    attacker,
                    0,
                    0,
                    false,
                );
                let m = s.mitigation_mut(&dst.guid);
                m.miss(*kind);
                match kind {
                    MissKind::Absorb => m.absorbed_full += prevented,
                    MissKind::Block => m.blocked_full += prevented,
                    _ => {}
                }
            }
            // R19: a share of a hit or heal already counted by R1 / R2 —
            // bookkeeping on the supporter (given) and the buffed source
            // (received), never damage or healing. Through the passive gate
            // like a miss: the scanner ignores every support family, so a
            // share must never open, extend or split a segment, and one
            // logged before a pull's first hit (or past the trash gap)
            // belongs to nobody — full = lazy. Raw-keyed on every side —
            // supporter, buffed source, and the targets drill's inner key:
            // the supporter's name is not on the line, and a buffed pet's
            // owner may not be known yet — everything resolves at read.
            Event::Support {
                src,
                dst,
                supporter,
                amount,
                healing,
                ..
            } => {
                self.learn(src);
                self.learn(dst);
                let Some(s) = self.open_segment_for_passive(ts) else {
                    return;
                };
                let given = s.support.entry(supporter.clone()).or_default();
                if *healing {
                    given.given_healing += amount;
                } else {
                    given.given_damage += amount;
                }
                let received = s.support.entry(src.guid.clone()).or_default();
                if *healing {
                    received.received_healing += amount;
                } else {
                    received.received_damage += amount;
                }
                let t = s
                    .support_targets
                    .entry(supporter.clone())
                    .or_default()
                    .entry(src.guid.clone())
                    .or_default();
                if *healing {
                    t.healing += amount;
                } else {
                    t.damage += amount;
                }
                t.lines += 1;
            }
            Event::Other => {}
        }
    }

    /// History, oldest first; the last entry is the live/current segment.
    pub fn segments(&self) -> &[Segment] {
        &self.segments
    }

    /// R10: every instance visit seen, in file order (ordinals index here).
    pub fn visits(&self) -> &[Visit] {
        &self.visits
    }

    /// R10: the visit's Overall — its member segments merged into one
    /// synthetic segment. `None` until the visit has a member.
    pub fn overall(&self, ordinal: u32) -> Option<Segment> {
        let v = self.visits.get(ordinal as usize)?;
        let mut members = self
            .segments
            .iter()
            .filter(|s| s.visit == Some(ordinal))
            .peekable();
        members.peek()?;
        let mut out = Segment::new(SegmentKind::Overall, v.display_name(), v.start_ms, self);
        out.visit = Some(ordinal);
        out.end_ms = v.end_ms;
        out.key = v.keyed;
        out.official_ms = v.official_ms;
        for m in members {
            out.absorb(m);
        }
        // The TIMED verdict, evaluated at the newest merged event — the
        // daemon's live paths re-evaluate against its own clock instead.
        out.success = v.verdict(out.last_ms);
        Some(out)
    }

    pub fn current_index(&self) -> usize {
        self.segments.len().saturating_sub(1)
    }

    /// The player's latest COMBATANT_INFO loadout across the whole log so
    /// far. The per-segment view is `Segment::loadout`.
    pub fn loadout(&self, player_guid: &str) -> Option<&Loadout> {
        self.loadouts.get(player_guid).map(Arc::as_ref)
    }
}

/// Replay raw lines into a fresh meter — the lazy-load path, shared with the
/// tests. Pure: no I/O, no clock.
pub fn meter_from_lines<'a, I: IntoIterator<Item = &'a str>>(lines: I) -> Meter {
    let mut meter = Meter::new();
    for line in lines {
        if let Some(parsed) = crate::parser::parse_line(line) {
            meter.feed(parsed);
        }
    }
    meter
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::Spell;

    const P1: &str = "Player-1-AAA";
    const P2: &str = "Player-1-BBB";
    const PET: &str = "Pet-0-111";
    const BOSS: &str = "Creature-0-999";

    fn unit(guid: &str, name: &str, flags: u32) -> Unit {
        Unit {
            guid: guid.into(),
            name: name.into(),
            flags,
        }
    }

    fn p1() -> Unit {
        unit(P1, "Alice", 0x511)
    }
    fn p2() -> Unit {
        unit(P2, "Bob", 0x512)
    }
    fn pet() -> Unit {
        unit(PET, "Felhunter", 0x1114)
    }
    fn boss() -> Unit {
        unit(BOSS, "Ulgrax", 0xa48)
    }

    fn sp(id: u32, name: &str) -> Spell {
        Spell {
            id,
            name: name.into(),
            school: 1,
        }
    }

    fn at(ts: i64, event: Event) -> LogLine {
        LogLine::new(ts, event)
    }

    fn damage(ts: i64, src: Unit, spell: Option<Spell>, amount: u64) -> LogLine {
        at(
            ts,
            Event::Damage {
                src,
                dst: boss(),
                spell,
                amount,
                overkill: -1,
                absorbed: 0,
                blocked: 0,
                critical: false,
                periodic: false,
            },
        )
    }

    fn heal(ts: i64, src: Unit, amount: u64, overheal: u64) -> LogLine {
        at(
            ts,
            Event::Heal {
                src,
                dst: p2(),
                spell: sp(2061, "Flash Heal"),
                amount,
                overheal,
                absorbed: 0,
                critical: false,
            },
        )
    }

    fn fed(lines: Vec<LogLine>) -> Meter {
        let mut m = Meter::new();
        for l in lines {
            m.feed(l);
        }
        m
    }

    fn start(ts: i64, name: &str) -> LogLine {
        at(
            ts,
            Event::EncounterStart {
                id: 1,
                name: name.into(),
                difficulty: 14,
                group_size: 20,
            },
        )
    }

    fn end(ts: i64, name: &str, success: bool) -> LogLine {
        at(
            ts,
            Event::EncounterEnd {
                id: 1,
                name: name.into(),
                success,
            },
        )
    }

    // ---- segmentation -----------------------------------------------------

    #[test]
    fn encounter_start_and_end_bound_a_segment() {
        let m = fed(vec![
            start(1_000, "Ulgrax"),
            damage(2_000, p1(), Some(sp(133, "Fireball")), 100),
            end(5_000, "Ulgrax", true),
        ]);
        assert_eq!(m.segments().len(), 1);
        let s = &m.segments()[0];
        assert_eq!(s.kind, SegmentKind::Encounter);
        assert_eq!(s.name, "Ulgrax");
        assert_eq!(s.start_ms, 1_000);
        assert_eq!(s.end_ms, Some(5_000));
        assert_eq!(s.success, Some(true));
        assert_eq!(s.duration_ms(9_999), 4_000, "closed segments ignore now_ms");
    }

    #[test]
    fn wipe_records_failure() {
        let m = fed(vec![start(0, "Ulgrax"), end(1_000, "Ulgrax", false)]);
        assert_eq!(m.segments()[0].success, Some(false));
    }

    #[test]
    fn live_segment_duration_uses_now() {
        let m = fed(vec![start(1_000, "Ulgrax"), damage(2_000, p1(), None, 10)]);
        let s = &m.segments()[0];
        assert_eq!(s.end_ms, None);
        assert_eq!(s.duration_ms(4_000), 3_000);
    }

    #[test]
    fn damage_outside_an_encounter_opens_a_trash_segment() {
        let m = fed(vec![damage(1_000, p1(), Some(sp(133, "Fireball")), 500)]);
        assert_eq!(m.segments().len(), 1);
        assert_eq!(m.segments()[0].kind, SegmentKind::Trash);
        assert_eq!(m.segments()[0].name, "Ulgrax", "named after the enemy hit");
        assert_eq!(m.segments()[0].rows(View::Damage)[0].amount, 500);
    }

    /// A pull is named Details-style: the most-hit enemy, `+N` for the rest.
    #[test]
    fn trash_segments_are_named_after_their_enemies() {
        fn hit(ts: i64, dst: Unit) -> LogLine {
            at(
                ts,
                Event::Damage {
                    src: p1(),
                    dst,
                    spell: None,
                    amount: 100,
                    overkill: -1,
                    absorbed: 0,
                    blocked: 0,
                    critical: false,
                    periodic: false,
                },
            )
        }
        let wolf = || unit("Creature-0-100", "Rabid Wolf", 0xa48);
        let bear = || unit("Creature-0-101", "Angry Bear", 0xa48);

        let m = fed(vec![hit(0, wolf())]);
        assert_eq!(m.segments()[0].name, "Rabid Wolf");

        let m = fed(vec![hit(0, wolf()), hit(1_000, bear()), hit(2_000, wolf())]);
        assert_eq!(m.segments()[0].name, "Rabid Wolf +1", "most-hit wins");

        // Damage *taken* must not name the pull after a player, and enemy
        // in-fighting must not name it either.
        let m = fed(vec![
            hit(0, wolf()),
            at(
                1_000,
                Event::Damage {
                    src: boss(),
                    dst: p1(),
                    spell: None,
                    amount: 900,
                    overkill: -1,
                    absorbed: 0,
                    blocked: 0,
                    critical: false,
                    periodic: false,
                },
            ),
        ]);
        assert_eq!(m.segments()[0].name, "Rabid Wolf");
    }

    #[test]
    fn encounters_keep_their_boss_name() {
        let m = fed(vec![
            start(0, "Ulgrax the Devourer"),
            damage(1_000, p1(), None, 500),
        ]);
        assert_eq!(m.segments()[0].name, "Ulgrax the Devourer");
    }

    #[test]
    fn combat_gap_over_60s_starts_a_new_trash_segment() {
        let m = fed(vec![
            damage(0, p1(), None, 100),
            damage(10_000, p1(), None, 100),
            // 61s later
            damage(71_001, p1(), None, 100),
        ]);
        assert_eq!(m.segments().len(), 2, "gap > 60s splits");
        assert_eq!(m.segments()[0].rows(View::Damage)[0].amount, 200);
        assert_eq!(m.segments()[1].rows(View::Damage)[0].amount, 100);
    }

    #[test]
    fn gap_under_60s_stays_in_one_trash_segment() {
        let m = fed(vec![
            damage(0, p1(), None, 100),
            damage(59_000, p1(), None, 100),
        ]);
        assert_eq!(m.segments().len(), 1);
        assert_eq!(m.segments()[0].rows(View::Damage)[0].amount, 200);
    }

    #[test]
    fn encounter_start_closes_an_open_trash_segment() {
        let m = fed(vec![
            damage(0, p1(), None, 100),
            start(1_000, "Ulgrax"),
            damage(2_000, p1(), None, 50),
        ]);
        assert_eq!(m.segments().len(), 2);
        assert_eq!(m.segments()[0].kind, SegmentKind::Trash);
        assert!(
            m.segments()[0].end_ms.is_some(),
            "trash closed on encounter start"
        );
        assert_eq!(m.segments()[1].kind, SegmentKind::Encounter);
        assert_eq!(m.current_index(), 1);
    }

    #[test]
    fn damage_after_encounter_end_goes_to_a_new_trash_segment() {
        let m = fed(vec![
            start(0, "Ulgrax"),
            damage(1_000, p1(), None, 100),
            end(2_000, "Ulgrax", true),
            damage(3_000, p1(), None, 70),
        ]);
        assert_eq!(m.segments().len(), 2);
        assert_eq!(m.segments()[0].rows(View::Damage)[0].amount, 100);
        assert_eq!(m.segments()[1].kind, SegmentKind::Trash);
        assert_eq!(m.segments()[1].rows(View::Damage)[0].amount, 70);
    }

    /// R6: a mid-log COMBAT_LOG_VERSION means the logger restarted.
    #[test]
    fn mid_log_version_is_a_hard_boundary() {
        let m = fed(vec![
            at(
                0,
                Event::Summon {
                    owner: p1(),
                    pet: pet(),
                },
            ),
            damage(1_000, pet(), None, 100),
            at(
                2_000,
                Event::Version {
                    log_version: 22,
                    advanced: true,
                    build: (0, 0, 0),
                    project_id: 0,
                },
            ),
            damage(3_000, pet(), None, 40),
        ]);
        assert_eq!(m.segments().len(), 2);
        assert!(m.segments()[0].end_ms.is_some());
        // Pet ownership was reset, so the orphan pet no longer rolls up to Alice.
        assert!(
            m.segments()[1].rows(View::Damage).is_empty(),
            "pet-owner map must reset across the seam"
        );
    }

    // ---- R7: duration semantics -------------------------------------------

    /// R7: a Trash segment measures active combat, so idle time before it is closed
    /// must not inflate the duration (and thereby deflate DPS).
    #[test]
    fn trash_duration_is_first_to_last_combat_event() {
        let m = fed(vec![
            damage(0, p1(), None, 100),
            damage(18_000, p1(), None, 100),
            // 40s of nothing, then a boss pull closes the trash segment.
            start(58_000, "Ulgrax"),
        ]);
        let trash = &m.segments()[0];
        assert_eq!(trash.kind, SegmentKind::Trash);
        assert_eq!(
            trash.duration_ms(99_999),
            18_000,
            "18s of combat, not 58s of wall clock"
        );
    }

    #[test]
    fn live_trash_duration_ignores_now_ms() {
        let m = fed(vec![
            damage(0, p1(), None, 100),
            damage(5_000, p1(), None, 100),
        ]);
        let trash = &m.segments()[0];
        assert_eq!(trash.end_ms, None);
        assert_eq!(trash.duration_ms(999_999), 5_000);
    }

    #[test]
    fn split_trash_segments_each_measure_their_own_combat_span() {
        let m = fed(vec![
            damage(0, p1(), None, 100),
            damage(10_000, p1(), None, 100),
            // >60s gap splits here
            damage(80_000, p1(), None, 100),
            damage(83_000, p1(), None, 100),
        ]);
        assert_eq!(m.segments().len(), 2);
        assert_eq!(m.segments()[0].duration_ms(99_999), 10_000);
        assert_eq!(m.segments()[1].duration_ms(99_999), 3_000);
    }

    /// R7 leaves Encounters alone: ENCOUNTER_START..ENCOUNTER_END exactly, including
    /// the idle head and tail around the actual swings.
    #[test]
    fn encounter_duration_is_start_to_end_even_with_idle_tails() {
        let m = fed(vec![
            start(1_000, "Ulgrax"),
            damage(20_000, p1(), None, 100),
            damage(21_000, p1(), None, 100),
            end(60_000, "Ulgrax", true),
        ]);
        assert_eq!(m.segments()[0].duration_ms(99_999), 59_000);
    }

    #[test]
    fn trash_dps_uses_combat_time_not_wall_clock() {
        let m = fed(vec![
            damage(0, p1(), None, 1_000),
            damage(10_000, p1(), None, 1_000),
            start(70_000, "Ulgrax"),
        ]);
        let row = &m.segments()[0].rows(View::Damage)[0];
        assert_eq!(row.amount, 2_000);
        // 2000 damage over 10s of combat = 200 dps, not 2000/70s.
        assert!(
            (row.per_sec - 200.0).abs() < 1e-6,
            "got {} dps",
            row.per_sec
        );
    }

    /// A mid-log VERSION boundary closes the segment too, and must not stretch it.
    #[test]
    fn version_boundary_does_not_stretch_trash_duration() {
        let m = fed(vec![
            damage(0, p1(), None, 100),
            damage(4_000, p1(), None, 100),
            at(
                50_000,
                Event::Version {
                    log_version: 22,
                    advanced: true,
                    build: (0, 0, 0),
                    project_id: 0,
                },
            ),
        ]);
        assert_eq!(m.segments()[0].duration_ms(99_999), 4_000);
    }

    // ---- attribution ------------------------------------------------------

    #[test]
    fn pet_damage_rolls_up_to_its_owner() {
        let m = fed(vec![
            at(
                0,
                Event::Summon {
                    owner: p1(),
                    pet: pet(),
                },
            ),
            damage(1_000, p1(), None, 100),
            damage(1_500, pet(), None, 50),
        ]);
        let rows = m.segments()[0].rows(View::Damage);
        assert_eq!(rows.len(), 1, "the pet must not get its own meter row");
        assert_eq!(rows[0].key, P1);
        assert_eq!(rows[0].label, "Alice");
        assert_eq!(rows[0].amount, 150);
    }

    /// Pets act before their summon line in real logs; read-time resolution fixes it.
    #[test]
    fn pet_damage_before_the_summon_is_attributed_retroactively() {
        let m = fed(vec![
            damage(1_000, pet(), None, 700),
            at(
                2_000,
                Event::Summon {
                    owner: p1(),
                    pet: pet(),
                },
            ),
            damage(3_000, p1(), None, 300),
        ]);
        let rows = m.segments()[0].rows(View::Damage);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].amount, 1000, "the pre-summon 700 must be credited");
    }

    #[test]
    fn owner_hint_from_the_advanced_block_attributes_pets() {
        let mut l = damage(1_000, pet(), None, 250);
        l.owner_hint = Some(crate::parser::OwnerHint {
            unit_guid: PET.into(),
            owner_guid: P1.into(),
        });
        let m = fed(vec![damage(500, p1(), None, 10), l]);
        let rows = m.segments()[0].rows(View::Damage);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].amount, 260);
    }

    /// Real logs emit damage from a nil GUID carrying PLAYER flags; it must not become
    /// a phantom row.
    #[test]
    fn nil_source_with_player_flags_gets_no_row() {
        let nil_but_flagged = unit("0000000000000000", "", 0x514);
        let m = fed(vec![
            damage(0, p1(), None, 100),
            damage(1_000, nil_but_flagged, None, 5_000),
        ]);
        let rows = m.segments()[0].rows(View::Damage);
        assert_eq!(rows.len(), 1, "only the real player, got {rows:?}");
        assert_eq!(rows[0].key, P1);
        assert_eq!(rows[0].amount, 100);
    }

    #[test]
    fn nil_unit_death_is_not_a_player_death() {
        let m = fed(vec![
            damage(0, p1(), None, 1),
            at(
                1_000,
                Event::Death {
                    unit: unit("0000000000000000", "", 0x514),
                },
            ),
        ]);
        assert!(m.segments()[0].rows(View::Deaths).is_empty());
    }

    #[test]
    fn non_player_actors_get_no_rows() {
        let m = fed(vec![
            damage(0, p1(), None, 100),
            damage(1_000, boss(), None, 9_999),
        ]);
        let rows = m.segments()[0].rows(View::Damage);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].key, P1);
    }

    // ---- R1: damage accounting -------------------------------------------

    #[test]
    fn damage_amount_includes_the_absorbed_field() {
        let m = fed(vec![at(
            0,
            Event::Damage {
                src: p1(),
                dst: boss(),
                spell: Some(sp(133, "Fireball")),
                amount: 1_000,
                overkill: -1,
                absorbed: 250,
                blocked: 0,
                critical: false,
                periodic: false,
            },
        )]);
        assert_eq!(m.segments()[0].rows(View::Damage)[0].amount, 1_250);
    }

    #[test]
    fn overkill_is_clamped_to_zero_when_not_a_killing_blow() {
        let m = fed(vec![
            damage(0, p1(), None, 100), // overkill -1
            at(
                1_000,
                Event::Damage {
                    src: p1(),
                    dst: boss(),
                    spell: None,
                    amount: 500,
                    overkill: 300,
                    absorbed: 0,
                    blocked: 0,
                    critical: false,
                    periodic: false,
                },
            ),
        ]);
        let row = &m.segments()[0].rows(View::Damage)[0];
        assert_eq!(row.amount, 600);
        assert_eq!(row.extra, 300, "only the real overkill, -1 clamped away");
    }

    // ---- R2: healing accounting ------------------------------------------

    #[test]
    fn healing_is_effective_with_overheal_in_extra() {
        let m = fed(vec![heal(0, p1(), 20_000, 5_000)]);
        let row = &m.segments()[0].rows(View::Healing)[0];
        assert_eq!(row.amount, 15_000);
        assert_eq!(row.extra, 5_000);
    }

    #[test]
    fn full_overheal_contributes_nothing() {
        let m = fed(vec![heal(0, p1(), 8_000, 8_000), heal(1_000, p1(), 100, 0)]);
        let row = &m.segments()[0].rows(View::Healing)[0];
        assert_eq!(row.amount, 100);
        assert_eq!(row.extra, 8_000);
    }

    /// R2/R3: SPELL_ABSORBED credits the ABSORBER as healing.
    #[test]
    fn absorb_credits_the_absorber_as_healing() {
        let m = fed(vec![at(
            0,
            Event::Absorbed {
                src: boss(),
                dst: p2(),
                absorber: p1(),
                spell: None,
                absorb_spell: sp(17, "Power Word: Shield"),
                amount: 4_500,
            },
        )]);
        let rows = m.segments()[0].rows(View::Healing);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].key, P1, "the shield caster, not the shielded unit");
        assert_eq!(rows[0].amount, 4_500);
        assert_eq!(rows[0].extra, 0, "absorbs have no overheal component");
    }

    #[test]
    fn stagger_and_cheat_death_absorbs_are_not_healing() {
        let m = fed(vec![
            at(
                0,
                Event::Absorbed {
                    src: boss(),
                    dst: p1(),
                    absorber: p1(),
                    spell: None,
                    absorb_spell: sp(115069, "Stagger"),
                    amount: 99_999,
                },
            ),
            at(
                1_000,
                Event::Absorbed {
                    src: boss(),
                    dst: p1(),
                    absorber: p1(),
                    spell: None,
                    absorb_spell: sp(17, "Power Word: Shield"),
                    amount: 500,
                },
            ),
        ]);
        let rows = m.segments()[0].rows(View::Healing);
        assert_eq!(rows[0].amount, 500, "Stagger excluded");
    }

    // ---- ordering, pct, rates --------------------------------------------

    #[test]
    fn rows_are_sorted_desc_with_pct_and_dps() {
        let m = fed(vec![
            damage(0, p1(), None, 250),
            damage(1_000, p2(), None, 750),
            end_of_combat(),
        ]);
        let s = &m.segments()[0];
        let rows = s.rows(View::Damage);
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].key, P2, "biggest first");
        assert_eq!(rows[0].amount, 750);
        assert!((rows[0].pct - 75.0).abs() < 1e-6);
        assert!((rows[1].pct - 25.0).abs() < 1e-6);
        assert!((rows.iter().map(|r| r.pct).sum::<f64>() - 100.0).abs() < 1e-6);
    }

    fn end_of_combat() -> LogLine {
        // A no-op line just to give the segment a stable end for rate math.
        at(2_000, Event::Other)
    }

    #[test]
    fn per_sec_is_zero_for_count_views() {
        let m = fed(vec![at(
            0,
            Event::Interrupt {
                src: p1(),
                dst: boss(),
                spell: sp(57994, "Wind Shear"),
                interrupted_spell: sp(1, "Cast"),
            },
        )]);
        let rows = m.segments()[0].rows(View::Interrupts);
        assert_eq!(rows[0].amount, 1);
        assert_eq!(rows[0].per_sec, 0.0);
    }

    #[test]
    fn empty_view_has_no_rows_and_does_not_divide_by_zero() {
        let m = fed(vec![damage(0, p1(), None, 100)]);
        assert!(m.segments()[0].rows(View::Healing).is_empty());
        assert!(m.segments()[0].rows(View::Deaths).is_empty());
    }

    // ---- count views ------------------------------------------------------

    #[test]
    fn interrupts_and_dispels_count_events() {
        let m = fed(vec![
            at(
                0,
                Event::Interrupt {
                    src: p1(),
                    dst: boss(),
                    spell: sp(57994, "Wind Shear"),
                    interrupted_spell: sp(1, "Cast"),
                },
            ),
            at(
                1_000,
                Event::Interrupt {
                    src: p1(),
                    dst: boss(),
                    spell: sp(57994, "Wind Shear"),
                    interrupted_spell: sp(1, "Cast"),
                },
            ),
            at(
                2_000,
                Event::Dispel {
                    src: p2(),
                    dst: p1(),
                    spell: sp(527, "Purify"),
                    dispelled_spell: sp(2, "Curse"),
                },
            ),
        ]);
        let s = &m.segments()[0];
        assert_eq!(s.rows(View::Interrupts)[0].amount, 2);
        assert_eq!(s.rows(View::Dispels)[0].amount, 1);
        assert_eq!(s.rows(View::Dispels)[0].key, P2);
    }

    #[test]
    fn interrupt_drill_names_the_interrupted_cast() {
        let m = fed(vec![at(
            0,
            Event::Interrupt {
                src: p1(),
                dst: boss(),
                spell: sp(57994, "Wind Shear"),
                interrupted_spell: sp(686, "Shadow Bolt"),
            },
        )]);
        let (by_spell, by_target) = m.segments()[0].breakdown(P1, View::Interrupts);
        assert_eq!(by_spell[0].label, "Shadow Bolt (Wind Shear)");
        assert_eq!(by_target[0].label, "Ulgrax");
    }

    #[test]
    fn cc_drill_names_the_victim() {
        let m = fed(vec![at(
            0,
            Event::AuraApplied {
                src: p1(),
                dst: boss(),
                spell: sp(118, "Polymorph"),
                aura_type: AuraType::Debuff,
                absorb: None,
            },
        )]);
        let (by_spell, _) = m.segments()[0].breakdown(P1, View::CrowdControl);
        assert_eq!(by_spell[0].label, "Polymorph (Ulgrax)");
    }

    #[test]
    fn deaths_view_counts_player_deaths_only() {
        let m = fed(vec![
            damage(0, p1(), None, 1),
            at(1_000, Event::Death { unit: p1() }),
            at(2_000, Event::Death { unit: boss() }),
        ]);
        let rows = m.segments()[0].rows(View::Deaths);
        assert_eq!(rows.len(), 1, "boss deaths are not player deaths");
        assert_eq!(rows[0].key, P1);
        assert_eq!(rows[0].amount, 1);
    }

    fn hit_player(
        ts: i64,
        dst: Unit,
        spell_name: &str,
        amount: u64,
        overkill: i64,
        hp: Option<(u64, u64)>,
    ) -> LogLine {
        let guid = dst.guid.clone();
        let mut l = at(
            ts,
            Event::Damage {
                src: boss(),
                dst,
                spell: Some(sp(999, spell_name)),
                amount,
                overkill,
                absorbed: 0,
                blocked: 0,
                critical: false,
                periodic: false,
            },
        );
        if let Some((current, max)) = hp {
            l.hp_hint = Some(HpHint {
                unit_guid: guid,
                current,
                max,
                flags: 0,
            });
        }
        l
    }

    #[test]
    fn deaths_list_in_death_order_not_alphabetical() {
        let m = fed(vec![
            damage(0, p1(), None, 1),
            at(1_000, Event::Death { unit: p2() }),
            at(2_000, Event::Death { unit: p1() }),
        ]);
        let rows = m.segments()[0].rows(View::Deaths);
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].key, P2, "Bob died first, Alice sorts first");
        assert_eq!(rows[1].key, P1);
    }

    #[test]
    fn death_recap_leads_with_the_kill_hit_and_flags_gains() {
        let m = fed(vec![
            hit_player(0, p1(), "Slam", 50_000, -1, Some((100_000, 150_000))),
            at(
                500,
                Event::Heal {
                    src: p2(),
                    dst: p1(),
                    spell: sp(2061, "Flash Heal"),
                    amount: 30_000,
                    overheal: 10_000,
                    absorbed: 0,
                    critical: false,
                },
            ),
            hit_player(1_000, p1(), "Crush", 120_000, 20_000, Some((0, 150_000))),
            at(1_100, Event::Death { unit: p1() }),
        ]);
        let (events, attackers) = m.segments()[0].breakdown(P1, View::Deaths);
        assert_eq!(events.len(), 3, "newest first");
        assert_eq!(events[0].label, "Crush (Ulgrax)", "the kill hit leads");
        assert_eq!(events[0].extra, 20_000, "overkill rides in extra");
        assert_eq!(events[0].hp, Some((0, 150_000)));
        assert!(!events[0].gain);
        assert!(events[1].gain, "the heal is a gain");
        assert_eq!(events[1].amount, 20_000, "effective, overheal in extra");
        assert_eq!(events[1].extra, 10_000);
        assert_eq!(events[2].label, "Slam (Ulgrax)");
        assert_eq!(attackers.len(), 1, "gains never total as attackers");
        assert_eq!(attackers[0].label, "Ulgrax");
        assert_eq!(attackers[0].amount, 170_000);
    }

    #[test]
    fn recap_hp_backfills_from_a_following_report() {
        // A swing's advanced block describes its source, so the entry lands
        // without HP; the LANDED twin (Event::Other + hp_hint) fills it in.
        let mut landed = at(100, Event::Other);
        landed.hp_hint = Some(HpHint {
            unit_guid: P1.into(),
            current: 60_000,
            max: 150_000,
            flags: 0,
        });
        let m = fed(vec![
            hit_player(100, p1(), "Melee", 40_000, -1, None),
            landed,
            at(200, Event::Death { unit: p1() }),
        ]);
        let (events, _) = m.segments()[0].breakdown(P1, View::Deaths);
        assert_eq!(events[0].hp, Some((60_000, 150_000)));
    }

    #[test]
    fn recap_is_bounded_and_keeps_the_newest() {
        let mut lines: Vec<LogLine> = (0..40)
            .map(|i| hit_player(i, p1(), "Peck", 100 + i as u64, -1, None))
            .collect();
        lines.push(at(50, Event::Death { unit: p1() }));
        let m = fed(lines);
        let (events, _) = m.segments()[0].breakdown(P1, View::Deaths);
        assert_eq!(events.len(), RECAP_CAP);
        assert_eq!(events[0].amount, 139, "newest kept, oldest evicted");
    }

    #[test]
    fn binding_shot_counts_as_crowd_control() {
        let m = fed(vec![at(
            0,
            Event::AuraApplied {
                src: p1(),
                dst: boss(),
                spell: sp(117526, "Binding Shot"),
                aura_type: AuraType::Debuff,
                absorb: None,
            },
        )]);
        assert_eq!(m.segments()[0].rows(View::CrowdControl)[0].amount, 1);
    }

    #[test]
    fn crowd_control_counts_listed_debuffs_only() {
        let m = fed(vec![
            // Polymorph (118) is CC.
            at(
                0,
                Event::AuraApplied {
                    src: p1(),
                    dst: boss(),
                    spell: sp(118, "Polymorph"),
                    aura_type: AuraType::Debuff,
                    absorb: None,
                },
            ),
            // A random damage debuff is not CC.
            at(
                1_000,
                Event::AuraApplied {
                    src: p1(),
                    dst: boss(),
                    spell: sp(172, "Corruption"),
                    aura_type: AuraType::Debuff,
                    absorb: None,
                },
            ),
            // A CC-listed spell applied as a BUFF is not a CC application.
            at(
                2_000,
                Event::AuraApplied {
                    src: p1(),
                    dst: boss(),
                    spell: sp(118, "Polymorph"),
                    aura_type: AuraType::Buff,
                    absorb: None,
                },
            ),
        ]);
        let rows = m.segments()[0].rows(View::CrowdControl);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].amount, 1);
    }

    // ---- breakdown --------------------------------------------------------

    #[test]
    fn breakdown_splits_by_spell_and_target() {
        let m = fed(vec![
            damage(0, p1(), Some(sp(133, "Fireball")), 100),
            damage(1_000, p1(), Some(sp(133, "Fireball")), 50),
            damage(2_000, p1(), Some(sp(172, "Corruption")), 30),
        ]);
        let (spells, targets) = m.segments()[0].breakdown(P1, View::Damage);
        assert_eq!(spells.len(), 2);
        assert_eq!(spells[0].label, "Fireball");
        assert_eq!(spells[0].amount, 150);
        assert_eq!(spells[1].label, "Corruption");
        assert_eq!(spells[1].amount, 30);
        assert!((spells[0].pct - 150.0 / 180.0 * 100.0).abs() < 1e-6);

        assert_eq!(targets.len(), 1);
        assert_eq!(targets[0].label, "Ulgrax");
        assert_eq!(targets[0].amount, 180);
    }

    #[test]
    fn breakdown_counts_hits_and_crits() {
        let mut crit = damage(0, p1(), Some(sp(133, "Fireball")), 200);
        if let Event::Damage { critical, .. } = &mut crit.event {
            *critical = true;
        }
        let m = fed(vec![
            crit,
            damage(1_000, p1(), Some(sp(133, "Fireball")), 100),
            damage(2_000, p1(), Some(sp(133, "Fireball")), 100),
            damage(3_000, p1(), Some(sp(172, "Corruption")), 30),
        ]);
        let (spells, targets) = m.segments()[0].breakdown(P1, View::Damage);
        assert_eq!(spells[0].label, "Fireball");
        assert_eq!((spells[0].count, spells[0].crits), (3, 1));
        assert!((spells[0].crit_pct() - 100.0 / 3.0).abs() < 1e-6);
        assert_eq!((spells[1].count, spells[1].crits), (1, 0));
        assert_eq!(spells[1].crit_pct(), 0.0);
        // Targets and the meter row aggregate the same tallies.
        assert_eq!((targets[0].count, targets[0].crits), (4, 1));
        let rows = m.segments()[0].rows(View::Damage);
        assert_eq!((rows[0].count, rows[0].crits), (4, 1));
    }

    #[test]
    fn swings_appear_as_melee_in_the_breakdown() {
        let m = fed(vec![damage(0, p1(), None, 100)]);
        let (spells, _) = m.segments()[0].breakdown(P1, View::Damage);
        assert_eq!(spells[0].label, "Melee");
    }

    /// R5: pet rows are labelled "{spell} ({petName})" in the by-spell breakdown only.
    #[test]
    fn pet_spells_are_labelled_in_the_breakdown() {
        let m = fed(vec![
            at(
                0,
                Event::Summon {
                    owner: p1(),
                    pet: pet(),
                },
            ),
            damage(1_000, p1(), Some(sp(133, "Fireball")), 100),
            damage(2_000, pet(), Some(sp(3110, "Firebolt")), 40),
        ]);
        let s = &m.segments()[0];
        // The meter row stays merged under the owner.
        assert_eq!(s.rows(View::Damage).len(), 1);

        let (spells, _) = s.breakdown(P1, View::Damage);
        let labels: Vec<_> = spells.iter().map(|r| r.label.as_str()).collect();
        assert!(labels.contains(&"Fireball"), "got {labels:?}");
        assert!(labels.contains(&"Firebolt (Felhunter)"), "got {labels:?}");
    }

    /// R5 amendment: same-NAMED pet instances aggregate into one row —
    /// swarm specs summon dozens of identical ghouls/imps per fight, and a
    /// row per instance is unreadable. Different names stay separate.
    #[test]
    fn same_named_pet_instances_merge_in_the_breakdown() {
        let ghoul_a = unit("Creature-0-1111", "Lesser Ghoul", 0x2111);
        let ghoul_b = unit("Creature-0-2222", "Lesser Ghoul", 0x2111);
        let magus = unit("Creature-0-3333", "Magus of the Dead", 0x2111);
        let mut lines = vec![damage(0, p1(), Some(sp(133, "Fireball")), 100)];
        for pet in [&ghoul_a, &ghoul_b, &magus] {
            lines.push(at(
                0,
                Event::Summon {
                    owner: p1(),
                    pet: (*pet).clone(),
                },
            ));
        }
        lines.push(damage(1_000, ghoul_a, Some(sp(91776, "Claw")), 40));
        lines.push(damage(2_000, ghoul_b, Some(sp(91776, "Claw")), 60));
        lines.push(damage(3_000, magus, Some(sp(288548, "Shadow Bolt")), 30));
        let m = fed(lines);

        let s = &m.segments()[0];
        assert_eq!(s.rows(View::Damage).len(), 1, "one owner row");
        let (spells, _) = s.breakdown(P1, View::Damage);
        let claw: Vec<_> = spells
            .iter()
            .filter(|r| r.label == "Claw (Lesser Ghoul)")
            .collect();
        assert_eq!(claw.len(), 1, "one row for both ghouls: {spells:?}");
        assert_eq!((claw[0].amount, claw[0].count), (100, 2));
        assert!(
            spells
                .iter()
                .any(|r| r.label == "Shadow Bolt (Magus of the Dead)" && r.amount == 30),
            "differently named pets stay separate"
        );
    }

    #[test]
    fn breakdown_of_an_unknown_player_is_empty() {
        let m = fed(vec![damage(0, p1(), None, 100)]);
        let (spells, targets) = m.segments()[0].breakdown("Player-nope", View::Damage);
        assert!(spells.is_empty() && targets.is_empty());
    }

    #[test]
    fn healing_breakdown_carries_overheal_in_extra() {
        let m = fed(vec![heal(0, p1(), 20_000, 5_000)]);
        let (spells, targets) = m.segments()[0].breakdown(P1, View::Healing);
        assert_eq!(spells[0].label, "Flash Heal");
        assert_eq!((spells[0].amount, spells[0].extra), (15_000, 5_000));
        assert_eq!(targets[0].label, "Bob");
    }

    // ---- misc -------------------------------------------------------------

    #[test]
    fn a_fresh_meter_has_no_segments() {
        let m = Meter::new();
        assert!(m.segments().is_empty());
        assert_eq!(m.current_index(), 0);
    }

    #[test]
    fn non_combat_events_do_not_open_a_segment() {
        let m = fed(vec![
            at(0, Event::Other),
            at(
                1_000,
                Event::CombatantInfo {
                    guid: P1.into(),
                    spec_id: None,
                    faction: 0,
                    talents: vec![],
                    gear: vec![],
                },
            ),
        ]);
        assert!(m.segments().is_empty());
    }

    #[test]
    fn combatant_info_spec_colors_the_meter_row_and_its_pet_breakdown() {
        // Spec 253 = Beast Mastery -> Hunter. The class must show on the player's
        // meter row and on their drilldown rows, even for damage dealt by the pet.
        let m = fed(vec![
            at(
                0,
                Event::CombatantInfo {
                    guid: P1.into(),
                    spec_id: Some(253),
                    faction: 0,
                    talents: vec![],
                    gear: vec![],
                },
            ),
            at(
                1_000,
                Event::Summon {
                    owner: p1(),
                    pet: unit(PET, "Sharptooth", 0x1000),
                },
            ),
            damage(
                2_000,
                unit(PET, "Sharptooth", 0x1000),
                Some(sp(17253, "Bite")),
                500,
            ),
            // An id no class table knows: without COMBATANT_INFO, P2 stays
            // colorless (inference needs a class-identifying spell, R8).
            damage(3_000, p2(), Some(sp(999_999_999, "Odd Trinket")), 300),
        ]);
        let seg = m.segments().last().unwrap();
        let rows = seg.rows(View::Damage);
        let r1 = rows.iter().find(|r| r.key == P1).unwrap();
        assert_eq!(r1.class, Some(Class::Hunter));
        assert_eq!(r1.spec, Some(Spec::BeastMastery));
        // P2 never produced a COMBATANT_INFO: colorless, not wrong.
        let r2 = rows.iter().find(|r| r.key == P2).unwrap();
        assert_eq!(r2.class, None);
        assert_eq!(r2.spec, None);
        let (by_spell, _) = seg.breakdown(P1, View::Damage);
        assert!(by_spell.iter().all(|r| r.class == Some(Class::Hunter)));
    }

    #[test]
    fn r8_spell_cast_infers_class_without_combatant_info() {
        // Smite (585) is on the Priest class skill line: casting it colors the
        // row Priest even though no COMBATANT_INFO ever arrives.
        let m = fed(vec![damage(1_000, p2(), Some(sp(585, "Smite")), 300)]);
        let rows = m.segments().last().unwrap().rows(View::Damage);
        let r2 = rows.iter().find(|r| r.key == P2).unwrap();
        assert_eq!(r2.class, Some(Class::Priest));
        // Class-wide spell: identifies the class but not a spec.
        assert_eq!(r2.spec, None);
    }

    #[test]
    fn r8_inference_ignores_non_players_and_melee() {
        let m = fed(vec![
            damage(1_000, boss(), Some(sp(585, "Smite")), 500),
            damage(2_000, p1(), None, 400),
        ]);
        let seg = m.segments().last().unwrap();
        assert!(seg.rows(View::Damage).iter().all(|r| r.class.is_none()));
    }

    #[test]
    fn r8_combatant_info_overrides_inference() {
        // P1 casts a warrior spell (Mortal Strike 12294) before an encounter's
        // COMBATANT_INFO reveals spec 254 (Marksmanship Hunter): the
        // authoritative source wins.
        let m = fed(vec![
            damage(1_000, p1(), Some(sp(12294, "Mortal Strike")), 500),
            at(
                2_000,
                Event::CombatantInfo {
                    guid: P1.into(),
                    spec_id: Some(254),
                    faction: 0,
                    talents: vec![],
                    gear: vec![],
                },
            ),
        ]);
        let rows = m.segments().last().unwrap().rows(View::Damage);
        let r1 = rows.iter().find(|r| r.key == P1).unwrap();
        assert_eq!(r1.class, Some(Class::Hunter));
        assert_eq!(r1.spec, Some(Spec::Marksmanship));
    }

    #[test]
    fn r8_inference_is_segment_local_but_combatant_info_carries_forward() {
        // Segment 1: P1 inferred (Mortal Strike), P2 known via COMBATANT_INFO.
        // After a >60s lull, segment 2 opens: P1's inferred class must NOT
        // carry over (the lazy-load path couldn't reconstruct it), while P2's
        // COMBATANT_INFO-derived class must (it is seeded).
        let m = fed(vec![
            at(
                0,
                Event::CombatantInfo {
                    guid: P2.into(),
                    spec_id: Some(257),
                    faction: 0,
                    talents: vec![],
                    gear: vec![],
                },
            ),
            damage(1_000, p1(), Some(sp(12294, "Mortal Strike")), 500),
            damage(1_500, p2(), None, 300),
            damage(200_000, p1(), None, 500),
            damage(200_500, p2(), None, 300),
        ]);
        assert_eq!(m.segments().len(), 2);
        let s1 = &m.segments()[0];
        let r1 = s1.rows(View::Damage);
        assert_eq!(
            r1.iter().find(|r| r.key == P1).unwrap().class,
            Some(Class::Warrior)
        );
        let s2 = m.segments().last().unwrap();
        let r2 = s2.rows(View::Damage);
        let p1_row = r2.iter().find(|r| r.key == P1).unwrap();
        assert_eq!(
            p1_row.class, None,
            "inference must not leak across segments"
        );
        let p2_row = r2.iter().find(|r| r.key == P2).unwrap();
        assert_eq!(p2_row.class, Some(Class::Priest));
        assert_eq!(p2_row.spec, Some(Spec::HolyPriest));
    }

    #[test]
    fn combatant_info_loadout_is_stored_and_carries_forward() {
        use wowdps_model::{GearItem, Loadout, TalentPick};
        let picks = vec![TalentPick {
            node_id: 91024,
            entry_id: 124871,
            rank: 1,
        }];
        let gear = vec![GearItem {
            item_id: 212446,
            ilvl: 639,
            enchants: vec![],
            bonus_ids: vec![6652],
            gems: vec![],
        }];
        let m = fed(vec![
            at(
                0,
                Event::CombatantInfo {
                    guid: P1.into(),
                    spec_id: Some(71),
                    faction: 0,
                    talents: picks.clone(),
                    gear: gear.clone(),
                },
            ),
            damage(1_000, p1(), None, 500),
            // >60s lull: a second segment opens; the loadout must be seeded
            // into it exactly like classes/specs.
            damage(200_000, p1(), None, 500),
        ]);
        assert_eq!(m.segments().len(), 2);
        let want = Loadout {
            spec_id: Some(71),
            talents: picks,
            gear,
        };
        assert_eq!(m.loadout(P1), Some(&want));
        for s in m.segments() {
            assert_eq!(s.loadout(P1), Some(&want), "seeded into {}", s.name);
        }
        assert_eq!(m.loadout(P2), None);
    }

    #[test]
    fn empty_brackets_do_not_wipe_an_established_loadout() {
        use wowdps_model::{GearItem, Loadout, TalentPick};
        let picks = vec![TalentPick {
            node_id: 1,
            entry_id: 2,
            rank: 1,
        }];
        let repicks = vec![TalentPick {
            node_id: 3,
            entry_id: 4,
            rank: 1,
        }];
        let gear = vec![GearItem {
            item_id: 212446,
            ilvl: 639,
            enchants: vec![],
            bonus_ids: vec![],
            gems: vec![],
        }];
        let m = fed(vec![
            at(
                0,
                Event::CombatantInfo {
                    guid: P1.into(),
                    spec_id: Some(71),
                    faction: 0,
                    talents: picks.clone(),
                    gear: gear.clone(),
                },
            ),
            damage(1_000, p1(), None, 500),
            // A re-fire truncated INSIDE the gear bracket parses full talents
            // and empty gear: the new talents land, the intact gear survives
            // (the wipe guard is per field).
            at(
                2_000,
                Event::CombatantInfo {
                    guid: P1.into(),
                    spec_id: Some(71),
                    faction: 0,
                    talents: repicks.clone(),
                    gear: vec![],
                },
            ),
            // A fully truncated re-fire carries nothing and changes nothing.
            at(
                3_000,
                Event::CombatantInfo {
                    guid: P1.into(),
                    spec_id: Some(71),
                    faction: 0,
                    talents: vec![],
                    gear: vec![],
                },
            ),
        ]);
        let want = Loadout {
            spec_id: Some(71),
            talents: repicks,
            gear,
        };
        assert_eq!(m.loadout(P1), Some(&want));
    }

    #[test]
    fn r8_inference_never_writes_into_a_closed_segment() {
        // An aura applied after ENCOUNTER_END belongs to no segment's byte
        // range; retro-coloring the closed segment would break lazy parity.
        let m = fed(vec![
            at(
                0,
                Event::EncounterStart {
                    id: 1,
                    name: "Boss".into(),
                    difficulty: 16,
                    group_size: 20,
                },
            ),
            damage(1_000, p1(), None, 500),
            at(
                2_000,
                Event::EncounterEnd {
                    id: 1,
                    name: "Boss".into(),
                    success: true,
                },
            ),
            at(
                3_000,
                Event::AuraApplied {
                    src: p1(),
                    dst: p1(),
                    spell: sp(585, "Smite"),
                    aura_type: AuraType::Buff,
                    absorb: None,
                },
            ),
        ]);
        let seg = m.segments().last().unwrap();
        let rows = seg.rows(View::Damage);
        assert_eq!(rows.iter().find(|r| r.key == P1).unwrap().class, None);
    }

    // ---- R12: comparison timelines -------------------------------------

    /// A trinket on-use, from the generated item table. Deliberately looked
    /// up rather than hard-coded twice: a regenerated table for a new patch
    /// must fail *here*, loudly, and not silently stop marking trinkets.
    const TRINKET: u32 = 1282741;
    const POTION: u32 = 1262857;

    #[test]
    fn r12_item_table_still_knows_the_spot_ids() {
        use crate::item_spells::item_kind;
        assert_eq!(item_kind(TRINKET), Some(ItemKind::Trinket));
        assert_eq!(item_kind(POTION), Some(ItemKind::Potion));
        // The trigger chase is generous on purpose: Fireball is in here
        // because some trinket procs one. `note_mark` is what keeps that off
        // a graph — see `r12_class_spells_are_never_item_markers`.
        assert!(item_kind(133).is_some());
        assert!(crate::class_spells::resolve(133).is_some());
    }

    fn cast(ts: i64, src: Unit, spell: Spell) -> LogLine {
        at(ts, Event::Cast { src, spell })
    }

    fn buff(ts: i64, dst: Unit, spell: Spell) -> LogLine {
        at(
            ts,
            Event::AuraApplied {
                src: dst.clone(),
                dst,
                spell,
                aura_type: AuraType::Buff,
                absorb: None,
            },
        )
    }

    fn unbuff(ts: i64, dst: Unit, spell: Spell) -> LogLine {
        at(
            ts,
            Event::AuraRemoved {
                src: dst.clone(),
                dst,
                spell,
                aura_type: AuraType::Buff,
                absorb: None,
            },
        )
    }

    /// R12: `compare_spells(None)` must agree with `breakdown` — same rows,
    /// same labels, same tallies — because the sparse series rides the very
    /// same `record` call.
    #[test]
    fn r12_compare_spells_full_range_matches_breakdown() {
        let m = fed(vec![
            damage(0, p1(), Some(sp(133, "Fireball")), 100),
            damage(1_500, p1(), Some(sp(116, "Frostbolt")), 40),
            damage(2_000, pet(), Some(sp(3110, "Firebolt")), 25),
            at(
                2_500,
                Event::Summon {
                    owner: p1(),
                    pet: pet(),
                },
            ),
            damage(3_000, pet(), Some(sp(3110, "Firebolt")), 25),
        ]);
        let seg = &m.segments()[0];
        let (by_spell, _) = seg.breakdown(P1, View::Damage);
        let (total, spells) = seg.compare_spells(P1, None);
        let key = |r: &Row| (r.label.clone(), r.amount, r.count, r.crits, r.spell_id);
        assert_eq!(
            spells.iter().map(key).collect::<Vec<_>>(),
            by_spell.iter().map(key).collect::<Vec<_>>()
        );
        assert_eq!(total.amount, 190, "pet folds into the owner's total");
    }

    /// R12: a window keeps only the buckets it touches, and the windowed
    /// total carries the window's own DPS.
    #[test]
    fn r12_compare_spells_windows_by_time() {
        let m = fed(vec![
            damage(0, p1(), Some(sp(133, "Fireball")), 100),
            damage(5_000, p1(), Some(sp(116, "Frostbolt")), 40),
            damage(9_500, p1(), Some(sp(133, "Fireball")), 60),
        ]);
        let seg = &m.segments()[0];
        let (total, spells) = seg.compare_spells(P1, Some((4_000, 10_000)));
        assert_eq!(
            spells
                .iter()
                .map(|r| (r.label.clone(), r.amount))
                .collect::<Vec<_>>(),
            vec![("Fireball".to_string(), 60), ("Frostbolt".to_string(), 40)]
        );
        assert_eq!(total.amount, 100);
        assert!((total.per_sec - 100.0 / 6.0).abs() < 1e-9);
        // An empty window answers empty, not a panic or NaN.
        let (t0, s0) = seg.compare_spells(P1, Some((20_000, 30_000)));
        assert!(s0.is_empty());
        assert_eq!(t0.amount, 0);
    }

    #[test]
    fn r12_buckets_damage_on_a_one_second_grid() {
        let m = fed(vec![
            damage(0, p1(), Some(sp(133, "Fireball")), 100),
            damage(500, p1(), Some(sp(133, "Fireball")), 50),
            damage(2_400, p1(), Some(sp(133, "Fireball")), 30),
        ]);
        let t = m.segments()[0].timeline(P1);
        assert_eq!(t.bucket_ms, 1_000);
        assert_eq!(t.buckets, vec![150, 0, 30]);
        assert_eq!(t.cumulative(), vec![150, 150, 180]);
    }

    /// v16: one ability's own curve, keyed exactly like its breakdown row —
    /// a plain spell by name, a pet's spell by the "spell\0pet" composite.
    #[test]
    fn v16_spell_timeline_answers_by_breakdown_key() {
        let m = fed(vec![
            at(
                0,
                Event::Summon {
                    owner: p1(),
                    pet: pet(),
                },
            ),
            damage(0, p1(), Some(sp(133, "Fireball")), 100),
            damage(2_400, p1(), Some(sp(133, "Fireball")), 30),
            damage(500, p1(), Some(sp(116, "Frostbolt")), 40),
            damage(1_000, pet(), Some(sp(3110, "Firebolt")), 25),
        ]);
        let seg = &m.segments()[0];
        // The player's own spell: only its buckets, other spells excluded.
        assert_eq!(seg.spell_timeline(P1, "Fireball").buckets, vec![100, 0, 30]);
        // The pet's spell answers by the breakdown row's composite key…
        let (by_spell, _) = seg.breakdown(P1, View::Damage);
        let pet_key = &by_spell
            .iter()
            .find(|r| r.label.contains('('))
            .expect("pet row")
            .key;
        assert_eq!(seg.spell_timeline(P1, pet_key).buckets, vec![0, 25]);
        // …and the bare pet spell name alone matches nothing.
        assert!(seg.spell_timeline(P1, "Firebolt").buckets.is_empty());
    }

    /// v17: the ability drill's target table — per-target tallies of one
    /// spell, pct of the spell's own total, wearing the spell's school.
    #[test]
    fn v17_spell_targets_split_one_ability_by_victim() {
        let mut boss2 = boss();
        boss2.guid = "Creature-0-8".into();
        boss2.name = "Adds".into();
        let m = fed(vec![
            damage(0, p1(), Some(sp(133, "Fireball")), 75),
            at(
                500,
                Event::Damage {
                    src: p1(),
                    dst: boss2,
                    spell: Some(sp(133, "Fireball")),
                    amount: 25,
                    overkill: -1,
                    absorbed: 0,
                    blocked: 0,
                    critical: true,
                    periodic: false,
                },
            ),
            damage(1_000, p1(), Some(sp(116, "Frostbolt")), 999),
        ]);
        let seg = &m.segments()[0];
        let rows = seg.spell_targets(P1, "Fireball", View::Damage);
        assert_eq!(
            rows.iter()
                .map(|r| (r.label.as_str(), r.amount, r.crits))
                .collect::<Vec<_>>(),
            vec![("Ulgrax", 75, 0), ("Adds", 25, 1)],
            "only Fireball's victims, sorted desc"
        );
        assert!((rows[0].pct - 75.0).abs() < 1e-9, "pct is of the spell");
        assert!(rows.iter().all(|r| r.school == 1), "rows wear the school");
    }

    /// v15: by-spell rows carry the spell's school bitmask; a swing (no
    /// spell block) is Physical, and meter rows stay 0.
    #[test]
    fn v15_by_spell_rows_carry_the_school() {
        let shadowflame = Spell {
            id: 603,
            name: "Doom".into(),
            school: 0x24,
        };
        let m = fed(vec![
            damage(0, p1(), Some(shadowflame), 100),
            damage(500, p1(), None, 50), // Melee
        ]);
        let seg = &m.segments()[0];
        let (by_spell, by_target) = seg.breakdown(P1, View::Damage);
        let school_of = |label: &str| by_spell.iter().find(|r| r.label == label).map(|r| r.school);
        assert_eq!(school_of("Doom"), Some(0x24));
        assert_eq!(school_of("Melee"), Some(1));
        assert!(by_target.iter().all(|r| r.school == 0));
        assert!(seg.rows(View::Damage).iter().all(|r| r.school == 0));
    }

    /// v14: healing gets its own curve — effective amounts only (R2), on the
    /// same grid, without leaking into (or reading from) the damage series.
    #[test]
    fn v14_buckets_effective_healing_on_its_own_grid() {
        let m = fed(vec![
            damage(0, p1(), Some(sp(133, "Fireball")), 100),
            heal(0, p1(), 80, 30),
            heal(2_400, p1(), 50, 0),
        ]);
        let seg = &m.segments()[0];
        // amount arrives R2-effective: 80-30=50 in bucket 0, 50 in bucket 2.
        assert_eq!(seg.heal_timeline(P1).buckets, vec![50, 0, 50]);
        // The damage curve is untouched by heals, and vice versa.
        assert_eq!(seg.timeline(P1).buckets, vec![100]);
    }

    #[test]
    fn r12_pet_damage_joins_its_owner_curve() {
        let m = fed(vec![
            at(
                0,
                Event::Summon {
                    owner: p1(),
                    pet: pet(),
                },
            ),
            damage(0, p1(), Some(sp(133, "Fireball")), 100),
            damage(0, pet(), Some(sp(3110, "Firebolt")), 40),
        ]);
        assert_eq!(m.segments()[0].timeline(P1).buckets, vec![140]);
        // The pet has no curve of its own — it was never a meter row either.
        assert!(m.segments()[0].timeline(PET).buckets.is_empty());
    }

    #[test]
    fn r12_a_cast_marks_a_use_and_its_own_buff_is_not_a_proc() {
        let m = fed(vec![
            damage(0, p1(), Some(sp(133, "Fireball")), 100),
            cast(1_000, p1(), sp(TRINKET, "Sigil")),
            // The aura the on-use applies to itself, just after the cast.
            buff(1_100, p1(), sp(TRINKET, "Sigil")),
        ]);
        let marks = m.segments()[0].timeline(P1).marks;
        assert_eq!(marks.len(), 1, "{marks:?}");
        assert_eq!(marks[0].kind, MarkKind::TrinketUse);
        assert_eq!(marks[0].at_ms, 1_000);
        assert_eq!(marks[0].label, "Sigil");
    }

    #[test]
    fn r12_an_uncast_trinket_buff_is_a_proc_deduped_while_it_refreshes() {
        let m = fed(vec![
            damage(0, p1(), Some(sp(133, "Fireball")), 100),
            buff(1_000, p1(), sp(TRINKET, "Sigil")),
            // Stack refreshes while the aura is still ON are the same proc —
            // the open span keeps running (v13).
            buff(1_200, p1(), sp(TRINKET, "Sigil")),
            buff(9_000, p1(), sp(TRINKET, "Sigil")),
            // The aura coming off closes the span; the next application is a
            // fresh, independent proc.
            unbuff(15_000, p1(), sp(TRINKET, "Sigil")),
            buff(20_000, p1(), sp(TRINKET, "Sigil")),
        ]);
        let marks = m.segments()[0].timeline(P1).marks;
        assert_eq!(marks.len(), 2, "{marks:?}");
        assert!(marks.iter().all(|m| m.kind == MarkKind::TrinketProc));
        assert_eq!(marks[0].dur_ms, 14_000, "applied 1s, removed 15s");
        assert_eq!(marks[1].at_ms, 20_000);
        assert_eq!(marks[1].dur_ms, 0, "never removed: span unknown");
    }

    /// v13: externals — Bloodlust landing on a player marks a span; the
    /// class-spells veto must not eat Power Infusion; persistent raid buffs
    /// never mark.
    #[test]
    fn v13_external_buffs_mark_spans_and_persistent_buffs_do_not() {
        let m = fed(vec![
            damage(0, p1(), Some(sp(133, "Fireball")), 100),
            buff(2_000, p1(), sp(2825, "Bloodlust")),
            unbuff(42_000, p1(), sp(2825, "Bloodlust")),
            buff(50_000, p1(), sp(10060, "Power Infusion")),
            // Persistent raid buffs are exactly what must NOT clutter the
            // graph (Arcane Intellect).
            buff(51_000, p1(), sp(1459, "Arcane Intellect")),
        ]);
        let marks = m.segments()[0].timeline(P1).marks;
        assert_eq!(marks.len(), 2, "{marks:?}");
        assert_eq!(marks[0].kind, MarkKind::External);
        assert_eq!(marks[0].label, "Bloodlust");
        assert_eq!(marks[0].dur_ms, 40_000);
        assert_eq!(marks[1].label, "Power Infusion");
        assert_eq!(marks[1].kind, MarkKind::External);
    }

    #[test]
    fn r12_consumables_count_only_when_used() {
        let m = fed(vec![
            damage(0, p1(), Some(sp(133, "Fireball")), 100),
            cast(1_000, p1(), sp(POTION, "Tempered Potion")),
            // A flask buff re-applying on its own is not a consumable event.
            buff(2_000, p1(), sp(POTION, "Tempered Potion")),
        ]);
        let marks = m.segments()[0].timeline(P1).marks;
        assert_eq!(marks.len(), 1, "{marks:?}");
        assert_eq!(marks[0].kind, MarkKind::Consumable);
    }

    #[test]
    fn r12_class_spells_are_never_item_markers() {
        // Some trinkets trigger ordinary class spells, so the generated table
        // lists them; a Fireball must still never draw a trinket bar.
        let m = fed(vec![
            damage(0, p1(), Some(sp(133, "Fireball")), 100),
            cast(1_000, p1(), sp(133, "Fireball")),
            buff(2_000, p1(), sp(133, "Fireball")),
        ]);
        assert!(m.segments()[0].timeline(P1).marks.is_empty());
    }

    #[test]
    fn r12_casts_never_open_or_extend_a_segment() {
        // A lone cast, with no combat anywhere, must not create a segment —
        // the index scanner does not see casts, and lockstep is the whole
        // basis of lazy/full parity.
        let m = fed(vec![cast(0, p1(), sp(TRINKET, "Sigil"))]);
        assert!(m.segments().is_empty());

        // And inside a segment it must not push the combat clock out.
        let m = fed(vec![
            damage(0, p1(), Some(sp(133, "Fireball")), 100),
            cast(30_000, p1(), sp(TRINKET, "Sigil")),
        ]);
        assert_eq!(m.segments()[0].duration_ms(60_000), 0);
    }

    #[test]
    fn r12_rolling_dps_smooths_over_the_window() {
        let t = Timeline {
            bucket_ms: 1_000,
            buckets: vec![0, 0, 300, 0, 0],
            marks: Vec::new(),
        };
        let dps = t.rolling_dps(3_000);
        // The spike spreads across its neighbours but the total is conserved.
        assert!(dps[2] > dps[0]);
        assert_eq!(dps.len(), 5);
        assert!((dps[2] - 100.0).abs() < 1e-9, "{dps:?}");
    }

    #[test]
    fn r8_class_spells_spot_ids_resolve() {
        use crate::class_spells::resolve;
        // Fixture/test anchor ids; the generator regenerates the table per
        // patch, but these mappings are stable client data.
        assert_eq!(resolve(585).map(|(c, _)| c), Some(Class::Priest));
        assert_eq!(resolve(19434).map(|(c, _)| c), Some(Class::Hunter));
        assert_eq!(resolve(12294).map(|(c, _)| c), Some(Class::Warrior));
        assert_eq!(resolve(163201).map(|(c, _)| c), Some(Class::Warrior));
        assert_eq!(resolve(999_999_999), None);
        // Hunter-pet Bite must NOT be in the table (pet skill lines excluded).
        assert_eq!(resolve(17253), None);
    }

    #[test]
    fn spec_to_class_maps_every_class_and_rejects_unknowns() {
        for (spec, class) in [
            (71, Class::Warrior),
            (70, Class::Paladin),
            (255, Class::Hunter),
            (260, Class::Rogue),
            (257, Class::Priest),
            (250, Class::DeathKnight),
            (262, Class::Shaman),
            (63, Class::Mage),
            (266, Class::Warlock),
            (269, Class::Monk),
            (105, Class::Druid),
            (577, Class::DemonHunter),
            (1473, Class::Evoker),
        ] {
            assert_eq!(Class::from_spec(spec), Some(class), "spec {spec}");
        }
        assert_eq!(Class::from_spec(0), None);
        assert_eq!(Class::from_spec(9999), None);
    }

    fn keyed_visit(par_ms: Option<i64>) -> Visit {
        Visit {
            map_id: 1,
            difficulty: 8,
            name: "Test".into(),
            key_level: Some(10),
            keyed: true,
            start_ms: 0,
            end_ms: None,
            completed: None,
            official_ms: None,
            pars_ms: par_ms.map(|p| (p, p * 4 / 5, p * 3 / 5)),
        }
    }

    #[test]
    fn key_verdict_is_timed_against_par_not_the_end_flag() {
        // Completed 26s over par: the game logs success=1, the verdict is OVER.
        let mut v = keyed_visit(Some(2_040_000));
        v.completed = Some(true);
        v.official_ms = Some(2_065_365);
        v.end_ms = Some(2_070_000);
        assert_eq!(v.verdict(0), Some(false));
        // At or under par: timed.
        v.official_ms = Some(2_040_000);
        assert_eq!(v.verdict(0), Some(true));
    }

    #[test]
    fn key_verdict_flips_to_over_live_when_the_timer_elapses() {
        let v = keyed_visit(Some(2_040_000));
        // 10s countdown: the clock is now - start - 10s.
        assert_eq!(v.verdict(2_050_000), None, "on par exactly: not over yet");
        assert_eq!(v.verdict(2_050_001), Some(false), "one ms past par: OVER");
    }

    #[test]
    fn key_verdict_falls_back_to_the_end_flag_without_a_par() {
        let mut v = keyed_visit(None);
        assert_eq!(v.verdict(i64::MAX), None);
        v.completed = Some(true);
        v.official_ms = Some(2_065_365);
        assert_eq!(v.verdict(0), Some(true));
    }

    #[test]
    fn abandoned_key_is_failed_regardless_of_the_clock() {
        let mut v = keyed_visit(Some(2_040_000));
        v.completed = Some(false);
        v.end_ms = Some(60_000);
        assert_eq!(v.verdict(0), Some(false));
    }

    #[test]
    fn r13_enemy_bit_only_in_arena_segments() {
        // World PvP: a hostile-flagged player fighting near us in the open
        // world (no ARENA_MATCH_START) rows up, but never as `enemy` — the
        // team divider belongs to arenas alone.
        let hostile = unit("Player-2-XXX", "Xar", 0x548);
        let m = fed(vec![
            damage(1_000, p1(), None, 400),
            at(
                2_000,
                Event::Damage {
                    src: hostile.clone(),
                    dst: p1(),
                    spell: None,
                    amount: 900,
                    overkill: -1,
                    absorbed: 0,
                    blocked: 0,
                    critical: false,
                    periodic: false,
                },
            ),
        ]);
        let seg = &m.segments()[0];
        assert!(!seg.arena);
        let rows = seg.rows(View::Damage);
        let xar = rows.iter().find(|r| r.label == "Xar").expect("Xar rows up");
        assert!(
            !xar.enemy,
            "hostile flag outside an arena must not split teams"
        );
    }

    #[test]
    fn r11_trash_counts_only_with_enemy_damage_or_a_player_death() {
        // Player damage on an enemy: a real pull.
        let m = fed(vec![damage(1_000, p1(), None, 100)]);
        assert!(m.segments()[0].counts());

        // Out-of-combat topping-off heals: live meter only, no list row.
        let m = fed(vec![heal(1_000, p1(), 500, 0)]);
        assert!(!m.segments()[0].counts());

        // NPC-vs-NPC noise near the player: never worth a row.
        let m = fed(vec![at(
            1_000,
            Event::Damage {
                src: boss(),
                dst: unit("Creature-0-2", "Guard", 0xa48),
                spell: None,
                amount: 50,
                overkill: -1,
                absorbed: 0,
                blocked: 0,
                critical: false,
                periodic: false,
            },
        )]);
        assert!(!m.segments()[0].counts());

        // Duels and world PvP count: a player damaged another player.
        let m = fed(vec![at(
            1_000,
            Event::Damage {
                src: p1(),
                dst: p2(),
                spell: None,
                amount: 500,
                overkill: -1,
                absorbed: 0,
                blocked: 0,
                critical: false,
                periodic: false,
            },
        )]);
        assert!(m.segments()[0].counts());

        // Self-damage does not (Blood DKs would make every ride count).
        let m = fed(vec![at(
            1_000,
            Event::Damage {
                src: p1(),
                dst: p1(),
                spell: None,
                amount: 500,
                overkill: -1,
                absorbed: 0,
                blocked: 0,
                critical: false,
                periodic: false,
            },
        )]);
        assert!(!m.segments()[0].counts());

        // A player death alone keeps its segment — the recap must survive
        // even when nobody hit back.
        let m = fed(vec![
            at(
                1_000,
                Event::Damage {
                    src: boss(),
                    dst: p1(),
                    spell: None,
                    amount: 9_999,
                    overkill: 100,
                    absorbed: 0,
                    blocked: 0,
                    critical: false,
                    periodic: false,
                },
            ),
            at(2_000, Event::Death { unit: p1() }),
        ]);
        assert!(m.segments()[0].counts());

        // Encounters always count.
        let m = fed(vec![start(0, "Boss"), end(10_000, "Boss", true)]);
        assert!(m.segments()[0].counts());
    }

    #[test]
    fn keystone_timer_spot_check() {
        // Magisters' Terrace (challengeID 558): 34:00 par as of the pinned
        // build — the 34:25 run this gate was born from was 26s over it.
        assert_eq!(
            crate::keystone_timers::pars_ms(558),
            Some((2_040_000, 1_632_000, 1_224_000))
        );
        assert_eq!(crate::keystone_timers::pars_ms(0), None);
    }

    // ---- R16 boss health ----------------------------------------------------

    /// A line whose advanced block reports `guid` at `current`/`max` with
    /// the given unit flags — the shape of a `_LANDED` twin.
    fn hp_report(ts: i64, guid: &str, current: u64, max: u64, flags: u32) -> LogLine {
        let mut l = at(ts, Event::Other);
        l.hp_hint = Some(HpHint {
            unit_guid: guid.into(),
            current,
            max,
            flags,
        });
        l
    }

    #[test]
    fn best_pct_grades_the_boss_not_an_add_or_a_friendly_guardian() {
        const ADD: &str = "Creature-0-1001";
        const TOTEM: &str = "Creature-0-2002";
        let m = fed(vec![
            start(0, "Boss"),
            damage(100, p1(), Some(sp(1, "Bolt")), 1_000),
            hp_report(100, BOSS, 7_000_000, 10_000_000, 0xa48),
            // An add dies: a hostile NPC at 0/max, but a tenth of the boss.
            hp_report(200, ADD, 0, 1_000_000, 0xa48),
            // A friendly guardian (a totem) dies too: a Creature guid, but
            // the reaction bit is friendly.
            hp_report(300, TOTEM, 0, 9_000_000, 0x2111),
            end(400, "Boss", false),
        ]);
        let seg = &m.segments()[0];
        assert_eq!(
            seg.best_pct(),
            Some(70),
            "the wipe stopped at the boss's 70%"
        );
    }

    #[test]
    fn best_pct_takes_the_lowest_of_a_council() {
        const TWIN: &str = "Creature-0-3134-3004-84050-261835-0000065ACC";
        let m = fed(vec![
            start(0, "Twins"),
            damage(100, p1(), Some(sp(1, "Bolt")), 1_000),
            hp_report(100, BOSS, 6_000_000, 10_000_000, 0xa48),
            // Comparable max health: a boss too. It died — progress, not the
            // grade: the pull is where the survivor stood.
            hp_report(200, TWIN, 0, 8_000_000, 0xa48),
            end(300, "Twins", false),
        ]);
        assert_eq!(m.segments()[0].best_pct(), Some(60));

        // The game parks a boss it will not let die yet at 1 HP: down, not
        // a survivor at 0 %.
        let m = fed(vec![
            start(0, "Twins"),
            damage(100, p1(), Some(sp(1, "Bolt")), 1_000),
            hp_report(100, BOSS, 770_000, 10_000_000, 0xa48),
            hp_report(200, TWIN, 1, 8_000_000, 0xa48),
            end(300, "Twins", false),
        ]);
        assert_eq!(m.segments()[0].best_pct(), Some(7));

        // Eighteen spawns of one creature id, each as big as a boss: an add
        // pack, never a council — the boss at 1 HP grades the pull 0.
        let mut lines = vec![
            start(0, "Altar"),
            damage(100, p1(), Some(sp(1, "Bolt")), 1_000),
            hp_report(100, BOSS, 1, 10_000_000, 0xa48),
        ];
        for i in 0..18u32 {
            let add = format!("Creature-0-3132-3004-21620-261218-0000{i:04X}");
            lines.push(hp_report(200 + i as i64, &add, 6_000_000, 6_000_000, 0xa48));
        }
        lines.push(end(400, "Altar", false));
        assert_eq!(fed(lines).segments()[0].best_pct(), Some(0));

        // Both down: the kill. And a kill whose last member died by script,
        // with no 0/max report, is 0 all the same — ENCOUNTER_END said so.
        let m = fed(vec![
            start(0, "Twins"),
            damage(100, p1(), Some(sp(1, "Bolt")), 1_000),
            hp_report(100, BOSS, 6_000_000, 10_000_000, 0xa48),
            hp_report(200, TWIN, 0, 8_000_000, 0xa48),
            end(300, "Twins", true),
        ]);
        assert_eq!(m.segments()[0].best_pct(), Some(0));
    }

    // ---- R17: damage taken and mitigation ---------------------------------

    /// A hit on `dst` from `src`, with the partial-mitigation fields set.
    #[allow(clippy::too_many_arguments)]
    fn hit(
        ts: i64,
        src: Unit,
        dst: Unit,
        spell: Option<Spell>,
        amount: u64,
        absorbed: u64,
        blocked: u64,
        critical: bool,
    ) -> LogLine {
        at(
            ts,
            Event::Damage {
                src,
                dst,
                spell,
                amount,
                overkill: -1,
                absorbed,
                blocked,
                critical,
                periodic: false,
            },
        )
    }

    fn miss(
        ts: i64,
        src: Unit,
        dst: Unit,
        spell: Option<Spell>,
        kind: MissKind,
        prevented: u64,
    ) -> LogLine {
        at(
            ts,
            Event::Missed {
                src,
                dst,
                spell,
                kind,
                off_hand: false,
                prevented,
            },
        )
    }

    fn row_of<'a>(rows: &'a [Row], key: &str) -> &'a Row {
        rows.iter().find(|r| r.key == key).expect("row present")
    }

    #[test]
    fn r17_a_hit_lands_on_the_victims_taken_row_and_the_attackers_damage_row() {
        let m = fed(vec![
            hit(
                1_000,
                boss(),
                p1(),
                Some(sp(7, "Cleave")),
                900,
                100,
                250,
                true,
            ),
            hit(2_000, boss(), p1(), None, 400, 0, 0, false),
        ]);
        let seg = &m.segments()[0];
        let taken = seg.rows(View::Taken);
        assert_eq!(taken.len(), 1, "one victim: {taken:?}");
        let alice = row_of(&taken, P1);
        assert_eq!(
            alice.amount, 1_400,
            "amount + absorbed, blocked NOT added (post-block)"
        );
        assert_eq!(alice.extra, 100, "extra = absorbed");
        assert_eq!(alice.count, 2);
        assert_eq!(alice.crits, 1);
        assert!(alice.per_sec > 0.0, "Taken is a rate view");

        // The identity on this one victim: the boss's Damage by_target row
        // for Alice carries exactly the same numbers.
        let (_, boss_targets) = seg.breakdown(BOSS, View::Damage);
        let dealt = row_of(&boss_targets, "Alice");
        assert_eq!((dealt.amount, dealt.count, dealt.crits), (1_400, 2, 1));

        // The drill: by ability, by ATTACKER NAME.
        let (by_spell, by_attacker) = seg.breakdown(P1, View::Taken);
        let mut spells: Vec<(String, u64, u64)> = by_spell
            .iter()
            .map(|r| (r.label.clone(), r.amount, r.count))
            .collect();
        spells.sort();
        assert_eq!(
            spells,
            vec![("Cleave".into(), 1_000, 1), ("Melee".into(), 400, 1)]
        );
        assert_eq!(by_attacker.len(), 1);
        assert_eq!(by_attacker[0].label, "Ulgrax");
        assert_eq!(by_attacker[0].amount, 1_400);

        let mit = seg.mitigation(P1).expect("something was swung at Alice");
        assert_eq!((mit.absorbed, mit.blocked), (100, 250));
        assert_eq!(mit.mitigated(), 350);
        assert!((mit.mitigated_pct(alice.amount) - 25.0).abs() < 1e-9);
        assert_eq!(seg.mitigation(P2), None, "nothing was ever swung at Bob");
        // R1 untouched: nothing lands on the boss's or Alice's Damage row.
        assert!(seg.rows(View::Damage).is_empty());
    }

    #[test]
    fn r17_a_miss_counts_once_at_zero_amount_and_by_kind() {
        let m = fed(vec![
            hit(1_000, boss(), p1(), None, 500, 0, 0, false),
            miss(1_500, boss(), p1(), None, MissKind::Dodge, 0),
            miss(
                1_600,
                boss(),
                p1(),
                Some(sp(8, "Smash")),
                MissKind::Block,
                700,
            ),
            miss(
                1_700,
                boss(),
                p1(),
                Some(sp(8, "Smash")),
                MissKind::Absorb,
                300,
            ),
        ]);
        let seg = &m.segments()[0];
        let alice = row_of(&seg.rows(View::Taken), P1).clone();
        assert_eq!(alice.amount, 500, "misses add no amount");
        assert_eq!(alice.count, 4, "but they count");
        let (by_spell, by_attacker) = seg.breakdown(P1, View::Taken);
        let melee = row_of(&by_spell, "Melee");
        assert_eq!(
            (melee.amount, melee.count),
            (500, 2),
            "the dodge sits under Melee"
        );
        let smash = row_of(&by_spell, "Smash");
        assert_eq!((smash.amount, smash.count), (0, 2));
        assert_eq!((by_attacker[0].amount, by_attacker[0].count), (500, 4));

        let mit = seg.mitigation(P1).unwrap();
        assert_eq!(mit.misses_of(MissKind::Dodge), 1);
        assert_eq!(mit.misses_of(MissKind::Block), 1);
        assert_eq!(mit.misses_of(MissKind::Absorb), 1);
        assert_eq!(mit.misses(), 3);
        assert_eq!((mit.blocked_full, mit.absorbed_full), (700, 300));
        assert_eq!(mit.mitigated(), 1_000);
        // Denominator = taken + the full-miss amounts; the dodge carries none.
        assert!((mit.mitigated_pct(alice.amount) - 1_000.0 * 100.0 / 1_500.0).abs() < 1e-9);
    }

    #[test]
    fn r17_a_player_who_was_only_dodged_has_a_taken_row() {
        let m = fed(vec![
            hit(1_000, boss(), p2(), None, 500, 0, 0, false),
            miss(1_500, boss(), p1(), None, MissKind::Dodge, 0),
        ]);
        let seg = &m.segments()[0];
        let rows = seg.rows(View::Taken);
        assert_eq!(rows.len(), 2, "{rows:?}");
        let alice = row_of(&rows, P1);
        assert_eq!((alice.amount, alice.extra, alice.count), (0, 0, 1));
        assert_eq!(rows[0].key, P2, "amount desc: Bob's 500 leads");
        // Other views keep their `amount == 0 && extra == 0` skip.
        assert!(seg.rows(View::Damage).is_empty());
    }

    #[test]
    fn r17_a_pet_hit_before_its_summon_folds_onto_the_owner() {
        let m = fed(vec![
            hit(1_000, boss(), pet(), None, 300, 50, 0, false),
            miss(1_100, boss(), pet(), None, MissKind::Parry, 0),
            at(
                2_000,
                Event::Summon {
                    owner: p1(),
                    pet: pet(),
                },
            ),
            hit(3_000, boss(), p1(), None, 1_000, 0, 0, false),
        ]);
        let seg = &m.segments()[0];
        let rows = seg.rows(View::Taken);
        assert_eq!(rows.len(), 1, "the pet folds: {rows:?}");
        assert_eq!(
            (
                rows[0].key.as_str(),
                rows[0].amount,
                rows[0].extra,
                rows[0].count
            ),
            (P1, 1_350, 50, 3)
        );
        let (by_spell, by_attacker) = seg.breakdown(P1, View::Taken);
        let mut labels: Vec<&str> = by_spell.iter().map(|r| r.label.as_str()).collect();
        labels.sort_unstable();
        assert_eq!(
            labels,
            vec!["Melee", "Melee (Felhunter)"],
            "R5 pet labelling"
        );
        assert_eq!(by_attacker.len(), 1, "one attacker name: {by_attacker:?}");
        let mit = seg.mitigation(P1).expect("folded record");
        assert_eq!(mit.absorbed, 50, "the pet's partial absorb");
        assert_eq!(mit.misses_of(MissKind::Parry), 1, "the pet's parry");
        assert_eq!(
            seg.mitigation(PET),
            None,
            "the pet resolves to its owner, never itself"
        );
    }

    #[test]
    fn r17_stagger_is_taken_once_on_the_hit_and_ticks_are_tallied_apart() {
        let monk = p1();
        let m = fed(vec![
            // The shield line comes first in real logs, R3-excluded from healing.
            hit(900, boss(), monk.clone(), None, 100, 0, 0, false),
            at(
                1_000,
                Event::Absorbed {
                    src: boss(),
                    dst: monk.clone(),
                    absorber: monk.clone(),
                    spell: None,
                    absorb_spell: sp(115069, "Stagger"),
                    amount: 400,
                },
            ),
            hit(1_000, boss(), monk.clone(), None, 600, 400, 0, false),
            // The staggered portion re-lands as a self-sourced tick.
            hit(
                1_500,
                monk.clone(),
                monk.clone(),
                Some(sp(124255, "Stagger")),
                150,
                0,
                0,
                false,
            ),
        ]);
        let seg = &m.segments()[0];
        let row = row_of(&seg.rows(View::Taken), P1).clone();
        assert_eq!(
            row.amount, 1_100,
            "the hit once, absorbed part included; the tick excluded"
        );
        assert_eq!(row.count, 2);
        assert_eq!(row.extra, 400);
        let (by_spell, _) = seg.breakdown(P1, View::Taken);
        assert!(
            by_spell.iter().all(|r| r.label != "Stagger"),
            "{by_spell:?}"
        );

        let mit = seg.mitigation(P1).unwrap();
        assert_eq!(mit.stagger, 400, "what the shield soaked");
        assert_eq!(mit.stagger_ticked, 150, "what re-landed so far");
        assert_eq!(mit.absorbed, 400);
        assert_eq!(
            mit.mitigated(),
            400,
            "stagger is inside absorbed, never added again"
        );
        assert_eq!(
            seg.rows(View::Healing).len(),
            0,
            "R2: stagger is not healing"
        );
        // R1 is not reopened: the tick still counts as damage done by the monk.
        let dealt = row_of(&seg.rows(View::Damage), P1).clone();
        assert_eq!(dealt.amount, 150);
    }

    #[test]
    fn r17_a_miss_after_encounter_end_changes_nothing_and_opens_nothing() {
        let m = fed(vec![
            start(1_000, "Ulgrax"),
            hit(2_000, boss(), p1(), None, 500, 0, 0, false),
            end(3_000, "Ulgrax", true),
            miss(4_000, boss(), p1(), None, MissKind::Dodge, 0),
            miss(4_100, boss(), p2(), None, MissKind::Block, 800),
        ]);
        assert_eq!(m.segments().len(), 1, "a miss never opens a segment");
        let seg = &m.segments()[0];
        assert_eq!(seg.end_ms, Some(3_000));
        assert_eq!(seg.last_combat_ms(), 2_000, "nor extends one");
        let rows = seg.rows(View::Taken);
        assert_eq!(rows.len(), 1);
        assert_eq!((rows[0].amount, rows[0].count), (500, 1));
        assert_eq!(seg.mitigation(P1).unwrap().misses(), 0);
        assert_eq!(seg.mitigation(P2), None);
    }

    #[test]
    fn r17_a_miss_with_no_open_segment_opens_nothing() {
        let m = fed(vec![miss(1_000, boss(), p1(), None, MissKind::Dodge, 0)]);
        assert!(m.segments().is_empty());
        // And once combat does start, the earlier miss is gone for good.
        let m = fed(vec![
            miss(1_000, boss(), p1(), None, MissKind::Dodge, 0),
            hit(2_000, boss(), p1(), None, 500, 0, 0, false),
        ]);
        assert_eq!(m.segments()[0].start_ms, 2_000);
        assert_eq!(m.segments()[0].mitigation(P1).unwrap().misses(), 0);
    }

    #[test]
    fn r17_a_miss_after_the_trash_gap_writes_nowhere_new() {
        // The open Trash segment is still "open" until the next recordable
        // line splits it; a miss must not be what splits it.
        let m = fed(vec![
            hit(1_000, boss(), p1(), None, 500, 0, 0, false),
            miss(90_000, boss(), p1(), None, MissKind::Dodge, 0),
        ]);
        assert_eq!(m.segments().len(), 1);
        assert_eq!(m.segments()[0].last_combat_ms(), 1_000);
    }

    /// The lull shape: a miss 61 s after the last hit is past the trash gap.
    /// It is not combat, so it cannot split the segment — but it must not be
    /// credited to the stale pull either (the segment closes at 1 s, before
    /// the miss), and the pull the next hit opens never saw it. It lands
    /// nowhere; mitigation on both sides is untouched.
    #[test]
    fn r17_a_miss_past_the_trash_gap_lands_in_no_segment() {
        let m = fed(vec![
            hit(1_000, boss(), p1(), None, 500, 0, 0, false),
            miss(1_500, boss(), p1(), None, MissKind::Parry, 0),
            // 61 s after the last hit: past TRASH_GAP_MS (strictly greater).
            miss(62_001, boss(), p1(), None, MissKind::Dodge, 0),
            miss(62_001, boss(), p2(), None, MissKind::Block, 800),
            hit(63_000, boss(), p1(), None, 200, 0, 0, false),
        ]);
        assert_eq!(
            m.segments().len(),
            2,
            "the hit split the trash, the misses did not"
        );
        let stale = &m.segments()[0];
        assert_eq!(stale.end_ms, Some(1_000), "closed at its last combat");
        let mit = stale.mitigation(P1).unwrap();
        assert_eq!(mit.misses(), 1, "only the in-gap parry");
        assert_eq!(mit.misses_of(MissKind::Dodge), 0);
        assert_eq!(
            stale.mitigation(P2),
            None,
            "P2 was never swung at inside the pull"
        );
        assert_eq!(row_of(&stale.rows(View::Taken), P1).count, 2, "hit + parry");
        let fresh = &m.segments()[1];
        assert_eq!(fresh.start_ms, 63_000);
        assert_eq!(fresh.mitigation(P1).unwrap().misses(), 0);
        assert_eq!(fresh.mitigation(P2), None);
        assert_eq!(row_of(&fresh.rows(View::Taken), P1).count, 1);

        // Exactly at the gap is NOT past it (`ensure_combat` splits on `>`).
        let m = fed(vec![
            hit(1_000, boss(), p1(), None, 500, 0, 0, false),
            miss(61_000, boss(), p1(), None, MissKind::Dodge, 0),
            hit(61_000, boss(), p1(), None, 200, 0, 0, false),
        ]);
        assert_eq!(m.segments().len(), 1);
        assert_eq!(m.segments()[0].mitigation(P1).unwrap().misses(), 1);

        // An Encounter never goes stale by time: only Trash gap-splits.
        let m = fed(vec![
            start(0, "Ulgrax"),
            hit(1_000, boss(), p1(), None, 500, 0, 0, false),
            miss(200_000, boss(), p1(), None, MissKind::Dodge, 0),
        ]);
        assert_eq!(m.segments()[0].mitigation(P1).unwrap().misses(), 1);
    }

    /// The pre-pull Stagger shape: the game logs the shield's SPELL_ABSORBED
    /// just BEFORE the hit it shields. When that hit is a pull's first (after
    /// an ENCOUNTER_END, or a >60 s lull), the absorb line precedes the line
    /// that opens the segment, so it belongs to no segment's byte range that
    /// a lazy load would replay — it is dropped, never credited backwards to
    /// the stale pull nor forwards to the new one. `index.rs` proves the
    /// lazy = full half of this; this is the meter-side shape.
    #[test]
    fn r17_a_stagger_absorb_before_a_pulls_first_hit_is_not_attributed() {
        let monk = p1();
        let stagger = |ts: i64, amount: u64| {
            at(
                ts,
                Event::Absorbed {
                    src: boss(),
                    dst: monk.clone(),
                    absorber: monk.clone(),
                    spell: None,
                    absorb_spell: sp(115069, "Stagger"),
                    amount,
                },
            )
        };
        // After a lull: the stale trash segment is still open.
        let m = fed(vec![
            stagger(900, 100),
            hit(900, boss(), monk.clone(), None, 500, 100, 0, false),
            stagger(62_000, 400),
            hit(62_000, boss(), monk.clone(), None, 600, 400, 0, false),
            stagger(63_000, 50),
            hit(63_000, boss(), monk.clone(), None, 100, 50, 0, false),
        ]);
        assert_eq!(m.segments().len(), 2);
        let stale = m.segments()[0].mitigation(P1).unwrap();
        assert_eq!(
            (stale.stagger, stale.absorbed),
            (0, 100),
            "the log's first line is pre-pull too: no segment was open"
        );
        let fresh = m.segments()[1].mitigation(P1).unwrap();
        assert_eq!(
            (fresh.stagger, fresh.absorbed),
            (50, 450),
            "the hit's absorbed is Taken in full; only the in-pull shield line is stagger"
        );

        // After an ENCOUNTER_END: nothing is open at all.
        let m = fed(vec![
            start(0, "Ulgrax"),
            hit(1_000, boss(), monk.clone(), None, 500, 0, 0, false),
            end(2_000, "Ulgrax", true),
            stagger(3_000, 400),
            hit(3_000, boss(), monk.clone(), None, 600, 400, 0, false),
            stagger(4_000, 50),
            hit(4_000, boss(), monk, None, 100, 50, 0, false),
        ]);
        assert_eq!(m.segments().len(), 2);
        assert_eq!(m.segments()[0].mitigation(P1).unwrap().stagger, 0);
        let trash = m.segments()[1].mitigation(P1).unwrap();
        assert_eq!((trash.stagger, trash.absorbed), (50, 450));
    }

    #[test]
    fn r17_stagger_absorb_needs_an_open_segment() {
        let monk = p1();
        let m = fed(vec![at(
            1_000,
            Event::Absorbed {
                src: boss(),
                dst: monk.clone(),
                absorber: monk.clone(),
                spell: None,
                absorb_spell: sp(115069, "Stagger"),
                amount: 400,
            },
        )]);
        assert!(m.segments().is_empty(), "R2/R17: never opens a segment");
    }

    #[test]
    fn r17_environmental_and_nil_sources_are_labelled() {
        let nil = unit("0000000000000000", "nil", 0x80000000);
        let m = fed(vec![
            hit(1_000, boss(), p1(), None, 100, 0, 0, false),
            hit(
                2_000,
                nil.clone(),
                p1(),
                Some(Spell {
                    id: 0,
                    name: "Falling".into(),
                    school: 1,
                }),
                4_000,
                0,
                0,
                false,
            ),
            miss(2_500, nil, p1(), None, MissKind::Immune, 0),
        ]);
        let seg = &m.segments()[0];
        let (by_spell, by_attacker) = seg.breakdown(P1, View::Taken);
        assert!(
            by_spell
                .iter()
                .any(|r| r.label == "Falling" && r.amount == 4_000),
            "{by_spell:?}"
        );
        let env = row_of(&by_attacker, ENVIRONMENT);
        assert_eq!((env.amount, env.count), (4_000, 2));
        assert_eq!(row_of(&seg.rows(View::Taken), P1).amount, 4_100);
    }

    #[test]
    fn r17_taken_never_lists_npcs_but_arena_enemies_wear_the_flag() {
        let enemy = unit("Player-2-XXX", "Xar", 0x548);
        let m = fed(vec![
            at(
                500,
                Event::ArenaMatchStart {
                    map_id: 1,
                    match_type: "Skirmish".into(),
                },
            ),
            hit(1_000, p1(), enemy.clone(), None, 300, 0, 0, false),
            hit(1_100, enemy.clone(), p1(), None, 200, 0, 0, false),
            hit(1_200, p1(), boss(), None, 999, 0, 0, false),
        ]);
        let rows = m.segments()[0].rows(View::Taken);
        assert_eq!(rows.len(), 2, "the boss took 999 and gets no row: {rows:?}");
        assert!(!rows[0].enemy && rows[0].key == P1, "friendly team leads");
        assert!(
            rows[1].enemy && rows[1].key == "Player-2-XXX",
            "R13 enemy bit"
        );
    }

    /// The R10 merge: an Overall's Taken rows and mitigation are the sums of
    /// its members', pets folded exactly as the members fold them.
    #[test]
    fn r17_overall_sums_members_taken_and_mitigation() {
        let m = fed(vec![
            at(
                0,
                Event::ZoneChange {
                    map_id: 2526,
                    name: "Algeth'ar Academy".into(),
                    difficulty: 8,
                },
            ),
            hit(1_000, boss(), pet(), None, 300, 30, 0, false),
            miss(1_100, boss(), p1(), None, MissKind::Parry, 0),
            start(10_000, "Crawth"),
            hit(
                11_000,
                boss(),
                p1(),
                Some(sp(7, "Cleave")),
                900,
                100,
                250,
                true,
            ),
            at(
                11_500,
                Event::Summon {
                    owner: p1(),
                    pet: pet(),
                },
            ),
            miss(12_000, boss(), p1(), None, MissKind::Block, 700),
            end(13_000, "Crawth", true),
        ]);
        assert_eq!(m.segments().len(), 2);
        let ov = m.overall(0).expect("the visit has members");
        let rows = ov.rows(View::Taken);
        assert_eq!(rows.len(), 1, "{rows:?}");
        assert_eq!(
            (rows[0].amount, rows[0].extra, rows[0].count, rows[0].crits),
            (1_330, 130, 4, 1)
        );
        let mit = ov.mitigation(P1).unwrap();
        assert_eq!(
            (mit.absorbed, mit.blocked, mit.blocked_full),
            (130, 250, 700)
        );
        assert_eq!(mit.misses_of(MissKind::Parry), 1);
        assert_eq!(mit.misses_of(MissKind::Block), 1);
        // The Trash member never saw the summon, so on its own it lists only
        // Alice's parry; the Overall's unioned owner map (R10) folds the
        // orphaned pet's 330 retroactively — raw keys are what make that work.
        let members: u64 = m
            .segments()
            .iter()
            .flat_map(|s| s.rows(View::Taken))
            .map(|r| r.amount)
            .sum();
        assert_eq!(members, 1_000);
        assert_eq!(m.segments()[0].mitigation(P1).unwrap().absorbed, 0);
        let (by_spell, by_attacker) = ov.breakdown(P1, View::Taken);
        assert_eq!(
            by_spell.len(),
            3,
            "Melee (Felhunter), Melee, Cleave: {by_spell:?}"
        );
        assert_eq!(by_attacker.len(), 1);
    }

    // ---- R19: support attribution + the R2 amendment ----------------------

    const EVOKER: &str = "Player-1-EVO";

    fn evoker() -> Unit {
        unit(EVOKER, "Vessyra", 0x511)
    }

    /// A `*_SUPPORT` share: `supporter`'s buff accounts for `amount` of a
    /// hit (or, with `healing`, a heal) `src` just landed.
    fn share(ts: i64, src: Unit, supporter: &str, amount: u64, healing: bool) -> LogLine {
        at(
            ts,
            Event::Support {
                src,
                dst: boss(),
                spell: sp(395152, "Ebon Might"),
                supporter: supporter.into(),
                amount,
                healing,
            },
        )
    }

    fn heal_on(ts: i64, src: Unit, dst: Unit, spell: Spell, amount: u64, overheal: u64) -> LogLine {
        at(
            ts,
            Event::Heal {
                src,
                dst,
                spell,
                amount,
                overheal,
                absorbed: 0,
                critical: false,
            },
        )
    }

    fn absorbed(ts: i64, absorber: Unit, dst: Unit, spell: Spell, amount: u64) -> LogLine {
        at(
            ts,
            Event::Absorbed {
                src: boss(),
                dst,
                absorber,
                spell: None,
                absorb_spell: spell,
                amount,
            },
        )
    }

    fn summon(ts: i64, owner: Unit, pet: Unit) -> LogLine {
        at(ts, Event::Summon { owner, pet })
    }

    fn damage_of(seg: &Segment, key: &str) -> u64 {
        seg.rows(View::Damage)
            .iter()
            .find(|r| r.key == key)
            .map_or(0, |r| r.amount)
    }

    /// A share lands as the buffed player's `received` and the supporter's
    /// `given`, the targets drill names the buffed player, and `effective`
    /// nets it out on one side and in on the other — Σ effective = Σ damage.
    #[test]
    fn r19_a_share_is_received_by_the_source_and_given_by_the_supporter() {
        let m = fed(vec![
            damage(1_000, p1(), Some(sp(133, "Fireball")), 1_000),
            share(1_000, p1(), EVOKER, 40, false),
            damage(2_000, evoker(), Some(sp(395160, "Eruption")), 500),
        ]);
        let seg = &m.segments()[0];
        assert_eq!(
            seg.support(P1),
            Some(Support {
                received_damage: 40,
                ..Support::default()
            })
        );
        assert_eq!(
            seg.support(EVOKER),
            Some(Support {
                given_damage: 40,
                ..Support::default()
            })
        );
        assert_eq!(seg.support(P2), None, "never named on either side");
        assert_eq!(seg.effective(P1), 960);
        assert_eq!(seg.effective(EVOKER), 540);
        assert_eq!(seg.effective(P1) + seg.effective(EVOKER), 1_500);
        assert_eq!(seg.effective(P2), 0);
        // R1 / R2 do not move: the share is not damage.
        assert_eq!(damage_of(seg, P1), 1_000);
        assert_eq!(damage_of(seg, EVOKER), 500);
        assert_eq!(seg.rows(View::Damage).len(), 2);

        let targets = seg.support_targets(EVOKER);
        assert_eq!(targets.len(), 1);
        let t = &targets[0];
        assert_eq!((t.key.as_str(), t.label.as_str()), (P1, "Alice"));
        assert_eq!((t.amount, t.extra, t.count), (40, 0, 1));
        assert!((t.pct - 100.0).abs() < 1e-9, "pct of the supporter's given");
        assert!(t.per_sec > 0.0, "a rate like a Damage drill");
        assert!(seg.support_targets(P1).is_empty());
        assert!(seg.support_targets(P2).is_empty());
    }

    /// A buffed pet's share is its owner's received (raw-keyed, folded at
    /// read, so a pet buffed before its summon still lands), the targets
    /// drill names the OWNER, and the supporter's own pet never gives:
    /// `given` is keyed on the guid the line trails with, nothing else.
    #[test]
    fn r19_a_buffed_pet_is_its_owners_received_and_a_supporters_pet_never_gives() {
        let evoker_pet = unit("Pet-0-222", "Ember", 0x1114);
        let m = fed(vec![
            damage(1_000, pet(), Some(sp(1, "Bite")), 300),
            share(1_000, pet(), EVOKER, 30, false),
            // The summon arrives AFTER the share: raw keying + read fold.
            summon(1_500, p1(), pet()),
            // The Evoker's own pet hits, buffed by its owner — received by
            // the Evoker (fold), given by the Evoker (the trailing guid).
            summon(2_000, evoker(), evoker_pet.clone()),
            damage(2_500, evoker_pet.clone(), Some(sp(2, "Flame")), 100),
            share(2_500, evoker_pet.clone(), EVOKER, 10, false),
        ]);
        let seg = &m.segments()[0];
        assert_eq!(
            seg.support(P1),
            Some(Support {
                received_damage: 30,
                ..Support::default()
            })
        );
        assert_eq!(seg.support(PET), None, "the pet folds away");
        assert_eq!(
            seg.support(EVOKER),
            Some(Support {
                given_damage: 40,
                received_damage: 10,
                ..Support::default()
            })
        );
        assert_eq!(seg.support(&evoker_pet.guid), None);
        let labels: Vec<(String, String, u64)> = seg
            .support_targets(EVOKER)
            .into_iter()
            .map(|r| (r.key, r.label, r.amount))
            .collect();
        // The pet's share was recorded before its summon, yet the drill
        // names the OWNER: the inner key is the raw buffed guid, walked to
        // its owner at read — never the pet's name, never the pet's guid.
        assert_eq!(
            labels,
            vec![
                (P1.to_string(), "Alice".to_string(), 30),
                (EVOKER.to_string(), "Vessyra".to_string(), 10),
            ]
        );
        assert_eq!(seg.effective(P1), 270);
        assert_eq!(seg.effective(EVOKER), 100 - 10 + 40);
        assert_eq!(seg.effective(P1) + seg.effective(EVOKER), 400);
    }

    /// The Evoker's own proc is logged twice — as its hit and as a share
    /// naming itself. Given and received cancel, so it is counted once.
    #[test]
    fn r19_a_self_supported_proc_is_counted_once() {
        let m = fed(vec![
            damage(1_000, evoker(), Some(sp(434481, "Bombardments")), 7_506),
            share(1_000, evoker(), EVOKER, 7_506, false),
        ]);
        let seg = &m.segments()[0];
        assert_eq!(
            seg.support(EVOKER),
            Some(Support {
                given_damage: 7_506,
                received_damage: 7_506,
                ..Support::default()
            })
        );
        assert_eq!(seg.effective(EVOKER), 7_506);
        assert_eq!(damage_of(seg, EVOKER), 7_506);
        let t = seg.support_targets(EVOKER);
        assert_eq!(
            (t.len(), t[0].key.as_str(), t[0].amount),
            (1, EVOKER, 7_506)
        );
    }

    /// Healing shares ride the same ledger on the healing side, and a
    /// player can be both — the Evoker's Fate Mirror on the healer's heal.
    #[test]
    fn r19_healing_shares_are_kept_apart_from_damage_shares() {
        let m = fed(vec![
            heal(1_000, p1(), 5_000, 500),
            share(1_000, p1(), EVOKER, 4_500, true),
            damage(2_000, p1(), None, 100),
            share(2_000, p1(), EVOKER, 3, false),
        ]);
        let seg = &m.segments()[0];
        assert_eq!(
            seg.support(P1),
            Some(Support {
                received_damage: 3,
                received_healing: 4_500,
                ..Support::default()
            })
        );
        assert_eq!(
            seg.support(EVOKER),
            Some(Support {
                given_damage: 3,
                given_healing: 4_500,
                ..Support::default()
            })
        );
        let t = seg.support_targets(EVOKER);
        assert_eq!((t[0].amount, t[0].extra, t[0].count), (3, 4_500, 2));
        // Healing shares never touch `effective`.
        assert_eq!(seg.effective(P1), 97);
        assert_eq!(seg.effective(EVOKER), 3);
        // Nor the Healing row (R2 does not move).
        assert_eq!(seg.rows(View::Healing)[0].amount, 4_500);
    }

    /// R2 amendment: healing received counts every source — a peer, an
    /// NPC, oneself (the self subset), a heal on one's pet — but never an
    /// absorb (that is the absorber's `absorbed`, a half of their Healing
    /// row) and never the NON_HEALING_ABSORBS family.
    #[test]
    fn r2_healing_received_counts_every_source_but_never_an_absorb() {
        let m = fed(vec![
            damage(500, p1(), None, 1),
            summon(600, p1(), pet()),
            heal_on(1_000, p2(), p1(), sp(2061, "Flash Heal"), 1_000, 200),
            heal_on(2_000, boss(), p1(), sp(9, "Earthen Mending"), 500, 0),
            heal_on(3_000, p1(), p1(), sp(139, "Renew"), 300, 0),
            heal_on(4_000, p2(), pet(), sp(2061, "Flash Heal"), 100, 0),
            absorbed(5_000, p2(), p1(), sp(17, "Power Word: Shield"), 250),
            absorbed(6_000, p1(), p1(), sp(115069, "Stagger"), 90),
            heal_on(7_000, p1(), p1(), sp(114556, "Purgatory"), 40, 0),
            // A heal on an NPC is nobody's received.
            heal_on(8_000, p2(), boss(), sp(2061, "Flash Heal"), 70, 0),
        ]);
        let seg = &m.segments()[0];
        assert_eq!(
            seg.healed(P1),
            Some(Healed {
                received: 800 + 500 + 300 + 100,
                self_healed: 300,
            })
        );
        assert_eq!(seg.healed(PET), None, "folded onto Alice");
        assert_eq!(seg.healed(P2), None, "Bob was never healed");
        assert_eq!(seg.healed(BOSS), None, "not a friendly");
        assert_eq!(seg.absorbed_healing(P2), 250);
        assert_eq!(seg.absorbed_healing(P1), 0, "Stagger is not healing");
        let bob = &seg.rows(View::Healing)[0];
        assert_eq!(bob.key, P2);
        assert_eq!(bob.amount, 800 + 100 + 250 + 70);
        assert!(seg.absorbed_healing(P2) <= bob.amount);
    }

    /// R19's passive gate: a share before any segment, or 61 s after the
    /// last hit (past the trash gap), lands nowhere — it never opens,
    /// extends or splits a segment; exactly at the gap it is kept; and an
    /// Encounter never goes stale by time.
    #[test]
    fn r19_a_share_past_the_trash_gap_lands_in_no_segment() {
        let m = fed(vec![
            share(0, p1(), EVOKER, 999, false),
            damage(1_000, p1(), None, 500),
            share(1_500, p1(), EVOKER, 5, false),
            share(62_001, p1(), EVOKER, 400, false),
            damage(63_000, p1(), None, 200),
            share(63_000, p1(), EVOKER, 2, false),
        ]);
        assert_eq!(
            m.segments().len(),
            2,
            "the hit split the trash, the shares did not"
        );
        let stale = &m.segments()[0];
        assert_eq!(stale.end_ms, Some(1_000), "closed at its last combat");
        assert_eq!(
            stale.last_combat_ms(),
            1_000,
            "a share never touches last_ms"
        );
        assert_eq!(stale.support(EVOKER).map(|s| s.given_damage), Some(5));
        assert_eq!(stale.support(P1).map(|s| s.received_damage), Some(5));
        let fresh = &m.segments()[1];
        assert_eq!(fresh.start_ms, 63_000);
        assert_eq!(fresh.support(EVOKER).map(|s| s.given_damage), Some(2));
        assert_eq!(fresh.support_targets(EVOKER)[0].amount, 2);

        let m = fed(vec![
            damage(1_000, p1(), None, 500),
            share(61_000, p1(), EVOKER, 7, false),
            damage(61_000, p1(), None, 200),
        ]);
        assert_eq!(m.segments().len(), 1);
        assert_eq!(
            m.segments()[0].support(EVOKER).map(|s| s.given_damage),
            Some(7)
        );

        let m = fed(vec![
            start(0, "Ulgrax"),
            damage(1_000, p1(), None, 500),
            share(200_000, p1(), EVOKER, 9, false),
        ]);
        assert_eq!(
            m.segments()[0].support(EVOKER).map(|s| s.given_damage),
            Some(9)
        );
        assert_eq!(m.segments()[0].last_combat_ms(), 1_000);
    }

    /// R2 amendment: the absorb credit is written into the segment the
    /// Healing record chose — after a gap-split, the NEW one — and the
    /// healing-received counter follows the heal the same way.
    #[test]
    fn r2_absorb_credit_and_healing_received_follow_a_gap_split() {
        let m = fed(vec![
            damage(1_000, p1(), None, 500),
            absorbed(62_001, p2(), p1(), sp(17, "Power Word: Shield"), 250),
            heal_on(62_002, p2(), p1(), sp(2061, "Flash Heal"), 100, 0),
        ]);
        assert_eq!(
            m.segments().len(),
            2,
            "an absorb is combat: it split the trash"
        );
        let stale = &m.segments()[0];
        assert_eq!(stale.absorbed_healing(P2), 0);
        assert_eq!(stale.healed(P1), None);
        let fresh = &m.segments()[1];
        assert_eq!(fresh.start_ms, 62_001);
        assert_eq!(fresh.absorbed_healing(P2), 250);
        assert_eq!(fresh.healed(P1).map(|h| h.received), Some(100));
        assert_eq!(fresh.rows(View::Healing)[0].amount, 350);
    }

    /// The R10 merge: an Overall's ledgers are the sums of its members'.
    #[test]
    fn r19_and_r2_overall_sums_members() {
        let m = fed(vec![
            at(
                0,
                Event::ZoneChange {
                    map_id: 2526,
                    name: "Algeth'ar Academy".into(),
                    difficulty: 8,
                },
            ),
            damage(1_000, p1(), None, 300),
            share(1_000, p1(), EVOKER, 30, false),
            heal_on(1_500, p2(), p1(), sp(2061, "Flash Heal"), 100, 0),
            absorbed(1_600, p2(), p1(), sp(17, "Power Word: Shield"), 20),
            start(10_000, "Crawth"),
            damage(11_000, p1(), None, 700),
            share(11_000, p1(), EVOKER, 70, false),
            damage(11_500, evoker(), None, 1_000),
            share(11_500, evoker(), EVOKER, 1_000, false),
            heal_on(12_000, p1(), p1(), sp(139, "Renew"), 50, 0),
            end(20_000, "Crawth", true),
        ]);
        assert_eq!(m.segments().len(), 2);
        let ov = m.overall(0).expect("the visit");
        assert_eq!(
            ov.support(P1),
            Some(Support {
                received_damage: 100,
                ..Support::default()
            })
        );
        assert_eq!(
            ov.support(EVOKER),
            Some(Support {
                given_damage: 1_100,
                received_damage: 1_000,
                ..Support::default()
            })
        );
        let t = ov.support_targets(EVOKER);
        assert_eq!(
            t.iter()
                .map(|r| (r.key.as_str(), r.amount, r.count))
                .collect::<Vec<_>>(),
            vec![(EVOKER, 1_000, 1), (P1, 100, 2)]
        );
        assert_eq!(
            ov.healed(P1),
            Some(Healed {
                received: 150,
                self_healed: 50,
            })
        );
        assert_eq!(ov.absorbed_healing(P2), 20);
        assert_eq!(ov.effective(P1), 900);
        assert_eq!(ov.effective(EVOKER), 1_100);
        assert_eq!(ov.effective(P1) + ov.effective(EVOKER), 2_000);
        // Members untouched by the merge.
        assert_eq!(
            m.segments()[0].support(P1).map(|s| s.received_damage),
            Some(30)
        );
        assert_eq!(m.segments()[1].absorbed_healing(P2), 0);
    }

    /// The raw ledger (before any fold) balances on the support fixture:
    /// Σ given = Σ received per segment, damage and healing apart, and the
    /// targets drill re-sums to exactly the given side.
    #[test]
    fn r19_the_raw_ledger_balances_on_the_support_fixture() {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/fixtures/support.txt");
        let text = std::fs::read_to_string(path);
        assert!(text.is_ok(), "fixtures/support.txt must exist");
        let m = meter_from_lines(text.unwrap_or_default().lines());
        let mut segments_with_shares = 0;
        for seg in m.segments() {
            let (mut gd, mut gh, mut rd, mut rh) = (0u64, 0u64, 0u64, 0u64);
            for s in seg.support.values() {
                gd += s.given_damage;
                gh += s.given_healing;
                rd += s.received_damage;
                rh += s.received_healing;
            }
            assert_eq!(
                (gd, gh),
                (rd, rh),
                "{}: raw Σ given vs Σ received",
                seg.name
            );
            let (mut td, mut th) = (0u64, 0u64);
            for targets in seg.support_targets.values() {
                for t in targets.values() {
                    td += t.damage;
                    th += t.healing;
                }
            }
            assert_eq!((td, th), (gd, gh), "{}: targets re-sum to given", seg.name);
            if gd + gh > 0 {
                segments_with_shares += 1;
                // A pet's raw entry exists and folds: the raw map has a key
                // the folded accessor answers `None` for.
                for raw in seg.support.keys().filter(|k| k.starts_with("Pet-")) {
                    assert_eq!(seg.support(raw), None, "{raw} folds onto its owner");
                    assert!(seg.support(seg.resolve_owner(raw)).is_some());
                }
            }
        }
        assert_eq!(segments_with_shares, 2, "the kill and the city pull");
    }
}
