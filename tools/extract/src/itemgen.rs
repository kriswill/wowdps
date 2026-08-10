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
/// (from wowdev/wow-listfile; stable per file, forever).
pub const TABLES: [(&str, u32); 4] = [
    ("Item", 841626),
    ("ItemEffect", 969941),
    ("ItemXItemEffect", 3177687),
    ("SpellEffect", 1140088),
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
    for row in &xref.rows {
        let Some(&kind) = item_kind.get(cell(row, x_item, "ItemXItemEffect")?) else {
            continue;
        };
        let Some(&spell) = effect_spell.get(cell(row, x_effect, "ItemXItemEffect")?) else {
            continue;
        };
        if kind == "Trinket" {
            trinket_seeds.push(spell);
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

    Ok(Generated {
        chased: entries.len() - direct,
        spells: entries.len(),
        trinkets,
        content: emit(&entries, build)?,
    })
}

fn emit(table: &[(u32, u8)], build: &str) -> Result<String, String> {
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
                 200,0,1,0\n\
                 300,0,5,0\n\
                 400,2,7,13\n",
            )
            .unwrap(),
        );
        t.insert(
            "ItemEffect",
            parse_csv(
                "ID,SpellID\n\
                 1,5000\n2,6000\n3,7000\n4,8000\n",
            )
            .unwrap(),
        );
        t.insert(
            "ItemXItemEffect",
            parse_csv(
                "ID,ItemEffectID,ItemID\n\
                 1,1,100\n2,2,200\n3,3,300\n4,4,400\n",
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

    #[test]
    fn table_is_sorted() {
        let g = generate(&tables(), "1.2.3.4").unwrap();
        let ids: Vec<u32> = g
            .content
            .split('(')
            .filter_map(|s| s.split(',').next()?.parse().ok())
            .collect();
        assert!(ids.windows(2).all(|w| w[0] < w[1]), "{ids:?}");
    }
}
