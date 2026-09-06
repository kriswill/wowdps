#!/usr/bin/env bash
# verify.sh [<log> [<golden.tsv>]] — recompute totals with check.awk and diff vs
# golden. Exit 0 = match, 1 = mismatch. This is the harness the negative control
# must fail. With no arguments every gated fixture runs: sample.txt (R1-R7),
# taken.txt (R17), support.txt (R19 + the R2 amendment), spans.txt (R18) and
# shields.txt (R20); a log given without a golden pairs with <log>.expected.tsv,
# falling back to sample.expected.tsv (the corrupt.txt negative control).
# check.awk's own self-checks (R20: a REMOVED trailer disagreeing with the
# running remaining) exit non-zero and FAIL the log regardless of the diff.
set -uo pipefail
here="$(cd "$(dirname "$0")" && pwd)"

check() {
    local log="$1" golden="$2" actual rc=0
    actual="$(gawk -f "$here/check.awk" "$log" "$log")" || rc=$?
    if [ "$rc" -ne 0 ]; then
        echo "FAIL: $(basename "$log") — check.awk self-check failed (exit $rc)"
        return 1
    fi
    if diff -u "$golden" <(printf '%s\n' "$actual") > /tmp/wowdps-verify.diff 2>&1; then
        echo "PASS: $(basename "$log") matches $(basename "$golden")"
        return 0
    else
        echo "FAIL: $(basename "$log") does NOT match $(basename "$golden")"
        sed -n '1,40p' /tmp/wowdps-verify.diff
        return 1
    fi
}

if [ $# -eq 0 ]; then
    rc=0
    for name in sample taken support spans shields; do
        check "$here/$name.txt" "$here/$name.expected.tsv" || rc=1
    done
    exit $rc
fi

log="$1"
default_golden="${log%.txt}.expected.tsv"
[ -f "$default_golden" ] || default_golden="$here/sample.expected.tsv"
check "$log" "${2:-$default_golden}"
