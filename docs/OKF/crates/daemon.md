---
type: Crate
title: wowdps-daemon
description: 'Headless wowdps daemon: tails the combat log and serves meter snapshots over a unix socket.'
resource: crates/daemon
tags: [crate]
status: stable
generated: { by: okflight/0.4.0, at: 2026-09-05T17:18:33-07:00 }
---

The wowdps daemon: one process owns tail → index → parse → meter → snapshots for every client. Threads + channels, no async runtime. `run` is the whole daemon; `DaemonOptions` makes every path and grace injectable so the integration suite can run real daemons on temp sockets against the fixtures.

## Source

- Manifest: [`crates/daemon/Cargo.toml`](../../../crates/daemon/Cargo.toml)
- Root: [`crates/daemon/src/lib.rs`](../../../crates/daemon/src/lib.rs)

## Contract

Public signatures and dependency policy: [`CONTRACT.md`](../../../CONTRACT.md).
