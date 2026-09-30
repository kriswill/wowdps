//! The `crates/core/src/item_spells.rs` generator: spell id → the kind of
//! item that grants it (CONTRACT.md R12), decoded from the local install the
//! same way `classgen` decodes the class table.
//!
//! The join is three tables deep:
//!
//!   ItemXItemEffect (ItemID → ItemEffectID) → ItemEffect (→ SpellID)
//!   and Item (ClassID / SubclassID / InventoryType) to say what the item *is*
//!
//! Trinkets additionally chase `SpellEffect.EffectTriggerSpell` two levels out
//! from their effect spells. A trinket's on-use effect appears in ItemEffect
//! directly, but its *proc* is almost never that spell — the equip effect
//! triggers a second spell, and that second spell is the buff the combat log
//! actually reports. Without the chase, `TrinketProc` markers would be empty
//! for most trinkets in the game.
//!
//! A spell granted by items of several kinds keeps the most specific one
//! (`KIND_ORDER`), so output is deterministic per build regardless of table
//! order.
//!
//! The chase is deliberately generous and therefore not authoritative: some
//! trinkets trigger ordinary class spells (a trinket that procs a free
//! Fireball puts spell 133 in here as a "trinket"). The meter resolves that
//! by consulting `class_spells` FIRST — a spell any class can cast is never
//! an item marker — so this table only ever has to answer for the spells
//! nothing else claims.

use crate::table::Csv;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fmt::Write as _;

/// The tables the generator consumes, with their FileDataIDs
/// (from wowdev/wow-listfile; stable per file, forever). ItemSparse is
/// only for the trinkets' names (R26).
pub const TABLES: [(&str, u32); 5] = [
    ("Item", 841626),
    ("ItemEffect", 969941),
    ("ItemXItemEffect", 3177687),
    ("SpellEffect", 1140088),
    ("ItemSparse", 1572924),
];

/// Item.InventoryType for an equipped trinket.
const INVTYPE_TRINKET: &str = "12";
/// Item.ClassID for consumables.
const CLASS_CONSUMABLE: &str = "0";

/// Emitted kind codes; must match `wowdps_model::ItemKind::code`, and the
/// KINDS array in the generated file. Earlier entries win a collision — a
/// trinket that is also flagged consumable stays a trinket.
const KIND_ORDER: [&str; 5] = ["Trinket", "Potion", "Flask", "Food", "Consumable"];

/// How far to follow EffectTriggerSpell out of a trinket's effect spells.
/// One level catches the common "equip effect → proc buff" shape; two catches
/// the "equip effect → proc → damage/buff" trinkets. Beyond that the chain
/// starts pulling in generic shared spells.
const TRIGGER_DEPTH: usize = 2;

#[derive(Debug)]
pub struct Generated {
    pub content: String,
    pub spells: usize,
    pub trinkets: usize,
    /// Proc spells found only by chasing EffectTriggerSpell.
    pub chased: usize,
    /// R26: trinket spells attributed to exactly one named trinket.
    pub owned: usize,
}

/// One cell of a CSV row. The column index comes from `Csv::col`, so a miss
/// means the row itself is short — a malformed table, not a bug here.
fn cell<'a>(row: &'a [String], c: usize, what: &str) -> Result<&'a str, String> {
    row.get(c)
        .map(String::as_str)
        .ok_or_else(|| format!("{what}: row has no column {c}"))
}

/// Classify an item by what the client says it is. `None` for everything we
/// draw no marker for (gear, weapons, quest items…).
fn classify(class_id: &str, subclass_id: &str, inv_type: &str) -> Option<&'static str> {
    if inv_type == INVTYPE_TRINKET {
        return Some("Trinket");
    }
    if class_id != CLASS_CONSUMABLE {
        return None;
    }
    Some(match subclass_id {
        "1" => "Potion",
        // Elixirs and flasks are one concept for a damage meter's purposes.
        "2" | "3" => "Flask",
        "5" => "Food",
        _ => "Consumable",
    })
}

pub fn generate(tables: &HashMap<&str, Csv>, build: &str) -> Result<Generated, String> {
    let get = |name: &str| {
        tables
            .get(name)
            .ok_or_else(|| format!("missing table {name}"))
    };
    let rank_of: HashMap<&str, usize> = KIND_ORDER
        .iter()
        .enumerate()
        .map(|(i, k)| (*k, i))
        .collect();
    // Every kind written into the table comes from KIND_ORDER, so a miss is
    // impossible; ranking it last keeps that from ever being a panic.
    let rank = |k: &str| rank_of.get(k).copied().unwrap_or(KIND_ORDER.len());

    // Item -> kind, for the items we care about at all.
    let item = get("Item")?;
    let (c_id, c_class, c_sub, c_inv) = (
        item.col("ID")?,
        item.col("ClassID")?,
        item.col("SubclassID")?,
        item.col("InventoryType")?,
    );
    let mut item_kind: HashMap<&str, &'static str> = HashMap::new();
    for row in &item.rows {
        let kind = classify(
            cell(row, c_class, "Item")?,
            cell(row, c_sub, "Item")?,
            cell(row, c_inv, "Item")?,
        );
        if let Some(k) = kind {
            item_kind.insert(cell(row, c_id, "Item")?, k);
        }
    }

    // ItemEffect -> the spell it casts.
    let effects = get("ItemEffect")?;
    let (e_id, e_spell) = (effects.col("ID")?, effects.col("SpellID")?);
    let mut effect_spell: HashMap<&str, u32> = HashMap::new();
    for row in &effects.rows {
        let spell: u32 = cell(row, e_spell, "ItemEffect")?.parse().unwrap_or(0);
        if spell != 0 {
            effect_spell.insert(cell(row, e_id, "ItemEffect")?, spell);
        }
    }

    // The join: every (item, effect) pair contributes its spell under the
    // item's kind.
    let xref = get("ItemXItemEffect")?;
    let (x_effect, x_item) = (xref.col("ItemEffectID")?, xref.col("ItemID")?);
    let mut table: BTreeMap<u32, &'static str> = BTreeMap::new();
    let mut trinket_seeds: Vec<u32> = Vec::new();
    // R26: each trinket's own effect spells, for the per-item chase below.
    let mut trinket_roots: BTreeMap<u32, Vec<u32>> = BTreeMap::new();
    for row in &xref.rows {
        let item_id = cell(row, x_item, "ItemXItemEffect")?;
        let Some(&kind) = item_kind.get(item_id) else {
            continue;
        };
        let Some(&spell) = effect_spell.get(cell(row, x_effect, "ItemXItemEffect")?) else {
            continue;
        };
        if kind == "Trinket" {
            trinket_seeds.push(spell);
            if let Ok(item) = item_id.parse::<u32>() {
                trinket_roots.entry(item).or_default().push(spell);
            }
        }
        let slot = table.entry(spell).or_insert(kind);
        if rank(kind) < rank(slot) {
            *slot = kind;
        }
    }
    let direct = table.len();

    // Trinket proc chase: spell -> the spells its effects trigger.
    let se = get("SpellEffect")?;
    let (s_spell, s_trigger) = (se.col("SpellID")?, se.col("EffectTriggerSpell")?);
    let mut triggers: HashMap<u32, Vec<u32>> = HashMap::new();
    for row in &se.rows {
        let trig: u32 = cell(row, s_trigger, "SpellEffect")?.parse().unwrap_or(0);
        if trig == 0 {
            continue;
        }
        let spell: u32 = cell(row, s_spell, "SpellEffect")?.parse().unwrap_or(0);
        if spell != 0 {
            triggers.entry(spell).or_default().push(trig);
        }
    }

    let mut seen: HashSet<u32> = trinket_seeds.iter().copied().collect();
    let mut frontier = trinket_seeds;
    for _ in 0..TRIGGER_DEPTH {
        let mut next = Vec::new();
        for spell in frontier {
            for &trig in triggers.get(&spell).into_iter().flatten() {
                if seen.insert(trig) {
                    next.push(trig);
                    // A chased spell never downgrades an item's own kind: a
                    // potion that happens to sit on a trinket's trigger chain
                    // stays a potion.
                    table.entry(trig).or_insert("Trinket");
                }
            }
        }
        frontier = next;
    }

    let trinkets = table.values().filter(|k| **k == "Trinket").count();
    let entries: Vec<(u32, u8)> = table
        .iter()
        .map(|(spell, kind)| (*spell, rank(kind) as u8))
        .collect();

    let owners = trinket_owners(&trinket_roots, &triggers, &table, get("ItemSparse")?)?;

    Ok(Generated {
        chased: entries.len() - direct,
        spells: entries.len(),
        trinkets,
        owned: owners.spells.len(),
        content: emit(&entries, &owners, build)?,
    })
}

/// R26: which trinket each trinket spell belongs to, for the ability tree
/// (a proc's damage nests under the item that fired it). Each trinket's
/// own effect spells are chased exactly as the kind table's are — the
/// union of those per-item reaches IS the global chase — and a spell
/// keeps an owner only when every trinket reaching it bears ONE name (the
/// same trinket re-issued under several item ids stays itself; a stat
/// proc a hundred trinkets share belongs to none). The name is ItemSparse's
/// `Display_lang`; an item without one attributes nothing. A spell the
/// kind table files under a consumable is never a trinket's.
struct Owners {
    /// Distinct names, sorted; `spells` index into it.
    names: Vec<String>,
    /// (spell id, lowest item id of the name, name index), by spell id.
    spells: Vec<(u32, u32, u16)>,
}

fn trinket_owners(
    roots: &BTreeMap<u32, Vec<u32>>,
    triggers: &HashMap<u32, Vec<u32>>,
    kinds: &BTreeMap<u32, &'static str>,
    sparse: &Csv,
) -> Result<Owners, String> {
    let (c_id, c_name) = (sparse.col("ID")?, sparse.col("Display_lang")?);
    let mut name_of: HashMap<u32, &str> = HashMap::new();
    for row in &sparse.rows {
        let name = cell(row, c_name, "ItemSparse")?;
        if let Ok(id) = cell(row, c_id, "ItemSparse")?.parse::<u32>()
            && !name.is_empty()
        {
            name_of.insert(id, name);
        }
    }
    // spell -> (name -> lowest item id bearing it)
    let mut reach: BTreeMap<u32, BTreeMap<&str, u32>> = BTreeMap::new();
    for (&item, spells) in roots {
        let Some(&name) = name_of.get(&item) else {
            continue;
        };
        let mut seen: HashSet<u32> = spells.iter().copied().collect();
        let mut frontier: Vec<u32> = spells.clone();
        let mut hit: Vec<u32> = spells.clone();
        for _ in 0..TRIGGER_DEPTH {
            let mut next = Vec::new();
            for spell in frontier {
                for &trig in triggers.get(&spell).into_iter().flatten() {
                    if seen.insert(trig) {
                        next.push(trig);
                        hit.push(trig);
                    }
                }
            }
            frontier = next;
        }
        for spell in hit {
            let slot = reach.entry(spell).or_default().entry(name).or_insert(item);
            *slot = (*slot).min(item);
        }
    }
    let mut owned: Vec<(u32, u32, &str)> = Vec::new();
    for (spell, names) in &reach {
        if kinds.get(spell).is_some_and(|k| *k != "Trinket") {
            continue;
        }
        if names.len() == 1
            && let Some((name, item)) = names.iter().next()
        {
            owned.push((*spell, *item, *name));
        }
    }
    let mut names: Vec<String> = owned.iter().map(|(_, _, n)| n.to_string()).collect();
    names.sort();
    names.dedup();
    let spells = owned
        .into_iter()
        .map(|(spell, item, name)| {
            let i = names
                .binary_search_by(|n| n.as_str().cmp(name))
                .map_err(|_| format!("trinket name {name:?} lost"))?;
            let i = u16::try_from(i).map_err(|_| "more than 65535 trinket names".to_string())?;
            Ok((spell, item, i))
        })
        .collect::<Result<Vec<_>, String>>()?;
    Ok(Owners { names, spells })
}

/// A Rust string literal for `s`: quotes, backslashes and control
/// characters escaped (item names carry apostrophes and the odd quote).
fn literal(s: &str) -> String {
    format!("{s:?}")
}

fn emit(table: &[(u32, u8)], owners: &Owners, build: &str) -> Result<String, String> {
    let mut o = String::new();
    o.push_str("//! GENERATED by tools/gen-item-spells.sh — do not edit by hand.\n");
    // No timestamp: same build in, same bytes out.
    writeln!(
        o,
        "//! Source: local client DB2s via wowdps-extract, build {build}."
    )
    .map_err(|e| format!("emit: {e}"))?;
    writeln!(o, "//! {} item spells.", table.len()).map_err(|e| format!("emit: {e}"))?;
    o.push_str(
        "//!\n\
         //! Maps a combat-log spell id to the kind of item that grants it, so the\n\
         //! meter can mark trinket uses, trinket procs and consumables on a player's\n\
         //! timeline (CONTRACT.md R12).\n\
         \n\
         use wowdps_model::ItemKind;\n\
         \n\
         /// The kind of item a spell comes from, or `None` for a spell no item grants.\n\
         pub(crate) fn item_kind(spell_id: u32) -> Option<ItemKind> {\n\
         \x20   let i = TABLE.binary_search_by_key(&spell_id, |e| e.0).ok()?;\n\
         \x20   let &(_, code) = TABLE.get(i)?;\n\
         \x20   KINDS.get(code as usize).copied()\n\
         }\n\
         \n\
         /// R26: the one trinket a spell comes from — (item id, the item's name)\n\
         /// — or `None` for a spell no single trinket grants: the ability tree\n\
         /// nests a proc's damage under the trinket that fired it.\n\
         pub(crate) fn trinket_of(spell_id: u32) -> Option<(u32, &'static str)> {\n\
         \x20   let i = TRINKETS.binary_search_by_key(&spell_id, |e| e.0).ok()?;\n\
         \x20   let &(_, item, name) = TRINKETS.get(i)?;\n\
         \x20   Some((item, NAMES.get(name as usize).copied()?))\n\
         }\n\
         \n\
         const KINDS: [ItemKind; 5] = [\n",
    );
    for kind in KIND_ORDER {
        writeln!(o, "    ItemKind::{kind},").map_err(|e| format!("emit: {e}"))?;
    }
    o.push_str(
        "];\n\
         \n\
         /// (spell id, kind code), sorted by spell id.\n\
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
         /// R26: the trinkets' names, sorted; `TRINKETS` indexes here.\n\
         #[rustfmt::skip]\n\
         static NAMES: &[&str] = &[\n",
    );
    for name in &owners.names {
        writeln!(o, "    {},", literal(name)).map_err(|e| format!("emit: {e}"))?;
    }
    writeln!(
        o,
        "];\n\n/// R26: (spell id, item id, name index), sorted by spell id.\n\
         #[rustfmt::skip]\nstatic TRINKETS: &[(u32, u32, u16)] = &["
    )
    .map_err(|e| format!("emit: {e}"))?;
    for chunk in owners.spells.chunks(6) {
        let cells: Vec<String> = chunk
            .iter()
            .map(|(s, i, n)| format!("({s},{i},{n}),"))
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
         \x20       assert!(super::TRINKETS.windows(2).all(|w| w[0].0 < w[1].0));\n\
         \x20       let names = super::NAMES.len();\n\
         \x20       assert!(super::TRINKETS.iter().all(|t| (t.2 as usize) < names));\n\
         \x20   }\n\
         }\n",
    );
    Ok(o)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::table::parse_csv;

    fn tables() -> HashMap<&'static str, Csv> {
        let mut t = HashMap::new();
        t.insert(
            "Item",
            parse_csv(
                "ID,ClassID,SubclassID,InventoryType\n\
                 100,4,0,12\n\
                 110,4,0,12\n\
                 120,4,0,12\n\
                 130,4,0,12\n\
                 131,4,0,12\n\
                 200,0,1,0\n\
                 300,0,5,0\n\
                 400,2,7,13\n",
            )
            .unwrap(),
        );
        // R26: 110 and 120 share a stat proc under two names; 130 and 131
        // are one trinket issued twice; 200 (a potion) has a name too.
        t.insert(
            "ItemSparse",
            parse_csv(
                "ID,Display_lang\n\
                 100,Omnium Folio\n\
                 110,Signet of Stats\n\
                 120,Idol of Stats\n\
                 130,Twice-Issued Charm\n\
                 131,Twice-Issued Charm\n\
                 200,Tempered Potion\n",
            )
            .unwrap(),
        );
        t.insert(
            "ItemEffect",
            parse_csv(
                "ID,SpellID\n\
                 1,5000\n2,6000\n3,7000\n4,8000\n\
                 5,9000\n6,9000\n7,9100\n8,9100\n",
            )
            .unwrap(),
        );
        t.insert(
            "ItemXItemEffect",
            parse_csv(
                "ID,ItemEffectID,ItemID\n\
                 1,1,100\n2,2,200\n3,3,300\n4,4,400\n\
                 5,5,110\n6,6,120\n7,7,130\n8,8,131\n",
            )
            .unwrap(),
        );
        t.insert(
            "SpellEffect",
            parse_csv(
                "ID,EffectTriggerSpell,SpellID\n\
                 1,5001,5000\n2,5002,5001\n3,5003,5002\n4,0,6000\n",
            )
            .unwrap(),
        );
        t
    }

    #[test]
    fn classifies_by_item_class_and_slot() {
        let g = generate(&tables(), "1.2.3.4").unwrap();
        // Trinket (5000), potion (6000), food (7000); the weapon (8000) is
        // not markable and must not appear.
        assert!(g.content.contains("(5000,0),"));
        assert!(g.content.contains("(6000,1),"));
        assert!(g.content.contains("(7000,3),"));
        assert!(!g.content.contains("(8000,"));
        assert!(g.content.contains("build 1.2.3.4"));
    }

    #[test]
    fn chases_trinket_procs_two_levels_and_no_further() {
        let g = generate(&tables(), "1.2.3.4").unwrap();
        assert!(g.content.contains("(5001,0),"), "first trigger level");
        assert!(g.content.contains("(5002,0),"), "second trigger level");
        assert!(!g.content.contains("(5003,"), "depth is bounded");
        assert_eq!(g.chased, 2);
    }

    /// The emitted body of `static <name>`: from its line to the `];`.
    fn section<'a>(content: &'a str, name: &str) -> &'a str {
        let (_, rest) = content.split_once(&format!("static {name}")).unwrap();
        rest.split_once("];").unwrap().0
    }

    #[test]
    fn table_is_sorted() {
        let g = generate(&tables(), "1.2.3.4").unwrap();
        for name in ["TABLE", "TRINKETS"] {
            let ids: Vec<u32> = section(&g.content, name)
                .split('(')
                .filter_map(|s| s.split(',').next()?.parse().ok())
                .collect();
            assert!(!ids.is_empty(), "{name}");
            assert!(ids.windows(2).all(|w| w[0] < w[1]), "{name}: {ids:?}");
        }
    }

    /// R26: a trinket owns its effect spell and the procs chased out of
    /// it, by its name; the name table holds it once.
    #[test]
    fn a_trinket_owns_its_chased_procs() {
        let g = generate(&tables(), "1.2.3.4").unwrap();
        let trinkets = section(&g.content, "TRINKETS");
        let names = section(&g.content, "NAMES");
        let folio = names
            .lines()
            .skip(1)
            .position(|l| l.trim() == "\"Omnium Folio\",")
            .unwrap();
        for spell in [5000, 5001, 5002] {
            assert!(
                trinkets.contains(&format!("({spell},100,{folio}),")),
                "{spell}: {trinkets}"
            );
        }
        assert!(!trinkets.contains("(5003,"), "past the chase's depth");
        assert!(!trinkets.contains("(6000,"), "a potion is no trinket's");
        assert!(!names.contains("Tempered Potion"));
    }

    /// R26: a proc two differently named trinkets share belongs to
    /// neither; one trinket issued under two ids is one owner, the lower.
    #[test]
    fn a_shared_proc_has_no_owner_and_a_reissue_has_one() {
        let g = generate(&tables(), "1.2.3.4").unwrap();
        let trinkets = section(&g.content, "TRINKETS");
        assert!(!trinkets.contains("(9000,"), "{trinkets}");
        assert!(trinkets.contains("(9100,130,"), "{trinkets}");
        assert_eq!(g.owned, 4, "5000, 5001, 5002 and 9100");
    }
}
