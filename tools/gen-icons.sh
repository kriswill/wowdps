#!/usr/bin/env bash
# Regenerate the class/spec icon cache from the LOCAL game install.
#
# The game's own class crests (interface/icons/classicon_*.blp) and spec
# icons (ChrSpecialization.SpellIconFileID), BLP-decoded, downscaled to
# 32x32, circle-masked and written to $XDG_DATA_HOME/wowdps/class-icons.bin —
# a PER-MACHINE cache the GUI reads at runtime (rules in icongen.rs, BLP in
# blp.rs). Network is only used for the WoWDBDefs schema and the wowdev
# TACTKeys list, fetched fresh each run — this runs once per game patch.
# Output is deterministic: same build in, same bytes out.
#
# Like spell-icons.bin this is extracted Blizzard artwork and lives OUTSIDE
# the repository on purpose; a machine without it falls back to drawn discs.
#
# usage: tools/gen-icons.sh [wow-dir]
#   wow-dir: folder holding .build.info and Data/. When omitted the tool
#   locates the install itself ($WOWDPS_WOW_DIR, the wowdps config's
#   logs_dir, or a scan of Steam compatdata prefixes).
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
wow="${1:-}"

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

# Table list must match icongen::TABLE (the tool errors on a missing dbd).
curl -sfL "https://raw.githubusercontent.com/wowdev/WoWDBDefs/master/definitions/ChrSpecialization.dbd" \
    -o "$work/ChrSpecialization.dbd" || { echo "failed to fetch ChrSpecialization.dbd" >&2; exit 1; }
curl -sfL "https://raw.githubusercontent.com/wowdev/TACTKeys/master/WoW.txt" \
    -o "$work/tactkeys.txt" || { echo "failed to fetch TACTKeys" >&2; exit 1; }

cargo build -q --release --manifest-path "$root/Cargo.toml" -p wowdps-extract
"$root/target/release/wowdps-extract" gen-icons ${wow:+"$wow"} \
    --dbd-dir "$work" --keys "$work/tactkeys.txt"
