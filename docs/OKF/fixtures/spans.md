---
type: Fixture
title: spans.txt
description: 'Aura spans with caster and target — Shield Block refreshed with no apply and open at the kill, overlapping Shield Wall and Pain Suppression for the union, externals, Time Warp, support buffs, a trinket proc, and a pre-pull aura in the dead zone.'
resource: crates/core/fixtures/spans.txt
tags: [fixture]
status: stable
generated: { by: human:kris, at: 2026-09-05T17:38:40-07:00 }
---

The admission table is the generated `role_spells.rs` ([gen-role-spells](../tools/gen-role-spells.md)); an open role span closes at read time, and a pre-pull aura in the trash dead zone lands nowhere.

## Rulings exercised

- [R18 Aura spans with caster & target](../rulings/r18.md), [R12 Timelines & markers](../rulings/r12.md) (proved untouched).

## Gate

`crates/core/tests/spans.rs` against `spans.expected.tsv`; `check.awk` computes the active-mitigation union as a per-second bitmap; the ignored `real_log_spans.rs` gate.

## Source

- Log: [`crates/core/fixtures/spans.txt`](../../../crates/core/fixtures/spans.txt)
- Format: [`FORMAT-NOTES.md`](../../../crates/core/fixtures/FORMAT-NOTES.md); parser-independent check: [`check.awk`](../../../crates/core/fixtures/check.awk) via [`verify.sh`](../../../crates/core/fixtures/verify.sh).
