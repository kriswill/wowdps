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
//!   ship; a guess is a build failure, not a silent zero.
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

/// The table. Every id is the AURA the combat log applies to a player, with
/// the name `SpellName` must carry; the census counts beside each entry
/// live in `role_spells.expected.md`. Grouped by kind, then by class, for
/// review — the generator sorts by id.
const CURATED: &[(u32, &str, RoleSpellKind)] = &[
    // -- ActiveMitigation: a tank's rotational mitigation buff ---------------
    (132404, "Shield Block", RoleSpellKind::ActiveMitigation),
    (
        132403,
        "Shield of the Righteous",
        RoleSpellKind::ActiveMitigation,
    ),
    (192081, "Ironfur", RoleSpellKind::ActiveMitigation),
    (203819, "Demon Spikes", RoleSpellKind::ActiveMitigation),
    (195181, "Bone Shield", RoleSpellKind::ActiveMitigation),
    (215479, "Shuffle", RoleSpellKind::ActiveMitigation),
    (77535, "Blood Shield", RoleSpellKind::ActiveMitigation),
    // -- Defensive: a personal damage-reduction cooldown --------------------
    (871, "Shield Wall", RoleSpellKind::Defensive),
    (86659, "Guardian of Ancient Kings", RoleSpellKind::Defensive),
    (48792, "Icebound Fortitude", RoleSpellKind::Defensive),
    (81256, "Dancing Rune Weapon", RoleSpellKind::Defensive),
    // The buff, not the cast (115203).
    (120954, "Fortifying Brew", RoleSpellKind::Defensive),
    (61336, "Survival Instincts", RoleSpellKind::Defensive),
    (47585, "Dispersion", RoleSpellKind::Defensive),
    (363916, "Obsidian Scales", RoleSpellKind::Defensive),
    (186265, "Aspect of the Turtle", RoleSpellKind::Defensive),
    (5277, "Evasion", RoleSpellKind::Defensive),
    // The buff, not the cast (198589).
    (212800, "Blur", RoleSpellKind::Defensive),
    (108271, "Astral Shift", RoleSpellKind::Defensive),
    (104773, "Unending Resolve", RoleSpellKind::Defensive),
    (342246, "Alter Time", RoleSpellKind::Defensive),
    // Raid-wide from one warrior: a span on every member, caster = the warrior.
    (97463, "Rallying Cry", RoleSpellKind::Defensive),
    (145629, "Anti-Magic Zone", RoleSpellKind::Defensive),
    // The per-player aura the totem applies (cast 98008); its caster is the
    // totem creature, not the shaman.
    (325174, "Spirit Link Totem", RoleSpellKind::Defensive),
    (31821, "Aura Mastery", RoleSpellKind::Defensive),
    // -- External: a buff cast on someone else ------------------------------
    (2825, "Bloodlust", RoleSpellKind::External),
    (32182, "Heroism", RoleSpellKind::External),
    (80353, "Time Warp", RoleSpellKind::External),
    (264667, "Primal Rage", RoleSpellKind::External),
    (390386, "Fury of the Aspects", RoleSpellKind::External),
    (10060, "Power Infusion", RoleSpellKind::External),
    (33206, "Pain Suppression", RoleSpellKind::External),
    (47788, "Guardian Spirit", RoleSpellKind::External),
    (102342, "Ironbark", RoleSpellKind::External),
    (116849, "Life Cocoon", RoleSpellKind::External),
    (6940, "Blessing of Sacrifice", RoleSpellKind::External),
    (1022, "Blessing of Protection", RoleSpellKind::External),
    (29166, "Innervate", RoleSpellKind::External),
    (357170, "Time Dilation", RoleSpellKind::External),
    // The rescued ally's buff (cast 370665; 370666 is the evoker's own). The
    // log writes its source as the target, so the caster is lost here.
    (370667, "Rescue", RoleSpellKind::External),
    // -- SupportBuff: a buff whose value is the target's output --------------
    // The ally-side aura; 395296 is the evoker's own Ebon Might.
    (395152, "Ebon Might", RoleSpellKind::SupportBuff),
    (410089, "Prescience", RoleSpellKind::SupportBuff),
    (413984, "Shifting Sands", RoleSpellKind::SupportBuff),
    // -- Cooldown: a major offensive cooldown's own buff ---------------------
    // Havoc's buff (cast 191427); Devourer's is "Void Metamorphosis".
    (162264, "Metamorphosis", RoleSpellKind::Cooldown),
    (107574, "Avatar", RoleSpellKind::Cooldown),
    (190319, "Combustion", RoleSpellKind::Cooldown),
    (365362, "Arcane Surge", RoleSpellKind::Cooldown),
    (375087, "Dragonrage", RoleSpellKind::Cooldown),
    (114051, "Ascendance", RoleSpellKind::Cooldown),
    (114052, "Ascendance", RoleSpellKind::Cooldown),
    // The buff, not the cast (227847).
    (446035, "Bladestorm", RoleSpellKind::Cooldown),
    (1719, "Recklessness", RoleSpellKind::Cooldown),
    (121471, "Shadow Blades", RoleSpellKind::Cooldown),
    (194249, "Voidform", RoleSpellKind::Cooldown),
    (
        102560,
        "Incarnation: Chosen of Elune",
        RoleSpellKind::Cooldown,
    ),
    (19574, "Bestial Wrath", RoleSpellKind::Cooldown),
    (288613, "Trueshot", RoleSpellKind::Cooldown),
    (51271, "Pillar of Frost", RoleSpellKind::Cooldown),
    (42650, "Army of the Dead", RoleSpellKind::Cooldown),
    (31884, "Avenging Wrath", RoleSpellKind::Cooldown),
    (274837, "Feral Frenzy", RoleSpellKind::Cooldown),
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

/// The generator over an explicit list, so tests can run it on a synthetic
/// handful instead of the real sixty.
pub fn generate_curated(
    curated: &[(u32, &str, RoleSpellKind)],
    tables: &HashMap<&str, Csv>,
    census: &Census,
    build: &str,
) -> Result<Generated, String> {
    let get = |name: &str| {
        tables
            .get(name)
            .ok_or_else(|| format!("missing table {name}"))
    };
    let wanted: HashSet<u32> = curated.iter().map(|e| e.0).collect();
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

    let mut table: BTreeMap<u32, (&str, RoleSpellKind, &[u64])> = BTreeMap::new();
    for &(id, expected, kind) in curated {
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
        let Some((log_name, per_log)) = census.counts.get(&id) else {
            return Err(format!(
                "role spell {id} ({expected}): never applied to a player in the census"
            ));
        };
        if log_name != expected {
            return Err(format!(
                "role spell {id}: the census names it {log_name:?}, curated as {expected:?}"
            ));
        }
        if per_log.iter().all(|&n| n == 0) {
            return Err(format!(
                "role spell {id} ({expected}): census row is all zeroes"
            ));
        }
        table.insert(id, (expected, kind, per_log.as_slice()));
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
    table: &BTreeMap<u32, (&str, RoleSpellKind, &[u64])>,
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
         \x20 failure — the coach requests an id, the census proves it, then it ships.\n\
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
        for (id, (name, k, per_log)) in table {
            if *k != kind {
                continue;
            }
            let mut line = format!("| {id} | {name} | {} |", kind.name());
            for n in *per_log {
                write!(line, " {n} |").map_err(|e| format!("emit: {e}"))?;
            }
            w(&mut o, &line)?;
        }
    }
    Ok(o)
}

#[cfg(test)]
mod tests {
    use super::*;

    const GOOD: &[(u32, &str, RoleSpellKind)] = &[
        (871, "Shield Wall", RoleSpellKind::Defensive),
        (10060, "Power Infusion", RoleSpellKind::External),
        (132404, "Shield Block", RoleSpellKind::ActiveMitigation),
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
        let list = [(5000, "Old Name", RoleSpellKind::Cooldown)];
        // The client still says "Old Name" but the census (the log) says "New
        // Name": the curated name is stale on one side.
        let err = generate_curated(&list, &tables(), &census(), "b").unwrap_err();
        assert!(err.contains("5000") && err.contains("New Name"), "{err}");

        let list = [(871, "Shield Wall II", RoleSpellKind::Defensive)];
        let err = generate_curated(&list, &tables(), &census(), "b").unwrap_err();
        assert!(
            err.contains("871") && err.contains("Shield Wall II"),
            "{err}"
        );

        let list = [(7000, "Unknown", RoleSpellKind::Defensive)];
        let err = generate_curated(&list, &tables(), &census(), "b").unwrap_err();
        assert!(
            err.contains("7000") && err.contains("no SpellName"),
            "{err}"
        );
    }

    #[test]
    fn cast_id_without_apply_aura_fails() {
        let list = [(191427, "Metamorphosis", RoleSpellKind::Cooldown)];
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
        let list = [(8000, "Ghost", RoleSpellKind::Defensive)];
        let err = generate_curated(&list, &t, &census(), "b").unwrap_err();
        assert!(err.contains("8000") && err.contains("census"), "{err}");

        let zero = Census::parse("id,name,a.txt\n8000,\"Ghost\",0\n").unwrap();
        let err = generate_curated(&list, &t, &zero, "b").unwrap_err();
        assert!(err.contains("8000") && err.contains("zero"), "{err}");
    }

    #[test]
    fn duplicate_curated_id_fails() {
        let list = [
            (871, "Shield Wall", RoleSpellKind::Defensive),
            (871, "Shield Wall", RoleSpellKind::Cooldown),
        ];
        let err = generate_curated(&list, &tables(), &census(), "b").unwrap_err();
        assert!(err.contains("duplicate"), "{err}");
    }

    #[test]
    fn table_is_sorted_and_deterministic() {
        // Curated out of id order; the table comes back sorted.
        let list = [
            (132404, "Shield Block", RoleSpellKind::ActiveMitigation),
            (871, "Shield Wall", RoleSpellKind::Defensive),
            (10060, "Power Infusion", RoleSpellKind::External),
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
        let ids: HashSet<u32> = CURATED.iter().map(|e| e.0).collect();
        assert_eq!(ids.len(), CURATED.len());
        for (i, k) in KIND_ORDER.iter().enumerate() {
            assert_eq!(k.code() as usize, i);
        }
        assert!(CURATED.iter().all(|e| !e.1.is_empty()));
    }
}
