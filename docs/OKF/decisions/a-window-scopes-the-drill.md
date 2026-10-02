---
type: Decision
title: A Graph's Window Scopes The Drill, And The Inspector Widens
description: 'v38: a drag across a Damage or Healing drill''s graph now re-asks the daemon for the ability list (and Damage''s targets) inside that window, read from the sparse per-second series the comparison already windowed, with every list saying whether it kept the window or the whole fight; and the window''s inspector gains a corner button (`f`) that widens it over the whole stage under the view tabs, its panes side by side.'
tags: [gui, wire, design]
status: stable
generated: { by: claude-code/opus-5.5, at: 2026-10-01T18:00:00-07:00 }
sources:
  - id: contract
    resource: ../../../CONTRACT.md
    title: CONTRACT.md — the v38 row of the version table, `Segment::spells_in` / `damage_targets_in`, and the graph gestures
  - id: shots
    resource: ../../../crates/gui/SHOTS.md
    title: Design shots — the `zoom` and `wide-*` states
---

**Where:** [`wowdps-core`](../crates/core.md) (`Segment::spells_in`,
`damage_targets_in`, `spell_targets_ranged`, `breakdown_ranged`),
[`wowdps-model`](../crates/model.md) (`View::windows_drill` /
`windows_targets`, `SpellTree::windowed`), [`wowdps-daemon`](../crates/daemon.md)'s
engine, [`wowdps-proto`](../crates/proto.md)'s `ClientState`,
[`wowdps-gui-logic`](../crates/gui-logic.md) (`inspect::wide`,
`inspect::list::Room`) and [`wowdps-gui`](../crates/gui.md)'s inspector.
Grows [An Inspector Beside The Meter](inspector-beside-the-meter.md) and
[Enemy Taken Is A Live View](enemy-taken-is-a-live-view.md)'s v33 window.

## Context

Zooming the drill graph (a left drag) was the client's own slice of a
whole-fight timeline: the curve zoomed and the ability list under it kept
the whole pull, so "what did I do in that burst" had no answer. v33 had
already put a window on `Cursor::Segment` for the enemies' attackers, and
R12's per-spell sparse series (and R26's healing twin) already answered a
window for the comparison's tables — the data was there, the drill never
asked for it. Separately, the inspector stands 520 px wide beside the
meter; a 25-player raid's ability list with its graph and lanes reads
cramped there, and there was no way to give it the stage.

## Decision

**The window rides the cursor wherever a series stands behind the rows.**
`View::windows_drill` (Damage, Healing, EnemyTaken) says where; the daemon
honours `Cursor::Segment.range` there and echoes it on `Breakdown.range`.
A Damage or Healing drill's abilities are `Segment::spells_in` — the
comparison's `compare_spells`, made generic over the two series, the window
snapped out to whole seconds as v33's is, `per_sec` over it, each row the
school of its whole-fight row. Damage's targets (`View::windows_targets`)
come from R24's per-enemy series (`damage_targets_in`), hostile units only.
Healing keeps no per-target series, so its targets stay the whole fight,
and so does Taken, whose per-spell hits keep no clock — rather than invent
a series to fill them, every list says which it is: "Ability, 0:42–1:13"
or "Target, whole fight", and the graph's top line gives the window's rate.
The tree beside a window's rows keeps its groups alone
(`SpellTree::windowed`): casts, parts, misses and uptime are whole-fight
counts, and beside a window's tallies they would lie (parts that no longer
sum to their row). The comparison's window now covers Healing too.

**`PROTO_VERSION` 38 though no byte moved.** A new client against an old
daemon would send a window the daemon ignores and wait forever on an echo
that never comes (`ClientState` shows nothing from a reply whose echo is
not the window it asked for), so the meaning change renames the socket
like a shape change would. A stored pull's synthetic answers carry no
window, so its state is told not to send one (`set_drill_windows(false)`)
and its lists say "whole pull". The overlay's drill zoom rides the same
`ClientState`, so its ability list scopes too.

**Widened, the inspector takes the stage under the tabs.** A corner button
at the head's right (and `f`, window-local; Esc narrows it before the chain
reaches Home) swaps the meter and the inspector column for one inspector
the stage's width. The meter's keys still walk the players behind it. The
layout grows for the room rather than stretching: the head on one line with
the actions beside the name, every number in one line, the graph 180 px
tall instead of 96 (the plot height became a parameter of gui-logic's
geometry), and the panes the tabs would switch side by side 3:2 on a stage
of 1100 px or more — abilities beside targets, a recap beside its
attackers, R21's matrices under them — one over the other below that. A
list with the room shows the rate and the view's last word (overheal,
absorbed); a pair's halves add rate, hits and crit. Session-wide, never in
a narrow window, whose inspector is already pushed over everything.

## Consequences

The list under a zoom answers the question the zoom asks, and says when it
cannot. Stored pulls gained their seconds in v39:
[Stored Pulls Keep Their Seconds In A Binary Series Tier](a-series-tier-for-stored-windows.md). Healing's targets and Taken's lists are the two honest gaps; each
would need a per-second series of its own (the memory R12's and R24's
already spend), which this change declined to add. Every `wide-*` and
`zoom` design shot renders over the Coiled Altar slice.[^shots] The binding
text is CONTRACT.md's v38 row.[^contract]

[^contract]: CONTRACT.md — the v38 row of the version table, `Segment::spells_in` / `damage_targets_in`, and the graph gestures
[^shots]: Design shots — the `zoom` and `wide-*` states
