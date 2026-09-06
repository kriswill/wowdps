---
type: Fixture
title: sample.txt
description: 'The canonical synthetic advanced-format log — two encounters and trash inside one raid visit, three players and a pet, every modeled event type — with hand-computed golden totals.'
resource: crates/core/fixtures/sample.txt
tags: [fixture]
status: stable
generated: { by: human:kris, at: 2026-09-05T17:38:40-07:00 }
---

Goldens live in `sample.expected.md` (worked by hand) and `sample.expected.tsv` (machine-checked). Lazy segment loading (`index.rs`) must match a full replay of this file exactly — the lazy/full parity gate runs over it.

## Rulings exercised

- [R1 Damage](../rulings/r1.md), [R2 Healing](../rulings/r2.md), [R3 Absorb attribution](../rulings/r3.md), [R4 Segment boundaries](../rulings/r4.md), [R5 Pets](../rulings/r5.md), [R7 Duration](../rulings/r7.md), [R9 Deaths & recap](../rulings/r9.md), [R10 Visits & Overall](../rulings/r10.md).

## Gate

`crates/core/tests/fixture_totals.rs` against `sample.expected.tsv`; `verify.sh` recomputes the goldens with gawk independently of the parser. The same TSV also carries the R17 `taken` … `stagger_ticked` rows.

## Source

- Log: [`crates/core/fixtures/sample.txt`](../../../crates/core/fixtures/sample.txt)
- Format: [`FORMAT-NOTES.md`](../../../crates/core/fixtures/FORMAT-NOTES.md); parser-independent check: [`check.awk`](../../../crates/core/fixtures/check.awk) via [`verify.sh`](../../../crates/core/fixtures/verify.sh).
