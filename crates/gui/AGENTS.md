# crates/gui (wowdps-gui)

The meter window and, with `--overlay`, an overlay surface over the game —
a wlr-layer-shell surface on Linux, a floating panel on macOS — on
Zed's GPUI through GPUI Kit. A pure client of model, proto and gui-logic:
it cannot parse a log. Changing a window surface or the talent viewer:
read `crates/gui/src/window/AGENTS.md` too.

Read before the work it covers:

- `docs/gpui/README.md` before any GPUI work. Its mirror under
  `docs/gpui/` is gitignored, so ripgrep skips it: `grep -rn … docs/gpui/src/`.
  `tools/fetch-gpui-docs.sh [kit-version]` regenerates it.
- `crates/gui/SHOTS.md`: the shot tests and the render guard.

## Rules

- **Logic lives in gui-logic.** Anything computed without drawing (config,
  Hyprland IPC, keys, models, words, geometry) belongs to
  `crates/gui-logic`, with its tests. The GUI brings a text measure and its
  paint.
- **Stock GPUI only.** `gpui-kit` is pinned exactly (`=0.8.0`) with its
  default features and no `tree-sitter*`; no `[patch]`, no fork. Design
  around a missing capability or contribute it upstream. A GPUI or Kit bump
  is its own change. Our code names no transitive crate except serde/toml
  and `image` (only to build `RenderImage` frames).
- **No literal colors, faces, corners or effects.** Each comes from the
  theme `Def` (gui-logic's `theme`); a new color is a token on every `Def`.
  Navy's pixels are the prototype's: a new token, effect or corner role
  leaves them unmoved (the overlay render guard and the navy window shots
  are the check).
- **Every animation is finite.** GPUI re-renders the whole root view on
  each animation frame; an endless pulse idled the window at 5.7 % of a
  core. Key an animation to its trigger and let it settle. Anything that
  must keep moving runs on a timer at the slowest rate that reads as motion.
- **Reduced motion settles to parity.** Motion beyond the prototype is gone
  under `cx.reduce_motion()`, which every shot test sets.
- **Kit's Root is for the window.** The overlay opens through
  `cx.open_window`, never `gpui_kit::open_window`: on a layer surface Root
  adds a 20 px shadow border and paints the ground.
- **Zoom by hand.** GPUI has no app scale factor: window sizes go through
  `w.z(…)`, overlay sizes through `ov.z(…)`.
- **One image per tile.** An art tile becomes one `Arc<RenderImage>` on
  first use (`images.rs`), so GPUI uploads it once; never build a frame per
  render. The art caches are optional: every surface draws without them.
- **Never block a frame.** `main.rs` makes the daemon link before GPUI
  starts; UI changes go through `Session::act`, one-shot reads through
  `Session::request`.
- **Tests drive surfaces like a user:** Kit's `TestWindowExt` over the
  daemon's mock through the `Session` link (`testkit.rs`); a test calls
  `pump` itself and runs no timer.
- **Text metrics need real text.** Kit's default `TestAppContext` stubs
  text (every glyph 0.6 em). A test whose answer depends on widths,
  heights, truncation or the chrome budget runs in `testkit::headless()`.
- **Build in the dev shell.** `build.rs` bakes the shell's
  `LD_LIBRARY_PATH` into the RUNPATH; a GUI built outside it cannot find
  Wayland or Vulkan.

## The overlay

- On Linux gpui-pre sets a layer surface's margin only at creation, so the
  surface spans its whole edge, the content sits at an offset inside it, and
  the input region is exactly the content. Hidden is a 1 px strip with an
  empty region. Never ask for a zero length: it is a protocol error.
- On macOS the surface is a floating GPUI panel (`WindowKind::PopUp` — a
  non-activating NSPanel, above all Spaces) exactly the content's size:
  there is no input region, so the strip's spanning surface would eat the
  game's clicks along the edge. Hidden is a 1 px window. A size change
  re-anchors (the `Reanchor` event reopens; a `resize` keeps the wrong
  corner fixed and an origin cannot move), and a drag slides nothing — the
  offset stays as configured.
- Its output/display is chosen before the app starts: Linux finds it by
  recomputing GPUI's UUIDv5 of the output name
  (`gui_logic::output::output_uuid`), macOS by the display's 1-based index
  in GPUI's order (`primary` is 1); never by bounds.
- It has no keyboard: every gesture is a click, a right press or a wheel.
- It is single-instance: a new `--overlay` launch evicts the running one.
  Never kill an overlay by a PID from earlier output.
- **Nothing it prints may panic.** A spawned overlay's stderr is a pipe to
  the daemon that spawned it, and `eprintln!` panics once that daemon is
  gone. Runtime notices use `writeln!(std::io::stderr(), …)` and drop the
  result; only `main.rs`'s answers to arguments the daemon never passes
  print with the macros.
- Debug aids: the `WOWDPS_OVERLAY_*` variables (`overlay/panel/autos.rs`,
  `docs/tracing.md`).
- **The render guard.** Before merging anything that touches the overlay
  or gui-logic's overlay models, run it by name on the machine that blessed
  it (it draws in the system face). An intended change re-blesses with
  `WOWDPS_BLESS=1` and commits the new PNGs under `snapshots/overlay/`. It
  renders without the art caches, so the committed pictures hold no game art.

## Shots

Ignored tests render through GPUI's headless renderer over the daemon's
mock: no window, no daemon, safe beside a running game. Prefer them to
launching the GUI.

```sh
WOWDPS_SHOTS_DIR=/tmp/shots cargo test -p wowdps-gui window_shots -- --ignored --nocapture
WOWDPS_SHOTS_DIR=/tmp/shots cargo test -p wowdps-gui overlay_shots -- --ignored
cargo test -p wowdps-gui overlay_render_guard -- --ignored
WOWDPS_SHOTS_THEME=onyx WOWDPS_SHOTS_DIR=/tmp/onyx cargo test -p wowdps-gui window_shots -- --ignored
```

Also `talent_shots`, `inspector_plot_shots`,
`the_chrome_budget_holds_on_the_log` and `real_dataset_draws_every_spec`.
Shots over a real log or a frozen history store (`WOWDPS_SHOTS_LOG`,
`WOWDPS_SHOTS_HISTORY`) hold real names: write them outside the checkout
and never commit them.
