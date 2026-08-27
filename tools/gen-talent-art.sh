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
wow="${1:-}"

work=$(mktemp -d); trap 'rm -rf "$work"' EXIT

# Table list must match artgen::TABLES (the tool errors on a missing dbd).
for t in UiTextureAtlas UiTextureAtlasMember UiTextureAtlasElement \
         TraitSubTree ChrSpecialization; do
    curl -sfL "https://raw.githubusercontent.com/wowdev/WoWDBDefs/master/definitions/$t.dbd" \
        -o "$work/$t.dbd" || { echo "failed to fetch $t.dbd" >&2; exit 1; }
done
curl -sfL "https://raw.githubusercontent.com/wowdev/TACTKeys/master/WoW.txt" \
    -o "$work/tactkeys.txt" || { echo "failed to fetch TACTKeys" >&2; exit 1; }

cargo build -q --release --manifest-path "$root/Cargo.toml" -p wowdps-extract
"$root/target/release/wowdps-extract" gen-talent-art ${wow:+"$wow"} \
    --dbd-dir "$work" --keys "$work/tactkeys.txt"
