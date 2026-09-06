---
type: Tool
title: gen-spell-icons
description: Regenerate the spell-icon cache from the LOCAL game install.
resource: tools/gen-spell-icons.sh
tags: [tool, game-data]
status: stable
generated: { by: okflight/0.4.0, at: 2026-08-09T22:32:53-07:00 }
---

Regenerate the spell-icon cache from the LOCAL game install. Every spell id's icon (SpellMisc.SpellIconFileDataID), BLP-decoded to 32x32 RGBA and deduplicated into $XDG_DATA_HOME/wowdps/spell-icons.bin (~60 MiB) — the GUI reads it lazily to draw ability icons next to spell rows, and draws none when the file is absent. Unlike the other gen-* outputs this is a PER-MACHINE CACHE, never committed. Network is only used for the WoWDBDefs schema and the wowdev TACTKeys list — this runs once per game patch, and takes a few minutes (~14k icon files). usage: tools/gen-spell-icons.sh [wow-dir] wow-dir: folder holding .build.info and Data/. When omitted the tool locates the install itself ($WOWDPS_WOW_DIR, the wowdps config's logs_dir, or a scan of Steam compatdata prefixes).

## Source

- Script: [`tools/gen-spell-icons.sh`](../../../tools/gen-spell-icons.sh)
- Extraction rules: [`tools/extract`](../crates/extract.md) (the `wowdps-extract` crate).
