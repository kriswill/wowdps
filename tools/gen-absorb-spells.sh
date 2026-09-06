#!/usr/bin/env bash
# Regenerate crates/core/src/absorb_spells.rs (+ absorb_spells.expected.md) from
# the LOCAL game install.
#
# Twin of gen-item-spells.sh, for CONTRACT.md R20: which aura ids are a damage
# SHIELD, so the meter's shield ledger admits their APPLIED / REFRESH / REMOVED
# lines (and their trailers) and nothing else's. The membership is DISCOVERED,
# not curated: every spell with a SpellEffect row whose EffectAura is 69
# (SCHOOL_ABSORB), read straight out of the install (rules in
# tools/extract/src/absorbgen.rs) — no hand list, no census. The fail-loud
# gate is the fixture: crates/core/fixtures/shields.txt's shield spells must
# be in the table (crates/core/tests/shields.rs) — an absorb naming a spell
# OUTSIDE the table still ledgers as unknown-applied, so a stale table loses
# no healing, only sizes. Network is only used for the WoWDBDefs schemas and
# the wowdev TACTKeys list, fetched fresh each run — this runs once per game
# patch. Output is deterministic: same build in, same bytes out.
#
# Note SpellEffect is a large table (~30 MB compressed in CASC); this takes
# noticeably longer than the class-spell generator.
#
# usage: tools/gen-absorb-spells.sh [wow-dir]
#   wow-dir: folder holding .build.info and Data/. When omitted the tool
#   locates the install itself ($WOWDPS_WOW_DIR, the wowdps config's
#   logs_dir, or a scan of Steam compatdata prefixes).
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
wow="${1:-}"

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

# Table list must match absorbgen::TABLES (the tool errors on a missing dbd).
for t in SpellName SpellEffect; do
    curl -sfL "https://raw.githubusercontent.com/wowdev/WoWDBDefs/master/definitions/$t.dbd" \
        -o "$work/$t.dbd" || { echo "failed to fetch $t.dbd" >&2; exit 1; }
done
curl -sfL "https://raw.githubusercontent.com/wowdev/TACTKeys/master/WoW.txt" \
    -o "$work/tactkeys.txt" || { echo "failed to fetch TACTKeys" >&2; exit 1; }

cargo build -q --release --manifest-path "$root/Cargo.toml" -p wowdps-extract
"$root/target/release/wowdps-extract" gen-absorb-spells ${wow:+"$wow"} \
    --dbd-dir "$work" --keys "$work/tactkeys.txt" \
    --census "$root/tools/absorb-spells-census.csv" \
    -o "$root/crates/core/src/absorb_spells.rs"
