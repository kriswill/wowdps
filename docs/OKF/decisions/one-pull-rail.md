---
type: Decision
title: One Pull Rail For Tonight And Every Stored Night
description: 'The window redesign''s Rail step replaces the fight list screen, the History screen and the icon tab strip with a top bar and one pull rail — tonight''s log and the history store''s nights as one list grouped by local night and visit, deduplicated by fight id — and opens a stored pull in the same workspace by feeding GetFight answers to a ClientState of its own, so one set of renderers draws both.'
tags: [gui, design, history]
status: stable
generated: { by: claude-code/opus-5.5, at: 2026-09-28T18:30:00-07:00 }
sources:
  - id: prototype
    resource: ../../design/window-redesign.html
    title: The window redesign prototype — its `.top`, `.rail`, the 1180 px drawer, and finding 2 ("One fight lives in two places")
  - id: shots
    resource: ../../../crates/gui/SHOTS.md
    title: Design shots, the chrome budget measure and the overlay snapshot guard
---

**Where:** [`wowdps-gui`](../crates/gui.md) (new `rail.rs` and `top_bar.rs`;
`history.rs` rewritten as the store's pages and a stored pull; `view.rs`'s
body, `window.rs`'s keys and messages, `fight_head.rs`'s rail button); one
additive accessor in [`wowdps-proto`](../crates/proto.md)'s `ClientState`
(`log_id`). No wire change. Part of
[the window redesign](window-redesign.md).

## Context

Finding 2 of the prototype[^prototype]: tonight's pulls sat under
"fights", a list titled with the log's file name; older pulls under
"history", undated, opening in a second renderer (`history::stored_screen`)
with no graph, deaths, mitigation, comparison or strip; and four controls
moved between pulls — strip discs, the ‹ › chips, `[` `]` and the fights
tab — each over a different scope. `SegmentList.log_id` plus a row's
`start_ms` already names its stored card, so the two lists merge without
duplicates.

## Decision

**A top bar and a rail.** The icon tab strip goes; the bar is the
prototype's `.top` — wordmark, the places Home and Fights, a jump box (the
command palette is a later step's: until then it and Ctrl K open the `?`
sheet, which then drops what is typed into it — a name typed into what
looks like a search field must not quit at its `q` or pin at its `p` — and
closes on Esc, Enter, Ctrl K or `?`), a live pill that pins the log's newest
pull as `m` does, the picker, gear and help. The rail stands beside Home
and the stage, 236 px, above 1180 px; at 1180 and under it is a 280 px
drawer over a scrim that holds the pointer (the fight header's list button
or `H` opens it; the scrim, Esc, Enter or a pick closes it). The drawer has
the keys: j, k and the arrows walk a highlight over the rows it draws and
Enter opens that one; `[` `]` (and ← →) step the stage and leave the
drawer open on the row they reached — the fight list and History let the
keyboard choose a pull from a visible list, and so must the drawer (the
`?` sheet's "pull list" surface). A step keeps its row in sight with the
prototype's `near()` room of 40 px under it; the drawer opening on a row it
must scroll to stands that night's heading at the top when both fit, else
centres the row. **Deliberate departure:** Esc closes an
open drawer before anything under it, where the prototype's chain walks
the filter, the inspector and the pair first — the drawer is the topmost
layer, and an Esc that changed what is under a scrim would be invisible.

**Nights are local, and a raid past midnight is one.** A pull's night is
its local date (the log's clock is local) with the small hours counted
with the evening before — the 06:00 cutover the mcp's local buckets
default to. "Tonight" is the night it is now, in the timezone of the
store's newest card (UTC with none), or any night holding a pull still
going; the rest read "Saturday, Sep 26", the year only off this one.

**Visits, as the reader lived them.** The log's visits are
`timeline::blocks` (an instance and its difficulty; its Σ last, "Whole
visit"; a Σ still while the visit goes on), and a segment the daemon files
under no visit but whose start falls inside one's span — trash between
two bosses — joins it; the rest runs of "Open world". A stored night
groups its raid visits by their Overall card (members by log, map and
start), its keys and dungeon runs as one "Mythic+ keys" visit — a dungeon
visit that a key on its map starts within ten minutes of is the key's
zone-in and is left out — and what no visit claims by kind ("Delve",
"Arena"). A card with no visit (an arena match, the open world's) joins no
raid visit; a stored pull of the tailed log that its list does not hold
joins the log's visit it followed; and a raid pull no visit claims is named
for its instance — the log's visit that night at its difficulty, else any
Σ card of its map in hand — "Raid, Heroic" only when nothing names it. The
log's own world runs call an arena's matches "Arena". A visit one character played wears their class
dot; one several did puts a dot on each row, and every dot names its
character under the pointer (colour alone cannot tell two alts of a class
apart). Trash is "Trash" whatever the engine named its open segment. A
key's "+14" is kept whole after its dungeon's name, which is what gives
way to "…" in the 236 px rail. A pinned card wears a small ★ in its row's
left gutter, before the glyph, and after the fight header's title — a
shape that takes nothing from the name (117 of the shots' 802 cards are
pinned: a star in the right cluster cut every key's dungeon short), where
the first cut left the mark off and so left the pin nowhere to be seen or
set. The star is in the quiet ink, 2 px clear of the current row's edge:
the one gold at the rail's left is that edge (the prototype has no pin
mark, and gold is its active and focus colour); a narrow header drops its
star for the title's room. A pull under a second reads "<0:01", where
"0:00" read as a value that failed to arrive.

**Dedup by fight id.** A card whose id is the log's id and a row's start
(a Σ's with its mark) is listed once, as the log's row, lending it what
only the store knows — a wipe's best health, whose pull it was — and a
link from Home to such a card opens the log's segment: the log answers
what the store cannot.

**One workspace.** A stored pull is a `ClientState` of its own
(`history::Stored`): one list entry, follow-selection on, fed synthetic
snapshots built from `GetFight` answers; every `Watch` it emits becomes
the `GetFight` for the same view, drill and death window — one in flight
for the whole window: the newest want waits behind it (a held `j` asks for
the player it lands on, not each one it passes), a stored pull stepped onto
while the last one's read is out waits for that answer (a held `[` walks
the rail without stacking reads the pull landed on would queue behind), and
the answer to a want since passed is set aside for it. The daemon answers a
read it refuses exactly as it answers for a fight it no longer holds, so
an empty answer is asked again once — after a 750 ms pause, not in the
instant the quota refused it — the stage keeping what it shows, before
the pull is called gone — under its fight header, so the rail button and
the steps are still there, and worded apart when the store is off. A
reconnect (the dev unit bounces the daemon on every rebuild) asks again
for the rail's page, the stored pull's `GetFight` and Home's page, which
the old connection took with it. The meter, the fight header and the
inspector read `Gui::fight()` — the stored pull's state or the log's — so
no renderer knows which it draws, and `history::stored_screen` and
`stored_stats` are deleted, with the table facilities only they used. What
the store has no answer for is refused rather than asked, and says so: a
comparison, an ability's own curve, the enemies' view — its tab stays on
the strip, disabled, with a tooltip; the inspector's Compare stays, inert,
saying why; a key for any of them answers with a toast and is dimmed on
the `?` sheet — so the workspace keeps its shape from pull to pull. The
log's view a stored pull cannot show waits for the step back onto the log.
A drill the store kept no breakdown for says so rather than wait. A stored
pull's header names its night ("Mon, Sep 21") when it is not tonight's —
the one locator left with the drawer shut — and its "you" is whichever of
the owner's characters its card says played it, before the window's lock
(until the Wire step's `Row.mine`); a pull of the log wears its paired
card's best health on its badge, as its rail row does.

**One walk.** `[` `]` (and ← →) walk the rail's drawn order — tonight's
log, then the stored nights, over what "Hide trash" leaves — as the
header's ‹ › do; past the last card in hand `[` asks for the next page. A
move keeps the view and the inspected player. On Home, where the rail
lights no row, they start from the rail's top. `q` quits the window from a
stored pull as from the log's. The window never shows the
`ClientState` List screen (the TUI does): with no pull on the stage the
log's newest takes it, as the list's Enter used to. The earlier nights
page in `HistoryQuery::Fights` answers, every character's, `home::PAGE`
cards (under `FIGHTS_CAP`), one request in flight; after
`HistoryChanged` the newest 20 cards are re-asked and merged (not a
whole page per closed pull, trash included), so pages the reader paged in
stay, and an older page already wanted still goes after it. A page of no
cards and a total of 0 while cards are in hand is the daemon's refusal,
never the store's word: asked again once after a pause, and a second such
answer leaves the rail and its total as they stood.

## Consequences

History and its scopes are gone from the window; its `p` pin is the
stage's (window-local, on a stored pull or a log pull whose card the rail
holds); Home's keys and raid rows open their best run or
kill on the stage. A stored pull cannot be compared or drilled to one
ability until the store keeps what those need. The rail is rebuilt from
its sources on every frame — cheap at a page or five, a cache if the
store grows past that. While the stage watches one segment, the daemon
re-sends the list only when its shape changes, so other rows' verdicts can
lag until the next pull; the watched row takes its snapshot's. The overlay
is untouched (its snapshot guard passes unchanged). Rendered against the
reference at all three sizes by the harness[^shots], `tile-rail-open` now
its own state.

[^prototype]: The window redesign prototype — its `.top`, `.rail`, the 1180 px drawer, and finding 2 ("One fight lives in two places")
[^shots]: Design shots, the chrome budget measure and the overlay snapshot guard
