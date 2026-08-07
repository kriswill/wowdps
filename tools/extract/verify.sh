#!/usr/bin/env bash
# Parity gate for wowdps-extract (dev-time, needs network).
#
# For each table: download the raw client .db2 from wago.tools' CASC mirror,
# the matching WoWDBDefs schema, and wago's own CSV export of the same build;
# extract locally and compare with `wowdps-extract diffcsv` (exact cells,
# floats by f32 bits — wago re-formats floats through PHP, see main.rs).
# A mismatch means our WDC5 decoding diverged from DBCD's: fail loudly.
#
# usage: tools/extract/verify.sh [build]     # default: latest live build
# cache: $WOWDPS_DB2_CACHE (default /tmp/wowdps-db2-verify), per build
set -euo pipefail

root="$(cd "$(dirname "$0")/../.." && pwd)"
ua="wowdps-extract-verify"

# The gen-class-spells.py input tables, plus GameObjects for float/string
# coverage. FileDataIDs from wowdev/wow-listfile (stable per file, forever).
tables=(
    SkillLine:1240935
    SkillLineAbility:1266278
    SpecializationSpells:1240335
    TraitDefinition:4420327
    TraitNodeEntry:4420298
    TraitNodeXTraitNodeEntry:4420304
    TraitNode:4420297
    TraitTreeLoadout:4669507
    GameObjects:841620
)

build="${1:-}"
if [ -z "$build" ]; then
    build=$(curl -sfL -A "$ua" https://wago.tools/api/builds \
        | grep -o '"wow":\[{[^}]*}' | grep -o '"version":"[^"]*"' \
        | head -1 | cut -d'"' -f4)
    [ -n "$build" ] || { echo "could not determine latest build" >&2; exit 1; }
fi
echo "build $build"

cache="${WOWDPS_DB2_CACHE:-/tmp/wowdps-db2-verify}/$build"
mkdir -p "$cache"

cargo build -q --manifest-path "$root/Cargo.toml" -p wowdps-extract
bin="$root/target/debug/wowdps-extract"

fetch() { # fetch <url> <dest>: cache hit or download-then-rename
    [ -s "$2" ] && return 0
    curl -sfL -A "$ua" "$1" -o "$2.part" && mv "$2.part" "$2"
}

fail=0
for entry in "${tables[@]}"; do
    name="${entry%%:*}" fdid="${entry##*:}"
    printf '%-26s ' "$name"
    if ! fetch "https://wago.tools/api/casc/$fdid?version=$build&download" "$cache/$name.db2" \
        || ! fetch "https://raw.githubusercontent.com/wowdev/WoWDBDefs/master/definitions/$name.dbd" "$cache/$name.dbd" \
        || ! fetch "https://wago.tools/db2/$name/csv?build=$build" "$cache/$name.wago.csv"; then
        echo "DOWNLOAD FAILED"; fail=1; continue
    fi
    if ! "$bin" csv "$cache/$name.db2" --dbd "$cache/$name.dbd" -o "$cache/$name.ours.csv"; then
        fail=1; continue
    fi
    if ! "$bin" diffcsv "$cache/$name.ours.csv" "$cache/$name.wago.csv"; then
        fail=1
    fi
done

exit $fail
