---
type: Crate
title: wowdps-gui
description: 'wowdps GUI: iced window and wlr-layer-shell overlay client.'
resource: crates/gui
tags: [crate]
status: stable
generated: { by: okflight/0.4.0, at: 2026-09-06T13:42:20-07:00 }
---

iced frontend: a pure rendering client of the wowdps daemon, drawn either in a regular window (default) or as a wlr-layer-shell overlay tab (`--overlay`) that pins to a screen edge above the game. This binary depends on `wowdps-model` and `wowdps-proto` only: it cannot open a combat log or parse a line even by accident. What to tail is the daemon's decision (config `logs_dir`, or `wowdps daemon --file …`).

## Source

- Manifest: [`crates/gui/Cargo.toml`](../../../crates/gui/Cargo.toml)
- Root: [`crates/gui/src/main.rs`](../../../crates/gui/src/main.rs)
- Binaries: `wowdps-gui`

## Seams

The window's screens stack window-locally over `ClientState` — talents, then Home, then a pull's workspace — under a top bar (`top_bar.rs`) and beside one pull rail (`rail.rs`) that lists tonight's log and the history store's nights as one list; a stored pull opens in the same workspace through a `ClientState` of its own fed from `GetFight` (`history.rs`), read with the log's through `Gui::fight()` — [the Rail decision](../decisions/one-pull-rail.md). Every list of `Row`s is drawn through `table.rs`, one column list per surface (see [One Table For Every Row List](../decisions/one-table-for-every-row-list.md)). Home's derivation is client-side over `Fights` ([decision](../decisions/home-derives-from-fights.md)); the window's chrome is gold, or the owner's class by [the luminance rule](../decisions/one-accent-from-the-class-color.md); its palette, fonts and the `Look` its shared renderers take so the overlay never moves are [the Look decision](../decisions/gold-chrome-and-a-look-per-surface.md). The meter wears one window-only fight header (`fight_head.rs`) with the filter in its tab row — [the Header decision](../decisions/one-fight-header-over-the-meter.md). Its drill is the inspector beside it (`inspector.rs`, with `plot.rs`, `lanes.rs`, `list.rs`), following the selection through `ClientState`'s opt-in follow-selection, with lanes coloured by caster and a comparison overlaid in it — [the Inspector decision](../decisions/inspector-beside-the-meter.md).

## Contract

Public signatures and dependency policy: [`CONTRACT.md`](../../../CONTRACT.md).
