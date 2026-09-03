//! Encounter segmentation and per-player aggregation.
//!
//! Accounting follows CONTRACT.md rulings R1-R6.

use std::collections::{HashMap, VecDeque};
use std::sync::Arc;

use crate::parser::{AuraType, Event, HpHint, LogLine, Spell, Unit};
use wowdps_model::{Encounter, ItemKind, Loadout, Mark, MarkKind, Timeline};

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

/// R12/v13: temporary EXTERNAL buffs worth a timeline marker — the Bloodlust
/// family and Power Infusion. A curated list, not a generated table: these
/// are the handful of burst externals a damage comparison hinges on, and the
/// point is precisely to EXCLUDE persistent raid buffs (Arcane Intellect,
/// Mark of the Wild), which a "temporary buff" heuristic could not.
const EXTERNAL_BUFFS: &[u32] = &[
    2825,   // Bloodlust
    32182,  // Heroism
    80353,  // Time Warp
    90355,  // Ancient Hysteria
    160452, // Netherwinds
    264667, // Primal Rage
    390386, // Fury of the Aspects
    466904, // Harrier's Cry
    10060,  // Power Infusion
];

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

    /// R16: how low the boss got, as a whole percent rounded down (0 on a
    /// kill, 100 at a pull that never scratched it). The boss is the hostile
    /// NPC with the largest max health seen while this Encounter was open;
    /// every NPC with at least half that much is a boss too (councils), and
    /// the answer is the lowest fraction any of them reached. Adds and
    /// friendly guardians dying at 0 never count. `None` off raid bosses and
    /// when no hostile health report was seen.
    pub fn best_pct(&self) -> Option<u16> {
        if self.kind != SegmentKind::Encounter || self.arena {
            return None;
        }
        let top = self.boss_hp.values().map(|b| b.peak_max).max()?;
        let (current, max) = self
            .boss_hp
            .values()
            .filter(|b| b.peak_max.saturating_mul(2) >= top)
            .map(|b| b.low)
            .min_by(|(c1, m1), (c2, m2)| {
                ((*c1 as u128) * (*m2 as u128)).cmp(&((*c2 as u128) * (*m1 as u128)))
            })?;
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
        self.last_ms = self.last_ms.max(other.last_ms);
        self.overall_ms += other.duration_ms(other.last_ms);
    }

    /// Timestamp of the last combat event recorded here — the deterministic
    /// "now" the Overall merge uses for an open member's duration (R10).
    pub fn last_combat_ms(&self) -> i64 {
        self.last_ms
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
            if st.total.amount == 0 && st.total.extra == 0 {
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

    /// The player's item markers, rebased onto the segment's start (R12) —
    /// shared by every timeline flavor.
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
            })
            .collect();
        marks.sort_by_key(|m| m.at_ms);
        marks
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
        // v13: externals are checked FIRST — Power Infusion is a priest
        // spell, so the class-spells veto below would silently eat it. Only
        // the buff landing marks (cast=false); the caster's own cast line is
        // not the buff being ON someone.
        let kind = if !cast && EXTERNAL_BUFFS.contains(&spell.id) {
            MarkKind::External
        } else {
            if crate::class_spells::resolve(spell.id).is_some() {
                return false;
            }
            let Some(item) = crate::item_spells::item_kind(spell.id) else {
                return false;
            };
            match (item, cast) {
                (ItemKind::Trinket, true) => MarkKind::TrinketUse,
                (ItemKind::Trinket, false) => MarkKind::TrinketProc,
                // Consumables only count when the player actually used one; a
                // flask's buff re-applying on a reload is not a consumable event.
                (_, true) => MarkKind::Consumable,
                (_, false) => return false,
            }
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
            Event::Cast { src, spell } => {
                if src.is_player()
                    && let Some(s) = self.segments.last_mut()
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
                self.record(
                    ts,
                    &guid,
                    View::Damage,
                    &label,
                    spell.as_ref().map_or(0, |s| s.id),
                    // v15: a swing has no spell block — it is Physical (1).
                    spell.as_ref().map_or(1, |s| s.school),
                    &target,
                    amount + absorbed,
                    (*overkill).max(0) as u64,
                    *critical,
                );
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
            } => {
                self.learn(src);
                self.learn(dst);
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
                // R12: a buff landing on a player with no cast behind it is a
                // proc. Like the health reports, this never opens or extends
                // a segment.
                if *aura_type == AuraType::Buff
                    && dst.is_player()
                    && let Some(s) = self.segments.last_mut()
                {
                    let guid = dst.guid.clone();
                    s.note_mark(&guid, spell, ts, false);
                }
                // After the possible record: a CC aura is combat and may have
                // just gap-split; any other aura never records in either the
                // meter or the scanner, so inferring from it here is safe.
                self.infer(src, spell);
            }

            // v13: the buff coming off closes the player's open marker span.
            // Like AuraApplied's marker path, this never opens or extends a
            // segment (scanner lockstep).
            Event::AuraRemoved {
                src,
                dst,
                spell,
                aura_type,
            } => {
                self.learn(src);
                self.learn(dst);
                if *aura_type == AuraType::Buff
                    && dst.is_player()
                    && let Some(s) = self.segments.last_mut()
                {
                    let guid = dst.guid.clone();
                    s.close_mark(&guid, spell.id, ts);
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
        const TWIN: &str = "Creature-0-1002";
        let m = fed(vec![
            start(0, "Twins"),
            damage(100, p1(), Some(sp(1, "Bolt")), 1_000),
            hp_report(100, BOSS, 6_000_000, 10_000_000, 0xa48),
            // Comparable max health: a boss too, and it went lower.
            hp_report(200, TWIN, 0, 8_000_000, 0xa48),
            end(300, "Twins", false),
        ]);
        assert_eq!(m.segments()[0].best_pct(), Some(0));
    }
}
