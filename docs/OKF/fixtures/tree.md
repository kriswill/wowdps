---
type: Fixture
title: tree.txt
description: 'The ability tree''s fixture — a Destruction Warlock''s Sayaad and Infernal summoned mid-pull, Wither''s hit and tick under two ids, two trinkets'' procs, and every cast the passive gate turns away; a Priest''s one-id Shadow Word: Pain hit and ticks and Renew''s instant heal and ticks.'
resource: crates/core/fixtures/tree.txt
tags: [fixture]
status: stable
generated: { by: claude-code/opus-5.5, at: 2026-09-29T19:30:00-07:00 }
---

Real spell ids throughout (SpellName in 12.1.0): both Wither ids are named
"Wither", and 1232797 / 1232802 are both "Araz's Ritual Forge", so one row
holds two parts of each shape — two ids under one name, and one id (589,
Shadow Word: Pain; 139, Renew) landing both directly and as a tick. The
trinket spells resolve through the generated item table
([gen-item-spells](../tools/gen-item-spells.md)) to items 242402 and 242394.
The per-row structure is laid out by hand in `tree.expected.md`; the TSV holds
the per-player totals `check.awk` computes.

## Rulings exercised

- [R26 Ability tree](../rulings/r26.md): groups (two summons, two trinkets),
  parts, casts per row, a cast with no row under it (Summon Infernal).
- [R5 Pets](../rulings/r5.md): the Sayaad and the Infernal fold onto Vexxa.
- [R4 Segment boundaries](../rulings/r4.md) and the passive gate: a precast
  before ENCOUNTER_START, a cast after the kill, one before the trash's first
  hit and one 88.5 s after its last — none counted — and a hostile unit's cast.
  Step 3: a dodged swing and an immune Chaos Bolt (two misses), Wither on
  the boss and an add at once (one union, 24.8 s), Shadow Word: Pain still up
  at the kill (closed there), and a Wither after the kill that lands nowhere.
  Step 4: a Blackened Soul hit on the boss while Wither is up — the curated
  proc and its driver form one `Spell` group labelled "Wither"
  ([A Talent Proc Nests Under Its Driver](../decisions/proc-under-its-driver.md)).

## Gate

`crates/core/tests/fixture_totals.rs` against `tree.expected.tsv` (the R26
metrics — `casts`, `damage_periodic`, `heal_periodic`, `misses_dealt`,
`dot_uptime_ms` — are emitted for every fixture's players, so every golden
carries them) and
`crates/core/tests/tree.rs` for the structure; the mcp's
`a_drill_s_abilities_carry_the_ability_tree` reads it through a real daemon.

## Source

- Log: [`crates/core/fixtures/tree.txt`](../../../crates/core/fixtures/tree.txt)
- Hand-worked values: [`tree.expected.md`](../../../crates/core/fixtures/tree.expected.md)
- Parser-independent check: [`check.awk`](../../../crates/core/fixtures/check.awk) via [`verify.sh`](../../../crates/core/fixtures/verify.sh).
