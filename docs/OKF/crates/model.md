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
whose `unit` is `None`. Format 3: a `Post` also carries the block's shields
(`absorb`) and the last reading of every pool kind its unit's posts have
named (`pools`, grown by `set_pool`, the one rule the cut and the tier
share), so `Post` is `Clone`, no longer `Copy`; the buff rows (`buff_applied`,
`buff_removed`, `buff_dose`) join the debuff rows, which now also hold a
player's debuffs on NPCs. Format 4 bounds the pools (`POOL_KINDS` 64, no
power type at or above it; `MAX_POOLS` 8 kinds a post) and tidies a
hand-built track: `next_pools` (what the tier writes for any post) and
`fill_pools` (a track whose posts fill `power` alone). Format 6: the head's
`KeyHead` carries the key's `affixes` beside its name and level
([Keep A Keystone Run's Affixes](../decisions/keep-keystone-affixes.md)).

## Source

- Manifest: [`crates/model/Cargo.toml`](../../../crates/model/Cargo.toml)
- Root: [`crates/model/src/lib.rs`](../../../crates/model/src/lib.rs)

## Contract

Public signatures and dependency policy: [`CONTRACT.md`](../../../CONTRACT.md).
