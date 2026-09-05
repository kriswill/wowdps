//! The `crates/core/src/role_spells.rs` generator: aura id → the role the
//! buff plays (CONTRACT.md R18), validated against the local install the
//! same way `itemgen` derives the item table.
//!
//! Unlike the other generators this one does not *discover* its table: the
//! membership is `CURATED` below — `(aura id, expected name, kind)`, the
//! meter's `EXTERNAL_BUFFS` list grown to five kinds — and the client tables
//! only *prove* each entry:
//!
//! - `SpellName` must carry exactly the expected name (a renamed or removed
//!   spell fails the build, naming the id);
//! - `SpellEffect` must hold at least one `Effect == 6` (APPLY_AURA) row for
//!   the id. A cast id whose buff the log writes under another id has no
//!   such row — Metamorphosis 191427 vs its buff 162264, Bladestorm 227847
//!   vs 446035 — and a name check alone would wave it through;
//! - the committed census of real logs (`tools/role-spells-census.csv`,
//!   written by `tools/census-role-spells.sh`) must show the id applied to a
//!   player at least once, under the same name. Only census-exercised ids
//!   ship; a guess is a build failure, not a silent zero — unless the entry
//!   carries a `census_exempt` reason (`Curated::exempt`): a real external
//!   the committed logs happen not to hold (the hunter lusts) still gets its
//!   name and APPLY_AURA proven, and the reason is printed in the review
//!   twin's observed column in place of the counts, so the waiver is on
//!   record beside the evidence it replaces.
//!
//! The ids are AURA ids — the spell the log names on `SPELL_AURA_APPLIED` —
//! never the cast's, which is why several differ from the tooltip id a
//! player would look up (Blur 212800, Fortifying Brew 120954, Spirit Link
//! Totem 325174, Rescue 370667). No class/spec gate: an external lands on
//! its target whatever the target's class, and nothing reads one.
//!
//! Output is two files: the table (`role_spells.rs`) and its review twin
//! (`role_spells.expected.md`) listing every entry with its census counts.

use crate::table::{Csv, parse_csv};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fmt::Write as _;
use wowdps_core::model::RoleSpellKind;

/// The tables the generator consumes, with their FileDataIDs
/// (from wowdev/wow-listfile; stable per file, forever).
pub const TABLES: [(&str, u32); 2] = [("SpellName", 1990283), ("SpellEffect", 1140088)];

/// SpellEffect.Effect for APPLY_AURA.
const EFFECT_APPLY_AURA: &str = "6";

/// Emitted kind order; index = `RoleSpellKind::code`, and the KINDS array
/// in the generated file.
const KIND_ORDER: [RoleSpellKind; 5] = [
    RoleSpellKind::ActiveMitigation,
    RoleSpellKind::Defensive,
    RoleSpellKind::External,
    RoleSpellKind::SupportBuff,
    RoleSpellKind::Cooldown,
];

/// One curated entry: the AURA id, the name `SpellName` must carry, its
/// kind, and — for the few real externals the committed census has never
/// seen — the reason the census requirement is waived.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Curated {
    pub id: u32,
    pub name: &'static str,
    pub kind: RoleSpellKind,
    /// `Some(reason)` waives the census check (name + APPLY_AURA still
    /// prove the id); the reason lands in `role_spells.expected.md`.
    pub census_exempt: Option<&'static str>,
}

impl Curated {
    /// An entry the census must exercise.
    pub const fn seen(id: u32, name: &'static str, kind: RoleSpellKind) -> Self {
        Self {
            id,
            name,
            kind,
            census_exempt: None,
        }
    }

    /// An entry the census is not required to hold, with the reason why.
    pub const fn exempt(
        id: u32,
        name: &'static str,
        kind: RoleSpellKind,
        reason: &'static str,
    ) -> Self {
        Self {
            id,
            name,
            kind,
            census_exempt: Some(reason),
        }
    }
}

/// The waiver on the hunter pets' lusts: R12's `EXTERNAL_BUFFS` marked
/// them, no committed log has a hunter lusting, and they are Bloodlust to
/// the letter.
const HUNTER_LUSTS: &str = "hunter lusts — unobserved in the committed logs, R12 marked them";

/// The table. Every id is the AURA the combat log applies to a player, with
/// the name `SpellName` must carry; the census counts beside each entry
/// live in `role_spells.expected.md`. Grouped by kind, then by class, for
/// review — the generator sorts by id.
const CURATED: &[Curated] = &[
    // -- ActiveMitigation: a tank's rotational mitigation buff ---------------
    Curated::seen(132404, "Shield Block", RoleSpellKind::ActiveMitigation),
    Curated::seen(
        132403,
        "Shield of the Righteous",
        RoleSpellKind::ActiveMitigation,
    ),
    Curated::seen(192081, "Ironfur", RoleSpellKind::ActiveMitigation),
    Curated::seen(203819, "Demon Spikes", RoleSpellKind::ActiveMitigation),
    Curated::seen(195181, "Bone Shield", RoleSpellKind::ActiveMitigation),
    Curated::seen(215479, "Shuffle", RoleSpellKind::ActiveMitigation),
    Curated::seen(77535, "Blood Shield", RoleSpellKind::ActiveMitigation),
    // -- Defensive: a personal damage-reduction cooldown --------------------
    Curated::seen(871, "Shield Wall", RoleSpellKind::Defensive),
    Curated::seen(86659, "Guardian of Ancient Kings", RoleSpellKind::Defensive),
    Curated::seen(48792, "Icebound Fortitude", RoleSpellKind::Defensive),
    Curated::seen(81256, "Dancing Rune Weapon", RoleSpellKind::Defensive),
    // The buff, not the cast (115203).
    Curated::seen(120954, "Fortifying Brew", RoleSpellKind::Defensive),
    Curated::seen(61336, "Survival Instincts", RoleSpellKind::Defensive),
    Curated::seen(47585, "Dispersion", RoleSpellKind::Defensive),
    Curated::seen(363916, "Obsidian Scales", RoleSpellKind::Defensive),
    Curated::seen(186265, "Aspect of the Turtle", RoleSpellKind::Defensive),
    Curated::seen(5277, "Evasion", RoleSpellKind::Defensive),
    // The buff, not the cast (198589).
    Curated::seen(212800, "Blur", RoleSpellKind::Defensive),
    Curated::seen(108271, "Astral Shift", RoleSpellKind::Defensive),
    Curated::seen(104773, "Unending Resolve", RoleSpellKind::Defensive),
    Curated::seen(342246, "Alter Time", RoleSpellKind::Defensive),
    // Raid-wide from one warrior: a span on every member, caster = the warrior.
    Curated::seen(97463, "Rallying Cry", RoleSpellKind::Defensive),
    Curated::seen(145629, "Anti-Magic Zone", RoleSpellKind::Defensive),
    // The per-player aura the totem applies (cast 98008); its caster is the
    // totem creature, not the shaman.
    Curated::seen(325174, "Spirit Link Totem", RoleSpellKind::Defensive),
    Curated::seen(31821, "Aura Mastery", RoleSpellKind::Defensive),
    // -- External: a buff cast on someone else ------------------------------
    Curated::seen(2825, "Bloodlust", RoleSpellKind::External),
    Curated::seen(32182, "Heroism", RoleSpellKind::External),
    Curated::seen(80353, "Time Warp", RoleSpellKind::External),
    Curated::seen(264667, "Primal Rage", RoleSpellKind::External),
    Curated::seen(390386, "Fury of the Aspects", RoleSpellKind::External),
    // The hunter pets' lusts — real externals no committed log holds.
    Curated::exempt(
        90355,
        "Ancient Hysteria",
        RoleSpellKind::External,
        HUNTER_LUSTS,
    ),
    Curated::exempt(160452, "Netherwinds", RoleSpellKind::External, HUNTER_LUSTS),
    Curated::exempt(
        466904,
        "Harrier's Cry",
        RoleSpellKind::External,
        HUNTER_LUSTS,
    ),
    Curated::seen(10060, "Power Infusion", RoleSpellKind::External),
    Curated::seen(33206, "Pain Suppression", RoleSpellKind::External),
    Curated::seen(47788, "Guardian Spirit", RoleSpellKind::External),
    Curated::seen(102342, "Ironbark", RoleSpellKind::External),
    Curated::seen(116849, "Life Cocoon", RoleSpellKind::External),
    Curated::seen(6940, "Blessing of Sacrifice", RoleSpellKind::External),
    Curated::seen(1022, "Blessing of Protection", RoleSpellKind::External),
    Curated::seen(29166, "Innervate", RoleSpellKind::External),
    Curated::seen(357170, "Time Dilation", RoleSpellKind::External),
    // The rescued ally's buff (cast 370665; 370666 is the evoker's own). The
    // log writes its source as the target, so the caster is lost here.
    Curated::seen(370667, "Rescue", RoleSpellKind::External),
    // -- SupportBuff: a buff whose value is the target's output --------------
    // The ally-side aura; 395296 is the evoker's own Ebon Might.
    Curated::seen(395152, "Ebon Might", RoleSpellKind::SupportBuff),
    Curated::seen(410089, "Prescience", RoleSpellKind::SupportBuff),
    Curated::seen(413984, "Shifting Sands", RoleSpellKind::SupportBuff),
    // -- Cooldown: a major offensive cooldown's own buff ---------------------
    // Havoc's buff (cast 191427); Devourer's is "Void Metamorphosis".
    Curated::seen(162264, "Metamorphosis", RoleSpellKind::Cooldown),
    Curated::seen(107574, "Avatar", RoleSpellKind::Cooldown),
    Curated::seen(190319, "Combustion", RoleSpellKind::Cooldown),
    Curated::seen(365362, "Arcane Surge", RoleSpellKind::Cooldown),
    Curated::seen(375087, "Dragonrage", RoleSpellKind::Cooldown),
    Curated::seen(114051, "Ascendance", RoleSpellKind::Cooldown),
    Curated::seen(114052, "Ascendance", RoleSpellKind::Cooldown),
    // The buff, not the cast (227847).
    Curated::seen(446035, "Bladestorm", RoleSpellKind::Cooldown),
    Curated::seen(1719, "Recklessness", RoleSpellKind::Cooldown),
    Curated::seen(121471, "Shadow Blades", RoleSpellKind::Cooldown),
    Curated::seen(194249, "Voidform", RoleSpellKind::Cooldown),
    Curated::seen(
        102560,
        "Incarnation: Chosen of Elune",
        RoleSpellKind::Cooldown,
    ),
    Curated::seen(19574, "Bestial Wrath", RoleSpellKind::Cooldown),
    Curated::seen(288613, "Trueshot", RoleSpellKind::Cooldown),
    Curated::seen(51271, "Pillar of Frost", RoleSpellKind::Cooldown),
    Curated::seen(42650, "Army of the Dead", RoleSpellKind::Cooldown),
    Curated::seen(31884, "Avenging Wrath", RoleSpellKind::Cooldown),
    Curated::seen(274837, "Feral Frenzy", RoleSpellKind::Cooldown),
];

/// The committed real-log census: for every aura id the logs applied to a
/// player, the name the log wrote and one count per log.
#[derive(Debug, Default)]
pub struct Census {
    /// Log basenames, in column order.
    pub logs: Vec<String>,
    /// id → (name, count per log).
    pub counts: HashMap<u32, (String, Vec<u64>)>,
}

impl Census {
    /// Parse `tools/census-role-spells.sh`'s CSV: `id,name,<log>...`.
    pub fn parse(text: &str) -> Result<Self, String> {
        let csv = parse_csv(text).map_err(|e| format!("census: {e}"))?;
        let (id_c, name_c) = (csv.col("id")?, csv.col("name")?);
        let logs: Vec<String> = csv.header.get(2..).unwrap_or(&[]).to_vec();
        if logs.is_empty() {
            return Err("census: no log columns after id,name".into());
        }
        let mut counts = HashMap::new();
        for row in &csv.rows {
            let id: u32 = cell(row, id_c, "census")?
                .parse()
                .map_err(|_| format!("census: bad id {:?}", row.first()))?;
            let name = cell(row, name_c, "census")?.to_string();
            let mut per_log = Vec::with_capacity(logs.len());
            for c in 2..2 + logs.len() {
                let v = cell(row, c, "census")?;
                per_log.push(
                    v.parse()
                        .map_err(|_| format!("census: bad count {v:?} for {id}"))?,
                );
            }
            if counts.insert(id, (name, per_log)).is_some() {
                return Err(format!("census: duplicate id {id}"));
            }
        }
        Ok(Self { logs, counts })
    }
}

#[derive(Debug)]
pub struct Generated {
    /// `role_spells.rs`.
    pub content: String,
    /// `role_spells.expected.md`.
    pub expected: String,
    pub spells: usize,
}

/// One cell of a CSV row. The column index comes from `Csv::col`, so a miss
/// means the row itself is short — a malformed table, not a bug here.
fn cell<'a>(row: &'a [String], c: usize, what: &str) -> Result<&'a str, String> {
    row.get(c)
        .map(String::as_str)
        .ok_or_else(|| format!("{what}: row has no column {c}"))
}

pub fn generate(
    tables: &HashMap<&str, Csv>,
    census: &Census,
    build: &str,
) -> Result<Generated, String> {
    generate_curated(CURATED, tables, census, build)
}

/// What the review twin prints beside an entry: its census counts, or the
/// reason it ships without any.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Evidence<'a> {
    Counts(&'a [u64]),
    Exempt(&'a str),
}

/// The generator over an explicit list, so tests can run it on a synthetic
/// handful instead of the real sixty.
pub fn generate_curated(
    curated: &[Curated],
    tables: &HashMap<&str, Csv>,
    census: &Census,
    build: &str,
) -> Result<Generated, String> {
    let get = |name: &str| {
        tables
            .get(name)
            .ok_or_else(|| format!("missing table {name}"))
    };
    let wanted: HashSet<u32> = curated.iter().map(|e| e.id).collect();
    if wanted.len() != curated.len() {
        return Err("curated list holds a duplicate id".into());
    }

    // SpellName: id -> name, for the curated ids only.
    let sn = get("SpellName")?;
    let (n_id, n_name) = (sn.col("ID")?, sn.col("Name_lang")?);
    let mut names: HashMap<u32, &str> = HashMap::new();
    for row in &sn.rows {
        let id: u32 = cell(row, n_id, "SpellName")?.parse().unwrap_or(0);
        if wanted.contains(&id) {
            names.insert(id, cell(row, n_name, "SpellName")?);
        }
    }

    // SpellEffect: the curated ids with an APPLY_AURA effect on any difficulty.
    let se = get("SpellEffect")?;
    let (e_spell, e_effect) = (se.col("SpellID")?, se.col("Effect")?);
    let mut applies_aura: HashSet<u32> = HashSet::new();
    for row in &se.rows {
        if cell(row, e_effect, "SpellEffect")? != EFFECT_APPLY_AURA {
            continue;
        }
        let id: u32 = cell(row, e_spell, "SpellEffect")?.parse().unwrap_or(0);
        if wanted.contains(&id) {
            applies_aura.insert(id);
        }
    }

    let mut table: BTreeMap<u32, (&str, RoleSpellKind, Evidence<'_>)> = BTreeMap::new();
    for &Curated {
        id,
        name: expected,
        kind,
        census_exempt,
    } in curated
    {
        match names.get(&id) {
            None => return Err(format!("role spell {id} ({expected}): no SpellName row")),
            Some(actual) if *actual != expected => {
                return Err(format!(
                    "role spell {id}: SpellName says {actual:?}, curated as {expected:?}"
                ));
            }
            Some(_) => {}
        }
        if !applies_aura.contains(&id) {
            return Err(format!(
                "role spell {id} ({expected}): no APPLY_AURA SpellEffect row — a cast id, \
                 not the buff the log applies?"
            ));
        }
        // The census: required, unless the entry carries a waiver — and
        // even then a census row that does exist must agree on the name,
        // and its counts are the evidence printed (a waiver never hides
        // observations).
        let evidence = match (census.counts.get(&id), census_exempt) {
            (None, None) => {
                return Err(format!(
                    "role spell {id} ({expected}): never applied to a player in the census"
                ));
            }
            (Some((log_name, _)), _) if log_name != expected => {
                return Err(format!(
                    "role spell {id}: the census names it {log_name:?}, curated as {expected:?}"
                ));
            }
            (Some((_, per_log)), reason) if per_log.iter().all(|&n| n == 0) => match reason {
                Some(r) => Evidence::Exempt(r),
                None => {
                    return Err(format!(
                        "role spell {id} ({expected}): census row is all zeroes"
                    ));
                }
            },
            (Some((_, per_log)), _) => Evidence::Counts(per_log.as_slice()),
            (None, Some(reason)) => Evidence::Exempt(reason),
        };
        table.insert(id, (expected, kind, evidence));
    }

    let entries: Vec<(u32, u8)> = table.iter().map(|(id, e)| (*id, e.1.code())).collect();
    Ok(Generated {
        spells: entries.len(),
        content: emit(&entries, build)?,
        expected: emit_expected(&table, &census.logs, build)?,
    })
}

fn emit(table: &[(u32, u8)], build: &str) -> Result<String, String> {
    let mut o = String::new();
    o.push_str("//! GENERATED by tools/gen-role-spells.sh — do not edit by hand.\n");
    // No timestamp: same build in, same bytes out.
    writeln!(
        o,
        "//! Source: local client DB2s via wowdps-extract, build {build}."
    )
    .map_err(|e| format!("emit: {e}"))?;
    writeln!(o, "//! {} role spells.", table.len()).map_err(|e| format!("emit: {e}"))?;
    o.push_str(
        "//!\n\
         //! Maps a combat-log aura id to the role its buff plays — active mitigation,\n\
         //! a defensive, an external, a support buff or an offensive cooldown — so\n\
         //! the meter can open a span on the buff's target with its caster\n\
         //! (CONTRACT.md R18). Membership is curated in tools/extract/src/rolegen.rs;\n\
         //! every entry's name and APPLY_AURA effect are proven against the client,\n\
         //! and its real-log census sits in role_spells.expected.md.\n\
         \n\
         use wowdps_model::RoleSpellKind;\n\
         \n\
         /// The role a buff plays, or `None` for a spell the table does not curate.\n\
         #[allow(dead_code)] // consumed by the meter's R18 spans (4a-ii)\n\
         pub(crate) fn role_kind(spell_id: u32) -> Option<RoleSpellKind> {\n\
         \x20   let i = TABLE.binary_search_by_key(&spell_id, |e| e.0).ok()?;\n\
         \x20   let &(_, code) = TABLE.get(i)?;\n\
         \x20   KINDS.get(code as usize).copied()\n\
         }\n\
         \n\
         const KINDS: [RoleSpellKind; 5] = [\n",
    );
    for kind in KIND_ORDER {
        writeln!(o, "    RoleSpellKind::{kind:?},").map_err(|e| format!("emit: {e}"))?;
    }
    o.push_str(
        "];\n\
         \n\
         /// (aura id, kind code), sorted by aura id.\n\
         #[rustfmt::skip]\n\
         static TABLE: &[(u32, u8)] = &[\n",
    );
    for chunk in table.chunks(8) {
        let cells: Vec<String> = chunk.iter().map(|(s, k)| format!("({s},{k}),")).collect();
        writeln!(o, "    {}", cells.join(" ")).map_err(|e| format!("emit: {e}"))?;
    }
    o.push_str(
        "];\n\
         \n\
         #[cfg(test)]\n\
         mod tests {\n\
         \x20   /// Strictly ascending: binary search demands it, and it doubles as\n\
         \x20   /// a dedup check.\n\
         \x20   #[test]\n\
         \x20   fn table_is_sorted_by_spell_id() {\n\
         \x20       assert!(super::TABLE.windows(2).all(|w| w[0].0 < w[1].0));\n\
         \x20   }\n\
         \n\
         \x20   /// Every code in the table names a kind, and a stranger is `None`.\n\
         \x20   #[test]\n\
         \x20   fn every_entry_resolves() {\n\
         \x20       assert!(super::TABLE.iter().all(|e| super::role_kind(e.0).is_some()));\n\
         \x20       assert_eq!(super::role_kind(0), None);\n\
         \x20   }\n\
         }\n",
    );
    Ok(o)
}

fn emit_expected(
    table: &BTreeMap<u32, (&str, RoleSpellKind, Evidence<'_>)>,
    logs: &[String],
    build: &str,
) -> Result<String, String> {
    let mut o = String::new();
    let w = |o: &mut String, s: &str| -> Result<(), String> {
        writeln!(o, "{s}").map_err(|e| format!("emit: {e}"))
    };
    w(
        &mut o,
        "# role_spells.rs — the curated table and its evidence",
    )?;
    w(&mut o, "")?;
    w(
        &mut o,
        "GENERATED by tools/gen-role-spells.sh beside `role_spells.rs` — do not edit by hand.",
    )?;
    writeln!(o, "Build {build}, {} role spells.", table.len()).map_err(|e| format!("emit: {e}"))?;
    w(&mut o, "")?;
    w(&mut o, "## Rules")?;
    w(&mut o, "")?;
    w(
        &mut o,
        "- Membership is curated in `tools/extract/src/rolegen.rs` as `(aura id, name, kind)`\n\
         \x20 — the id is the AURA the log names on `SPELL_AURA_APPLIED`, never the cast's\n\
         \x20 (Blur is 212800, not 198589; Metamorphosis 162264, not 191427).\n\
         - Every entry is proven against the client: `SpellName.Name_lang` must equal the\n\
         \x20 curated name, and `SpellEffect` must hold an `Effect == 6` (APPLY_AURA) row for\n\
         \x20 the id on some difficulty. Either miss fails the build, naming the id.\n\
         - Every entry must be exercised by the committed census `tools/role-spells-census.csv`\n\
         \x20 (`tools/census-role-spells.sh <log>...`: every `SPELL_AURA_APPLIED` whose target\n\
         \x20 is a `Player-` guid and whose aura type is `BUFF`, counted per (id, name) per\n\
         \x20 log), under the same name. A curated id nobody has seen on a player is a build\n\
         \x20 failure — the coach requests an id, the census proves it, then it ships — unless\n\
         \x20 the entry carries a `census_exempt` reason (`Curated::exempt`): name and\n\
         \x20 APPLY_AURA are still proven, and the reason is printed below in place of the\n\
         \x20 counts (`exempt: …`) so the waiver stands beside the evidence it replaces.\n\
         - No class/spec gate: an external lands on its target regardless of class, and\n\
         \x20 nothing reads one. Kinds: `active_mitigation` (a tank's rotational mitigation),\n\
         \x20 `defensive` (a personal damage-reduction cooldown), `external` (a buff cast on\n\
         \x20 someone else), `support_buff` (a buff whose value is the target's output),\n\
         \x20 `cooldown` (a major offensive cooldown's own buff).\n\
         - The engine (CONTRACT.md R18) opens a span keyed by the target with the caster as\n\
         \x20 `src` on a Buff apply/refresh of a curated id, and closes it on the removal.",
    )?;
    w(&mut o, "")?;
    w(&mut o, "## Entries")?;
    w(&mut o, "")?;
    let mut header = String::from("| id | name | kind |");
    let mut rule = String::from("| ---: | --- | --- |");
    for log in logs {
        write!(header, " observed ({log}) |").map_err(|e| format!("emit: {e}"))?;
        rule.push_str(" ---: |");
    }
    w(&mut o, &header)?;
    w(&mut o, &rule)?;
    // Grouped by kind (the reviewer's order), then by id.
    for kind in KIND_ORDER {
        for (id, (name, k, evidence)) in table {
            if *k != kind {
                continue;
            }
            let mut line = format!("| {id} | {name} | {} |", kind.name());
            match evidence {
                Evidence::Counts(per_log) => {
                    for n in *per_log {
                        write!(line, " {n} |").map_err(|e| format!("emit: {e}"))?;
                    }
                }
                // The reason in the first observed column, the rest dashed.
                Evidence::Exempt(reason) => {
                    write!(line, " exempt: {reason} |").map_err(|e| format!("emit: {e}"))?;
                    for _ in logs.iter().skip(1) {
                        line.push_str(" — |");
                    }
                }
            }
            w(&mut o, &line)?;
        }
    }
    Ok(o)
}

#[cfg(test)]
mod tests {
    use super::*;

    const GOOD: &[Curated] = &[
        Curated::seen(871, "Shield Wall", RoleSpellKind::Defensive),
        Curated::seen(10060, "Power Infusion", RoleSpellKind::External),
        Curated::seen(132404, "Shield Block", RoleSpellKind::ActiveMitigation),
    ];

    fn tables() -> HashMap<&'static str, Csv> {
        let mut t = HashMap::new();
        t.insert(
            "SpellName",
            parse_csv(
                "ID,Name_lang\n\
                 871,Shield Wall\n\
                 10060,Power Infusion\n\
                 132404,Shield Block\n\
                 191427,Metamorphosis\n\
                 5000,Old Name\n",
            )
            .unwrap(),
        );
        // Shield Wall has an aura on two difficulties; Metamorphosis (the
        // cast) only a dummy effect; 5000 an aura under a stale name.
        t.insert(
            "SpellEffect",
            parse_csv(
                "ID,DifficultyID,Effect,EffectAura,SpellID\n\
                 1,0,6,22,871\n\
                 2,1,6,22,871\n\
                 3,0,6,4,10060\n\
                 4,0,6,4,132404\n\
                 5,0,3,0,191427\n\
                 6,0,6,4,5000\n",
            )
            .unwrap(),
        );
        t
    }

    fn census() -> Census {
        Census::parse(
            "id,name,raid.txt,dummy.txt\n\
             871,\"Shield Wall\",56,0\n\
             10060,\"Power Infusion\",147,14\n\
             132404,\"Shield Block\",371,0\n\
             191427,\"Metamorphosis\",0,3\n\
             5000,\"New Name\",1,1\n\
             6000,\"Never Curated\",9,9\n",
        )
        .unwrap()
    }

    #[test]
    fn good_entries_emit_their_lines_and_counts() {
        let g = generate_curated(GOOD, &tables(), &census(), "1.2.3.4").unwrap();
        assert_eq!(g.spells, 3);
        assert!(
            g.content.contains("(871,1), (10060,2), (132404,0),"),
            "{}",
            g.content
        );
        assert!(g.content.contains("build 1.2.3.4"));
        assert!(g.content.contains("//! 3 role spells."));
        assert!(g.content.contains("pub(crate) fn role_kind"));
        assert!(
            g.expected
                .contains("| observed (raid.txt) | observed (dummy.txt) |")
        );
        assert!(
            g.expected
                .contains("| 132404 | Shield Block | active_mitigation | 371 | 0 |")
        );
        assert!(
            g.expected
                .contains("| 10060 | Power Infusion | external | 147 | 14 |")
        );
        // Kind order in the review file, id order in the table.
        let am = g.expected.find("| 132404 |").unwrap();
        let def = g.expected.find("| 871 |").unwrap();
        assert!(am < def);
        assert!(!g.expected.contains("6000"));
    }

    #[test]
    fn renamed_spell_fails_naming_the_id() {
        let list = [Curated::seen(5000, "Old Name", RoleSpellKind::Cooldown)];
        // The client still says "Old Name" but the census (the log) says "New
        // Name": the curated name is stale on one side.
        let err = generate_curated(&list, &tables(), &census(), "b").unwrap_err();
        assert!(err.contains("5000") && err.contains("New Name"), "{err}");

        let list = [Curated::seen(
            871,
            "Shield Wall II",
            RoleSpellKind::Defensive,
        )];
        let err = generate_curated(&list, &tables(), &census(), "b").unwrap_err();
        assert!(
            err.contains("871") && err.contains("Shield Wall II"),
            "{err}"
        );

        let list = [Curated::seen(7000, "Unknown", RoleSpellKind::Defensive)];
        let err = generate_curated(&list, &tables(), &census(), "b").unwrap_err();
        assert!(
            err.contains("7000") && err.contains("no SpellName"),
            "{err}"
        );
    }

    #[test]
    fn cast_id_without_apply_aura_fails() {
        let list = [Curated::seen(
            191427,
            "Metamorphosis",
            RoleSpellKind::Cooldown,
        )];
        let err = generate_curated(&list, &tables(), &census(), "b").unwrap_err();
        assert!(
            err.contains("191427") && err.contains("APPLY_AURA"),
            "{err}"
        );
    }

    #[test]
    fn unobserved_id_fails() {
        let mut t = tables();
        t.get_mut("SpellName")
            .unwrap()
            .rows
            .push(vec!["8000".into(), "Ghost".into()]);
        t.get_mut("SpellEffect").unwrap().rows.push(vec![
            "9".into(),
            "0".into(),
            "6".into(),
            "4".into(),
            "8000".into(),
        ]);
        let list = [Curated::seen(8000, "Ghost", RoleSpellKind::Defensive)];
        let err = generate_curated(&list, &t, &census(), "b").unwrap_err();
        assert!(err.contains("8000") && err.contains("census"), "{err}");

        let zero = Census::parse("id,name,a.txt\n8000,\"Ghost\",0\n").unwrap();
        let err = generate_curated(&list, &t, &zero, "b").unwrap_err();
        assert!(err.contains("8000") && err.contains("zero"), "{err}");
    }

    /// A `census_exempt` reason waives the census (absent row, or a row of
    /// zeroes) — the name and APPLY_AURA checks still bind — and the reason is
    /// what the review twin prints; a census row that does exist keeps its
    /// counts and its name check.
    #[test]
    fn exempt_entry_ships_without_a_census_row() {
        let mut t = tables();
        t.get_mut("SpellName")
            .unwrap()
            .rows
            .push(vec!["8000".into(), "Ghost".into()]);
        t.get_mut("SpellEffect").unwrap().rows.push(vec![
            "9".into(),
            "0".into(),
            "6".into(),
            "4".into(),
            "8000".into(),
        ]);
        let list = [Curated::exempt(
            8000,
            "Ghost",
            RoleSpellKind::External,
            "a waiver",
        )];
        let g = generate_curated(&list, &t, &census(), "b").unwrap();
        assert_eq!(g.spells, 1);
        assert!(g.content.contains("(8000,2),"), "{}", g.content);
        assert!(
            g.expected
                .contains("| 8000 | Ghost | external | exempt: a waiver | — |"),
            "{}",
            g.expected
        );
        let zero = Census::parse("id,name,a.txt\n8000,\"Ghost\",0\n").unwrap();
        let g = generate_curated(&list, &t, &zero, "b").unwrap();
        assert!(
            g.expected.contains("| exempt: a waiver |"),
            "{}",
            g.expected
        );
        // A census row that exists still has to agree on the name …
        let renamed = Census::parse("id,name,a.txt\n8000,\"Spectre\",3\n").unwrap();
        let err = generate_curated(&list, &t, &renamed, "b").unwrap_err();
        assert!(err.contains("8000") && err.contains("Spectre"), "{err}");
        // … and its counts are what gets printed, not the waiver.
        let seen = Census::parse("id,name,a.txt\n8000,\"Ghost\",3\n").unwrap();
        let g = generate_curated(&list, &t, &seen, "b").unwrap();
        assert!(
            g.expected.contains("| 8000 | Ghost | external | 3 |"),
            "{}",
            g.expected
        );
        // The waiver does not reach the client checks.
        let list = [Curated::exempt(
            191427,
            "Metamorphosis",
            RoleSpellKind::Cooldown,
            "a waiver",
        )];
        let err = generate_curated(&list, &t, &census(), "b").unwrap_err();
        assert!(err.contains("APPLY_AURA"), "{err}");
        // The three hunter lusts ship exempt in the real list.
        for id in [90355, 160452, 466904] {
            let e = CURATED.iter().find(|e| e.id == id).unwrap();
            assert_eq!(e.kind, RoleSpellKind::External);
            assert!(e.census_exempt.is_some());
        }
    }

    #[test]
    fn duplicate_curated_id_fails() {
        let list = [
            Curated::seen(871, "Shield Wall", RoleSpellKind::Defensive),
            Curated::seen(871, "Shield Wall", RoleSpellKind::Cooldown),
        ];
        let err = generate_curated(&list, &tables(), &census(), "b").unwrap_err();
        assert!(err.contains("duplicate"), "{err}");
    }

    #[test]
    fn table_is_sorted_and_deterministic() {
        // Curated out of id order; the table comes back sorted.
        let list = [
            Curated::seen(132404, "Shield Block", RoleSpellKind::ActiveMitigation),
            Curated::seen(871, "Shield Wall", RoleSpellKind::Defensive),
            Curated::seen(10060, "Power Infusion", RoleSpellKind::External),
        ];
        let a = generate_curated(&list, &tables(), &census(), "b").unwrap();
        let ids: Vec<u32> = a
            .content
            .split('(')
            .filter_map(|s| s.split(',').next()?.parse().ok())
            .collect();
        assert!(ids.windows(2).all(|w| w[0] < w[1]), "{ids:?}");
        let b = generate_curated(&list, &tables(), &census(), "b").unwrap();
        assert_eq!(a.content, b.content);
        assert_eq!(a.expected, b.expected);
    }

    #[test]
    fn census_parser_rejects_shapes_it_cannot_read() {
        assert!(
            Census::parse("id,name\n1,\"x\"\n").is_err(),
            "no log columns"
        );
        assert!(
            Census::parse("id,name,a\n1,\"x\",1\n1,\"x\",2\n").is_err(),
            "duplicate"
        );
        assert!(Census::parse("id,name,a\nx,\"x\",1\n").is_err(), "bad id");
        assert!(
            Census::parse("id,name,a\n1,\"x\",many\n").is_err(),
            "bad count"
        );
        let c = Census::parse("id,name,a,b\n1,\"x, y\",3,4\n").unwrap();
        assert_eq!(c.logs, ["a", "b"]);
        assert_eq!(c.counts[&1], ("x, y".to_string(), vec![3, 4]));
    }

    /// The real list is what ships: unique ids, and a kind for every code.
    #[test]
    fn curated_list_is_well_formed() {
        let ids: HashSet<u32> = CURATED.iter().map(|e| e.id).collect();
        assert_eq!(ids.len(), CURATED.len());
        for (i, k) in KIND_ORDER.iter().enumerate() {
            assert_eq!(k.code() as usize, i);
        }
        assert!(CURATED.iter().all(|e| !e.name.is_empty()));
        assert!(
            CURATED
                .iter()
                .all(|e| e.census_exempt.is_none_or(|r| !r.is_empty()))
        );
    }
}
