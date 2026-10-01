---
type: Crate
title: wowdps-gui-new
description: 'The window and the overlay being rebuilt on Zed''s GPUI through GPUI Kit, beside the iced wowdps-gui until a measured cutover; a pure client of model, proto and gui-logic, so far a skeleton whose window shows the daemon''s Status and whose --overlay opens an empty layer surface.'
resource: crates/gui-new
tags: [crate, gui]
status: stable
generated: { by: claude-code/opus-5.5, at: 2026-09-30T22:07:00-07:00 }
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
  every 100 ms; step 1.2 makes the link a trait so tests drive the
  daemon's mock through the same `pump` without a timer[^spec].
- **The window connects before the app starts**, as the iced one does:
  `DaemonClient::connect` may spawn a daemon and wait for it, which the UI
  thread must never do once frames draw.
- **The overlay opens through GPUI, not `gpui_kit::open_window`.** Kit's
  Root paints the theme's ground under its content and, on a
  client-decorated surface (a layer surface always is), adds a 20 px
  shadow border that it also claims as the client inset. A 28 × 96 tab
  mapped as 68 × 136 through it[^gpui].
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
