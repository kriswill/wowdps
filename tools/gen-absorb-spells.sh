#!/usr/bin/env bash
# Regenerate crates/core/src/absorb_spells.rs (+ absorb_spells.expected.md) from
# the LOCAL game install, or with --product wowf Forever's
# crates/core/src/forever/absorb_spells.rs from its client.
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
# Each edition's run reads its own committed census (Retail's
# tools/absorb-spells-census.csv, Forever's tools/forever/absorb-spells-census.csv,
# both from tools/census-absorb-spells.sh) unless --census names another.
#
# usage: tools/gen-absorb-spells.sh [wow-dir] [--product ROW [-o FILE]]
#   wow-dir: folder holding .build.info and Data/. When omitted the tool
#   locates the install itself ($WOWDPS_WOW_DIR, the wowdps config's
#   logs_dir, or a scan of the install roots (Steam compatdata on
#   Linux, /Applications and ~/Applications on macOS).
#   --product ROW: another edition of the game (tools/lib.sh): Forever's
#   (wowf) writes its committed table and reads its census; one with
#   none needs -o FILE, and one no edition names (a test realm's row)
#   --census CSV too.
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
. "$root/tools/lib.sh"

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

build_extract
fetch_dbds gen-absorb-spells "$work"
fetch_keys "$work/tactkeys.txt"

# The words given come last, so a -o among them wins; without one the
# table goes to its edition's committed file in this checkout.
"$extract" gen-absorb-spells \
    --dbd-dir "$work" --keys "$work/tactkeys.txt" \
    --checkout "$root" "$@"
