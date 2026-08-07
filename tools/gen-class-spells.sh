#!/usr/bin/env bash
# Regenerate crates/core/src/class_spells.rs from the LOCAL game install.
#
# Replaces the retired gen-class-spells.py, which downloaded wago.tools CSV
# exports; the tables now come straight out of the install's own CASC
# storage via `wowdps-extract gen-class-spells` (attribution rules per
# CONTRACT.md R8 live in tools/extract/src/classgen.rs). Network is only
# used for the WoWDBDefs schemas and the wowdev TACTKeys list, fetched
# fresh each run — this runs once per game patch. Output is deterministic:
# same build in, same bytes out.
#
# usage: tools/gen-class-spells.sh [wow-dir]
#   wow-dir: folder holding .build.info and Data/ (defaults to the Proton
#   path matching DEFAULT_LOGS_DIR in crates/core/src/cli.rs)
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
wow="${1:-/home/k/.local/share/Steam/steamapps/compatdata/3082075026/pfx/drive_c/Program Files (x86)/World of Warcraft}"
[ -f "$wow/.build.info" ] || { echo "$wow: no .build.info (pass the World of Warcraft dir)" >&2; exit 1; }

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

# Table list must match classgen::TABLES (the tool errors on a missing dbd).
for t in SkillLine SkillLineAbility SpecializationSpells TraitDefinition \
         TraitNodeEntry TraitNodeXTraitNodeEntry TraitNode TraitTreeLoadout; do
    curl -sfL "https://raw.githubusercontent.com/wowdev/WoWDBDefs/master/definitions/$t.dbd" \
        -o "$work/$t.dbd" || { echo "failed to fetch $t.dbd" >&2; exit 1; }
done
curl -sfL "https://raw.githubusercontent.com/wowdev/TACTKeys/master/WoW.txt" \
    -o "$work/tactkeys.txt" || { echo "failed to fetch TACTKeys" >&2; exit 1; }

cargo build -q --release --manifest-path "$root/Cargo.toml" -p wowdps-extract
"$root/target/release/wowdps-extract" gen-class-spells "$wow" \
    --dbd-dir "$work" --keys "$work/tactkeys.txt" \
    -o "$root/crates/core/src/class_spells.rs"
