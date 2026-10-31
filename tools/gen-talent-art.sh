#!/usr/bin/env bash
# Regenerate ~/.local/share/wowdps/talent-art.bin: the talent UI's own
# artwork cropped from the client's texture atlases — per-spec pane
# background paintings, each hero tree's round medallion, and the golden
# medallion ring. A per-machine cache like the icon bins (extracted
# Blizzard art never lands in the repo); the GUI's talent viewer renders
# fine without it. Network is used only for the WoWDBDefs schemas and
# TACT keys; the textures come from the local install's CASC storage.
#
#   tools/gen-talent-art.sh [wow-dir]
set -euo pipefail
root="$(cd "$(dirname "$0")/.." && pwd)"
. "$root/tools/lib.sh"
wow="${1:-}"

work=$(mktemp -d); trap 'rm -rf "$work"' EXIT

build_extract
fetch_dbds gen-talent-art "$work"
fetch_keys "$work/tactkeys.txt"

"$extract" gen-talent-art ${wow:+"$wow"} \
    --dbd-dir "$work" --keys "$work/tactkeys.txt"
