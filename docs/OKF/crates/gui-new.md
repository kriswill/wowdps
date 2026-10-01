---
type: Crate
title: wowdps-gui-new
description: 'The window and the overlay rebuilt on Zed''s GPUI through GPUI Kit, beside the iced wowdps-gui until a measured cutover; a pure client of model, proto and gui-logic whose overlay, talent viewer and window (stage, inspector, rail, Home, palette) are built, the window''s remaining cards in progress.'
resource: crates/gui-new
tags: [crate, gui]
status: stable
generated: { by: claude-code/opus-5.5, at: 2026-10-01T10:40:00-07:00 }
sources:
  - id: spec
    resource: ../../spec-gui-new.md
    title: gui-new specification — §3 platform, §6 architecture, §10 coexistence
  - id: plan
    resource: ../../plan-gui-new.md
    title: gui-new implementation plan — phase 1, step 1.1 and its As-built note
  - id: gpui
    resource: ../../gpui/README.md
    title: The GPUI reference — which GPUI, findings checked against the 0.3.7 sources
---

The GPUI successor of [`wowdps-gui`](gui.md), decided in
[Rebuild The GUI On GPUI Kit](../decisions/gui-on-gpui.md) and built in the
plan's order: phase 1's skeleton and spikes, then the overlay, the window,
the talent viewer and the cutover[^plan]. Like the iced GUI it is a pure
client: [`wowdps-model`](model.md), [`wowdps-proto`](proto.md) and
[`wowdps-gui-logic`](gui-logic.md), never the engine. Until the cutover
the daemon spawns its overlay only when config `gui_binary` names it
([wowdps-daemon](daemon.md)).

## Source

- Manifest: [`crates/gui-new/Cargo.toml`](../../../crates/gui-new/Cargo.toml)
- Root: [`crates/gui-new/src/main.rs`](../../../crates/gui-new/src/main.rs)
- Binaries: `wowdps-gui-new` (`wowdps gui-new` through the dispatcher)

## Seams

- **`Session`** (`session.rs`) is one daemon link and its `ClientState`
  as an entity, and `pump` is all of its work: reconnect without waiting,
  drain, apply, send, and notify when anything arrived. The app runs it
  every 100 ms; the link is a trait, so tests drive the
  daemon's mock through the same `pump` without a timer[^spec].
- **The window connects before the app starts**, as the iced one does:
  `DaemonClient::connect` may spawn a daemon and wait for it, which the UI
  thread must never do once frames draw.
- **The overlay opens through GPUI, not `gpui_kit::open_window`.** Kit's
  Root paints the theme's ground under its content and, on a
  client-decorated surface (a layer surface always is), adds a 20 px
  shadow border that it also claims as the client inset. A 28 × 96 tab
  mapped as 68 × 136 through it[^gpui].
- **The overlay is an edge strip** (`overlay/`, phase 2). Its layer
  surface spans the whole edge and places its content at an offset; the
  input region is exactly the content, so the rest is click-through, and
  "hidden" is a 1 px strip with an empty region. A surface opened with a
  zero length dies on a `wp_viewport` protocol error, so it opens at the
  output's real length (Hyprland's, which knows rotation and scale). An
  edge change opens the new surface before closing the old.
- **The window is one entity** (`window.rs`, phase 3) over the fight on
  the stage, `Gui::fight` — the log's `ClientState`, or a stored pull's
  own — and every gesture acts through `act`, which routes to whichever
  it is. The inspector (`window/inspector/`) is a pure model built once a
  frame and drawn by its views; its graph, matrices and chips are
  components of their own.
- **Keys are GPUI actions in contexts.** The meter's fire on
  `Meter && !Input`, so no meter key fires while a Kit `Input` has focus;
  a field's own Esc, arrows or Enter bind on `Filter > Input` or
  `Palette > Input`.
- **One scrollbar** (`scrollbar.rs`) for both surfaces, styled per
  surface: Kit's own hides itself; iced's lists always showed theirs.
- **Pixels are checked against iced's.** Each surface has ignored shot
  tests (`overlay_shots`, `window_shots`, `talent_shots`,
  `inspector_plot_shots`) rendered headless at iced's frames; the
  overlay's states are also pinned by `overlay_render_guard`, rendered
  without the art caches so its committed PNGs hold no Blizzard art.
- **Built per package.** CI and the flake build it in its own cargo
  invocation: a workspace-wide one would unify the features it shares
  with the iced GUI (`image`, `swash`, `wayland-client`). The flake's
  `.#wowdps-gui-new` has its own crane dependency layer, and the modules'
  `guiNewPackage` puts it on the daemon's `PATH`.
- **Libraries.** It links libxkbcommon(-x11), libxcb and fontconfig, and
  dlopens wayland-client, vulkan and EGL, the same three as the iced GUI.
  `build.rs` bakes the dev shell's `LD_LIBRARY_PATH` into the RUNPATH, so a
  binary built in the shell runs from anywhere.

## Contract

Public signatures and dependency policy: [`CONTRACT.md`](../../../CONTRACT.md).

[^spec]: `docs/spec-gui-new.md` §6: sessions, `pump`, views as entities, keys, ids as the test surface.
[^plan]: `docs/plan-gui-new.md`: phase 1 (step 1.1 the crate, 1.2 the `Session` and first harness test, 1.3 spikes S1–S12).
[^gpui]: `docs/gpui/README.md`: the findings section, with Kit's Root on layer surfaces added in step 1.1.
