//! v45 (R29): the replay cut — one segment's parsed lines read into a
//! [`Cut`]: its units numbered in order of first sight, where each stood
//! whenever the advanced block said (on the floor the players were posted
//! on most), the events a replay draws, what players placed in the room and
//! the raid's world markers. The history store keeps it as its REPLAY tier
//! (`proto::replay`), and the files a replay reads are written from it.
//!
//! [`cut`] takes the segment's SEED lines and its slice, as
//! `index::SegmentText` hands them out (or every line before the slice and
//! the slice — a full replay — with the same answer: the seeds carry every
//! COMBATANT_INFO, ZONE_CHANGE, CHALLENGE_MODE and WORLD_MARKER line). It
//! parses each line once (`parser::parse_line`) and reads:
//!
//! - from the seeds and the slice: each player's COMBATANT_INFO spec (the
//!   latest wins), the zone, the keystone, and the world markers standing
//!   on the instance's map when the slice starts;
//! - from the slice alone: everything else. Its first line is the cut's
//!   start (t 0: ENCOUNTER_START for a pull, the run's first line for a
//!   keystone run).
//!
//! THE FLOOR: the UiMap id the advanced block reports players at most often
//! (a tie to the lower id). Only posts on it are kept — a pull is drawn on
//! one floor — and a hit's position is its victim's post there.
//!
//! THE UNITS: numbered where first seen — a post (a player, creature or
//! vehicle the block puts on the floor), or an event row naming them —
//! line by line, the post before the line's event; the units only
//! `placed.csv` names come after all of them. A player is `player` (class
//! and spec from COMBATANT_INFO, else R8's inference from their casts); a
//! unit some line flagged hostile (`0x40`) is a `boss` when an encounter in
//! the slice names it (its title, or a name its commas and "and"s divide it
//! into), else when its health is at least 30% of the strongest hostile's
//! — an `add` otherwise; anything else is `friendly`.
//!
//! THE EVENTS (each kind's line, `model::replay::EventKind`): a player's
//! death and an NPC's (a creature's or a vehicle's `UNIT_DIED`, or its
//! `unconsciousOnDeath` going down); a resurrection on a player; a hostile
//! unit's cast begun or landed (a unit flagged hostile by now, no player's
//! or pet's), a player's interrupt of one; a player's cast begun, landed
//! or failed; a hostile spell's hit on a player (where the victim stood,
//! its amount before mitigation) or its miss; an aura from a hostile unit
//! or from no one going on a player, coming off, or its stacks changing.
//!
//! THE PLACED THINGS ([`PlacedTable`] says which spells): a player's landed
//! cast of a placing spell (where they stood) and its aim; their summon or
//! create of one and the unit it made; a telling spell reaching a unit
//! already numbered (an aura on it, or a heal on it where the heal found
//! it); and a made unit dying or destroyed.
//!
//! ## Differences from the text cutter
//!
//! The replay spike's inputs were cut by `wowdps-extract cut-pull`, which
//! read the log as text with the old Perl scripts' field split. This cut
//! reads the parser's events instead — one reading of the log — and parts
//! from it where the parser reads better, each on purpose:
//!
//! - Numbers are the parser's: a field that is no number is no number (the
//!   text cutter read Perl's prefix of a field), positions and facings are
//!   exact fixed-point integers, and a unit name is unquoted once.
//! - A unit's first sight is by parsed event in line order; the text cutter
//!   also learned names and hostility from lines the parser does not model
//!   (a melee swing's `_LANDED` twin, an aura breaking, a drain), so a unit
//!   seen ONLY on such lines is no longer named or flagged hostile by them.
//! - A player is named by the log on every line, never "Unknown"; one
//!   without a COMBATANT_INFO in the pull keeps the latest from the seeds,
//!   else R8's inference (the text cutter left both blank).
//! - A Hunter's Feign Death is no death (R9); a creature's
//!   `unconsciousOnDeath` still reads as down, as before.
//! - A hit is any hostile spell-borne damage on a player — `RANGE_DAMAGE`,
//!   a damage shield and a building's damage too, not `SPELL_DAMAGE` alone
//!   — and a miss any spell-borne miss (`RANGE_MISSED` too); environmental
//!   damage is no hit.
//! - A post needs the block's health to read (max above 0) and a creature
//!   with no pool writes no power (the parser's `None` at max 0, where the
//!   text cutter wrote `1,0,0`).
//! - Times are the parser's millisecond clock, so a pull across midnight
//!   keeps counting up (the text cutter's time of day went negative).
//! - COMBATANT_INFO is the parser's (its stat block can no longer pass for
//!   an advanced block whose map field happens to equal the floor).

use std::collections::{BTreeMap, HashMap, HashSet};

use wowdps_model::replay::{
    Cut, EncounterHead, Event, EventKind, Head, KeyHead, Marker, MarkerKind, Placed, PlacedKind,
    Post, Power, Unit, UnitId, UnitKind,
};
use wowdps_model::{Class, Spec};

use crate::meter::Meter;
use crate::parser::{self, Event as Ev, LogLine, Position, Unit as LogUnit};

/// Which spells place something in the room, and which tell where one
/// stands: the encounter rubric's generated `placed` table, which core
/// cannot name (the daemon implements this over it; a test over its own).
pub trait PlacedTable {
    /// A cast, summon or create of `spell` places something the replay draws.
    fn places(&self, spell: u32) -> bool;
    /// `spell` reaching a unit tells where a placed thing stands.
    fn tells(&self, spell: u32) -> bool;
}

/// No placed spells at all: a cut with no `placed.csv` rows.
pub struct NoPlaced;

impl PlacedTable for NoPlaced {
    fn places(&self, _: u32) -> bool {
        false
    }
    fn tells(&self, _: u32) -> bool {
        false
    }
}

/// The unit flag a hostile unit carries (REACTION_HOSTILE).
const HOSTILE: u32 = 0x40;
/// The GUID the log writes for no unit.
const NOBODY: &str = "0000000000000000";
/// A boss by health: at least this share of the strongest hostile's.
const BOSS_SHARE: f64 = 0.3;

/// The cut of one segment: its `seeds` (state from before it) and its
/// `slice` (its own lines, the first its start), parsed once. `owner` is
/// the player whose log it is ("you"); [`Cut::set_owner`] can name them
/// later.
pub fn cut<'a>(
    seeds: impl IntoIterator<Item = &'a str>,
    slice: impl IntoIterator<Item = &'a str>,
    placed: &dyn PlacedTable,
    owner: Option<&str>,
) -> Cut {
    let mut c = Cutter::default();
    for raw in seeds {
        if let Some(line) = parser::parse_line(raw) {
            c.seed(&line);
        }
    }
    for raw in slice {
        if let Some(line) = parser::parse_line(raw) {
            c.line(raw, &line, placed);
        }
    }
    c.finish(owner)
}

/// [`cut`] and the meter the same lines make (`meter::meter_from_seeded`:
/// the seeds through `Meter::seed`, the slice through `Meter::feed`), each
/// line parsed once for both — what the history store's import path runs
/// for a fight it both stores and cuts.
pub fn cut_and_meter<'a>(
    seeds: impl IntoIterator<Item = &'a str>,
    slice: impl IntoIterator<Item = &'a str>,
    placed: &dyn PlacedTable,
    owner: Option<&str>,
) -> (Meter, Cut) {
    let mut c = Cutter::default();
    let mut meter = Meter::new();
    for raw in seeds {
        if let Some(line) = parser::parse_line(raw) {
            c.seed(&line);
            meter.seed(line);
        }
    }
    for raw in slice {
        if let Some(line) = parser::parse_line(raw) {
            c.line(raw, &line, placed);
            meter.feed(line);
        }
    }
    (meter, c.finish(owner))
}

// ---- the reading ------------------------------------------------------------------

/// A unit by its place in [`Cutter::guids`]: every GUID the slice names,
/// interned once.
type Gix = u32;

/// What a line said, before the floor is known: a post, then the event it
/// may make a row of. Resolved in order once every line is read.
enum Draft {
    Post {
        t: u32,
        g: Gix,
        pos: Position,
        hp: u16,
        max_hp: u64,
        power: Option<Power>,
    },
    Event(DraftEvent),
}

/// An `events.csv` row in waiting: units as GUIDs, numbered in the order
/// the text cutter numbered them (`unit`, then `src`, then `target`).
struct DraftEvent {
    t: u32,
    kind: EventKind,
    unit: Gix,
    spell_id: u32,
    spell: String,
    /// A hit's victim, where the block placed it (kept only on the floor).
    at: Option<Position>,
    src: Option<Gix>,
    stacks: Option<u16>,
    base: Option<u64>,
    target: Option<Gix>,
}

/// A `placed.csv` row in waiting, resolved after every other row.
enum DraftPlaced {
    Cast {
        t: u32,
        src: Gix,
        spell_id: u32,
        spell: String,
        /// The caster's own block (only kept on the floor).
        at: Option<Position>,
        target: Option<Gix>,
    },
    Summon {
        t: u32,
        src: Gix,
        spell_id: u32,
        spell: String,
        made: Option<Gix>,
    },
    /// An aura put on a unit: its track places it.
    Aura {
        t: u32,
        unit: Gix,
        spell_id: u32,
        spell: String,
        src: Option<Gix>,
    },
    /// A heal on a unit, where the heal found it.
    Heal {
        t: u32,
        unit: Gix,
        spell_id: u32,
        spell: String,
        src: Option<Gix>,
        at: Position,
    },
    Gone {
        t: u32,
        unit: Gix,
    },
}

/// One interned GUID: the name the first line naming it gave, whether a
/// line flagged it hostile.
struct Seen {
    guid: String,
    name: Option<String>,
    hostile: bool,
}

#[derive(Default)]
struct Cutter {
    guids: Vec<Seen>,
    index: HashMap<String, Gix>,
    /// COMBATANT_INFO specs by player GUID, latest wins (seeds and slice).
    specs: HashMap<String, Spec>,
    /// R8's inference by player GUID, over the slice: (class, spec).
    inferred: HashMap<String, (Class, Option<Spec>)>,
    /// The zone at the start; the keystone standing (seeds).
    zone: String,
    zone_map: u32,
    key: Option<(KeyHead, u32)>,
    /// World markers standing, per instance map: marker → (x, y).
    on: BTreeMap<u32, BTreeMap<u32, (i32, i32)>>,
    /// The slice: its start (local ms) and first line's facts.
    start: Option<i64>,
    head: Head,
    /// Every encounter title the slice names (the boss rule's names).
    titles: Vec<String>,
    floor_votes: HashMap<u32, u64>,
    drafts: Vec<Draft>,
    placed: Vec<DraftPlaced>,
    markers: Vec<Marker>,
}

impl Cutter {
    fn gix(&mut self, guid: &str) -> Gix {
        if let Some(&g) = self.index.get(guid) {
            return g;
        }
        let g = self.guids.len() as Gix;
        self.guids.push(Seen {
            guid: guid.to_string(),
            name: None,
            hostile: false,
        });
        self.index.insert(guid.to_string(), g);
        g
    }

    /// The state a seed line carries: specs, the zone, the keystone, the
    /// markers. Nothing else of a seed reaches the cut.
    fn seed(&mut self, line: &LogLine) {
        match &line.event {
            Ev::CombatantInfo {
                guid,
                spec_id: Some(id),
                ..
            } => {
                if let Some(spec) = Spec::from_id(*id) {
                    self.specs.insert(guid.clone(), spec);
                }
            }
            Ev::ZoneChange { map_id, name, .. } => {
                self.zone = name.clone();
                self.zone_map = *map_id;
            }
            Ev::ChallengeModeStart {
                map_id,
                key_level,
                name,
                ..
            } => {
                self.key = Some((
                    KeyHead {
                        name: name.clone(),
                        level: *key_level,
                    },
                    *map_id,
                ));
            }
            Ev::ChallengeModeEnd { .. } => self.key = None,
            Ev::MarkerPlaced {
                map_id,
                marker,
                x,
                y,
            } => {
                self.on
                    .entry(*map_id)
                    .or_default()
                    .insert(*marker, (*x, *y));
            }
            Ev::MarkerRemoved { marker } => {
                for markers in self.on.values_mut() {
                    markers.remove(marker);
                }
            }
            _ => {}
        }
    }

    /// The slice's first line: the cut's start, its head and the markers
    /// standing on its map.
    fn begin(&mut self, raw: &str, line: &LogLine) {
        self.start = Some(line.ts_ms);
        let tz = parser::tz_offset_min(raw).map_or(0, i64::from);
        self.head.start_utc_ms = line.ts_ms - tz * 60_000;
        self.head.date = civil_date(line.ts_ms);
        match &line.event {
            Ev::EncounterStart {
                id,
                name,
                difficulty,
                group_size,
                instance_id,
            } => {
                self.head.encounter = Some(EncounterHead {
                    id: *id,
                    name: name.clone(),
                    difficulty: *difficulty,
                    size: *group_size,
                });
                self.head.map = *instance_id;
            }
            Ev::ChallengeModeStart { .. } | Ev::ZoneChange { .. } => {
                self.seed(line);
                self.head.map = self.key.as_ref().map_or(self.zone_map, |k| k.1);
            }
            _ => self.head.map = self.key.as_ref().map_or(self.zone_map, |k| k.1),
        }
        self.head.zone = self.zone.clone();
        self.head.key = self.key.as_ref().map(|k| k.0.clone());
        let map = self.head.map;
        if let Some(standing) = self.on.get(&map) {
            for (&marker, &(x, y)) in standing {
                self.markers.push(Marker {
                    t_ms: 0,
                    kind: MarkerKind::Placed,
                    marker: u8::try_from(marker).unwrap_or(u8::MAX),
                    at: Some((x, y)),
                });
            }
        }
    }

    fn line(&mut self, raw: &str, line: &LogLine, placed: &dyn PlacedTable) {
        let Some(start) = self.start else {
            self.begin(raw, line);
            if let Ev::EncounterStart { name, .. } = &line.event {
                self.titles.push(name.clone());
            }
            return;
        };
        let t = u32::try_from((line.ts_ms - start).max(0)).unwrap_or(u32::MAX);
        let ev = &line.event;
        // Names and hostility from every unit the line names.
        let (a, b) = sides(ev);
        for u in [a, b].into_iter().flatten() {
            if u.guid.contains('-') {
                let g = self.gix(&u.guid);
                if let Some(seen) = self.guids.get_mut(g as usize) {
                    if seen.name.is_none() {
                        seen.name = Some(u.name.clone());
                    }
                    if u.flags & HOSTILE != 0 {
                        seen.hostile = true;
                    }
                }
            }
        }
        self.state(t, ev);
        self.infer(ev);
        // The post: the block's unit on whatever floor; the floor is chosen
        // once every line is in.
        if let Some(h) = &line.hp_hint
            && let Some(pos) = h.pos
            && is_posted(&h.unit_guid)
        {
            if h.unit_guid.starts_with("Player-") {
                *self.floor_votes.entry(pos.map).or_default() += 1;
            }
            let g = self.gix(&h.unit_guid);
            self.drafts.push(Draft::Post {
                t,
                g,
                pos,
                hp: hp_tenths(h.current, h.max),
                max_hp: h.max,
                power: h.power.map(|p| Power {
                    kind: p.kind,
                    current: p.current,
                    max: p.max,
                }),
            });
        }
        if let Some(e) = self.event(t, line) {
            self.drafts.push(Draft::Event(e));
        }
        self.place(t, line, placed);
    }

    /// The head's end, the keystone, the encounters named, the markers.
    fn state(&mut self, t: u32, ev: &Ev) {
        match ev {
            Ev::EncounterStart { name, .. } => self.titles.push(name.clone()),
            Ev::EncounterEnd {
                id,
                success,
                duration_ms,
                ..
            } if self.head.encounter.as_ref().is_some_and(|e| e.id == *id) => {
                self.head.success = Some(*success);
                self.head.fight_ms = *duration_ms;
            }
            Ev::ChallengeModeEnd {
                success,
                total_ms,
                key_level,
                ..
            } if self.head.encounter.is_none() => {
                self.head.success = Some(*success);
                self.head.fight_ms = u64::try_from(*total_ms).ok().filter(|ms| *ms > 0);
                // A run the log joined mid-way never saw its START: its END
                // names the level (R10).
                if self.head.key.is_none() && *key_level > 0 {
                    self.head.key = Some(KeyHead {
                        name: self.head.zone.clone(),
                        level: *key_level,
                    });
                }
            }
            Ev::MarkerPlaced {
                map_id,
                marker,
                x,
                y,
            } if *map_id == self.head.map => self.markers.push(Marker {
                t_ms: t,
                kind: MarkerKind::Placed,
                marker: u8::try_from(*marker).unwrap_or(u8::MAX),
                at: Some((*x, *y)),
            }),
            Ev::MarkerRemoved { marker } => self.markers.push(Marker {
                t_ms: t,
                kind: MarkerKind::Removed,
                marker: u8::try_from(*marker).unwrap_or(u8::MAX),
                at: None,
            }),
            Ev::CombatantInfo {
                guid,
                spec_id: Some(id),
                ..
            } => {
                if let Some(spec) = Spec::from_id(*id) {
                    self.specs.insert(guid.clone(), spec);
                }
            }
            _ => {}
        }
    }

    /// R8 over the slice: a player's first class-identifying spell names
    /// their class, a spec-unique one their spec (never against the class).
    /// The meter's evidence: damage, healing, an absorb's shield, an
    /// interrupt, an aura applied, a dispel.
    fn infer(&mut self, ev: &Ev) {
        let (unit, spell) = match ev {
            Ev::Damage {
                src,
                spell: Some(spell),
                ..
            } => (src, spell),
            Ev::Heal { src, spell, .. }
            | Ev::Interrupt { src, spell, .. }
            | Ev::AuraApplied { src, spell, .. }
            | Ev::Dispel { src, spell, .. }
            | Ev::Stolen { src, spell, .. } => (src, spell),
            Ev::Absorbed {
                absorber,
                absorb_spell,
                ..
            } => (absorber, absorb_spell),
            _ => return,
        };
        if !unit.is_player() {
            return;
        }
        let Some((class, spec)) = crate::class_spells::resolve(spell.id) else {
            return;
        };
        let e = self
            .inferred
            .entry(unit.guid.clone())
            .or_insert((class, None));
        if e.1.is_none()
            && let Some(spec) = spec
            && spec.class() == e.0
        {
            e.1 = Some(spec);
        }
    }

    /// The `events.csv` row a line makes, if any.
    fn event(&mut self, t: u32, line: &LogLine) -> Option<DraftEvent> {
        let ev = &line.event;
        let row = |kind, unit, spell_id, spell: &str| DraftEvent {
            t,
            kind,
            unit,
            spell_id,
            spell: spell.to_string(),
            at: None,
            src: None,
            stacks: None,
            base: None,
            target: None,
        };
        let hostile_now = |c: &Cutter, u: &LogUnit| {
            !ours(&u.guid)
                && c.index
                    .get(&u.guid)
                    .and_then(|&g| c.guids.get(g as usize))
                    .is_some_and(|s| s.hostile)
        };
        let hostile_or_none = |c: &Cutter, u: &LogUnit| u.guid == NOBODY || hostile_now(c, u);
        Some(match ev {
            Ev::Death { unit } if is_player(unit) => {
                row(EventKind::Death, self.gix(&unit.guid), 0, "")
            }
            Ev::Death { unit } | Ev::Unconscious { unit } if is_creature(unit) => {
                row(EventKind::NpcDied, self.gix(&unit.guid), 0, "")
            }
            Ev::Resurrect { src, dst, spell } if is_player(dst) => {
                let mut r = row(EventKind::Rez, self.gix(&dst.guid), spell.id, &spell.name);
                r.src = self.src_of(src);
                r
            }
            Ev::CastStart { src, dst, spell } | Ev::Cast { src, dst, spell }
                if hostile_now(self, src) =>
            {
                let kind = if matches!(ev, Ev::CastStart { .. }) {
                    EventKind::CastStart
                } else {
                    EventKind::CastSuccess
                };
                let mut r = row(kind, self.gix(&src.guid), spell.id, &spell.name);
                r.target = self.target_of(dst);
                r
            }
            Ev::Interrupt {
                src,
                dst,
                interrupted_spell,
                ..
            } if !ours(&dst.guid) && ours(&src.guid) => {
                let mut r = row(
                    EventKind::Interrupt,
                    self.gix(&dst.guid),
                    interrupted_spell.id,
                    &interrupted_spell.name,
                );
                r.src = Some(self.gix(&src.guid));
                r
            }
            Ev::CastStart { src, dst, spell }
            | Ev::Cast { src, dst, spell }
            | Ev::CastFailed {
                src, dst, spell, ..
            } if is_player(src) => {
                let kind = match ev {
                    Ev::CastStart { .. } => EventKind::PcastStart,
                    Ev::Cast { .. } => EventKind::PcastSuccess,
                    _ => EventKind::PcastFailed,
                };
                let mut r = row(kind, self.gix(&src.guid), spell.id, &spell.name);
                r.target = self.target_of(dst);
                r
            }
            Ev::Damage {
                src,
                dst,
                spell: Some(spell),
                unmitigated,
                ..
            } if is_player(dst) && !ours(&src.guid) && spell.id != 0 => {
                // The victim's own block: only a hit placed on the floor
                // makes a row (checked once the floor is known).
                let at = line
                    .hp_hint
                    .as_ref()
                    .filter(|h| h.unit_guid == dst.guid)
                    .and_then(|h| h.pos)?;
                let mut r = row(EventKind::Hit, self.gix(&dst.guid), spell.id, &spell.name);
                r.at = Some(at);
                r.src = self.src_of(src);
                r.base = Some(*unmitigated);
                r
            }
            Ev::Missed {
                src,
                dst,
                spell: Some(spell),
                ..
            } if is_player(dst) && !ours(&src.guid) => {
                let mut r = row(EventKind::Hit, self.gix(&dst.guid), spell.id, &spell.name);
                r.src = self.src_of(src);
                r
            }
            Ev::AuraApplied {
                src, dst, spell, ..
            }
            | Ev::AuraRemoved {
                src, dst, spell, ..
            } if is_player(dst) && hostile_or_none(self, src) => {
                let kind = if matches!(ev, Ev::AuraApplied { .. }) {
                    EventKind::DebuffApplied
                } else {
                    EventKind::DebuffRemoved
                };
                let mut r = row(kind, self.gix(&dst.guid), spell.id, &spell.name);
                r.src = self.src_of(src);
                r
            }
            Ev::AuraDose {
                src,
                dst,
                spell,
                stacks,
                ..
            } if is_player(dst) && hostile_or_none(self, src) => {
                let mut r = row(
                    EventKind::DebuffDose,
                    self.gix(&dst.guid),
                    spell.id,
                    &spell.name,
                );
                r.src = self.src_of(src);
                r.stacks = Some(*stacks);
                r
            }
            _ => return None,
        })
    }

    /// A source: none for no one, else its unit.
    fn src_of(&mut self, u: &LogUnit) -> Option<Gix> {
        (u.guid != NOBODY && !u.guid.is_empty()).then(|| self.gix(&u.guid))
    }

    /// A target: none for no one or what is no GUID, else its unit.
    fn target_of(&mut self, u: &LogUnit) -> Option<Gix> {
        (u.guid != NOBODY && u.guid.contains('-')).then(|| self.gix(&u.guid))
    }

    /// The `placed.csv` row a line may make, resolved after every other.
    fn place(&mut self, t: u32, line: &LogLine, placed: &dyn PlacedTable) {
        // The line's own block, and whom it describes.
        let block = |guid: &str| {
            line.hp_hint
                .as_ref()
                .filter(|h| h.unit_guid == guid)
                .and_then(|h| h.pos)
        };
        let draft = match &line.event {
            Ev::Cast { src, dst, spell } if is_player(src) && placed.places(spell.id) => {
                DraftPlaced::Cast {
                    t,
                    src: self.gix(&src.guid),
                    spell_id: spell.id,
                    spell: spell.name.clone(),
                    at: block(&src.guid),
                    target: (dst.guid != NOBODY && dst.guid.contains('-'))
                        .then(|| self.gix(&dst.guid)),
                }
            }
            Ev::Summon {
                owner: src,
                pet: dst,
                spell,
            }
            | Ev::Create { src, dst, spell }
                if is_player(src) && placed.places(spell.id) =>
            {
                DraftPlaced::Summon {
                    t,
                    src: self.gix(&src.guid),
                    spell_id: spell.id,
                    spell: spell.name.clone(),
                    made: (dst.guid.contains('-') && dst.guid != NOBODY)
                        .then(|| self.gix(&dst.guid)),
                }
            }
            Ev::AuraApplied {
                src, dst, spell, ..
            } if placed.tells(spell.id) => DraftPlaced::Aura {
                t,
                unit: self.gix(&dst.guid),
                spell_id: spell.id,
                spell: spell.name.clone(),
                src: self.nobody_or(&src.guid),
            },
            Ev::Heal {
                src, dst, spell, ..
            } if placed.tells(spell.id) => {
                let Some(at) = block(&dst.guid) else { return };
                DraftPlaced::Heal {
                    t,
                    unit: self.gix(&dst.guid),
                    spell_id: spell.id,
                    spell: spell.name.clone(),
                    src: self.nobody_or(&src.guid),
                    at,
                }
            }
            Ev::Death { unit } | Ev::Unconscious { unit } | Ev::Destroyed { unit } => {
                DraftPlaced::Gone {
                    t,
                    unit: self.gix(&unit.guid),
                }
            }
            _ => return,
        };
        self.placed.push(draft);
    }

    fn nobody_or(&mut self, guid: &str) -> Option<Gix> {
        (guid != NOBODY && !guid.is_empty()).then(|| self.gix(guid))
    }

    /// Choose the floor, number the units and write the cut.
    fn finish(mut self, owner: Option<&str>) -> Cut {
        // The most posts; a tie to the lower id.
        let floor = self
            .floor_votes
            .iter()
            .max_by(|a, b| a.1.cmp(b.1).then_with(|| b.0.cmp(a.0)))
            .map_or(0, |(m, _)| *m);
        let mut units = Numbering::default();
        let mut posts: Vec<Vec<Post>> = Vec::new();
        let mut max_hp: HashMap<Gix, u64> = HashMap::new();
        let mut events = Vec::new();
        for d in std::mem::take(&mut self.drafts) {
            match d {
                Draft::Post {
                    t,
                    g,
                    pos,
                    hp,
                    max_hp: m,
                    power,
                } if pos.map == floor => {
                    let u = units.of(g) as usize;
                    if posts.len() <= u {
                        posts.resize_with(u + 1, Vec::new);
                    }
                    let Some(track) = posts.get_mut(u) else {
                        continue;
                    };
                    track.push(Post {
                        t_ms: t,
                        x: pos.x,
                        y: pos.y,
                        facing: pos.facing,
                        hp,
                        power,
                    });
                    let best = max_hp.entry(g).or_insert(0);
                    *best = (*best).max(m);
                }
                Draft::Post { .. } => {}
                Draft::Event(e) => {
                    // A hit makes a row only where its victim stood on the floor.
                    if e.kind == EventKind::Hit && e.at.is_some_and(|p| p.map != floor) {
                        continue;
                    }
                    let unit = units.of(e.unit);
                    let src = e.src.map(|g| units.of(g));
                    let target = e.target.map(|g| units.of(g));
                    events.push(Event {
                        t_ms: e.t,
                        kind: e.kind,
                        unit,
                        spell_id: e.spell_id,
                        spell: e.spell,
                        at: e.at.map(|p| (p.x, p.y)),
                        src,
                        stacks: e.stacks,
                        base: e.base,
                        target,
                    });
                }
            }
        }
        // The placed things, after every other row: a unit only they name
        // is numbered last.
        let mut placed_rows = Vec::new();
        let mut summoned: HashSet<Gix> = HashSet::new();
        for p in std::mem::take(&mut self.placed) {
            let on_floor = |at: Option<Position>| at.filter(|p| p.map == floor).map(|p| (p.x, p.y));
            let row = match p {
                DraftPlaced::Cast {
                    t,
                    src,
                    spell_id,
                    spell,
                    at,
                    target,
                } => Placed {
                    t_ms: t,
                    kind: PlacedKind::Cast,
                    unit: Some(units.of(src)),
                    spell_id,
                    spell,
                    at: on_floor(at),
                    src: None,
                    target: target.and_then(|g| units.get(g)),
                },
                DraftPlaced::Summon {
                    t,
                    src,
                    spell_id,
                    spell,
                    made,
                } => {
                    let unit = units.of(src);
                    let target = made.map(|g| {
                        summoned.insert(g);
                        units.of(g)
                    });
                    Placed {
                        t_ms: t,
                        kind: PlacedKind::Summon,
                        unit: Some(unit),
                        spell_id,
                        spell,
                        at: None,
                        src: None,
                        target,
                    }
                }
                DraftPlaced::Aura {
                    t,
                    unit,
                    spell_id,
                    spell,
                    src,
                } => {
                    let Some(u) = units.get(unit) else { continue };
                    Placed {
                        t_ms: t,
                        kind: PlacedKind::Touch,
                        unit: Some(u),
                        spell_id,
                        spell,
                        at: None,
                        src: src.and_then(|g| units.get(g)),
                        target: None,
                    }
                }
                DraftPlaced::Heal {
                    t,
                    unit,
                    spell_id,
                    spell,
                    src,
                    at,
                } => {
                    let Some(at) = on_floor(Some(at)) else {
                        continue;
                    };
                    Placed {
                        t_ms: t,
                        kind: PlacedKind::Touch,
                        unit: units.get(unit),
                        spell_id,
                        spell,
                        at: Some(at),
                        src: src.and_then(|g| units.get(g)),
                        target: None,
                    }
                }
                DraftPlaced::Gone { t, unit } if summoned.contains(&unit) => Placed {
                    t_ms: t,
                    kind: PlacedKind::Gone,
                    unit: Some(units.of(unit)),
                    spell_id: 0,
                    spell: String::new(),
                    at: None,
                    src: None,
                    target: None,
                },
                DraftPlaced::Gone { .. } => continue,
            };
            placed_rows.push(row);
        }
        posts.resize_with(units.order.len(), Vec::new);
        let parts: HashSet<String> = self.titles.iter().flat_map(|t| title_parts(t)).collect();
        let units = self.units(&units.order, posts, &max_hp, &parts, owner);
        Cut {
            head: self.head,
            floor,
            units,
            events,
            placed: placed_rows,
            markers: self.markers,
        }
    }

    /// Each numbered unit as `units.tsv` describes it.
    fn units(
        &self,
        order: &[Gix],
        posts: Vec<Vec<Post>>,
        max_hp: &HashMap<Gix, u64>,
        parts: &HashSet<String>,
        owner: Option<&str>,
    ) -> Vec<Unit> {
        let seen = |g: Gix| self.guids.get(g as usize);
        let hostile_npc =
            |g: Gix| seen(g).is_some_and(|s| s.hostile && !s.guid.starts_with("Player-"));
        let top = order
            .iter()
            .filter(|&&g| hostile_npc(g))
            .filter_map(|g| max_hp.get(g))
            .max()
            .copied()
            .unwrap_or(0);
        let named: HashSet<Gix> = order
            .iter()
            .copied()
            .filter(|&g| {
                hostile_npc(g)
                    && parts.contains(
                        &seen(g)
                            .and_then(|s| s.name.as_deref())
                            .unwrap_or("")
                            .to_ascii_lowercase(),
                    )
            })
            .collect();
        order
            .iter()
            .zip(posts)
            .filter_map(|(&g, posts)| {
                let s = seen(g)?;
                let name = s.name.clone().unwrap_or_else(|| "?".to_string());
                if s.guid.starts_with("Player-") {
                    let (class, spec) = match self.specs.get(&s.guid) {
                        Some(spec) => (Some(spec.class()), Some(*spec)),
                        None => self
                            .inferred
                            .get(&s.guid)
                            .map_or((None, None), |&(c, sp)| (Some(c), sp)),
                    };
                    return Some(Unit {
                        kind: UnitKind::Player,
                        name,
                        guid: s.guid.clone(),
                        class,
                        spec,
                        you: owner.is_some_and(|o| o == s.guid),
                        npc: 0,
                        posts,
                    });
                }
                let boss = if named.is_empty() {
                    max_hp.get(&g).copied().unwrap_or(0) as f64 >= BOSS_SHARE * top as f64
                } else {
                    named.contains(&g)
                };
                let kind = match (s.hostile, boss) {
                    (false, _) => UnitKind::Friendly,
                    (true, true) => UnitKind::Boss,
                    (true, false) => UnitKind::Add,
                };
                Some(Unit {
                    kind,
                    name,
                    guid: String::new(),
                    class: None,
                    spec: None,
                    you: false,
                    npc: npc_id(&s.guid).unwrap_or(0),
                    posts,
                })
            })
            .collect()
    }
}

/// Units numbered where first seen.
#[derive(Default)]
struct Numbering {
    order: Vec<Gix>,
    ids: HashMap<Gix, UnitId>,
}

impl Numbering {
    fn of(&mut self, g: Gix) -> UnitId {
        if let Some(&u) = self.ids.get(&g) {
            return u;
        }
        let u = self.order.len() as UnitId;
        self.order.push(g);
        self.ids.insert(g, u);
        u
    }

    fn get(&self, g: Gix) -> Option<UnitId> {
        self.ids.get(&g).copied()
    }
}

/// The units a line names: its source and its destination, where it has
/// them.
fn sides(ev: &Ev) -> (Option<&LogUnit>, Option<&LogUnit>) {
    match ev {
        Ev::Damage { src, dst, .. }
        | Ev::Missed { src, dst, .. }
        | Ev::Support { src, dst, .. }
        | Ev::Heal { src, dst, .. }
        | Ev::Absorbed { src, dst, .. }
        | Ev::Interrupt { src, dst, .. }
        | Ev::AuraApplied { src, dst, .. }
        | Ev::AuraRefresh { src, dst, .. }
        | Ev::AuraDose { src, dst, .. }
        | Ev::AuraRemoved { src, dst, .. }
        | Ev::Dispel { src, dst, .. }
        | Ev::Stolen { src, dst, .. }
        | Ev::Cast { src, dst, .. }
        | Ev::CastStart { src, dst, .. }
        | Ev::CastFailed { src, dst, .. }
        | Ev::Energize { src, dst, .. }
        | Ev::Create { src, dst, .. }
        | Ev::Resurrect { src, dst, .. }
        | Ev::InstaKill { src, dst, .. } => (Some(src), Some(dst)),
        Ev::Summon { owner, pet, .. } => (Some(owner), Some(pet)),
        Ev::Empower { src, .. } => (Some(src), None),
        Ev::Death { unit } | Ev::Unconscious { unit } | Ev::Destroyed { unit } => {
            (None, Some(unit))
        }
        _ => (None, None),
    }
}

/// A player's or a pet's: never a hostile caster.
fn ours(guid: &str) -> bool {
    guid.starts_with("Player-") || guid.starts_with("Pet-")
}

fn is_player(u: &LogUnit) -> bool {
    u.guid.starts_with("Player-")
}

fn is_creature(u: &LogUnit) -> bool {
    u.guid.starts_with("Creature-") || u.guid.starts_with("Vehicle-")
}

/// A unit the advanced block posts: a player, a creature, a vehicle.
fn is_posted(guid: &str) -> bool {
    guid.starts_with("Player-") || guid.starts_with("Creature-") || guid.starts_with("Vehicle-")
}

/// Health as a share of the most, in tenths of a percent, exactly as
/// `tracks.csv`'s one decimal writes it (the share formatted, then read
/// back: a tie falls where the file's formatting puts it).
fn hp_tenths(current: u64, max: u64) -> u16 {
    if max == 0 {
        return 0;
    }
    let pct = 100.0 * current as f64 / max as f64;
    let shown = format!("{pct:.1}");
    let (whole, tenth) = shown.split_once('.').unwrap_or((&shown, "0"));
    let tenths = whole
        .parse::<u64>()
        .unwrap_or(0)
        .saturating_mul(10)
        .saturating_add(tenth.parse::<u64>().unwrap_or(0));
    u16::try_from(tenths).unwrap_or(u16::MAX)
}

/// A GUID's NPC id: its sixth dash-separated field, where the four before
/// it are numbers (`Creature-0-3883-2813-74658-235597-…`).
fn npc_id(g: &str) -> Option<u32> {
    let f: Vec<&str> = g.splitn(8, '-').collect();
    let [kind, a, b, c, d, npc, _, ..] = f[..] else {
        return None;
    };
    let word = !kind.is_empty() && kind.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'_');
    let whole = |s: &str| !s.is_empty() && s.bytes().all(|c| c.is_ascii_digit());
    (word && [a, b, c, d, npc].iter().all(|s| whole(s)))
        .then(|| npc.parse().ok())
        .flatten()
}

/// A title's parts: the whole, then each name a comma or a lone "and"
/// divides it into, lowercased (ASCII only).
fn title_parts(title: &str) -> Vec<String> {
    let b = title.as_bytes();
    let at = |i: usize| b.get(i).copied();
    let word = |i: usize| at(i).is_some_and(|c| c.is_ascii_alphanumeric() || c == b'_');
    let space = |i: usize| at(i).is_some_and(|c| c.is_ascii_whitespace());
    let mut parts = vec![title.to_ascii_lowercase()];
    // The separators: a comma, or "and" standing alone, with the white
    // space round them.
    let mut cuts: Vec<(usize, usize)> = Vec::new();
    let mut i = 0;
    while i < b.len() {
        let sep = if at(i) == Some(b',') {
            Some(i + 1)
        } else if b.get(i..).is_some_and(|r| r.starts_with(b"and"))
            && (i == 0 || !word(i - 1))
            && !word(i + 3)
        {
            Some(i + 3)
        } else {
            None
        };
        match sep {
            Some(end) => {
                let mut s = i;
                while s > 0 && space(s - 1) {
                    s -= 1;
                }
                let mut e = end;
                while space(e) {
                    e += 1;
                }
                cuts.push((s, e));
                i = end;
            }
            None => i += 1,
        }
    }
    let mut pieces = Vec::new();
    let mut from = 0;
    for (s, e) in cuts {
        // A cut inside white space already taken by the last one.
        let s = s.max(from);
        pieces.push(title.get(from..s).unwrap_or(""));
        from = e.max(s);
    }
    pieces.push(title.get(from..).unwrap_or(""));
    while pieces.last() == Some(&"") {
        pieces.pop();
    }
    parts.extend(pieces.iter().map(|p| p.to_ascii_lowercase()));
    parts
}

/// The local date of a local-time epoch (ms): year, month, day. Howard
/// Hinnant's civil-from-days.
fn civil_date(local_ms: i64) -> (u16, u8, u8) {
    let z = local_ms.div_euclid(86_400_000) + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    (
        u16::try_from(y).unwrap_or(0),
        u8::try_from(m).unwrap_or(0),
        u8::try_from(d).unwrap_or(0),
    )
}

#[cfg(test)]
mod tests;
