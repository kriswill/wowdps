---
type: Tool
title: gen-open-world-maps
description: Regenerate crates/core/src/open_world_maps.rs from the LOCAL game install.
resource: tools/gen-open-world-maps.sh
tags: [tool, game-data]
status: stable
generated: { by: okflight/0.4.0, at: 2026-10-05T05:53:07+00:00 }
---

Regenerate crates/core/src/open_world_maps.rs from the LOCAL game install. Map.db2 comes straight out of the install's own CASC storage via `wowdps-extract gen-open-world-maps` (emission rules in tools/extract/src/mapgen.rs): every map whose InstanceType is 0, the open world, so [R10](../rulings/r10.md) can read a door onto one as zoned out whatever difficulty the game stamped on it ([the decision](../decisions/an-open-world-door-is-zoned-out.md)). A map the table does not know (a newer patch) keeps the door's own difficulty, so a stale table fails toward the old reading. Network is only used for the WoWDBDefs schema and the wowdev TACTKeys list, fetched fresh each run — this runs once per game patch (a new expansion brings its continents). Output is deterministic: same build in, same bytes out. usage: tools/gen-open-world-maps.sh [wow-dir] wow-dir: folder holding .build.info and Data/. When omitted the tool locates the install itself ($WOWDPS_WOW_DIR, the wowdps config's logs_dir, or a scan of Steam compatdata prefixes).

## Source

- Script: [`tools/gen-open-world-maps.sh`](../../../tools/gen-open-world-maps.sh)
- Extraction rules: [`tools/extract`](../crates/extract.md) (the `wowdps-extract` crate).
