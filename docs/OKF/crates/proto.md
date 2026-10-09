---
type: Crate
title: wowdps-proto
description: 'Wire codec, daemon client and shared client state for wowdps.'
resource: crates/proto
tags: [crate]
status: stable
generated: { by: okflight/0.4.0, at: 2026-09-06T15:50:50-07:00 }
---

The wowdps wire protocol: hand-rolled, zero-dependency, binary, length-prefixed frames over a unix socket, plus the client library that speaks it. Depends on `wowdps-model` only — never on the engine. Also home to the shared client-side extras every frontend may need without touching the daemon: the hand-rolled JSON value (`json`) and the talent dataset + import-string codec (`talents`, ruling R14) — here rather than in one frontend so mcp and gui read the same code. `history` is the on-disk record codec of the history store (roadmap item 1): the daemon writes these documents, the readers parse them.

## Source

- Manifest: [`crates/proto/Cargo.toml`](../../../crates/proto/Cargo.toml)
- Root: [`crates/proto/src/lib.rs`](../../../crates/proto/src/lib.rs)

## The replay tier

`replay.rs` (v45, [R29](../rulings/r29.md)) is the history store's second
binary tier, `replay/<id>.bin` (`WDRP`, a section index, each post against
its unit's last), and `replay::csv` the one writer of the seven files a
replay reads; `GetReplay` / `Replay` carry it
([why](../decisions/replay-tier-in-the-store.md)).

## Seams

`ClientState` is every frontend's state machine, and a frontend changes its
semantics only by opting in: `set_follow(true)` — the window's
master-and-detail, never the TUI's — makes the drill follow the meter
selection (each move re-watches the segment with the row as the drill,
`v` pins one half of a pair); off, a drill is the screen it always was
([the Inspector decision](../decisions/inspector-beside-the-meter.md)).
`log_id` hands on what `SegmentList` names, the tailed log's identity,
which with a row's start is its stored card's fight id — additive, read
by the window's pull rail alone
([the Rail decision](../decisions/one-pull-rail.md)).
`raid()` holds the snapshot's raid timeline for the segment, and
`open_death` — opt-in like following, the TUI never calls it — drills the
Deaths view into one death window in one Watch
([the Wire decision](../decisions/raid-timeline-and-mine-on-the-wire.md)).
`select_player` — opt-in too — selects a player by key rather than by row,
for the window's command palette, which names a player the chart in hand
may not hold yet (a view switch still on its way)
([the Redesign decision](../decisions/window-redesign.md)).

`dirs` resolves the XDG base directories once (`data_home`, `cache_home`,
`state_home`, `config_home`, `data_path`): an empty or relative variable
counts as unset, as the spec words it, and `Base::resolve` is the rule as a
pure function so its tests never touch the process environment.
`talents::data_path` delegates to it, and
[`wowdps-encounter-rubric`](encounter-rubric.md) resolves its text sidecars
through it.

## Contract

Public signatures and dependency policy: [`CONTRACT.md`](../../../CONTRACT.md).
