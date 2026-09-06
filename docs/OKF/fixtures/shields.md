---
type: Fixture
title: shields.txt
description: 'A Discipline Priest''s Power Word: Shield ledger in every state — partial waste, running-total refreshes up and down, re-apply while open, an over-absorb, a pre-pull shield seen only by its absorb, one open at the kill — plus Ice Barrier, Blood Shield, an excluded stagger and a non-shield trailer.'
resource: crates/core/fixtures/shields.txt
tags: [fixture]
status: stable
generated: { by: human:kris, at: 2026-09-05T17:38:40-07:00 }
---

The admission table is the generated `absorb_spells.rs` ([gen-absorb-spells](../tools/gen-absorb-spells.md)). Invariants: Σ `shields().consumed` = `absorbed_healing`, `applied = consumed + wasted` on every known row, and `absorb_wasted` is None — never a silent 0 — when no waste was known.

## Rulings exercised

- [R20 Shield ledger & absorb efficiency](../rulings/r20.md), [R2 Healing](../rulings/r2.md), [R3 Absorb attribution](../rulings/r3.md).

## Gate

`crates/core/tests/shields.rs` against `shields.expected.tsv`; `check.awk` runs the same per-key state machine and fails on a `remaining ≠ REMOVED trailer` mismatch; the ignored `real_log_shields.rs` gate.

## Source

- Log: [`crates/core/fixtures/shields.txt`](../../../crates/core/fixtures/shields.txt)
- Format: [`FORMAT-NOTES.md`](../../../crates/core/fixtures/FORMAT-NOTES.md); parser-independent check: [`check.awk`](../../../crates/core/fixtures/check.awk) via [`verify.sh`](../../../crates/core/fixtures/verify.sh).
