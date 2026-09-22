---
type: Crate
title: wowdps-daemon
description: 'Headless wowdps daemon: tails the combat log and serves meter snapshots over a unix socket.'
resource: crates/daemon
tags: [crate]
status: stable
generated: { by: okflight/0.4.0, at: 2026-09-06T18:29:58-07:00 }
---

The wowdps daemon: one process owns tail → index → parse → meter → snapshots for every client. Threads + channels, no async runtime. `run` is the whole daemon; `DaemonOptions` makes every path and grace injectable so the integration suite can run real daemons on temp sockets against the fixtures.


## History store

`history.rs` is a thread owning the fight lake: the hub hands it one clone
per closed segment, the tailed log's index for backlog, and — since PR #56 —
a `Retire` for the log the tailer just left, which it rescans as an older
session so an abandoned pull and the night's Σ import without a restart
([why the trigger is the file switch and never the game process](../decisions/retire-a-log-on-tail-switch.md)).

## Source

- Manifest: [`crates/daemon/Cargo.toml`](../../../crates/daemon/Cargo.toml)
- Root: [`crates/daemon/src/lib.rs`](../../../crates/daemon/src/lib.rs)

## Contract

Public signatures and dependency policy: [`CONTRACT.md`](../../../CONTRACT.md).
