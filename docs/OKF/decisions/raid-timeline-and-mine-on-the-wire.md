---
type: Decision
title: The Raid Timeline And Whose Rows On The Wire
description: 'The window redesign''s Wire step — one protocol bump (PROTO_VERSION 35) that puts on every meter snapshot the whole group''s fight (R25: the view''s raid series, every death in order with its killing blow and rez, the lust windows), a time-before-death on each recap entry, and a per-row `mine` flag from the daemon''s owner resolution, so the window''s ribbon, its chronological Deaths view and its "you" need no client-side guessing.'
tags: [wire, gui, core, daemon]
status: stable
generated: { by: claude-code/opus-5.5, at: 2026-09-28T16:46:42-07:00 }
sources:
  - id: prototype
    resource: ../../design/window-redesign.html
    title: The window redesign prototype — its `.ribbon`, `.v-deaths`, `.recap` / `.insight`, findings 6 and 8, and the build plan's Wire row
  - id: contract
    resource: ../../../CONTRACT.md
    title: CONTRACT.md — R25 and the wire table's v35 row
---

**Where:** [`wowdps-core`](../crates/core.md) (`Segment::raid_timeline`,
the recap's `offset_ms`), [`wowdps-model`](../crates/model.md)
(`RaidTimeline`, `Row.mine`, `Row.offset_ms`), [`wowdps-proto`](../crates/proto.md)
(the codec, `ClientState::raid` and the opt-in `open_death`),
[`wowdps-daemon`](../crates/daemon.md) (`mine.rs`, the engine and the
store marking answers), [`wowdps-gui`](../crates/gui.md) (`ribbon.rs`,
`deaths.rs`, the recap's time column and insight, the header's deaths),
[`wowdps-mcp`](../crates/mcp.md) (`fight`'s `raid`, rows' `mine`, recaps'
`offset_secs`). Ruling: [R25](../rulings/r25.md). Part of
[the window redesign](window-redesign.md).

## Context

Three things in the prototype[^prototype] had no data behind them. The
ribbon — the pull's signature under the header — needs the WHOLE GROUP's
rate for the view, every death with its killing blow, and the lust; a
snapshot carried one view's rows, and a raid curve summed client-side from
drill timelines would have cost 25 drills. The Deaths view had six identical
count bars where the order of deaths is the question, and a recap with no
sense of time. And "you" was matched by NAME against config
(`history_characters`, the accent's resolved name), so an alt the addon
knows was a stranger and a stored pull borrowed its card's owner.

## Decision

One bump, because each renames the socket and restarts every client.

- **R25, read time only.** `Segment::raid_timeline(view)` sums the series
  R12/R17 already keep over exactly the actors the friendly rows fold
  (the Σ identity is the test), takes the deaths from R9's windows (killing
  blow = the recap's newest damage) with the rez from R23's spans — which
  now keep the resurrect's spell and rezzer — and unions the lust family's
  R18 spans. Nothing is written at feed time, so segmentation, the index
  scanner and lazy = full are untouched by construction; `tests/raid.rs`
  holds the parity anyway. It rides every meter `Snapshot` as a trailing
  `Option`, for the SEGMENT: a count view carries the damage series, and
  the window keeps it across a view switch.
- **Recap offsets on the row.** A recap entry is already a `Row` with
  recap-only fields (`hp`, `gain`); `offset_ms` joins them, non-increasing
  down the newest-first list. The rows tier writes it on recap events only
  (`recap_events_json`), so no other row of any file grows a null column.
- **`mine` is the daemon's, at answer time.** `daemon::mine::Mine` is every
  character of the account the store knows — the addon's own set, the
  store-wide owner, every card's owner, by guid — plus the configured names.
  The history thread publishes it (`HistoryLink::mine`); the hub hands it
  to the engine before each build; the engine marks meter rows, a drill's
  player-naming rows (by the name the fight gives those guids), comparison
  totals and the timeline's deaths; the store marks a stored fight's.
  Never stored: an alt the addon names tomorrow is "you" on last week's
  pulls too.
- **A stored pull carries one too, in the same bump.** `StoredFight` gains
  a trailing `raid`, rebuilt by the store from what its tiers keep — the
  recaps' deaths with their rez off the R23 death marks, the lust off the
  External marks (one `meter::lust_windows` union for both paths), the
  details tier's 1 s series or the coarse 10 s one — and it equals the
  live timeline on everything the tiers hold. Leaving it for later would
  have cost a second socket rename, and every earlier night on the rail
  would have opened without its ribbon and on the count table the
  prototype's finding 6 retired.
- **The group's deaths, not an arena's.** `RaidDeath.enemy` flags the
  other team's deaths with the rows' own R13 test; the list keeps them
  (it is every window the Deaths rows count), and every reader that counts
  the group's deaths or battle rezzes skips them. A battle rez is a rez by
  someone else (`RaidDeath::battle_rezzed`), never a Reincarnation.
- **Opt-in client state.** `ClientState::open_death` (the Deaths view
  drilled into one death window) is the window's; the TUI never calls it
  and keeps today's semantics.

## Consequences

The window draws the ribbon from the snapshot it already has, the Deaths
view as the deaths in the order they happened (j/k walk them, a press opens
that death's recap), the recap with a time column and an insight line about
self-inflicted damage, and "you" wherever a row is marked — the config's
names are the daemon's to read, the window's own only while nothing is
marked (a daemon without a store). The header's deaths ("Deaths 6", "First
1:10", "Battle rezzes 2", "died 5:45") stopped waiting.

Costs to watch: a live Σ over a long visit sends its whole 1 s series with
every push (~8 bytes a second of visit, ~86 KB for three hours) — to the
window and the mcp alone: the engine builds the timeline only for a session
whose kind uses it (`engine::wants_raid`), so the overlay's and the TUI's
10 Hz pushes carry `None` — all but one: the overlay's Σ split
(`overlay_split`) watches the visit's Σ over a second connection of kind
`Window`, so that connection is sent the whole series, which the overlay
never reads (a distinct kind or a flag would spare it; the overlay's
pixels would not change); a stored
pull's series is coarser (10 s) once its details are demoted, and a Damage
view then draws no curve (the axis, the lust and the deaths only); boss
health ("Wipe at N%" live) is still not
on the wire. Binding text: R25 and the v35 row[^contract].

[^prototype]: The window redesign prototype — its `.ribbon`, `.v-deaths`, `.recap` / `.insight`, findings 6 and 8, and the build plan's Wire row
[^contract]: CONTRACT.md — R25 and the wire table's v35 row
