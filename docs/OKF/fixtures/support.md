---
type: Fixture
title: support.txt
description: 'An Augmentation Evoker buffing a Mage, a Warrior and a pet, plus a Holy Priest with shields, overheal, a self-heal and an NPC-sourced heal — support attribution and the healing split.'
resource: crates/core/fixtures/support.txt
tags: [fixture]
status: stable
generated: { by: human:kris, at: 2026-09-05T17:38:40-07:00 }
---

Includes shares, a self-supported proc the log writes twice and a melee support line. `Segment::effective` = damage − received + given is derived, never stored, so Σ effective = Σ damage.

## Rulings exercised

- [R19 Support attribution](../rulings/r19.md), [R2 Healing](../rulings/r2.md) (its amendment).

## Gate

`crates/core/tests/support.rs` against `support.expected.tsv`; `check.awk`'s seven support/healing metrics; the ignored `real_log_support.rs` gate.

## Source

- Log: [`crates/core/fixtures/support.txt`](../../../crates/core/fixtures/support.txt)
- Format: [`FORMAT-NOTES.md`](../../../crates/core/fixtures/FORMAT-NOTES.md); parser-independent check: [`check.awk`](../../../crates/core/fixtures/check.awk) via [`verify.sh`](../../../crates/core/fixtures/verify.sh).
