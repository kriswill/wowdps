---
type: Fixture
title: taken.txt
description: 'Three players, every miss kind, staggered hits, a pet hit before its summon and a guardian that staggers itself — the destination-side Taken view, per-player mitigation and the R22 self-harm split.'
resource: crates/core/fixtures/taken.txt
tags: [fixture]
status: stable
generated: { by: human:kris, at: 2026-09-05T17:38:40-07:00 }
---

Σ dealt to friendlies + Σ `self_harm` = Σ Taken + Σ `stagger_ticked` per segment,
exactly — nothing in Taken opens or extends a segment, and nothing in
[R22](../rulings/r22.md)'s split changes which segments exist. Zenlí carries both halves of that split: his own
two Stagger ticks (10 000, which Taken also sees as `stagger_ticked`) and
Niuzao's one (2 500, which it does not — the guardian is summoned as a
`Creature-` unit, outside R17's destination universe, so it pins `self_harm`
12 500 apart from the `on_friendly` 10 000 the identity uses). The ox's Stomp
on the boss stays ordinary pet damage, 6 000 of it.

## Rulings exercised

- [R17 Damage taken & mitigation](../rulings/r17.md), [R5 Pets](../rulings/r5.md),
  [R22 Self-harm](../rulings/r22.md).

## Gate

`crates/core/tests/taken.rs` against `taken.expected.tsv`; `check.awk` recomputes the destination-side metrics; the ignored `real_log_taken.rs` gate runs the invariant over a real log.

## Source

- Log: [`crates/core/fixtures/taken.txt`](../../../crates/core/fixtures/taken.txt)
- Format: [`FORMAT-NOTES.md`](../../../crates/core/fixtures/FORMAT-NOTES.md); parser-independent check: [`check.awk`](../../../crates/core/fixtures/check.awk) via [`verify.sh`](../../../crates/core/fixtures/verify.sh).
