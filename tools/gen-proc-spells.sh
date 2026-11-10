#!/usr/bin/env bash
# Regenerate crates/core/src/proc_spells.rs (+ proc_spells.expected.md) from
# the LOCAL game install.
#
# Twin of gen-role-spells.sh, for CONTRACT.md R26's ability tree: which
# talent proc hangs under which driver (Blackened Soul under Wither,
# Expurgation under Blade of Justice). The membership is CURATED in
# tools/extract/src/procgen.rs; the install proves each entry (SpellName must
# carry both names, and the client's own text — the proc's description or one
# it references — must name the driver, reference its id, or have one of the
# driver's effects trigger the proc) and the committed real-log census
# (tools/proc-spells-census.csv, from tools/census-proc-spells.sh) must show
# the pair in play. Network is only used for the WoWDBDefs schemas and the
# wowdev TACTKeys list, fetched fresh each run — this runs once per game
# patch or when the curated list changes. Output is deterministic: same
# build + census in, same bytes out.
#
# Note SpellEffect and Spell are large tables; this takes a minute or two.
#
# usage: tools/gen-proc-spells.sh [wow-dir] [--product ROW -o FILE]
#   wow-dir: folder holding .build.info and Data/. When omitted the tool
#   locates the install itself ($WOWDPS_WOW_DIR, the wowdps config's
#   logs_dir, or a scan of the install roots (Steam compatdata on
#   Linux, /Applications and ~/Applications on macOS).
#   --product ROW: another edition of the game (tools/lib.sh); no edition
#   but Retail has this table yet, so its table and review twin go only
#   where -o FILE names, never over a committed one.
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
. "$root/tools/lib.sh"

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

build_extract
fetch_dbds gen-proc-spells "$work"
fetch_keys "$work/tactkeys.txt"

# The words given come last, so a -o among them wins; without one the
# table goes to its edition's committed file in this checkout.
"$extract" gen-proc-spells \
    --dbd-dir "$work" --keys "$work/tactkeys.txt" \
    --census "$root/tools/proc-spells-census.csv" \
    --checkout "$root" "$@"
