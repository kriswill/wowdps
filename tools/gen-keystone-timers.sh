#!/usr/bin/env bash
# Regenerate crates/core/src/keystone_timers.rs from the LOCAL game install.
#
# Replaces the retired gen-keystone-timers.py, which downloaded wago.tools
# CSV exports; MapChallengeMode.db2 now comes straight out of the install's
# own CASC storage via `wowdps-extract gen-keystone-timers` (emission rules
# in tools/extract/src/keystonegen.rs). Network is only used for the
# WoWDBDefs schema and the wowdev TACTKeys list, fetched fresh each run —
# this runs once per game patch (par times are retuned between seasons).
# Output is deterministic: same build in, same bytes out.
#
# usage: tools/gen-keystone-timers.sh [wow-dir]
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
fetch_dbds gen-keystone-timers "$work"
fetch_keys "$work/tactkeys.txt"

"$extract" gen-keystone-timers ${wow:+"$wow"} \
    --dbd-dir "$work" --keys "$work/tactkeys.txt" \
    -o "$root/crates/core/src/keystone_timers.rs"
