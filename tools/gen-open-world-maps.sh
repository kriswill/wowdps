#!/usr/bin/env bash
# Regenerate crates/core/src/open_world_maps.rs from the LOCAL game install.
#
# Map.db2 comes straight out of the install's own CASC storage via
# `wowdps-extract gen-open-world-maps` (emission rules in
# tools/extract/src/mapgen.rs): every map whose InstanceType is 0, the open
# world, so R10 can read a door onto one as zoned out whatever difficulty
# the game stamped on it. Network is only used for the WoWDBDefs schema and
# the wowdev TACTKeys list, fetched fresh each run — this runs once per game
# patch (a new expansion brings its continents). Output is deterministic:
# same build in, same bytes out.
#
# usage: tools/gen-open-world-maps.sh [wow-dir] [--product ROW -o FILE]
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
fetch_dbds gen-open-world-maps "$work"
fetch_keys "$work/tactkeys.txt"

# The words given come last, so a -o among them wins.
"$extract" gen-open-world-maps \
    --dbd-dir "$work" --keys "$work/tactkeys.txt" \
    -o "$root/crates/core/src/open_world_maps.rs" "$@"
