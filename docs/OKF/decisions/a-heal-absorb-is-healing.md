---
type: Decision
title: A Heal-Absorb's Part Is Healing, Shown Apart
description: 'R2 keeps the part of a heal a heal-absorb ate (the heal line''s `absorbed`) inside healing done, as Warcraft Logs counts it, and surfaces it as `heal_absorbed` — a third half of the healing split, capped at what the line healed — on the Healing rows, the card, SQL, the MCP and the window''s inspector.'
tags: [meter, ruling, proto, history, gui]
status: stable
generated: { by: claude-code/opus-5.5, at: 2026-10-09T10:30:00-07:00 }
sources:
  - id: contract
    resource: ../../../CONTRACT.md
    title: CONTRACT.md — R2's v44 amendment and the v44 wire row
  - id: format
    resource: ../../../crates/core/fixtures/FORMAT-NOTES.md
    title: FORMAT-NOTES — the heal suffix's offset 34, corrected
  - id: expected
    resource: ../../../crates/core/fixtures/support.expected.md
    title: support.expected.md — the two eaten heals, line by line
---

**Where:** [`wowdps-core`](../crates/core.md) (the meter's heal arm, the
per-healer `heal_absorbed` map), [`wowdps-model`](../crates/model.md)
(`Row::heal_absorbed`), [`wowdps-proto`](../crates/proto.md) (the Row and
`CardPlayer` wire fields, the store's JSON), [`wowdps-history`](../crates/history.md)
(`players.heal_absorbed`), [`wowdps-mcp`](../crates/mcp.md),
[`wowdps-gui-logic`](../crates/gui-logic.md) (the inspector's "Heal absorbed"),
[R2](../rulings/r2.md); gated by [support.txt](../fixtures/support.md).

## Context

A heal line ends `amount, overheal, absorbed, critical`, and the meter read
`absorbed` and dropped it; FORMAT-NOTES called it "absorbed to shield".[^format]
On a real raid night it is the part of the heal a heal-absorb took — a
boss's heal-absorb debuff, Light of the Martyr's drawback, Death Pact — 6 201
of 169 947 heal lines in a 300 MB slice, 1.5 % of a whole night's healing.
Two readings were open: subtract it (the target gained no health from it) or
count it (the heal had to land to clear the absorb). The user decided:
count it, as Warcraft Logs does, and show it.

## Decision

The Healing row's amount stays `amount − overheal`, the eaten part
included, so no total moves. Per healer per spell name (raw source guid,
folded at read like R26's casts, so a totem's is its Shaman's) the meter
keeps `heal_absorbed` += `min(absorbed, amount − overheal)` in the segment
`record` just chose.[^contract] The cap is load-bearing: 56 of those 6 201
lines log more eaten than they healed — every one a Shadow Priest's
self-heal at amount 0 — and capping keeps `heal_absorbed ≤ healing` true line
by line, the identity every fixture and every real segment is held to. It
rides `Row.heal_absorbed` on the Healing meter row and each Healing
by-ability row (0 elsewhere and on rows the per-second series build — a
zoom window's, a comparison's — since the series keep no such split), the
card's player line beside `overheal` and `absorbed`, every rows-tier row,
SQL's `players.heal_absorbed` (probed, 0 on an older card), the MCP's
healing rows and graded rows, and the inspector's numbers for a healer, next
to Overheal, when anything was eaten. `SPELL_HEAL_ABSORBED`, the absorb's
own line, stays `Other` — a follow-up, not in scope.

## Consequences

A coach can say a healer's numbers were inflated (or carried) by a fight's
heal-absorb without the meter disagreeing with the game. `healed_received`
still counts the eaten part as received. Stored cards and rows gain the
field only when regraded. The Row wire shape grew eight bytes under
PROTO_VERSION 44, with [R28](../rulings/r28.md)'s power series and R26's
empowered stages ([A Pool, Second by Second](a-pool-second-by-second.md)).
The goldens are line by line in support.expected.md.[^expected]

[^contract]: CONTRACT.md — R2's v44 amendment and the v44 wire row
[^format]: FORMAT-NOTES — the heal suffix's offset 34, corrected
[^expected]: support.expected.md — the two eaten heals, line by line
