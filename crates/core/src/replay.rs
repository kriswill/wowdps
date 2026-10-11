//! v45 (R29): the replay cut — one segment's parsed lines read into a
//! [`Cut`]: its units numbered in order of first sight, where each stood
//! whenever the advanced block said (on whichever floor it said), the
//! events a replay draws, where each boss fight began and ended, what
//! players placed in the room and the raid's world markers. The history
//! store keeps it as its REPLAY tier
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
//!   keystone run), then read as every other line is.
//!
//! THE FLOORS: every post keeps the UiMap id its block reports (a
//! dungeon's level, a raid's room), so a keystone run follows the party
//! from floor to floor and a pull keeps a unit that stepped onto another
//! one. The cut's FLOOR is the one the block reports players at most often
//! (a tie to the lower id): what a boss pull is drawn on. A hit's position
//! is its victim's post at that moment, and a placed thing's its caster's
//! or its target's, on whatever floor that is.
//!
//! THE UNITS: numbered where first seen — a post (a player, creature or
//! vehicle the block puts on any floor), or an event row naming them —
//! line by line, the post before the line's event; the units only
//! `placed.csv` names come after all of them. A player is `player` (class
//! and spec from COMBATANT_INFO, else R8's inference from their casts); a
//! unit some line flagged hostile (`0x40`) is a `boss` when an encounter in
//! the slice names it (its title, or a name its commas and "and"s divide it
//! into), else when its health is at least 30% of the strongest posted
//! hostile's (with none posted, none is) — an `add` otherwise; anything
//! else is `friendly`.
//!
//! THE EVENTS (each kind's line, `model::replay::EventKind`): a player's
//! death and an NPC's (a creature's or a vehicle's `UNIT_DIED`, or its
//! `unconsciousOnDeath` going down); a resurrection on a player; a hostile
//! unit's cast begun or landed (a unit flagged hostile by now, no player's
//! or pet's), a player's interrupt of one; a player's cast begun, landed
//! or failed; a spell's hit on a player from a source not ours (where the
//! victim stood, its amount before mitigation) or its miss — ours being a
//! player or a pet, or a unit one of ours summoned (R22's fold, so Spirit
//! Link Totem's redistribution is no hit, while a neutral NPC's spell and
//! one from no unit still are); a debuff from a hostile unit or from no one
//! going on a player, coming off, or its stacks changing; every
//! ENCOUNTER_START (`boss_engaged`) and ENCOUNTER_END (`boss_killed` or
//! `boss_wiped`) the slice holds, the encounter's id and name as the row's
//! spell and no unit — a keystone run's bosses where they begin and end,
//! and a boss pull's own two (t 0 and its end); and (format 3) the auras
//! a followed unit's HUD reads:
//!
//! - a player's debuff on an NPC (a creature or a vehicle) going on, coming
//!   off or changing stacks, every one (`unit` the NPC, `src` the player):
//!   the evidence of what a player pressed, a triggered effect whose cast
//!   the log never writes included (a guardian druid's Feral Frenzy logs
//!   only its triggered spell, whose debuff on the target is such a row);
//!   a pet's, a guardian's or a totem's debuff is none (never credited to
//!   its owner);
//! - a buff on a player that matters to the fight (`buff_applied`,
//!   `buff_removed`, `buff_dose`): one whose spell R18's curated table names
//!   a defensive, a tank's mitigation, an external, a major cooldown or a
//!   healing cooldown (`role_spells`, its support buffs aside), or any buff
//!   whose source is not ours (an NPC's, or no one's: an empowerment, a
//!   mark), `src` its caster. A long buff from before the pull (stamina, a
//!   flask) is neither, so it makes no row at all.
//!
//! AN AURA UP BEFORE THE CUT (format 4) counts R18's way: a player's debuff
//! on an NPC, or a buff R18's table names, whose first sight in the slice is
//! a refresh, a dose or a removal (by unit, spell and caster) was up when it
//! began, so an applied row opens it at t 0, once per key, and the dose or
//! removal that follows is kept; a later one with nothing up is dropped. An
//! NPC's buff R18 does not name keeps a removal or dose only once applied in
//! the slice. The applied row is found at its line (its units numbered
//! there) and sorted to t 0, after the rows the first lines made.
//!
//! THE POSTS carry (format 3) the block's absorb — the unit's shields in
//! total — and, beside the pool the line names first, the last reading of
//! every pool kind the unit's posts have named so far (`Post::pools`): a
//! line names one pool, not always the same, and a cast's `a|b` lists name
//! two. Format 5 reads them as the game's own bars do: a cast's success
//! after its cost (the block reports its caster before the cast pays; the
//! parser takes field 13 off), and an energize's gain on the pool it names
//! in its suffix, which the target's block, naming another, never shows —
//! a warlock's shards, a paladin's holy power, a rogue's combo points rise
//! there and nowhere else. The gain counts in the block's units (a shard
//! energize's `1.0000` is 10 of the block's 50: `display_mod`), at most
//! the pool's max, and a gain that lost any of itself to the cap leaves the
//! pool full. Nothing between two lines is inferred.
//!
//! THE PLACED THINGS ([`PlacedTable`] says which spells): a player's landed
//! cast of a placing spell (where they stood) and its aim; their summon or
//! create of one and the unit it made; a telling spell reaching a unit
//! already numbered (an aura on it, or a heal on it where the heal found
//! it); and a made unit dying or destroyed.
//!
//! THE MARKERS: those standing on the pull's map at t 0 (from the seeds),
//! then each placement there and each removal. A marker's number is one
//! object, so placed on another map while it stands here it has left: a
//! removal.
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
//! - A hit's source is not ours: the text cutter took any source that was
//!   not a player or a pet, so a totem or guardian of ours (Spirit Link
//!   Totem's redistribution, a warlock's imps on a mind-controlled player)
//!   made a hit and numbered a unit.
//! - A world marker's number is one object: placed on one map it leaves any
//!   other (the text cutter kept a placement per map and marker, so a marker
//!   moved away and back could stand at its old place when a pull began).
//! - Every floor's posts are kept, each with its map (`tracks.csv`'s
//!   trailing `map_id`): the text cutter kept the floor's alone, dropping a
//!   unit's steps onto another floor, the hits that found it there and a
//!   placed thing's place there — and numbering a unit first seen off the
//!   floor where it was next seen instead.
//! - The slice's ENCOUNTER_START and ENCOUNTER_END lines are rows
//!   (`boss_engaged`, `boss_killed`, `boss_wiped`); the text cutter wrote
//!   none.
//! - An aura row on a player is a DEBUFF's (format 3): a buff from a hostile
//!   unit or from no one, which the text cutter wrote as a debuff row, is a
//!   buff row. Players' debuffs on NPCs, the buffs that matter to the fight,
//!   each post's absorb and every pool kind are format 3's own.
//! - A pool reads as the game's bar (format 5): a cast's success after its
//!   cost, and an energize's gain on the pool it fills (the text cutter
//!   wrote the block's numbers as they stood, a spender's before it paid).

use std::collections::{BTreeMap, HashMap, HashSet};

use wowdps_model::replay::{
    Cut, EncounterHead, Event, EventKind, Head, KeyHead, Marker, MarkerKind, Placed, PlacedKind,
    Post, Power, Unit, UnitId, UnitKind, set_pool,
};
use wowdps_model::{Class, RoleSpellKind, Spec};

use crate::meter::Meter;
use crate::parser::{self, AuraType, Event as Ev, LogLine, Position, Spell, Unit as LogUnit};

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

/// What a line said: a post, then the event it may make a row of. Resolved
/// in order once every line is read, the units numbered as they come.
enum Draft {
    Post {
        t: u32,
        g: Gix,
        pos: Position,
        hp: u16,
        max_hp: u64,
        power: Option<Power>,
        /// The block's shields in total.
        absorb: u64,
        /// The unit's last reading of every pool kind, this line's in.
        pools: Vec<Power>,
    },
    Event(DraftEvent),
}

/// Which line of an aura's life: its application, its removal, a change of
/// its stacks.
#[derive(Clone, Copy)]
enum AuraStep {
    Applied,
    Removed,
    Dose(u16),
    /// `SPELL_AURA_REFRESH`: never a row of its own, but a first sight.
    Refresh,
}

/// Which of an aura's rows the cut keeps.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Keep {
    /// Every application, removal and dose.
    Every,
    /// Every one, and an applied row at t 0 for a key first seen otherwise
    /// (R18's segment-start rule): up before the cut began.
    FromStart,
    /// A removal or dose only once the cut saw the key applied.
    OnceApplied,
}

/// An `events.csv` row in waiting: units as GUIDs, numbered in the order
/// the text cutter numbered them (`unit`, then `src`, then `target`).
struct DraftEvent {
    t: u32,
    kind: EventKind,
    /// None on a boss row alone.
    unit: Option<Gix>,
    spell_id: u32,
    spell: String,
    /// A hit's victim, where the block placed it (on whatever floor).
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
        /// The caster's own block (on whatever floor).
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
    /// Who summoned each unit (`SPELL_SUMMON`, seeds and slice; a version
    /// line resets it): R22's fold, so a totem or guardian of ours is ours.
    summoned: HashMap<String, String>,
    /// The slice: its start (local ms) and first line's facts.
    start: Option<i64>,
    head: Head,
    /// Every encounter title the slice names (the boss rule's names).
    titles: Vec<String>,
    /// Players' posts per floor: the cut's main floor is the most voted.
    floor_votes: HashMap<u32, u64>,
    /// Each posted unit's last reading of every pool kind its posts named.
    pools: HashMap<Gix, Vec<Power>>,
    /// The keyed auras the slice has seen, by (unit, spell, caster) — a
    /// buff on a player, a player's debuff on an NPC — and whether each is
    /// up (`Cutter::aura`).
    auras: HashMap<(Gix, u32, Option<Gix>), bool>,
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

    /// One of ours: a player or a pet, or a unit one of ours summoned
    /// (followed up the chain, as R22 folds it — a totem's last lines carry
    /// a neutral NPC's flags, so no flag tells it). Never the nil unit, and
    /// never by a line's flags: the log writes the nil unit with a player's
    /// (a boss debuff whose caster is gone), and a boss that turns friendly
    /// as it is beaten still ticks its debuffs on the raid.
    fn ours_now(&self, u: &LogUnit) -> bool {
        if ours(&u.guid) {
            return true;
        }
        let mut cur = u.guid.as_str();
        for _ in 0..8 {
            match self.summoned.get(cur) {
                Some(next) if next != cur => cur = next.as_str(),
                _ => break,
            }
        }
        cur != u.guid && ours(cur)
    }

    /// A marker placed: a marker's number is one object, so placed on
    /// `map` it is gone from any other map it stood on.
    fn stand_marker(&mut self, map: u32, marker: u32, at: (i32, i32)) {
        self.lift_marker(marker);
        self.on.entry(map).or_default().insert(marker, at);
    }

    /// A marker removed: off every map.
    fn lift_marker(&mut self, marker: u32) {
        for markers in self.on.values_mut() {
            markers.remove(&marker);
        }
    }

    /// A `SPELL_SUMMON`: who made the unit (never itself, never no unit).
    fn summon(&mut self, owner: &LogUnit, pet: &LogUnit) {
        if pet.guid != owner.guid && pet.guid != NOBODY && pet.guid.contains('-') {
            self.summoned.insert(pet.guid.clone(), owner.guid.clone());
        }
    }

    /// The state a seed line carries: specs, the zone, the keystone, the
    /// markers, who summoned whom. Nothing else of a seed reaches the cut.
    fn seed(&mut self, line: &LogLine) {
        match &line.event {
            Ev::Summon { owner, pet, .. } => self.summon(owner, pet),
            Ev::Version { .. } => self.summoned.clear(),
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
            } => self.stand_marker(*map_id, *marker, (*x, *y)),
            Ev::MarkerRemoved { marker } => self.lift_marker(*marker),
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
        // The slice's first line opens the cut, then reads as every line
        // does: a run that opens on a hit keeps its post and its row.
        let start = match self.start {
            Some(start) => start,
            None => {
                self.begin(raw, line);
                line.ts_ms
            }
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
        // The post: the block's unit on whatever floor, kept with it; the
        // main floor is chosen once every line is in.
        if let Some(h) = &line.hp_hint
            && let Some(pos) = h.pos
            && is_posted(&h.unit_guid)
        {
            if h.unit_guid.starts_with("Player-") {
                *self.floor_votes.entry(pos.map).or_default() += 1;
            }
            let g = self.gix(&h.unit_guid);
            // The parser reads a cast's pools after its cost (format 5).
            let reading = |p: parser::Power| Power {
                kind: p.kind,
                current: p.current,
                max: p.max,
            };
            let pools = self.pools.entry(g).or_default();
            for p in h.pools() {
                set_pool(pools, reading(p));
            }
            // An energize names its gain in the suffix, not the block, which
            // describes its target: a warlock's shards beside the mana his
            // block names advance by what the line says (format 5).
            if let Ev::Energize {
                dst,
                amount,
                over,
                power_type,
                max_power,
                ..
            } = ev
                && dst.guid == h.unit_guid
                && h.pools().all(|p| p.kind != *power_type)
            {
                energize(pools, *power_type, *amount, *over, *max_power);
            }
            let pools = pools.clone();
            self.drafts.push(Draft::Post {
                t,
                g,
                pos,
                hp: hp_tenths(h.current, h.max),
                max_hp: h.max,
                power: h.power.map(reading),
                absorb: h.absorb,
                pools,
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
            } => {
                // Placed on the pull's map: a placement. Placed elsewhere
                // while it stood here: it left (one object), a removal —
                // or the replay would keep drawing it where it was.
                let here = self.head.map;
                let stood_here = self.on.get(&here).is_some_and(|m| m.contains_key(marker));
                self.stand_marker(*map_id, *marker, (*x, *y));
                let marker = u8::try_from(*marker).unwrap_or(u8::MAX);
                if *map_id == here {
                    self.markers.push(Marker {
                        t_ms: t,
                        kind: MarkerKind::Placed,
                        marker,
                        at: Some((*x, *y)),
                    });
                } else if stood_here {
                    self.markers.push(Marker {
                        t_ms: t,
                        kind: MarkerKind::Removed,
                        marker,
                        at: None,
                    });
                }
            }
            Ev::MarkerRemoved { marker } => {
                self.lift_marker(*marker);
                self.markers.push(Marker {
                    t_ms: t,
                    kind: MarkerKind::Removed,
                    marker: u8::try_from(*marker).unwrap_or(u8::MAX),
                    at: None,
                });
            }
            Ev::CombatantInfo {
                guid,
                spec_id: Some(id),
                ..
            } => {
                if let Some(spec) = Spec::from_id(*id) {
                    self.specs.insert(guid.clone(), spec);
                }
            }
            Ev::Summon { owner, pet, .. } => self.summon(owner, pet),
            Ev::Version { .. } => self.summoned.clear(),
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
        let row = |kind, unit: Gix, spell_id, spell: &str| DraftEvent {
            t,
            kind,
            unit: Some(unit),
            spell_id,
            spell: spell.to_string(),
            at: None,
            src: None,
            stacks: None,
            base: None,
            target: None,
        };
        let hostile_now = |c: &Cutter, u: &LogUnit| c.hostile_now(u);
        // A hit's source: anything but one of ours — a neutral NPC's spell
        // that finds a player is still the room's — so a totem or guardian
        // of ours (Spirit Link Totem's redistribution) is never one.
        let foe = |c: &Cutter, u: &LogUnit| !c.ours_now(u);
        // A boss fight's start or end: the encounter as its spell, no unit.
        let boss = |kind, id: u32, name: &str| DraftEvent {
            t,
            kind,
            unit: None,
            spell_id: id,
            spell: name.to_string(),
            at: None,
            src: None,
            stacks: None,
            base: None,
            target: None,
        };
        Some(match ev {
            Ev::EncounterStart { id, name, .. } => boss(EventKind::BossEngaged, *id, name),
            Ev::EncounterEnd {
                id, name, success, ..
            } => {
                let kind = if *success {
                    EventKind::BossKilled
                } else {
                    EventKind::BossWiped
                };
                boss(kind, *id, name)
            }
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
            } if is_player(dst) && foe(self, src) && spell.id != 0 => {
                // The victim's own block: only a hit it placed makes a row,
                // on whatever floor it placed them.
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
            } if is_player(dst) && foe(self, src) => {
                let mut r = row(EventKind::Hit, self.gix(&dst.guid), spell.id, &spell.name);
                r.src = self.src_of(src);
                r
            }
            Ev::AuraApplied {
                src,
                dst,
                spell,
                aura_type,
                ..
            } => return self.aura(t, AuraStep::Applied, src, dst, spell, *aura_type),
            Ev::AuraRemoved {
                src,
                dst,
                spell,
                aura_type,
                ..
            } => return self.aura(t, AuraStep::Removed, src, dst, spell, *aura_type),
            Ev::AuraDose {
                src,
                dst,
                spell,
                aura_type,
                stacks,
                ..
            } => return self.aura(t, AuraStep::Dose(*stacks), src, dst, spell, *aura_type),
            Ev::AuraRefresh {
                src,
                dst,
                spell,
                aura_type,
                ..
            } => return self.aura(t, AuraStep::Refresh, src, dst, spell, *aura_type),
            _ => return None,
        })
    }

    /// A unit some line flagged hostile by now, never one of ours.
    fn hostile_now(&self, u: &LogUnit) -> bool {
        !ours(&u.guid)
            && self
                .index
                .get(&u.guid)
                .and_then(|&g| self.guids.get(g as usize))
                .is_some_and(|s| s.hostile)
    }

    /// The row an aura's line makes, if any, and (format 4) the applied row
    /// at t 0 it may open first:
    ///
    /// - a debuff on a player from a hostile unit or from no one: every row;
    /// - a player's debuff on an NPC, and a buff on a player whose spell
    ///   R18's curated table names (its support buffs aside): every row,
    ///   and R18's segment-start rule — a refresh, dose or removal of a key
    ///   (unit, spell, caster) the cut has not seen yet was up before it
    ///   began, so an applied row opens it at t 0, once per key;
    /// - any other buff on a player whose source is not ours (an NPC's, no
    ///   one's): its removal or dose only after the cut saw it applied.
    fn aura(
        &mut self,
        t: u32,
        step: AuraStep,
        src: &LogUnit,
        dst: &LogUnit,
        spell: &Spell,
        aura_type: AuraType,
    ) -> Option<DraftEvent> {
        let (kinds, keep) = if aura_type == AuraType::Buff {
            if !is_player(dst) {
                return None;
            }
            // A defensive, a tank's mitigation, an external, a major or a
            // healing cooldown: R18's curated table, its support buffs aside.
            let fights = crate::role_spells::role_kind(spell.id)
                .is_some_and(|k| k != RoleSpellKind::SupportBuff);
            let kinds = [
                EventKind::BuffApplied,
                EventKind::BuffRemoved,
                EventKind::BuffDose,
            ];
            if fights {
                (kinds, Keep::FromStart)
            } else if !self.ours_now(src) {
                (kinds, Keep::OnceApplied)
            } else {
                return None;
            }
        } else {
            let kinds = [
                EventKind::DebuffApplied,
                EventKind::DebuffRemoved,
                EventKind::DebuffDose,
            ];
            if is_player(dst) && (src.guid == NOBODY || self.hostile_now(src)) {
                (kinds, Keep::Every)
            } else if is_creature(dst) && is_player(src) {
                (kinds, Keep::FromStart)
            } else {
                return None;
            }
        };
        let [applied, removed, dose] = kinds;
        let row = |c: &mut Cutter, t, kind, stacks| DraftEvent {
            t,
            kind,
            unit: Some(c.gix(&dst.guid)),
            spell_id: spell.id,
            spell: spell.name.clone(),
            at: None,
            src: c.src_of(src),
            stacks,
            base: None,
            target: None,
        };
        let (kind, stacks) = match step {
            AuraStep::Applied => (applied, None),
            AuraStep::Removed => (removed, None),
            AuraStep::Dose(n) => (dose, Some(n)),
            AuraStep::Refresh => (applied, None),
        };
        if keep == Keep::Every {
            return (!matches!(step, AuraStep::Refresh)).then(|| row(self, t, kind, stacks));
        }
        // Keyed: whether the cut has seen this aura, and whether it is up.
        let key = (self.gix(&dst.guid), spell.id, self.src_of(src));
        let up = match (self.auras.get(&key).copied(), step) {
            (_, AuraStep::Applied) => {
                self.auras.insert(key, true);
                return Some(row(self, t, kind, stacks));
            }
            // First seen otherwise: up before the cut began. R18 opens it at
            // the start; a buff R18 does not name stays out.
            (None, _) if keep == Keep::FromStart => {
                let opened = row(self, 0, applied, None);
                self.drafts.push(Draft::Event(opened));
                true
            }
            (seen, _) => seen.unwrap_or(false),
        };
        match step {
            AuraStep::Refresh => {
                self.auras.insert(key, up);
                None
            }
            _ if !up => None,
            AuraStep::Removed => {
                self.auras.insert(key, false);
                Some(row(self, t, kind, stacks))
            }
            _ => {
                self.auras.insert(key, true);
                Some(row(self, t, kind, stacks))
            }
        }
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

    /// Choose the main floor, number the units and write the cut.
    fn finish(mut self, owner: Option<&str>) -> Cut {
        // The most players' posts; a tie to the lower id.
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
                    absorb,
                    pools,
                } => {
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
                        map_id: pos.map,
                        absorb,
                        pools,
                    });
                    let best = max_hp.entry(g).or_insert(0);
                    *best = (*best).max(m);
                }
                Draft::Event(e) => {
                    let unit = e.unit.map(|g| units.of(g));
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
            let xy = |at: Option<Position>| at.map(|p| (p.x, p.y));
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
                    at: xy(at),
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
                } => Placed {
                    t_ms: t,
                    kind: PlacedKind::Touch,
                    unit: units.get(unit),
                    spell_id,
                    spell,
                    at: Some((at.x, at.y)),
                    src: src.and_then(|g| units.get(g)),
                    target: None,
                },
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
        // In time order: an applied row opened at t 0 (R18's segment-start
        // rule) is found at a later line, numbering its units there, and moves
        // to the start after the rows the first lines made; a stable sort
        // keeps the log's order otherwise.
        events.sort_by_key(|e: &Event| e.t_ms);
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
                // With no hostile posted (no health to weigh), none is a boss
                // by health: every hostile is an add.
                let boss = if named.is_empty() {
                    top > 0
                        && max_hp.get(&g).copied().unwrap_or(0) as f64 >= BOSS_SHARE * top as f64
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

/// How many of the block's units make one of an energize's (format 5): the
/// block and the suffix's max count rage, runic power, soul shards and
/// astral power in tenths and insanity in hundredths, while the gain is
/// written as the game's bar shows it (a warlock's `1.0000` shard is 10 of
/// the block's 50; a druid's 25 rage, 250 of 1000). Every other pool counts
/// alike in both — mana, energy, focus, fury, holy power, combo points,
/// chi, runes, arcane charges, essence, maelstrom.
fn display_mod(kind: u32) -> f64 {
    match kind {
        1 | 6 | 7 | 8 => 10.0,
        13 => 100.0,
        _ => 1.0,
    }
}

/// An energize's gain on the pool it fills, the line's block naming
/// another (format 5): the unit's last reading of that kind plus the gain
/// in the block's units, at most the pool's max — the suffix's, else the
/// last reading's; a gain with no reading before it starts from 0. A gain
/// that lost any of itself to the cap (`over` above 0) left the pool full.
/// No max known, no reading. Only what the line says: nothing between two
/// lines (a regenerating pool) is ever inferred.
fn energize(pools: &mut Vec<Power>, kind: u32, amount: f64, over: f64, max_power: u32) {
    let last = pools.iter().find(|p| p.kind == kind).copied();
    let max = match u64::from(max_power) {
        0 => last.map_or(0, |p| p.max),
        max => max,
    };
    if max == 0 {
        return;
    }
    let current = if over > 0.0 {
        max
    } else {
        // The parser's amounts are finite and never negative; `as`
        // saturates whatever it is handed.
        let gain = (amount * display_mod(kind)).round() as u64;
        last.map_or(0, |p| p.current).saturating_add(gain).min(max)
    };
    set_pool(pools, Power { kind, current, max });
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
