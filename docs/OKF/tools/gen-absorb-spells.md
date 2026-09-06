---
type: Tool
title: gen-absorb-spells
description: Regenerate crates/core/src/absorb_spells.rs (+ absorb_spells.expected.md) from the LOCAL game install.
resource: tools/gen-absorb-spells.sh
tags: [tool, game-data]
status: stable
generated: { by: okflight/0.4.0, at: 2026-09-05T00:53:27-07:00 }
---

Regenerate crates/core/src/absorb_spells.rs (+ absorb_spells.expected.md) from the LOCAL game install. Twin of gen-item-spells.sh, for CONTRACT.md R20: which aura ids are a damage SHIELD, so the meter's shield ledger admits their APPLIED / REFRESH / REMOVED lines (and their trailers) and nothing else's. The membership is DISCOVERED, not curated: every spell with a SpellEffect row whose EffectAura is 69 (SCHOOL_ABSORB), read straight out of the install (rules in tools/extract/src/absorbgen.rs) — no hand list, no census. The fail-loud gate is the fixture: crates/core/fixtures/shields.txt's shield spells must be in the table (crates/core/tests/shields.rs) — an absorb naming a spell OUTSIDE the table still ledgers as unknown-applied, so a stale table loses no healing, only sizes. Network is only used for the WoWDBDefs schemas and the wowdev TACTKeys list, fetched fresh each run — this runs once per game patch. Output is deterministic: same build in, same bytes out. Note SpellEffect is a large table (~30 MB compressed in CASC); this takes noticeably longer than the class-spell generator. usage: tools/gen-absorb-spells.sh [wow-dir] wow-dir: folder holding .build.info and Data/. When omitted the tool locates the install itself ($WOWDPS_WOW_DIR, the wowdps config's logs_dir, or a scan of Steam compatdata prefixes).

## Source

- Script: [`tools/gen-absorb-spells.sh`](../../../tools/gen-absorb-spells.sh)
- Extraction rules: [`tools/extract`](../crates/extract.md) (the `wowdps-extract` crate).
