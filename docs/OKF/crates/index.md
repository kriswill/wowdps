# crates

The Cargo workspace members — the engine, the wire protocol, the daemon, the three frontends, the MCP server, the history reader and the DB2/CASC extractor — one doc each, scaffolded from Cargo.toml and the crate-root comment.

## Concepts

* [wowdps-core](core.md) - WoW combat-log engine: parser, meter, structural index, file tailer.
* [wowdps-daemon](daemon.md) - Headless wowdps daemon: tails the combat log and serves meter snapshots over a unix socket.
* [wowdps-extract](extract.md) - DB2/CASC extractor generating wowdps game-data tables from a local WoW install.
* [wowdps-gui-logic](gui-logic.md) - The logic the iced GUI and gui-new share and neither draws — config, Hyprland IPC, the overlay's takeover socket, the keymap as a chord table, history pages, the ability tree, the art-cache readers, the bundled fonts, the theme's names and table columns — moved out of crates/gui, never copied.
* [wowdps-gui-new](gui-new.md) - The window and the overlay being rebuilt on Zed's GPUI through GPUI Kit, beside the iced wowdps-gui until a measured cutover; a pure client of model, proto and gui-logic, so far a skeleton whose window shows the daemon's Status and whose --overlay opens an empty layer surface.
* [wowdps-gui](gui.md) - wowdps GUI: iced window and wlr-layer-shell overlay client.
* [wowdps-history](history.md) - wowdps-history binary: ad hoc SQL over the history store's lake (DuckDB).
* [wowdps-mcp](mcp.md) - wowdps-mcp binary: MCP server exposing fight data to LLM harnesses.
* [wowdps-model](model.md) - Zero-dependency domain types for the wowdps combat-log meter.
* [wowdps-proto](proto.md) - Wire codec, daemon client and shared client state for wowdps.
* [wowdps-tui](tui.md) - wowdps binary: daemon launcher and terminal meter client.
