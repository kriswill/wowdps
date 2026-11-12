#!/usr/bin/env bash
# Regenerate crates/core/src/role_spells.rs (+ role_spells.expected.md) from
# the LOCAL game install, or with --product wowf Forever's
# crates/core/src/forever/role_spells.rs from its client.
#
# Twin of gen-item-spells.sh, for CONTRACT.md R18: which aura ids are a tank's
# active mitigation, a defensive, an external, a support buff or an offensive
# cooldown, so the meter can open a span on the buff's target with its caster.
# The membership is CURATED in tools/extract/src/rolegen.rs; the install only
# proves each entry (SpellName must match, SpellEffect must hold an APPLY_AURA
# row) and the committed real-log census (tools/role-spells-census.csv, from
# tools/census-role-spells.sh) must show it applied to a player. Network is
# only used for the WoWDBDefs schemas and the wowdev TACTKeys list, fetched
# fresh each run — this runs once per game patch or when the curated list
# changes. Output is deterministic: same build + census in, same bytes out.
#
# Forever's list is curated by NAME in tools/extract/src/spangen.rs (each rank
# of a spell is an id of its own): the install expands each name to every id
# it proves (SpellName, an APPLY_AURA row, a class's spell family or a tie to
# a family cast), and Forever's own census (tools/forever/role-spells-census.csv)
# must show one applied, unless the entry carries a waiver. Each edition's run
# reads its own committed census unless --census names another.
#
# Note SpellEffect is a large table (~30 MB compressed in CASC); this takes
# noticeably longer than the class-spell generator.
#
# usage: tools/gen-role-spells.sh [wow-dir] [--product ROW [-o FILE]]
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
fetch_dbds gen-role-spells "$work"
fetch_keys "$work/tactkeys.txt"

# The words given come last, so a -o among them wins; without one the
# table goes to its edition's committed file in this checkout.
"$extract" gen-role-spells \
    --dbd-dir "$work" --keys "$work/tactkeys.txt" \
    --checkout "$root" "$@"
