---
type: Decision
title: Stored Pulls Keep Their Seconds In A Binary Series Tier
description: 'v39: kills, keys and pinned fights keep a fourth, binary history tier — every player''s abilities and damage targets second by second behind a per-player index — so a stored pull''s zoom window answers exactly what the live one did, read one player at a time and only when a window is asked for.'
tags: [history, wire, design]
status: stable
generated: { by: claude-code/opus-5.5, at: 2026-10-01T22:00:00-07:00 }
sources:
  - id: spec
    resource: ../../spec-history-store.md
    title: The history store specification — §6 what is stored, §7 layout and retention
  - id: contract
    resource: ../../../CONTRACT.md
    title: CONTRACT.md — the v39 row of the version table
---

**Where:** [`wowdps-model`](../crates/model.md) (`series`: the cell and row
types and `window_rows`, the one windowing function),
[`wowdps-core`](../crates/core.md) (`Segment::series_rows`,
`target_series_rows`), [`wowdps-proto`](../crates/proto.md) (`series`, the
codec; `GetFight.range`, `StoredFight.series`),
[`wowdps-daemon`](../crates/daemon.md)'s history store, `wowdps-gui-logic`'s
stored-pull adapter and `wowdps-mcp`'s `stored_fight`. Finishes what
[A Graph's Window Scopes The Drill](a-window-scopes-the-drill.md) left open
for stored pulls.

## Context

v38 scoped a live drill's lists to a dragged window from the meter's sparse
per-second series; a pull opened from the history store had no such series,
so its lists stayed "whole pull". Re-reading the pull from its log on
demand would have needed no new data, but only while the log is on disk;
the choice was to store the seconds. Measured on a 25-player Heroic night
before choosing a shape: a 7-minute kill is ~75 000 (player, ability,
second) cells — 1.3 MB as JSON, 487 KB as row-major varints, 349 KB
columnar with second-runs and a packed hits/crits byte, gzip saving a
further 37% that the stdlib-only rule would make us hand-roll; per-enemy
target seconds add ~17%, per-ability-per-target seconds 75% (not kept).

## Decision

**A fourth tier, binary, beside the details.** `series/<id>.bin`
(`proto::series`): a fixed head with a format byte and the index's length,
an index of (guid, offset, length), then per player three row lists —
Damage abilities, Healing abilities, damage targets — each row its key,
spell id and school, its seconds as maximal runs, a varint amount and a
packed hits/crits byte per second, and overkill or overheal stored dense or
sparse, whichever is smaller (overkill is almost always zero, overheal
almost never). Binary because it is the one tier sized by seconds ×
abilities × players and nothing reads it but a window: SQL keeps the coarse
series and no fixed answer needs these, so DuckDB never sees it. A stored
pull's answer never touches the file; a window reads the head, the index
and that one player's block (`Backend::read_range`, a few kilobytes of a
raid's file), and a refused or foreign-format file reads as absent.

**Kept for kills, keys and pinned fights, while their details last** (the
user's call). `Retention::wants_series`: the fight earns details AND is a
kill, a key or pinned; it is written with the details, unlinked when they
are demoted or evicted and when a wipe's pin is let go. Pinning a fight that
earns it but has none — a wipe, or anything stored before the tier — queues
its rewrite from the log, as `regrade` backfills older kills.

**One windowing function.** The live meter now folds its sparse series into
the same `SeriesRow`s the store keeps (`Segment::series_rows`,
`target_series_rows`) and windows them through `model::series::window_rows`
— `spells_in` and `damage_targets_in` were rebuilt on it — so a stored
window cannot drift from a live one: `crates/daemon/tests/series.rs`
compares every player of the fixture's kill on both views over four
windows, and its ignored real-log gate matched 306 windows on the night.

**v39 on the wire.** `GetFight` gains `range`; `StoredFight` gains `series`
so a stored pull's state windows its drill only where the store can answer
(`set_drill_windows`, which now re-asks when it turns on under a drawn
zoom). The mcp's `stored_fight` takes `from_secs` / `to_secs`.

## Consequences

Measured on the real night: 408 KB for the 7-minute kill beside its 752 KB
details file, 563 KB beside 865 KB for a 10-minute one — the tier adds
about 55–65% of a kept kill's details.[^spec] Older fights gain it only
through a regrade, while their logs exist. The format byte leaves room for
compression without a migration. The binding text is CONTRACT.md's v39
row.[^contract]

[^spec]: The history store specification — §6 what is stored, §7 layout and retention
[^contract]: CONTRACT.md — the v39 row of the version table
