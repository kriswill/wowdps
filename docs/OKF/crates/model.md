---
type: Crate
title: wowdps-model
description: Zero-dependency domain types for the wowdps combat-log meter.
resource: crates/model
tags: [crate]
status: stable
generated: { by: okflight/0.4.0, at: 2026-09-06T18:29:58-07:00 }
---

The wowdps domain vocabulary: what a meter shows, said without reference to how it is computed. Everything here is plain data — no I/O, no parser, no dependencies — so frontends (and the wire protocol) can bind to these types while the engine that produces them stays out of their build. `wowdps-core` re-exports everything here, so engine code and the fixture contract keep their existing paths.

## The replay rows

`replay` (v45, [R29](../rulings/r29.md)): `Cut` and its rows — `Unit`,
`Post`, `Event`, `Placed`, `Marker`, `Head` — named as the columns of the
files a replay reads, the raid's damage per second left to R25. A `Post`
carries its floor (`map_id`) and `Cut::maps` counts each floor's posts; a
boss row (`boss_engaged`, `boss_killed`, `boss_wiped`) is the one `Event`
whose `unit` is `None`.

## Source

- Manifest: [`crates/model/Cargo.toml`](../../../crates/model/Cargo.toml)
- Root: [`crates/model/src/lib.rs`](../../../crates/model/src/lib.rs)

## Contract

Public signatures and dependency policy: [`CONTRACT.md`](../../../CONTRACT.md).
