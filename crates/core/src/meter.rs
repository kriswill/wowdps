//! Encounter segmentation and per-player aggregation.
//!
//! Accounting follows CONTRACT.md rulings R1-R6.

use std::collections::{HashMap, VecDeque};

use crate::parser::{AuraType, Event, HpHint, LogLine, Spell, Unit};

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

#[derive(Debug, Clone, Default)]
struct ViewStats {
    total: Tally,
    by_spell: HashMap<String, Tally>,
    by_target: HashMap<String, Tally>,
}

#[derive(Debug, Clone, Default)]
struct ActorStats {
    views: Vec<ViewStats>,
}

#[derive(Debug, Clone)]
pub struct Segment {
    pub kind: SegmentKind,
    pub name: String,
    pub start_ms: i64,
    /// `None` while live.
    pub end_ms: Option<i64>,
    pub success: Option<bool>,

    /// Stats keyed by the RAW acting GUID. Ownership is resolved at read time so that
    /// a pet which acted before its SPELL_SUMMON still lands on its owner's row.
    actors: HashMap<String, ActorStats>,
    owners: HashMap<String, String>,
    names: HashMap<String, String>,
    flags: HashMap<String, u32>,
    classes: HashMap<String, Class>,
    specs: HashMap<String, Spec>,
    last_ms: i64,
    /// Damage-event counts against each hostile unit, Details-style: a Trash
    /// segment is named after the enemy it fought most.
    enemies: HashMap<String, u64>,
    /// R9: per-player ring of recent damage and gains, snapshotted on death.
    recent: HashMap<String, VecDeque<RecapEntry>>,
    /// R9: each player's latest death recap.
    recaps: HashMap<String, Vec<RecapEntry>>,
    /// R9: player GUIDs in first-death order.
    death_order: Vec<String>,
}

impl Segment {
    fn new(kind: SegmentKind, name: String, start_ms: i64, seed: &Meter) -> Self {
        Self {
            kind,
            name,
            start_ms,
            end_ms: None,
            success: None,
            actors: HashMap::new(),
            // Seed with what the meter already knows so a pet summoned in an earlier
            // segment still resolves here.
            owners: seed.owners.clone(),
            names: seed.names.clone(),
            flags: seed.flags.clone(),
            classes: seed.classes.clone(),
            specs: seed.specs.clone(),
            last_ms: start_ms,
            enemies: HashMap::new(),
            recent: HashMap::new(),
            recaps: HashMap::new(),
            death_order: Vec::new(),
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
        };
        (end - self.start_ms).max(0)
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
        // Descending by amount; ties broken by label so ordering is deterministic.
        rows.sort_by(|a, b| b.amount.cmp(&a.amount).then_with(|| a.label.cmp(&b.label)));
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
        let mut spells: HashMap<String, (String, Tally)> = HashMap::new();
        let mut targets: HashMap<String, Tally> = HashMap::new();

        for actor in self.actors.keys() {
            if self.resolve_owner(actor) != player_guid {
                continue;
            }
            let Some(st) = self.stats(actor, view) else {
                continue;
            };

            // R5: a pet's spells stay visible as "{spell} ({petName})" here, while the
            // meter row above remains merged under the owner.
            let pet_name = (actor != player_guid).then(|| self.label_for(actor));
            for (spell, t) in &st.by_spell {
                let (key, label) = match &pet_name {
                    Some(pet) => (format!("{spell}\u{0}{actor}"), format!("{spell} ({pet})")),
                    None => (spell.clone(), spell.clone()),
                };
                let e = spells
                    .entry(key)
                    .or_insert_with(|| (label, Tally::default()));
                e.1.merge(t);
            }
            for (target, t) in &st.by_target {
                targets.entry(target.clone()).or_default().merge(t);
            }
        }

        let class = self.classes.get(player_guid).copied();
        let spec = self.specs.get(player_guid).copied();
        let to_rows = |m: Vec<(String, String, Tally)>| -> Vec<Row> {
            m.into_iter()
                .map(|(key, label, t)| Row {
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
                })
                .collect()
        };

        let spell_rows = to_rows(spells.into_iter().map(|(k, (l, t))| (k, l, t)).collect());
        let target_rows = to_rows(
            targets
                .into_iter()
                .map(|(k, t)| (k.clone(), k, t))
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
            })
            .collect();
        (events, self.finish_rows(attacker_rows, View::Deaths))
    }

    #[allow(clippy::too_many_arguments)]
    fn record(
        &mut self,
        actor: &str,
        view: View,
        spell: &str,
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
        let v = &mut stats.views[view.index()];
        v.total.add(amount, extra, crit);
        v.by_spell
            .entry(spell.to_string())
            .or_default()
            .add(amount, extra, crit);
        if !target.is_empty() {
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
    last_combat_ms: Option<i64>,
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
        if !u.name.is_empty() {
            self.names.insert(u.guid.clone(), u.name.clone());
        }
        if u.flags != 0 {
            self.flags.insert(u.guid.clone(), u.flags);
        }
        if let Some(s) = self.segments.last_mut() {
            if !u.name.is_empty() {
                s.names.insert(u.guid.clone(), u.name.clone());
            }
            if u.flags != 0 {
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
        self.owners.insert(unit.to_string(), owner.to_string());
        if let Some(s) = self.segments.last_mut() {
            s.owners.insert(unit.to_string(), owner.to_string());
        }
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
            let seg = Segment::new(SegmentKind::Trash, "Trash".to_string(), ts, self);
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
        target: &str,
        amount: u64,
        extra: u64,
        crit: bool,
    ) {
        self.ensure_combat(ts);
        if let Some(s) = self.segments.last_mut() {
            s.record(actor, view, spell, target, amount, extra, crit);
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
        }

        match &line.event {
            // R6: the logger restarted; accumulated state across the seam is wrong.
            Event::Version { .. } => {
                self.close(ts, None);
                self.owners.clear();
                self.last_combat_ms = None;
            }
            Event::EncounterStart { name, .. } => {
                self.close(ts, None);
                let seg = Segment::new(SegmentKind::Encounter, name.clone(), ts, self);
                self.segments.push(seg);
                self.last_combat_ms = Some(ts);
            }
            // R4: close exactly here, no DoT-tail grace window.
            Event::EncounterEnd { success, .. } => self.close(ts, Some(*success)),

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
                    &target,
                    amount + absorbed,
                    (*overkill).max(0) as u64,
                    *critical,
                );
                self.name_trash(&guid, &dst_guid, &target);
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
                self.record(ts, &guid, View::Healing, &label, &target, *amount, 0, false);
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
                self.record(ts, &guid, View::Interrupts, &label, &target, 1, 0, false);
                self.infer(src, spell);
            }

            Event::Dispel {
                src, dst, spell, ..
            } => {
                self.learn(src);
                self.learn(dst);
                let (guid, label, target) =
                    (src.guid.clone(), spell.name.clone(), dst.name.clone());
                self.record(ts, &guid, View::Dispels, &label, &target, 1, 0, false);
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
                    self.record(ts, &guid, View::CrowdControl, &label, &target, 1, 0, false);
                }
                // After the possible record: a CC aura is combat and may have
                // just gap-split; any other aura never records in either the
                // meter or the scanner, so inferring from it here is safe.
                self.infer(src, spell);
            }

            Event::Death { unit } => {
                self.learn(unit);
                if unit.is_player() {
                    let guid = unit.guid.clone();
                    self.record(ts, &guid, View::Deaths, "Death", "", 1, 0, false);
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

            Event::CombatantInfo { guid, spec_id } => {
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
            }

            Event::Other => {}
        }
    }

    /// History, oldest first; the last entry is the live/current segment.
    pub fn segments(&self) -> &[Segment] {
        &self.segments
    }

    pub fn current_index(&self) -> usize {
        self.segments.len().saturating_sub(1)
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
}
