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
# usage: tools/gen-keystone-timers.sh [wow-dir] [--product ROW -o FILE]
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
fetch_dbds gen-keystone-timers "$work"
fetch_keys "$work/tactkeys.txt"

# The words given come last, so a -o among them wins.
"$extract" gen-keystone-timers \
    --dbd-dir "$work" --keys "$work/tactkeys.txt" \
    -o "$root/crates/core/src/keystone_timers.rs" "$@"
