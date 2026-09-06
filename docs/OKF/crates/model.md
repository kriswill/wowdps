---
type: Crate
title: wowdps-model
description: Zero-dependency domain types for the wowdps combat-log meter.
resource: crates/model
tags: [crate]
status: stable
generated: { by: okflight/0.4.0, at: 2026-09-05T00:53:27-07:00 }
---

The wowdps domain vocabulary: what a meter shows, said without reference to how it is computed. Everything here is plain data — no I/O, no parser, no dependencies — so frontends (and the wire protocol) can bind to these types while the engine that produces them stays out of their build. `wowdps-core` re-exports everything here, so engine code and the fixture contract keep their existing paths.

## Source

- Manifest: [`crates/model/Cargo.toml`](../../../crates/model/Cargo.toml)
- Root: [`crates/model/src/lib.rs`](../../../crates/model/src/lib.rs)

## Contract

Public signatures and dependency policy: [`CONTRACT.md`](../../../CONTRACT.md).
