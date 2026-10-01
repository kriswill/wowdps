# crates

The Cargo workspace members — the engine, the wire protocol, the daemon, the three frontends, the MCP server, the history reader and the DB2/CASC extractor — one doc each, scaffolded from Cargo.toml and the crate-root comment.

## Concepts

* [wowdps-core](core.md) - WoW combat-log engine: parser, meter, structural index, file tailer.
* [wowdps-daemon](daemon.md) - Headless wowdps daemon: tails the combat log and serves meter snapshots over a unix socket.
* [wowdps-extract](extract.md) - DB2/CASC extractor generating wowdps game-data tables from a local WoW install.
* [wowdps-gui-logic](gui-logic.md) - The GUI's framework-free half — config, Hyprland IPC, the keymap, history pages, the art-cache readers and fonts (wave A), then every model, word and geometry of the overlay, the window, the inspector, the rail, Home, the palette and the talent viewer (wave B) — moved out of the iced GUI, never copied, while it and its GPUI successor coexisted.
* [wowdps-gui-new](gui-new.md) _(deprecated)_ - Deprecated: the name the GPUI GUI was built under (crates/gui-new, binary wowdps-gui-new) beside the iced wowdps-gui; at the cutover it became crates/gui and took the wowdps-gui name, so its record is wowdps-gui.
* [wowdps-gui](gui.md) - The wowdps GUI on Zed's GPUI through GPUI Kit — the meter window and the wlr-layer-shell overlay, a pure client of model, proto and gui-logic; built as crates/gui-new beside the iced GUI, it took this crate's place at the cutover.
* [wowdps-history](history.md) - wowdps-history binary: ad hoc SQL over the history store's lake (DuckDB).
* [wowdps-mcp](mcp.md) - wowdps-mcp binary: MCP server exposing fight data to LLM harnesses.
* [wowdps-model](model.md) - Zero-dependency domain types for the wowdps combat-log meter.
* [wowdps-proto](proto.md) - Wire codec, daemon client and shared client state for wowdps.
* [wowdps-tui](tui.md) - wowdps binary: daemon launcher and terminal meter client.
