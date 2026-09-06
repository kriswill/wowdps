---
type: Tool
title: gen-talent-trees
description: Regenerate the talent-tree dataset from the LOCAL game install.
resource: tools/gen-talent-trees.sh
tags: [tool, game-data]
status: stable
generated: { by: okflight/0.4.0, at: 2026-08-27T15:37:26-07:00 }
---

Regenerate the talent-tree dataset from the LOCAL game install. Every class's full trait tree — nodes, edges, choice entries, hero subtrees, spec gating, point costs, spell names and icon names — joined out of the install's own CASC storage into $XDG_DATA_HOME/wowdps/talents.json, the file the MCP server's talent tools (talent_tree / decode_talents / encode_talents) and the wow-coach tree viewer read. Like the icon caches this is extracted Blizzard data and lives OUTSIDE the repository on purpose. Network is used for the WoWDBDefs schemas, the wowdev TACTKeys list, and (cached, it is ~140 MB) the wowdev community listfile that names icon files for the wowhead CDN — this runs once per game patch. Output is deterministic: same build in, same bytes out. usage: tools/gen-talent-trees.sh [wow-dir] wow-dir: folder holding .build.info and Data/. When omitted the tool locates the install itself ($WOWDPS_WOW_DIR, the wowdps config's logs_dir, or a scan of Steam compatdata prefixes).

## Source

- Script: [`tools/gen-talent-trees.sh`](../../../tools/gen-talent-trees.sh)
- Extraction rules: [`tools/extract`](../crates/extract.md) (the `wowdps-extract` crate).
