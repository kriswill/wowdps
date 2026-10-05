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
# usage: tools/gen-open-world-maps.sh [wow-dir]
#   wow-dir: folder holding .build.info and Data/. When omitted the tool
#   locates the install itself ($WOWDPS_WOW_DIR, the wowdps config's
#   logs_dir, or a scan of Steam compatdata prefixes).
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
wow="${1:-}"

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

curl -sfL "https://raw.githubusercontent.com/wowdev/WoWDBDefs/master/definitions/Map.dbd" \
    -o "$work/Map.dbd" || { echo "failed to fetch Map.dbd" >&2; exit 1; }
curl -sfL "https://raw.githubusercontent.com/wowdev/TACTKeys/master/WoW.txt" \
    -o "$work/tactkeys.txt" || { echo "failed to fetch TACTKeys" >&2; exit 1; }

cargo build -q --release --manifest-path "$root/Cargo.toml" -p wowdps-extract
"$root/target/release/wowdps-extract" gen-open-world-maps ${wow:+"$wow"} \
    --dbd-dir "$work" --keys "$work/tactkeys.txt" \
    -o "$root/crates/core/src/open_world_maps.rs"
