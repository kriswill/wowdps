#!/usr/bin/env bash
# Census of the spells players CAST in real combat logs — the evidence behind
# the curated role-signal table (CONTRACT.md R30, tools/extract/src/
# signalgen.rs), for an edition whose log carries no spec.
#
# For every log given, counts each `SPELL_CAST_SUCCESS` line whose source is
# a `Player-` guid, keyed by (spell id, name as the log wrote it). A form is
# cast before its aura goes on, so its cast is what is counted. One pass of
# grep + awk per log (the logs are large; never cat them). Output is a CSV,
# one row per (id, name), one count column per log named by the log's
# basename, sorted by id: the generator reads it via `wowdps-extract
# gen-role-signals --census` and writes the counts into
# crates/core/src/forever/role_signals.expected.md, so a curated name nobody
# has ever cast is visible in review. The committed copy is
# tools/role-signals-census.csv. Counts only: no player's name or guid
# leaves the log.
#
# usage: tools/census-role-signals.sh [-o out.csv] <WoWCombatLog-*.txt>...
set -euo pipefail

out=/dev/stdout
if [ "${1:-}" = "-o" ]; then
    out="$2"
    shift 2
fi
[ $# -ge 1 ] || { echo "usage: $0 [-o out.csv] <log>..." >&2; exit 2; }

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

header='id,name'
i=0
for log in "$@"; do
    i=$((i + 1))
    header="$header,$(basename "$log")"
    # $1 is "<ts>  SPELL_CAST_SUCCESS", $2 the source guid, $10/$11 the spell
    # id and quoted name (the log is CRLF, so the CR comes off first). A
    # name holding a comma shifts nothing before $11 and is counted under
    # its first part; none of the curated names holds one.
    LC_ALL=C grep -F '  SPELL_CAST_SUCCESS,Player-' "$log" \
        | LC_ALL=C awk -F, '{ sub(/\r$/, "") } { c[$10 "\t" $11]++ }
                            END { for (k in c) print k "\t" c[k] }' \
        > "$work/$i.tsv"
done

echo "$header" > "$out"
# Merge the per-log tallies on (id, name); a pair absent from a log reads 0.
LC_ALL=C awk -F'\t' -v n="$i" -v dir="$work" '
    { key = $1 "\t" $2; count[key, FILENAME] = $3; keys[key] = 1 }
    END {
        for (k in keys) {
            split(k, p, "\t")
            name = p[2]
            gsub(/^"|"$/, "", name); gsub(/"/, "\"\"", name)
            line = p[1] ",\"" name "\""
            for (j = 1; j <= n; j++) {
                v = count[k, dir "/" j ".tsv"]
                line = line "," (v == "" ? 0 : v)
            }
            print line
        }
    }' "$work"/[0-9]*.tsv | LC_ALL=C sort -t, -k1,1n >> "$out"
