---
type: Fixture
title: taken.txt
description: 'Three players, every miss kind, staggered hits and a pet hit before its summon — the destination-side Taken view and per-player mitigation.'
resource: crates/core/fixtures/taken.txt
tags: [fixture]
status: stable
generated: { by: human:kris, at: 2026-09-05T17:38:40-07:00 }
---

Σ dealt to friendlies = Σ Taken + Σ stagger_ticked per segment, exactly — nothing in Taken opens or extends a segment.

## Rulings exercised

- [R17 Damage taken & mitigation](../rulings/r17.md), [R5 Pets](../rulings/r5.md).

## Gate

`crates/core/tests/taken.rs` against `taken.expected.tsv`; `check.awk` recomputes the destination-side metrics; the ignored `real_log_taken.rs` gate runs the invariant over a real log.

## Source

- Log: [`crates/core/fixtures/taken.txt`](../../../crates/core/fixtures/taken.txt)
- Format: [`FORMAT-NOTES.md`](../../../crates/core/fixtures/FORMAT-NOTES.md); parser-independent check: [`check.awk`](../../../crates/core/fixtures/check.awk) via [`verify.sh`](../../../crates/core/fixtures/verify.sh).
