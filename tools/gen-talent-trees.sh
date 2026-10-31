#!/usr/bin/env bash
# Regenerate the talent-tree dataset from the LOCAL game install.
#
# Every class's full trait tree — nodes, edges, choice entries, hero
# subtrees, spec gating, point costs, spell names and icon names — joined
# out of the install's own CASC storage into
# $XDG_DATA_HOME/wowdps/talents.json, the file the MCP server's talent
# tools (talent_tree / decode_talents / encode_talents) and the wow-coach
# tree viewer read. Like the icon caches this is extracted Blizzard data
# and lives OUTSIDE the repository on purpose.
#
# Network is used for the WoWDBDefs schemas, the wowdev TACTKeys list, and
# (cached, it is ~140 MB) the wowdev community listfile that names icon
# files for the wowhead CDN — this runs once per game patch. Output is
# deterministic: same build in, same bytes out.
#
# usage: tools/gen-talent-trees.sh [wow-dir]
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

# The icons subset of the community listfile (fdid;interface/icons/...),
# cached across runs: icon names for existing fdids never change, so a
# week-old subset only ever misses brand-new icons.
cache="${WOWDPS_DB2_CACHE:-/tmp/wowdps-db2-verify}"
icons="$cache/icons-listfile.csv"
mkdir -p "$cache"
if ! [ -s "$icons" ] || [ -n "$(find "$icons" -mtime +7 2>/dev/null)" ]; then
    curl -sfL "https://github.com/wowdev/wow-listfile/releases/latest/download/community-listfile.csv" \
        | grep ';interface/icons/' > "$icons.part" \
        && mv "$icons.part" "$icons" \
        || { echo "failed to fetch community listfile" >&2; exit 1; }
fi

build_extract
fetch_dbds gen-talent-trees "$work"
fetch_keys "$work/tactkeys.txt"

"$extract" gen-talent-trees ${wow:+"$wow"} \
    --dbd-dir "$work" --keys "$work/tactkeys.txt" --listfile "$icons"
