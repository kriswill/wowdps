#!/usr/bin/env bash
# Run GitHub's CodeQL code scanning locally, the way the repository's default
# setup runs it on every PR, and fail only on an alert the committed baseline
# does not list.
#
#   wowdps-codeql [--accept] [rust|actions]...
#
# The same languages (rust, actions), default suites and query pack versions
# as GitHub's run; with no language it runs both. GitHub fails a PR only on an
# alert not already open on main, so this exits 1 only on an alert missing
# from tools/codeql-baseline.txt: one line per accepted alert, its rule,
# primaryLocationLineHash and path, tab-separated (the rule and hash identify
# it, the path is for people). --accept rewrites the baseline's lines for the
# languages it ran from this run's alerts, and exits 0.
#
# The dev shells carry the CLI and the wowdps-codeql wrapper
# (nix/dev/codeql.nix); run it through the wrapper, which also points the
# extractor at a standard-library source it can resolve. Everything it writes
# lives under target/codeql/: the staged source, and per language a database,
# a SARIF file and a log. Exit codes: 0 no new alert, 1 new alerts, 2 a usage
# error, a CodeQL failure or a degraded database.
set -euo pipefail

usage="usage: wowdps-codeql [--accept] [rust|actions]..."
accept=0
langs=()
for arg in "$@"; do
  case "$arg" in
    --accept) accept=1 ;;
    rust | actions) langs+=("$arg") ;;
    -h | --help) echo "$usage" && exit 0 ;;
    *) echo "$usage" >&2 && exit 2 ;;
  esac
done
[ "${#langs[@]}" -gt 0 ] || langs=(rust actions)

# The suites GitHub's default setup runs, in the pack versions its run on
# 2026-10-23 used (CodeQL 2.27.3; the SARIF it uploads names them). Pinned,
# so every machine runs the same queries and a pack release never shows up
# here as new alerts: move them when GitHub's run moves.
suite() {
  case "$1" in
    rust) echo codeql/rust-queries@0.1.45:codeql-suites/rust-code-scanning.qls ;;
    actions) echo codeql/actions-queries@0.6.38:codeql-suites/actions-code-scanning.qls ;;
  esac
}

command -v codeql >/dev/null || {
  echo "wowdps-codeql: codeql is not on PATH (the dev shell carries it)" >&2
  exit 2
}
# nixpkgs packages the all-platforms CLI, which prints a deprecation banner on
# every call unless this is set.
export CODEQL_ALLOW_ALL_PLATFORMS_DIST=true

root="$(git rev-parse --show-toplevel)"
out="$root/target/codeql"
src="$out/src"
baseline="$root/tools/codeql-baseline.txt"
tab=$'\t'

# GitHub scans a fresh checkout, so it never sees an ignored file. The Rust
# extractor reads every .rs under its source root, ignored or not (the
# docs/gpui mirror, build-script output in target/), so it reads a copy of the
# tree as git sees it instead: the tracked files plus untracked ones no
# .gitignore covers. (--ignore-failed-read: a tracked file deleted from the
# working tree is skipped with a warning.)
rm -rf "$src"
mkdir -p "$src"
git -C "$root" ls-files -z --cached --others --exclude-standard |
  tar -C "$root" --null --no-recursion --ignore-failed-read -T - -cf - |
  tar -C "$src" -xf -

# step <command...>: run a CodeQL step into the language's log; on failure,
# print the log's tail and stop.
step() {
  "$@" >>"$log" 2>&1 || {
    tail -n 20 "$log" >&2
    echo "wowdps-codeql: $1 $2 $3 failed; the full log is ${log#"$root"/}" >&2
    exit 2
  }
}

# scan <lang>: the database, the analysis, and $out/<lang>.tsv with one row
# per alert (rule, hash, path, line, the message's first line without its
# link targets).
scan() {
  local lang=$1 db="$out/$1-db" sarif="$out/$1.sarif" pack
  pack="$(suite "$lang")"
  log="$out/$lang.log"
  : >"$log"
  # The pack comes from GitHub's container registry the first time only:
  # resolving the suite is offline, and fails while the pack is missing.
  if ! codeql resolve queries "$pack" >/dev/null 2>&1; then
    echo "$lang: downloading ${pack%%:*}" >&2
    step codeql pack download "${pack%%:*}"
  fi
  # Recreated on every run (--overwrite): a stale database reports on code
  # that has since changed, which is worse than a slow run.
  echo "$lang: creating the database (log: ${log#"$root"/})" >&2
  step codeql database create "$db" --language="$lang" --build-mode=none \
    --source-root="$src" --overwrite --threads=0
  echo "$lang: analyzing with $pack" >&2
  rm -f "$sarif"
  step codeql database analyze "$db" "$pack" --format=sarif-latest \
    --output="$sarif" --threads=0
  # A database the extractor could not resolve still analyzes, and quietly
  # reports fewer alerts, which would pass the gate. CodeQL flags one in the
  # SARIF; refuse it. (The usual cause is the std source the extractor reads:
  # the wrapper points RUST_SRC_PATH at one it resolves, nix/dev/codeql.nix.)
  degraded="$(jq -r 'first(.runs[].invocations[]?.toolExecutionNotifications[]?
      | select(.descriptor.id | endswith("/diagnostic/database-quality"))
      | .message.text) // empty' "$sarif")"
  if [ -n "$degraded" ]; then
    echo "wowdps-codeql: the $lang database is degraded, so its alerts are incomplete:" >&2
    echo "  $degraded" >&2
    echo "  (RUST_SRC_PATH=${RUST_SRC_PATH:-unset}; run it as wowdps-codeql)" >&2
    exit 2
  fi
  jq -r '.runs[].results[] | [
      .ruleId // .rule.id,
      .partialFingerprints.primaryLocationLineHash,
      .locations[0].physicalLocation.artifactLocation.uri,
      .locations[0].physicalLocation.region.startLine,
      (.message.text | split("\n")[0] | gsub("\\[(?<t>[^\\]]*)\\]\\([0-9]+\\)"; .t))
    ] | @tsv' "$sarif" >"$out/$lang.tsv"
}

# rows <1|0>: the baseline's lines, comments dropped, whose language (a rule
# is named <language>/...) ran (1) or did not (0).
rows() {
  [ -e "$baseline" ] || return 0
  awk -F'\t' -v ran=" ${langs[*]} " -v keep="$1" '
    /^#/ || !NF { next }
    { split($1, r, "/"); if ((index(ran, " " r[1] " ") > 0) == keep) print }
  ' "$baseline"
}

show() { awk -F'\t' '{ printf "  %s  %s:%s  %s\n", $1, $3, $4, $5 }' "$1"; }
count() { echo $(($(wc -l <"$1"))); }
alerts() { if [ "$1" = 1 ]; then echo "1 alert"; else echo "$1 alerts"; fi; }

: >"$out/alerts.tsv"
for lang in "${langs[@]}"; do
  start=$SECONDS
  scan "$lang"
  took=$((SECONDS - start))
  printf '%s: %s in %dm%02ds\n' "$lang" "$(alerts "$(count "$out/$lang.tsv")")" \
    $((took / 60)) $((took % 60))
  show "$out/$lang.tsv"
  cat "$out/$lang.tsv" >>"$out/alerts.tsv"
done

# Matched on rule and hash: the hash follows the line's text, so an alert
# keeps it when the lines above it move.
rows 1 >"$out/baseline.tsv"
awk -F'\t' 'FILENAME == ARGV[1] { seen[$1 FS $2]; next } !(($1 FS $2) in seen)' \
  "$out/baseline.tsv" "$out/alerts.tsv" >"$out/new.tsv"
open=$(awk -F'\t' 'FILENAME == ARGV[1] { seen[$1 FS $2]; next } ($1 FS $2) in seen' \
  "$out/alerts.tsv" "$out/baseline.tsv" | wc -l)
open=$((open))
total=$(count "$out/baseline.tsv")
if [ "$open" -eq "$total" ]; then
  echo "baseline: $open of $total accepted alerts still open"
else
  echo "baseline: $open of $total accepted alerts still open (--accept drops the rest)"
fi

new=$(count "$out/new.tsv")
if [ "$new" -gt 0 ]; then
  echo "new: $(alerts "$new") not in tools/codeql-baseline.txt"
  show "$out/new.tsv"
fi

if [ "$accept" = 1 ]; then
  {
    cat <<'EOF'
# The CodeQL alerts accepted as open, which wowdps-codeql (tools/codeql.sh)
# lets pass: rule, primaryLocationLineHash and path, tab-separated. The rule
# and hash identify an alert; the path is for people. Rewritten by
# `wowdps-codeql --accept`.
EOF
    { rows 0 && cut -f1-3 "$out/alerts.tsv"; } |
      LC_ALL=C sort -u -t "$tab" -k3,3 -k1,1 -k2,2
  } >"$baseline.new"
  mv "$baseline.new" "$baseline"
  echo "accepted: tools/codeql-baseline.txt lists $(alerts "$(grep -vc '^#' "$baseline")")"
  exit 0
fi

[ "$new" -eq 0 ] || exit 1
echo "ok: no new alerts"
