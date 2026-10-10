---
type: Fixture
title: stacks.txt
description: 'A tank''s Tectonic Strike ladder 0 → 1 → 2 → 3 → refresh → 2 → 0 under Crushing Smash, a second debuff open beside it, an orphan dose opening at 4, a player-sourced Slow and a stacking buff as negative controls, a pet''s hit folding, a dodge at 3 stacks, an absorbed hit, a killing blow at its removal''s millisecond, and auras after the kill and past the trash gap landing nowhere.'
resource: crates/core/fixtures/stacks.txt
tags: [fixture]
status: stable
generated: { by: claude-code/opus-5.5, at: 2026-10-09T17:37:23-07:00 }
---

No table admits a debuff: any hostile `Debuff` on a player or pet
conditions their Taken hits by its level, so the fixture's negative controls
are the sources and aura types that must not. Every count is laid out cell
by cell in `stacks.expected.md`. Level 0 is the reader's derivation, the
unconditioned row less the cells, and never a cell of its own: under
Tectonic Strike it is 830 000 over five events, the dodge among them, with
its max unknown.

Three segments. The kill (Stacks Test Boss, 60 000 ms) holds the ladder
and every control. A trash pull after it holds an orphan `APPLIED_DOSE 2`
that opens its entry at 2 and conditions the next hit, while an `APPLIED`
written between `ENCOUNTER_END` and that pull lands nowhere. A dose 82 s
later, past the trash gap, lands nowhere either, and the hit after it opens
a third segment with an empty ledger.

## Rulings exercised

- [R21 Stacked-debuff conditioning](../rulings/r21.md): Brannoc's ladder,
  with Tectonic Strike's own hit at each new level and the max at 3; Cold
  Claws open beside it, so one Crushing Smash lands in two cells; the
  healer's orphan `APPLIED_DOSE 4`, opening at 4; the mage's Slow on the
  tank (a controlled source) and her Hellbent Commander (a Buff, doses
  3 → 2 → 1), which never enter; the death rule, a `REMOVED` and the
  700 000 killing blow at the same millisecond, the blow kept at 2.
- [R17 Damage taken & mitigation](../rulings/r17.md): a cell's amount is
  Taken's `amount + absorbed` (the Smash at 2 that a shield ate 20 000 of
  reads 380 000), and the dodge at 3 counts in the row's events and in no
  cell.
- [R5 Pets](../rulings/r5.md): the Water Elemental, under Tectonic Strike
  at 1, takes a Smash that folds onto the mage as `(Tectonic, Smash, 1)`.
- [R4 Segment boundaries](../rulings/r4.md): auras in the dead zone after
  the kill and past the trash gap land nowhere; the ledger is
  segment-local.

## Gate

`crates/core/tests/stacks.rs` holds the per-cell table through
`stack_cells` and `stacking_debuffs`, the ledger's transitions in
isolation, the cap, lazy = full = checkpoint-resume parity and the R10
merge, and the bound it holds eight other fixtures to as well (Σ a cell
group's hits and sum ≤ the Taken row). `tests/fixture_totals.rs` checks
the meter against `stacks.expected.tsv`, where `check.awk`'s own level
machine writes `stack_hits` / `stack_sum` / `stack_max` / `stack_cells` /
`stack_auras` per player beside the metrics every gated fixture carries.
The ignored `real_log_stacks.rs` gate runs the bound over a real log.

## Source

- Log: [`crates/core/fixtures/stacks.txt`](../../../crates/core/fixtures/stacks.txt)
- Expected: [`stacks.expected.md`](../../../crates/core/fixtures/stacks.expected.md), [`stacks.expected.tsv`](../../../crates/core/fixtures/stacks.expected.tsv)
- Format: [`FORMAT-NOTES.md`](../../../crates/core/fixtures/FORMAT-NOTES.md); parser-independent check: [`check.awk`](../../../crates/core/fixtures/check.awk) via [`verify.sh`](../../../crates/core/fixtures/verify.sh).
