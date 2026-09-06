---
type: Crate
title: wowdps-tui
description: 'wowdps binary: daemon launcher and terminal meter client.'
resource: crates/tui
tags: [crate]
status: stable
generated: { by: okflight/0.4.0, at: 2026-09-03T21:19:39-07:00 }
---

`wowdps`: the daemon, the launcher, and the TUI client — one binary. The TUI is a pure rendering client: it never opens the log, never parses a line. All state that matters lives in the daemon; this file connects, declares a cursor, and turns snapshots into frames.

## Source

- Manifest: [`crates/tui/Cargo.toml`](../../../crates/tui/Cargo.toml)
- Root: [`crates/tui/src/main.rs`](../../../crates/tui/src/main.rs)
- Binaries: `wowdps`

## Contract

Public signatures and dependency policy: [`CONTRACT.md`](../../../CONTRACT.md).
