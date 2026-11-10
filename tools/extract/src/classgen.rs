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
//!
//! Those are Retail's rules ([`Rules::Specs`]). Forever's client names its
//! classes otherwise ([`Rules::Masks`], [`generate_masks`]): its class
//! skill lines are named for talent trees, each ability row carries its
//! class as a bit mask, and its specializations hold no spell. Which rules
//! a run reads by is the product's ([`Rules::for_product`]).

use crate::table::{Tables, cell};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::fmt::Write as _;
use wowdps_core::model::Edition;

/// The tables the generator consumes (their FileDataIDs: `table::FILE_IDS`):
/// every table either rules read ([`Rules::tables`]), whose schemas are
/// fetched for any run.
pub const TABLES: [&str; 9] = [
    "SkillLine",
    "SkillLineAbility",
    "SpecializationSpells",
    "TraitDefinition",
    "TraitNodeEntry",
    "TraitNodeXTraitNodeEntry",
    "TraitNode",
    "TraitTreeLoadout",
    "ChrClasses",
];

/// How a client's tables say which class a spell is: the rules a product's
/// run reads by.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rules {
    /// Retail's ([`generate`]): the 13 class skill lines by name, then the
    /// specializations' spells and the trait trees' specs.
    Specs,
    /// Forever's ([`generate_masks`]): each class skill line ability's
    /// class mask, and no spec.
    Masks,
}

impl Rules {
    /// The rules of the client `product` (`.build.info`'s row) holds:
    /// Forever's class masks for its product, Retail's for every other (a
    /// client whose tables do not hold them fails loudly there).
    pub fn for_product(product: &str) -> Rules {
        if Edition::Forever.product() == Some(product) {
            Rules::Masks
        } else {
            Rules::Specs
        }
    }

    /// The client tables these rules read, a part of [`TABLES`].
    pub fn tables(self) -> &'static [&'static str] {
        match self {
            Rules::Specs => &SPEC_TABLES,
            Rules::Masks => &MASK_TABLES,
        }
    }
}

/// The tables Retail's rules read.
const SPEC_TABLES: [&str; 8] = [
    "SkillLine",
    "SkillLineAbility",
    "SpecializationSpells",
    "TraitDefinition",
    "TraitNodeEntry",
    "TraitNodeXTraitNodeEntry",
    "TraitNode",
    "TraitTreeLoadout",
];

/// The tables the class-mask rules read.
const MASK_TABLES: [&str; 3] = ["SkillLine", "SkillLineAbility", "ChrClasses"];

/// The class-mask rules' pet lines that are no "Pet - ..." line: the
/// hunter's Beast Training, whose rows carry the hunter's bit but teach a
/// pet its abilities.
const PET_TRAINING: &str = "Beast Training";

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
pub(crate) const SPEC_CLASS: [(u16, &str); 40] = [
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
    (1480, "DemonHunter"),
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
    /// The class-mask rules' rows on a class skill line whose mask names no
    /// class (0), which name no spell's class; 0 under Retail's rules.
    pub unclassed: usize,
}

pub fn generate(tables: &Tables, build: &str) -> Result<Generated, String> {
    let spec_class: HashMap<u16, &str> = SPEC_CLASS.into_iter().collect();
    let class_code: HashMap<&str, u8> = CLASS_ORDER
        .iter()
        .enumerate()
        .map(|(i, &c)| (c, i as u8))
        .collect();

    // SkillLine id (as text) -> class name.
    let sl = tables.get("SkillLine")?;
    let (id_c, cat_c, name_c) = (
        sl.col("ID")?,
        sl.col("CategoryID")?,
        sl.col("DisplayName_lang")?,
    );
    let mut class_lines: HashMap<&str, &str> = HashMap::new();
    for r in &sl.rows {
        if cell(r, cat_c, "SkillLine.CategoryID")? != "7" {
            continue;
        }
        let name = cell(r, name_c, "SkillLine.DisplayName_lang")?;
        if let Some(&(_, cls)) = CLASS_SKILL_NAMES.iter().find(|(n, _)| *n == name) {
            class_lines.insert(cell(r, id_c, "SkillLine.ID")?, cls);
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

    let sla = tables.get("SkillLineAbility")?;
    let (line_c, spell_c) = (sla.col("SkillLine")?, sla.col("Spell")?);
    for r in &sla.rows {
        if let Some(&cls) = class_lines.get(cell(r, line_c, "SkillLineAbility.SkillLine")?) {
            let spell = parse_u32(cell(r, spell_c, "SkillLineAbility.Spell")?)?;
            classes.entry(spell).or_default().insert(cls);
            classwide.insert(spell);
        }
    }

    let ss = tables.get("SpecializationSpells")?;
    let (spec_c, spell_c) = (ss.col("SpecID")?, ss.col("SpellID")?);
    for r in &ss.rows {
        let Ok(spec) = cell(r, spec_c, "SpecializationSpells.SpecID")?.parse::<u16>() else {
            continue;
        };
        if let Some(&cls) = spec_class.get(&spec) {
            let spell = parse_u32(cell(r, spell_c, "SpecializationSpells.SpellID")?)?;
            classes.entry(spell).or_default().insert(cls);
            specs.entry(spell).or_default().insert(spec);
        }
    }

    // Trait chain: definition -> entries -> nodes -> trees -> specs.
    let td = tables.get("TraitDefinition")?;
    let (id_c, spell_c) = (td.col("ID")?, td.col("SpellID")?);
    let mut def_spell: HashMap<&str, u32> = HashMap::new();
    for r in &td.rows {
        let spell = cell(r, spell_c, "TraitDefinition.SpellID")?;
        if spell == "0" {
            continue;
        }
        def_spell.insert(cell(r, id_c, "TraitDefinition.ID")?, parse_u32(spell)?);
    }

    let te = tables.get("TraitNodeEntry")?;
    let (id_c, def_c) = (te.col("ID")?, te.col("TraitDefinitionID")?);
    let mut entry_def: HashMap<&str, &str> = HashMap::new();
    for r in &te.rows {
        entry_def.insert(
            cell(r, id_c, "TraitNodeEntry.ID")?,
            cell(r, def_c, "TraitNodeEntry.TraitDefinitionID")?,
        );
    }

    let tx = tables.get("TraitNodeXTraitNodeEntry")?;
    let (node_c, entry_c) = (tx.col("TraitNodeID")?, tx.col("TraitNodeEntryID")?);
    let mut node_entries: HashMap<&str, Vec<&str>> = HashMap::new();
    for r in &tx.rows {
        node_entries
            .entry(cell(r, node_c, "TraitNodeXTraitNodeEntry.TraitNodeID")?)
            .or_default()
            .push(cell(
                r,
                entry_c,
                "TraitNodeXTraitNodeEntry.TraitNodeEntryID",
            )?);
    }

    let tn = tables.get("TraitNode")?;
    let (id_c, tree_c) = (tn.col("ID")?, tn.col("TraitTreeID")?);
    let mut node_tree: HashMap<&str, &str> = HashMap::new();
    for r in &tn.rows {
        node_tree.insert(
            cell(r, id_c, "TraitNode.ID")?,
            cell(r, tree_c, "TraitNode.TraitTreeID")?,
        );
    }

    let tl = tables.get("TraitTreeLoadout")?;
    let (tree_c, spec_c) = (tl.col("TraitTreeID")?, tl.col("ChrSpecializationID")?);
    let mut tree_specs: HashMap<&str, BTreeSet<u16>> = HashMap::new();
    for r in &tl.rows {
        let Ok(spec) = cell(r, spec_c, "TraitTreeLoadout.ChrSpecializationID")?.parse::<u16>()
        else {
            continue;
        };
        if spec_class.contains_key(&spec) {
            tree_specs
                .entry(cell(r, tree_c, "TraitTreeLoadout.TraitTreeID")?)
                .or_default()
                .insert(spec);
        }
    }

    for (node, entries) in &node_entries {
        let Some(specs_here) = node_tree.get(node).and_then(|t| tree_specs.get(t)) else {
            continue;
        };
        for &spec in specs_here {
            for entry in entries {
                if let Some(&spell) = entry_def.get(entry).and_then(|d| def_spell.get(d)) {
                    // tree_specs only ever holds specs drawn from spec_class.
                    let cls = spec_class
                        .get(&spec)
                        .copied()
                        .ok_or_else(|| format!("spec {spec} has no class"))?;
                    classes.entry(spell).or_default().insert(cls);
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
            // The length check above guarantees the set has an element.
            Some(s) if s.len() == 1 && !classwide.contains(spell) => {
                s.iter().next().copied().unwrap_or(0)
            }
            _ => 0,
        };
        let cls = cs.iter().next().ok_or("class set empty")?;
        let code = class_code
            .get(cls)
            .copied()
            .ok_or_else(|| format!("unknown class {cls}"))?;
        table.push((*spell, code, spec));
    }

    let spec_unique = table.iter().filter(|(_, _, s)| *s != 0).count();
    Ok(Generated {
        content: emit(&table, ambiguous, build, Rules::Specs)?,
        spells: table.len(),
        spec_unique,
        ambiguous,
        unclassed: 0,
    })
}

/// The class-mask rules ([`Rules::Masks`], Forever's client), CONTRACT.md
/// R8 for an edition whose specializations hold no spell:
///
///   * a class skill line is a SkillLine of CategoryID 7 that is no pet's
///     (no "Pet - ..." line, nor the hunter's Beast Training);
///   * each SkillLineAbility row on one adds the classes its ClassMask
///     names, a class's bit being `1 << (ChrClasses.ID - 1)` and a mask of
///     every bit (-1) naming every class;
///   * a row whose mask is 0 names no class and is counted, not read
///     ([`Generated::unclassed`]);
///   * a spell is kept when its classes come to exactly one, with no spec;
///     one naming more than one is dropped as ambiguous.
///
/// A mask bit that names no ChrClasses row, a ChrClasses row whose class
/// this build does not know, and a class with no spell at all each fail
/// loudly, as a missing class skill line fails Retail's rules.
pub fn generate_masks(tables: &Tables, build: &str) -> Result<Generated, String> {
    let parse_u32 = |cell: &str| -> Result<u32, String> {
        cell.parse()
            .map_err(|_| format!("bad numeric cell {cell:?}"))
    };

    // ChrClasses id -> its bit, each checked against the class it is here.
    let cc = tables.get("ChrClasses")?;
    let (id_c, file_c) = (cc.col("ID")?, cc.col("Filename")?);
    let mut bits: BTreeMap<u32, u8> = BTreeMap::new();
    for r in &cc.rows {
        let id = parse_u32(cell(r, id_c, "ChrClasses.ID")?)?;
        let file = cell(r, file_c, "ChrClasses.Filename")?;
        let code = id
            .checked_sub(1)
            .filter(|&c| (c as usize) < CLASS_ORDER.len())
            .ok_or_else(|| format!("ChrClasses {id} ({file}): no class of that id here"))?;
        let name = CLASS_ORDER.get(code as usize).copied().unwrap_or_default();
        if !file.eq_ignore_ascii_case(name) {
            return Err(format!("ChrClasses {id} is {file}, not {name}"));
        }
        bits.insert(1 << code, code as u8);
    }
    let every: BTreeSet<u8> = bits.values().copied().collect();

    // The class skill lines: category 7, no pet's.
    let sl = tables.get("SkillLine")?;
    let (id_c, cat_c, name_c) = (
        sl.col("ID")?,
        sl.col("CategoryID")?,
        sl.col("DisplayName_lang")?,
    );
    let mut lines: HashSet<&str> = HashSet::new();
    let mut training = false;
    for r in &sl.rows {
        if cell(r, cat_c, "SkillLine.CategoryID")? != "7" {
            continue;
        }
        let name = cell(r, name_c, "SkillLine.DisplayName_lang")?;
        if name == PET_TRAINING {
            training = true;
            continue;
        }
        if !name.starts_with("Pet - ") {
            lines.insert(cell(r, id_c, "SkillLine.ID")?);
        }
    }
    if !training {
        return Err(format!(
            "no class skill line is named {PET_TRAINING}: its pet abilities would read as a hunter's"
        ));
    }

    // spell id -> the classes its rows' masks name.
    let mut classes: BTreeMap<u32, BTreeSet<u8>> = BTreeMap::new();
    let mut unclassed = 0;
    let sla = tables.get("SkillLineAbility")?;
    let (row_c, line_c, spell_c, mask_c) = (
        sla.col("ID")?,
        sla.col("SkillLine")?,
        sla.col("Spell")?,
        sla.col("ClassMask")?,
    );
    for r in &sla.rows {
        if !lines.contains(cell(r, line_c, "SkillLineAbility.SkillLine")?) {
            continue;
        }
        let text = cell(r, mask_c, "SkillLineAbility.ClassMask")?;
        let mask: i32 = text
            .parse()
            .map_err(|_| format!("bad numeric cell {text:?}"))?;
        let spell = parse_u32(cell(r, spell_c, "SkillLineAbility.Spell")?)?;
        // The column is signed: -1 is every bit, every class.
        let mask = mask as u32;
        let named = if mask == u32::MAX {
            every.clone()
        } else {
            let mut named = BTreeSet::new();
            for bit in (0..32).map(|b| 1u32 << b).filter(|b| mask & b != 0) {
                let code = bits.get(&bit).ok_or_else(|| {
                    let row = cell(r, row_c, "SkillLineAbility.ID").unwrap_or("?");
                    format!(
                        "SkillLineAbility {row}: ClassMask {text} names bit {bit:#x}, which no ChrClasses row has"
                    )
                })?;
                named.insert(*code);
            }
            named
        };
        if named.is_empty() {
            // A mask of 0: the row names no class. Counted, not read. These
            // are mostly the talent lines' rows (about 1,200 on Forever's
            // client, Stormstrike among them), which the class masks leave
            // unclassed and a later change reads by talent line; Mounts and
            // Companions, whose rows are no one class's, must stay out of
            // any such reading.
            unclassed += 1;
        } else {
            classes.entry(spell).or_default().extend(named);
        }
    }

    // Resolve: one class or drop; never a spec.
    let mut table: Vec<(u32, u8, u16)> = Vec::new();
    let mut ambiguous = 0;
    for (spell, cs) in &classes {
        match (cs.len(), cs.first()) {
            (1, Some(&code)) => table.push((*spell, code, 0)),
            _ => ambiguous += 1,
        }
    }
    for &code in &every {
        if !table.iter().any(|e| e.1 == code) {
            let name = CLASS_ORDER.get(code as usize).copied().unwrap_or_default();
            return Err(format!("no spell names the class {name} alone"));
        }
    }

    Ok(Generated {
        content: emit(&table, ambiguous, build, Rules::Masks)?,
        spells: table.len(),
        spec_unique: 0,
        ambiguous,
        unclassed,
    })
}

fn emit(
    table: &[(u32, u8, u16)],
    ambiguous: usize,
    build: &str,
    rules: Rules,
) -> Result<String, String> {
    let mut o = String::new();
    let speced = table.iter().filter(|(_, _, s)| *s != 0).count();
    o.push_str("//! GENERATED by tools/gen-class-spells.sh — do not edit by hand.\n");
    // No timestamp: same build in, same bytes out.
    writeln!(
        o,
        "//! Source: local client DB2s via wowdps-extract, build {build}."
    )
    .map_err(|e| format!("emit: {e}"))?;
    writeln!(
        o,
        "//! {} spells ({speced} spec-unique); {ambiguous} multi-class ids dropped.",
        table.len()
    )
    .map_err(|e| format!("emit: {e}"))?;
    o.push_str(match rules {
        Rules::Specs => {
            "//!\n\
             //! Maps a combat-log spell id to the only class that can cast it, and — when\n\
             //! the spell is unique to one specialization — to that spec (CONTRACT.md R8).\n"
        }
        Rules::Masks => {
            "//!\n\
             //! Maps a combat-log spell id to the only class that can cast it, by the\n\
             //! class masks of the client's class skill lines; this edition's\n\
             //! specializations hold no spell of their own, so no entry names a spec\n\
             //! (CONTRACT.md R8).\n"
        }
    });
    o.push_str(
        "\n\
         use wowdps_model::{Class, Spec};\n\
         \n\
         /// The class (and, when spec-unique, the spec) identified by a spell cast.\n\
         pub(crate) fn resolve(spell_id: u32) -> Option<(Class, Option<Spec>)> {\n\
         \x20   let i = TABLE.binary_search_by_key(&spell_id, |e| e.0).ok()?;\n\
         \x20   let &(_, class_code, spec_id) = TABLE.get(i)?;\n\
         \x20   let class = *CLASSES.get(class_code as usize)?;\n\
         \x20   Some((class, Spec::from_id(spec_id as u32)))\n\
         }\n\
         \n\
         const CLASSES: [Class; 13] = [\n",
    );
    for cls in CLASS_ORDER {
        writeln!(o, "    Class::{cls},").map_err(|e| format!("emit: {e}"))?;
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
         \x20   }\n",
    );
    if rules == Rules::Masks {
        o.push_str(
            "\n\
             \x20   /// The edition's specializations hold no spell of their own: no\n\
             \x20   /// entry names one.\n\
             \x20   #[test]\n\
             \x20   fn no_entry_names_a_spec() {\n\
             \x20       assert!(super::TABLE.iter().all(|e| e.2 == 0));\n\
             \x20   }\n",
        );
    }
    o.push_str("}\n");
    Ok(o)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::table::{Csv, parse_csv};

    fn csv(text: &str) -> Csv {
        parse_csv(text).unwrap()
    }

    fn base_tables() -> Tables {
        // All 13 class skill lines (ids 100..112), plus a pet line and an
        // unrelated category that must both be ignored.
        let mut skill_line = String::from("ID,CategoryID,DisplayName_lang\n");
        for (i, (display, _)) in CLASS_SKILL_NAMES.iter().enumerate() {
            skill_line.push_str(&format!("{},7,{display}\n", 100 + i));
        }
        skill_line.push_str("200,7,Pet - Ferocity\n201,9,Cooking\n");

        let mut t = Tables::default();
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

    /// Each rules' tables are the command's, and between them all of it.
    #[test]
    fn the_rules_read_the_commands_tables() {
        for rules in [Rules::Specs, Rules::Masks] {
            assert!(
                rules.tables().iter().all(|t| TABLES.contains(t)),
                "{rules:?}"
            );
        }
        assert!(
            TABLES.iter().all(|t| {
                Rules::Specs.tables().contains(t) || Rules::Masks.tables().contains(t)
            })
        );
        assert_eq!(Rules::for_product("wowf"), Rules::Masks);
        assert_eq!(Rules::for_product("wow"), Rules::Specs);
        assert_eq!(Rules::for_product("wow_classic"), Rules::Specs);
    }

    /// A client of the class-mask kind: three classes (Warrior 1, Rogue 4,
    /// Druid 11); two talent-tree lines and a "Mounts" line of category 7,
    /// a pet line, Beast Training and a profession of another category.
    fn mask_tables() -> Tables {
        let mut t = Tables::default();
        t.insert(
            "ChrClasses",
            csv("ID,Filename\n1,WARRIOR\n4,ROGUE\n11,DRUID\n"),
        );
        t.insert(
            "SkillLine",
            csv("ID,CategoryID,DisplayName_lang\n\
                 26,7,Arms\n38,7,Combat\n777,7,Mounts\n\
                 208,7,Pet - Wolf\n261,7,Beast Training\n164,11,Blacksmithing\n"),
        );
        t
    }

    fn with_rows(rows: &str) -> Tables {
        let mut t = mask_tables();
        let text = format!("ID,SkillLine,Spell,ClassMask\n{rows}");
        t.insert("SkillLineAbility", csv(&text));
        t
    }

    /// One row of every shape: a class's own bit on a tree line (Warrior
    /// 1, Rogue 8, Druid 1024), two rows naming one class, two rows naming
    /// two, one row naming two, the every-class -1, a row with no bit, and
    /// rows on the pet line, Beast Training and a profession, which name
    /// nothing at all.
    #[test]
    fn a_class_mask_names_the_spells_class_and_never_a_spec() {
        let t = with_rows(
            "1,26,100,1\n2,38,8647,8\n3,777,200,1024\n\
             4,26,300,1\n5,38,300,1\n\
             6,26,400,1\n7,38,400,8\n\
             8,26,500,9\n\
             9,38,600,-1\n\
             10,26,700,0\n\
             11,208,800,1\n12,261,900,1\n13,164,1000,8\n",
        );
        let g = generate_masks(&t, "1.60.1.1 (product wowf)").unwrap();
        // 100 and 300 Warrior (code 0), 8647 Rogue (3), 200 Druid (10);
        // 400, 500 and 600 ambiguous; 700, 800, 900 and 1000 never named.
        assert_eq!((g.spells, g.spec_unique, g.ambiguous), (4, 0, 3));
        assert_eq!(g.unclassed, 1, "row 10's mask of 0");
        assert!(
            g.content
                .contains("    (100,0,0), (200,10,0), (300,0,0), (8647,3,0),\n"),
            "{}",
            g.content
        );
        assert!(g.content.contains("build 1.60.1.1 (product wowf)."));
        assert!(
            g.content
                .contains("4 spells (0 spec-unique); 3 multi-class ids dropped")
        );
        assert!(g.content.contains("by the\n//! class masks"));
        assert!(g.content.contains("fn no_entry_names_a_spec()"));
    }

    /// Retail's emitted table carries neither the class-mask words nor
    /// their test.
    #[test]
    fn retails_table_reads_as_it_always_did() {
        let g = generate(&base_tables(), "1.2.3.4").unwrap();
        assert!(!g.content.contains("class masks"));
        assert!(!g.content.contains("no_entry_names_a_spec"));
        assert!(g.content.contains(
            "//!\n//! Maps a combat-log spell id to the only class that can cast it, and — when\n"
        ));
        assert!(g.content.ends_with("    }\n}\n"));
    }

    /// What the class-mask rules refuse: a bit no ChrClasses row has, a
    /// ChrClasses row unlike the class its id is here or past every class,
    /// a class with no spell of its own, and a client with no Beast
    /// Training line to leave out.
    #[test]
    fn the_class_mask_rules_fail_loudly() {
        let classes = "1,26,100,1\n2,38,8647,8\n3,777,200,1024\n";
        let e = generate_masks(&with_rows(&format!("{classes}4,26,300,2\n")), "b").unwrap_err();
        assert!(
            e.contains("SkillLineAbility 4: ClassMask 2 names bit 0x2"),
            "{e}"
        );
        let e = generate_masks(&with_rows("1,26,100,1\n2,38,8647,8\n"), "b").unwrap_err();
        assert!(e.contains("no spell names the class Druid alone"), "{e}");
        let mut t = with_rows(classes);
        t.insert("ChrClasses", csv("ID,Filename\n1,WARRIOR\n4,PALADIN\n"));
        let e = generate_masks(&t, "b").unwrap_err();
        assert!(e.contains("ChrClasses 4 is PALADIN, not Rogue"), "{e}");
        t.insert("ChrClasses", csv("ID,Filename\n1,WARRIOR\n14,NEWCLASS\n"));
        let e = generate_masks(&t, "b").unwrap_err();
        assert!(e.contains("ChrClasses 14 (NEWCLASS)"), "{e}");
        let mut t = with_rows(classes);
        t.insert(
            "SkillLine",
            csv("ID,CategoryID,DisplayName_lang\n26,7,Arms\n38,7,Combat\n777,7,Mounts\n"),
        );
        let e = generate_masks(&t, "b").unwrap_err();
        assert!(
            e.contains("no class skill line is named Beast Training"),
            "{e}"
        );
    }
}
