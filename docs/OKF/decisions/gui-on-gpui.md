---
type: Decision
title: Rebuild The GUI On GPUI Kit, Beside The Iced One
description: 'The window and the overlay are to be rebuilt on Zed''s GPUI through GPUI Kit''s styled layer as crates/gui-new, run beside the iced crate until a measured cutover, with the framework-free logic moved into a shared crates/gui-logic, one theme definition feeding Kit''s Theme and the app''s Look, and Kit''s own click-through harness for interaction tests.'
tags: [gui, design, dependencies]
status: stable
generated: { by: claude-code/opus-5.5, at: 2026-09-30T20:40:00-07:00 }
sources:
  - id: spec
    resource: ../../spec-gui-new.md
    title: gui-new specification — platform, policy, shared crate, architecture, themes, overlay, window, testing, cutover
  - id: plan
    resource: ../../plan-gui-new.md
    title: gui-new implementation plan — six phases, spikes S1–S12, the devil's-advocate review log
  - id: gpui
    resource: ../../gpui/README.md
    title: The GPUI reference — which GPUI, what the 0.3.7 sources confirm, the test-harness survey
  - id: contract
    resource: ../../../CONTRACT.md
    title: CONTRACT.md §Dependencies — the gui-new and gui-logic clauses
---

**Where:** the planned crates `crates/gui-new` (binary `wowdps-gui-new`) and
`crates/gui-logic`, which replace [`wowdps-gui`](../crates/gui.md) at
cutover; the dependency policy[^contract]; a `gui_binary` key for the overlay
supervisor in [`wowdps-daemon`](../crates/daemon.md); the
[fetch-gpui-docs](../tools/fetch-gpui-docs.md) mirror behind the reference
guide[^gpui].

## Context

iced was chosen in 2026-08 because `iced_layershell` was the only maintained
native wlr-layer-shell binding, and the overlay is the destination. That
premise no longer holds. `gpui-pre`, the GPUI snapshot GPUI Kit pins
(`=0.3.7` for Kit 0.7.0; crates.io's own `gpui` is 0.2.2 from 2025-10 and
has none of this), has layer-shell windows, `Window::set_input_region` for
click-through and `display_id` as a layer surface's output.

The iced GUI is 57 k lines. Its costs are structural:
- one message enum and a hand-routed keymap;
- a custom widget just to ellipsise a label;
- `responsive` measuring tricks;
- interaction tests that can only look through rendered screens.

The window redesign is finished
([Redesign The Window From A Prototype](window-redesign.md)). Its prototype,
Tokens and step records describe exactly what to rebuild. Several themes are
planned, and Kit's styled components override through `Styled` and read a
runtime `Theme`.

## Decision

Decided with the user on 2026-09-30[^spec]:

- **Side by side, then rename.** gui-new builds `wowdps-gui-new`. The daemon
  keeps spawning `wowdps-gui` unless `gui_binary` names the new one. Cutover
  (delete the iced crate, take the name) waits on four measured criteria:
  parity, a raid week on the new overlay, resource use no worse, and the
  user's sign-off.
- **A shared crate, moved and never copied.** Config, Hyprland IPC, the
  overlay's takeover socket, the keybinding table, history pages, the
  ability tree, the icon caches and the bundled fonts move into
  `gui-logic`. The iced overlay's snapshot guard proves each move changed
  nothing, so while two GUIs run there is one `config.toml` writer, one
  takeover socket and one keymap the TUI's parity test iterates.
- **GPUI Kit's styled layer**, pinned exactly and restyled per control. A
  control falls back to the `gpui-base` primitive only where a component
  cannot reach the prototype.
- **One theme definition feeds Kit's `Theme` and the app's `Look`.** It
  generalises [the Look decision](gold-chrome-and-a-look-per-surface.md):
  no surface draws a literal colour. The built-in `gold` definition is the
  prototype's Tokens, and class chrome
  ([one accent from the class colour](one-accent-from-the-class-color.md))
  becomes an accent override on any theme.
- **The overlay first, same look, new engine.** gpui-pre sets a layer
  surface's margin only at creation, so the surface spans its edge's length
  and the input region follows the content. Spike S3 measures that before
  phase 2 depends on it.
- **Tests click, not just look.** Kit's `gpui_kit::test::TestWindowExt`
  drives the real views over the daemon's mock. Tests whose answer depends
  on text metrics run in `HeadlessAppContext`, because the default test
  platform's text system is a stub. The render guard compares with a
  tolerance, because the weekly lock update moves Mesa.
- **No forked or patched GPUI**: [No Forked Or Patched GPUI](no-gpui-forks.md).

Twelve spikes (S1–S12: layer shell, input region, the edge strip, output
choice by the display's UUIDv5, headless renders and real text, fonts,
images, canvas, build, lints, keys, theming) gate the rest of the
plan[^plan].

## Consequences

- **Two GUIs for months.** The shared crate keeps them from drifting, but a
  view-only fix to the iced GUI is noted for the gui-new step that ports
  that view.
- **A much larger dependency tree.** GPUI is about 530 packages, and CI
  builds it on every PR. The policy admits the Kit unit whole; our code
  still names only serde/toml and `image`[^contract].
- **The overlay cannot match iced byte for byte.** It is matched by eye at
  1:1 against the iced guard's pictures and gets its own guard.
- **The redesign's records keep binding the window.** This changes the
  engine, not the design.
- **GPUI bumps are deliberate PRs.** The mirror's digests make them
  reviewable.
- **The plan was reviewed adversarially before any code**
  (one blocker, ten majors, all folded in). Its review log is the list of
  traps to remember.

Landed as documents in `7045743` (the GPUI reference), `bdff9f3` (spec and
plan) and `4d33cee` (the dependency policy).
The overlay supervisor's `gui_binary` key landed in `c961ea5`, and the
dev unit stamping only that GUI in `1eac50e`.

[^spec]: `docs/spec-gui-new.md` §§1–13, especially §3–§6.1 and §11.
[^plan]: `docs/plan-gui-new.md`: phases 0–5, the spike table, the review log and the revision note.
[^gpui]: `docs/gpui/README.md`: which GPUI, findings checked against the 0.3.7 sources, the harness survey.
[^contract]: `CONTRACT.md` §Dependencies: the gui-new and gui-logic clauses, signed off 2026-09-30.
