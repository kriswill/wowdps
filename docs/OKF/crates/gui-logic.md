---
type: Crate
title: wowdps-gui-logic
description: 'The GUI''s framework-free half — config, Hyprland IPC, the keymap, history pages, the art-cache readers and fonts (wave A), then every model, word and geometry of the overlay, the window, the inspector, the rail, Home, the palette and the talent viewer (wave B) — moved out of the iced GUI, never copied, while it and its GPUI successor coexisted.'
resource: crates/gui-logic
tags: [crate]
status: stable
generated: { by: claude-code/opus-5.5, at: 2026-10-01T11:32:09-07:00 }
sources:
  - id: spec
    resource: ../../spec-gui-new.md
    title: gui-new specification — §5, the shared crate and its two waves
  - id: plan
    resource: ../../plan-gui-new.md
    title: gui-new implementation plan — step 0.2, wave A, one commit per move
---

What the GUI computes and never draws. It was carved out of the iced GUI
while that GUI and its GPUI successor ([gui-new](gui-new.md)) both ran, so
one config writer, one takeover socket and one keymap served both; since
the cutover [`wowdps-gui`](gui.md) (on GPUI) is its one drawing client, and
[the TUI's parity test](tui.md) reads its chord table
([Rebuild The GUI On GPUI Kit](../decisions/gui-on-gpui.md)). Model + proto
+ serde/toml, no UI framework.

## Source

- Manifest: [`crates/gui-logic/Cargo.toml`](../../../crates/gui-logic/Cargo.toml)
- Root: [`crates/gui-logic/src/lib.rs`](../../../crates/gui-logic/src/lib.rs)

## Seams

**Moved, never copied.** Each module left the iced GUI in a commit of its
own and that GUI re-imported it at its crate root, so its call sites kept
their paths and the move read as a rename. The iced overlay's snapshot
guard and a before/after design-shot run proved each move changed nothing
but where the code lives[^plan]. A piece that draws stays in the GUI: where
a type has both, the meaning lives here and the pixels in the GUI. The
GPUI GUI was written against these modules from its first step, and the
cutover changed none of them but their doc comments.

**What lives here** (wave A, plan step 0.2[^spec]):

- `config` — the one reader and writer of `~/.config/wowdps/config.toml`
  for the window and the overlay: every save atomic, and a casual
  gesture's key written alone through `Config::store` / `store_*`, so one
  surface's launch-time copy never overwrites the other's drag.
- `fold` — accent folding for the row filter and the command palette
  ("akanos" finds Akanôs; Latin-1 and Latin Extended-A only).
- `fonts` — the window's bundled OFL faces (`crates/gui-logic/fonts/`,
  with their provenance and the tabular bake in its `README.md`): `FONTS`,
  and the family names they register under. Assets, not dependencies.
- `history` — the history store as a GUI reads it: `Earlier`, the pages of
  stored fights the pull rail lists (`PAGE` cards a request, Home's too),
  and `Stored`, a stored pull fed to a `ClientState` of its own as
  synthetic snapshots built from `GetFight` answers, one read in flight.
- `hypr` — Hyprland IPC: the game window, its workspace and monitor, the
  cursor, and the event stream the overlay follows the game's workspace
  by. Its `fake` (a scratch Hyprland socket pair) is test support.
- `icons`, `spell_icons`, `talent_art`, `lazy_tiles` — the readers of the
  per-machine art caches ([gen-icons](../tools/gen-icons.md),
  [gen-spell-icons](../tools/gen-spell-icons.md),
  [gen-talent-art](../tools/gen-talent-art.md)): absent, truncated or
  garbage files answer `None`, never a panic. Each reader is generic over
  the handle a GUI makes of a tile (`Rgba` in, `H` out), memoized per
  tile, so the renderer is handed one image per tile, not a new one each
  frame; the GUI's handle is an `Arc<RenderImage>`, held in a static.
- `keys` — the keymap as data: `ACTIONS`, each `Chord` (a typed
  character, a Ctrl chord, a named key) and its core `Action`, which
  [the TUI's parity test](tui.md) iterates; the `?` sheet's `BINDINGS`
  by `Surface`; the zoom chords (`ZOOM_CHORDS`). The GUI's `keys.rs`
  registers them as GPUI `KeyBinding`s once at start.
- `sibling` — `daemon_bin`, the daemon the GUI starts when none runs: the
  `wowdps` beside its own binary, else the name on `$PATH` (moved in step
  1.1, when [gui-new](gui-new.md) needed it too).
- `simc` — the SimulationCraft addon export's parser, and the raw paste
  persisted per "Name-Realm" under `$XDG_DATA_HOME/wowdps/simc/`.
- `single` — the overlay's takeover socket. It is unversioned, so a new
  overlay evicts a running one whatever build either is.
- `table` — what a column means: `Col`'s heading, cell text and sort key,
  `sorted`, `figure`, `overheal_pct`, and `split_pet` for an ability's
  "Ability (Pet)" label; the GUI's `window/table.rs` gives a column its
  width and ink.
- `theme` — the names the config spells: `Chrome`, `Density`, `CLASSES`,
  `class_named` (and, from wave B, every token and measure below); since
  2026-10-03 the built-in themes (`navy`, `onyx`, `frost`, a file each), the
  `tokens!` groups a config names and `Registry`, which lays a config's
  `[themes]` over them ([themes as config data](../decisions/themes-as-config-data.md)).
- `fonts` — every bundled face: Barlow and Marcellus (`navy`), Saira
  Semi Condensed Tabular and Michroma (`onyx`).
- `tree` — [R26](../rulings/r26.md)'s ability-tree lines: what the inspector's
  list draws and the keys walk, and the overlay's drill rollups.

**Wave B** (split out by the phase that needed it, phases 2–4[^plan]): the
framework-free halves of what the iced GUI drew, each moved with its tests
and checked byte-identical against the iced design shots and the overlay
guard, then drawn by the GPUI GUI.

- The overlay (phase 2): `graph` (the drill graph's plot: marks, hover
  and probe words, the drag window), `drill` (the drill's columns, stat
  cards and recap words), `surface` (the edge strip's placement and
  `nearest_edge`), `output` (which output the overlay opens on), and
  `timeline` (the visit blocks the overlay's strip and the rail group a
  log by).
- The window's frame and stage (steps 3.1–3.2): `theme`'s measures and
  tokens (`defs`, `metrics`: sizes, pitches, shadows, every window and
  overlay colour), `glyph` (the line icons as data), `labels` (the
  window's words and view names), `fight_head`, `deaths`, `ribbon`,
  `axis`, `reveal`, the table's grid and `meter_step`, and `raid` (a
  synthetic 25-player raid, `test-support`).
- The inspector (step 3.3): `inspect` — the roster, `Fit`, `curves`,
  `nums`, `recap`, `lanes`, `stack`, `list` (columns, hues, the foe
  sphere), `plot`, `geometry` (the graph's every coordinate, hit test
  and tooltip placement; a renderer brings only text measure and paint)
  and `matrix` (R21's level-0 derivation and heat).
- The rail and Home (steps 3.4–3.5): `rail` (nights, visits, pulls, the
  trash rule, `Pull` stepping) and `home` (the week, panels, charts'
  geometry, the known characters).
- The command palette (step 3.6): `palette` — what it lists, how a query
  narrows it, the selection's steps; its fixtures ride `test-support`.
- The talent viewer (phase 4): `talents` — the layout, the editing state
  machine and the geometry the viewer draws from.

**Test hooks cross the crate line by feature.** A `#[cfg(test)]` item here
is compiled for this crate's own tests only, so a hook the GUI's tests call
(`Config::use_path_on_this_thread`, `simc::use_dir_on_this_thread`,
`hypr::fake`) is gated `cfg(any(test, feature = "test-support"))`, and the
GUI turns the feature on from its dev-dependency; a release build never
carries it.

## Contract

Public signatures and dependency policy: [`CONTRACT.md`](../../../CONTRACT.md).

[^spec]: `docs/spec-gui-new.md` §5: the shared crate, wave A (whole modules) and wave B (split out by the phase that needs it).
[^plan]: `docs/plan-gui-new.md` step 0.2: the move order and its acceptance.
