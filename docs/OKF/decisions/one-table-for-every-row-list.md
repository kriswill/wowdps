---
type: Decision
title: One Table For Every Row List, And History As A Window-Local Browser
description: 'GUI slice 2: every list of `Row`s the window draws goes through one table primitive whose headings, cells and total row are generated from the same column list; the History screen is window-local like Home and renders a stored fight on that same table; the `?` sheet is keyed on the surface; the R21 matrix derives level 0 per debuff.'
tags: [gui, history, wire]
status: stable
generated: { by: claude-code/fable-5.1, at: 2026-09-13T22:30:00-07:00 }
sources:
  - id: gui
    resource: ../crates/gui.md
    title: wowdps-gui — where the table, the History screen and the drills live
  - id: proto
    resource: ../crates/proto.md
    title: wowdps-proto — ClientState's death selection and drill accessors
  - id: design
    resource: ../../design/gui-window-features.html
    title: The window design study (layouts B–F, H)
---

**Where:** [`wowdps-gui`](../crates/gui.md) (`table.rs`, `history.rs`,
`taken.rs`, `view.rs`, `keys.rs`), [`wowdps-proto`](../crates/proto.md)
(`state.rs`).

## Context

Slice 1 landed the visual language and Home. What remained of the design
study was the part the window had been dropping on the floor:
wire fields sent since v27/v28 and never rendered (the R21 ledger, the
indexed death windows, the R17 record as one sentence), the meter as a bar
list rather than a table, a footer reciting a hint string on every screen,
a History tab that led nowhere, and Home panels that were not jump points.
`GetFight` answers in the live snapshot's shape — rows plus a breakdown —
so a stored fight needs no renderer of its own if the live one is reusable.

## Decision

- **One table primitive.** `table::Col` names a numeric column and knows
  its width, its heading per view (blank where meaningless — a count view
  has no rate), its cell text (blank where a row has nothing to say — no
  crits, no events — never a misleading 0) and its sort key. The meter, the
  by-spell pane and the by-target pane each name a column list, and the
  heading line, the row cells and the pinned total row are generated from
  that same list, so the three cannot drift. Sorting cycles desc → asc →
  the daemon's order and, like the filter, changes what is DRAWN and in
  what order but never what the numbers mean: every row keeps the index it
  arrived with, so ranks, shares and click targets hold and `j`/`k` walk the
  drawn order.
- **The `?` sheet is keyed on the surface.** Each binding names the
  surfaces it works on; the sheet lists what works HERE and the rest dimmed
  under "elsewhere". That is what earns the footer the right to carry only
  the daemon's status line.
- **History is window-local like Home** (`Gui::history`, above Home in the
  stack, never a `Screen` variant), pages `Fights` under Home's
  one-in-flight rule, and opens a pull through `GetFight` onto the same
  table the live meter uses — the design's "every stats surface leads back
  into the ordinary meter" without teaching the meter anything twice. The
  view keys on a stored fight refetch it; Esc walks drill → fight → list.
- **`ClientState` owns the death selection** (`death` on the Watch, reset
  with the drill and the view) rather than the window, so a stale index can
  never describe another player's death; `drill_breakdown` is the one
  accessor every drill-side reader goes through. No wire change.
- **The R21 matrix derives level 0 per debuff:** the by-ability baseline
  less that debuff's own cells. A hit lands in exactly one cell of each
  debuff open over it, so the remainder is exactly that debuff's level 0
  even when two debuffs overlap; subtracting every debuff's cells at once
  (the first draft) went negative on a real +13. No baseline → level 0 stays
  empty rather than invented.
- **The matrix is a section, not a footer.** Stacked under the panes it
  starved them; behind a "stacks" chip it gets the height it needs.

Commits `fdf2b76`, `71a589b`, `02fd24a`, `c96f0bd`, `da9f26f`, `a2db35e`.

## Consequences

The by-spell pane takes the larger share of the drill's width (3 : 2)
because six columns need it; a narrow window clips ability names before it
clips numbers. A stored fight's drill shows the panes the details tier
holds and says "no breakdown stored (tier N)" otherwise. History's measure
column is the owner's own number and stays a dash when the owner is not on
the pull or not known — never anyone else's. The lane chart of Layout F is
not built: no card carries a per-pull timeline, so the "you" bar against
the scope's best stands in for it.
