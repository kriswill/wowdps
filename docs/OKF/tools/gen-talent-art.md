---
type: Tool
title: gen-talent-art
description: 'Regenerate ~/.local/share/wowdps/talent-art.bin: the talent UI''s own artwork cropped from the client''s texture atlases — per-spec pane background paintings, each hero tree''s round medallion, and the golden medallion ring.'
resource: tools/gen-talent-art.sh
tags: [tool, game-data]
status: stable
generated: { by: okflight/0.4.0, at: 2026-08-27T16:34:35-07:00 }
---

Regenerate ~/.local/share/wowdps/talent-art.bin: the talent UI's own artwork cropped from the client's texture atlases — per-spec pane background paintings, each hero tree's round medallion, and the golden medallion ring. A per-machine cache like the icon bins (extracted Blizzard art never lands in the repo); the GUI's talent viewer renders fine without it. Network is used only for the WoWDBDefs schemas and TACT keys; the textures come from the local install's CASC storage. tools/gen-talent-art.sh [wow-dir].

## Source

- Script: [`tools/gen-talent-art.sh`](../../../tools/gen-talent-art.sh)
- Extraction rules: [`tools/extract`](../crates/extract.md) (the `wowdps-extract` crate).
