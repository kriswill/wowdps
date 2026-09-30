---
type: Tool
title: gen-proc-spells
description: Regenerate crates/core/src/proc_spells.rs (+ proc_spells.expected.md) from the LOCAL game install.
resource: tools/gen-proc-spells.sh
tags: [tool, game-data]
status: stable
generated: { by: okflight/0.4.0, at: 2026-09-30T06:08:55+00:00 }
---

Regenerate crates/core/src/proc_spells.rs (+ proc_spells.expected.md) from the LOCAL game install. Twin of gen-role-spells.sh, for CONTRACT.md R26's ability tree: which talent proc hangs under which driver (Blackened Soul under Wither, Expurgation under Blade of Justice). The membership is CURATED in tools/extract/src/procgen.rs; the install proves each entry (SpellName must carry both names, and the client's own text — the proc's description or one it references — must name the driver, reference its id, or have one of the driver's effects trigger the proc) and the committed real-log census (tools/proc-spells-census.csv, from tools/census-proc-spells.sh) must show the pair in play. Network is only used for the WoWDBDefs schemas and the wowdev TACTKeys list, fetched fresh each run — this runs once per game patch or when the curated list changes. Output is deterministic: same build + census in, same bytes out. Note SpellEffect and Spell are large tables; this takes a minute or two. usage: tools/gen-proc-spells.sh [wow-dir] wow-dir: folder holding .build.info and Data/. When omitted the tool locates the install itself ($WOWDPS_WOW_DIR, the wowdps config's logs_dir, or a scan of Steam compatdata prefixes).

## Source

- Script: [`tools/gen-proc-spells.sh`](../../../tools/gen-proc-spells.sh)
- Extraction rules: [`tools/extract`](../crates/extract.md) (the `wowdps-extract` crate). The curated list and both proofs live in `procgen.rs`; `wowdps-extract proc-pairs` prints the pairs the census counts.
- Census: [census-proc-spells](census-proc-spells.md) writes `tools/proc-spells-census.csv`, the log-side proof.
- Output: `crates/core/src/proc_spells.rs` and its review twin `proc_spells.expected.md`, read by [R26](../rulings/r26.md)'s `Segment::spell_tree` in [`wowdps-core`](../crates/core.md).
- Why it is curated: [A Talent Proc Nests Under Its Driver](../decisions/proc-under-its-driver.md).
