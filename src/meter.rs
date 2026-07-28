//! Encounter segmentation and per-player aggregation.
//!
//! Accounting follows CONTRACT.md rulings R1-R6.

use std::collections::HashMap;

use crate::parser::{AuraType, Event, LogLine, Unit};

/// A new Trash segment starts after this much combat silence.
const TRASH_GAP_MS: i64 = 60_000;

/// Self-absorb effects that are not healing (R2).
const NON_HEALING_ABSORBS: [u32; 4] = [114556, 31850, 31230, 115069];

/// Loss-of-control effects counted by the CrowdControl view. Exactness is not gated.
const CC_SPELLS: &[u32] = &[
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
    339, 102359, 64695, // Roots
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum View {
    Damage,
    Healing,
    Interrupts,
    CrowdControl,
    Dispels,
    Deaths,
}

impl View {
    const COUNT: usize = 6;

    fn index(self) -> usize {
        match self {
            View::Damage => 0,
            View::Healing => 1,
            View::Interrupts => 2,
            View::CrowdControl => 3,
            View::Dispels => 4,
            View::Deaths => 5,
        }
    }

    /// Count views report occurrences, not a rate.
    fn is_rate(self) -> bool {
        matches!(self, View::Damage | View::Healing)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SegmentKind {
    Encounter,
    Trash,
}

#[derive(Debug, Clone, Default)]
pub struct Row {
    /// Player GUID for meter rows; spell or target name for breakdown rows.
    pub key: String,
    pub label: String,
    /// Damage done, healing done, or an event count.
    pub amount: u64,
    /// Overheal for Healing, overkill for Damage, else 0.
    pub extra: u64,
    pub per_sec: f64,
    /// 0..100 of the view total.
    pub pct: f64,
}

#[derive(Debug, Clone, Default)]
struct Tally {
    amount: u64,
    extra: u64,
}

impl Tally {
    fn add(&mut self, amount: u64, extra: u64) {
        self.amount += amount;
        self.extra += extra;
    }
}

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
    last_ms: i64,
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
            last_ms: start_ms,
        }
    }

    pub fn duration_ms(&self, now_ms: i64) -> i64 {
        (self.end_ms.unwrap_or(now_ms) - self.start_ms).max(0)
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
            let e = merged.entry(owner).or_default();
            e.add(st.total.amount, st.total.extra);
        }

        let rows = merged
            .into_iter()
            .map(|(guid, t)| Row {
                key: guid.to_string(),
                label: self.label_for(guid),
                amount: t.amount,
                extra: t.extra,
                per_sec: 0.0,
                pct: 0.0,
            })
            .collect();
        self.finish_rows(rows, view)
    }

    /// Drilldown for one player: (by-spell rows, by-target rows).
    pub fn breakdown(&self, player_guid: &str, view: View) -> (Vec<Row>, Vec<Row>) {
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
                e.1.add(t.amount, t.extra);
            }
            for (target, t) in &st.by_target {
                targets
                    .entry(target.clone())
                    .or_default()
                    .add(t.amount, t.extra);
            }
        }

        let to_rows = |m: Vec<(String, String, Tally)>| -> Vec<Row> {
            m.into_iter()
                .map(|(key, label, t)| Row {
                    key,
                    label,
                    amount: t.amount,
                    extra: t.extra,
                    per_sec: 0.0,
                    pct: 0.0,
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

    fn record(
        &mut self,
        actor: &str,
        view: View,
        spell: &str,
        target: &str,
        amount: u64,
        extra: u64,
    ) {
        let stats = self
            .actors
            .entry(actor.to_string())
            .or_insert_with(|| ActorStats {
                views: vec![ViewStats::default(); View::COUNT],
            });
        let v = &mut stats.views[view.index()];
        v.total.add(amount, extra);
        v.by_spell
            .entry(spell.to_string())
            .or_default()
            .add(amount, extra);
        if !target.is_empty() {
            v.by_target
                .entry(target.to_string())
                .or_default()
                .add(amount, extra);
        }
    }
}

#[derive(Debug, Default)]
pub struct Meter {
    segments: Vec<Segment>,
    owners: HashMap<String, String>,
    names: HashMap<String, String>,
    flags: HashMap<String, u32>,
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
            s.last_ms = s.last_ms.max(ts);
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
    ) {
        self.ensure_combat(ts);
        if let Some(s) = self.segments.last_mut() {
            s.record(actor, view, spell, target, amount, extra);
        }
    }

    pub fn feed(&mut self, line: LogLine) {
        let ts = line.ts_ms;
        if let Some(h) = &line.owner_hint {
            let (unit, owner) = (h.unit_guid.clone(), h.owner_guid.clone());
            self.note_owner(&unit, &owner);
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
                ..
            } => {
                self.learn(src);
                self.learn(dst);
                let label = spell
                    .as_ref()
                    .map_or("Melee", |s| s.name.as_str())
                    .to_string();
                let (guid, target) = (src.guid.clone(), dst.name.clone());
                self.record(
                    ts,
                    &guid,
                    View::Damage,
                    &label,
                    &target,
                    amount + absorbed,
                    (*overkill).max(0) as u64,
                );
            }

            // R2: rows carry effective healing, with overheal in `extra`.
            Event::Heal {
                src,
                dst,
                spell,
                amount,
                overheal,
                ..
            } => {
                self.learn(src);
                self.learn(dst);
                if NON_HEALING_ABSORBS.contains(&spell.id) {
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
                );
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
                    return;
                }
                let (guid, label, target) = (
                    absorber.guid.clone(),
                    absorb_spell.name.clone(),
                    dst.name.clone(),
                );
                self.record(ts, &guid, View::Healing, &label, &target, *amount, 0);
            }

            Event::Interrupt {
                src, dst, spell, ..
            } => {
                self.learn(src);
                self.learn(dst);
                let (guid, label, target) =
                    (src.guid.clone(), spell.name.clone(), dst.name.clone());
                self.record(ts, &guid, View::Interrupts, &label, &target, 1, 0);
            }

            Event::Dispel {
                src, dst, spell, ..
            } => {
                self.learn(src);
                self.learn(dst);
                let (guid, label, target) =
                    (src.guid.clone(), spell.name.clone(), dst.name.clone());
                self.record(ts, &guid, View::Dispels, &label, &target, 1, 0);
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
                    let (guid, label, target) =
                        (src.guid.clone(), spell.name.clone(), dst.name.clone());
                    self.record(ts, &guid, View::CrowdControl, &label, &target, 1, 0);
                }
            }

            Event::Death { unit } => {
                self.learn(unit);
                if unit.is_player() {
                    let guid = unit.guid.clone();
                    self.record(ts, &guid, View::Deaths, "Death", "", 1, 0);
                }
            }

            Event::CombatantInfo { .. } | Event::Other => {}
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
        assert_eq!(m.segments()[0].name, "Trash");
        assert_eq!(m.segments()[0].rows(View::Damage)[0].amount, 500);
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
            at(1_000, Event::CombatantInfo { guid: P1.into() }),
        ]);
        assert!(m.segments().is_empty());
    }
}
