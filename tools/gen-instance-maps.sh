#!/usr/bin/env bash
# Regenerate crates/core/src/instance_maps.rs from the LOCAL game install.
#
# Map.db2 comes straight out of the install's own CASC storage via
# `wowdps-extract gen-instance-maps` (emission rules in
# tools/extract/src/mapgen.rs): every map whose InstanceType is not 0, with
# its name, so R10 can open the visit of a pull on one that no door named
# (Forever logs a walk-in door onto the open world's map at difficulty 0)
# under the map's own name. The network is only used for the WoWDBDefs
# schema and the wowdev TACTKeys list, fetched fresh each run — this runs
# once per game patch (a new expansion brings its instances). Output is
# deterministic: same build in, same bytes out.
#
# usage: tools/gen-instance-maps.sh [wow-dir] [--product ROW] [-o FILE]
#   wow-dir: folder holding .build.info and Data/. When omitted the tool
#   locates the install itself ($WOWDPS_WOW_DIR, the wowdps config's
#   logs_dir, or a scan of the install roots (Steam compatdata on
#   Linux, /Applications and ~/Applications on macOS).
#   --product ROW: another edition of the game (tools/lib.sh). Forever's
#   (wowf) table goes to its own committed file,
#   crates/core/src/forever/instance_maps.rs; any other edition's only where
#   -o FILE names, never over another edition's committed one.
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
. "$root/tools/lib.sh"

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

build_extract
fetch_dbds gen-instance-maps "$work"
fetch_keys "$work/tactkeys.txt"

# The words given come last, so a -o among them wins; without one the
# table goes to its edition's committed file in this checkout.
"$extract" gen-instance-maps \
    --dbd-dir "$work" --keys "$work/tactkeys.txt" \
    --checkout "$root" "$@"
