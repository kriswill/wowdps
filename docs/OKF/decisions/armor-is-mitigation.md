---
type: Decision
title: Armor Is Mitigation
description: 'R17''s record gains `reduced` — each Taken hit''s `unmitigated` amount (the damage suffix''s second, which the parser now reads) less `amount + absorbed + blocked`, floored at 0 — inside `mitigated` and the swung total, because a tank''s mitigated_pct counted shields and blocks and none of the armor and damage reduction that took most of a boss''s melee; one PROTO_VERSION bump (v43) for the 2026-10-08 parser series.'
tags: [meter, ruling, proto, history]
status: stable
generated: { by: claude-code/opus-5.5, at: 2026-10-08T23:00:00-07:00 }
sources:
  - id: contract
    resource: ../../../CONTRACT.md
    title: CONTRACT.md — R17's 2026-10-08 amendment and the v43 wire row
  - id: format
    resource: ../../../crates/core/fixtures/FORMAT-NOTES.md
    title: FORMAT-NOTES — the damage suffix's `unmitigated`, corrected
  - id: expected
    resource: ../../../crates/core/fixtures/taken.expected.md
    title: taken.expected.md — the line-by-line `reduced` table
---

**Where:** [`wowdps-core`](../crates/core.md) (the parser's `DamageSuffix`,
`Meter::file_hit`'s R17 half), [`wowdps-model`](../crates/model.md)
(`Mitigation::reduced`, the one `mitigated_pct`), [`wowdps-proto`](../crates/proto.md)
(the 96-byte record, `CardPlayer::reduced`), [`wowdps-history`](../crates/history.md)
(the probed `reduced` columns), [R17](../rulings/r17.md); gated by
[taken.txt](../fixtures/taken.md).

## Context

Every damage line ends `amount, unmitigated, overkill, school, resisted,
blocked, absorbed, critical, …`. The parser read the first and dropped the
second, which FORMAT-NOTES called "pre-mitigation, diagnostics only".[^format]
So [R17](../rulings/r17.md)'s `mitigated` was partial absorbs, blocks and full
absorbs and blocks — what shields and a shield arm did — and none of what
armor and damage reduction took off before anything landed. On a real Heroic
pull the boss's melee on the tanks was 121 M unmitigated and 34 M taken, and
hostile spells on players 1.42 B and 1.07 B: a tank's `mitigated_pct` mostly
described his healers' shields.

## Decision

`Event::Damage` and `Event::Missed` carry `unmitigated` (an ABSORB miss's
fourth tail field; 0 on a miss kind that has none). On every hit R17 files as
Taken, the victim's record adds `unmitigated − (amount + absorbed + blocked)`
over the log's raw amount (post-block, overkill in), floored at 0, as
`reduced`.[^contract] The floor is deliberate: a vulnerability debuff lifts a
hit above its unmitigated amount (20 of 19 241 hostile hits on players on
that pull, 1.1 M in all; on the group's hits on enemies it is the norm, which
is why the field is no use as "pre-mitigation" damage done). `reduced` is
never Taken; it joins `mitigated` and the swung total, so `mitigated_pct` =
mitigated / (taken + prevented + reduced), and `prevented` keeps meaning the
full blocks.

The record grows a trailing u64 on the wire (88 → 96 bytes) and so does the
card's player line, under one bump, v43, that the rest of the series rides.
The history store writes `reduced` on the card and the rows tier's record; a
file without it reads 0, and since such a card's `mitigated` lacks it too its
pct is the one it always had until a regrade rewrites it. DuckDB probes the
field before naming it, as it does every later card field, and the parity
gate holds the mitigation view's pct, the card's and the SQL recomputation
to the model's one function.

## Consequences

A tank's pct now reads like the game's own idea of mitigation (Warcraft Logs'
`mitigated` is the same subtraction); a Taken row, the R17 identity and every
damage number are untouched. `check.awk` computes `reduced` from the log
grammar alone and emits it for every fixture; `taken.txt` was given real
unmitigated amounts on six lines, an amplified hit among them.[^expected]
Stored cards need `wowdps history regrade` to count armor. Landed in the
commit that introduced this record.

[^contract]: CONTRACT.md — R17's 2026-10-08 amendment and the v43 wire row
[^format]: FORMAT-NOTES — the damage suffix's `unmitigated`, corrected
[^expected]: taken.expected.md — the line-by-line `reduced` table
