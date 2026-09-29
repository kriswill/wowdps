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
- Design record: [`docs/design/window-redesign.html`](../../design/window-redesign.html) — the prototype whose Tokens, sizes and behaviour the window implements
- Review loop: [`crates/gui/SHOTS.md`](../../../crates/gui/SHOTS.md) — the headless design shots and the overlay snapshot guard

## Seams

Two frontends, one binary, each over its own `ClientState`: the window and the overlay. The window was redesigned against the prototype in six steps, the whole in [the Redesign decision](../decisions/window-redesign.md); the overlay was forked around — `overlay.rs` edited only for the guard's seams and v35's test literals — and its pixels are held by a hash guard.

**The window's frame.** A top bar (`top_bar.rs`) over one pull rail (`rail.rs`) that lists tonight's log and the history store's nights as one list, beside Home or a pull's stage; a stored pull opens in the same workspace through a `ClientState` of its own fed from `GetFight` (`history.rs`), read with the log's through `Gui::fight()` — [the Rail decision](../decisions/one-pull-rail.md). Home, the talent viewer, the command palette, the `?` sheet and the rail are window-local: `ClientState` never learns they exist, and their keys stay out of `keys::action_for`, which the TUI's parity test reads.

**The stage.** One window-only fight header (`fight_head.rs`) with the filter in its tab row — [the Header decision](../decisions/one-fight-header-over-the-meter.md); under it the ribbon (`ribbon.rs`) draws the snapshot's raid timeline — the raid rate, the lust, a skull per death a press away from its recap — and on the Deaths view the meter is the deaths in the order they happened (`deaths.rs`); "you" is whichever row the daemon marked `mine` — [the Wire decision](../decisions/raid-timeline-and-mine-on-the-wire.md), ruling [R25](../rulings/r25.md). Every list of `Row`s is drawn through `table.rs`, one column list per surface ([One Table For Every Row List](../decisions/one-table-for-every-row-list.md)). The drill is the inspector beside the meter (`inspector.rs`, with `plot.rs`, `lanes.rs`, `list.rs`), following the selection through `ClientState`'s opt-in follow-selection, with lanes coloured by caster and a comparison overlaid in it — [the Inspector decision](../decisions/inspector-beside-the-meter.md).

**Home and the palette.** Home (`home.rs`, `home/panels.rs`, `home/charts.rs`) opens on the scope's last night and the week's keys, raid progress and key throughput, derived client-side from `Fights` answers ([decision](../decisions/home-derives-from-fights.md)); its scope is a chip row, never a lock. The command palette (`palette.rs`, Ctrl K) reaches any pull, player, view or screen, selecting a player through `ClientState::select_player` (opt-in) — both in [the Redesign decision](../decisions/window-redesign.md).

**Chrome, type and the overlay's pixels.** The window's chrome is gold, or the owner's class by [the luminance rule](../decisions/one-accent-from-the-class-color.md) under `chrome = "class"`; its palette, its bundled fonts (Barlow Semi Condensed with tabular digits baked in, Marcellus for titles — OFL assets under `crates/gui/fonts/`, loaded by `window::settings()` alone) and the `theme::Look` its shared renderers take so the overlay never moves are [the Look decision](../decisions/gold-chrome-and-a-look-per-surface.md). Line icons are canvas strokes (`line_icons.rs`) and the one custom widget is `ellipsis.rs`, because iced's "svg" feature would pull crates and "advanced" pulls none.

**Reviewed headless.** `window::shots::design_shots` renders the real window at the prototype's three sizes through iced_test's tiny-skia Simulator over the daemon's mock — no window, no GPU, no daemon — and `overlay::guard::overlay_snapshot_guard` checks the overlay's states against SHA-256 hashes in `crates/gui/snapshots/overlay/`; run the guard alone, on the machine that blessed it (SHOTS.md says why).

## Contract

Public signatures and dependency policy: [`CONTRACT.md`](../../../CONTRACT.md).
