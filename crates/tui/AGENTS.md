# crates/tui (wowdps)

The `wowdps` binary: the dispatcher (`wowdps <cmd>`, external commands such
as `wowdps mcp` and `wowdps history` found as siblings), the daemon entry
(`wowdps daemon`), the launcher and the terminal meter. Structure:
`docs/OKF/crates/tui.md`.

## Rules

- **A pure client.** It never opens the log or parses a line;
  `tests/no_engine.rs` fails if its sources name an engine module.
- **Pre-redesign semantics stay.** It calls none of `ClientState`'s opt-in
  capabilities (`set_follow`, `open_death`, `select_player`).
- **Keys mirror the GUI's table.** `tests/keybind_parity.rs` holds
  `keys.rs`'s match arms to gui-logic's `keys::ACTIONS`: a binding added on
  either side needs its twin, unless CONTRACT.md carves it out (R12's `v`
  and `g`).
- **Renders are tested** with ratatui's `TestBackend` over the daemon's
  mock.
- **Siblings are found by `current_exe`.** The dev shell's wrappers `exec`
  the real binary in `target/release` so the dispatcher finds its
  siblings there.

```sh
cargo test -p wowdps-tui
```
