---
type: Crate
title: wowdps-history
description: 'wowdps-history binary: ad hoc SQL over the history store''s lake (DuckDB).'
resource: crates/history
tags: [crate]
status: stable
generated: { by: okflight/0.4.0, at: 2026-09-06T18:29:58-07:00 }
---

The history store's analytical reader (roadmap item 1, spec §10): DuckDB over the lake the daemon writes under `$XDG_DATA_HOME/wowdps/history/v1/`. The daemon answers the fixed questions from its card index; this crate answers the ad hoc ones in SQL, and the parity gate in `tests/parity.rs` keeps the two readers honest against the same files. The engine opens in memory with autoinstall / autoload off and the configuration locked, so it never touches the network; `materialize` writes `cache.duckdb` beside the lake, a file only this binary ever opens (DuckDB's single-writer lock never crosses a process). DuckDB is SYSTEM-linked to nixpkgs' libduckdb, and the `duckdb` crate pin encodes the library version it was written against (`=1.10505.0` is DuckDB 1.5.5); `tests/duckdb_version.rs` asks the linked library for `version()` and fails by name when the two drift, so a dependabot bump ahead of nixpkgs, or a nixpkgs bump that leaves the pin behind, is a clear red check rather than a guess (the pin had sat at 1.5.5 over a 1.5.4 library for a month before it existed).

## The replay tier

`replay-export` (v45) writes a stored fight's replay tier as the seven files
a replay reads, offline through `proto::replay::csv`, `raid.csv` from the
details tier; `stats` counts the tier, `cards_without_replay` and the
kept-whole set by why ([R29](../rulings/r29.md),
[progression](../decisions/progression-kept-whole.md)). No SQL view: the
tier is binary and read whole.

## Source

- Manifest: [`crates/history/Cargo.toml`](../../../crates/history/Cargo.toml)
- Root: [`crates/history/src/lib.rs`](../../../crates/history/src/lib.rs)

## Contract

Public signatures and dependency policy: [`CONTRACT.md`](../../../CONTRACT.md).
