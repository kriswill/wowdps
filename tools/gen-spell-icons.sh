#!/usr/bin/env bash
# Regenerate the spell-icon cache from the LOCAL game install.
#
# Every spell id's icon (SpellMisc.SpellIconFileDataID), BLP-decoded to 32x32
# RGBA and deduplicated into $XDG_DATA_HOME/wowdps/spell-icons.bin (~60 MiB) —
# the GUI reads it lazily to draw ability icons next to spell rows, and draws
# none when the file is absent. Unlike the other gen-* outputs this is a
# PER-MACHINE CACHE, never committed. Network is only used for the WoWDBDefs
# schema and the wowdev TACTKeys list — this runs once per game patch, and
# takes a few minutes (~14k icon files).
#
# usage: tools/gen-spell-icons.sh [wow-dir]
#   wow-dir: folder holding .build.info and Data/. When omitted the tool
#   locates the install itself ($WOWDPS_WOW_DIR, the wowdps config's
#   logs_dir, or a scan of Steam compatdata prefixes).
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
wow="${1:-}"

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

# Table list must match spellicongen::TABLE (the tool errors on a missing dbd).
curl -sfL "https://raw.githubusercontent.com/wowdev/WoWDBDefs/master/definitions/SpellMisc.dbd" \
    -o "$work/SpellMisc.dbd" || { echo "failed to fetch SpellMisc.dbd" >&2; exit 1; }
curl -sfL "https://raw.githubusercontent.com/wowdev/TACTKeys/master/WoW.txt" \
    -o "$work/tactkeys.txt" || { echo "failed to fetch TACTKeys" >&2; exit 1; }

cargo build -q --release --manifest-path "$root/Cargo.toml" -p wowdps-extract
"$root/target/release/wowdps-extract" gen-spell-icons ${wow:+"$wow"} \
    --dbd-dir "$work" --keys "$work/tactkeys.txt"
