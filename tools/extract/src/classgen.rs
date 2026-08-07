//! The `crates/core/src/class_spells.rs` generator: a Rust port of the
//! retired `tools/gen-class-spells.py`, consuming the same eight tables
//! (now decoded from the local install instead of downloaded from
//! wago.tools) and emitting a byte-identical body, so regenerating on an
//! unchanged build is a no-op. Attribution rules (CONTRACT.md R8):
//!
//!   * SkillLineAbility rows on the 13 class skill lines -> (class, no spec)
//!   * SpecializationSpells                              -> (class, spec)
//!   * talent spells via the trait chain TraitDefinition ->
//!     TraitNodeEntry -> TraitNodeXTraitNodeEntry -> TraitNode ->
//!     TraitTreeLoadout(spec)
//!
//! A spell attributed to more than one class is dropped (not class
//! evidence). A spell keeps a spec only when spec sources name exactly one
//! spec AND no class-wide source (class skill line) also grants it.

use crate::table::Csv;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::fmt::Write as _;

/// The tables the generator consumes, with their FileDataIDs
/// (from wowdev/wow-listfile; stable per file, forever).
pub const TABLES: [(&str, u32); 8] = [
    ("SkillLine", 1240935),
    ("SkillLineAbility", 1266278),
    ("SpecializationSpells", 1240335),
    ("TraitDefinition", 4420327),
    ("TraitNodeEntry", 4420298),
    ("TraitNodeXTraitNodeEntry", 4420304),
    ("TraitNode", 4420297),
    ("TraitTreeLoadout", 4669507),
];

/// Class skill lines are matched by DisplayName so a renumbering patch
/// fails loudly here instead of silently shrinking the table. CategoryID 7
/// also holds pet skill lines ("Pet - ..."), which must stay excluded: pet
/// spells are not player class evidence.
const CLASS_SKILL_NAMES: [(&str, &str); 13] = [
    ("Warrior", "Warrior"),
    ("Paladin", "Paladin"),
    ("Hunter", "Hunter"),
    ("Rogue", "Rogue"),
    ("Priest", "Priest"),
    ("Death Knight", "DeathKnight"),
    ("Shaman", "Shaman"),
    ("Mage", "Mage"),
    ("Warlock", "Warlock"),
    ("Monk", "Monk"),
    ("Druid", "Druid"),
    ("Demon Hunter", "DemonHunter"),
    ("Evoker", "Evoker"),
];

/// Mirrors wowdps_model::Spec::from_id / Spec::class.
const SPEC_CLASS: [(u16, &str); 39] = [
    (71, "Warrior"),
    (72, "Warrior"),
    (73, "Warrior"),
    (65, "Paladin"),
    (66, "Paladin"),
    (70, "Paladin"),
    (253, "Hunter"),
    (254, "Hunter"),
    (255, "Hunter"),
    (259, "Rogue"),
    (260, "Rogue"),
    (261, "Rogue"),
    (256, "Priest"),
    (257, "Priest"),
    (258, "Priest"),
    (250, "DeathKnight"),
    (251, "DeathKnight"),
    (252, "DeathKnight"),
    (262, "Shaman"),
    (263, "Shaman"),
    (264, "Shaman"),
    (62, "Mage"),
    (63, "Mage"),
    (64, "Mage"),
    (265, "Warlock"),
    (266, "Warlock"),
    (267, "Warlock"),
    (268, "Monk"),
    (269, "Monk"),
    (270, "Monk"),
    (102, "Druid"),
    (103, "Druid"),
    (104, "Druid"),
    (105, "Druid"),
    (577, "DemonHunter"),
    (581, "DemonHunter"),
    (1467, "Evoker"),
    (1468, "Evoker"),
    (1473, "Evoker"),
];

/// Class code emitted into the table; must match the CLASSES array in the
/// generated file (which spells the variants out, so order here is
/// arbitrary but fixed).
const CLASS_ORDER: [&str; 13] = [
    "Warrior",
    "Paladin",
    "Hunter",
    "Rogue",
    "Priest",
    "DeathKnight",
    "Shaman",
    "Mage",
    "Warlock",
    "Monk",
    "Druid",
    "DemonHunter",
    "Evoker",
];

#[derive(Debug)]
pub struct Generated {
    pub content: String,
    pub spells: usize,
    pub spec_unique: usize,
    pub ambiguous: usize,
}

pub fn generate(tables: &HashMap<&str, Csv>, build: &str) -> Result<Generated, String> {
    let get = |name: &str| {
        tables
            .get(name)
            .ok_or_else(|| format!("missing table {name}"))
    };
    let spec_class: HashMap<u16, &str> = SPEC_CLASS.into_iter().collect();
    let class_code: HashMap<&str, u8> = CLASS_ORDER
        .iter()
        .enumerate()
        .map(|(i, &c)| (c, i as u8))
        .collect();

    // SkillLine id (as text) -> class name.
    let sl = get("SkillLine")?;
    let (id_c, cat_c, name_c) = (
        sl.col("ID")?,
        sl.col("CategoryID")?,
        sl.col("DisplayName_lang")?,
    );
    let mut class_lines: HashMap<&str, &str> = HashMap::new();
    for r in &sl.rows {
        if r[cat_c] == "7"
            && let Some(&(_, cls)) = CLASS_SKILL_NAMES.iter().find(|(n, _)| *n == r[name_c])
        {
            class_lines.insert(&r[id_c], cls);
        }
    }
    if class_lines.len() != 13 {
        return Err(format!(
            "expected 13 class skill lines, found {}: {class_lines:?}",
            class_lines.len()
        ));
    }

    // spell id -> classes / specs / class-wide flag.
    let mut classes: BTreeMap<u32, BTreeSet<&str>> = BTreeMap::new();
    let mut specs: HashMap<u32, BTreeSet<u16>> = HashMap::new();
    let mut classwide: HashSet<u32> = HashSet::new();
    let parse_u32 = |cell: &str| -> Result<u32, String> {
        cell.parse()
            .map_err(|_| format!("bad numeric cell {cell:?}"))
    };

    let sla = get("SkillLineAbility")?;
    let (line_c, spell_c) = (sla.col("SkillLine")?, sla.col("Spell")?);
    for r in &sla.rows {
        if let Some(&cls) = class_lines.get(r[line_c].as_str()) {
            let spell = parse_u32(&r[spell_c])?;
            classes.entry(spell).or_default().insert(cls);
            classwide.insert(spell);
        }
    }

    let ss = get("SpecializationSpells")?;
    let (spec_c, spell_c) = (ss.col("SpecID")?, ss.col("SpellID")?);
    for r in &ss.rows {
        let Ok(spec) = r[spec_c].parse::<u16>() else {
            continue;
        };
        if let Some(&cls) = spec_class.get(&spec) {
            let spell = parse_u32(&r[spell_c])?;
            classes.entry(spell).or_default().insert(cls);
            specs.entry(spell).or_default().insert(spec);
        }
    }

    // Trait chain: definition -> entries -> nodes -> trees -> specs.
    let td = get("TraitDefinition")?;
    let (id_c, spell_c) = (td.col("ID")?, td.col("SpellID")?);
    let def_spell: HashMap<&str, u32> = td
        .rows
        .iter()
        .filter(|r| r[spell_c] != "0")
        .map(|r| Ok((r[id_c].as_str(), parse_u32(&r[spell_c])?)))
        .collect::<Result<_, String>>()?;

    let te = get("TraitNodeEntry")?;
    let (id_c, def_c) = (te.col("ID")?, te.col("TraitDefinitionID")?);
    let entry_def: HashMap<&str, &str> = te
        .rows
        .iter()
        .map(|r| (r[id_c].as_str(), r[def_c].as_str()))
        .collect();

    let tx = get("TraitNodeXTraitNodeEntry")?;
    let (node_c, entry_c) = (tx.col("TraitNodeID")?, tx.col("TraitNodeEntryID")?);
    let mut node_entries: HashMap<&str, Vec<&str>> = HashMap::new();
    for r in &tx.rows {
        node_entries
            .entry(&r[node_c])
            .or_default()
            .push(&r[entry_c]);
    }

    let tn = get("TraitNode")?;
    let (id_c, tree_c) = (tn.col("ID")?, tn.col("TraitTreeID")?);
    let node_tree: HashMap<&str, &str> = tn
        .rows
        .iter()
        .map(|r| (r[id_c].as_str(), r[tree_c].as_str()))
        .collect();

    let tl = get("TraitTreeLoadout")?;
    let (tree_c, spec_c) = (tl.col("TraitTreeID")?, tl.col("ChrSpecializationID")?);
    let mut tree_specs: HashMap<&str, BTreeSet<u16>> = HashMap::new();
    for r in &tl.rows {
        let Ok(spec) = r[spec_c].parse::<u16>() else {
            continue;
        };
        if spec_class.contains_key(&spec) {
            tree_specs.entry(&r[tree_c]).or_default().insert(spec);
        }
    }

    for (node, entries) in &node_entries {
        let Some(specs_here) = node_tree.get(node).and_then(|t| tree_specs.get(t)) else {
            continue;
        };
        for &spec in specs_here {
            for entry in entries {
                if let Some(&spell) = entry_def.get(entry).and_then(|d| def_spell.get(d)) {
                    classes.entry(spell).or_default().insert(spec_class[&spec]);
                    specs.entry(spell).or_default().insert(spec);
                }
            }
        }
    }

    // Resolve: one class or drop; a spec only when unique and not
    // class-wide.
    let mut table: Vec<(u32, u8, u16)> = Vec::new();
    let mut ambiguous = 0;
    for (spell, cs) in &classes {
        if cs.len() != 1 {
            ambiguous += 1;
            continue;
        }
        let ss = specs.get(spell);
        let spec = match ss {
            Some(s) if s.len() == 1 && !classwide.contains(spell) => *s.iter().next().unwrap(),
            _ => 0,
        };
        table.push((*spell, class_code[cs.iter().next().unwrap()], spec));
    }

    let spec_unique = table.iter().filter(|(_, _, s)| *s != 0).count();
    Ok(Generated {
        content: emit(&table, ambiguous, build),
        spells: table.len(),
        spec_unique,
        ambiguous,
    })
}

fn emit(table: &[(u32, u8, u16)], ambiguous: usize, build: &str) -> String {
    let mut o = String::new();
    let speced = table.iter().filter(|(_, _, s)| *s != 0).count();
    o.push_str("//! GENERATED by tools/gen-class-spells.sh — do not edit by hand.\n");
    // No timestamp: same build in, same bytes out.
    writeln!(
        o,
        "//! Source: local client DB2s via wowdps-extract, build {build}."
    )
    .unwrap();
    writeln!(
        o,
        "//! {} spells ({speced} spec-unique); {ambiguous} multi-class ids dropped.",
        table.len()
    )
    .unwrap();
    o.push_str(
        "//!\n\
         //! Maps a combat-log spell id to the only class that can cast it, and — when\n\
         //! the spell is unique to one specialization — to that spec (CONTRACT.md R8).\n\
         \n\
         use wowdps_model::{Class, Spec};\n\
         \n\
         /// The class (and, when spec-unique, the spec) identified by a spell cast.\n\
         pub(crate) fn resolve(spell_id: u32) -> Option<(Class, Option<Spec>)> {\n\
         \x20   let i = TABLE.binary_search_by_key(&spell_id, |e| e.0).ok()?;\n\
         \x20   let (_, class_code, spec_id) = TABLE[i];\n\
         \x20   Some((CLASSES[class_code as usize], Spec::from_id(spec_id as u32)))\n\
         }\n\
         \n\
         const CLASSES: [Class; 13] = [\n",
    );
    for cls in CLASS_ORDER {
        writeln!(o, "    Class::{cls},").unwrap();
    }
    o.push_str(
        "];\n\
         \n\
         /// (spell id, class code, spec id or 0), sorted by spell id.\n\
         #[rustfmt::skip]\n\
         static TABLE: &[(u32, u8, u16)] = &[\n",
    );
    for chunk in table.chunks(6) {
        let cells: Vec<String> = chunk
            .iter()
            .map(|(s, c, p)| format!("({s},{c},{p}),"))
            .collect();
        writeln!(o, "    {}", cells.join(" ")).unwrap();
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
         }\n",
    );
    o
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::table::parse_csv;

    fn csv(text: &str) -> Csv {
        parse_csv(text).unwrap()
    }

    fn base_tables() -> HashMap<&'static str, Csv> {
        // All 13 class skill lines (ids 100..112), plus a pet line and an
        // unrelated category that must both be ignored.
        let mut skill_line = String::from("ID,CategoryID,DisplayName_lang\n");
        for (i, (display, _)) in CLASS_SKILL_NAMES.iter().enumerate() {
            skill_line.push_str(&format!("{},7,{display}\n", 100 + i));
        }
        skill_line.push_str("200,7,Pet - Ferocity\n201,9,Cooking\n");

        let mut t = HashMap::new();
        t.insert("SkillLine", csv(&skill_line));
        t.insert(
            "SkillLineAbility",
            // spell 1000 on the Warrior line; spell 2000 on both Warrior
            // and Mage lines (ambiguous); spell 3000 on the pet line only.
            csv("SkillLine,Spell\n100,1000\n100,2000\n107,2000\n200,3000\n"),
        );
        t.insert(
            "SpecializationSpells",
            // 4000: spec-unique (Arms). 5000: two Warrior specs -> class
            // only. 1000: spec source but class-wide wins -> no spec.
            csv("SpecID,SpellID\n71,4000\n71,5000\n72,5000\n71,1000\n999,6000\n"),
        );
        t.insert("TraitDefinition", csv("ID,SpellID\nd1,7000\nd2,0\n"));
        t.insert(
            "TraitNodeEntry",
            csv("ID,TraitDefinitionID\ne1,d1\ne2,d2\n"),
        );
        t.insert(
            "TraitNodeXTraitNodeEntry",
            csv("TraitNodeID,TraitNodeEntryID\nn1,e1\nn1,e2\n"),
        );
        t.insert("TraitNode", csv("ID,TraitTreeID\nn1,t1\n"));
        t.insert(
            "TraitTreeLoadout",
            csv("TraitTreeID,ChrSpecializationID\nt1,62\n"),
        );
        t
    }

    #[test]
    fn attribution_rules() {
        let g = generate(&base_tables(), "1.2.3.4").unwrap();
        assert_eq!((g.spells, g.spec_unique, g.ambiguous), (4, 2, 1));
        // 1000 Warrior class-wide; 4000 Warrior/Arms; 5000 Warrior only;
        // 7000 Mage/Arcane via the trait chain. 2000 dropped as ambiguous;
        // 3000 (pet) and 6000 (unknown spec) never attributed.
        assert!(
            g.content
                .contains("    (1000,0,0), (4000,0,71), (5000,0,0), (7000,7,62),\n")
        );
        assert!(g.content.contains("build 1.2.3.4"));
        assert!(
            g.content
                .contains("4 spells (2 spec-unique); 1 multi-class ids dropped")
        );
    }

    #[test]
    fn missing_class_line_fails_loudly() {
        let mut t = base_tables();
        t.insert(
            "SkillLine",
            csv("ID,CategoryID,DisplayName_lang\n100,7,Warrior\n"),
        );
        let err = generate(&t, "1.2.3.4").unwrap_err();
        assert!(err.contains("expected 13 class skill lines"), "{err}");
    }
}
