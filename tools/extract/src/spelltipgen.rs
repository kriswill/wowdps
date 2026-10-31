//! The spell tips cache (`gen-spell-tips`, `tools/gen-spell-tips.sh`):
//! what the replay's tooltips say of a spell and what its lanes need to
//! know of one, for every spell the history store's replays name and every
//! spell the curated tables in `crates/core/src` list, written per machine
//! beside `spell-icons.bin` as `spell-tips.bin`. Blizzard's words stay on
//! the machine, as the icons do: the file is never committed.
//!
//! Per spell, from the base rows (DifficultyID 0, else the first row):
//!
//! - **Name, school, cast, range, cooldown, cost, duration.** SpellName;
//!   SpellMisc's `SchoolMask`, its cast time (`SpellCastTimes`), range
//!   (`SpellRange`, the larger of its two) and the duration of the aura it
//!   applies (`SpellDuration`, -1 until cancelled); SpellCooldowns (the
//!   larger of its recovery and category recovery), else its charge
//!   category's recharge (`SpellCategories` → `SpellCategory`, with the
//!   charges where more than one); the cost line `spelltip` words.
//! - **Passive, channeled, empowered, on the GCD.** SpellMisc's
//!   `Attributes_0` 0x40 (passive) and `Attributes_1` 0x4 (channeled, but
//!   for an empowered spell: `SpellEmpower` lists those; the log writes
//!   one as its hold begins and its effect as it is let go, 0.7 to 1.1 s
//!   later in a real pull, so its length is the player's, never its
//!   duration's, and no channel is drawn for it);
//!   SpellCooldowns' `StartRecoveryTime` (the global cooldown it starts: a
//!   press does, a proc does not).
//! - **A class spell.** In R8's `class_spells` table: a spell a class has
//!   in its spellbook, its specialization or its talents.
//! - **The parent.** Where the log names a spell whose button logs another
//!   id, or none at all (a guardian druid's Feral Frenzy, 274837, logs
//!   only its damage spell, 274838), the lane asks which spell made it. A
//!   spell's parent is the spell whose description its own embeds (a
//!   description that is `$@spelldesc<id>`: the game's own tooltip naming
//!   its source, which is how 274838 names 274837, which no effect row
//!   links), else the spells whose effects trigger it
//!   (`SpellEffect.EffectTriggerSpell`, every difficulty's rows;
//!   `parents_of`). The chain is followed up to its roots (spells nothing
//!   makes). Where a root is active (the one named as the spell is, then
//!   the nearest, then the lowest id), the parent is the first active
//!   spell on the way to it, the button pressed (`root_of`); else it is a
//!   passive root, flagged so: an effect of a passive (a proc, a hero
//!   talent, a rune) is no press.
//! - **The description** through `spelltip::collect` at base values, and
//!   for a spell whose description is empty or terse, the encounter
//!   journal's text for it (or for a parent of it two levels up) where
//!   the journal's reading finds one (`journal::journal_texts`, the
//!   reading gen-rubric gives an ability).
//! - **The specs whose logs omit its press** (`omitted_presses`): for an
//!   effect whose parent is active, of its own name and on a cooldown
//!   (what the replay's lane would mark), the specs whose players, across
//!   the store's replays, put its aura on without a cast of the parent's
//!   name in more fights than with one. A guardian druid never logs Feral
//!   Frenzy, whose debuff it puts on; a protection warrior logs Shield
//!   Block in nearly every fight its buff shows in, and the few without
//!   are a proc's. The census is the store's: a press the log writes
//!   for a spec is no mark for it.
//!
//! File layout (all LE), read by `crates/gui-logic/src/spell_tips.rs`:
//!
//! ```text
//! "WDPT" | u32 version=2 | u64 stamp | u32 n
//! n × (u32 spell_id, u32 offset)          sorted by spell_id; offset into the body
//! body, a record each:
//!   u32 parent | u32 cast_ms | u32 cooldown_ms | i32 duration_ms | f32 range_yd
//!   u8 school | u8 flags | u8 charges | u8 m
//!   4 × (u32 len, UTF-8): name, cost, desc, journal
//!   m × u16: the specs whose logs omit its press (`omitted_presses`)
//! ```
//!
//! `flags`: 1 passive, 2 the parent is passive, 4 channeled, 8 a class
//! spell, 16 on the global cooldown, 32 empowered. The stamp is the
//! generator's rules, the build, the spells asked for and the census,
//! hashed: a rerun with the same four finds the file current and decodes
//! no table.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet, VecDeque};
use std::path::Path;

use wowdps_core::model::replay::{EventKind, UnitKind};

use crate::spelltip;
use crate::table::{Csv, Tables, int, text};

/// The tables it reads (their FileDataIDs: `table::FILE_IDS`): the spell
/// tables `spelltip` and the facts read, then the journal's
/// (`journal::JOURNAL_TABLES`).
pub const TABLES: [&str; 16] = [
    "SpellName",
    "SpellMisc",
    "Spell",
    "SpellEffect",
    "SpellPower",
    "SpellRange",
    "SpellCastTimes",
    "SpellDuration",
    "SpellRadius",
    "SpellDescriptionVariables",
    "SpellXDescriptionVariables",
    "SpellAuraOptions",
    "SpellCooldowns",
    "SpellCategories",
    "SpellCategory",
    "SpellEmpower",
];

/// The file's name beside the other per-machine caches.
pub const FILE: &str = "spell-tips.bin";

/// The file's format: 2 adds each record's specs whose logs omit its
/// press.
pub const VERSION: u32 = 2;

/// The rules a file was made by: part of its stamp, so a change to them
/// regenerates a file a rerun would otherwise find current. 4: an
/// expression read at base values (`spelltip`), the omitted presses.
const RULES: u32 = 4;

/// `flags` bits.
pub const PASSIVE: u8 = 1;
pub const PARENT_PASSIVE: u8 = 2;
pub const CHANNEL: u8 = 4;
pub const CLASS: u8 = 8;
pub const GCD: u8 = 16;
pub const EMPOWER: u8 = 32;

/// A description this short (in words) is terse: the journal's text, where
/// it has one, joins it.
const TERSE_WORDS: usize = 6;

/// How many spells a chain's walk up to its roots visits at most: a shared
/// spell many others trigger stops there.
const WALK_MOST: usize = 256;

/// One spell's record.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Record {
    pub id: u32,
    pub name: String,
    pub school: u8,
    pub flags: u8,
    pub charges: u8,
    pub cast_ms: u32,
    pub range_yd: f32,
    pub cooldown_ms: u32,
    pub duration_ms: i32,
    pub parent: u32,
    pub cost: String,
    pub desc: String,
    pub journal: String,
    /// The specs (the game's ids) whose logs omit the press this spell is
    /// the effect of (`omitted_presses`), ascending.
    pub marks: Vec<u16>,
}

/// One player's part of one stored fight, for the omitted presses'
/// census (`omitted_presses`): their spec (the game's id, 0 unknown), the
/// spells whose aura they put on, and the names of the spells they logged
/// a cast of (begun, landed or failed).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PlayerFight {
    pub spec: u32,
    pub applied: BTreeSet<u32>,
    pub cast: BTreeSet<String>,
}

/// What the history store's replays give the tips: their spells, each
/// player's part of each fight, and the files read and passed over (a
/// format this build does not decode).
#[derive(Debug, Default)]
pub struct StoreScan {
    pub ids: BTreeSet<u32>,
    pub fights: Vec<PlayerFight>,
    pub read: usize,
    pub unread: usize,
}

/// The spells asked for, and where from.
#[derive(Debug, Default)]
pub struct Wanted {
    pub ids: BTreeSet<u32>,
    /// R8's class spells (the `CLASS` flag).
    pub class: HashSet<u32>,
    /// The store's players' fights, for the omitted presses.
    pub fights: Vec<PlayerFight>,
    pub store: usize,
    pub store_files: usize,
    pub store_unread: usize,
    pub curated: usize,
}

/// The generated file and what it holds.
pub struct Generated {
    pub bytes: Vec<u8>,
    pub records: usize,
    pub parents: usize,
    pub active_parents: usize,
    pub journal: usize,
    pub nameless: usize,
    /// The effects some spec's logs omit the press of.
    pub marked: usize,
}

// ---- The spells asked for ------------------------------------------------------------

/// Whether a replay row's spell is a spell: a boss row's is its
/// encounter, a death's none.
fn names_a_spell(kind: EventKind) -> bool {
    !matches!(
        kind,
        EventKind::BossEngaged
            | EventKind::BossKilled
            | EventKind::BossWiped
            | EventKind::Death
            | EventKind::NpcDied
    )
}

/// Every spell the history store's replay tier names (`replay/<id>.bin`,
/// read offline as seed-creatures reads it): each event's and each placed
/// row's; and each player's part of each fight (`players_of`), for the
/// omitted presses. Files in name order, players in the cut's order.
pub fn store_spells(store: &Path) -> Result<StoreScan, String> {
    let dir = store.join("replay");
    let mut files: Vec<std::path::PathBuf> = std::fs::read_dir(&dir)
        .map_err(|e| format!("{}: {e}", dir.display()))?
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "bin"))
        .collect();
    files.sort();
    let mut scan = StoreScan::default();
    for f in &files {
        let Some(cut) = std::fs::read(f)
            .ok()
            .and_then(|b| wowdps_proto::replay::decode(&b))
        else {
            scan.unread += 1;
            continue;
        };
        scan.read += 1;
        scan.ids.extend(
            cut.events
                .iter()
                .filter(|e| names_a_spell(e.kind))
                .map(|e| e.spell_id),
        );
        scan.ids.extend(cut.placed.iter().map(|p| p.spell_id));
        scan.fights.extend(players_of(&cut));
    }
    scan.ids.remove(&0);
    Ok(scan)
}

/// Each player's part of a fight (`PlayerFight`): their spec, every aura
/// they put on (a debuff or a buff applied, on anyone), and every cast
/// they logged by its name. A player who put no aura on says nothing to
/// the census and is left out.
pub fn players_of(cut: &wowdps_core::model::replay::Cut) -> Vec<PlayerFight> {
    let mut out: Vec<PlayerFight> = cut
        .units
        .iter()
        .map(|u| PlayerFight {
            spec: match u.kind {
                UnitKind::Player => u.spec.map_or(0, |s| s.id()),
                _ => 0,
            },
            ..PlayerFight::default()
        })
        .collect();
    let player = |u: Option<u32>| {
        u.map(|u| u as usize)
            .filter(|&u| cut.units.get(u).is_some_and(|u| u.kind == UnitKind::Player))
    };
    for e in &cut.events {
        match e.kind {
            EventKind::DebuffApplied | EventKind::BuffApplied => {
                if let Some(p) = player(e.src).and_then(|u| out.get_mut(u)) {
                    p.applied.insert(e.spell_id);
                }
            }
            EventKind::PcastStart | EventKind::PcastSuccess | EventKind::PcastFailed => {
                if let Some(p) = player(e.unit).and_then(|u| out.get_mut(u)) {
                    p.cast.insert(e.spell.clone());
                }
            }
            _ => {}
        }
    }
    out.retain(|p| !p.applied.is_empty());
    out
}

/// For each record that is an effect a lane might mark (its parent
/// active, of its own name and on a cooldown, as the replay asks), the
/// specs whose logs omit the press: those whose players, across the
/// store's `fights`, put its aura on without a cast of the parent's name
/// (by that player in that fight) in more fights than with one. A spec
/// the store never shows, or a player whose spec is unknown, counts for
/// nothing. Returned by effect, each list ascending.
pub fn omitted_presses(records: &[Record], fights: &[PlayerFight]) -> BTreeMap<u32, Vec<u16>> {
    let by_id: HashMap<u32, &Record> = records.iter().map(|r| (r.id, r)).collect();
    let pressed: HashMap<u32, &str> = records
        .iter()
        .filter(|r| r.parent != 0 && r.parent != r.id && r.flags & PARENT_PASSIVE == 0)
        .filter_map(|r| {
            let p = by_id.get(&r.parent)?;
            (p.name == r.name && p.cooldown_ms > 0).then_some((r.id, p.name.as_str()))
        })
        .collect();
    // (effect, spec) → (fights without the press's name, fights with it).
    let mut seen: BTreeMap<(u32, u32), (u32, u32)> = BTreeMap::new();
    for f in fights.iter().filter(|f| f.spec != 0) {
        for id in &f.applied {
            if let Some(name) = pressed.get(id) {
                let n = seen.entry((*id, f.spec)).or_default();
                if f.cast.contains(*name) {
                    n.1 += 1;
                } else {
                    n.0 += 1;
                }
            }
        }
    }
    let mut out: BTreeMap<u32, Vec<u16>> = BTreeMap::new();
    for ((id, spec), (without, with)) in seen {
        if without > with
            && let Ok(spec) = u16::try_from(spec)
        {
            out.entry(id).or_default().push(spec);
        }
    }
    out
}

/// The spell ids of a committed table's `static TABLE` (`crates/core/src`'s
/// generated tables): each tuple's first number, or each number of a bare
/// list.
pub fn table_ids(src: &str) -> Vec<u32> {
    let Some((_, rest)) = src.split_once("static TABLE:") else {
        return Vec::new();
    };
    let Some((_, body)) = rest.split_once("= &[") else {
        return Vec::new();
    };
    let body = body.split_once("];").map_or(body, |(b, _)| b);
    let number = |s: &str| s.trim().parse::<u32>().ok();
    if body.contains('(') {
        body.split('(')
            .skip(1)
            .filter_map(|t| t.split(',').next().and_then(number))
            .collect()
    } else {
        body.split(',').filter_map(number).collect()
    }
}

/// The curated tables' spells, from the committed sources this build was
/// made from: R8's class spells (returned apart too, for the `CLASS`
/// flag), R12's item spells, R18's role spells, R20's absorbs and R26's
/// procs.
pub fn curated_spells() -> (BTreeSet<u32>, HashSet<u32>) {
    let class: Vec<u32> = table_ids(include_str!("../../../crates/core/src/class_spells.rs"));
    let others = [
        include_str!("../../../crates/core/src/item_spells.rs"),
        include_str!("../../../crates/core/src/role_spells.rs"),
        include_str!("../../../crates/core/src/absorb_spells.rs"),
        include_str!("../../../crates/core/src/proc_spells.rs"),
    ];
    let mut all: BTreeSet<u32> = class.iter().copied().collect();
    for src in others {
        all.extend(table_ids(src));
    }
    (all, class.into_iter().collect())
}

// ---- The stamp -----------------------------------------------------------------------

fn fnv(h: u64, bytes: &[u8]) -> u64 {
    bytes
        .iter()
        .fold(h, |h, b| (h ^ u64::from(*b)).wrapping_mul(0x0100_0000_01b3))
}

/// The rules, the build, the spells asked for and the omitted presses'
/// census (each player's part of each stored fight), hashed.
pub fn stamp(build: &str, ids: &BTreeSet<u32>, fights: &[PlayerFight]) -> u64 {
    let mut h = fnv(0xcbf2_9ce4_8422_2325, &RULES.to_le_bytes());
    h = fnv(h, &VERSION.to_le_bytes());
    h = fnv(h, build.as_bytes());
    for id in ids {
        h = fnv(h, &id.to_le_bytes());
    }
    for f in fights {
        h = fnv(h, &f.spec.to_le_bytes());
        for id in &f.applied {
            h = fnv(h, &id.to_le_bytes());
        }
        for name in &f.cast {
            h = fnv(h, name.as_bytes());
            h = fnv(h, &[0]);
        }
        h = fnv(h, &[1]);
    }
    h
}

/// The stamp a file was written with; none for a file of another format
/// or none at all.
pub fn read_stamp(bytes: &[u8]) -> Option<u64> {
    if bytes.get(..4)? != b"WDPT" {
        return None;
    }
    let version = u32::from_le_bytes(bytes.get(4..8)?.try_into().ok()?);
    (version == VERSION).then_some(())?;
    Some(u64::from_le_bytes(bytes.get(8..16)?.try_into().ok()?))
}

// ---- The tables read -----------------------------------------------------------------

/// A table's rows by spell, the base row (DifficultyID 0) preferred, else
/// the first: `cols` read from it.
fn base_rows<const N: usize>(csv: &Csv, cols: [&str; N]) -> Result<HashMap<u32, [i64; N]>, String> {
    let spell = csv.col("SpellID")?;
    let diff = csv.col("DifficultyID").ok();
    let mut at = [0usize; N];
    for (slot, name) in at.iter_mut().zip(cols) {
        *slot = csv.col(name)?;
    }
    let mut out: HashMap<u32, ([i64; N], bool)> = HashMap::new();
    for r in &csv.rows {
        let id = int(r, spell) as u32;
        let base = diff.is_none_or(|c| int(r, c) == 0);
        if out.get(&id).is_some_and(|(_, b)| *b) {
            continue;
        }
        if base || !out.contains_key(&id) {
            out.insert(id, (at.map(|c| int(r, c)), base));
        }
    }
    Ok(out.into_iter().map(|(k, (v, _))| (k, v)).collect())
}

/// A lookup table by `ID`: `col` read from each row.
fn by_id(csv: &Csv, col: &str) -> Result<HashMap<u32, f64>, String> {
    let (id, c) = (csv.col("ID")?, csv.col(col)?);
    Ok(csv
        .rows
        .iter()
        .map(|r| (int(r, id) as u32, text(r, c).parse().unwrap_or(0.0)))
        .collect())
}

/// Each spell's parents: the spell whose description its own embeds
/// (`$@spelldesc<id>` leading it), the game's own word for what it is an
/// effect of, alone where there is one; else the spells whose effects
/// trigger it (every difficulty's rows), sorted. No spell its own.
///
/// The description goes first because the trigger rows reach generic
/// spells: a rune's damage (Rune of Lingering) is triggered by another
/// rune's dummy, which a dozen nameless spells trigger, where its
/// description names the passive rune that grants it.
pub fn parents_of(
    triggers: impl IntoIterator<Item = (u32, u32)>,
    descs: &HashMap<u32, String>,
) -> HashMap<u32, Vec<u32>> {
    let mut out: HashMap<u32, Vec<u32>> = HashMap::new();
    for (spell, trig) in triggers {
        if trig != 0 && spell != 0 && trig != spell {
            out.entry(trig).or_default().push(spell);
        }
    }
    for list in out.values_mut() {
        list.sort_unstable();
        list.dedup();
    }
    for (&child, d) in descs {
        if let Some(id) = embedded_parent(d)
            && id != child
        {
            out.insert(child, vec![id]);
        }
    }
    out
}

/// SpellEffect's (spell, the spell its effect triggers) pairs, every row.
fn trigger_rows(tables: &Tables) -> Result<Vec<(u32, u32)>, String> {
    let se = tables.get("SpellEffect")?;
    let (s, t) = (se.col("SpellID")?, se.col("EffectTriggerSpell")?);
    Ok(se
        .rows
        .iter()
        .map(|r| (int(r, s) as u32, int(r, t) as u32))
        .filter(|&(_, t)| t != 0)
        .collect())
}

/// The spell a description that leads with `$@spelldesc<id>` embeds.
pub fn embedded_parent(desc: &str) -> Option<u32> {
    let rest = desc.trim_start().strip_prefix("$@spelldesc")?;
    let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
    digits.parse().ok().filter(|id| *id != 0)
}

/// A spell's parent as its chain gives it (`parents_of`), with whether it
/// is passive; none for a spell nothing makes. The chain is walked up to
/// its roots, and where one is active (the one named as the spell is,
/// then the nearest, then the lowest id) the parent is the first active
/// spell on the way there: the button pressed, which may itself be made
/// by another active spell (Fire Breath's burn is described as Fire
/// Breath, which a hero talent's active spell triggers; the burn's parent
/// is Fire Breath, which the log writes, not the root, which it never
/// does). Where every root is passive the parent is one of them (an
/// effect of a passive: a trinket's or a talent's proc, a rune).
pub fn root_of(
    spell: u32,
    parents: &HashMap<u32, Vec<u32>>,
    passive: &dyn Fn(u32) -> bool,
    name: &dyn Fn(u32) -> Option<String>,
) -> Option<(u32, bool)> {
    parents.get(&spell).filter(|p| !p.is_empty())?;
    // Each spell met, its depth and the spell it was reached from.
    let mut seen: HashMap<u32, (u32, u32)> = HashMap::from([(spell, (0, spell))]);
    let mut queue: VecDeque<u32> = VecDeque::from([spell]);
    let mut roots: Vec<(u32, u32)> = Vec::new();
    while let Some(at) = queue.pop_front() {
        let depth = seen.get(&at).map_or(0, |(d, _)| *d);
        match parents.get(&at).filter(|p| !p.is_empty()) {
            None if at != spell => roots.push((at, depth)),
            None => {}
            Some(list) => {
                for &p in list {
                    if seen.len() >= WALK_MOST {
                        break;
                    }
                    if let std::collections::hash_map::Entry::Vacant(e) = seen.entry(p) {
                        e.insert((depth + 1, at));
                        queue.push_back(p);
                    }
                }
            }
        }
    }
    let own = name(spell);
    let best = |active: bool| {
        roots
            .iter()
            .filter(|(r, _)| passive(*r) != active)
            .min_by_key(|(r, d)| (own.is_none() || name(*r) != own, *d, *r))
            .map(|(r, _)| *r)
    };
    if let Some(root) = best(true) {
        // The way from the root down to the spell, then its first active
        // spell above the spell itself.
        let mut way = vec![root];
        let mut at = root;
        while let Some(&(_, from)) = seen.get(&at)
            && from != at
            && from != spell
            && way.len() <= WALK_MOST
        {
            way.push(from);
            at = from;
        }
        let parent = way.iter().rev().copied().find(|s| !passive(*s));
        return Some((parent.unwrap_or(root), false));
    }
    best(false).map(|r| (r, true))
}

/// How many words a text has.
fn words(s: &str) -> usize {
    s.split_whitespace().count()
}

/// Every record for the spells of `wanted`, and their parents.
pub fn generate(tables: &Tables, wanted: &Wanted, stamp: u64) -> Result<Generated, String> {
    let sn = tables.get("SpellName")?;
    let (n_id, n_name) = (sn.col("ID")?, sn.col("Name_lang")?);
    let names: HashMap<u32, String> = sn
        .rows
        .iter()
        .map(|r| (int(r, n_id) as u32, text(r, n_name).to_string()))
        .filter(|(_, n)| !n.is_empty())
        .collect();
    let misc = base_rows(
        tables.get("SpellMisc")?,
        [
            "Attributes_0",
            "Attributes_1",
            "SchoolMask",
            "DurationIndex",
            "RangeIndex",
            "CastingTimeIndex",
        ],
    )?;
    let descs = spelltip::descriptions(tables)?;
    let parents = parents_of(trigger_rows(tables)?, &descs);
    let passive = |id: u32| misc.get(&id).is_some_and(|m| m[0] & 0x40 != 0);
    let name_of = |id: u32| names.get(&id).cloned();

    // Each spell asked for with its parent; the parents join the records,
    // so an effect's mark can say its parent's name.
    let mut roots: BTreeMap<u32, (u32, bool)> = BTreeMap::new();
    for &id in &wanted.ids {
        if let Some(r) = root_of(id, &parents, &passive, &name_of) {
            roots.insert(id, r);
        }
    }
    let mut ids: BTreeSet<u32> = wanted.ids.clone();
    ids.extend(roots.values().map(|(r, _)| *r));
    let ids: BTreeSet<u32> = ids
        .into_iter()
        .filter(|id| names.contains_key(id))
        .collect();
    let nameless = wanted
        .ids
        .iter()
        .filter(|id| !names.contains_key(id))
        .count();
    let set: HashSet<u32> = ids.iter().copied().collect();

    let tips = spelltip::collect(tables, &set, &names, &HashMap::new())?;

    let dur = by_id(tables.get("SpellDuration")?, "Duration")?;
    let sr = tables.get("SpellRange")?;
    let range: HashMap<u32, f64> = {
        let (a, b) = (by_id(sr, "RangeMax_0")?, by_id(sr, "RangeMax_1")?);
        a.into_iter()
            .map(|(k, v)| (k, v.max(b.get(&k).copied().unwrap_or(0.0))))
            .collect()
    };
    let cast = by_id(tables.get("SpellCastTimes")?, "Base")?;
    let empowered: HashSet<u32> = {
        let se = tables.get("SpellEmpower")?;
        let c = se.col("SpellID")?;
        se.rows.iter().map(|r| int(r, c) as u32).collect()
    };
    let cooldowns = base_rows(
        tables.get("SpellCooldowns")?,
        ["RecoveryTime", "CategoryRecoveryTime", "StartRecoveryTime"],
    )?;
    let categories = base_rows(tables.get("SpellCategories")?, ["ChargeCategory"])?;
    let charge: HashMap<u32, (f64, f64)> = {
        let sc = tables.get("SpellCategory")?;
        let (a, b) = (by_id(sc, "MaxCharges")?, by_id(sc, "ChargeRecoveryTime")?);
        a.into_iter()
            .map(|(k, v)| (k, (v, b.get(&k).copied().unwrap_or(0.0))))
            .collect()
    };

    // The journal's words, for the terse descriptions: a spell's own
    // section, else a parent's two levels up.
    let terse: Vec<u32> = ids
        .iter()
        .copied()
        .filter(|id| tips.get(id).is_none_or(|t| words(&t.desc) < TERSE_WORDS))
        .collect();
    let mut asked: HashSet<u32> = terse.iter().copied().collect();
    let up = |id: u32| parents.get(&id).cloned().unwrap_or_default();
    for &id in &terse {
        for p in up(id) {
            asked.insert(p);
            asked.extend(up(p));
        }
    }
    let journal = crate::journal::journal_texts(tables, &asked)?;
    let journal_of = |id: u32| {
        journal.get(&id).or_else(|| {
            let first = up(id);
            first.iter().find_map(|p| journal.get(p)).or_else(|| {
                first
                    .iter()
                    .flat_map(|&p| up(p))
                    .find_map(|q| journal.get(&q))
            })
        })
    };

    let mut records = Vec::with_capacity(ids.len());
    for &id in &ids {
        let m = misc.get(&id).copied().unwrap_or_default();
        let [attr0, attr1, school, dur_ix, range_ix, cast_ix] = m;
        let tip = tips.get(&id);
        let (mut cooldown, mut charges) = (
            cooldowns
                .get(&id)
                .map_or(0, |c| c[0].max(c[1]).max(0) as u32),
            0u8,
        );
        if let Some(&(most, recharge)) = categories
            .get(&id)
            .and_then(|c| charge.get(&(c[0] as u32)))
            .filter(|(most, recharge)| *most > 0.0 && *recharge > 0.0)
        {
            if cooldown == 0 {
                cooldown = recharge as u32;
            }
            if most > 1.0 {
                charges = most.min(255.0) as u8;
            }
        }
        let parent = roots.get(&id).copied();
        let mut flags = 0u8;
        if attr0 & 0x40 != 0 {
            flags |= PASSIVE;
        }
        if parent.is_some_and(|(_, p)| p) {
            flags |= PARENT_PASSIVE;
        }
        if empowered.contains(&id) {
            flags |= EMPOWER;
        } else if attr1 & 0x4 != 0 {
            flags |= CHANNEL;
        }
        if wanted.class.contains(&id) {
            flags |= CLASS;
        }
        if cooldowns.get(&id).is_some_and(|c| c[2] > 0) {
            flags |= GCD;
        }
        let desc = tip.map_or("", |t| t.desc.trim()).to_string();
        let journal = if words(&desc) < TERSE_WORDS {
            journal_of(id)
                .filter(|j| **j != desc)
                .cloned()
                .unwrap_or_default()
        } else {
            String::new()
        };
        records.push(Record {
            id,
            name: names.get(&id).cloned().unwrap_or_default(),
            school: (school & 0xFF) as u8,
            flags,
            charges,
            cast_ms: cast.get(&(cast_ix as u32)).map_or(0, |v| v.max(0.0) as u32),
            range_yd: range.get(&(range_ix as u32)).copied().unwrap_or(0.0) as f32,
            cooldown_ms: cooldown,
            duration_ms: match dur.get(&(dur_ix as u32)) {
                Some(v) if *v < 0.0 => -1,
                Some(v) => v.min(i32::MAX as f64) as i32,
                None => 0,
            },
            parent: parent.map_or(0, |(p, _)| p),
            cost: tip.map_or(String::new(), |t| t.cost.clone()),
            desc,
            journal,
            marks: Vec::new(),
        });
    }
    let mut marks = omitted_presses(&records, &wanted.fights);
    for r in &mut records {
        if let Some(specs) = marks.remove(&r.id) {
            r.marks = specs;
        }
    }
    Ok(Generated {
        marked: records.iter().filter(|r| !r.marks.is_empty()).count(),
        parents: records.iter().filter(|r| r.parent != 0).count(),
        active_parents: records
            .iter()
            .filter(|r| r.parent != 0 && r.flags & PARENT_PASSIVE == 0)
            .count(),
        journal: records.iter().filter(|r| !r.journal.is_empty()).count(),
        records: records.len(),
        nameless,
        bytes: encode(&records, stamp),
    })
}

// ---- The file ------------------------------------------------------------------------

fn put_str(out: &mut Vec<u8>, s: &str) {
    out.extend_from_slice(&(s.len() as u32).to_le_bytes());
    out.extend_from_slice(s.as_bytes());
}

/// The file's bytes for `records` (sorted by id here), under `stamp`.
pub fn encode(records: &[Record], stamp: u64) -> Vec<u8> {
    let mut sorted: Vec<&Record> = records.iter().collect();
    sorted.sort_by_key(|r| r.id);
    sorted.dedup_by_key(|r| r.id);
    let mut body: Vec<u8> = Vec::new();
    let mut index: Vec<(u32, u32)> = Vec::with_capacity(sorted.len());
    for r in &sorted {
        index.push((r.id, body.len() as u32));
        for v in [r.parent, r.cast_ms, r.cooldown_ms] {
            body.extend_from_slice(&v.to_le_bytes());
        }
        body.extend_from_slice(&r.duration_ms.to_le_bytes());
        body.extend_from_slice(&r.range_yd.to_le_bytes());
        let marks = r.marks.get(..r.marks.len().min(255)).unwrap_or_default();
        body.extend_from_slice(&[r.school, r.flags, r.charges, marks.len() as u8]);
        for s in [&r.name, &r.cost, &r.desc, &r.journal] {
            put_str(&mut body, s);
        }
        for spec in marks {
            body.extend_from_slice(&spec.to_le_bytes());
        }
    }
    let mut out = Vec::with_capacity(20 + index.len() * 8 + body.len());
    out.extend_from_slice(b"WDPT");
    out.extend_from_slice(&VERSION.to_le_bytes());
    out.extend_from_slice(&stamp.to_le_bytes());
    out.extend_from_slice(&(index.len() as u32).to_le_bytes());
    for (id, at) in index {
        out.extend_from_slice(&id.to_le_bytes());
        out.extend_from_slice(&at.to_le_bytes());
    }
    out.extend_from_slice(&body);
    out
}

#[cfg(test)]
mod tests;
