---
type: Tool
title: gen-class-spells
description: Regenerate crates/core/src/class_spells.rs from the LOCAL game install.
resource: tools/gen-class-spells.sh
tags: [tool, game-data]
status: stable
generated: { by: okflight/0.4.0, at: 2026-08-06T21:43:22-07:00 }
---

Regenerate crates/core/src/class_spells.rs from the LOCAL game install. Replaces the retired gen-class-spells.py, which downloaded wago.tools CSV exports; the tables now come straight out of the install's own CASC storage via `wowdps-extract gen-class-spells` (attribution rules per CONTRACT.md R8 live in tools/extract/src/classgen.rs). Network is only used for the WoWDBDefs schemas and the wowdev TACTKeys list, fetched fresh each run — this runs once per game patch. Output is deterministic: same build in, same bytes out. usage: tools/gen-class-spells.sh [wow-dir] wow-dir: folder holding .build.info and Data/. When omitted the tool locates the install itself ($WOWDPS_WOW_DIR, the wowdps config's logs_dir, or a scan of Steam compatdata prefixes).

## Source

- Script: [`tools/gen-class-spells.sh`](../../../tools/gen-class-spells.sh)
- Extraction rules: [`tools/extract`](../crates/extract.md) (the `wowdps-extract` crate).
