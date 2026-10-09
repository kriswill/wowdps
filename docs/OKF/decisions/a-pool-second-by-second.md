---
type: Decision
title: A Pool, Second by Second
description: 'New ruling R28 keeps each player''s power (the advanced block''s offsets 10–12) as a 1 s series per power type — the last report of a second, the largest max, gaps left empty, never a pet''s — passive, on the details tier and the drill, so the coach can read a healer''s mana curve and the window draws it under the graph; one PROTO_VERSION bump (v44) for it, R2''s heal-absorbs and R26''s empowered stages.'
tags: [meter, ruling, proto, history, gui]
status: stable
generated: { by: claude-code/opus-5.5, at: 2026-10-09T10:30:00-07:00 }
sources:
  - id: contract
    resource: ../../../CONTRACT.md
    title: CONTRACT.md — R28, R26's and R27's v44 amendments, the v44 wire row
  - id: format
    resource: ../../../crates/core/fixtures/FORMAT-NOTES.md
    title: FORMAT-NOTES — the advanced block's power fields and the empower trailers
  - id: tree
    resource: ../../../crates/core/fixtures/tree.expected.md
    title: tree.expected.md — the Evoker's empowers and every player's power
---

**Where:** [`wowdps-core`](../crates/core.md) (`Meter::feed`'s passive power
read, `Segment::power`), [`wowdps-model`](../crates/model.md) (`PowerSeries`,
`primary_power`, `Empower`), [`wowdps-proto`](../crates/proto.md),
[`wowdps-daemon`](../crates/daemon.md) (the live drill, the details tier),
[`wowdps-history`](../crates/history.md) (the probed `power` view),
[`wowdps-mcp`](../crates/mcp.md) (`stored_fight { player }`'s `power`),
[`wowdps-gui-logic`](../crates/gui-logic.md) and [`wowdps-gui`](../crates/gui.md)
(the inspector's mana strip and energize line); [R28](../rulings/r28.md),
[R27](../rulings/r27.md), [R26](../rulings/r26.md); gated by
[tree.txt](../fixtures/tree.md) and every fixture's `check.awk` power rows.

## Context

v43 parsed the advanced block's power (type, current, max) onto `HpHint` and
read nothing of it.[^format] R27 says what energized a player, not where
their pool stood: "when did the healer run dry" needs the pool itself. The
user decided on a 1 s series per player for the coach, drawn under a
healer's graph by default, and — riding the same bump — that R27's energize
reach the live drill and the inspector's head, and that R26 count an
Evoker's empowered releases by stage.

## Decision

R28: every line whose block describes a PLAYER with a pool writes their
second of that power type — the last report of the second wins, the type
keeps its largest max, a second nothing reported stays `None` (no
interpolation: a Druid in cat form reports energy, not mana).[^contract] It is
keyed by the raw guid and never folded — a pet's pool is its own, R27's rule
— and read at the top of `Meter::feed` through the passive gate, ahead of
the line's own event, exactly where R23 reads a sight of life: the scanner
reads no advanced block, so a report may never open, extend or split a
segment, and the line that opens a pull reports into nothing (full = lazy,
held by `tests/tree.rs`'s lazy picture on every fixture). One series per
type keeps a shapeshifter honest; `primary_power` names the type most
reported. It is stored on the details tier (demoted with it), answered on
`StoredFight.power` and on the drill's `Breakdown.power` — live for a
`Window` session only, like the stack's series, stored from the details
tier. The window draws a HEALER's mana under the graph's lanes on the
graph's own clock in the theme's new `power` data token (navy's other pixels
unmoved), with the pool's name in the lanes' gutter.

With it, `Breakdown.energize` (R27) rides every live drill, and the
inspector's head says it ("Mana 1.2M gained, 40.0k wasted"); and R26 counts
`SPELL_EMPOWER_END` by stage and `SPELL_EMPOWER_INTERRUPT` as cancels on the
spell's row (`SpellMeta::empower`, "Stage avg 2.8 · 3 cancelled" in the
opened ability's numbers). The four wire changes — `Row.heal_absorbed`,
`SpellMeta.empower`, `Breakdown.energize` / `power`, `StoredFight.power`,
`CardPlayer.heal_absorbed` — share PROTO_VERSION 44.

## Consequences

`check.awk` recomputes the series' shape independently on every fixture
(`power_seconds`, `power_sum`, `power_max`) from the same advanced blocks,
and it agreed with the meter on all eight on the first run;[^tree] a real
night holds 1 064 player series in 50 segments. The details tier grows by
about one number per player per reported second; an older details file has
no `power` and SQL defines no view over it until regraded. The live series
is sent to `Window` sessions only, so the overlay's Σ-split connection (a
`Window` kind) receives one it never reads, as it does the raid timeline.
The strip snaps to a zoom while the plot glides (under reduced motion both
settle at once).

[^contract]: CONTRACT.md — R28, R26's and R27's v44 amendments, the v44 wire row
[^format]: FORMAT-NOTES — the advanced block's power fields and the empower trailers
[^tree]: tree.expected.md — the Evoker's empowers and every player's power
