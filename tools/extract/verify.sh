#!/usr/bin/env bash
# Parity gate for wowdps-extract (dev-time, needs network).
#
# For each table: obtain the raw client .db2 — downloaded from wago.tools'
# CASC mirror, or with --game extracted from the local install's own CASC
# storage (`wowdps-extract fetch`, keys from wowdev/TACTKeys) — plus the
# matching WoWDBDefs schema and wago's own CSV export of the same build;
# extract and compare with `wowdps-extract diffcsv` (exact cells, floats by
# f32 bits — wago re-formats floats through PHP, see main.rs). A mismatch
# means our decoding diverged from the reference pipeline: fail loudly.
#
# usage: tools/extract/verify.sh [build]      # default: latest live build
#        tools/extract/verify.sh --game <dir> # dir holds .build.info + Data;
#                                             # build comes from .build.info
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

game="" build=""
while [ $# -gt 0 ]; do
    case "$1" in
        --game) game="$2"; shift 2 ;;
        *) build="$1"; shift ;;
    esac
done
if [ -n "$game" ] && [ -z "$build" ]; then
    # The wago CSVs must match the installed build exactly.
    build=$(awk -F'|' 'NR==1 { for (i=1; i<=NF; i++) { split($i,a,"!");
                if (a[1]=="Version") v=i; if (a[1]=="Product") p=i } }
            NR>1 && $p=="wow" { print $v; exit }' "$game/.build.info")
    [ -n "$build" ] || { echo "no wow row in $game/.build.info" >&2; exit 1; }
elif [ -z "$build" ]; then
    build=$(curl -sfL -A "$ua" https://wago.tools/api/builds \
        | grep -o '"wow":\[{[^}]*}' | grep -o '"version":"[^"]*"' \
        | head -1 | cut -d'"' -f4)
    [ -n "$build" ] || { echo "could not determine latest build" >&2; exit 1; }
fi
echo "build $build${game:+ (local install)}"

cache="${WOWDPS_DB2_CACHE:-/tmp/wowdps-db2-verify}/$build"
mkdir -p "$cache"

cargo build -q --release --manifest-path "$root/Cargo.toml" -p wowdps-extract
bin="$root/target/release/wowdps-extract"

fetch() { # fetch <url> <dest>: cache hit or download-then-rename
    [ -s "$2" ] && return 0
    curl -sfL -A "$ua" "$1" -o "$2.part" && mv "$2.part" "$2"
}

if [ -n "$game" ]; then
    fetch "https://raw.githubusercontent.com/wowdev/TACTKeys/master/WoW.txt" "$cache/tactkeys.txt" \
        || { echo "could not fetch TACTKeys" >&2; exit 1; }
fi

fail=0
for entry in "${tables[@]}"; do
    name="${entry%%:*}" fdid="${entry##*:}"
    printf '%-26s ' "$name"
    if [ -n "$game" ]; then
        if ! "$bin" fetch "$game" --fdid "$fdid" --keys "$cache/tactkeys.txt" \
            -o "$cache/$name.db2" 2>/dev/null; then
            echo "LOCAL EXTRACTION FAILED"; fail=1; continue
        fi
    elif ! fetch "https://wago.tools/api/casc/$fdid?version=$build&download" "$cache/$name.db2"; then
        echo "DOWNLOAD FAILED"; fail=1; continue
    fi
    if ! fetch "https://raw.githubusercontent.com/wowdev/WoWDBDefs/master/definitions/$name.dbd" "$cache/$name.dbd" \
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
