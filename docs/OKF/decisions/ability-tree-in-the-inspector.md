---
type: Decision
title: The Ability Tree In The Inspector
description: 'A player''s abilities nest the way Warcraft Logs'' damage pane does — a pet''s under its summon, a trinket''s effects under the item, a spell''s hit and tick as parts — computed by the daemon (R26, PROTO_VERSION 36) and drawn by the window as a folding tree inside the inspector''s existing five columns, casts going into the opened ability''s numbers rather than a column of their own.'
tags: [gui, core, wire]
status: stable
generated: { by: claude-code/opus-5.5, at: 2026-09-29T19:30:00-07:00 }
sources:
  - id: contract
    resource: ../../../CONTRACT.md
    title: CONTRACT.md — R26 and the wire table's v36 row
  - id: prototype
    resource: ../../design/window-redesign.html
    title: The window redesign prototype — the inspector's `.ilist` / `.t-ab` grid the tree stands on
---

**Where:** [`wowdps-core`](../crates/core.md) (`Segment::spell_tree`, the
parts, the casts), [`wowdps-model`](../crates/model.md) (`SpellTree`),
[`wowdps-proto`](../crates/proto.md) (`Breakdown.tree`, the details tier's
trees, `ClientState::drill_tree`), [`wowdps-daemon`](../crates/daemon.md),
[`wowdps-gui`](../crates/gui.md) (`inspector/tree.rs`, the list's tree lines),
[`wowdps-mcp`](../crates/mcp.md); ruling [R26](../rulings/r26.md).

## Context

The user pointed at Warcraft Logs' damage-done pane: a Wither row that opens
into Blackened Soul and its two Wither ids, Summon Sayaad opening into its
Melee and Lash of Pain, a trinket opening into its runes, and a stacked graph
per ability. The inspector had the flat list — a pet's ability as "Melee
(Sayaad)" among the player's own, one row per NAME (so Wither's two ids were
one row), no casts. Its lists are the prototype's five columns[^prototype] in a
520 px pane; WCL's table is ten columns across a page.

## Decision

The DAEMON decides the nesting, the window only draws it. Four kinds of link
come from four sources, and only three are automatic, so step 1 ships those:

- **A pet under its summon.** SPELL_SUMMON names the spell; the parser used to
  drop it. The meter keeps the first summon per (summoner, pet name) — by name,
  because the breakdown already keys pets by name — and labels the group with
  the spell ("Summon Sayaad"), except a hunter's "Call Pet N", which names a
  button: that group is the pet's. No summon seen → the pet's own name.
- **Parts of a row.** Every record splits by `(spell id, periodic)` — which
  covers both Wither's two ids AND one id that lands both ways (Shadow Word:
  Pain, Renew), the split WCL shows in parentheses.
- **A trinket's effects.** The item generator already chased trigger chains to
  find procs; it now also keeps the ONE trinket (by ItemSparse name) that owns a
  spell. Ambiguity is refused, not guessed: a proc two differently named
  trinkets share belongs to neither.
- **Talent procs under their spell** (Wither → Blackened Soul) are NOT in the
  game data as a link (the proc hangs off the talent's aura); that needs a
  curated, generator-validated table in the role-spells style — left for a
  later step, not faked.

Casts are counted passively (never combat, so the scanner is untouched) per
spell NAME and joined to the rows. On the wire the tree is a side structure
keyed by row key, NOT new fields on `Row`: `by_spell` keeps its meaning for the
TUI, the overlay, the comparison, the store and the mcp, which read it flat.

In the window the list becomes the tree inside its five columns: groups of two
or more and rows with parts fold (shut until opened, remembered for the
session), a group of one is drawn as its row with the pet's or trinket's name
after it, and a part leads with what it is ("Direct", "Over time") because a
narrow column cuts the tail first. Casts and Avg cast join the opened
ability's numbers — where there is room — rather than a sixth column. The keys
walk drawn LINES (a group is a stop), Enter on a group folds it, and ← → fold
while the keys are in the list, following the Deaths recap's precedent that
arrows in the inspector are the inspector's.

## Consequences

`ClientState` still thinks in row indices; the window keeps the line its keys
rest on when that is a group or a part. Stored pulls nest too: the details tier
keeps each player's trees, and a pre-v36 file reads flat. A Demonology lock's
drill on a real raid pull shows thirteen summon groups — the grouping is what
makes that list readable. Step 2 stacks the drill's graph: the six largest entries (or an open
ability's six largest targets) as bands in six hues validated for the
window's surface by the data-viz checks — no ochre (it reads as the
interface's gold) and no red (a death) — stacked in slot order so every two
that touch are a validated adjacent pair, a neutral "Other" on top; a colour
follows its entity (seated once per key, `inspector/stack.rs`), never its
rank; the list's bars wear the hues, so the list is the legend, and a switch
beside Per second draws the total alone. The series ride only to the
window; a Demonology lock's "Other" is its largest band, which is the
honest answer six hues can give. Step 3 adds the two remaining WCL columns without widening the list: Miss
% (the attacker's side of a miss, which R17 only counted on the defender)
and a DoT's uptime — the UNION of its debuff over every enemy, so a DoT on
two adds at once counts once — both in the opened ability's numbers and the
mcp's rows. Step 4 hangs a talent proc under the spell that drives it —
[A Talent Proc Nests Under Its Driver](proc-under-its-driver.md).[^contract]

[^contract]: CONTRACT.md — R26 and the wire table's v36 row
[^prototype]: The window redesign prototype — the inspector's `.ilist` / `.t-ab` grid the tree stands on
