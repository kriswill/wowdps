//! The `crates/proto/src/placed/placed_spells.rs` generator: the spells
//! that place something in the room (a totem, a gateway, a ring, a zone,
//! a statue, a sigil, a trap), how the log tells where each stands, and
//! its reach and duration (`wowdps_proto::placed`).
//!
//! The membership is `CURATED` below and each entry says how it is found
//! (`Found`: its own unit's posts, the units its spells touched, its
//! caster, its target, a gateway's trips, the enemies it threw) and drawn.
//! The client's tables prove and complete it:
//!
//! - `SpellName` must carry exactly the expected name, for the placing
//!   spell and for every spell an entry is found by;
//! - its reach is the largest radius among a named spell's effects
//!   (`SpellEffect` â `SpellRadius`): the totem's own aura where the totem's
//!   summon gives only the circle a player aims with (Earthbind's slow
//!   reaches 10 yd, the totem's summon says 8). A few areas have no radius
//!   in the client (the shape is a server-side area trigger), and those
//!   carry the tooltip's yards by hand, marked so in the review twin;
//! - its duration is a named spell's `SpellDuration`: the gates' own
//!   summons (113886 / 113890) where the gateway's cast lasts 1 ms;
//! - the committed census of real logs (`tools/placed-spells-census.csv`,
//!   written by `tools/census-placed-spells.sh`) must show the placing
//!   spell cast or summoned by a player, and every spell it is found by
//!   applied as an aura or landing as a heal, at least once each.
//!
//! Left out on purpose: the damage carpets every pull is covered in
//! (Consecration, Death and Decay, Sigil of Flame, Blizzard), and what no
//! committed log holds (Earthen Wall and Ancestral Protection totems).
//!
//! Output is the table and its review twin (`placed_spells.expected.md`),
//! every entry with its sources and census counts.

use crate::table::{Tables, cell, parse_csv, text};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fmt::Write as _;
use wowdps_proto::placed::{Form, Found, Purpose};

/// The tables the generator consumes (their FileDataIDs: `table::FILE_IDS`).
pub const TABLES: [&str; 8] = [
    "SpellName",
    "SpellMisc",
    "SpellDuration",
    "SpellEffect",
    "SpellRadius",
    "SpellCooldowns",
    "SpellCategories",
    "SpellCategory",
];

/// Where an entry's reach or duration comes from.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Source {
    /// A spell's own: its largest effect radius, or its SpellDuration.
    Spell(u32),
    /// By hand, where the client has none (a server-side area trigger's
    /// radius, a trap's or feather's stand): yards, or ms.
    Hand(f32),
    /// None: no reach drawn, or gone as it lands.
    None,
}

/// One curated entry.
#[derive(Debug, Clone, Copy)]
pub struct Curated {
    pub purpose: Purpose,
    pub spell: u32,
    pub name: &'static str,
    pub found: &'static [Found],
    pub form: Form,
    pub reach: Source,
    pub lasts: Source,
    pub until_touched: bool,
}

const fn entry(
    purpose: Purpose,
    spell: u32,
    name: &'static str,
    found: &'static [Found],
    form: Form,
    reach: Source,
    lasts: Source,
) -> Curated {
    Curated {
        purpose,
        spell,
        name,
        found,
        form,
        reach,
        lasts,
        until_touched: false,
    }
}

impl Curated {
    /// The entry, standing only until the first unit it touches.
    const fn until_touched(self) -> Curated {
        Curated {
            until_touched: true,
            ..self
        }
    }
}

use Form::{Area, Gate, Post, Ring};
use Found::{Caster, Target, Thrown, Touch, Trips, Unit};
use Purpose::{Control, Heal, Wall, Way};
use Source::{Hand, Spell};

/// The table. Grouped by what it is for, for review; the generator sorts
/// by spell id.
pub const CURATED: &[Curated] = &[
    // -- Ways across the room ------------------------------------------------
    // The near gate where the warlock stood; the far one where travelers
    // (113942 on each, once a trip, self-applied) leave or land. The gates
    // are summoned by 113886 / 113890, which the log never writes.
    entry(
        Way,
        111771,
        "Demonic Gateway",
        &[Caster, Trips(113942)],
        Gate,
        Source::None,
        Spell(113886),
    ),
    entry(
        Way,
        192077,
        "Wind Rush Totem",
        &[Unit, Touch(&[192082])],
        Area,
        Spell(192077),
        Spell(192077),
    ),
    entry(
        Way,
        121536,
        "Angelic Feather",
        &[Touch(&[121557])],
        Post,
        Spell(121536),
        Hand(15_000.0),
    )
    .until_touched(),
    entry(
        Way,
        48018,
        "Demonic Circle",
        &[Caster],
        Post,
        Source::None,
        Spell(48018),
    ),
    entry(
        Way,
        101643,
        "Transcendence",
        &[Unit, Caster],
        Post,
        Source::None,
        Spell(101643),
    ),
    // -- Crowd control -------------------------------------------------------
    // Ring of Peace's log line is its cast alone: it is placed where the
    // enemies it threw out were thrown from, when they say so.
    entry(
        Control,
        116844,
        "Ring of Peace",
        &[Thrown],
        Area,
        Spell(116844),
        Spell(116844),
    ),
    // Its summoned unit (44199) is never posted: the enemies it froze tell.
    entry(
        Control,
        113724,
        "Ring of Frost",
        &[Unit, Touch(&[82691])],
        Ring,
        Spell(113724),
        Spell(113724),
    ),
    entry(
        Control,
        192058,
        "Capacitor Totem",
        &[Unit],
        Area,
        Spell(118905),
        Spell(192058),
    ),
    entry(
        Control,
        2484,
        "Earthbind Totem",
        &[Unit],
        Area,
        Spell(3600),
        Spell(2484),
    ),
    entry(
        Control,
        51485,
        "Earthgrab Totem",
        &[Unit],
        Area,
        Spell(64695),
        Spell(51485),
    ),
    entry(
        Control,
        30283,
        "Shadowfury",
        &[Touch(&[30283])],
        Area,
        Spell(30283),
        Source::None,
    ),
    entry(
        Control,
        102793,
        "Ursol's Vortex",
        &[Touch(&[127797])],
        Area,
        Spell(102793),
        Spell(102793),
    ),
    entry(
        Control,
        207684,
        "Sigil of Misery",
        &[Touch(&[207685])],
        Area,
        Spell(207684),
        Spell(207684),
    ),
    entry(
        Control,
        202137,
        "Sigil of Silence",
        &[Touch(&[204490])],
        Area,
        Spell(202137),
        Spell(202137),
    ),
    entry(
        Control,
        202138,
        "Sigil of Chains",
        &[Touch(&[204843])],
        Area,
        Spell(202138),
        Spell(202138),
    ),
    entry(
        Control,
        109248,
        "Binding Shot",
        &[Touch(&[117405])],
        Area,
        Spell(109248),
        Spell(109248),
    ),
    entry(
        Control,
        187650,
        "Freezing Trap",
        &[Touch(&[3355])],
        Area,
        Spell(187650),
        Hand(60_000.0),
    )
    .until_touched(),
    entry(
        Control,
        108199,
        "Gorefiend's Grasp",
        &[Target],
        Area,
        Spell(108199),
        Source::None,
    ),
    entry(
        Control,
        78675,
        "Solar Beam",
        &[Target],
        Area,
        Hand(5.0),
        Spell(78675),
    ),
    // -- The raid's walls ----------------------------------------------------
    // AMZ's buff has no source; a DK may drop it where they stand or aim
    // it, so the allies it covered tell.
    entry(
        Wall,
        51052,
        "Anti-Magic Zone",
        &[Touch(&[145629])],
        Area,
        Spell(51052),
        Spell(51052),
    ),
    entry(
        Wall,
        62618,
        "Power Word: Barrier",
        &[Touch(&[81782])],
        Area,
        Spell(62618),
        Spell(62618),
    ),
    entry(
        Wall,
        196718,
        "Darkness",
        &[Caster],
        Area,
        Hand(8.0),
        Spell(196718),
    ),
    entry(
        Wall,
        98008,
        "Spirit Link Totem",
        &[Unit],
        Area,
        Spell(98008),
        Spell(98008),
    ),
    // -- Healing, where it stands --------------------------------------------
    entry(
        Heal,
        5394,
        "Healing Stream Totem",
        &[Unit, Caster],
        Post,
        Source::None,
        Spell(5394),
    ),
    entry(
        Heal,
        108280,
        "Healing Tide Totem",
        &[Unit, Caster],
        Post,
        Source::None,
        Spell(108280),
    ),
    entry(
        Heal,
        1267068,
        "Stormstream Totem",
        &[Unit, Caster],
        Post,
        Source::None,
        Spell(1267068),
    ),
    entry(
        Heal,
        383013,
        "Poison Cleansing Totem",
        &[Unit, Caster],
        Post,
        Source::None,
        Spell(383013),
    ),
    entry(
        Heal,
        8143,
        "Tremor Totem",
        &[Unit, Caster],
        Post,
        Source::None,
        Spell(8143),
    ),
    // Never posted: its shaman's Healing Rain, which falls where it stands,
    // tells (a heal line carries where its target stood).
    entry(
        Heal,
        444995,
        "Surging Totem",
        &[Unit, Touch(&[73921])],
        Area,
        Spell(444995),
        Spell(444995),
    ),
    entry(
        Heal,
        145205,
        "Efflorescence",
        &[Unit, Touch(&[81269])],
        Area,
        Spell(145205),
        Spell(145205),
    ),
    entry(
        Heal,
        1232285,
        "Efflorescence",
        &[Unit, Touch(&[81269])],
        Area,
        Spell(1232285),
        Spell(1232285),
    ),
    entry(
        Heal,
        115313,
        "Summon Jade Serpent Statue",
        &[Unit],
        Post,
        Source::None,
        Spell(115313),
    ),
    entry(
        Heal,
        115315,
        "Summon Black Ox Statue",
        &[Unit],
        Post,
        Source::None,
        Spell(115315),
    ),
];

/// Every spell the census counts: each entry's placing spell and every
/// spell it is found by, one id a line (`wowdps-extract placed-spells-ids`,
/// for tools/census-placed-spells.sh).
pub fn census_ids() -> String {
    let mut ids: Vec<u32> = CURATED
        .iter()
        .flat_map(|c| std::iter::once(c.spell).chain(telling(c.found)))
        .collect();
    ids.sort_unstable();
    ids.dedup();
    ids.iter().map(|id| format!("{id}\n")).collect()
}

/// The spells a list of `Found` is told by.
fn telling(found: &[Found]) -> Vec<u32> {
    found
        .iter()
        .flat_map(|f| match f {
            Touch(ids) => ids.to_vec(),
            Trips(id) => vec![*id],
            _ => Vec::new(),
        })
        .collect()
}

/// The census: per (event, spell), the name the log wrote and a count per
/// log. Events: `cast` (a player's SPELL_CAST_SUCCESS), `summon` (a
/// player's SPELL_SUMMON or SPELL_CREATE), `aura` (SPELL_AURA_APPLIED on
/// anyone), `heal` (SPELL_HEAL or SPELL_PERIODIC_HEAL).
#[derive(Debug, Default)]
pub struct Census {
    pub logs: Vec<String>,
    pub counts: HashMap<(String, u32), (String, Vec<u64>)>,
}

impl Census {
    pub fn parse(text: &str) -> Result<Self, String> {
        let csv = parse_csv(text).map_err(|e| format!("census: {e}"))?;
        let (ev_c, id_c, name_c) = (csv.col("event")?, csv.col("id")?, csv.col("name")?);
        let logs: Vec<String> = csv.header.get(3..).unwrap_or(&[]).to_vec();
        if logs.is_empty() {
            return Err("census: no log columns after event,id,name".into());
        }
        let mut counts = HashMap::new();
        for row in &csv.rows {
            let ev = cell(row, ev_c, "census")?.to_string();
            let id: u32 = cell(row, id_c, "census")?
                .parse()
                .map_err(|_| format!("census: bad id {:?}", row.get(id_c)))?;
            let name = cell(row, name_c, "census")?.to_string();
            let mut per_log = Vec::with_capacity(logs.len());
            for c in 3..3 + logs.len() {
                let v = cell(row, c, "census")?;
                per_log.push(
                    v.parse()
                        .map_err(|_| format!("census: bad count {v:?} for {ev} {id}"))?,
                );
            }
            if counts.insert((ev.clone(), id), (name, per_log)).is_some() {
                return Err(format!("census: duplicate {ev} {id}"));
            }
        }
        Ok(Self { logs, counts })
    }

    /// How many times the logs hold `spell` as any of `events`, and the
    /// name they wrote it with.
    fn seen(&self, events: &[&str], spell: u32) -> (u64, Option<&str>) {
        let mut total = 0;
        let mut name = None;
        for ev in events {
            if let Some((n, per_log)) = self.counts.get(&((*ev).to_string(), spell)) {
                total += per_log.iter().sum::<u64>();
                name = Some(n.as_str());
            }
        }
        (total, name)
    }
}

#[derive(Debug)]
pub struct Generated {
    pub content: String,
    pub expected: String,
    pub spells: usize,
}

/// What the client's tables say of the spells the list names.
struct Client {
    names: HashMap<u32, String>,
    /// Each spell's largest effect radius (yd), base difficulty.
    reach: HashMap<u32, f64>,
    /// Each spell's duration (ms), base difficulty.
    lasts: HashMap<u32, i64>,
    /// Each spell's cooldown (ms), base difficulty: its own, its category's,
    /// or its charges' recharge, whichever is longest.
    cooldown: HashMap<u32, u32>,
}

fn client(tables: &Tables, wanted: &HashSet<u32>) -> Result<Client, String> {
    let sn = tables.get("SpellName")?;
    let (n_id, n_name) = (sn.col("ID")?, sn.col("Name_lang")?);
    let mut names = HashMap::new();
    for row in &sn.rows {
        let id: u32 = text(row, n_id).parse().unwrap_or(0);
        if wanted.contains(&id) {
            names.insert(id, cell(row, n_name, "SpellName")?.to_string());
        }
    }

    let sr = tables.get("SpellRadius")?;
    let (r_id, r_r, r_max) = (sr.col("ID")?, sr.col("Radius")?, sr.col("RadiusMax")?);
    let radius: HashMap<u32, f64> = sr
        .rows
        .iter()
        .filter_map(|r| {
            let v: f64 = text(r, r_r).parse().unwrap_or(0.0);
            let v = if v > 0.0 {
                v
            } else {
                text(r, r_max).parse().unwrap_or(0.0)
            };
            Some((text(r, r_id).parse().ok()?, v))
        })
        .collect();
    let se = tables.get("SpellEffect")?;
    let (e_spell, e_r0, e_r1) = (
        se.col("SpellID")?,
        se.col("EffectRadiusIndex_0")?,
        se.col("EffectRadiusIndex_1")?,
    );
    let e_diff = se.col("DifficultyID").ok();
    let mut reach: HashMap<u32, f64> = HashMap::new();
    for row in &se.rows {
        let Ok(id) = text(row, e_spell).parse::<u32>() else {
            continue;
        };
        if !wanted.contains(&id) || e_diff.is_some_and(|c| !matches!(text(row, c), "" | "0")) {
            continue;
        }
        for c in [e_r0, e_r1] {
            let r = text(row, c)
                .parse::<u32>()
                .ok()
                .and_then(|ix| radius.get(&ix))
                .copied()
                .unwrap_or(0.0);
            let best = reach.entry(id).or_insert(0.0);
            if r > *best {
                *best = r;
            }
        }
    }

    let sd = tables.get("SpellDuration")?;
    let (d_id, d_ms) = (sd.col("ID")?, sd.col("Duration")?);
    let by_index: HashMap<u32, i64> = sd
        .rows
        .iter()
        .filter_map(|r| Some((text(r, d_id).parse().ok()?, text(r, d_ms).parse().ok()?)))
        .collect();
    let sm = tables.get("SpellMisc")?;
    let (m_spell, m_dur) = (sm.col("SpellID")?, sm.col("DurationIndex")?);
    let m_diff = sm.col("DifficultyID").ok();
    let mut lasts = HashMap::new();
    for row in &sm.rows {
        let Ok(id) = text(row, m_spell).parse::<u32>() else {
            continue;
        };
        if !wanted.contains(&id) {
            continue;
        }
        let base = m_diff.is_none_or(|c| matches!(text(row, c), "" | "0"));
        if base || !lasts.contains_key(&id) {
            let ix: u32 = text(row, m_dur).parse().unwrap_or(0);
            if let Some(&ms) = by_index.get(&ix) {
                lasts.insert(id, ms);
            }
        }
    }
    // Cooldowns: the spell's own and its category's (SpellCooldowns), and
    // the recharge of its charges (SpellCategories → SpellCategory).
    let base =
        |row: &[String], c: Option<usize>| c.is_none_or(|c| matches!(text(row, c), "" | "0"));
    let mut cooldown: HashMap<u32, u32> = HashMap::new();
    let mut longest = |id: u32, ms: u32| {
        let best = cooldown.entry(id).or_insert(0);
        *best = (*best).max(ms);
    };
    let sc = tables.get("SpellCooldowns")?;
    let (c_spell, c_rec, c_cat) = (
        sc.col("SpellID")?,
        sc.col("RecoveryTime")?,
        sc.col("CategoryRecoveryTime")?,
    );
    let c_diff = sc.col("DifficultyID").ok();
    for row in &sc.rows {
        let Ok(id) = text(row, c_spell).parse::<u32>() else {
            continue;
        };
        if wanted.contains(&id) && base(row, c_diff) {
            for c in [c_rec, c_cat] {
                longest(id, text(row, c).parse().unwrap_or(0));
            }
        }
    }
    let cats = tables.get("SpellCategory")?;
    let (k_id, k_charge) = (cats.col("ID")?, cats.col("ChargeRecoveryTime")?);
    let recharge: HashMap<u32, u32> = cats
        .rows
        .iter()
        .filter_map(|r| Some((text(r, k_id).parse().ok()?, text(r, k_charge).parse().ok()?)))
        .collect();
    let scs = tables.get("SpellCategories")?;
    let (s_spell, s_charge) = (scs.col("SpellID")?, scs.col("ChargeCategory")?);
    let s_diff = scs.col("DifficultyID").ok();
    for row in &scs.rows {
        let Ok(id) = text(row, s_spell).parse::<u32>() else {
            continue;
        };
        if wanted.contains(&id) && base(row, s_diff) {
            let category: u32 = text(row, s_charge).parse().unwrap_or(0);
            if let Some(&ms) = recharge.get(&category).filter(|_| category != 0) {
                longest(id, ms);
            }
        }
    }
    Ok(Client {
        names,
        reach,
        lasts,
        cooldown,
    })
}

/// One entry as proven: its reach and duration, and the census counts
/// beside the spells it names.
struct Proven<'a> {
    c: &'a Curated,
    reach: f64,
    lasts_ms: u32,
    cooldown_ms: u32,
    /// (spell, name, the events it was counted as, total).
    evidence: Vec<(u32, String, &'static str, u64)>,
}

pub fn generate(tables: &Tables, census: &Census, build: &str) -> Result<Generated, String> {
    generate_curated(CURATED, tables, census, build)
}

/// The generator over an explicit list, so tests can run it on a few.
pub fn generate_curated(
    curated: &[Curated],
    tables: &Tables,
    census: &Census,
    build: &str,
) -> Result<Generated, String> {
    let spells: HashSet<u32> = curated.iter().map(|c| c.spell).collect();
    if spells.len() != curated.len() {
        return Err("curated list holds a duplicate spell".into());
    }
    let source_ids = |s: &Source| match s {
        Spell(id) => Some(*id),
        _ => None,
    };
    let wanted: HashSet<u32> = curated
        .iter()
        .flat_map(|c| {
            [c.spell]
                .into_iter()
                .chain(telling(c.found))
                .chain(source_ids(&c.reach))
                .chain(source_ids(&c.lasts))
        })
        .collect();
    let client = client(tables, &wanted)?;
    let name_of = |id: u32| {
        client
            .names
            .get(&id)
            .cloned()
            .ok_or_else(|| format!("placed spell {id}: no SpellName row"))
    };

    let mut table: BTreeMap<u32, Proven<'_>> = BTreeMap::new();
    for c in curated {
        let actual = name_of(c.spell)?;
        if actual != c.name {
            return Err(format!(
                "placed spell {}: SpellName says {actual:?}, curated as {:?}",
                c.spell, c.name
            ));
        }
        let reach = match c.reach {
            Spell(id) => match client.reach.get(&id) {
                Some(&r) if r > 0.0 => r,
                _ => {
                    return Err(format!(
                        "placed spell {} ({}): {id} has no effect radius",
                        c.spell, c.name
                    ));
                }
            },
            Hand(yd) => f64::from(yd),
            Source::None => 0.0,
        };
        let lasts_ms = match c.lasts {
            Spell(id) => match client.lasts.get(&id) {
                Some(&ms) if ms > 0 => u32::try_from(ms).map_err(|e| format!("{id}: {e}"))?,
                other => {
                    return Err(format!(
                        "placed spell {} ({}): {id} lasts {other:?}, not a time",
                        c.spell, c.name
                    ));
                }
            },
            Hand(ms) => ms as u32,
            Source::None => 0,
        };
        // The census: the placing spell cast or summoned by a player, and
        // every spell it is found by applied or landing as a heal.
        let mut evidence = Vec::new();
        let (n, logged) = census.seen(&["cast", "summon"], c.spell);
        if n == 0 {
            return Err(format!(
                "placed spell {} ({}): no player casts or summons it in the census",
                c.spell, c.name
            ));
        }
        if logged.is_some_and(|l| l != c.name) {
            return Err(format!(
                "placed spell {}: the census names it {logged:?}, curated as {:?}",
                c.spell, c.name
            ));
        }
        evidence.push((c.spell, c.name.to_string(), "cast or summon", n));
        for id in telling(c.found) {
            let name = name_of(id)?;
            let (n, logged) = census.seen(&["aura", "heal"], id);
            if n == 0 {
                return Err(format!(
                    "placed spell {} ({}): {id} ({name}) is never applied or healed with in the census",
                    c.spell, c.name
                ));
            }
            if logged.is_some_and(|l| l != name) {
                return Err(format!(
                    "placed spell {}: the census names {id} {logged:?}, SpellName {name:?}",
                    c.spell
                ));
            }
            evidence.push((id, name, "aura or heal", n));
        }
        table.insert(
            c.spell,
            Proven {
                c,
                reach,
                lasts_ms,
                cooldown_ms: client.cooldown.get(&c.spell).copied().unwrap_or(0),
                evidence,
            },
        );
    }
    Ok(Generated {
        spells: table.len(),
        content: emit(&table, build).map_err(|e| format!("emit: {e}"))?,
        expected: emit_expected(&table, census, build).map_err(|e| format!("emit: {e}"))?,
    })
}

/// A `Found` as the Rust that builds it.
fn found_rs(f: &Found) -> String {
    match f {
        Unit => "Unit".into(),
        Touch(ids) => format!(
            "Touch(&[{}])",
            ids.iter()
                .map(u32::to_string)
                .collect::<Vec<_>>()
                .join(", ")
        ),
        Caster => "Caster".into(),
        Target => "Target".into(),
        Trips(id) => format!("Trips({id})"),
        Thrown => "Thrown".into(),
    }
}

/// A reach in yards as Rust: the client's radius, to a hundredth.
fn yards(r: f64) -> String {
    let s = format!("{r:.2}");
    let s = s.trim_end_matches('0');
    if s.ends_with('.') {
        format!("{s}0")
    } else {
        s.to_string()
    }
}

fn emit(table: &BTreeMap<u32, Proven<'_>>, build: &str) -> Result<String, std::fmt::Error> {
    let mut o = String::new();
    writeln!(
        o,
        "//! GENERATED by tools/gen-placed-spells.sh — do not edit by hand."
    )?;
    // No timestamp: same build in, same bytes out.
    writeln!(
        o,
        "//! Source: local client DB2s via wowdps-extract, build {build}."
    )?;
    writeln!(o, "//! {} placed spells.", table.len())?;
    o.push_str(
        "//!\n\
         //! The spells that place something in the room, by spell id: how the\n\
         //! log tells where each stands, how it is drawn, its reach and how\n\
         //! long it stands (`wowdps_proto::placed::Placed`). Membership is\n\
         //! curated in tools/extract/src/placegen.rs; every name, reach and\n\
         //! duration is the client's (or marked as by hand), and the real-log\n\
         //! census behind each entry sits in placed_spells.expected.md.\n\n\
         use super::Form::{Area, Gate, Post, Ring};\n\
         use super::Found::{Caster, Target, Thrown, Touch, Trips, Unit};\n\
         use super::Placed;\n\
         use super::Purpose::{Control, Heal, Wall, Way};\n\n\
         /// Sorted by spell id.\n\
         #[rustfmt::skip]\n\
         pub(super) static TABLE: &[Placed] = &[\n",
    );
    for (id, p) in table {
        let found: Vec<String> = p.c.found.iter().map(found_rs).collect();
        writeln!(
            o,
            "    Placed {{ spell: {id}, name: {:?}, purpose: {:?}, found: &[{}], form: {:?}, reach: {}, lasts_ms: {}, cooldown_ms: {}, until_touched: {} }},",
            p.c.name,
            p.c.purpose,
            found.join(", "),
            p.c.form,
            yards(p.reach),
            p.lasts_ms,
            p.cooldown_ms,
            p.c.until_touched
        )?;
    }
    o.push_str("];\n");
    Ok(o)
}

fn emit_expected(
    table: &BTreeMap<u32, Proven<'_>>,
    census: &Census,
    build: &str,
) -> Result<String, std::fmt::Error> {
    let mut o = String::new();
    writeln!(o, "# Placed spells — review twin of placed_spells.rs")?;
    writeln!(o)?;
    writeln!(
        o,
        "GENERATED by tools/gen-placed-spells.sh from build {build} and a census of {} log(s). \
         The reach and duration say where they came from: a spell's own (its id), or by hand \
         where the client has none. The census counts what the logs hold: the placing spell \
         cast or summoned by a player, and each spell it is found by applied as an aura or \
         landing as a heal.",
        census.logs.len()
    )?;
    writeln!(o)?;
    writeln!(
        o,
        "| spell | name | for | found | form | reach (yd) | lasts (ms) | cooldown (s) | census |"
    )?;
    writeln!(o, "|---|---|---|---|---|---|---|---|---|")?;
    let source = |s: Source, v: String| match s {
        Spell(id) => format!("{v} ({id})"),
        Hand(_) => format!("{v} (by hand)"),
        Source::None => "—".to_string(),
    };
    for (id, p) in table {
        let found: Vec<String> = p.c.found.iter().map(found_rs).collect();
        let lasts = match p.c.until_touched {
            true => format!(
                "{} until touched",
                source(p.c.lasts, p.lasts_ms.to_string())
            ),
            false => source(p.c.lasts, p.lasts_ms.to_string()),
        };
        let census: Vec<String> = p
            .evidence
            .iter()
            .map(|(id, name, what, n)| format!("{name} {id}: {n} {what}"))
            .collect();
        writeln!(
            o,
            "| {id} | {} | {:?} | {} | {:?} | {} | {lasts} | {} | {} |",
            p.c.name,
            p.c.purpose,
            found.join(", "),
            p.c.form,
            source(p.c.reach, yards(p.reach)),
            p.cooldown_ms as f32 / 1000.0,
            census.join("; ")
        )?;
    }
    Ok(o)
}

#[cfg(test)]
mod tests;
