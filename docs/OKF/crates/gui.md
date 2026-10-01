---
type: Crate
title: wowdps-gui
description: 'The wowdps GUI on Zed''s GPUI through GPUI Kit — the meter window and the wlr-layer-shell overlay, a pure client of model, proto and gui-logic; built as crates/gui-new beside the iced GUI, it took this crate''s place at the cutover.'
resource: crates/gui
tags: [crate, gui]
status: stable
generated: { by: claude-code/opus-5.5, at: 2026-10-01T11:32:09-07:00 }
sources:
  - id: spec
    resource: ../../spec-gui-new.md
    title: The GUI's specification — §3 platform, §6 architecture and themes, §9 testing, §10 the cutover
  - id: plan
    resource: ../../plan-gui-new.md
    title: The implementation plan — phases 1–5 and each step's As-built note
  - id: gpui
    resource: ../../gpui/README.md
    title: The GPUI reference — which GPUI, findings checked against the 0.3.7 sources
  - id: shots
    resource: ../../../crates/gui/SHOTS.md
    title: Shots, the render guard and the chrome budget
---

A pure rendering client of the daemon, drawn as the meter window or, with
`--overlay`, a layer surface over the game. Like every frontend it depends
on [`wowdps-model`](model.md) and [`wowdps-proto`](proto.md), plus the
framework-free [`wowdps-gui-logic`](gui-logic.md), never the engine, so it
cannot parse a log. It was rebuilt on GPUI as `crates/gui-new`
([the deprecated record](gui-new.md)) beside the iced GUI that held this
name, reproducing every overlay guard state and design-shot state, and took
the name at the cutover[^plan] — [Rebuild The GUI On GPUI Kit](../decisions/gui-on-gpui.md),
[No Forked Or Patched GPUI](../decisions/no-gpui-forks.md). The daemon's
overlay supervisor spawns it by config `gui_binary`, default `wowdps-gui`
([wowdps-daemon](daemon.md)).

## Source

- Manifest: [`crates/gui/Cargo.toml`](../../../crates/gui/Cargo.toml)
- Root: [`crates/gui/src/main.rs`](../../../crates/gui/src/main.rs)
- Binaries: `wowdps-gui` (`wowdps gui` through the dispatcher)
- Design record: [`docs/design/window-redesign.html`](../../design/window-redesign.html) — the prototype whose Tokens, sizes and behaviour the window implements
- Review loop: [`crates/gui/SHOTS.md`](../../../crates/gui/SHOTS.md) — the shot tests and the overlay render guard

## Seams

**`Session`** (`session.rs`) is one daemon link and its `ClientState` as an
entity; `pump` is all of its work — reconnect without waiting, drain,
apply, send what the state asks for, and notify on any non-empty drain
(not on `snapshot_gen`, which `CompareSnapshot` and `SegmentList` never
move). The app runs it every 100 ms; the link is a trait, so tests drive the
daemon's mock through the same `pump` with no timer[^spec]. One-shot
answers (`Loadout`, `History`, `Fight`, `HistoryChanged`) come back as
`Reply` events to whoever asked. `main.rs` makes the link before GPUI
starts, because `DaemonClient::connect` may spawn a daemon and wait.

**Keys are GPUI actions in contexts.** [gui-logic](gui-logic.md)'s chord
table, which [the TUI's parity test](tui.md) iterates, is registered once:
every meter action rides one `Do(Action)` on `Meter && !Input`, so no meter
key fires while a text field has the keys; the window-local gestures are
`Go(Gesture)`; menus switch the root to `Modal`, and a field's own keys bind
on `Filter > Input` or `Palette > Input`.

**One theme definition.** gui-logic's `theme::Def` (`gold`, `frost`) feeds
Kit's `Theme` slot by slot and the app's `Look`, so no surface draws a
literal colour — [the Look decision](../decisions/gold-chrome-and-a-look-per-surface.md)
generalised; class chrome is an accent over any theme
([one accent](../decisions/one-accent-from-the-class-color.md)). Kit's
styled `Input` decides its own placeholder colour and text size, so the
palette and the row filter use `window/field.rs`, Kit's unstyled input
dressed in the window's tokens.

**The window** (`window.rs` and `window/`) is one entity, `Gui`, over the
fight on the stage (`Gui::fight` — the log's `ClientState` or a stored
pull's own, fed from `GetFight` — with every gesture through `act`). The
redesign's surfaces are all here, each on its decision: the top bar and one
pull rail ([the Rail decision](../decisions/one-pull-rail.md)), one fight
header over the meter ([the Header decision](../decisions/one-fight-header-over-the-meter.md)),
the ribbon and "you" off the daemon's `mine`
([the Wire decision](../decisions/raid-timeline-and-mine-on-the-wire.md),
[R25](../rulings/r25.md)), one column model for every row list
([One Table](../decisions/one-table-for-every-row-list.md)), the inspector
beside the meter ([the Inspector decision](../decisions/inspector-beside-the-meter.md))
with [R26](../rulings/r26.md)'s ability tree and stacked graph
([the Ability Tree decision](../decisions/ability-tree-in-the-inspector.md)),
Home derived from `Fights` answers ([decision](../decisions/home-derives-from-fights.md)),
and the command palette and cards ([the Redesign decision](../decisions/window-redesign.md)).
The inspector is a pure model built once a frame (`window/inspector/model.rs`)
and drawn by its views; its graph, matrices and chips are components that
bring only a text measure and their paint to gui-logic's geometry. GPUI has
no app scale factor, so every window size goes through `w.z(…)`.

**The overlay** (`overlay.rs`, `overlay/`) draws what the iced overlay drew
in the theme's overlay palette. It opens through GPUI's `cx.open_window`,
never Kit's Root (which adds a 20 px shadow inset on a layer surface)[^gpui].
gpui-pre cannot move a layer surface after creation, so the surface spans
its whole edge, the content sits at an offset inside it, and the input
region is exactly the content; "hidden" is a 1 px strip with an empty
region. It never asks for a zero length (a viewport protocol error under
GPUI), and its output is the game's monitor, found by recomputing GPUI's
UUIDv5 of the output name.

**Animation is finite.** GPUI's animation frame re-renders the whole view,
and the window is one: an endless pulse idled it at 5.7 % of a core, three
pulses per live pull at 0.4 %. Every delight (bars easing, the tab
underline, a zoom gliding, a card's entrance, the talent ripples) settles
to the reviewed pixels under `cx.reduce_motion()`, which every shot test
sets.

**Reviewed headless.** Kit's `TestWindowExt` clicks through the real views
over the daemon's mock on every `cargo test`; tests that depend on text
metrics run in a `HeadlessAppContext` with the real cosmic-text system,
which needs a wgpu adapter (the GPU, or Mesa's lavapipe in CI and the nix
sandbox). Ignored shot tests render each surface at the prototype's sizes,
and `overlay_render_guard` compares the overlay's states, rendered without
the art caches, with committed PNGs within a tolerance[^shots].

**Libraries.** It links libxkbcommon(-x11), libxcb and fontconfig, and
dlopens wayland-client, vulkan and EGL; `build.rs` bakes the dev shell's
`LD_LIBRARY_PATH` into the RUNPATH, and the flake's `.#wowdps-gui` (its
own crane dependency layer) runs through a wrapper, which both modules put
on the daemon's `PATH`.

## Contract

Public signatures and dependency policy: [`CONTRACT.md`](../../../CONTRACT.md).

[^spec]: `docs/spec-gui-new.md` §6: sessions, `pump`, views as entities, keys, themes.
[^plan]: `docs/plan-gui-new.md`: phases 1–4 as built, and phase 5's "As built: cutover prepared (not merged)".
[^gpui]: `docs/gpui/README.md`: the findings section, Kit's Root on layer surfaces, and an animation frame redrawing the whole view.
[^shots]: `crates/gui/SHOTS.md`: every shot test, its variables, and the guard's tolerance.
