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

## Views and subcommands

The views over the lake's JSON files:

- `fights`, `players` (with `role`, derived by spec id so an un-regraded
  lake answers), `rows`, `details`, `loadouts`, `annotations` and
  `role_ranks` (the mcp grader's role-relative rank, floors included, in
  SQL; the DPS role ranked by `players.effective_dps_sql` under one label).
- `taken`, `mitigation`, `taken_spells`, `taken_sources` — R17 on the rows
  tier.
- `support`, `support_targets`, `players.effective_dps_sql` (recomputed
  with a coalesce and a clamp so a card older than R19's grading ranks
  exactly as before) and a derived `support` flag.
- `uptime` (rows.uptime[]: fight × target × spell × caster, `kind` stored as
  its name), `coarse` (the 10 s `taken10` / `heal10` lists cast to
  `BIGINT[]`, plus the merged mark list) and `players.am_uptime_pct_sql`
  (coalesced, DOUBLE first).
- `shields` (rows.shields[] per absorber × spell: applied / consumed /
  wasted / count / unknown) and `players.absorb_wasted` /
  `shields_unknown` / `absorb_efficiency_sql` (NULL is the honest
  pre-R20 and unknown value, never 0).
- `players.heal_absorbed` (0 on an older card) and `power` (fight × guid ×
  power type: `max`, and `per_sec` a BIGINT list with NULL gaps).

**Probe-defined columns.** Each nested view is defined only after a probe
proves its field exists AND is typed: DuckDB types an all-empty nested field
as JSON, and a JSON column answers struct references with more JSON instead
of erroring; an all-empty LIST column types as `JSON[]`, so the probe
rejects any type starting with JSON. An un-regraded or mixed lake still
opens, and `stats` reports `cards_without_taken`, `rows_without_mitigation`,
`cards_without_am_uptime` and `rows_without_uptime`.

**Subcommands:** `sql`, `views`, `best-kill`, `progression`, `trend`,
`role-night` (the SQL twin of the daemon's `RoleNight`, UTC nights),
`export`, `stats`, `materialize`, `replay-export` (below), `import` (a thin
client of the daemon's `ImportLog`) and `regrade`. `docs/history-queries.md`
is the recipe list every parity run executes, and `tests/parity.rs` the gate
that holds the daemon's fixed answers equal to SQL's over the same files.

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
