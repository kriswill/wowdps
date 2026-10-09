---
type: Crate
title: wowdps-core
description: 'WoW combat-log engine: parser, meter, structural index, file tailer.'
resource: crates/core
tags: [crate]
status: stable
generated: { by: okflight/0.4.0, at: 2026-09-06T18:29:58-07:00 }
---

wowdps-core: the engine. Parsing (`parser`), aggregation (`meter`), the startup index (`index`) and log following (`tail`) — everything between bytes on disk and domain rows. Only the daemon runs this; frontends are pure clients binding to `wowdps-model` types over `wowdps-proto`.

## The replay cut

`replay.rs` (v45, [R29](../rulings/r29.md)) cuts a segment's parsed lines
into a `model::replay::Cut` — one reading of the log for the meter and the
replay alike: the parser's `HpHint.pos` and the passive replay events feed
it, the index seeds every WORLD_MARKER line, and the module doc lists each
deliberate difference from the extractor's old text cutter
([why](../decisions/replay-tier-in-the-store.md)); gated by
[replay.txt](../fixtures/replay.md) and every fixture's R29 rows.

## Source

- Manifest: [`crates/core/Cargo.toml`](../../../crates/core/Cargo.toml)
- Root: [`crates/core/src/lib.rs`](../../../crates/core/src/lib.rs)

## Contract

Public signatures and dependency policy: [`CONTRACT.md`](../../../CONTRACT.md).
