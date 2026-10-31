#!/usr/bin/env bash
# Regenerate the class/spec icon cache from the LOCAL game install.
#
# The game's own class crests (interface/icons/classicon_*.blp) and spec
# icons (ChrSpecialization.SpellIconFileID), BLP-decoded, cropped inside the
# bevel the game frames them with, circle-masked and written to
# $XDG_DATA_HOME/wowdps/class-icons.bin — a PER-MACHINE cache the GUI reads
# at runtime (rules in icongen.rs, BLP in blp.rs).
#
# The game's icons are 64 px, which blurs on a large disc (the replay's
# players, a zoomed window), so each is upscaled 4x by Real-ESRGAN's
# illustration model (realesrgan-ncnn-vulkan, MIT, on the GPU through
# Vulkan; from $PATH, else built from nixpkgs), as the replay's
# gen-portraits does the journal's portraits: 208 px tiles, which the GUI halves to the
# size each disc is drawn at. Without the upscaler (or with --no-upscale)
# the tiles are the game's own pixels, 52 px.
#
# Network is only used for the WoWDBDefs schema, the wowdev TACTKeys list
# (both fetched fresh each run — this runs once per game patch) and, when it
# is not on PATH, the upscaler from the nix binary cache. Output is
# deterministic: same build (and upscaler) in, same bytes out.
#
# Like spell-icons.bin this is extracted Blizzard artwork and lives OUTSIDE
# the repository on purpose; a machine without it falls back to drawn discs.
#
# usage: tools/gen-icons.sh [wow-dir] [--no-upscale]
#   wow-dir: folder holding .build.info and Data/. When omitted the tool
#   locates the install itself ($WOWDPS_WOW_DIR, the wowdps config's
#   logs_dir, or a scan of the install roots (Steam compatdata on
#   Linux, /Applications and ~/Applications on macOS).
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
. "$root/tools/lib.sh"
upscale=1
wow=""
for a in "$@"; do
    case "$a" in
        --no-upscale) upscale=0 ;;
        *) wow=$a ;;
    esac
done

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

build_extract
fetch_dbds gen-icons "$work"
fetch_keys "$work/tactkeys.txt"

gen=("$extract" gen-icons ${wow:+"$wow"} --dbd-dir "$work" --keys "$work/tactkeys.txt")
esrgan=""
[ "$upscale" = 1 ] && esrgan=$(find_upscaler)
if [ -n "$esrgan" ]; then
    # Every icon whole at 64 px, upscaled beside them, then the cache cut
    # from the upscales.
    "${gen[@]}" --whole "$work/whole" --whole-only >"$work/whole.log" 2>&1 ||
        { cat "$work/whole.log" >&2; exit 1; }
    mkdir -p "$work/x4"
    if upscale_4x "$esrgan" "$work/whole" "$work/x4" >"$work/esrgan.log" 2>&1; then
        "${gen[@]}" --upscaled "$work/x4"
        exit 0
    fi
    echo "the upscaler failed; keeping the game's own pixels:" >&2
    tail -5 "$work/esrgan.log" >&2
elif [ "$upscale" = 1 ]; then
    echo "no realesrgan-ncnn-vulkan; keeping the game's own pixels" >&2
fi
"${gen[@]}"
