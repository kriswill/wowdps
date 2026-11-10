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
# usage: tools/gen-class-spells.sh [wow-dir] [--product ROW -o FILE]
#   wow-dir: folder holding .build.info and Data/. When omitted the tool
#   locates the install itself ($WOWDPS_WOW_DIR, the wowdps config's
#   logs_dir, or a scan of the install roots (Steam compatdata on
#   Linux, /Applications and ~/Applications on macOS).
#   --product ROW: another edition of the game (tools/lib.sh); its table
#   goes only where -o FILE names, never over the committed one.
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
. "$root/tools/lib.sh"

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

build_extract
fetch_dbds gen-class-spells "$work"
fetch_keys "$work/tactkeys.txt"

# The words given come last, so a -o among them wins.
"$extract" gen-class-spells \
    --dbd-dir "$work" --keys "$work/tactkeys.txt" \
    -o "$root/crates/core/src/class_spells.rs" "$@"
