//! The talent-tree dataset generator: every class's full trait tree,
//! straight out of the local install, into one JSON document
//! (`$XDG_DATA_HOME/wowdps/talents.json`) that the MCP server's talent
//! tools and the wow-coach tree viewer read at runtime.
//!
//! Per class tree: the specs it serves, its point currencies (index 0 =
//! class points, 1 = spec points, later = hero trees), hero subtrees with
//! their spec eligibility, every node (position, type, ranks, costs,
//! spec-visibility, granted-for, gate points, edges, choice entries in
//! client order with spell id/name/icon), and `nodeOrder` — all node ids
//! ascending, the exact walk order of the in-game talent import string
//! (serialization version 2; see crates/mcp's decoder).
//!
//! Blizzard-derived strings (spell names) stay out of the repository, like
//! the icon caches: this is a per-machine file, regenerated once per patch.
//! Output is deterministic: same build in, same bytes out.

use crate::table::Csv;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fmt::Write as _;

/// The tables the generator consumes, with their FileDataIDs
/// (from wowdev/wow-listfile; stable per file, forever).
pub const TABLES: [(&str, u32); 23] = [
    ("ChrSpecialization", 1343390),
    ("SkillLine", 1240935),
    ("SkillLineXTraitTree", 4505477),
    ("TraitTreeLoadout", 4669507),
    ("TraitNode", 4420297),
    ("TraitNodeXTraitNodeEntry", 4420304),
    ("TraitNodeEntry", 4420298),
    ("TraitDefinition", 4420327),
    ("TraitEdge", 4420308),
    ("TraitSubTree", 5534447),
    ("TraitCond", 4543085),
    ("TraitNodeGroupXTraitNode", 4420302),
    ("TraitNodeGroupXTraitCond", 4543090),
    ("TraitNodeXTraitCond", 4543092),
    ("TraitNodeEntryXTraitCond", 4543088),
    ("SpecSetMember", 2057624),
    ("TraitCost", 4420295),
    ("TraitNodeXTraitCost", 4420303),
    ("TraitNodeGroupXTraitCost", 4420301),
    ("TraitCurrency", 4524216),
    ("TraitTreeXTraitCurrency", 4524218),
    ("SpellName", 1990283),
    ("SpellMisc", 1003144),
];

/// ChrClasses ids; spelled out so an unexpected class id fails loudly.
const CLASS_NAMES: [(u32, &str); 13] = [
    (1, "Warrior"),
    (2, "Paladin"),
    (3, "Hunter"),
    (4, "Rogue"),
    (5, "Priest"),
    (6, "DeathKnight"),
    (7, "Shaman"),
    (8, "Mage"),
    (9, "Warlock"),
    (10, "Monk"),
    (11, "Druid"),
    (12, "DemonHunter"),
    (13, "Evoker"),
];

/// TraitNode.Type → the dataset's node-type tag (Enum.TraitNodeType).
fn node_type_tag(t: u32) -> &'static str {
    match t {
        0 => "single",
        1 => "tiered",
        2 => "choice",
        3 => "subtree",
        _ => "unknown",
    }
}

/// TraitCond.CondType values (Enum.TraitConditionType).
const COND_AVAILABLE: u32 = 0;
const COND_VISIBLE: u32 = 1;
const COND_GRANTED: u32 = 2;

#[derive(Debug)]
pub struct Generated {
    pub content: String,
    pub trees: usize,
    pub specs: usize,
    pub nodes: usize,
    /// Trait spells whose name was not found in SpellName.
    pub nameless: usize,
}

/// One cell of a CSV row. The column index comes from `Csv::col`, so a miss
/// means the row itself is short — a malformed table, not a bug here.
fn cell<'a>(row: &'a [String], c: usize, what: &str) -> Result<&'a str, String> {
    row.get(c)
        .map(String::as_str)
        .ok_or_else(|| format!("{what}: row has no column {c}"))
}

fn parse_u32(cell: &str) -> Result<u32, String> {
    cell.parse()
        .map_err(|_| format!("bad numeric cell {cell:?}"))
}

fn parse_i64(cell: &str) -> Result<i64, String> {
    // Positions are written as floats by some layouts ("1200.0").
    if let Ok(v) = cell.parse::<i64>() {
        return Ok(v);
    }
    cell.parse::<f64>()
        .map(|f| f as i64)
        .map_err(|_| format!("bad numeric cell {cell:?}"))
}

/// JSON string escape (control chars, quote, backslash).
fn jstr(s: &str) -> String {
    let mut o = String::with_capacity(s.len() + 2);
    o.push('"');
    for ch in s.chars() {
        match ch {
            '"' => o.push_str("\\\""),
            '\\' => o.push_str("\\\\"),
            '\n' => o.push_str("\\n"),
            '\r' => o.push_str("\\r"),
            '\t' => o.push_str("\\t"),
            c if (c as u32) < 0x20 => {
                let _ = write!(o, "\\u{:04x}", c as u32);
            }
            c => o.push(c),
        }
    }
    o.push('"');
    o
}

#[derive(Debug, Default, Clone)]
struct Cond {
    cond_type: u32,
    spec_set: u32,
    granted_ranks: u32,
    spent_required: u32,
}

#[derive(Debug, Default)]
struct Node {
    tree: u32,
    pos_x: i64,
    pos_y: i64,
    node_type: u32,
    sub_tree: u32,
}

#[derive(Debug, Default)]
struct Entry {
    definition: u32,
    max_ranks: u32,
    entry_type: u32,
    sub_tree: u32,
}

#[derive(Debug, Default)]
struct Definition {
    spell: u32,
    override_name: String,
    override_icon: u32,
}

pub fn generate(
    tables: &HashMap<&str, Csv>,
    icon_names: &HashMap<u32, String>,
    build: &str,
) -> Result<Generated, String> {
    let get = |name: &str| {
        tables
            .get(name)
            .ok_or_else(|| format!("missing table {name}"))
    };
    let class_names: HashMap<u32, &str> = CLASS_NAMES.into_iter().collect();

    // Spec metadata: id -> (name, class id, role).
    let chr = get("ChrSpecialization")?;
    let (id_c, name_c, class_c, role_c) = (
        chr.col("ID")?,
        chr.col("Name_lang")?,
        chr.col("ClassID")?,
        chr.col("Role")?,
    );
    let mut spec_meta: HashMap<u32, (String, u32, u32)> = HashMap::new();
    for r in &chr.rows {
        let class_id = parse_u32(cell(r, class_c, "ChrSpecialization.ClassID")?)?;
        if !class_names.contains_key(&class_id) {
            continue; // pet specs, initial shells
        }
        spec_meta.insert(
            parse_u32(cell(r, id_c, "ChrSpecialization.ID")?)?,
            (
                cell(r, name_c, "ChrSpecialization.Name_lang")?.to_string(),
                class_id,
                parse_u32(cell(r, role_c, "ChrSpecialization.Role")?)?,
            ),
        );
    }

    // The ACTIVE tree per class, via the class skill line (matched by
    // display name, as classgen does, so a renumbering patch fails loudly)
    // → SkillLineXTraitTree. TraitTreeLoadout alone also carries retired
    // and dev/test trees (three Shaman trees, a Fire+Arms "Mage").
    let name_class: HashMap<&str, u32> = [
        ("Warrior", 1),
        ("Paladin", 2),
        ("Hunter", 3),
        ("Rogue", 4),
        ("Priest", 5),
        ("Death Knight", 6),
        ("Shaman", 7),
        ("Mage", 8),
        ("Warlock", 9),
        ("Monk", 10),
        ("Druid", 11),
        ("Demon Hunter", 12),
        ("Evoker", 13),
    ]
    .into_iter()
    .collect();
    let sl = get("SkillLine")?;
    let (id_c, cat_c, name_c) = (
        sl.col("ID")?,
        sl.col("CategoryID")?,
        sl.col("DisplayName_lang")?,
    );
    let mut line_class: HashMap<u32, u32> = HashMap::new();
    for r in &sl.rows {
        if cell(r, cat_c, "SkillLine.CategoryID")? != "7" {
            continue;
        }
        if let Some(&class_id) = name_class.get(cell(r, name_c, "SkillLine.DisplayName_lang")?) {
            line_class.insert(parse_u32(cell(r, id_c, "SkillLine.ID")?)?, class_id);
        }
    }
    let sx = get("SkillLineXTraitTree")?;
    let (line_c, tree_c) = (sx.col("SkillLineID")?, sx.col("TraitTreeID")?);
    let mut tree_class: BTreeMap<u32, u32> = BTreeMap::new();
    for r in &sx.rows {
        let line = parse_u32(cell(r, line_c, "SkillLineXTraitTree.SkillLineID")?)?;
        if let Some(&class_id) = line_class.get(&line) {
            tree_class.insert(
                parse_u32(cell(r, tree_c, "SkillLineXTraitTree.TraitTreeID")?)?,
                class_id,
            );
        }
    }
    if tree_class.len() != 13 {
        return Err(format!(
            "expected 13 active class trees, found {}: {tree_class:?}",
            tree_class.len()
        ));
    }

    // Tree -> specs, active class trees only.
    let tl = get("TraitTreeLoadout")?;
    let (tree_c, spec_c) = (tl.col("TraitTreeID")?, tl.col("ChrSpecializationID")?);
    let mut tree_specs: BTreeMap<u32, BTreeSet<u32>> = BTreeMap::new();
    for r in &tl.rows {
        let tree = parse_u32(cell(r, tree_c, "TraitTreeLoadout.TraitTreeID")?)?;
        let spec = parse_u32(cell(r, spec_c, "TraitTreeLoadout.ChrSpecializationID")?)?;
        if tree_class.contains_key(&tree) && spec_meta.contains_key(&spec) {
            tree_specs.entry(tree).or_default().insert(spec);
        }
    }

    // Nodes.
    let tn = get("TraitNode")?;
    let (id_c, tree_c, x_c, y_c, type_c, sub_c) = (
        tn.col("ID")?,
        tn.col("TraitTreeID")?,
        tn.col("PosX")?,
        tn.col("PosY")?,
        tn.col("Type")?,
        tn.col("TraitSubTreeID")?,
    );
    let mut nodes: BTreeMap<u32, Node> = BTreeMap::new();
    for r in &tn.rows {
        let tree = parse_u32(cell(r, tree_c, "TraitNode.TraitTreeID")?)?;
        if !tree_specs.contains_key(&tree) {
            continue;
        }
        nodes.insert(
            parse_u32(cell(r, id_c, "TraitNode.ID")?)?,
            Node {
                tree,
                pos_x: parse_i64(cell(r, x_c, "TraitNode.PosX")?)?,
                pos_y: parse_i64(cell(r, y_c, "TraitNode.PosY")?)?,
                node_type: parse_u32(cell(r, type_c, "TraitNode.Type")?)?,
                sub_tree: parse_u32(cell(r, sub_c, "TraitNode.TraitSubTreeID")?)?,
            },
        );
    }

    // Node -> entries, in client (Index) order — the choice-entry order the
    // import string's 2-bit choiceEntryIndex refers to.
    let tx = get("TraitNodeXTraitNodeEntry")?;
    let (node_c, entry_c, idx_c) = (
        tx.col("TraitNodeID")?,
        tx.col("TraitNodeEntryID")?,
        tx.col("Index").or_else(|_| tx.col("_Index"))?,
    );
    let mut node_entries: BTreeMap<u32, Vec<(u32, u32)>> = BTreeMap::new();
    for r in &tx.rows {
        let node = parse_u32(cell(r, node_c, "TraitNodeXTraitNodeEntry.TraitNodeID")?)?;
        if !nodes.contains_key(&node) {
            continue;
        }
        node_entries.entry(node).or_default().push((
            parse_u32(cell(r, idx_c, "TraitNodeXTraitNodeEntry.Index")?)?,
            parse_u32(cell(
                r,
                entry_c,
                "TraitNodeXTraitNodeEntry.TraitNodeEntryID",
            )?)?,
        ));
    }
    for list in node_entries.values_mut() {
        list.sort_unstable();
    }

    let te = get("TraitNodeEntry")?;
    let (id_c, def_c, ranks_c, type_c, sub_c) = (
        te.col("ID")?,
        te.col("TraitDefinitionID")?,
        te.col("MaxRanks")?,
        te.col("NodeEntryType")?,
        te.col("TraitSubTreeID")?,
    );
    let mut entries: HashMap<u32, Entry> = HashMap::new();
    for r in &te.rows {
        entries.insert(
            parse_u32(cell(r, id_c, "TraitNodeEntry.ID")?)?,
            Entry {
                definition: parse_u32(cell(r, def_c, "TraitNodeEntry.TraitDefinitionID")?)?,
                max_ranks: parse_u32(cell(r, ranks_c, "TraitNodeEntry.MaxRanks")?)?,
                entry_type: parse_u32(cell(r, type_c, "TraitNodeEntry.NodeEntryType")?)?,
                sub_tree: parse_u32(cell(r, sub_c, "TraitNodeEntry.TraitSubTreeID")?)?,
            },
        );
    }

    let td = get("TraitDefinition")?;
    let (id_c, spell_c, oname_c, oicon_c) = (
        td.col("ID")?,
        td.col("SpellID")?,
        td.col("OverrideName_lang")?,
        td.col("OverrideIcon")?,
    );
    let mut defs: HashMap<u32, Definition> = HashMap::new();
    for r in &td.rows {
        defs.insert(
            parse_u32(cell(r, id_c, "TraitDefinition.ID")?)?,
            Definition {
                spell: parse_u32(cell(r, spell_c, "TraitDefinition.SpellID")?)?,
                override_name: cell(r, oname_c, "TraitDefinition.OverrideName_lang")?.to_string(),
                override_icon: parse_u32(cell(r, oicon_c, "TraitDefinition.OverrideIcon")?)?,
            },
        );
    }

    // Edges.
    let ted = get("TraitEdge")?;
    let (l_c, r_c) = (ted.col("LeftTraitNodeID")?, ted.col("RightTraitNodeID")?);
    let mut next: BTreeMap<u32, BTreeSet<u32>> = BTreeMap::new();
    let mut prev: BTreeMap<u32, BTreeSet<u32>> = BTreeMap::new();
    for r in &ted.rows {
        let from = parse_u32(cell(r, l_c, "TraitEdge.LeftTraitNodeID")?)?;
        let to = parse_u32(cell(r, r_c, "TraitEdge.RightTraitNodeID")?)?;
        if nodes.contains_key(&from) && nodes.contains_key(&to) {
            next.entry(from).or_default().insert(to);
            prev.entry(to).or_default().insert(from);
        }
    }

    // Hero subtrees.
    let ts = get("TraitSubTree")?;
    let (id_c, name_c, tree_c) = (ts.col("ID")?, ts.col("Name_lang")?, ts.col("TraitTreeID")?);
    let mut sub_trees: BTreeMap<u32, (String, u32)> = BTreeMap::new();
    for r in &ts.rows {
        sub_trees.insert(
            parse_u32(cell(r, id_c, "TraitSubTree.ID")?)?,
            (
                cell(r, name_c, "TraitSubTree.Name_lang")?.to_string(),
                parse_u32(cell(r, tree_c, "TraitSubTree.TraitTreeID")?)?,
            ),
        );
    }

    // Conditions and their attachments (via groups, nodes, entries).
    let tc = get("TraitCond")?;
    let (id_c, type_c, set_c, granted_c, spent_c) = (
        tc.col("ID")?,
        tc.col("CondType")?,
        tc.col("SpecSetID")?,
        tc.col("GrantedRanks")?,
        tc.col("SpentAmountRequired")?,
    );
    let mut conds: HashMap<u32, Cond> = HashMap::new();
    for r in &tc.rows {
        conds.insert(
            parse_u32(cell(r, id_c, "TraitCond.ID")?)?,
            Cond {
                cond_type: parse_u32(cell(r, type_c, "TraitCond.CondType")?)?,
                spec_set: parse_u32(cell(r, set_c, "TraitCond.SpecSetID")?)?,
                granted_ranks: parse_u32(cell(r, granted_c, "TraitCond.GrantedRanks")?)?,
                spent_required: parse_u32(cell(r, spent_c, "TraitCond.SpentAmountRequired")?)?,
            },
        );
    }

    let ssm = get("SpecSetMember")?;
    let (spec_c, set_c) = (ssm.col("ChrSpecializationID")?, ssm.col("SpecSet")?);
    let mut spec_sets: HashMap<u32, BTreeSet<u32>> = HashMap::new();
    for r in &ssm.rows {
        spec_sets
            .entry(parse_u32(cell(r, set_c, "SpecSetMember.SpecSet")?)?)
            .or_default()
            .insert(parse_u32(cell(
                r,
                spec_c,
                "SpecSetMember.ChrSpecializationID",
            )?)?);
    }

    let two_col = |name: &str, a: &str, b: &str| -> Result<Vec<(u32, u32)>, String> {
        let t = get(name)?;
        let (a_c, b_c) = (t.col(a)?, t.col(b)?);
        t.rows
            .iter()
            .map(|r| {
                Ok((
                    parse_u32(cell(r, a_c, name)?)?,
                    parse_u32(cell(r, b_c, name)?)?,
                ))
            })
            .collect()
    };

    // group -> member nodes; group/node/entry -> conds.
    let mut group_nodes: HashMap<u32, Vec<u32>> = HashMap::new();
    for (group, node) in two_col(
        "TraitNodeGroupXTraitNode",
        "TraitNodeGroupID",
        "TraitNodeID",
    )? {
        group_nodes.entry(group).or_default().push(node);
    }
    let mut node_conds: HashMap<u32, Vec<u32>> = HashMap::new();
    for (cond, group) in two_col(
        "TraitNodeGroupXTraitCond",
        "TraitCondID",
        "TraitNodeGroupID",
    )? {
        for node in group_nodes.get(&group).map_or(&[][..], Vec::as_slice) {
            node_conds.entry(*node).or_default().push(cond);
        }
    }
    for (cond, node) in two_col("TraitNodeXTraitCond", "TraitCondID", "TraitNodeID")? {
        node_conds.entry(node).or_default().push(cond);
    }
    let mut entry_conds: HashMap<u32, Vec<u32>> = HashMap::new();
    for (cond, entry) in two_col(
        "TraitNodeEntryXTraitCond",
        "TraitCondID",
        "TraitNodeEntryID",
    )? {
        entry_conds.entry(entry).or_default().push(cond);
    }

    // Costs: node -> [(currency, amount)], direct and via groups.
    let tco = get("TraitCost")?;
    let (id_c, amt_c, cur_c) = (
        tco.col("ID")?,
        tco.col("Amount")?,
        tco.col("TraitCurrencyID")?,
    );
    let mut costs: HashMap<u32, (u32, u32)> = HashMap::new();
    for r in &tco.rows {
        costs.insert(
            parse_u32(cell(r, id_c, "TraitCost.ID")?)?,
            (
                parse_u32(cell(r, cur_c, "TraitCost.TraitCurrencyID")?)?,
                parse_u32(cell(r, amt_c, "TraitCost.Amount")?)?,
            ),
        );
    }
    let mut node_costs: HashMap<u32, BTreeMap<u32, u32>> = HashMap::new();
    for (node, cost) in two_col("TraitNodeXTraitCost", "TraitNodeID", "TraitCostID")? {
        if let Some(&(currency, amount)) = costs.get(&cost) {
            node_costs.entry(node).or_default().insert(currency, amount);
        }
    }
    for (group, cost) in two_col(
        "TraitNodeGroupXTraitCost",
        "TraitNodeGroupID",
        "TraitCostID",
    )? {
        if let Some(&(currency, amount)) = costs.get(&cost) {
            for node in group_nodes.get(&group).map_or(&[][..], Vec::as_slice) {
                node_costs
                    .entry(*node)
                    .or_default()
                    .insert(currency, amount);
            }
        }
    }

    // Tree -> ordered currencies (index 0 class points, 1 spec points, …).
    let tt = get("TraitTreeXTraitCurrency")?;
    let (tree_c, cur_c, idx_c) = (
        tt.col("TraitTreeID")?,
        tt.col("TraitCurrencyID")?,
        tt.col("Index").or_else(|_| tt.col("_Index"))?,
    );
    let mut tree_currencies: BTreeMap<u32, Vec<(u32, u32)>> = BTreeMap::new();
    for r in &tt.rows {
        let tree = parse_u32(cell(r, tree_c, "TraitTreeXTraitCurrency.TraitTreeID")?)?;
        if tree_specs.contains_key(&tree) {
            tree_currencies.entry(tree).or_default().push((
                parse_u32(cell(r, idx_c, "TraitTreeXTraitCurrency.Index")?)?,
                parse_u32(cell(r, cur_c, "TraitTreeXTraitCurrency.TraitCurrencyID")?)?,
            ));
        }
    }
    for list in tree_currencies.values_mut() {
        list.sort_unstable();
    }

    // Names and icons, only for spells the trees reference.
    let mut wanted_spells: BTreeSet<u32> = BTreeSet::new();
    for list in node_entries.values() {
        for (_, entry_id) in list {
            if let Some(d) = entries.get(entry_id).and_then(|e| defs.get(&e.definition))
                && d.spell != 0
            {
                wanted_spells.insert(d.spell);
            }
        }
    }
    let sn = get("SpellName")?;
    let (id_c, name_c) = (sn.col("ID")?, sn.col("Name_lang")?);
    let mut spell_names: HashMap<u32, String> = HashMap::new();
    for r in &sn.rows {
        let id = parse_u32(cell(r, id_c, "SpellName.ID")?)?;
        if wanted_spells.contains(&id) {
            spell_names.insert(id, cell(r, name_c, "SpellName.Name_lang")?.to_string());
        }
    }
    let sm = get("SpellMisc")?;
    let (spell_c, icon_c) = (sm.col("SpellID")?, sm.col("SpellIconFileDataID")?);
    let diff_c = sm.col("DifficultyID").ok();
    let mut spell_icons: HashMap<u32, (u32, bool)> = HashMap::new();
    for r in &sm.rows {
        let (Some(spell), Some(icon)) = (
            r.get(spell_c).and_then(|s| s.parse::<u32>().ok()),
            r.get(icon_c).and_then(|s| s.parse::<u32>().ok()),
        ) else {
            continue;
        };
        if icon == 0 || !wanted_spells.contains(&spell) {
            continue;
        }
        // Base-difficulty row wins, as in spellicongen.
        let base = diff_c.and_then(|c| r.get(c)).is_none_or(|d| d == "0");
        let slot = spell_icons.entry(spell).or_insert((icon, base));
        if base && !slot.1 {
            *slot = (icon, true);
        }
    }

    // Emit.
    let mut o = String::new();
    o.push_str("{\n");
    let _ = writeln!(o, "  \"build\": {},", jstr(build));
    let _ = writeln!(o, "  \"format\": 1,");
    o.push_str("  \"trees\": [\n");

    let mut n_trees = 0usize;
    let mut n_specs = 0usize;
    let mut n_nodes = 0usize;
    let mut nameless = 0usize;

    let cond_specs = |cond_ids: &[u32], want: u32| -> BTreeSet<u32> {
        // Union of the spec sets on conds of the wanted type.
        let mut out = BTreeSet::new();
        for id in cond_ids {
            if let Some(c) = conds.get(id)
                && c.cond_type == want
                && (want != COND_GRANTED || c.granted_ranks > 0)
                && c.spec_set != 0
                && let Some(members) = spec_sets.get(&c.spec_set)
            {
                out.extend(members.iter().copied());
            }
        }
        out
    };

    let tree_list: Vec<(u32, &BTreeSet<u32>)> = tree_specs.iter().map(|(t, s)| (*t, s)).collect();
    for (ti, (tree_id, specs)) in tree_list.iter().enumerate() {
        let class_id = tree_class
            .get(tree_id)
            .copied()
            .ok_or_else(|| format!("tree {tree_id}: no class skill line"))?;
        let class_name = class_names
            .get(&class_id)
            .ok_or_else(|| format!("unknown class id {class_id}"))?;
        n_trees += 1;
        n_specs += specs.len();

        o.push_str("    {\n");
        let _ = writeln!(o, "      \"treeId\": {tree_id},");
        let _ = writeln!(o, "      \"classId\": {class_id},");
        let _ = writeln!(o, "      \"className\": {},", jstr(class_name));

        // Specs.
        o.push_str("      \"specs\": [");
        for (i, spec) in specs.iter().enumerate() {
            let (name, _, role) = spec_meta
                .get(spec)
                .ok_or_else(|| format!("spec {spec} lost its metadata"))?;
            let _ = write!(
                o,
                "{}{{\"specId\": {spec}, \"name\": {}, \"role\": {role}}}",
                if i > 0 { ", " } else { "" },
                jstr(name)
            );
        }
        o.push_str("],\n");

        // Currencies.
        o.push_str("      \"currencies\": [");
        if let Some(list) = tree_currencies.get(tree_id) {
            for (i, (index, currency)) in list.iter().enumerate() {
                let _ = write!(
                    o,
                    "{}{{\"index\": {index}, \"id\": {currency}}}",
                    if i > 0 { ", " } else { "" }
                );
            }
        }
        o.push_str("],\n");

        // Hero subtrees of this tree, with spec eligibility read off the
        // conds of the subtree-selection entries that offer them.
        let mut sub_specs: BTreeMap<u32, BTreeSet<u32>> = BTreeMap::new();
        for (node_id, node) in &nodes {
            if node.tree != *tree_id || node.node_type != 3 {
                continue;
            }
            for (_, entry_id) in node_entries.get(node_id).map_or(&[][..], Vec::as_slice) {
                let Some(e) = entries.get(entry_id) else {
                    continue;
                };
                if e.sub_tree == 0 {
                    continue;
                }
                let gated = entry_conds.get(entry_id).map_or_else(BTreeSet::new, |c| {
                    let mut s = cond_specs(c, COND_VISIBLE);
                    s.extend(cond_specs(c, COND_AVAILABLE));
                    s
                });
                sub_specs.entry(e.sub_tree).or_default().extend(gated);
            }
        }
        o.push_str("      \"subTrees\": [");
        for (i, (sub_id, gated)) in sub_specs.iter().enumerate() {
            let name = sub_trees.get(sub_id).map_or("", |(n, _)| n.as_str());
            let _ = write!(
                o,
                "{}{{\"id\": {sub_id}, \"name\": {}, \"specs\": [{}]}}",
                if i > 0 { ", " } else { "" },
                jstr(name),
                gated
                    .iter()
                    .map(|s| s.to_string())
                    .collect::<Vec<_>>()
                    .join(", ")
            );
        }
        o.push_str("],\n");

        // nodeOrder: every node of the tree, ascending id — the import
        // string's walk order.
        let tree_nodes: Vec<u32> = nodes
            .iter()
            .filter(|(_, n)| n.tree == *tree_id)
            .map(|(id, _)| *id)
            .collect(); // BTreeMap iteration is already ascending
        o.push_str("      \"nodeOrder\": [");
        for (i, id) in tree_nodes.iter().enumerate() {
            let _ = write!(o, "{}{id}", if i > 0 { ", " } else { "" });
        }
        o.push_str("],\n");

        // Nodes.
        o.push_str("      \"nodes\": [\n");
        for (i, node_id) in tree_nodes.iter().enumerate() {
            let node = nodes
                .get(node_id)
                .ok_or_else(|| format!("node {node_id} vanished"))?;
            n_nodes += 1;
            let cond_ids: &[u32] = node_conds.get(node_id).map_or(&[], Vec::as_slice);
            let visible = {
                let mut s = cond_specs(cond_ids, COND_VISIBLE);
                s.extend(cond_specs(cond_ids, COND_AVAILABLE));
                s
            };
            let granted = cond_specs(cond_ids, COND_GRANTED);
            let granted_all = cond_ids.iter().any(|c| {
                conds.get(c).is_some_and(|c| {
                    c.cond_type == COND_GRANTED && c.granted_ranks > 0 && c.spec_set == 0
                })
            });
            let req_points = cond_ids
                .iter()
                .filter_map(|c| conds.get(c))
                .map(|c| c.spent_required)
                .max()
                .unwrap_or(0);
            let entry_list: &[(u32, u32)] = node_entries.get(node_id).map_or(&[], Vec::as_slice);
            let max_ranks = entry_list
                .iter()
                .filter_map(|(_, e)| entries.get(e))
                .map(|e| e.max_ranks)
                .max()
                .unwrap_or(0);

            o.push_str("        {");
            let _ = write!(
                o,
                "\"id\": {node_id}, \"type\": {}, \"posX\": {}, \"posY\": {}, \"maxRanks\": {max_ranks}",
                jstr(node_type_tag(node.node_type)),
                node.pos_x,
                node.pos_y,
            );
            if node.sub_tree != 0 {
                let _ = write!(o, ", \"subTreeId\": {}", node.sub_tree);
            }
            if req_points != 0 {
                let _ = write!(o, ", \"reqPoints\": {req_points}");
            }
            if let Some(cs) = node_costs.get(node_id) {
                let cells: Vec<String> = cs
                    .iter()
                    .map(|(cur, amt)| format!("{{\"currency\": {cur}, \"amount\": {amt}}}"))
                    .collect();
                let _ = write!(o, ", \"costs\": [{}]", cells.join(", "));
            }
            let ids = |s: &BTreeSet<u32>| {
                s.iter()
                    .map(|v| v.to_string())
                    .collect::<Vec<_>>()
                    .join(", ")
            };
            if !visible.is_empty() {
                let _ = write!(o, ", \"visibleFor\": [{}]", ids(&visible));
            }
            if granted_all {
                o.push_str(", \"granted\": true");
            } else if !granted.is_empty() {
                let _ = write!(o, ", \"grantedFor\": [{}]", ids(&granted));
            }
            for (key, map) in [("next", &next), ("prev", &prev)] {
                if let Some(s) = map.get(node_id) {
                    let _ = write!(o, ", \"{key}\": [{}]", ids(s));
                }
            }

            o.push_str(", \"entries\": [");
            for (j, (_, entry_id)) in entry_list.iter().enumerate() {
                let e = entries
                    .get(entry_id)
                    .ok_or_else(|| format!("entry {entry_id} missing from TraitNodeEntry"))?;
                let d = defs.get(&e.definition);
                let spell = d.map_or(0, |d| d.spell);
                let name = match d {
                    Some(d) if !d.override_name.is_empty() => d.override_name.clone(),
                    _ => match spell_names.get(&spell) {
                        Some(n) => n.clone(),
                        None => {
                            nameless += 1;
                            String::new()
                        }
                    },
                };
                let icon_fdid = match d {
                    Some(d) if d.override_icon != 0 => d.override_icon,
                    _ => spell_icons.get(&spell).map_or(0, |(f, _)| *f),
                };
                let _ = write!(
                    o,
                    "{}{{\"id\": {entry_id}, \"definitionId\": {}, \"spellId\": {spell}, \
                     \"name\": {}, \"maxRanks\": {}, \"entryType\": {}",
                    if j > 0 { ", " } else { "" },
                    e.definition,
                    jstr(&name),
                    e.max_ranks,
                    e.entry_type,
                );
                if e.sub_tree != 0 {
                    let _ = write!(o, ", \"subTreeId\": {}", e.sub_tree);
                }
                if icon_fdid != 0 {
                    let _ = write!(o, ", \"iconFdid\": {icon_fdid}");
                    if let Some(name) = icon_names.get(&icon_fdid) {
                        let _ = write!(o, ", \"icon\": {}", jstr(name));
                    }
                }
                o.push('}');
            }
            o.push_str("]}");
            o.push_str(if i + 1 < tree_nodes.len() {
                ",\n"
            } else {
                "\n"
            });
        }
        o.push_str("      ]\n");
        o.push_str(if ti + 1 < tree_list.len() {
            "    },\n"
        } else {
            "    }\n"
        });
    }
    o.push_str("  ]\n}\n");

    Ok(Generated {
        content: o,
        trees: n_trees,
        specs: n_specs,
        nodes: n_nodes,
        nameless,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::table::parse_csv;

    fn csv(text: &str) -> Csv {
        parse_csv(text).unwrap()
    }

    /// A one-class synthetic universe: tree 10 for Mage specs 62/63, with a
    /// granted root (1), a 2-rank single (2), a choice node (3) whose entry
    /// order comes from Index, a spec-gated node (4, spec 62 only, behind an
    /// 8-point gate), and a hero subtree 77 selectable by both specs via
    /// subtree-selection node 5.
    fn base_tables() -> HashMap<&'static str, Csv> {
        let mut t = HashMap::new();
        t.insert(
            "ChrSpecialization",
            csv("ID,Name_lang,ClassID,Role\n62,Arcane,8,2\n63,Fire,8,2\n74,Ferocity,0,2\n"),
        );
        // All 13 class skill lines (ids 100..112) each mapped to a trait
        // tree; only Mage's tree 10 has loadouts/nodes below. Tree 11 is a
        // RETIRED Mage tree with loadout rows that must be excluded, tree
        // 99 a pet tree with no skill line at all.
        let mut skill_line = String::from("ID,CategoryID,DisplayName_lang\n");
        let mut sxt = String::from("ID,SkillLineID,TraitTreeID\n");
        for (i, name) in [
            "Warrior",
            "Paladin",
            "Hunter",
            "Rogue",
            "Priest",
            "Death Knight",
            "Shaman",
            "Mage",
            "Warlock",
            "Monk",
            "Druid",
            "Demon Hunter",
            "Evoker",
        ]
        .iter()
        .enumerate()
        {
            skill_line.push_str(&format!("{},7,{name}\n", 100 + i));
            let tree = if *name == "Mage" { 10 } else { 500 + i };
            sxt.push_str(&format!("{},{},{tree}\n", i + 1, 100 + i));
        }
        skill_line.push_str("200,7,Pet - Ferocity\n201,9,Cooking\n");
        t.insert("SkillLine", csv(&skill_line));
        t.insert("SkillLineXTraitTree", csv(&sxt));
        t.insert(
            "TraitTreeLoadout",
            csv("ID,TraitTreeID,ChrSpecializationID\n1,10,62\n2,10,63\n3,99,74\n4,11,62\n"),
        );
        t.insert(
            "TraitNode",
            csv("ID,TraitTreeID,PosX,PosY,Type,Flags,TraitSubTreeID\n\
                 1,10,100,100,0,0,0\n2,10,100,200,0,0,0\n3,10,200,200,2,0,0\n\
                 4,10,300,200,0,0,0\n5,10,400,100,3,0,0\n6,10,400,200,0,0,77\n\
                 50,99,0,0,0,0,0\n"),
        );
        t.insert(
            "TraitNodeXTraitNodeEntry",
            // Node 3's entries deliberately out of row order: Index must win.
            csv("ID,TraitNodeID,TraitNodeEntryID,Index\n\
                 1,1,101,0\n2,2,102,0\n3,3,132,1\n4,3,131,0\n5,4,104,0\n\
                 6,5,151,0\n7,6,106,0\n"),
        );
        t.insert(
            "TraitNodeEntry",
            csv(
                "ID,TraitDefinitionID,MaxRanks,NodeEntryType,TraitSubTreeID\n\
                 101,201,1,0,0\n102,202,2,0,0\n131,231,1,0,0\n132,232,1,0,0\n\
                 104,204,1,0,0\n151,0,1,3,77\n106,206,1,0,0\n",
            ),
        );
        t.insert(
            "TraitDefinition",
            csv("ID,SpellID,OverrideName_lang,OverrideIcon\n\
                 201,1001,,0\n202,1002,,0\n231,1031,,0\n232,1032,Renamed,555\n\
                 204,1004,,0\n206,1006,,0\n"),
        );
        t.insert(
            "TraitEdge",
            csv("ID,VisualStyle,LeftTraitNodeID,RightTraitNodeID,Type\n1,0,1,2,0\n2,0,2,3,0\n"),
        );
        t.insert(
            "TraitSubTree",
            csv("ID,Name_lang,TraitTreeID\n77,Sunfury,10\n"),
        );
        t.insert(
            "TraitCond",
            // 301: granted (all specs) on node 1. 302: visible spec-set 900
            // (=spec 62) on node 4's group. 303: 8-point gate on node 4's
            // group. 304: visible spec-set 901 (62+63) on entry 151.
            csv("ID,CondType,SpecSetID,GrantedRanks,SpentAmountRequired\n\
                 301,2,0,1,0\n302,1,900,0,0\n303,0,0,0,8\n304,1,901,0,0\n"),
        );
        t.insert(
            "TraitNodeGroupXTraitNode",
            csv("ID,TraitNodeGroupID,TraitNodeID,Index\n1,401,4,0\n"),
        );
        t.insert(
            "TraitNodeGroupXTraitCond",
            csv("ID,TraitCondID,TraitNodeGroupID\n1,302,401\n2,303,401\n"),
        );
        t.insert(
            "TraitNodeXTraitCond",
            csv("ID,TraitCondID,TraitNodeID\n1,301,1\n"),
        );
        t.insert(
            "TraitNodeEntryXTraitCond",
            csv("ID,TraitCondID,TraitNodeEntryID\n1,304,151\n"),
        );
        t.insert(
            "SpecSetMember",
            csv("ID,ChrSpecializationID,SpecSet\n1,62,900\n2,62,901\n3,63,901\n"),
        );
        t.insert(
            "TraitCost",
            csv("InternalName,ID,Amount,TraitCurrencyID\nc,501,1,601\n"),
        );
        t.insert(
            "TraitNodeXTraitCost",
            csv("ID,TraitNodeID,TraitCostID\n1,2,501\n"),
        );
        t.insert(
            "TraitNodeGroupXTraitCost",
            csv("ID,TraitNodeGroupID,TraitCostID\n1,401,501\n"),
        );
        t.insert(
            "TraitCurrency",
            csv("ID,Type,CurrencyTypesID,Flags,Icon\n601,0,0,0,0\n602,0,0,0,0\n"),
        );
        t.insert(
            "TraitTreeXTraitCurrency",
            csv("ID,Index,TraitTreeID,TraitCurrencyID\n1,1,10,602\n2,0,10,601\n"),
        );
        t.insert(
            "SpellName",
            csv(
                "ID,Name_lang\n1001,Root\n1002,Filler\n1031,Left Pick\n1032,Right Pick\n\
                 1004,Gated\n1006,Hero Node\n",
            ),
        );
        t.insert(
            "SpellMisc",
            csv("ID,SpellID,DifficultyID,SpellIconFileDataID\n\
                 1,1001,0,7001\n2,1002,0,7002\n3,1002,1,7999\n4,1031,0,7031\n\
                 5,1004,0,7004\n6,1006,0,7006\n"),
        );
        t
    }

    #[test]
    fn dataset_shape() {
        let icons: HashMap<u32, String> = [(7001, "spell_root".to_string())].into();
        let g = generate(&base_tables(), &icons, "12.1.0.69497").unwrap();
        assert_eq!((g.trees, g.specs, g.nodes), (1, 2, 6));
        let c = &g.content;
        assert!(c.contains("\"build\": \"12.1.0.69497\""), "{c}");
        // Pet tree 99 (no skill line) and retired Mage tree 11 (loadout
        // rows but no SkillLineXTraitTree row) are both excluded.
        assert!(!c.contains("\"treeId\": 99"), "{c}");
        assert!(!c.contains("\"treeId\": 11"), "{c}");
        assert!(c.contains("\"classId\": 8"), "{c}");
        // Walk order: every node of the tree, ascending.
        assert!(c.contains("\"nodeOrder\": [1, 2, 3, 4, 5, 6]"), "{c}");
        // Choice entries in Index order (131 before 132), not row order.
        let (p131, p132) = (
            c.find("\"id\": 131").unwrap(),
            c.find("\"id\": 132").unwrap(),
        );
        assert!(p131 < p132, "choice entries must follow Index order");
        // Override name and icon win; SpellName/SpellMisc fill the rest.
        assert!(c.contains("\"name\": \"Renamed\""), "{c}");
        assert!(c.contains("\"iconFdid\": 555"), "{c}");
        assert!(c.contains("\"name\": \"Left Pick\""), "{c}");
        // Base-difficulty icon row wins for spell 1002.
        assert!(c.contains("\"iconFdid\": 7002"), "{c}");
        assert!(!c.contains("7999"), "{c}");
        // Listfile-resolved icon name lands next to its fdid.
        assert!(c.contains("\"icon\": \"spell_root\""), "{c}");
        // Granted root, spec gating, point gate, costs, edges.
        assert!(c.contains("\"granted\": true"), "{c}");
        assert!(c.contains("\"visibleFor\": [62]"), "{c}");
        assert!(c.contains("\"reqPoints\": 8"), "{c}");
        assert!(
            c.contains("\"costs\": [{\"currency\": 601, \"amount\": 1}]"),
            "{c}"
        );
        assert!(c.contains("\"next\": [2]"), "{c}");
        // Hero subtree with both specs eligible; currency order by Index.
        assert!(
            c.contains("{\"id\": 77, \"name\": \"Sunfury\", \"specs\": [62, 63]}"),
            "{c}"
        );
        assert!(
            c.contains(
                "\"currencies\": [{\"index\": 0, \"id\": 601}, {\"index\": 1, \"id\": 602}]"
            ),
            "{c}"
        );
        // Deterministic: same input, same bytes.
        let g2 = generate(&base_tables(), &icons, "12.1.0.69497").unwrap();
        assert_eq!(g.content, g2.content);
    }

    #[test]
    fn valid_json() {
        // The document must parse as JSON; lean on serde-free hand checks:
        // balanced braces/brackets outside strings and no trailing commas.
        let g = generate(&base_tables(), &HashMap::new(), "1.2.3.4").unwrap();
        let (mut depth, mut in_str, mut esc) = (0i64, false, false);
        let mut last_significant = ' ';
        for ch in g.content.chars() {
            if in_str {
                match (esc, ch) {
                    (true, _) => esc = false,
                    (false, '\\') => esc = true,
                    (false, '"') => in_str = false,
                    _ => {}
                }
                continue;
            }
            match ch {
                '"' => in_str = true,
                '{' | '[' => depth += 1,
                '}' | ']' => {
                    assert_ne!(last_significant, ',', "trailing comma before {ch}");
                    depth -= 1;
                }
                _ => {}
            }
            if !ch.is_whitespace() {
                last_significant = ch;
            }
        }
        assert_eq!(depth, 0, "unbalanced brackets");
        assert!(!in_str, "unterminated string");
    }
}
