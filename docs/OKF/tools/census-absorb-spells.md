---
type: Tool
title: census-absorb-spells
description: 'Census of the shields real combat logs ABSORB with — the evidence beside the discovered absorb-spell table (CONTRACT.md R20, tools/extract/src/absorbgen.rs).'
resource: tools/census-absorb-spells.sh
tags: [tool, game-data]
status: stable
generated: { by: okflight/0.4.0, at: 2026-09-05T00:53:27-07:00 }
---

Census of the shields real combat logs ABSORB with — the evidence beside the discovered absorb-spell table (CONTRACT.md R20, tools/extract/src/absorbgen.rs). For every log given, counts each `SPELL_ABSORBED` line by the absorb spell it names — (spell id, name as the log wrote it). One pass of grep + awk per log (the logs are large; never cat them). Output is a CSV, one row per (id, name), one count column per log named by the log's basename, sorted by id: the generator reads it via `wowdps-extract gen-absorb-spells --census` and writes the counts into crates/core/src/absorb_spells.expected.md, listing every census id the client tables do NOT mark as SCHOOL_ABSORB — the shields the ledger can only count as unknown-applied. The committed copy is tools/absorb-spells-census.csv. Twin of tools/census-role-spells.sh. usage: tools/census-absorb-spells.sh [-o out.csv] <WoWCombatLog-*.txt>...

## Source

- Script: [`tools/census-absorb-spells.sh`](../../../tools/census-absorb-spells.sh)
- Extraction rules: [`tools/extract`](../crates/extract.md) (the `wowdps-extract` crate).
