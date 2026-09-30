#!/usr/bin/env bash
# Regenerate crates/core/src/proc_spells.rs (+ proc_spells.expected.md) from
# the LOCAL game install.
#
# Twin of gen-role-spells.sh, for CONTRACT.md R26's ability tree: which
# talent proc hangs under which driver (Blackened Soul under Wither,
# Expurgation under Blade of Justice). The membership is CURATED in
# tools/extract/src/procgen.rs; the install proves each entry (SpellName must
# carry both names, and the client's own text — the proc's description or one
# it references — must name the driver, reference its id, or have one of the
# driver's effects trigger the proc) and the committed real-log census
# (tools/proc-spells-census.csv, from tools/census-proc-spells.sh) must show
# the pair in play. Network is only used for the WoWDBDefs schemas and the
# wowdev TACTKeys list, fetched fresh each run — this runs once per game
# patch or when the curated list changes. Output is deterministic: same
# build + census in, same bytes out.
#
# Note SpellEffect and Spell are large tables; this takes a minute or two.
#
# usage: tools/gen-proc-spells.sh [wow-dir]
#   wow-dir: folder holding .build.info and Data/. When omitted the tool
#   locates the install itself ($WOWDPS_WOW_DIR, the wowdps config's
#   logs_dir, or a scan of Steam compatdata prefixes).
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
wow="${1:-}"

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

# Table list must match procgen::TABLES (the tool errors on a missing dbd).
for t in SpellName Spell SpellEffect; do
    curl -sfL "https://raw.githubusercontent.com/wowdev/WoWDBDefs/master/definitions/$t.dbd" \
        -o "$work/$t.dbd" || { echo "failed to fetch $t.dbd" >&2; exit 1; }
done
curl -sfL "https://raw.githubusercontent.com/wowdev/TACTKeys/master/WoW.txt" \
    -o "$work/tactkeys.txt" || { echo "failed to fetch TACTKeys" >&2; exit 1; }

cargo build -q --release --manifest-path "$root/Cargo.toml" -p wowdps-extract
"$root/target/release/wowdps-extract" gen-proc-spells ${wow:+"$wow"} \
    --dbd-dir "$work" --keys "$work/tactkeys.txt" \
    --census "$root/tools/proc-spells-census.csv" \
    -o "$root/crates/core/src/proc_spells.rs"
