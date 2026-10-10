# crates/mcp (wowdps-mcp)

An MCP stdio server exposing the daemon's fight data as tools, reached as
`wowdps mcp` through the dispatcher's external-command lookup. Tool shapes:
`docs/OKF/crates/mcp.md`.

## Rules

- **As thin as every frontend:** model + proto only, so it cannot parse a
  log. Live and stored answers come from the daemon (the history tools are
  the store's fixed questions); never read the store's files here.
- **Talent tools answer locally.** `talent_tree`, `decode_talents` and
  `encode_talents` read the per-machine `talents.json` (R14) through
  `proto::talents`, never the daemon.
- **JSON is hand-rolled** (`proto::json`, re-exported here); parse never
  panics, and no serde.
- **Say the clock.** Amounts are raw totals with overkill left out;
  `per_sec` divides by the combat clock (`combat_ms`), and a key's rows add
  `run_per_sec` over the key timer. Tool descriptions say so.
- **Fields are additive.** A field's name keeps its meaning once shipped:
  coaching rubrics read them (`trend` keeps `per_sec` as an alias). A
  rename is a breaking change for every reader.
- **Grading is role-relative** (`grade.rs`). `crates/history`'s
  `role_ranks` ranks the same way, held by its parity gate.
- **`history_sql`** shells out to `wowdps-history` and is registered only
  where that binary exists.
- **A running server keeps its build.** `.mcp.json` starts it through
  `cargo run -q -p wowdps-mcp`, once per Claude session; after a
  PROTO_VERSION bump each session's server must restart.

```sh
cargo test -p wowdps-mcp
```
