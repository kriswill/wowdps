---
type: Decision
title: An Inspector Beside The Meter
description: 'The window redesign''s Inspector step replaces the drill screen with master and detail: an inspector column on the selected player beside the meter (pushed over it at 820 px and under), fed by an opt-in follow-selection mode in ClientState so every move re-watches the segment with the new row as the drill; its graph moves spans out of the plot into lanes coloured by caster, a comparison overlays two curves inside it, and Esc walks one chain that ends at Home.'
tags: [gui, design]
status: stable
generated: { by: claude-code/opus-5.5, at: 2026-09-28T16:00:00-07:00 }
sources:
  - id: prototype
    resource: ../../design/window-redesign.html
    title: The window redesign prototype — its `.split`, `.insp`, `.lanes` and `.cmp2`, and finding 3 ("Drilling in throws the meter away")
  - id: shots
    resource: ../../../crates/gui/SHOTS.md
    title: Design shots, the chrome budget measure and the overlay snapshot guard
---

**Where:** [`wowdps-gui`](../crates/gui.md) (`inspector.rs` and its
`plot.rs`, `lanes.rs`, `list.rs`; `view.rs`'s stage, `window.rs`'s keys and
messages, `table.rs`'s inspector grids, `keys.rs`' sheet); an opt-in mode in
[`wowdps-proto`](../crates/proto.md)'s `ClientState`. No wire change. Part
of [the window redesign](window-redesign.md).

## Context

Finding 3 of the prototype[^prototype]: to look at a second player the
reader backed out, moved and drilled again, and at 1440 px a player's name
and their numbers sat 1,000 px apart while the drill had no player header
at all. The drill graph washed every R18 span over the plot as a
full-height band — on a tank's Taken drill a red wash that hid the curve —
with no axis and no scale. The prototype answers with master and detail
above 820 px and push-and-back below, lanes under the curve, and a
comparison drawn in the inspector with the meter still in sight.

A `Watch` with `drill` set already returns the rows and the breakdown in
one snapshot; only `ClientState` treated a drill as a screen change.

## Decision

**Follow-selection is `ClientState`'s, and opt-in.** `set_follow(true)`
(the window's; the TUI never calls it) makes the drill the selected row's
for as long as the meter is up: `select_row` and j/k re-watch
`Cursor::Segment` with the row's key as the drill and the screen stays the
meter. The drill is keyed by guid, so a snapshot that re-sorts the rows
moves the selection with the player; a player a new view or pull does not
have hands the drill to the row the selection is on, and an answered view
with nobody in it (no one dispelled) drops the drill rather than wait on
numbers that are not coming. A move drops the breakdown in hand rather
than show the last player's under the new name; the window holds the last
player's graph and lists (`inspector::Held`) and draws them dimmed until
the new breakdown lands, so the column keeps its height as j is held.
Enter hands the keys to the drill's panes (`inspecting`), where j/k walk
the list — the keyed row wears the accent down its edge, the meter's
selection steps back to the hover's weight, and the window scrolls the
row into sight (`window::FindRow`, a two-step widget operation: find the
row's place in the scrollable's content, then `RevealSpan`) — and Enter
opens the ability as it always did; Back pops the ability, then the keys,
then leaves for the list. A move to another pull keeps the followed
player. A filter that hides the selection moves it to the first drawn
row. Off, every path is the old one — the proto tests hold both.

**Pin, then move.** `v` pins the selection (a gold-dim "A" after the
pinned row's name, a "B" on its partner once there is one, each named
under the pointer, and the prototype's toast — "Pinned X. Move to another
player to compare." — for 2.6 s or until the pair forms); the next move
makes the pair (`Screen::Compare`), the
pin alone again when the selection returns to it, `v` or Esc stops. The
comparison's cursor carries no rows, so the meter beside it keeps the rows
it had — on a live pull its heading says "paused while comparing" — and a
view switch or a pull step mid-pair re-asks for the meter first, the pair
re-forming on the new rows.

**The inspector is owned data drawn by width.** `Insp::of` reads the
window once per frame; the stage's `responsive` lays out the meter and an
inspector of 520 px (410 px in a 821–1180 tile — the maximum of the
prototype's `minmax`, which a grid always hands the column, the meter's
`1fr` giving way), or at 820 px and under the meter alone and, while
inspecting, the inspector over the whole stage with a back button. The
meter's columns narrow with the WINDOW, as the prototype's container
query does, not with the room the inspector leaves. The keys ask the same
width (`Gui::window_w`, from the window's open and resize events): Enter
on a pair beside the meter gives no keys to lists that have no keyed row,
and Tab and `g` in a narrow window push the inspector they change. The
view's numbers are the spec's, Healing's Rank included for everyone — a
place among their own role, the prototype's `roleRank`. Deaths'
inspector is the recap as the prototype's `recapPanel` draws it: oldest
first, the change signed and inked in a 78 px column, the source quiet
after the event ("yours" for the owner's own), the health after it a
6 px bar that turns amber under 15 % and red under 3 %, the killing
blow's row tinted — no fill under any words; with the keys in it, ← →
step the player's death windows (on the meter they step pulls, whoever
is selected). Enemies' is its numbers and then the attackers, as the
prototype's `enemyPanel` (the curve is one attacker's level down); a
pair's two curves and two lists, side by side in a wide window and
stacked in a tile or a narrow one, where 410 px halves would cut every
ability to a few letters. Taken's R21 ledger is a third tab ("Stacks"),
its matrices in the list's place — the stacks chip's job (reach the
ledger in the list's place) in the inspector's own tab row, rather than a
chip row that would put a second switcher over the first.

**One canvas for the graph, and lanes by caster.** `plot.rs` draws the
curve as an area (a pair as two lines on one scale, the second dashed
when one colour would draw both — a shared class, the prototype's
`graphBlock` — with its legend swatch to match), the rate in the
prototype's 10 s buckets (finer only when the stretch on show is too
short to hold forty — a zoom, a short pull — and the top line says the
bucket) drawn through a Catmull-Rom spline, minute ticks over the fight's
span, a death hatched to its rez or — with none — to the fight's end
(since v41 two rules instead, where they died and where they were alive
again, with the stretch between clear —
[A Death Ends Where They Are First Seen Alive](a-death-ends-at-first-sight.md))
with its words in a band the curves peak under (a second
death's words a row down rather than over the first's), and four lanes
under it: cooldowns, items, externals (someone else's defensive
included), defensives — each row as tall as its label's line (19.6 px
apart, the track centred), each span edged in the panel's surface so two
that overlap read as two. A pair's lanes are both players': each lane
split, the first's spans over the second's, the hover leading with whose
— R12's comparison is there to explain a gap by when each pressed their
cooldowns and their trinkets fired. **Deliberate departure:** with lanes,
the plot starts where their tracks do (74 px label column and its 8 px
gap), where the prototype runs the plot full width over offset lanes —
one x is one instant in the curve, the axis and every lane, where the
prototype's 82 px offset puts a cooldown at 0:20 under the curve's 1:00.
The gutter that leaves is the plot's scale: the peak level with it, 0 on
the baseline, and the top line then names the measure alone ("dps");
without lanes (an attacker's curve) the plot runs full width and the top
line says the peak, as the prototype's does. At 460 px that costs the
curve a fifth of its width; labels over their tracks would cost every
narrow inspector a lane's height again. **Second departure:** the lanes
take EVERY mark, not `compare::view_draws_mark`'s per-view set
(Healthstone off the Damage graph, defensives and mitigation on Taken
alone, healing cooldowns on Healing alone) — that rule keeps bands from
washing over a curve that is not about them, and a lane is its own row
under the curve, washing nothing; a Damage inspector shows a Defensives
lane because the player pressed a defensive. A span wears its CASTER's
class colour through `inspector::Roster`, every player the window has
seen on a meter (one it never saw is "someone else", never a guid's
tail); the same roster answers a drill's target rows by name, since those
wear the drilled player's class on the wire. Hover is the canvas's own —
a gold crosshair and the values, or a span's name, time, length and
caster — so it never round-trips; drag still zooms (R24's attacker window
rides it) and a zoom holds every span to the track. iced sets a canvas's
text over all of its shapes, so every label a tooltip would cover is left
out rather than printed through it.

**Esc is one chain.** Menus (the picker, the ⚙ card, the sheet — each
modal) → the filter → the inspector's ability → its keys → the comparison
→ Home, where it ends: Esc on Home only leaves a focused section, which
retires the toggle the prototype listed as a defect. The filter's text
goes before the inspector's keys wherever the field shows — beside the
inspector, keys in it or not; only a narrow window's push covers the
field, and there its keys come back first. A pair beside the meter holds
none. Keys on Home no longer reach the meter under it; a view key or a
pull step leaves Home for the fight; `m` and the live tab end on the
live meter, a pushed inspector's keys given back.

A drag across its graph now scopes its lists (v38), and its corner button
widens it over the stage: [A Graph's Window Scopes The Drill, And The
Inspector Widens](a-window-scopes-the-drill.md).

## Consequences

The drill screen, its two panes, the Taken cards and miss chips and the
window's comparison screen are gone from the window (the overlay keeps all
of its own, and its snapshot guard passes unchanged). The window graph no
longer shows marker icons along its top; lanes carry them. A comparison
on a live pull shows the meter as it stood when the pair formed, until
the pair ends — a second connection could keep it moving, if that matters.
A `Snapshot`'s breakdown carries no drill key, so a push for the last
player already in flight when the selection moves is taken for the new
one's until the Watch's reply, right behind it on the socket, replaces
it — a transient of one message (most
likely while j is held on a live pull at 10 Hz); echoing the drill on the
snapshot would close it, at the cost of a wire bump. The body the window
holds for the next move (`inspector::Held`) is re-taken on every snapshot
(`ClientState::snapshot_gen`), so neither that transient nor a live
pull's first answer outlives the next message.
Rendered against the reference at all three sizes by the harness[^shots].

[^prototype]: The window redesign prototype — its `.split`, `.insp`, `.lanes` and `.cmp2`, and finding 3 ("Drilling in throws the meter away")
[^shots]: Design shots, the chrome budget measure and the overlay snapshot guard
