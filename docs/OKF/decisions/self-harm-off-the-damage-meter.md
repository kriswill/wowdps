---
type: Decision
title: Keep Self-Harm Off The Damage Meter
description: 'Damage an actor deals to itself (own pets folded) is tallied as `self_harm` instead of landing on its Damage row, because a Brewmaster''s Stagger ticks were 21% of his "damage" against every in-game meter''s 0%.'
tags: [meter, ruling]
status: stable
generated: { by: claude-code/opus-5, at: 2026-09-06T19:05:00-07:00 }
sources:
  - id: r22
    resource: ../rulings/r22.md
    title: R22 Self-harm — the binding call
  - id: r17
    resource: ../rulings/r17.md
    title: R17 Damage taken & mitigation — the identity this restates
---

**Where:** [`wowdps-core`](../crates/core.md), [R22](../rulings/r22.md).

## Context

A live +14 Voidscar Arena run had the group's Brewmaster fourth at 89.4M while
two in-game meters watching the same fight showed 66.7M. The engine was not
wrong about any event: 16.4M was the monk's own Stagger ticks and 2.3M was his
Niuzao's, both self-sourced damage that [R1](../rulings/r1.md) had always
counted as damage done. Every other player agreed with the in-game meters to
within rounding, so the gap was one ruling, not a parsing bug.

The tension was real, not sloppiness: [R17](../rulings/r17.md) deliberately
counts a stagger tick as damage dealt so that its identity — Σ dealt to
friendlies = Σ Taken + Σ `stagger_ticked` — closes exactly.[^r17]

## Decision

A damage event whose FOLDED source equals its folded destination is recorded
only as `Segment::self_harm`, never on a Damage row, drill, by-target,
timeline or DPS.[^r22] Three things made this cheap:

- The fold (own summons included) is what makes Niuzao's own ticks the monk's
  self-harm, matching how every other pet number reaches its owner. It walks
  a SUMMON-only map, not the ownership map R4 folds with: that one is also
  written from the advanced block's ownerGUID, which a mind-controlled mob
  carries, and folding through it would silently drop a Priest's damage to a
  mob they charmed — onto neither side of any identity. Found in review.
- `ensure_combat` still runs on these lines. The index scanner counts damage
  lines structurally without reading amounts, so skipping one would desync
  scanner from meter and break lazy/full parity — segmentation is untouched.
- The identity restates rather than breaks: each side now names the tally
  holding what it excludes, `self_harm` on the dealt side and
  `stagger_ticked` on the taken side.

The Taken side was left alone. Non-stagger self-harm (Shadow Word: Death and
friends) was already on the victim's Taken row with themselves as the
attacker, and adding stagger ticks there would count the staggered hit twice.
The mitigation line simply started printing `stagger ticked`, a field already
on the wire.

## Consequences

No wire or store change: `self_harm` is on neither `Row` nor `Mitigation`, so
`PROTO_VERSION` stands and old cards read correctly. Damage rows, DPS, shares
and `effective` all move for self-harming specs — every Brewmaster total in
the history lake is now ~20% lower and closer to what the group saw. The
fixtures carry both halves of the fold (`taken.txt`'s Niuzao, `shields.txt`'s
Brewmoon) and `check.awk` recomputes the split independently.

[^r22]: R22 Self-harm — the binding call.
[^r17]: R17 Damage taken & mitigation — the identity this restates.
