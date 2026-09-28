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

## Seams

`ClientState` is every frontend's state machine, and a frontend changes its
semantics only by opting in: `set_follow(true)` — the window's
master-and-detail, never the TUI's — makes the drill follow the meter
selection (each move re-watches the segment with the row as the drill,
`v` pins one half of a pair); off, a drill is the screen it always was
([the Inspector decision](../decisions/inspector-beside-the-meter.md)).

## Contract

Public signatures and dependency policy: [`CONTRACT.md`](../../../CONTRACT.md).
