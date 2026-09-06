---
type: Crate
title: wowdps-proto
description: 'Wire codec, daemon client and shared client state for wowdps.'
resource: crates/proto
tags: [crate]
status: stable
generated: { by: okflight/0.4.0, at: 2026-09-05T17:18:33-07:00 }
---

The wowdps wire protocol: hand-rolled, zero-dependency, binary, length-prefixed frames over a unix socket, plus the client library that speaks it. Depends on `wowdps-model` only — never on the engine. Also home to the shared client-side extras every frontend may need without touching the daemon: the hand-rolled JSON value (`json`) and the talent dataset + import-string codec (`talents`, ruling R14) — here rather than in one frontend so mcp and gui read the same code. `history` is the on-disk record codec of the history store (roadmap item 1): the daemon writes these documents, the readers parse them.

## Source

- Manifest: [`crates/proto/Cargo.toml`](../../../crates/proto/Cargo.toml)
- Root: [`crates/proto/src/lib.rs`](../../../crates/proto/src/lib.rs)

## Contract

Public signatures and dependency policy: [`CONTRACT.md`](../../../CONTRACT.md).
