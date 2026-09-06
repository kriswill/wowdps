---
type: Crate
title: wowdps-core
description: 'WoW combat-log engine: parser, meter, structural index, file tailer.'
resource: crates/core
tags: [crate]
status: stable
generated: { by: okflight/0.4.0, at: 2026-09-05T00:53:27-07:00 }
---

wowdps-core: the engine. Parsing (`parser`), aggregation (`meter`), the startup index (`index`) and log following (`tail`) — everything between bytes on disk and domain rows. Only the daemon runs this; frontends are pure clients binding to `wowdps-model` types over `wowdps-proto`.

## Source

- Manifest: [`crates/core/Cargo.toml`](../../../crates/core/Cargo.toml)
- Root: [`crates/core/src/lib.rs`](../../../crates/core/src/lib.rs)

## Contract

Public signatures and dependency policy: [`CONTRACT.md`](../../../CONTRACT.md).
