# crates/model (wowdps-model)

The domain vocabulary: plain data, no I/O, no parser. Frontends and the
wire bind to these types while the engine stays out of their build;
`wowdps-core` re-exports them.

## Rules

- **Zero dependencies,** not even workspace crates.
- **A type on the wire is a wire shape.** Changing a field of a type that
  `crates/proto` encodes is a `PROTO_VERSION` bump with its goldens
  (`crates/proto/AGENTS.md`).
- **A player's label is split here alone** (`names.rs`), by the fight's
  edition: Retail's `Name-Realm-Region`, Forever's
  `First-Last-Realm-Region`. A reader that shows, matches or looks up a
  player's name goes through it; nothing else splits a label at a dash, and
  nothing keys on a name (CONTRACT.md, "Player names").
- **Shared arithmetic lives here once:** `rate(amount, duration_ms)`,
  `series::window_rows` and `series::stack` serve the live and the stored
  answer alike. Never re-derive them in a caller.
