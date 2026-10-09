---
type: Fixture
title: taken.txt
description: 'Three players, every miss kind, staggered hits, a pet hit before its summon and a guardian that staggers itself — the destination-side Taken view, per-player mitigation and the R22 self-harm split — plus a trash pull built for the combat clock and friendly fire.'
resource: crates/core/fixtures/taken.txt
tags: [fixture]
status: stable
generated: { by: human:kris, at: 2026-09-05T17:38:40-07:00 }
---

Σ dealt to friendlies + Σ `self_harm` on friendlies + Σ `friendly_fire` = Σ Taken
+ Σ `stagger_ticked` per segment, exactly — nothing in Taken opens or extends a segment, and nothing in
[R22](../rulings/r22.md)'s split changes which segments exist. Zenlí carries both halves of that split: his own
two Stagger ticks (10 000, which Taken also sees as `stagger_ticked`) and
Niuzao's one (2 500, which it does not — the guardian is summoned as a
`Creature-` unit, outside R17's destination universe, so it pins `self_harm`
12 500 apart from the `on_friendly` 10 000 the identity uses). The ox's Stomp
on the boss stays ordinary pet damage, 6 000 of it.

Two of the boss's hits were absorbed whole — a Smoldering tick by Zenlí's
Celestial Brew, an Ember Bolt by Pyralis's Ice Barrier. Since
[R1](../rulings/r1.md) counts a hit a shield took whole, they are 3 000 and
21 000 of the victims' Taken (and `absorbed`, still counted as misses), their
`prevented` is 0, and the boss's Damage `by_target` carries them — so the
identity above holds with them on both sides, and `mitigated_pct` reads what
it read before ([A Hit A Shield Took Whole Is A Hit](../decisions/whole-absorb-is-a-hit.md)).

Since 2026-10-08 six hits carry a real `unmitigated` amount (and their
`_LANDED` twins, which nothing reads): an armored, partly blocked swing on
Durgan, his partly absorbed Cinder Lash, Zenlí's staggered swing and his
whole-absorbed Smoldering tick, the Water Elemental's hit folding onto
Pyralis, a boar's swing in the trash after — and an Ember Spit a
vulnerability lifted above its unmitigated amount, which counts 0, not a
negative ([Armor Is Mitigation](../decisions/armor-is-mitigation.md)):
`reduced` 33 000 / 13 500 / 1 000 in the pull, 500 in the trash.

## Rulings exercised

- [R17 Damage taken & mitigation](../rulings/r17.md), [R1 Damage](../rulings/r1.md) (a hit absorbed whole), [R5 Pets](../rulings/r5.md),
  [R22 Self-harm](../rulings/r22.md), [R24 Enemy damage taken](../rulings/r24.md) —
  the boss's enemy row is exactly what the three players and the ox dealt it
  (306 000, Niuzao's Stomp included and, since [R1](../rulings/r1.md) leaves it
  out, the killing Fire Blast's 25 000 overkill not), and Niuzao — ours, though a `Creature-`
  guid — never earns one for its own tick.

## The third segment (2026-10-02)

A 45 s boar pull in Dornogal built for
[Count And Clock The Way The Game's Own Meter Does](../decisions/match-the-game-meter.md):

- **The combat clock** ([R7](../rulings/r7.md)) — three engagement lines at
  0, 5 and 45 s with a 40 s walk between the last two (a Riptide tick in it,
  which keeps the segment open but never runs the clock): `combat_ms` 5 000
  of a 45 000 ms segment, so Durgan's `dps` is 70 000 / 5 s.
- **Friendly fire** ([R22](../rulings/r22.md)) — a Restoration Shaman's
  Spirit Link Totem hits Durgan for 40 000 by the flags (`0x2114` on `0x511`)
  and Pyralis for 8 000 from a neutral NPC's `0xa28` (the summoned arm):
  `friendly_fire` 48 000, no Damage row, both still in the victims' Taken.
  Its heal stays healing.
- **Overkill** ([R1](../rulings/r1.md)) — the boar's killing swing counts
  20 000 of its 25 000.

## Gate

`crates/core/tests/taken.rs` against `taken.expected.tsv`; `check.awk` recomputes the destination-side metrics; the ignored `real_log_taken.rs` gate runs the invariant over a real log.

## Source

- Log: [`crates/core/fixtures/taken.txt`](../../../crates/core/fixtures/taken.txt)
- Format: [`FORMAT-NOTES.md`](../../../crates/core/fixtures/FORMAT-NOTES.md); parser-independent check: [`check.awk`](../../../crates/core/fixtures/check.awk) via [`verify.sh`](../../../crates/core/fixtures/verify.sh).
