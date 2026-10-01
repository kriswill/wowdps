---
type: Fixture
title: support.txt
description: 'An Augmentation Evoker buffing a Mage, a Warrior and a pet, plus a Holy Priest with shields, overheal, a self-heal and an NPC-sourced heal — support attribution and the healing split.'
resource: crates/core/fixtures/support.txt
tags: [fixture]
status: stable
generated: { by: human:kris, at: 2026-09-05T17:38:40-07:00 }
---

Includes shares, a self-supported proc the log writes twice and a melee support line. It also has Earthen Ward, a friendly NPC nobody owns, hit the boss with an Ebon Might share of it (22:05:12.5): the share lands on nobody — not the Evoker's `given`, not a `received` — so Σ effective = Σ damage still holds ([R19](../rulings/r19.md), 2026-09-30). Right after it Ignatia's Fireball is one the boss's shield took whole — `SPELL_MISSED … ABSORB,nil,20000,20000,1,ST` — with an Ebon Might share logged as amount 0 + absorbed 200, the shape a real log writes: [R1](../rulings/r1.md) counts the hit as 20 000 of her damage (a crit, not a miss), so the share nets against damage on her row (see [A Hit A Shield Took Whole Is A Hit](../decisions/whole-absorb-is-a-hit.md)). `Segment::effective` = damage − received + given is derived, never stored, so Σ effective = Σ damage.

## Rulings exercised

- [R19 Support attribution](../rulings/r19.md), [R2 Healing](../rulings/r2.md) (its amendment), [R1 Damage](../rulings/r1.md) (a hit absorbed whole).

## Gate

`crates/core/tests/support.rs` against `support.expected.tsv`; `check.awk`'s seven support/healing metrics; the ignored `real_log_support.rs` gate.

## Source

- Log: [`crates/core/fixtures/support.txt`](../../../crates/core/fixtures/support.txt)
- Format: [`FORMAT-NOTES.md`](../../../crates/core/fixtures/FORMAT-NOTES.md); parser-independent check: [`check.awk`](../../../crates/core/fixtures/check.awk) via [`verify.sh`](../../../crates/core/fixtures/verify.sh).
