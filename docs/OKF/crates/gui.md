---
type: Crate
title: wowdps-gui
description: 'wowdps GUI: iced window and wlr-layer-shell overlay client.'
resource: crates/gui
tags: [crate]
status: stable
generated: { by: okflight/0.4.0, at: 2026-09-04T22:55:56-07:00 }
---

iced frontend: a pure rendering client of the wowdps daemon, drawn either in a regular window (default) or as a wlr-layer-shell overlay tab (`--overlay`) that pins to a screen edge above the game. This binary depends on `wowdps-model` and `wowdps-proto` only: it cannot open a combat log or parse a line even by accident. What to tail is the daemon's decision (config `logs_dir`, or `wowdps daemon --file …`).

## Source

- Manifest: [`crates/gui/Cargo.toml`](../../../crates/gui/Cargo.toml)
- Root: [`crates/gui/src/main.rs`](../../../crates/gui/src/main.rs)
- Binaries: `wowdps-gui`

## Contract

Public signatures and dependency policy: [`CONTRACT.md`](../../../CONTRACT.md).
