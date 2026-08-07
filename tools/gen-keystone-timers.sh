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
#   wow-dir: folder holding .build.info and Data/ (defaults to the Proton
#   path matching DEFAULT_LOGS_DIR in crates/core/src/cli.rs)
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
wow="${1:-/home/k/.local/share/Steam/steamapps/compatdata/3082075026/pfx/drive_c/Program Files (x86)/World of Warcraft}"
[ -f "$wow/.build.info" ] || { echo "$wow: no .build.info (pass the World of Warcraft dir)" >&2; exit 1; }

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

curl -sfL "https://raw.githubusercontent.com/wowdev/WoWDBDefs/master/definitions/MapChallengeMode.dbd" \
    -o "$work/MapChallengeMode.dbd" || { echo "failed to fetch MapChallengeMode.dbd" >&2; exit 1; }
curl -sfL "https://raw.githubusercontent.com/wowdev/TACTKeys/master/WoW.txt" \
    -o "$work/tactkeys.txt" || { echo "failed to fetch TACTKeys" >&2; exit 1; }

cargo build -q --release --manifest-path "$root/Cargo.toml" -p wowdps-extract
"$root/target/release/wowdps-extract" gen-keystone-timers "$wow" \
    --dbd-dir "$work" --keys "$work/tactkeys.txt" \
    -o "$root/crates/core/src/keystone_timers.rs"
