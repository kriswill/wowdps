# crates/model (wowdps-model)

The domain vocabulary: plain data, no I/O, no parser. Frontends and the
wire bind to these types while the engine stays out of their build;
`wowdps-core` re-exports them. Structure: `docs/OKF/crates/model.md`.

## Rules

- **Zero dependencies,** not even workspace crates.
- **A type on the wire is a wire shape.** Changing a field of a type that
  `crates/proto` encodes is a `PROTO_VERSION` bump with its goldens
  (`crates/proto/AGENTS.md`).
- **Shared arithmetic lives here once:** `rate(amount, duration_ms)`,
  `series::window_rows` and `series::stack` serve the live and the stored
  answer alike. Never re-derive them in a caller.
