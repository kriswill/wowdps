# crates/history (wowdps-history)

`wowdps history`: DuckDB over the history store's files, for the ad hoc
questions; the daemon answers the fixed ones from its card index. Views and
subcommands: `docs/OKF/crates/history.md`. Recipes:
`docs/history-queries.md`.

## Rules

- **Offline by construction.** DuckDB opens in memory with autoinstall and
  autoload off and its configuration locked. The bundled build compiles
  both defaults ON, so those `SET`s in `Lake::open_with` are the only
  guard; `network_access_is_off` holds them. Never add an extension or a
  network path.
- **DuckDB stays in this binary**, never in the daemon. It is `bundled`:
  the `duckdb` crate compiles its own DuckDB source (`=1.10506.1` is DuckDB
  1.5.6) with the `json` and `parquet` features; ICU is not in that build,
  so no query may use it (no time zone functions). The C++ compile (about
  a minute on 24 cores) is reused by sccache only while libduckdb-sys's
  OUT_DIR is unchanged: any version change in its build-dependency closure
  compiles it again. Build in the dev shell, which provides the C++
  compiler. Stay on DuckDB 1.x (dependabot ignores `>= 1.20000.0`); 2.0 is
  its own change.
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
