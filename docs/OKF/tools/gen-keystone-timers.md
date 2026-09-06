---
type: Tool
title: gen-keystone-timers
description: Regenerate crates/core/src/keystone_timers.rs from the LOCAL game install.
resource: tools/gen-keystone-timers.sh
tags: [tool, game-data]
status: stable
generated: { by: okflight/0.4.0, at: 2026-08-06T21:43:22-07:00 }
---

Regenerate crates/core/src/keystone_timers.rs from the LOCAL game install. Replaces the retired gen-keystone-timers.py, which downloaded wago.tools CSV exports; MapChallengeMode.db2 now comes straight out of the install's own CASC storage via `wowdps-extract gen-keystone-timers` (emission rules in tools/extract/src/keystonegen.rs). Network is only used for the WoWDBDefs schema and the wowdev TACTKeys list, fetched fresh each run — this runs once per game patch (par times are retuned between seasons). Output is deterministic: same build in, same bytes out. usage: tools/gen-keystone-timers.sh [wow-dir] wow-dir: folder holding .build.info and Data/. When omitted the tool locates the install itself ($WOWDPS_WOW_DIR, the wowdps config's logs_dir, or a scan of Steam compatdata prefixes).

## Source

- Script: [`tools/gen-keystone-timers.sh`](../../../tools/gen-keystone-timers.sh)
- Extraction rules: [`tools/extract`](../crates/extract.md) (the `wowdps-extract` crate).
