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

Whose rows are the reader's is the history thread's to say: `mine.rs`'s
`Mine` (every character of the account the store knows, by guid, plus the
configured names) is published through `HistoryLink::mine`, and the engine
marks each snapshot's rows, drill and raid deaths from it before they go
out; the store marks a stored fight's when it answers
([the Wire decision](../decisions/raid-timeline-and-mine-on-the-wire.md)).

## Overlay supervisor

`overlay.rs` spawns `<gui_binary> --overlay` when the game appears. The
config key `gui_binary` (default `wowdps-gui`) lets
[gui-new's](../decisions/gui-on-gpui.md) overlay follow the game while both
GUIs exist. `Config::gui_bin` resolves it once, at start:

- **A bare name** is the daemon binary's sibling when one exists. That is a
  dev build's own GUI.
- **Otherwise** it is the name itself, for the spawn to find on `$PATH`.
  The home-manager and NixOS modules depend on that step: the daemon's
  package holds no GUI, and their `guiPackage` option puts one on the
  service's `PATH`.
- **A value with a `/`** is a path, never re-rooted.

`Status` does not name the binary, because naming it would be a wire
change. A failed spawn already does, in `Failed`'s
`spawning <path>: …`. The dev unit (`tools/dev-unit.sh`) stamps the daemon
and the configured GUI only, so a build of the other GUI never restarts the
live daemon.

## Source

- Manifest: [`crates/daemon/Cargo.toml`](../../../crates/daemon/Cargo.toml)
- Root: [`crates/daemon/src/lib.rs`](../../../crates/daemon/src/lib.rs)

## Contract

Public signatures and dependency policy: [`CONTRACT.md`](../../../CONTRACT.md).
