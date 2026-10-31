#!/usr/bin/env bash
# Census of the placed spells in real combat logs: the evidence behind the
# curated table of what players place in the room (totems, gateways, rings,
# zones; tools/extract/src/placegen.rs, crates/proto/src/placed.rs).
#
# For every log given, counts the lines of each spell the table names
# (`wowdps-extract placed-spells-ids`: the placing spells and the spells
# that tell where they stand), keyed by (event, spell id, name as the log
# wrote it): `cast`, a player's SPELL_CAST_SUCCESS; `summon`, a player's
# SPELL_SUMMON or SPELL_CREATE; `aura`, a SPELL_AURA_APPLIED on anyone;
# `heal`, a SPELL_HEAL or SPELL_PERIODIC_HEAL. One pass of grep + awk per log
# (the logs are large; never cat them). Output is a CSV, one row per
# (event, id, name), one count column per log named by the log's basename,
# sorted by id: the generator reads it via `wowdps-extract gen-placed-spells
# --census` and requires each entry's spells in it, writing the counts into
# crates/proto/src/placed/placed_spells.expected.md. The committed copy is
# tools/placed-spells-census.csv.
#
# usage: tools/census-placed-spells.sh [-o out.csv] <WoWCombatLog-*.txt>...
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
out=/dev/stdout
if [ "${1:-}" = "-o" ]; then
    out="$2"
    shift 2
fi
[ $# -ge 1 ] || { echo "usage: $0 [-o out.csv] <log>..." >&2; exit 2; }

. "$root/tools/lib.sh"
build_extract
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
"$extract" placed-spells-ids > "$work/ids"

header='event,id,name'
i=0
for log in "$@"; do
    i=$((i + 1))
    header="$header,$(basename "$log")"
    # $1 is "<ts>  EVENT", $2 the source guid, $10/$11 the spell id and
    # quoted name (the log is CRLF, so the CR comes off first).
    LC_ALL=C grep -E '  (SPELL_CAST_SUCCESS|SPELL_SUMMON|SPELL_CREATE|SPELL_AURA_APPLIED|SPELL_HEAL|SPELL_PERIODIC_HEAL),' "$log" \
        | LC_ALL=C awk -F, -v ids="$work/ids" '
            BEGIN { while ((getline id < ids) > 0) want[id] = 1 }
            { sub(/\r$/, "") }
            !($10 in want) { next }
            { split($1, h, "  "); ev = h[2]; name = $11; gsub(/"/, "", name) }
            ev == "SPELL_CAST_SUCCESS" && $2 ~ /^Player-/ { c["cast\t" $10 "\t" name]++ }
            (ev == "SPELL_SUMMON" || ev == "SPELL_CREATE") && $2 ~ /^Player-/ { c["summon\t" $10 "\t" name]++ }
            ev == "SPELL_AURA_APPLIED" { c["aura\t" $10 "\t" name]++ }
            ev == "SPELL_HEAL" || ev == "SPELL_PERIODIC_HEAL" { c["heal\t" $10 "\t" name]++ }
            END { for (k in c) print k "\t" c[k] }' > "$work/$i.tsv"
done

# Every (event, id, name) seen in any log, its count per log (0 where a log
# lacks it), sorted by id then event.
cat "$work"/*.tsv | cut -f1-3 | sort -u | sort -t$'\t' -k2,2n -k1,1 > "$work/keys"
{
    echo "$header"
    while IFS=$'\t' read -r ev id name; do
        row="$ev,$id,\"$name\""
        for j in $(seq 1 "$i"); do
            n=$(awk -F'\t' -v e="$ev" -v d="$id" -v m="$name" '$1 == e && $2 == d && $3 == m { print $4 }' "$work/$j.tsv")
            row="$row,${n:-0}"
        done
        echo "$row"
    done < "$work/keys"
} > "$out"
