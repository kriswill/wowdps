---
type: Tool
title: census-proc-spells
description: 'Census of how real combat logs tie each curated proc to its driver — the log-side evidence behind the curated proc table (CONTRACT.md R26, tools/extract/src/procgen.rs), beside the install-side evidence the generator reads from the client''s own spell text.'
resource: tools/census-proc-spells.sh
tags: [tool, game-data]
status: stable
generated: { by: okflight/0.4.0, at: 2026-09-30T06:08:55+00:00 }
---

Census of how real combat logs tie each curated proc to its driver — the log-side evidence behind the curated proc table (CONTRACT.md R26, tools/extract/src/procgen.rs), beside the install-side evidence the generator reads from the client's own spell text. For every curated (proc id, driver name) pair and every log given, counts: onsets the proc's direct events by a player (or a unit a player summoned): SPELL_DAMAGE, SPELL_HEAL, SPELL_MISSED, and its aura applied or refreshed — never a periodic tick f50 f250 f1500 of those onsets, how many came within 50 / 250 / 1500 ms after the same player's own event of the driver's name (a cast, a hit, a heal, a miss, the aura applied or refreshed) lands every event of the proc, ticks included on of those, how many landed on a target that carried the same player's aura of the driver's name — or lost it within 250 ms, for a proc that fires as its host dies A pet's lines count as its owner's through SPELL_SUMMON; a pet summoned before the log began counts as nobody's. One pass of grep + awk per log (the logs are large; never cat them). The pairs come from the generator itself (`wowdps-extract proc-pairs`), so the census and the table cannot disagree on what is curated. Output is a CSV, one row per (pair, log), sorted; the committed copy is tools/proc-spells-census.csv, which `wowdps-extract gen-proc-spells --census` reads. usage: tools/census-proc-spells.sh [-o out.csv] <WoWCombatLog-*.txt>...

## Source

- Script: [`tools/census-proc-spells.sh`](../../../tools/census-proc-spells.sh)
- Extraction rules: [`tools/extract`](../crates/extract.md) (the `wowdps-extract` crate). Its pairs come from `wowdps-extract proc-pairs`, so the census and the table never disagree on what is curated.
- Consumer: [gen-proc-spells](gen-proc-spells.md) requires the better metric at 90% for every entry ([R26](../rulings/r26.md), step 4).
- Why two metrics: [A Talent Proc Nests Under Its Driver](../decisions/proc-under-its-driver.md).
