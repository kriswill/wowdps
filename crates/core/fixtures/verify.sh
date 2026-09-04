#!/usr/bin/env bash
# verify.sh [<log> [<golden.tsv>]] — recompute totals with check.awk and diff vs
# golden. Exit 0 = match, 1 = mismatch. This is the harness the negative control
# must fail. With no arguments every gated fixture runs: sample.txt (R1-R7) and
# taken.txt (R17); a log given without a golden pairs with <log>.expected.tsv,
# falling back to sample.expected.tsv (the corrupt.txt negative control).
set -uo pipefail
here="$(cd "$(dirname "$0")" && pwd)"

check() {
    local log="$1" golden="$2" actual
    actual="$(gawk -f "$here/check.awk" "$log" "$log")"
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
    for name in sample taken; do
        check "$here/$name.txt" "$here/$name.expected.tsv" || rc=1
    done
    exit $rc
fi

log="$1"
default_golden="${log%.txt}.expected.tsv"
[ -f "$default_golden" ] || default_golden="$here/sample.expected.tsv"
check "$log" "${2:-$default_golden}"
