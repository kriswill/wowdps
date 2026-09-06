#!/usr/bin/env bash
# Regenerate crates/core/src/role_spells.rs (+ role_spells.expected.md) from
# the LOCAL game install.
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
# Note SpellEffect is a large table (~30 MB compressed in CASC); this takes
# noticeably longer than the class-spell generator.
#
# usage: tools/gen-role-spells.sh [wow-dir]
#   wow-dir: folder holding .build.info and Data/. When omitted the tool
#   locates the install itself ($WOWDPS_WOW_DIR, the wowdps config's
#   logs_dir, or a scan of Steam compatdata prefixes).
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
wow="${1:-}"

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

# Table list must match rolegen::TABLES (the tool errors on a missing dbd).
for t in SpellName SpellEffect; do
    curl -sfL "https://raw.githubusercontent.com/wowdev/WoWDBDefs/master/definitions/$t.dbd" \
        -o "$work/$t.dbd" || { echo "failed to fetch $t.dbd" >&2; exit 1; }
done
curl -sfL "https://raw.githubusercontent.com/wowdev/TACTKeys/master/WoW.txt" \
    -o "$work/tactkeys.txt" || { echo "failed to fetch TACTKeys" >&2; exit 1; }

cargo build -q --release --manifest-path "$root/Cargo.toml" -p wowdps-extract
"$root/target/release/wowdps-extract" gen-role-spells ${wow:+"$wow"} \
    --dbd-dir "$work" --keys "$work/tactkeys.txt" \
    --census "$root/tools/role-spells-census.csv" \
    -o "$root/crates/core/src/role_spells.rs"
