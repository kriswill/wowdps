---
type: Decision
title: Retire A Log When The Tailer Leaves It, Never When The Game Exits
description: A lingering daemon imports the previous session's open tail — an abandoned pull, the raid night's Σ — the moment the tailer switches to a newer log, through the same aborted-import path the start-up sweep uses, because the trigger must be derivable from the files alone; the game-process signal stays a liveness and overlay hint and closes nothing.
tags: [daemon, history]
status: stable
generated: { by: claude-fable/5.1, at: 2026-09-21T23:10:00-07:00 }
sources:
  - id: history-rs
    resource: ../../../crates/daemon/src/history.rs
    title: the history thread — HistoryReq::Retire, sweep, scan_next
  - id: hub-rs
    resource: ../../../crates/daemon/src/hub.rs
    title: the hub's Tail arm, where a Switched retires the previous source
  - id: contract
    resource: ../../../CONTRACT.md
    title: CONTRACT.md — rulings R6 (mid-log version reset) and R10 (visits)
---

**Where:** [`wowdps-daemon`](../crates/daemon.md) (the hub and the history
thread), [R10 Visits & Overall](../rulings/r10.md), [R6 Mid-log version
reset](../rulings/r6.md).

## Context

Exiting the game raised the question "should we flush the log on exit,
since the next session starts a new one anyway?" The daemon cannot flush
the game's buffer (a normal exit already does; a crash loses what was
buffered), so the only thing "flush" could mean was finalizing what the
meter still had open. Three things are open at exit: the trailing trash
segment (not stored by default), an abandoned pull (the game never writes
its ENCOUNTER_END), and the night's visit — a raid visit ends only on a
ZONE_CHANGE out, and people log off inside the instance, so its Σ was never
written live.[^contract]

The start-up sweep already handled all of this: an older log's open tail
imports as an aborted pull and its open visit as the night's Σ. But a
lingering daemon never re-swept. On the tailer's `Switched` the engine
reset and only the NEW file's index reached the history thread, so the old
session's records waited for the next daemon restart. On a dev box the
path-unit's rebuild restarts masked it; under the packaged nix module,
which lingers for weeks, the gap was real.

## Decision

- **The trigger is the file switch, not the process.** Every stored card
  must be a pure function of the log file, or a regrade and an import
  disagree with what the live daemon wrote. A /proc verdict is not in the
  file. The tailer's move to a newer log is: it says the previous file is a
  finished session, at exactly the moment a new one begins.
- **One request, one existing path.** The hub's `Tail` arm sends
  `HistoryReq::Retire(old)` when a `Switched` names a different file than
  the engine's current source (the daemon's first log retires nothing; a
  re-announce of the same file is not a switch).[^hub-rs] The thread pushes
  the path onto its scan list flagged NOT live — superseding a pending scan
  of the same file that a start-up sweep listed as the newest — and
  `scan_next` runs the sweep's older-log branch over it: open tail as
  aborted, open visit as the Σ (or an aborted key), closed segments deduped
  against the store.[^history-rs]
- **R6 needs nothing.** A mid-file COMBAT_LOG_VERSION (one log across
  sessions) already closes the open segment in the meter, so that pull
  stores live; the visit only suspends there, which is right for a /reload
  mid-key and for a raid re-entered next session.[^contract]
- **The game-process signal is untouched.** It still bridges the log's
  multi-minute flush bursts for liveness and drives the overlay supervisor;
  it never closes a segment.

## Consequences

- A raid night's Σ and an abandoned last pull appear in History a few
  seconds after the next session's log is created, no restart needed. Until
  that next session they remain unstored, as before — the honest cost of a
  file-derived trigger.
- A crash mid-pull now stores an aborted card when the next session starts
  rather than at the next restart: earlier, not new.
- Gated by a hub unit test (first log, same file, real switch) and by
  `tests/history.rs`'s live-daemon test, which writes a newer log beside a
  tailed one cut mid-pull and waits for the old session's aborted boss and
  Σ to land with the daemon still running.

Landed in commit `e50f608` (PR #56).

[^history-rs]: the history thread — `HistoryReq::Retire`, `sweep`, `scan_next`
[^hub-rs]: the hub's `Tail` arm, where a `Switched` retires the previous source
[^contract]: CONTRACT.md — rulings R6 (mid-log version reset) and R10 (visits)
