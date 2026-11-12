//! The `crates/core/src/absorb_spells.rs` generator: the set of spell ids
//! that place an absorb shield (CONTRACT.md R20's absorb-spell gate), read
//! from the local install the same way `rolegen` proves its entries.
//!
//! Unlike `rolegen` this table is *discovered*, not curated: every spell
//! with a `SpellEffect` row whose `EffectAura` is 69 (SCHOOL_ABSORB) on any
//! difficulty is in. The ledger admits a Buff aura only when its id is here
//! — never on the log's absorb trailer alone, because Feast of Souls, Soul
//! Fragments and every `Second Wind …,BUFF,0,0` carry one too.
//!
//! Each edition of the game has its own table, discovered from its own
//! client the same way: Retail's `absorb_spells.rs`, and Forever's
//! `forever/absorb_spells.rs` (`--product wowf`), whose ids are its own.
//!
//! Two safety rails, each the edition's own (`Rail`):
//!
//! - the edition's fixture shields (Retail's `FIXTURE_SHIELDS`: Power Word:
//!   Shield 17, Ice Barrier 11426, Blood Shield 77535; Forever's
//!   `FOREVER_FIXTURE_SHIELDS`: the Power Word: Shield its fixture casts)
//!   must resolve, or the generator fails naming the id — a schema drift
//!   that emptied the table would otherwise ship as a silent "nothing is a
//!   shield";
//! - the edition's committed census of real logs
//!   (`tools/absorb-spells-census.csv`, Forever's
//!   `tools/forever/absorb-spells-census.csv`, written by
//!   `tools/census-absorb-spells.sh`: every `SPELL_ABSORBED`'s absorb spell,
//!   counted per (id, name) per log) is printed beside each entry in the
//!   review twin, and every census id NOT in the table is listed under its
//!   own heading — those are the shields the ledger still counts as
//!   `known = false` applied (their consumed amounts are never lost), so the
//!   reviewer sees exactly what the client tables miss.
//!
//! Output is two files: the table (`absorb_spells.rs`) and its review twin
//! (`absorb_spells.expected.md`).

use crate::rolegen::Census;
use crate::table::{Tables, cell};
use std::collections::BTreeMap;
use std::fmt::Write as _;

/// The tables the generator consumes (their FileDataIDs: `table::FILE_IDS`).
pub const TABLES: [&str; 2] = ["SpellName", "SpellEffect"];

/// SpellEffect.EffectAura for SCHOOL_ABSORB.
const AURA_SCHOOL_ABSORB: &str = "69";

/// The shields Retail's committed fixtures cast; the generator fails when
/// any is missing from the discovered table.
pub const FIXTURE_SHIELDS: [(u32, &str); 3] = [
    (17, "Power Word: Shield"),
    (11426, "Ice Barrier"),
    (77535, "Blood Shield"),
];

/// The shield Forever's committed fixture (`forever.txt`) casts.
pub const FOREVER_FIXTURE_SHIELDS: [(u32, &str); 1] = [(17, "Power Word: Shield")];

/// What one edition's run is held to: its fixtures' shields, which must be
/// in the discovered table, and its committed census, which the review twin
/// names.
#[derive(Debug, Clone, Copy)]
pub struct Rail {
    pub shields: &'static [(u32, &'static str)],
    pub census: &'static str,
}

/// Retail's rail.
pub const RETAIL: Rail = Rail {
    shields: &FIXTURE_SHIELDS,
    census: "tools/absorb-spells-census.csv",
};

/// Forever's rail.
pub const FOREVER: Rail = Rail {
    shields: &FOREVER_FIXTURE_SHIELDS,
    census: "tools/forever/absorb-spells-census.csv",
};

/// The rail of the edition a product's client is (`.build.info`'s row):
/// Forever's for `wowf`, else Retail's.
pub fn rail(product: &str) -> Rail {
    if product == "wowf" { FOREVER } else { RETAIL }
}

#[derive(Debug)]
pub struct Generated {
    /// `absorb_spells.rs`.
    pub content: String,
    /// `absorb_spells.expected.md`.
    pub expected: String,
    pub spells: usize,
    /// Census ids absent from the table: (id, log name, counts per log).
    pub unknown: Vec<(u32, String, Vec<u64>)>,
}

/// Retail's table ([`generate_for`] with [`RETAIL`]).
pub fn generate(tables: &Tables, census: &Census, build: &str) -> Result<Generated, String> {
    generate_for(tables, census, build, RETAIL)
}

/// One edition's table, held to its `rail`.
pub fn generate_for(
    tables: &Tables,
    census: &Census,
    build: &str,
    rail: Rail,
) -> Result<Generated, String> {
    // SpellEffect: every spell with a SCHOOL_ABSORB aura on any difficulty.
    let se = tables.get("SpellEffect")?;
    let (e_spell, e_aura) = (se.col("SpellID")?, se.col("EffectAura")?);
    let mut table: BTreeMap<u32, &str> = BTreeMap::new();
    for row in &se.rows {
        if cell(row, e_aura, "SpellEffect")? != AURA_SCHOOL_ABSORB {
            continue;
        }
        let id: u32 = cell(row, e_spell, "SpellEffect")?.parse().unwrap_or(0);
        if id != 0 {
            table.insert(id, "");
        }
    }

    // SpellName: the names, for the review twin only (an unnamed id stays).
    let sn = tables.get("SpellName")?;
    let (n_id, n_name) = (sn.col("ID")?, sn.col("Name_lang")?);
    for row in &sn.rows {
        let id: u32 = cell(row, n_id, "SpellName")?.parse().unwrap_or(0);
        if let Some(slot) = table.get_mut(&id) {
            *slot = cell(row, n_name, "SpellName")?;
        }
    }

    for &(id, name) in rail.shields {
        if !table.contains_key(&id) {
            return Err(format!(
                "absorb spell {id} ({name}): no SCHOOL_ABSORB SpellEffect row — the fixture's \
                 shield would not ledger; schema drift?"
            ));
        }
    }

    let mut unknown: Vec<(u32, String, Vec<u64>)> = census
        .counts
        .iter()
        .filter(|(id, _)| !table.contains_key(id))
        .map(|(id, (name, per_log))| (*id, name.clone(), per_log.clone()))
        .collect();
    unknown.sort_by_key(|e| e.0);

    let ids: Vec<u32> = table.keys().copied().collect();
    Ok(Generated {
        spells: ids.len(),
        content: emit(&ids, build, rail)?,
        expected: emit_expected(&table, census, &unknown, build, rail)?,
        unknown,
    })
}

/// The rail's shield ids, as the generated header lists them: `17, 11426`.
fn shield_ids(rail: Rail) -> String {
    let ids: Vec<String> = rail.shields.iter().map(|(id, _)| id.to_string()).collect();
    ids.join(", ")
}

fn emit(table: &[u32], build: &str, rail: Rail) -> Result<String, String> {
    let mut o = String::new();
    o.push_str("//! GENERATED by tools/gen-absorb-spells.sh — do not edit by hand.\n");
    // No timestamp: same build in, same bytes out.
    writeln!(
        o,
        "//! Source: local client DB2s via wowdps-extract, build {build}."
    )
    .map_err(|e| format!("emit: {e}"))?;
    writeln!(o, "//! {} absorb spells.", table.len()).map_err(|e| format!("emit: {e}"))?;
    o.push_str(
        "//!\n\
         //! Every spell id with a `SpellEffect` row whose `EffectAura` is 69\n\
         //! (SCHOOL_ABSORB): the shields the R20 ledger admits on a Buff aura apply\n\
         //! (CONTRACT.md R20). Discovered by tools/extract/src/absorbgen.rs; the\n",
    );
    writeln!(
        o,
        "//! fixture's shields ({}) are proven present at generation",
        shield_ids(rail)
    )
    .map_err(|e| format!("emit: {e}"))?;
    o.push_str(
        "//! time, and the real-log census sits in absorb_spells.expected.md.\n\
         \n\
         /// Whether a spell places an absorb shield.\n\
         #[allow(dead_code)] // consumed by the meter's R20 shield ledger\n\
         pub(crate) fn is_absorb_spell(spell_id: u32) -> bool {\n\
         \x20   TABLE.binary_search(&spell_id).is_ok()\n\
         }\n\
         \n\
         /// Spell ids, sorted.\n\
         #[rustfmt::skip]\n\
         static TABLE: &[u32] = &[\n",
    );
    for chunk in table.chunks(10) {
        let cells: Vec<String> = chunk.iter().map(|s| format!("{s},")).collect();
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
         \x20       assert!(super::TABLE.windows(2).all(|w| w[0] < w[1]));\n\
         \x20   }\n\
         \n\
         \x20   /// The fixtures' shields resolve, and a stranger does not.\n\
         \x20   #[test]\n\
         \x20   fn fixture_shields_resolve() {\n",
    );
    for (id, _) in rail.shields {
        writeln!(o, "        assert!(super::is_absorb_spell({id}));")
            .map_err(|e| format!("emit: {e}"))?;
    }
    o.push_str(
        "\x20       assert!(!super::is_absorb_spell(0));\n\
         \x20   }\n\
         }\n",
    );
    Ok(o)
}

fn emit_expected(
    table: &BTreeMap<u32, &str>,
    census: &Census,
    unknown: &[(u32, String, Vec<u64>)],
    build: &str,
    rail: Rail,
) -> Result<String, String> {
    let mut o = String::new();
    let w = |o: &mut String, s: &str| -> Result<(), String> {
        writeln!(o, "{s}").map_err(|e| format!("emit: {e}"))
    };
    w(
        &mut o,
        "# absorb_spells.rs — the discovered table and its evidence",
    )?;
    w(&mut o, "")?;
    w(
        &mut o,
        "GENERATED by tools/gen-absorb-spells.sh beside `absorb_spells.rs` — do not edit by hand.",
    )?;
    let seen = table
        .keys()
        .filter(|id| census.counts.contains_key(id))
        .count();
    writeln!(
        o,
        "Build {build}, {} absorb spells; {seen} seen in the census, {} census ids outside the table.",
        table.len(),
        unknown.len()
    )
    .map_err(|e| format!("emit: {e}"))?;
    w(&mut o, "")?;
    w(&mut o, "## Rules")?;
    w(&mut o, "")?;
    w(
        &mut o,
        "- Membership is discovered by `tools/extract/src/absorbgen.rs`: every spell id with\n\
         \x20 a `SpellEffect` row whose `EffectAura == 69` (SCHOOL_ABSORB) on some difficulty.\n\
         \x20 Nothing is curated; the name column comes from `SpellName` for review only.",
    )?;
    let shields: Vec<String> = rail
        .shields
        .iter()
        .map(|(id, name)| format!("{name} {id}"))
        .collect();
    w(
        &mut o,
        &wrap_rule(&format!(
            "- The fixtures' shields — {} — must be in the result or the generator fails, naming the id.",
            shields.join(", ")
        )),
    )?;
    // Unwrapped: its next line goes on with the command's words.
    w(
        &mut o,
        &format!(
            "- The census `{}` (`tools/census-absorb-spells.sh",
            rail.census
        ),
    )?;
    w(
        &mut o,
        "\x20 <log>...`: every `SPELL_ABSORBED`'s absorb spell — field n−5 of n — counted per\n\
         \x20 (id, name) per log) is evidence, not a gate: its counts print beside each entry,\n\
         \x20 and every census id the table lacks is listed under *Census ids outside the\n\
         \x20 table*. Those are the shields the R20 ledger opens as `known = false` applied on\n\
         \x20 their first `SPELL_ABSORBED` — their consumed amounts still count, only the\n\
         \x20 applied/wasted split is unknown.\n\
         - The engine (CONTRACT.md R20) admits a Buff aura into the shield ledger only when\n\
         \x20 `is_absorb_spell(id)` holds — never on the aura trailer alone.",
    )?;
    w(&mut o, "")?;
    w(&mut o, "## Census ids outside the table")?;
    w(&mut o, "")?;
    if unknown.is_empty() {
        w(&mut o, "None.")?;
    } else {
        let (header, rule) = census_columns("| id | name (from the log) |", census)?;
        w(&mut o, &header)?;
        w(&mut o, &rule)?;
        for (id, name, per_log) in unknown {
            let mut line = format!("| {id} | {name} |");
            for n in per_log {
                write!(line, " {n} |").map_err(|e| format!("emit: {e}"))?;
            }
            w(&mut o, &line)?;
        }
    }
    w(&mut o, "")?;
    w(&mut o, "## Entries")?;
    w(&mut o, "")?;
    let (header, rule) = census_columns("| id | name |", census)?;
    w(&mut o, &header)?;
    w(&mut o, &rule)?;
    for (id, name) in table {
        let mut line = format!("| {id} | {name} |");
        match census.counts.get(id) {
            Some((_, per_log)) => {
                for n in per_log {
                    write!(line, " {n} |").map_err(|e| format!("emit: {e}"))?;
                }
            }
            None => {
                for _ in &census.logs {
                    line.push_str(" 0 |");
                }
            }
        }
        w(&mut o, &line)?;
    }
    Ok(o)
}

/// A rule of the review twin's list, wrapped as the hand-wrapped ones are:
/// words to 80 columns, each line after the first indented by two.
fn wrap_rule(text: &str) -> String {
    let mut lines: Vec<String> = Vec::new();
    let mut line = String::new();
    for word in text.split(' ') {
        let indent = if lines.is_empty() { "" } else { "  " };
        let len = |s: &str| s.chars().count();
        if !line.is_empty() && len(indent) + len(&line) + 1 + len(word) > 80 {
            lines.push(format!("{indent}{line}"));
            line.clear();
        }
        if !line.is_empty() {
            line.push(' ');
        }
        line.push_str(word);
    }
    let indent = if lines.is_empty() { "" } else { "  " };
    lines.push(format!("{indent}{line}"));
    lines.join("\n")
}

/// A markdown table header + rule with one `observed (<log>)` column per
/// census log appended to `lead` (which ends in `|`).
fn census_columns(lead: &str, census: &Census) -> Result<(String, String), String> {
    let mut header = lead.to_string();
    let mut rule = String::from("| ---: |");
    for _ in 1..lead.matches('|').count() - 1 {
        rule.push_str(" --- |");
    }
    for log in &census.logs {
        write!(header, " observed ({log}) |").map_err(|e| format!("emit: {e}"))?;
        rule.push_str(" ---: |");
    }
    Ok((header, rule))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::table::parse_csv;

    fn tables() -> Tables {
        let mut t = Tables::default();
        t.insert(
            "SpellName",
            parse_csv(
                "ID,Name_lang\n\
                 17,Power Word: Shield\n\
                 11426,Ice Barrier\n\
                 77535,Blood Shield\n\
                 871,Shield Wall\n\
                 300,Nameless Later\n",
            )
            .unwrap(),
        );
        // 17 absorbs on two difficulties; 871 applies a non-absorb aura; 300
        // and 9999 absorb but only 300 has a SpellName row; a row with an
        // absorb aura under SpellID 0 is noise.
        t.insert(
            "SpellEffect",
            parse_csv(
                "ID,DifficultyID,Effect,EffectAura,SpellID\n\
                 1,0,6,69,17\n\
                 2,1,6,69,17\n\
                 3,0,6,69,11426\n\
                 4,0,6,69,77535\n\
                 5,0,6,22,871\n\
                 6,0,6,69,300\n\
                 7,0,6,69,9999\n\
                 8,0,6,69,0\n",
            )
            .unwrap(),
        );
        t
    }

    fn census() -> Census {
        Census::parse(
            "id,name,log 1,log 2\n\
             17,\"Power Word: Shield\",159,10\n\
             77535,\"Blood Shield\",371,0\n\
             5000,\"Feast of Souls\",9,9\n\
             4000,\"Unknown Shield\",1,0\n",
        )
        .unwrap()
    }

    #[test]
    fn discovers_sorted_table_with_names_and_census() {
        let g = generate(&tables(), &census(), "1.2.3.4").unwrap();
        assert_eq!(g.spells, 5);
        assert!(
            g.content.contains("    17, 300, 9999, 11426, 77535,\n"),
            "{}",
            g.content
        );
        assert!(g.content.contains("build 1.2.3.4"));
        assert!(g.content.contains("//! 5 absorb spells."));
        assert!(g.content.contains("pub(crate) fn is_absorb_spell"));
        assert!(
            g.expected
                .contains("| id | name | observed (log 1) | observed (log 2) |")
        );
        assert!(
            g.expected
                .contains("| 17 | Power Word: Shield | 159 | 10 |")
        );
        assert!(g.expected.contains("| 11426 | Ice Barrier | 0 | 0 |"));
        assert!(g.expected.contains("| 9999 |  | 0 | 0 |"), "{}", g.expected);
        // The census ids the table lacks are flagged, sorted by id.
        assert_eq!(
            g.unknown,
            vec![
                (4000, "Unknown Shield".to_string(), vec![1, 0]),
                (5000, "Feast of Souls".to_string(), vec![9, 9]),
            ]
        );
        assert!(g.expected.contains("| 4000 | Unknown Shield | 1 | 0 |"));
        assert!(g.expected.contains("| 5000 | Feast of Souls | 9 | 9 |"));
        assert!(g.expected.contains("2 census ids outside the table"));
        assert!(!g.expected.contains("| 871 |"));
    }

    #[test]
    fn missing_fixture_shield_fails_naming_it() {
        let mut t = tables();
        t.get_mut("SpellEffect")
            .unwrap()
            .rows
            .retain(|r| r[4] != "11426");
        let err = generate(&t, &census(), "b").unwrap_err();
        assert!(
            err.contains("11426") && err.contains("Ice Barrier") && err.contains("SCHOOL_ABSORB"),
            "{err}"
        );
        // A table with no absorb rows at all fails the same way, never
        // shipping an empty table.
        let mut t = tables();
        t.get_mut("SpellEffect").unwrap().rows.clear();
        assert!(generate(&t, &census(), "b").is_err());
    }

    #[test]
    fn missing_table_or_column_fails() {
        let mut t = tables();
        t.remove("SpellName");
        assert!(
            generate(&t, &census(), "b")
                .unwrap_err()
                .contains("SpellName")
        );
        let mut t = tables();
        t.insert("SpellEffect", parse_csv("ID,SpellID\n1,17\n").unwrap());
        assert!(
            generate(&t, &census(), "b")
                .unwrap_err()
                .contains("EffectAura")
        );
    }

    /// Retail's rules read as the committed twin's, line for line: the
    /// rail's words wrap where the hand-wrapped ones did.
    #[test]
    fn retails_rules_read_as_they_did() {
        let g = generate(&tables(), &census(), "b").unwrap();
        assert!(
            g.expected.contains(
                "- The fixtures' shields — Power Word: Shield 17, Ice Barrier 11426, Blood Shield\n\
                 \x20 77535 — must be in the result or the generator fails, naming the id.\n\
                 - The census `tools/absorb-spells-census.csv` (`tools/census-absorb-spells.sh\n\
                 \x20 <log>...`: every"
            ),
            "{}",
            g.expected
        );
        assert!(g.content.contains(
            "//! fixture's shields (17, 11426, 77535) are proven present at generation\n"
        ));
        assert!(
            g.content
                .contains("        assert!(super::is_absorb_spell(77535));\n")
        );
    }

    /// Forever's run is held to its own fixture's shield and names its own
    /// census; a Retail shield it lacks does not fail it, its own does.
    #[test]
    fn forevers_run_is_held_to_its_own_rail() {
        assert_eq!(rail("wowf").census, FOREVER.census);
        assert_eq!(rail("wow").census, RETAIL.census);
        let mut t = tables();
        t.get_mut("SpellEffect")
            .unwrap()
            .rows
            .retain(|r| r[4] != "77535");
        let g = generate_for(&t, &census(), "b", FOREVER).unwrap();
        assert!(g.content.contains("//! fixture's shields (17) are proven"));
        assert!(!g.content.contains("is_absorb_spell(77535)"));
        assert!(
            g.expected
                .contains("- The fixtures' shields — Power Word: Shield 17 — must")
        );
        assert!(
            g.expected
                .contains("`tools/forever/absorb-spells-census.csv`")
        );
        t.get_mut("SpellEffect")
            .unwrap()
            .rows
            .retain(|r| r[4] != "17");
        let err = generate_for(&t, &census(), "b", FOREVER).unwrap_err();
        assert!(
            err.contains("17") && err.contains("Power Word: Shield"),
            "{err}"
        );
    }

    #[test]
    fn deterministic_and_empty_census_prints_none() {
        let a = generate(&tables(), &census(), "b").unwrap();
        let b = generate(&tables(), &census(), "b").unwrap();
        assert_eq!(a.content, b.content);
        assert_eq!(a.expected, b.expected);
        let empty = Census::parse("id,name,log 1\n").unwrap();
        let g = generate(&tables(), &empty, "b").unwrap();
        assert!(g.unknown.is_empty());
        assert!(
            g.expected
                .contains("## Census ids outside the table\n\nNone.\n")
        );
        assert!(g.expected.contains("| 17 | Power Word: Shield | 0 |"));
    }
}
