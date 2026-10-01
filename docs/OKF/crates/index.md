# crates

The Cargo workspace members — the engine, the wire protocol, the daemon, the three frontends, the MCP server, the history reader and the DB2/CASC extractor — one doc each, scaffolded from Cargo.toml and the crate-root comment.

## Concepts

* [wowdps-core](core.md) - WoW combat-log engine: parser, meter, structural index, file tailer.
* [wowdps-daemon](daemon.md) - Headless wowdps daemon: tails the combat log and serves meter snapshots over a unix socket.
* [wowdps-extract](extract.md) - DB2/CASC extractor generating wowdps game-data tables from a local WoW install.
* [wowdps-gui-logic](gui-logic.md) - The logic the iced GUI and gui-new share and neither draws — config, Hyprland IPC, the keymap, history pages, the art-cache readers and fonts (wave A), then every framework-free half of the overlay, the window, the inspector, the rail, Home, the palette and the talent viewer (wave B) — moved out of crates/gui, never copied.
* [wowdps-gui-new](gui-new.md) - The window and the overlay rebuilt on Zed's GPUI through GPUI Kit, beside the iced wowdps-gui until a measured cutover; a pure client of model, proto and gui-logic whose overlay, talent viewer and window (stage, inspector, rail, Home, palette) are built, the window's remaining cards in progress.
* [wowdps-gui](gui.md) - wowdps GUI: iced window and wlr-layer-shell overlay client.
* [wowdps-history](history.md) - wowdps-history binary: ad hoc SQL over the history store's lake (DuckDB).
* [wowdps-mcp](mcp.md) - wowdps-mcp binary: MCP server exposing fight data to LLM harnesses.
* [wowdps-model](model.md) - Zero-dependency domain types for the wowdps combat-log meter.
* [wowdps-proto](proto.md) - Wire codec, daemon client and shared client state for wowdps.
* [wowdps-tui](tui.md) - wowdps binary: daemon launcher and terminal meter client.
