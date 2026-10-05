---
type: Fixture
title: relog.txt
description: 'Two mid-log COMBAT_LOG_VERSION boundaries — the pet-owner map must reset at each, and the open visit suspends.'
resource: crates/core/fixtures/relog.txt
tags: [fixture]
status: stable
generated: { by: human:kris, at: 2026-09-05T17:38:40-07:00 }
---

A pet summoned before a relog must not stay attributed after it; the expected TSV pins the split.

## Rulings exercised

- [R6 Version seam](../rulings/r6.md), [R5 Pets](../rulings/r5.md).

## Gate

`relog_boundary_resets_pet_ownership` in `crates/core/tests/fixture_totals.rs` against `relog.expected.tsv`; also in the taken and spans fixture lists. `verify.sh` with no arguments recomputes its goldens with `check.awk` too (as CI runs it) — it was missing from that loop until 2026-10-04, so its golden lagged six rounds of new metrics while `cargo test`, which compares only the metrics a golden lists, stayed green.

## Source

- Log: [`crates/core/fixtures/relog.txt`](../../../crates/core/fixtures/relog.txt)
- Format: [`FORMAT-NOTES.md`](../../../crates/core/fixtures/FORMAT-NOTES.md); parser-independent check: [`check.awk`](../../../crates/core/fixtures/check.awk) via [`verify.sh`](../../../crates/core/fixtures/verify.sh).
