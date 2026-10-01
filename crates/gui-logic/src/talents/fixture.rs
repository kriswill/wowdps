//! The viewer's test fixture, for every GUI's tests (`test-support`): a
//! synthetic dataset small enough to reason about, the strings and pastes
//! minted against it, and a sandbox that points the dataset and the simc
//! store at them for one thread, so no test reads or writes
//! `~/.local/share/wowdps`.

use std::path::PathBuf;

use wowdps_proto::json::Json;
use wowdps_proto::talents as codec;

/// The mcp codec's synthetic two-spec dataset, reduced and enriched:
/// node 1 (3 ranks, active → square) and choice node 2 (gated at 2
/// points) in the class half on currency 601 (cap 3); node 3 and the
/// tiered node 6 in the spec half on currency 602 (cap 2); 4 the subtree
/// selector; 5 a hero node in tree 77.
const DATASET: &str = r#"{
  "build": "12.1.0.69497",
  "trees": [{
    "treeId": 10, "classId": 8, "className": "Mage",
    "specs": [{"specId": 62, "name": "Arcane", "role": 2}],
    "currencies": [{"index": 0, "id": 601, "max": 3},
                   {"index": 1, "id": 602, "max": 2}],
    "subTrees": [{"id": 77, "name": "Sunfury", "specs": [62]}],
    "nodeOrder": [1, 2, 3, 4, 5, 6],
    "nodes": [
      {"id": 1, "type": "single", "posX": 0, "posY": 0, "maxRanks": 3,
       "next": [2], "costs": [{"currency": 601, "amount": 1}],
       "entries": [{"id": 101, "spellId": 1001, "name": "Filler", "maxRanks": 3,
                    "entryType": 1, "desc": "Deals damage.\n\nCurses: one.",
                    "cost": "2% mana", "range": "40 yd", "cast": "Instant",
                    "descRanks": ["Deals 1.", "Deals 2.", "Deals 3."]}]},
      {"id": 2, "type": "choice", "posX": 0, "posY": 600, "maxRanks": 1,
       "reqPoints": 2, "costs": [{"currency": 601, "amount": 1}],
       "entries": [{"id": 131, "spellId": 1031, "name": "Left", "maxRanks": 1,
                    "desc": "Goes left."},
                   {"id": 132, "spellId": 1032, "name": "Right", "maxRanks": 1,
                    "desc": "Goes right.", "cost": "1 rune"}]},
      {"id": 3, "type": "single", "posX": 3000, "posY": 0, "maxRanks": 1,
       "next": [6], "costs": [{"currency": 602, "amount": 1}],
       "entries": [{"id": 104, "spellId": 1004, "name": "Gated", "maxRanks": 1}]},
      {"id": 6, "type": "tiered", "posX": 3000, "posY": 600, "maxRanks": 2,
       "costs": [{"currency": 602, "amount": 1}],
       "entries": [{"id": 161, "spellId": 1061, "name": "Stage One", "maxRanks": 1,
                    "desc": "First."},
                   {"id": 162, "spellId": 1062, "name": "Stage Two", "maxRanks": 1,
                    "desc": "Second."}]},
      {"id": 4, "type": "subtree", "posX": 3600, "posY": 600, "maxRanks": 1,
       "entries": [{"id": 151, "subTreeId": 77, "name": "", "maxRanks": 1}]},
      {"id": 5, "type": "single", "posX": 6000, "posY": 0, "maxRanks": 1,
       "subTreeId": 77,
       "entries": [{"id": 106, "spellId": 1006, "name": "Hero", "maxRanks": 1}]}
    ]
  }]
}"#;

/// The fixture dataset, parsed once per process. An unparsable fixture is
/// an empty object, which every test then fails loudly on.
pub fn dataset() -> &'static Json {
    static DS: std::sync::OnceLock<Json> = std::sync::OnceLock::new();
    DS.get_or_init(|| wowdps_proto::json::parse(DATASET).unwrap_or(Json::Obj(Vec::new())))
}

/// Mint an import string against the fixture from selection objects in the
/// codec's own shape; "" when the fixture refuses them.
pub fn string_for(sels: &[&str]) -> String {
    let sels: Vec<Json> = sels
        .iter()
        .filter_map(|s| wowdps_proto::json::parse(s).ok())
        .collect();
    codec::encode(dataset(), 62, &sels)
        .ok()
        .and_then(|enc| enc.get("string").and_then(Json::as_str).map(str::to_string))
        .unwrap_or_default()
}

/// The fixture build: node 1 at one rank, "Right" on node 2, node 3,
/// hero tree 77 with its node 5.
pub fn full_string() -> String {
    string_for(&[
        r#"{"node_id": 1, "ranks": 1}"#,
        r#"{"node_id": 2, "choice_index": 1}"#,
        r#"{"node_id": 3}"#,
        r#"{"node_id": 4, "choice_index": 0}"#,
        r#"{"node_id": 5}"#,
    ])
}

/// A SimulationCraft export with the given talent strings (the first is
/// the active one) and, optionally, an inventory.
pub fn simc_paste(name: &str, server: Option<&str>, strings: &[&str], gear: bool) -> String {
    let mut p = format!("mage=\"{name}\"\nlevel=80\nspec=arcane\n");
    if let Some(s) = server {
        p.push_str(&format!("server={s}\n"));
    }
    if let Some(first) = strings.first() {
        p.push_str(&format!("\ntalents={first}\n"));
    }
    for (i, s) in strings.iter().enumerate().skip(1) {
        p.push_str(&format!("\n# Saved Loadout: Build {i}\n# talents={s}\n"));
    }
    p.push_str("\n# WoW 12.1.0.69497, TOC 120100\n");
    if gear {
        p.push_str(
            "\n# Abyssal Hood (639)\nhead=,id=212095,gem_id=1/2,enchant_id=7328\n\
             neck=,id=252009\n\n# upgrade_currencies=c:2915:406/i:210221:5\n\
             # catalyst_currencies=c:3116:4\n\n### Gear from Bags\n#\n\
             # feet=,id=221507\n",
        );
    }
    p
}

/// One test's sandbox: the fixture dataset installed on this thread and a
/// scratch store directory, both undone when the guard drops.
pub struct Sandbox {
    pub dir: PathBuf,
}

impl Sandbox {
    /// `tag` names the scratch directory; make it unique per test.
    pub fn new(tag: &str) -> Self {
        let dir = std::env::temp_dir().join(format!(
            "wowdps-talents-sandbox-{}-{tag}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        super::use_dataset_on_this_thread(Some(dataset()));
        crate::simc::use_dir_on_this_thread(Some(dir.clone()));
        Self { dir }
    }

    /// What the store holds for `player`.
    pub fn stored(&self, player: &str) -> Option<String> {
        crate::simc::load_stored(player)
    }
}

impl Drop for Sandbox {
    fn drop(&mut self) {
        super::use_dataset_on_this_thread(None);
        crate::simc::use_dir_on_this_thread(None);
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}
