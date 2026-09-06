---
type: Tool
title: census-role-spells
description: 'Census of the buffs real combat logs apply to PLAYERS — the evidence behind the curated role-spell table (CONTRACT.md R18, tools/extract/src/rolegen.rs).'
resource: tools/census-role-spells.sh
tags: [tool, game-data]
status: stable
generated: { by: okflight/0.4.0, at: 2026-09-04T22:17:56-07:00 }
---

Census of the buffs real combat logs apply to PLAYERS — the evidence behind the curated role-spell table (CONTRACT.md R18, tools/extract/src/rolegen.rs). For every log given, counts each `SPELL_AURA_APPLIED` line whose target is a `Player-` guid and whose aura type is `BUFF`, keyed by (spell id, name as the log wrote it). One pass of grep + awk per log (the logs are large; never cat them). Output is a CSV, one row per (id, name), one count column per log named by the log's basename, sorted by id: the generator reads it via `wowdps-extract gen-role-spells --census` and writes the counts into crates/core/src/role_spells.expected.md, so a curated id nobody has ever seen applied to a player is visible in review. The committed copy is tools/role-spells-census.csv. usage: tools/census-role-spells.sh [-o out.csv] <WoWCombatLog-*.txt>...

## Source

- Script: [`tools/census-role-spells.sh`](../../../tools/census-role-spells.sh)
- Extraction rules: [`tools/extract`](../crates/extract.md) (the `wowdps-extract` crate).
