#!/usr/bin/env bash
# Census of how real combat logs tie each curated proc to its driver — the
# log-side evidence behind the curated proc table (CONTRACT.md R26,
# tools/extract/src/procgen.rs), beside the install-side evidence the
# generator reads from the client's own spell text.
#
# For every curated (proc id, driver name) pair and every log given, counts:
#   onsets     the proc's direct events by a player (or a unit a player
#              summoned): SPELL_DAMAGE, SPELL_HEAL, SPELL_MISSED, and its aura
#              applied or refreshed — never a periodic tick
#   f50 f250 f1500
#              of those onsets, how many came within 50 / 250 / 1500 ms after
#              the same player's own event of the driver's name (a cast, a
#              hit, a heal, a miss, the aura applied or refreshed)
#   lands      every event of the proc, ticks included
#   on         of those, how many landed on a target that carried the same
#              player's aura of the driver's name — or lost it within 250 ms,
#              for a proc that fires as its host dies
# A pet's lines count as its owner's through SPELL_SUMMON; a pet summoned
# before the log began counts as nobody's. One pass of grep + awk per log
# (the logs are large; never cat them). The pairs come from the generator
# itself (`wowdps-extract proc-pairs`), so the census and the table cannot
# disagree on what is curated. Output is a CSV, one row per (pair, log),
# sorted; the committed copy is tools/proc-spells-census.csv, which
# `wowdps-extract gen-proc-spells --census` reads.
#
# usage: tools/census-proc-spells.sh [-o out.csv] <WoWCombatLog-*.txt>...
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
out=/dev/stdout
if [ "${1:-}" = "-o" ]; then
    out="$2"
    shift 2
fi
[ $# -ge 1 ] || { echo "usage: $0 [-o out.csv] <log>..." >&2; exit 2; }

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

cargo build -q --release --manifest-path "$root/Cargo.toml" -p wowdps-extract
"$root/target/release/wowdps-extract" proc-pairs > "$work/pairs.tsv"

i=0
for log in "$@"; do
    i=$((i + 1))
    # $1 is "<ts>  EVENT", $2 the source guid, $6 the destination, $10/$11
    # the spell id and quoted name (a name holding a comma shifts them and
    # the line is skipped — a handful per log). The log is CRLF.
    LC_ALL=C grep -F '  SPELL_' "$log" \
        | LC_ALL=C awk -F, -v LOG="$(basename "$log")" -v PAIRS="$work/pairs.tsv" '
            function ms(ts,   a, t) {
                split(ts, a, " "); split(a[2], t, ":")
                return (t[1] * 3600 + t[2] * 60 + t[3]) * 1000
            }
            function who(g) {
                if (g ~ /^Player-/) return g
                return (g in owner) ? owner[g] : ""
            }
            BEGIN {
                while ((getline line < PAIRS) > 0) {
                    split(line, f, "\t")
                    np++; pid[np] = f[1]; pdrv[np] = f[2]
                    procs[f[1]] = procs[f[1]] (procs[f[1]] == "" ? "" : SUBSEP) np
                    drivers["\"" f[2] "\""] = 1
                }
            }
            {
                sub(/\r$/, "")
                split($1, h, "  "); ev = h[2]
                if (ev == "SPELL_SUMMON") {
                    w = who($2); if (w != "") owner[$6] = w
                    next
                }
                id = $10; name = $11
                isproc = (id in procs); isdrv = (name in drivers)
                if (!isproc && !isdrv) next
                w = who($2); if (w == "") next
                t = ms(h[1])
                direct = (ev == "SPELL_DAMAGE" || ev == "SPELL_HEAL" || ev == "SPELL_MISSED" \
                          || ev == "SPELL_AURA_APPLIED" || ev == "SPELL_AURA_REFRESH")
                if (isproc) {
                    pname[id] = name
                    n = split(procs[id], ks, SUBSEP)
                    for (j = 1; j <= n; j++) {
                        k = ks[j]; d = "\"" pdrv[k] "\""
                        lands[k]++
                        o = ((w, $6, d) in open) && open[w, $6, d]
                        if (!o && ((w, $6, d) in gone) && t - gone[w, $6, d] <= 250) o = 1
                        if (o) on[k]++
                        if (direct) {
                            onsets[k]++
                            if ((w, d) in last) {
                                lag = t - last[w, d]
                                if (lag >= 0 && lag <= 50) f50[k]++
                                if (lag >= 0 && lag <= 250) f250[k]++
                                if (lag >= 0 && lag <= 1500) f1500[k]++
                            }
                        }
                    }
                }
                if (isdrv) {
                    if (direct || ev == "SPELL_CAST_SUCCESS") last[w, name] = t
                    if (ev == "SPELL_AURA_APPLIED" || ev == "SPELL_AURA_REFRESH" || ev == "SPELL_AURA_APPLIED_DOSE") {
                        open[w, $6, name] = 1
                    } else if (ev == "SPELL_AURA_REMOVED") {
                        open[w, $6, name] = 0; gone[w, $6, name] = t
                    }
                }
            }
            END {
                for (k = 1; k <= np; k++) {
                    nm = (pid[k] in pname) ? pname[pid[k]] : "\"\""
                    printf "%s,%s,\"%s\",%s,%d,%d,%d,%d,%d,%d\n", pid[k], nm, pdrv[k], LOG, \
                        onsets[k], f50[k], f250[k], f1500[k], lands[k], on[k]
                }
            }' > "$work/$i.csv"
done

{
    echo 'proc_id,proc_name,driver,log,onsets,f50,f250,f1500,lands,on'
    LC_ALL=C sort -t, -k1,1n -k3,3 -k4,4 "$work"/*.csv
} > "$out"
