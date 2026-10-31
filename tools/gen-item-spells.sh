#!/usr/bin/env bash
# Regenerate crates/core/src/item_spells.rs from the LOCAL game install.
#
# Twin of gen-class-spells.sh, for CONTRACT.md R12: which spells come from a
# trinket and which from a consumable, so the comparison timeline can mark
# trinket uses, trinket procs and pots — and, R26, WHICH trinket (id and
# ItemSparse name), so the ability tree nests a proc under its item. Tables
# come out of the install's own CASC storage via `wowdps-extract
# gen-item-spells` (join rules live in tools/extract/src/itemgen.rs).
# Network is only used for the WoWDBDefs schemas and the wowdev TACTKeys
# list, fetched fresh each run — this runs once per game patch. Output is
# deterministic: same build in, same bytes out.
#
# Note SpellEffect is a large table (~30 MB compressed in CASC); this takes
# noticeably longer than the class-spell generator.
#
# usage: tools/gen-item-spells.sh [wow-dir]
#   wow-dir: folder holding .build.info and Data/. When omitted the tool
#   locates the install itself ($WOWDPS_WOW_DIR, the wowdps config's
#   logs_dir, or a scan of the install roots (Steam compatdata on
#   Linux, /Applications and ~/Applications on macOS).
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
. "$root/tools/lib.sh"
wow="${1:-}"

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

build_extract
fetch_dbds gen-item-spells "$work"
fetch_keys "$work/tactkeys.txt"

"$extract" gen-item-spells ${wow:+"$wow"} \
    --dbd-dir "$work" --keys "$work/tactkeys.txt" \
    -o "$root/crates/core/src/item_spells.rs"
