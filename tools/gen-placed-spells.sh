#!/usr/bin/env bash
# Regenerate crates/proto/src/placed/placed_spells.rs (+
# placed_spells.expected.md) from the LOCAL game install, or with
# --product wowf Forever's forever_placed_spells.rs beside it: what players
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
# deterministic: same build + census in, same bytes out. Each edition's run
# reads its own list (Forever's by its own ids, each totem rank an entry) and
# its own committed census (Forever's tools/forever/placed-spells-census.csv),
# unless --census names another.
#
# Note SpellEffect is a large table (~30 MB compressed in CASC); this takes
# noticeably longer than the class-spell generator.
#
# usage: tools/gen-placed-spells.sh [wow-dir] [--product ROW [-o FILE]]
#   wow-dir: folder holding .build.info and Data/. When omitted the tool
#   locates the install itself ($WOWDPS_WOW_DIR, the wowdps config's
#   logs_dir, or a scan of Steam compatdata prefixes).
#   --product ROW: another edition of the game (tools/lib.sh): Forever's
#   (wowf) writes its committed table; one with none needs -o FILE.
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
. "$root/tools/lib.sh"

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

build_extract
fetch_dbds gen-placed-spells "$work"
fetch_keys "$work/tactkeys.txt"

# The words given come last, so a -o among them wins; without one the
# table goes to its edition's committed file in this checkout.
"$extract" gen-placed-spells \
    --dbd-dir "$work" --keys "$work/tactkeys.txt" \
    --checkout "$root" "$@"
