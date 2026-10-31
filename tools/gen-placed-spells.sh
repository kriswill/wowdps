#!/usr/bin/env bash
# Regenerate crates/proto/src/placed/placed_spells.rs (+
# placed_spells.expected.md) from the LOCAL game install: what players
# place in the room that the replay draws (totems, gateways, rings, zones,
# statues, sigils, traps),
# how the log tells where each stands, its reach and how long it stands.
#
# The membership is CURATED in tools/extract/src/placegen.rs; the install
# proves each entry's names (SpellName) and gives its reach (the largest
# radius of a named spell's effects, SpellEffect + SpellRadius) and its
# duration (SpellMisc + SpellDuration), and the committed real-log census
# (tools/placed-spells-census.csv, from tools/census-placed-spells.sh) must
# show every spell an entry names in a log. Network is only used for the
# WoWDBDefs schemas and the wowdev TACTKeys list, fetched fresh each run —
# this runs once per game patch or when the curated list changes. Output is
# deterministic: same build + census in, same bytes out.
#
# Note SpellEffect is a large table (~30 MB compressed in CASC); this takes
# noticeably longer than the class-spell generator.
#
# usage: tools/gen-placed-spells.sh [wow-dir]
#   wow-dir: folder holding .build.info and Data/. When omitted the tool
#   locates the install itself ($WOWDPS_WOW_DIR, the wowdps config's
#   logs_dir, or a scan of Steam compatdata prefixes).
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
. "$root/tools/lib.sh"
wow="${1:-}"

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

build_extract
fetch_dbds gen-placed-spells "$work"
fetch_keys "$work/tactkeys.txt"

"$extract" gen-placed-spells ${wow:+"$wow"} \
    --dbd-dir "$work" --keys "$work/tactkeys.txt" \
    --census "$root/tools/placed-spells-census.csv" \
    -o "$root/crates/proto/src/placed/placed_spells.rs"
