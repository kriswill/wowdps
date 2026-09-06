#!/usr/bin/env bash
# Census of the buffs real combat logs apply to PLAYERS — the evidence behind
# the curated role-spell table (CONTRACT.md R18, tools/extract/src/rolegen.rs).
#
# For every log given, counts each `SPELL_AURA_APPLIED` line whose target is
# a `Player-` guid and whose aura type is `BUFF`, keyed by (spell id, name as
# the log wrote it). One pass of grep + awk per log (the logs are large; never
# cat them). Output is a CSV, one row per (id, name), one count column per
# log named by the log's basename, sorted by id: the generator reads it via
# `wowdps-extract gen-role-spells --census` and writes the counts into
# crates/core/src/role_spells.expected.md, so a curated id nobody has ever
# seen applied to a player is visible in review. The committed copy is
# tools/role-spells-census.csv.
#
# usage: tools/census-role-spells.sh [-o out.csv] <WoWCombatLog-*.txt>...
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
    # $1 is "<ts>  SPELL_AURA_APPLIED", $6 the target guid, $10/$11 the aura
    # id and quoted name, $13 BUFF/DEBUFF (13 fields; trailing optionals are
    # ignored; the log is CRLF, so the CR comes off first). A spell name
    # holding a comma ("First In, Last Out") shifts $13 and the line is
    # skipped — a handful per log, none of them a role spell.
    LC_ALL=C grep -F '  SPELL_AURA_APPLIED,' "$log" \
        | LC_ALL=C awk -F, '{ sub(/\r$/, "") }
                            $6 ~ /^Player-/ && $13 == "BUFF" { c[$10 "\t" $11]++ }
                            END { for (k in c) print k "\t" c[k] }' \
        > "$work/$i.tsv"
done

echo "$header" > "$out"
# Merge the per-log tallies on (id, name); a pair absent from a log reads 0.
LC_ALL=C awk -F'\t' -v n="$i" '
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
    }' dir="$work" "$work"/[0-9]*.tsv | LC_ALL=C sort -t, -k1,1n >> "$out"
