---
type: Decision
title: A Hit A Shield Took Whole Is A Hit
description: 'A `*_MISSED` ABSORB — a hit the target''s shield took whole — counts as the attacker''s damage (a player''s or an NPC''s) and the victim''s Taken at amount 0 + absorbed, as a partial absorb''s absorbed part always did, because otherwise an enemy''s shield ate the hit from the attacker''s row while the game still shared it; it stays passive, so segmentation never moves.'
tags: [meter, ruling]
status: stable
generated: { by: claude-code/opus-5.5, at: 2026-09-30T17:30:00-07:00 }
sources:
  - id: contract
    resource: ../../../CONTRACT.md
    title: CONTRACT.md — R1, R17, R19 (METER) and R26 (misses), amended together
  - id: r1
    resource: ../rulings/r1.md
    title: R1 Damage — the convention this extends
  - id: r17
    resource: ../rulings/r17.md
    title: R17 Damage taken & mitigation — the identity and the percentage it keeps
---

**Where:** [`wowdps-core`](../crates/core.md) (`Meter::file_hit`, the one
filing path a damage line and a whole absorb share), [R1](../rulings/r1.md),
[R17](../rulings/r17.md), [R19](../rulings/r19.md), [R26](../rulings/r26.md);
gated by [taken.txt](../fixtures/taken.md) and
[support.txt](../fixtures/support.md).

## Context

[R1](../rulings/r1.md) always counted a hit's absorbed part as damage done
(`amount + absorbed`, the in-game meters' convention), but only when the
shield took *part* of the hit. A shield that took all of it turns the line into
`SPELL_MISSED … ABSORB,nil,amountMissed,unmitigated,critical`, and the meter
filed that as a miss: nothing on the attacker's Damage row, a count on the
victim's Taken row, and the amount as `prevented`.[^contract]

The 2026-09-23 raid log showed what that cost. The game still SHARES a hit
absorbed whole: a support line (amount 0, absorbed = the share, the real
shape) names the Evoker, so [R19](../rulings/r19.md) moved 336 746 of shares
from attackers' `effective` onto the supporter's, against damage that no row
held. The same accounting hid boss output the user wants measured next: every
boss hit a Power Word: Shield or a tank's barrier ate whole was on no row at
all.

## Decision

A whole absorb is a hit: amount 0, absorbed = its `amountMissed`, filed for
ANY attacker, player or NPC, wherever a partial absorb's absorbed part is.
The damage arm's filing moved into `Meter::file_hit`, and the ABSORB miss
calls it with `whole` set, so the two cannot drift:

- **Attacker:** the Damage row (a hit, its crit from the tail, its periodic
  part on a `SPELL_PERIODIC_MISSED`, R12's series), R22's self-harm when it hit
  itself, R24's enemy row. Not an [R26](../rulings/r26.md) miss any more.
- **Victim ([R17](../rulings/r17.md)):** the Taken row at the amount, with
  the amount as `extra`, the taken series and R21's stack cells, and still a
  miss of its kind with the amount in `absorbed_full`.
- **Segmentation:** untouched. The index scanner ignores `*_MISSED`, so the
  hit goes through the passive gate (`open_segment_for_passive`) like every
  other miss: it never opens, extends or splits a segment, and one before a
  pull's first hit or past the trash gap lands nowhere, full = lazy. A damage
  line's other side effects (R8 inference, the R9 recap, R11, R13, R23's proof
  of life, trash naming) do not follow it.

Counting the absorb in Taken is what keeps R17's identity (Σ dealt to
friendlies + self-harm = Σ Taken + stagger ticked) exact: both sides move
together.[^r17] With one named exception: a `*_MISSED` line carries no
advanced block, so it cannot teach the meter a pet's owner. A pet that did
nothing else all pull takes its whole absorbs on its own record, which no row
lists — nobody's, as [R4](../rulings/r4.md) has every unowned pet — and the
real-log gate counts that apart (53 120 on two idle pets in one 2026-09-23
pull, 0 on the other logs). It forced one redefinition. `prevented` is now the full blocks
alone — the only amount a miss carries that never becomes Taken — so
`mitigated_pct` = mitigated / (taken + prevented) is the same number it was:
the whole absorb just moved from the denominator's `prevented` half into its
`taken` half. The display words the Taken row's `extra` as "absorbed", every
absorb, through `Mitigation::absorbs`.

## Consequences

Damage rows move up in any fight with shielded enemies, and Taken and dtps
move up for anyone who wore a shield that ate whole hits. A tank's
`mitigated_pct` does not move. Stored cards change only when a regrade
rewrites them; until then a card's percentage is still right, because its
`taken + prevented` is the same sum, while the rows-tier SQL view, which
recomputes from `absorbed_full`, reads an old row's percentage high. No wire
change: `PROTO_VERSION` stands.

The fixtures carry both directions, recomputed independently by
`check.awk`'s `absorbed_whole()`: in `taken.txt` the boss's hits that
Zenlí's Celestial Brew and Pyralis's Ice Barrier took whole are now 3 000 and
21 000 of their Taken, with `prevented` 0; in `support.txt` a Fireball the
boss's shield took whole (a crit) is 20 000 of Ignatia's damage, and its
Ebon Might share of 200 is read as amount 0 + absorbed, with the partition
exact. The real-log taken gate collects attackers from whole absorbs too.[^r1]

Richer NPC output (bosses, their pets and minions) and attributing prevention
to the shield or defensive that did it are the next step, not this one.

[^contract]: CONTRACT.md — R1, R17, R19 (METER) and R26 (misses), amended together
[^r1]: R1 Damage — the convention this extends
[^r17]: R17 Damage taken & mitigation — the identity and the percentage it keeps
