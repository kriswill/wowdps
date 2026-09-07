---
type: Crate
title: wowdps-history
description: 'wowdps-history binary: ad hoc SQL over the history store''s lake (DuckDB).'
resource: crates/history
tags: [crate]
status: stable
generated: { by: okflight/0.4.0, at: 2026-09-06T13:42:20-07:00 }
---

The history store's analytical reader (roadmap item 1, spec §10): DuckDB over the lake the daemon writes under `$XDG_DATA_HOME/wowdps/history/v1/`. The daemon answers the fixed questions from its card index; this crate answers the ad hoc ones in SQL, and the parity gate in `tests/parity.rs` keeps the two readers honest against the same files. The engine opens in memory with autoinstall / autoload off and the configuration locked, so it never touches the network; `materialize` writes `cache.duckdb` beside the lake, a file only this binary ever opens (DuckDB's single-writer lock never crosses a process).

## Source

- Manifest: [`crates/history/Cargo.toml`](../../../crates/history/Cargo.toml)
- Root: [`crates/history/src/lib.rs`](../../../crates/history/src/lib.rs)

## Contract

Public signatures and dependency policy: [`CONTRACT.md`](../../../CONTRACT.md).
