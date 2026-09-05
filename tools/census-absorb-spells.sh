#!/usr/bin/env bash
# Census of the shields real combat logs ABSORB with — the evidence beside
# the discovered absorb-spell table (CONTRACT.md R20, tools/extract/src/absorbgen.rs).
#
# For every log given, counts each `SPELL_ABSORBED` line by the absorb spell
# it names — (spell id, name as the log wrote it). One pass of grep + awk per
# log (the logs are large; never cat them). Output is a CSV, one row per (id,
# name), one count column per log named by the log's basename, sorted by id:
# the generator reads it via `wowdps-extract gen-absorb-spells --census` and
# writes the counts into crates/core/src/absorb_spells.expected.md, listing
# every census id the client tables do NOT mark as SCHOOL_ABSORB — the shields
# the ledger can only count as unknown-applied. The committed copy is
# tools/absorb-spells-census.csv. Twin of tools/census-role-spells.sh.
#
# usage: tools/census-absorb-spells.sh [-o out.csv] <WoWCombatLog-*.txt>...
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
    # SPELL_ABSORBED is 19 or 22 fields after the "<ts>  event" head (the
    # 22-field shape carries the damage spell block), so the absorb spell
    # block is addressed from the END: its quoted name ends at field n-4 and
    # its id sits just before the name (the log is CRLF, so the CR comes off
    # first). A shield name holding a comma ("First In, Last Out") spans
    # several fields, so the name is walked back to its opening quote and
    # rejoined. No target filter: every shield that absorbed is evidence.
    LC_ALL=C grep -F '  SPELL_ABSORBED,' "$log" \
        | LC_ALL=C awk -F, '{ sub(/\r$/, ""); j = NF - 4
                              while (j > 1 && $j !~ /^"/) j--
                              name = $j; for (k = j + 1; k <= NF - 4; k++) name = name "," $k
                              c[$(j-1) "\t" name]++ }
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
