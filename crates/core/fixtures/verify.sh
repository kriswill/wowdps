#!/usr/bin/env bash
# verify.sh <log> <golden.tsv> — recompute totals with check.awk and diff vs golden.
# Exit 0 = match, 1 = mismatch. This is the harness the negative control must fail.
set -uo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
log="${1:-$here/sample.txt}"
golden="${2:-$here/sample.expected.tsv}"
actual="$(gawk -f "$here/check.awk" "$log" "$log")"
if diff -u "$golden" <(printf '%s\n' "$actual") > /tmp/wowdps-verify.diff 2>&1; then
    echo "PASS: $(basename "$log") matches $(basename "$golden")"
    exit 0
else
    echo "FAIL: $(basename "$log") does NOT match $(basename "$golden")"
    sed -n '1,40p' /tmp/wowdps-verify.diff
    exit 1
fi
