---
type: Tool
title: gen-icons
description: Regenerate the class/spec icon cache from the LOCAL game install.
resource: tools/gen-icons.sh
tags: [tool, game-data]
status: stable
generated: { by: okflight/0.4.0, at: 2026-08-09T22:32:53-07:00 }
---

Regenerate the class/spec icon cache from the LOCAL game install. The game's own class crests (interface/icons/classicon_*.blp) and spec icons (ChrSpecialization.SpellIconFileID), BLP-decoded, downscaled to 32x32, circle-masked and written to $XDG_DATA_HOME/wowdps/class-icons.bin — a PER-MACHINE cache the GUI reads at runtime (rules in icongen.rs, BLP in blp.rs). Network is only used for the WoWDBDefs schema and the wowdev TACTKeys list, fetched fresh each run — this runs once per game patch. Output is deterministic: same build in, same bytes out. Like spell-icons.bin this is extracted Blizzard artwork and lives OUTSIDE the repository on purpose; a machine without it falls back to drawn discs. usage: tools/gen-icons.sh [wow-dir] wow-dir: folder holding .build.info and Data/. When omitted the tool locates the install itself ($WOWDPS_WOW_DIR, the wowdps config's logs_dir, or a scan of Steam compatdata prefixes).

## Source

- Script: [`tools/gen-icons.sh`](../../../tools/gen-icons.sh)
- Extraction rules: [`tools/extract`](../crates/extract.md) (the `wowdps-extract` crate).
