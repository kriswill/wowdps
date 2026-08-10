#!/usr/bin/env bash
# Regenerate crates/core/src/item_spells.rs from the LOCAL game install.
#
# Twin of gen-class-spells.sh, for CONTRACT.md R12: which spells come from a
# trinket and which from a consumable, so the comparison timeline can mark
# trinket uses, trinket procs and pots. Tables come out of the install's own
# CASC storage via `wowdps-extract gen-item-spells` (join rules live in
# tools/extract/src/itemgen.rs). Network is only used for the WoWDBDefs
# schemas and the wowdev TACTKeys list, fetched fresh each run — this runs
# once per game patch. Output is deterministic: same build in, same bytes out.
#
# Note SpellEffect is a large table (~30 MB compressed in CASC); this takes
# noticeably longer than the class-spell generator.
#
# usage: tools/gen-item-spells.sh [wow-dir]
#   wow-dir: folder holding .build.info and Data/. When omitted the tool
#   locates the install itself ($WOWDPS_WOW_DIR, the wowdps config's
#   logs_dir, or a scan of Steam compatdata prefixes).
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
wow="${1:-}"

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

# Table list must match itemgen::TABLES (the tool errors on a missing dbd).
for t in Item ItemEffect ItemXItemEffect SpellEffect; do
    curl -sfL "https://raw.githubusercontent.com/wowdev/WoWDBDefs/master/definitions/$t.dbd" \
        -o "$work/$t.dbd" || { echo "failed to fetch $t.dbd" >&2; exit 1; }
done
curl -sfL "https://raw.githubusercontent.com/wowdev/TACTKeys/master/WoW.txt" \
    -o "$work/tactkeys.txt" || { echo "failed to fetch TACTKeys" >&2; exit 1; }

cargo build -q --release --manifest-path "$root/Cargo.toml" -p wowdps-extract
"$root/target/release/wowdps-extract" gen-item-spells ${wow:+"$wow"} \
    --dbd-dir "$work" --keys "$work/tactkeys.txt" \
    -o "$root/crates/core/src/item_spells.rs"
