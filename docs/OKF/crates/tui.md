---
type: Crate
title: wowdps-tui
description: 'wowdps binary: daemon launcher and terminal meter client.'
resource: crates/tui
tags: [crate]
status: stable
generated: { by: okflight/0.4.0, at: 2026-09-06T13:42:20-07:00 }
---

`wowdps`: the daemon, the launcher, and the TUI client — one binary. The TUI is a pure rendering client: it never opens the log, never parses a line. All state that matters lives in the daemon; this file connects, declares a cursor, and turns snapshots into frames.

## Source

- Manifest: [`crates/tui/Cargo.toml`](../../../crates/tui/Cargo.toml)
- Root: [`crates/tui/src/main.rs`](../../../crates/tui/src/main.rs)
- Binaries: `wowdps`

## Seams

**Two tests read the TUI's own source**, because the crate is a binary a
test cannot call into. `tests/no_engine.rs` greps that the TUI's modules
never name an engine module. `tests/keybind_parity.rs` reads `keys.rs`'s
match arms and holds them against the GUIs' chord table,
[gui-logic](gui-logic.md)'s `keys::ACTIONS`, compiled in as a
dev-dependency: every TUI binding must be mirrored, and a GUI binding the
TUI lacks fails unless the contract carves it out (R12's `v` and `g`).
The window's own gestures stay out of that table.

## Contract

Public signatures and dependency policy: [`CONTRACT.md`](../../../CONTRACT.md).
