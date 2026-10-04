---
type: Decision
title: Boss Kills And Timed Keys Are Kept Whole, And A Stored Pull Answers As A Live One
description: 'v42: every boss kill and timed key joins the protected set for the season, the series tier''s format 2 keeps what opens an ability, and a stored pull''s drill stacks, opens an ability and compares byte for byte as the live meter does — the user''s rule that the stacked ability colours, drill-downs and comparisons are never lost where they matter.'
tags: [history, wire, retention, design]
status: stable
generated: { by: claude-code/opus-5.5, at: 2026-10-03T23:30:00-07:00 }
sources:
  - id: spec
    resource: ../../spec-history-store.md
    title: The history store specification — §6 what is stored, §7 layout and retention
  - id: contract
    resource: ../../../CONTRACT.md
    title: CONTRACT.md — the v42 row of the version table
---

**Where:** [`wowdps-model`](../crates/model.md) (`series::stack`,
`rank_curves`, `target_rows`, `total_of` — the one ranking and wording, live
and stored), [`wowdps-core`](../crates/core.md)
(`Segment::spell_target_rows`, `spell_targets_all`, `spell_tallies_all`),
[`wowdps-proto`](../crates/proto.md) (series format 2; `GetFight.spell` /
`pair`, `StoredFight.pair`; the details tier's `counts[]`),
[`wowdps-daemon`](../crates/daemon.md)'s history store (`Ask`, one
`answer` for the stored and the derived path, `Retention::keeps_whole`,
`Store::rewrites`), `wowdps-gui-logic`'s stored-pull adapter (`Kept`) and
the window's inspector. Extends
[Stored Pulls Keep Their Seconds In A Binary Series Tier](a-series-tier-for-stored-windows.md)
and [The Ability Tree In The Inspector](ability-tree-in-the-inspector.md).

## Context

A timed +15 opened from the store drew one flat curve where the same key
opened from the tailed log stacked its abilities in colour: a stored drill
never built `ability_series`, though the v39 series tier held every
ability second by second. Compare and Enter on an ability were refused on
every stored pull ("the store keeps no pair"), and the count views had no
stored drill at all. Retention made it worse over time: the details cap
counted kills too, so a dungeon run more than ten times, or a boss with
ten newer long wipes, lost its older kills' details — and with them the
series. Thirteen kills and timed keys of the real store had already gone
that way. The user ruled: never lose the ability colours, the drill-downs
or the comparisons for timed keys, killed bosses and pinned fights, for the
season (an archive at a season's turn is later work); wipes, trash and
over-time keys may degrade.

## Decision

**Kept whole.** `Retention::keeps_whole` — a boss kill or a timed key, never
aborted — joins pinned and annotated fights in the protected set: never
demoted, never evicted. Both caps now count the unprotected fights alone,
so the newest ten long wipes and over-time keys of a group keep their
details whatever its kills number (before, protected fights counted toward
the cap and squeezed the wipes out). An over-time key is not kept whole —
a pin keeps one. `history_keep_kills_whole = false` turns the rule off for
a store that would rather stay small, and the caps count kills again.

**Format 2 keeps what opens an ability.** Per player, each Damage ability's
enemy targets second by second (R24's per-unit series, one row per enemy
name — the ability's windowed target list and its stack by target), every
Damage and Healing ability's whole-fight target tallies (the list over the
whole fight, which counts more than hostile units and words overkill), and
the dense 1 s damage taken (the Taken drill's and a Taken comparison's
curve, where the rows tier keeps 10 s). The details tier gains the count
views' drills. A format-1 file still reads, its new parts empty, and the answer says so
(`StoredFight::abilities` false): the window then opens no ability rather
than list none of its targets. A file in a NEWER format is a later
build's and is never rewritten down.

**One answer, as live.** A `GetFight` carries the opened ability and a
second player; the daemon's `Ask` takes them with `stacked` (the window
draws a stack — `engine::wants_series`, the hub passes it), and one
`answer` builds the stored and the just-parsed (a key's member boss)
replies alike. The live meter was rebuilt onto the same functions —
`ability_series` through `model::series::stack`, an opened ability's
targets and stack through `spell_target_rows`, its whole-fight list through
`target_rows`, a windowed comparison's total through `total_of` — so stored
equals live by construction, and `crates/daemon/tests/series.rs` holds it:
every player's stack, every opened ability whole and windowed, the count
views and 24 comparisons on the fixture's kill; its ignored real-log gate
matched 109 abilities on a night of keys and 270 on a 25-player kill.

**Self-healing.** `Store::wants_rewrite` names a kept fight short of its
details, or a series file missing or older than the format (each file's
format read once at open); after its first status the history thread
queues them, newest first, and rewrites them from their logs a log's
fights at a time — one scan per log — while its mailbox is idle. A log no
longer on disk skips its fights.

**The window offers what the store kept.** A stored pull's last answer
says what it holds (`gui_logic::history::Kept`: details, series,
abilities): Compare stands live with the details, Enter opens an ability
with the abilities, and otherwise each is refused with a word that says
pinning keeps them — but before the first answer everything is offered
(the answer decides, and an ask it cannot serve backs out), and `v`
always lets a pin or a pair go.

## Consequences

Format 2 is ~2.1× format 1 (841 KB for a 7-minute 25-player kill, 734 KB
for a 30-minute +15 — the per-ability targets v39 measured at +75% and
chose not to keep, plus the tallies and the taken seconds); with every
kill and timed key kept, the store grows ~90 MB a month at the October 2026
pace, which the season's archive will bound.[^spec] The start-up rewrite
regrades every kept fight once (a few hundred at deployment, minutes of
one loader thread), which also brings their cards up to the current rules.
A stored Taken drill's ability list is still the rows tier's, capped at 16
with the rest rolled up. The binding text is CONTRACT.md's v42 row.[^contract]

[^spec]: The history store specification — §6 what is stored, §7 layout and retention
[^contract]: CONTRACT.md — the v42 row of the version table
