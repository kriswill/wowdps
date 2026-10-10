# crates/history (wowdps-history)

`wowdps history`: DuckDB over the history store's files, for the ad hoc
questions; the daemon answers the fixed ones from its card index. Views and
subcommands: `docs/OKF/crates/history.md`. Recipes:
`docs/history-queries.md`.

## Rules

- **Offline by construction.** DuckDB opens in memory with autoinstall and
  autoload off and its configuration locked. Never add an extension or a
  network path.
- **DuckDB stays in this binary**, never in the daemon. It is
  system-linked to nixpkgs' libduckdb, never `bundled`; the `duckdb` crate
  pin encodes the library version (`=1.10505.0` is DuckDB 1.5.5), and
  `tests/duckdb_version.rs` fails by name when they drift. Build and test
  in the dev shell (it sets `DUCKDB_LIB_DIR`).
- **Probe before you define.** A view over a nested field is defined only
  after a probe proves the field exists and is typed: DuckDB types an
  all-empty field as JSON, which answers struct references with more JSON
  instead of an error, so the probe rejects any type starting with JSON. An
  un-regraded or mixed lake must still open, and `stats` names what it lacks.
- **Honest unknowns.** NULL is the unknown, never 0 (absorb waste,
  efficiency). A field older cards lack is coalesced to the value that ranks
  them exactly as before.
- **Parity is the gate.** The daemon's fixed answers must equal SQL's over
  the same files (`tests/parity.rs`, which runs every recipe in
  `docs/history-queries.md`). A new stored field or grading rule changes
  both sides, and the mcp grader, together.
- **This crate never writes the store.** `import` and `regrade` go through
  the daemon. `materialize` writes `cache.duckdb` beside the lake, a file
  only this binary opens.
- **The lake holds real names.** Query output never lands in the repo.

```sh
cargo run --bin wowdps-history -- sql "select name, duration_ms from fights order by start_utc_ms desc"
cargo run --bin wowdps-history -- best-kill 3130 15
wowdps history regrade --kind key          # or <fight_id>, --encounter N; pins kept
wowdps history import ~/Games/wow/Logs     # the daemon sweeps a log or a directory
wowdps history replay-export <fight_id> -o DIR
cargo test -p wowdps-history
```
