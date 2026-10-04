---
type: Decision
title: A Death Ends Where They Are First Seen Alive
description: 'R23 ends a death span at the rez that raised them or the first line whose advanced block reports them above zero health — never at damage dealt in their name, which a real +15 showed landing after the player died — an Overall carries a death its member left open into the next member, and the wire says which spans never closed (v41 `Mark::open`), so the inspector draws a death as a red rule and its return as a green one instead of a hatch.'
tags: [meter, ruling, wire, gui]
status: stable
generated: { by: claude-code/opus-5.5, at: 2026-10-03T12:00:00-07:00 }
sources:
  - id: contract
    resource: ../../../CONTRACT.md
    title: CONTRACT.md — R23 (amended) and the wire table's v41 row
  - id: format
    resource: ../../../crates/core/fixtures/FORMAT-NOTES.md
    title: FORMAT-NOTES.md — which unit the advanced block describes, per event
  - id: census
    resource: ../../../crates/core/tests/real_log_deaths.rs
    title: The real-log R23 gate — how real spans ended
---

**Where:** [R23](../rulings/r23.md) in [`wowdps-core`](../crates/core.md)
(`Meter::feed`'s health-report pass, `Segment::seen_alive`,
`Segment::first_alive`, the Overall's `absorb`), the `Mark` codec in
[`wowdps-proto`](../crates/proto.md) and the history record, the inspector's
graph in [`wowdps-gui`](../crates/gui.md) over
[`wowdps-gui-logic`](../crates/gui-logic.md)'s `inspect::geometry::rules`,
and the [mcp](../crates/mcp.md) marks.

## Context

The inspector hatched a death from the moment to its rez or, without one,
to the fight's end, because R23's other close — "the first thing only the
living do: a cast, damage dealt" — was a guess. A Mythic+ death is almost
never rezzed (the player releases and respawns at the entrance), so every
death in a key ran to the end of the graph and each one laid another hatch
over the last: seven deaths, seven layers. The user asked for when they died
and when they were alive again, as a line, and the hatch gone.

The guess was also wrong where it did answer. On the user's Voidscar Arena
+15 three of Tranqlock's seven deaths closed within 70 ms: his imps'
Isolated Implosion landed in his name 65 ms after UNIT_DIED, and projectiles
in flight do the same. One of the run's two battle rezzes went unnamed,
because a stray hit had closed the span before `SPELL_RESURRECT` arrived.
The log itself held the answer: 5.1 s after that death a heal landed whose
advanced block described Tranqlock at 759 581 health, the respawn.[^format]

## Decision

- **Proof of life is a sight of them alive.** A line whose advanced block
  describes a player above zero health (the parser's `hp_hint`: a heal or a
  hit landing on them, their own cast or swing) ends their death span, and
  so does a cast (for a log without the block). Damage they deal and
  healing they do never count. The sight goes through the passive gate a
  cast uses, so it never opens or extends a segment, and lazy = full
  holds.[^contract]
- **An Overall carries an open death.** A member's death still open at its
  close (a wipe) said they were dead when that pull ended, not that they got
  up then. The Overall keeps it open, and the next member ends it at its
  first sight of them (`first_alive`, the same gate), or at their next death
  there. Role spans keep their member-clock close. The merge holds in either
  order (`Segment::alive_after`): the daemon's mid-visit attach absorbs the
  earlier, scanned prefix INTO the live Overall, so the prefix's open deaths
  close against the live side's first sight (CodeRabbit's review of #76).
- **The wire says which spans never closed** (PROTO_VERSION 41, `Mark` +
  trailing bool `open`; the history record writes `"open"` on every mark).
  No reader could work it out from `at_ms + dur_ms`: an open span reads to
  the segment's close, and a key's Σ closes on its wall clock (31:32 on the
  Voidscar run) while its `duration_ms`, the graph's axis, is the key timer
  with its death penalties (35:24).
- **The graph draws two rules.** A dashed rule in the bad ink where they
  died ("died 0:45") and one in the good ink where they were alive again
  ("alive 0:50", "alive 18:52, rezzed by Stonehenge", "alive 3:22,
  Reincarnation"), none while `open`. The stretch between is left clear.
  Every death's words are placed before any return's, so a return close
  behind its death gives way rather than pushing a death's words out.

## Consequences

The census that cleared the rule ran over the machine's eight largest logs
(3 361 player deaths): no player was ever reported alive before the rez that
raised them (2 154 rezzes), and every death that ended under half a second
was a Hunter's Feign Death, whose UNIT_DIED carries a trailing `1` (the old
rule closed 554 deaths under a second). On the Voidscar night's real-log gate
the deaths ending under a second went from 5 to 0, and both rezzes are
named.[^census]

- Feign Death is no death at all ([R9](../rulings/r9.md), on the user's call
  the same day): the parser reads a `UNIT_DIED` whose trailing
  `unconsciousOnDeath` is 1 as `Other`, and the scanner drops it from
  `is_combat` alike, so a Hunter who feigns counts nothing, freezes no recap
  window and opens no span. `sample.txt` holds one as a negative control.
- Every stored card's death marks and R25 rezzes were written under the old
  rule: `wowdps history regrade` rewrites them. Until then the reader reads
  a pre-v41 death mark with no rez as `open` (its end was the old guess),
  so a stored pull draws no false return, only the deaths and their rezzes.
- The bump renames the socket (`wowdps-v41.sock`): the daemon, mcp and the
  overlay restart together.

[^contract]: CONTRACT.md — R23 (amended) and the wire table's v41 row
[^format]: FORMAT-NOTES.md — which unit the advanced block describes, per event
[^census]: The real-log R23 gate — how real spans ended
