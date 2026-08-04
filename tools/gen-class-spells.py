#!/usr/bin/env python3
"""Regenerate crates/core/src/class_spells.rs from wago.tools DB2 exports.

Dev-time only — the output is committed so builds never touch the network.
Run once per game patch:

    python3 tools/gen-class-spells.py            # latest live build
    python3 tools/gen-class-spells.py --build 12.1.0.68914
    python3 tools/gen-class-spells.py --cache /tmp/db2   # reuse downloads

Attribution sources (see CONTRACT.md R8):
  * SkillLineAbility rows on the 13 class skill lines  -> (class, no spec)
  * SpecializationSpells                               -> (class, spec)
  * talent spells via the trait chain TraitDefinition -> TraitNodeEntry ->
    TraitNodeXTraitNodeEntry -> TraitNode -> TraitTreeLoadout(spec)

A spell attributed to more than one class is dropped (not class evidence).
A spell keeps a spec only when spec sources name exactly one spec AND no
class-wide source (class skill line) also grants it.
"""

import argparse
import csv
import datetime
import io
import json
import os
import sys
import urllib.request

WAGO = "https://wago.tools"

# Class skill lines are matched by DisplayName so a renumbering patch fails
# loudly here instead of silently shrinking the table. CategoryID 7 also holds
# pet skill lines ("Pet - ..."), which must stay excluded: pet spells are not
# player class evidence.
CLASS_SKILL_NAMES = {
    "Warrior": "Warrior",
    "Paladin": "Paladin",
    "Hunter": "Hunter",
    "Rogue": "Rogue",
    "Priest": "Priest",
    "Death Knight": "DeathKnight",
    "Shaman": "Shaman",
    "Mage": "Mage",
    "Warlock": "Warlock",
    "Monk": "Monk",
    "Druid": "Druid",
    "Demon Hunter": "DemonHunter",
    "Evoker": "Evoker",
}

# Mirrors wowdps_model::Spec::from_id / Spec::class.
SPEC_CLASS = {
    71: "Warrior", 72: "Warrior", 73: "Warrior",
    65: "Paladin", 66: "Paladin", 70: "Paladin",
    253: "Hunter", 254: "Hunter", 255: "Hunter",
    259: "Rogue", 260: "Rogue", 261: "Rogue",
    256: "Priest", 257: "Priest", 258: "Priest",
    250: "DeathKnight", 251: "DeathKnight", 252: "DeathKnight",
    262: "Shaman", 263: "Shaman", 264: "Shaman",
    62: "Mage", 63: "Mage", 64: "Mage",
    265: "Warlock", 266: "Warlock", 267: "Warlock",
    268: "Monk", 269: "Monk", 270: "Monk",
    102: "Druid", 103: "Druid", 104: "Druid", 105: "Druid",
    577: "DemonHunter", 581: "DemonHunter",
    1467: "Evoker", 1468: "Evoker", 1473: "Evoker",
}

# Class code emitted into the table; must match the CLASSES array in the
# generated file (which spells the variants out, so order here is arbitrary
# but fixed).
CLASS_CODE = {
    name: i
    for i, name in enumerate(
        [
            "Warrior", "Paladin", "Hunter", "Rogue", "Priest", "DeathKnight",
            "Shaman", "Mage", "Warlock", "Monk", "Druid", "DemonHunter",
            "Evoker",
        ]
    )
}

TABLES = [
    "SkillLine",
    "SkillLineAbility",
    "SpecializationSpells",
    "TraitDefinition",
    "TraitNodeEntry",
    "TraitNodeXTraitNodeEntry",
    "TraitNode",
    "TraitTreeLoadout",
]


def fetch(url):
    req = urllib.request.Request(url, headers={"User-Agent": "wowdps-gen-class-spells"})
    with urllib.request.urlopen(req, timeout=120) as r:
        return r.read()


def latest_build():
    builds = json.loads(fetch(f"{WAGO}/api/builds"))
    for product in ("wow", "wow_classic_era", "wowt"):
        if builds.get(product):
            return builds[product][0]["version"]
    sys.exit("no build found in wago.tools /api/builds")


def load_table(name, build, cache_dir):
    path = os.path.join(cache_dir, f"{name}-{build}.csv") if cache_dir else None
    if path and os.path.exists(path):
        data = open(path, "rb").read()
    else:
        data = fetch(f"{WAGO}/db2/{name}/csv?build={build}")
        if path:
            os.makedirs(cache_dir, exist_ok=True)
            with open(path, "wb") as f:
                f.write(data)
    rows = list(csv.DictReader(io.StringIO(data.decode("utf-8"))))
    if not rows:
        sys.exit(f"{name}: empty table for build {build}")
    return rows


def build_map(build, cache_dir):
    t = {name: load_table(name, build, cache_dir) for name in TABLES}

    class_lines = {}  # SkillLine id -> class name
    for r in t["SkillLine"]:
        if r["CategoryID"] == "7" and r["DisplayName_lang"] in CLASS_SKILL_NAMES:
            class_lines[r["ID"]] = CLASS_SKILL_NAMES[r["DisplayName_lang"]]
    if len(class_lines) != 13:
        sys.exit(f"expected 13 class skill lines, found {len(class_lines)}: {class_lines}")

    # spell id -> set of classes / set of specs / classwide flag
    classes, specs, classwide = {}, {}, set()

    def attribute(spell, cls, spec=None, wide=False):
        classes.setdefault(spell, set()).add(cls)
        if spec is not None:
            specs.setdefault(spell, set()).add(spec)
        if wide:
            classwide.add(spell)

    for r in t["SkillLineAbility"]:
        cls = class_lines.get(r["SkillLine"])
        if cls:
            attribute(int(r["Spell"]), cls, wide=True)

    for r in t["SpecializationSpells"]:
        spec = int(r["SpecID"])
        if spec in SPEC_CLASS:
            attribute(int(r["SpellID"]), SPEC_CLASS[spec], spec)

    # Trait chain: definition -> entries -> nodes -> trees -> specs.
    def_spell = {r["ID"]: int(r["SpellID"]) for r in t["TraitDefinition"] if r["SpellID"] != "0"}
    entry_def = {r["ID"]: r["TraitDefinitionID"] for r in t["TraitNodeEntry"]}
    node_entries = {}
    for r in t["TraitNodeXTraitNodeEntry"]:
        node_entries.setdefault(r["TraitNodeID"], []).append(r["TraitNodeEntryID"])
    node_tree = {r["ID"]: r["TraitTreeID"] for r in t["TraitNode"]}
    tree_specs = {}
    for r in t["TraitTreeLoadout"]:
        spec = int(r["ChrSpecializationID"])
        if spec in SPEC_CLASS:
            tree_specs.setdefault(r["TraitTreeID"], set()).add(spec)

    for node, entries in node_entries.items():
        for spec in tree_specs.get(node_tree.get(node), ()):
            for entry in entries:
                spell = def_spell.get(entry_def.get(entry))
                if spell:
                    attribute(spell, SPEC_CLASS[spec], spec)

    table = []
    ambiguous = 0
    for spell in sorted(classes):
        cs = classes[spell]
        if len(cs) != 1:
            ambiguous += 1
            continue
        ss = specs.get(spell, set())
        spec = ss.pop() if len(ss) == 1 and spell not in classwide else 0
        table.append((spell, CLASS_CODE[next(iter(cs))], spec))
    return table, ambiguous


def emit(table, ambiguous, build, out_path):
    today = datetime.date.today().isoformat()
    speced = sum(1 for _, _, s in table if s)
    lines = [
        "//! GENERATED by tools/gen-class-spells.py — do not edit by hand.",
        f"//! Source: wago.tools DB2 exports, build {build}, generated {today}.",
        f"//! {len(table)} spells ({speced} spec-unique); {ambiguous} multi-class ids dropped.",
        "//!",
        "//! Maps a combat-log spell id to the only class that can cast it, and — when",
        "//! the spell is unique to one specialization — to that spec (CONTRACT.md R8).",
        "",
        "use wowdps_model::{Class, Spec};",
        "",
        "/// The class (and, when spec-unique, the spec) identified by a spell cast.",
        "pub(crate) fn resolve(spell_id: u32) -> Option<(Class, Option<Spec>)> {",
        "    let i = TABLE.binary_search_by_key(&spell_id, |e| e.0).ok()?;",
        "    let (_, class_code, spec_id) = TABLE[i];",
        "    Some((CLASSES[class_code as usize], Spec::from_id(spec_id as u32)))",
        "}",
        "",
        "const CLASSES: [Class; 13] = [",
        "    Class::Warrior,",
        "    Class::Paladin,",
        "    Class::Hunter,",
        "    Class::Rogue,",
        "    Class::Priest,",
        "    Class::DeathKnight,",
        "    Class::Shaman,",
        "    Class::Mage,",
        "    Class::Warlock,",
        "    Class::Monk,",
        "    Class::Druid,",
        "    Class::DemonHunter,",
        "    Class::Evoker,",
        "];",
        "",
        "/// (spell id, class code, spec id or 0), sorted by spell id.",
        "#[rustfmt::skip]",
        "static TABLE: &[(u32, u8, u16)] = &[",
    ]
    per_line = 6
    for i in range(0, len(table), per_line):
        chunk = table[i : i + per_line]
        lines.append("    " + " ".join(f"({s},{c},{p})," for s, c, p in chunk))
    lines += [
        "];",
        "",
        "#[cfg(test)]",
        "mod tests {",
        "    /// Strictly ascending: binary search demands it, and it doubles as",
        "    /// a dedup check.",
        "    #[test]",
        "    fn table_is_sorted_by_spell_id() {",
        "        assert!(super::TABLE.windows(2).all(|w| w[0].0 < w[1].0));",
        "    }",
        "}",
        "",
    ]
    with open(out_path, "w") as f:
        f.write("\n".join(lines))


def main():
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--build", help="game build version (default: latest live)")
    ap.add_argument("--cache", help="directory to cache downloaded CSVs")
    ap.add_argument(
        "--out",
        default=os.path.join(os.path.dirname(__file__), "..", "crates/core/src/class_spells.rs"),
    )
    args = ap.parse_args()
    build = args.build or latest_build()
    print(f"build {build}", file=sys.stderr)
    table, ambiguous = build_map(build, args.cache)
    emit(table, ambiguous, build, os.path.normpath(args.out))
    speced = sum(1 for _, _, s in table if s)
    print(f"{len(table)} spells ({speced} spec-unique), {ambiguous} ambiguous dropped", file=sys.stderr)


if __name__ == "__main__":
    main()
