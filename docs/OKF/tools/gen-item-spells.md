---
type: Tool
title: gen-item-spells
description: Regenerate crates/core/src/item_spells.rs from the LOCAL game install.
resource: tools/gen-item-spells.sh
tags: [tool, game-data]
status: stable
generated: { by: okflight/0.4.0, at: 2026-08-09T22:32:53-07:00 }
---

Regenerate crates/core/src/item_spells.rs from the LOCAL game install. Twin of gen-class-spells.sh, for CONTRACT.md R12: which spells come from a trinket and which from a consumable, so the comparison timeline can mark trinket uses, trinket procs and pots. Tables come out of the install's own CASC storage via `wowdps-extract gen-item-spells` (join rules live in tools/extract/src/itemgen.rs). Network is only used for the WoWDBDefs schemas and the wowdev TACTKeys list, fetched fresh each run — this runs once per game patch. Output is deterministic: same build in, same bytes out. Note SpellEffect is a large table (~30 MB compressed in CASC); this takes noticeably longer than the class-spell generator. usage: tools/gen-item-spells.sh [wow-dir] wow-dir: folder holding .build.info and Data/. When omitted the tool locates the install itself ($WOWDPS_WOW_DIR, the wowdps config's logs_dir, or a scan of Steam compatdata prefixes).

## Source

- Script: [`tools/gen-item-spells.sh`](../../../tools/gen-item-spells.sh)
- Extraction rules: [`tools/extract`](../crates/extract.md) (the `wowdps-extract` crate).
