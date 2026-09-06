---
type: Fixture
title: corrupt.txt
description: 'The negative control — a deliberately damaged log that the parser-independent check must FAIL on.'
resource: crates/core/fixtures/corrupt.txt
tags: [fixture]
status: stable
generated: { by: human:kris, at: 2026-09-05T17:38:40-07:00 }
---

A copy of `sample.txt` with corrupted events; the fixture's job is to make sure the verification pipeline is not vacuous.

## Rulings exercised

- Every ruling, negatively: proves `check.awk` and the tests actually detect divergence.

## Gate

CI runs `verify.sh crates/core/fixtures/corrupt.txt` and asserts a non-zero exit.

## Source

- Log: [`crates/core/fixtures/corrupt.txt`](../../../crates/core/fixtures/corrupt.txt)
- Format: [`FORMAT-NOTES.md`](../../../crates/core/fixtures/FORMAT-NOTES.md); parser-independent check: [`check.awk`](../../../crates/core/fixtures/check.awk) via [`verify.sh`](../../../crates/core/fixtures/verify.sh).
