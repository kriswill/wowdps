---
type: Crate
title: wowdps-gui-logic
description: 'The logic the iced GUI and gui-new share and neither draws — the theme''s names, table columns, and (as wave A lands) config, Hyprland IPC, keys, history pages and the icon caches — moved out of crates/gui, never copied.'
resource: crates/gui-logic
tags: [crate]
status: stable
generated: { by: claude-code/opus-5.5, at: 2026-09-30T20:55:00-07:00 }
sources:
  - id: spec
    resource: ../../spec-gui-new.md
    title: gui-new specification — §5, the shared crate and its two waves
  - id: plan
    resource: ../../plan-gui-new.md
    title: gui-new implementation plan — step 0.2, wave A, one commit per move
---

What the wowdps GUIs share and neither draws: the logic the iced
[`wowdps-gui`](gui.md) and its GPUI successor `crates/gui-new` both run while
they coexist, and the successor keeps after the cutover
([Rebuild The GUI On GPUI Kit](../decisions/gui-on-gpui.md)). Model + proto +
serde/toml, no UI framework.

## Source

- Manifest: [`crates/gui-logic/Cargo.toml`](../../../crates/gui-logic/Cargo.toml)
- Root: [`crates/gui-logic/src/lib.rs`](../../../crates/gui-logic/src/lib.rs)

## Seams

**Moved, never copied.** Each module leaves `crates/gui` in a commit of its
own and the GUI re-imports it at its crate root (`use
wowdps_gui_logic::config;`), so the GUI's call sites keep their
`crate::config::…` paths and the move reads as a rename. The iced overlay's
snapshot guard and a before/after design-shot run prove each move changed
nothing but where the code lives[^plan]. A piece that draws stays in its
GUI: where a type has both, the meaning moves and the pixels stay behind as
an extension trait in the GUI.

**What lives here** (wave A, plan step 0.2[^spec]):

- `config` — the one reader and writer of `~/.config/wowdps/config.toml`
  for both GUIs: every save atomic, and a casual gesture's key written
  alone through `Config::store_*`, so one GUI's launch-time copy never
  overwrites the other's drag.
- `fold` — accent folding for the row filter and the command palette
  ("akanos" finds Akanôs; Latin-1 and Latin Extended-A only).
- `hypr` — Hyprland IPC: the game window, its workspace and monitor, the
  cursor, and the event stream the overlay follows the game's workspace
  by. Its `fake` (a scratch Hyprland socket pair) is test support.
- `simc` — the SimulationCraft addon export's parser, and the raw paste
  persisted per "Name-Realm" under `$XDG_DATA_HOME/wowdps/simc/`.
- `single` — the overlay's takeover socket. It is unversioned, so a new
  overlay of either GUI evicts a running one of either.
- `theme` — the names the config spells: `Chrome`, `Density`, `CLASSES`,
  `class_named`. The iced GUI's `theme::DensityPitch` gives a density its
  row pitch and padding.
- `table` — what a column means: `Col`'s heading, cell text and sort key,
  `sorted`, `figure`, `overheal_pct`, and `split_pet` for an ability's
  "Ability (Pet)" label. The iced GUI's `table::ColDraw` gives a column its
  width and ink.

**Test hooks cross the crate line by feature.** A `#[cfg(test)]` item here
is compiled for this crate's own tests only, so a hook a GUI's tests call
(`Config::use_path_on_this_thread`, `simc::use_dir_on_this_thread`,
`hypr::fake`) is
gated `cfg(any(test, feature = "test-support"))`, and each GUI turns the
feature on from its dev-dependency; a release build never carries it.

## Contract

Public signatures and dependency policy: [`CONTRACT.md`](../../../CONTRACT.md).

[^spec]: `docs/spec-gui-new.md` §5: the shared crate, wave A (whole modules) and wave B (split out by the phase that needs it).
[^plan]: `docs/plan-gui-new.md` step 0.2: the move order and its acceptance.
