#!/usr/bin/env bash
# Regenerate crates/core/src/forever/role_signals.rs (+ role_signals.expected.md)
# from the LOCAL game install's Forever client.
#
# For CONTRACT.md R30: which spells are a taunt, a tank's mitigation, a cast
# that needs a shield, a tanking form or stance, or a heal, so the meter can
# infer a role per fight in an edition whose log carries no spec. The
# membership is CURATED by name in tools/extract/src/signalgen.rs; the
# install expands each name to its ids and proves each one (SpellName,
# SpellClassOptions' class family, and the kind's own effect, aura, form or
# shield requirement), and the committed real-log census
# (tools/role-signals-census.csv, from tools/census-role-signals.sh) must
# show a player casting it, unless the entry carries a waiver. Network is
# only used for the WoWDBDefs schemas and the wowdev TACTKeys list, fetched
# fresh each run. Output is deterministic: same build + census in, same
# bytes out. Retail has no such table (its role is its spec's), so a run
# reads Forever's product unless it names another with -o.
#
# usage: tools/gen-role-signals.sh [wow-dir] [-o FILE]
#   wow-dir: folder holding .build.info and Data/. When omitted the tool
#   locates the install itself ($WOWDPS_WOW_DIR, the wowdps config's
#   logs_dir, or a scan of the install roots).
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
. "$root/tools/lib.sh"

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

build_extract
fetch_dbds gen-role-signals "$work"
fetch_keys "$work/tactkeys.txt"

# The words given come last, so a -o or a --product among them wins.
"$extract" gen-role-signals \
    --dbd-dir "$work" --keys "$work/tactkeys.txt" \
    --census "$root/tools/role-signals-census.csv" \
    --checkout "$root" --product "${WOWDPS_PRODUCT:-wowf}" "$@"
