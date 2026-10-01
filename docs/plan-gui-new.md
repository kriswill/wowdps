# gui-new — implementation plan

Scope: deliver `docs/spec-gui-new.md`. Six phases; every numbered step is
one PR that leaves `cargo test` green and the overlay the daemon spawns
untouched (it stays the iced one until phase 5). Phases 0–1 are small and
decide whether the rest goes ahead as written; phase 3 is most of the work.

| Phase | What | Size | Gate to leave it |
| --- | --- | --- | --- |
| 0 | Groundwork in the iced tree: policy, docs, `gui-logic` wave A, `gui_binary` | M | iced guard unchanged, tests green |
| 1 | gui-new skeleton, `Session`, first harness tests, spikes S1–S12 | M | every spike meets its exit criterion, or the user picks an option |
| 2 | The overlay, at parity | L | every overlay guard state reproduced (its raid week runs alongside phase 3 and gates phase 5) |
| 3 | The window, in the redesign's order | XL | every design-shot state reproduced and reviewed |
| 4 | The talent viewer | L | every spec of the real dataset lays out |
| 5 | Cutover | S | spec §10's four criteria |

## 0. What already exists (don't rebuild)

- **`ClientState`** (`crates/proto/src/state.rs`) with the window's opt-in
  capabilities (`set_follow`, `open_death`, `select_player`,
  `select_death`, `view_answered`, `snapshot_gen`, `compare_view`, `log_id`).
  gui-new is another renderer of it; it adds nothing to it unless a step
  says so.
- **`MockDaemon`** (`crates/daemon/src/mock.rs`, `fixture_at`) — the test
  daemon over the real engine and fixtures, plus the `MemBackend` store.
- **The design**: `docs/design/window-redesign.html` (Tokens, CSS, JS), the
  decision records under `docs/OKF/decisions/`, the reference renders under
  `~/.local/share/wowdps/design-shots/reference/`, the frozen night slice and
  store under `~/.local/share/wowdps/design-shots/`.
- **The overlay's definition**: `crates/gui/src/overlay/guard.rs`'s states
  and `WOWDPS_GUARD_PNG=<dir>` to dump the iced pictures to compare against.
- **GPUI docs**: `docs/gpui/README.md` + the mirror
  (`tools/fetch-gpui-docs.sh`).
- **The per-machine caches** (`class-icons.bin`, `spell-icons.bin`,
  `talents.json`, `talent-art.bin`) and their formats — unchanged; gui-logic
  reads them for both GUIs.

## Phase 0 — groundwork (iced tree only, no GPUI)

### 0.1 Policy and docs

- CONTRACT.md §Dependencies: the amendment in spec §4 (user sign-off is the
  merge condition).
- Commit `docs/gpui/README.md`, `docs/gpui/.gitignore`,
  `tools/fetch-gpui-docs.sh`, `tools/rustdoc-digest.jq`, the spec and this
  plan.
- Knowledge bundle: a decision record `gui-on-gpui` (the spec §11 table,
  linking `window-redesign`), a `no-gpui-forks` decision, the Tool doc the
  scaffolder makes for `fetch-gpui-docs` re-tagged (`docs`, not
  `game-data`); `okf validate` exits 0.
- CLAUDE.md: one paragraph pointing at the spec, the plan and
  `docs/gpui/README.md`.

`[skip ci]` — docs only.

### 0.2 `crates/gui-logic`, wave A

New workspace member `crates/gui-logic` (`wowdps-gui-logic`: model + proto +
serde + toml; dev: `wowdps-daemon`, `wowdps-core`; workspace lints). One
commit per move, in this order (each leaves the tree green):

0. The pieces the "whole" modules lean on (review finding 1):
   `theme::{Chrome, Density}` as name-only enums + `class_named` (the
   GUI keeps `Density::row_h` / `pad` as an extension trait over
   `theme::pitch`), `home::PAGE`, `table::{Col, sorted}`,
   `inspector::list::split_pet`.
1. `config.rs` — `crates/gui` keeps a `pub use` shim at the old path for the
   commit, removed when every call site is updated.
2. `fold.rs`, `simc.rs` (with its persistence dir), `single.rs`.
3. `hypr.rs`.
4. `history.rs` (its one test needing `fight_head::badge` stays in
   `crates/gui`), `inspector/tree.rs`.
5. `keys.rs`: a STRUCTURED chord table in gui-logic (`Chord` → action,
   `BINDINGS`' display rows, `Surface`); `crates/gui`'s `action_for` becomes
   iced key → `Chord` → table lookup, with a unit test for that mapping
   there. `crates/tui/tests/keybind_parity.rs` stops grepping
   `gui/src/keys.rs`'s match arms (they no longer exist) and iterates the
   table: the TUI gains gui-logic as a dev-dependency.
6. The four cache readers (`icons`, `spell_icons`, `talent_art`,
   `lazy_tiles`): decode to an RGBA tile type, and move the cache GENERIC —
   `Tiles<K, H: Clone>` with a `make: FnOnce(Vec<u8>) -> H` — so `crates/gui`
   still caches `image::Handle`s. Building a handle per call would give iced
   a new image id every frame and re-upload the texture, with identical
   pixels the guard cannot see; check it with a frame-count probe, not the
   guard.
7. The bundled fonts: `git mv crates/gui/fonts crates/gui-logic/fonts`
   (README and provenance with them) and `theme::FONTS` becomes gui-logic's
   `pub const FONTS: &[&[u8]]`, which `window::settings()` loads as before.
   Both GUIs load the same bytes, and nothing has to move at cutover.

Acceptance, every commit: `cargo test` (workspace), `cargo clippy`,
`cargo fmt --check`; at the end of the step, the iced overlay guard run
ALONE passes with its hashes untouched
(`cargo test -p wowdps-gui overlay_snapshot_guard -- --ignored`), and one
design-shot run compared against a pre-step run (same inputs, pixels equal).
Each move updates the paths CLAUDE.md and the knowledge bundle cite
(`crates/gui/fonts`, `gui/src/hypr.rs`, `gui/src/keys.rs` …) in the same
commit, and the bundle gains a crate doc for `gui-logic`; `okf validate`
exits 0.

The live risk is the iced GUI the user raids with: release-build it and
restart through the dev unit only between raid nights.

**As built (2026-09-30, eight commits).** Where the build departed from the
text above:

- **No shims.** `crates/gui` imports each moved module at its crate root
  (`use wowdps_gui_logic::{config, …};`) and re-exports a moved piece from
  its old module path (`theme::Chrome`, `table::sorted`,
  `inspector::tree`). That import is permanent, not a shim to unwind, and
  every call site kept its path, so each move reads as a rename.
- **`home::PAGE`** moved with `history` (step 4), not in step 0. history
  owns it; `home.rs` re-exports it.
- **`table::Col`** split by meaning. Its heading, cell text and sort key
  moved, with `sorted`, `figure`, `overheal_pct` and `split_pet`. Its
  width and ink stay in the GUI as the `ColDraw` extension trait, as
  `Density`'s pitches stay as `DensityPitch`.
- **Test hooks need a feature.** A `#[cfg(test)]` item in gui-logic is
  invisible to the GUI's tests. So `Config::use_path_on_this_thread`, the
  simc paste store's `use_dir_on_this_thread` (the paste store moved out
  of `talents.rs` into `simc`) and `hypr::fake` are gated on a
  `test-support` feature that the GUI enables from its dev-dependency.
  The fake allows `unwrap_used` and `indexing_slicing`, since clippy.toml's
  test exemptions do not reach library code.
- **history's badge test was split.** Its routing assertions stayed with
  `history` (now checking the state the badge reads), and the GUI gained
  `fight_head::tests::a_stored_kill_reads_as_a_kill`.
- **keys.** `Chord` is `Char(&str)` (the modified character), `Ctrl(&str)`
  or `Named(Named)`, in iced's key names. Ctrl matters on characters only,
  as before. The table is `keys::ACTIONS`, and `zoom_for` reads chords too.
  The parity test compares an action's spelling: the TUI arm's text, minus
  `View::`, against the table's `Debug`.
- **The caches** hand out tiles as `lazy_tiles::Rgba` through `make:
  FnOnce(Rgba) -> H`. The probes are unit tests: gui's
  `a_tile_is_one_iced_image_across_frames` compares iced image ids, and
  gui-logic's memo tests count `make` calls.
- **Fonts.** gui-logic's `fonts` module holds `FONTS` (still a
  `[&[u8]; 4]`) and the family names `UI_FAMILY` and `TITLE_FAMILY`, which
  gui-new needs as much as the bytes.
- **Acceptance.** The design shots' source fingerprint now covers
  `crates/gui-logic`. Before and after sets are under
  `~/.local/share/wowdps/design-shots/gui-logic-{before,after}`.

### 0.3 `gui_binary`

- `crates/daemon/src/config.rs`: flat key `gui_binary` (default
  `wowdps-gui`). A bare name keeps today's rule (`crates/daemon/src/lib.rs`
  109–114): the sibling of the daemon binary if it exists, else the name on
  `$PATH`, which the home-manager and NixOS modules rely on. A value with a
  `/` is a path. **No `Status` change** — naming the binary there would be a
  wire change; a spawn failure already carries the path in `Failed`.
- `tools/dev-unit.sh` and `tools/dev-unit/wowdps-dev.path`: stamp and watch
  `wowdps` plus the CONFIGURED `gui_binary` only (watching both GUIs would
  bounce the live daemon and overlay on every gui-new build); widen
  `stop_daemon`'s pkill to `wowdps-gui(-new)? --overlay$`; warn about a
  missing configured binary rather than a missing `wowdps-gui`.
- Tests: the daemon's config tests (key parsed, default, name vs path,
  `$PATH` fallback) and the overlay supervisor test with a named binary.

**As built (2026-09-30).** Where the build departed from the text above:

- **Resolution.** `Config::gui_bin(exe)` holds the rule, so it is tested
  without a daemon. An empty `gui_binary` means the default, as nothing
  could spawn from it.
- **The watch.** Only the configured GUI is STAMPED. The path unit watches
  both GUI names in both profiles, because a path unit cannot follow a
  config edit without a reinstall. The stamp is what keeps the daemon up:
  a build of the GUI it does not spawn fires a reload that restarts
  nothing. A `gui_binary` path outside `target/` is stamped, not watched.
- **The stamp names each binary's path.** A `gui_binary` switch changes
  the stamp even between two missing binaries. So the first watched build
  after this lands restarts the daemon once, since the stamp format
  changed. Re-run `tools/dev-unit.sh install` once, so the path unit
  watches gui-new's binaries.
- **The dev unit reads the key itself.** `gui_binary()` in the script is an
  awk twin of the daemon's reader. It was checked against `Config::parse`
  on fifteen inputs: comments, sections, empty and unquoted values, the
  last key winning, `#` and spaces inside a path. The one difference is an
  escape (`\\`, `\"`), which the script reads as the default.
- **Tests.** The supervisor test runs a named sibling script, then `true`
  (found on `$PATH`: it runs and exits, which is not a spawn error), then
  an absent name, whose `Failed` reads `spawning <name>: …`. The
  production options test checks that a path value reaches `gui_bin`.

## Phase 1 — skeleton and spikes

### 1.1 The crate

- `crates/gui-new` (`wowdps-gui-new`, bin `wowdps-gui-new`):
  `gpui-kit = "=0.7.0"` (default features: `component`, `assets`; no
  `tree-sitter*`), `image`
  (`default-features = false`, `gpui-pre-wgpu`'s locked version, for
  `RenderImage` frames only), serde, toml, model, proto, gui-logic; dev:
  `gpui-kit` with `test-support`, `wowdps-daemon`, `wowdps-core`.
- `build.rs`: the RUNPATH bake `crates/gui/build.rs` does, for the libraries
  GPUI dlopens (wayland, vulkan-loader, libxkbcommon — S9 makes the list
  exact).
- Root `Cargo.toml`: `[profile.dev.package]` opt-level 3 for the gpui-pre
  family, gpui-base, gpui-component, gpui-kit, taffy, rustybuzz/swash,
  ttf-parser — the set
  S9 measures as worth it.
- Nix: `nix/dev/env.nix` gains what S9 finds (Vulkan loader + Mesa for
  lavapipe, fontconfig, wayland, libxkbcommon, x11 libs); `wrappers.nix` a
  `wowdps-gui-new` wrapper; `contract.nix` checks it; the flake a
  `.#wowdps-gui-new` package with its crane dependency layer
  (`.#wowdps-gui-new-deps`); the home-manager / NixOS modules an option to
  put it on the service's `$PATH`; CI builds and tests it on PRs, per
  package (a workspace-wide build would unify `image`'s features into the
  iced binary).
- `main.rs` opens the window with `gpui_kit::open_window` and shows the
  daemon's `Status` through a `Session`; `--overlay` opens an empty layer
  surface. No `unwrap`/`expect`: `open_window`'s `Result` is reported and
  the process exits non-zero.

**As built (2026-09-30).** Where the build departed from the text above:

- **Dependencies arrive with their first use.** The manifest names
  `gpui-kit`, model, proto and gui-logic. `image` comes with S7 or phase 2,
  serde/toml when gui-new reads a file itself, and the dev-dependencies
  with 1.2's tests.
- **`daemon_bin` moved.** The iced GUI's "sibling `wowdps`, else `$PATH`"
  helper is gui-logic's `sibling::daemon_bin`, and both GUIs import it.
- **A `Session` already.** The window's status needs one, so `session.rs`
  holds the real link: `pump` on a 100 ms timer, `GetStatus` once a
  second (the daemon never broadcasts it), and a reconnect that never
  waits. Step 1.2 turns the link into a trait and adds the tests.
- **The overlay skips Kit's Root.** Kit's Root paints the theme's ground
  and, on any client-decorated surface (a layer surface always is), a
  20 px shadow border set as the client inset, so a 28 × 96 tab mapped as
  68 × 136. The overlay opens its view with `cx.open_window`. It has no
  daemon link and claims no takeover socket: an Overlay-kind session
  would tell the supervisor an overlay is up, and an empty surface is not
  one yet.
- **Libraries, measured.** It LINKS libxkbcommon(-x11) and libxcb, and
  fontconfig is probed at build time. The dev shell gained fontconfig and
  libxcb. It DLOPENS wayland-client, vulkan and EGL, the iced GUI's three,
  so `env.nix` is unchanged. `LD_DEBUG` also showed libdbus, but that was
  the NVIDIA driver's own dlopen. Without `LD_LIBRARY_PATH` the debug
  binary runs on its baked RUNPATH; the package runs through its wrapper,
  because nix's fixup shrinks the RUNPATH to what the binary links.
- **Opt-levels.** Kit's recommended set, plus `gpui-pre-linux`,
  `gpui-pre-wgpu`, `gpui-base` and `swash`, until S9 measures them.
- **Numbers so far.**
  - The lockfile goes from 611 to 1,081 packages.
  - `cargo audit` still exits 0. The new warnings are three unmaintained
    crates (`instant`, `rustls-pemfile`, `rustybuzz`).
  - A debug build of the whole tree took about 30 s on this machine, and a
    first release build about 45 s, warm registry. The debug binary is
    830 MB with debug info; the release one is 50 MB, with a 116 MiB
    closure.
- **Nix and CI.**
  - `.#wowdps-gui-new` and `.#wowdps-gui-new-deps` build, and the 2 unit
    tests pass in the sandbox.
  - The modules gain `guiNewPackage` (null by default). The NixOS module
    was evaluated with and without it.
  - CI's workspace clippy and test exclude gui-new, and each gets a gui-new
    step of its own. The nix job still builds only `.#wowdps`, as it never
    built `.#wowdps-gui`.
  - The stable canary installs `libfontconfig-dev` for the build script
    clippy runs.
- **Checked live on a headless Hyprland output.**
  - The window, debug and packaged, showed the running daemon's status and
    the `ClientState`'s segment count.
  - `--overlay` mapped 28 × 96 on the overlay layer, flush with the focused
    monitor's right edge.
  - A release build through the new wrapper wrote
    `target/release/wowdps-gui-new`, which fired the dev unit's reload
    twice. Neither restarted the daemon, because `gui_binary` names
    `wowdps-gui` (step 0.3's gate, live).

### 1.2 `Session` and the first harness test

- `session.rs`: the link trait (`DaemonClient` / `MockDaemon`) and
  `Session::pump` — drain, apply, send, `cx.notify()` on any non-empty drain
  (spec §6: not on `snapshot_gen`, which `CompareSnapshot` / `SegmentList`
  never move). The app runs `pump` on a `TICK` timer; tests call it
  themselves and spawn no timer, so `run_until_parked` parks.
- A test that opens a minimal meter over `MockDaemon::fixture_at(sample)`,
  `click`s the second row by its guid-keyed id (`ElementId::NamedChild`),
  `pump`s, and asserts the selection through `find(…).selected()` and the
  `Session`'s `ClientState`; and its twin in `HeadlessAppContext` with a real
  text system, asserting a row's real height. These are the two templates
  every later test copies.

**As built (2026-09-30).**

- **`Link`** is the trait (`send`, `poll`, `reconnect`), implemented by
  `DaemonClient` and, in `testkit`, by `MockLink` over `MockDaemon`.
  `MockLink` buffers what a send answers until the next poll, as the
  socket would.
- **Two constructors.** `Session::new(link)` has no timer; tests hold
  it. `Session::running(link, cx)` adds the `TICK` loop and asks for the
  status once a second. `pump` drains until quiet, at most 8 rounds,
  because a mock answers a follow-up request inline. Every UI change goes
  through `Session::act(|state| …)`, which sends what the state asks
  for.
- **The real-text route needs no `gpui-pre-wgpu`.**
  `gpui_kit::platform::current_platform(true).text_system()` is the
  Linux headless platform's `CosmicTextSystem`; Kit's own rendering test
  uses it. `testkit::headless()` builds a `HeadlessAppContext` over it,
  plus Kit's assets and the headless renderer.
- **The two templates** are in `meter.rs`.
  - `clicking_a_row_selects_its_player` clicks the second row by its
    guid's id. It asserts `selected()` on both rows and `row_sel` in the
    state, and fails when the click handler is disabled.
  - `a_row_lays_out_real_text` checks that a label is as wide as its text
    shaped in the theme's font (±1 px), that this is not the stub's
    0.6 em per glyph, and that a row is one line tall.
- **The window** now draws the minimal meter under the status, on the
  newest pull (`pin_live`).

### 1.3 Spikes

Each spike is an example under `crates/gui-new/examples/` (kept as a manual
check, run by hand) plus a findings line written into spec §12. On the real
Hyprland session, never while the user is mid-pull; on a headless output
where the spike allows (see the memories on headless Hyprland).

| | Do | Record |
| --- | --- | --- |
| S1 | an overlay-layer surface, transparent, `KeyboardInteractivity::None`, over the running game | does it composite, steal nothing, survive a workspace switch |
| S2 | empty and partial `set_input_region` | click-through outside, input inside, updates take effect without a resize |
| S3 | the full-edge strip with content at an offset + input region tracking, vs recreate-on-release | frame time, compositor CPU, game frame pacing with each; pick one (decision record) |
| S4 | `display_id` from `hypr::game_monitor` by recomputed UUIDv5 of the output name; a fractional-scale monitor; the rotated DP-1 | born on the right output; hit-testing at that scale; `uuid` vs hand-rolled SHA-1 decided |
| S5 | `HeadlessAppContext` + `current_headless_renderer()` under lavapipe; a real text system without naming `gpui-pre-wgpu` | renders at all; text route chosen; guard tolerance set; whether a pinned Mesa input makes runs byte-identical |
| S6 | `add_fonts` with the bundled families; the overlay's symbol glyphs | faces resolve by name; fallback for ⚙ Σ ☠ ● |
| S7 | `RenderImage` from a decoded class-icon tile and a lazy spell tile | channel order, memory, lazy loading holds |
| S8 | `canvas()`: filled area + Catmull-Rom spline + text + an image over a path | paint order as written; text crisp at the window's scale |
| S9 | clean release + debug builds, binary size, the nix shell, RUNPATH | numbers; the dlopen list; opt-level set |
| S10 | the workspace clippy denies over the skeleton | clean, or a scoped allow with a reason |
| S11 | shifted bindings (`K T E ? ~ H`), `ctrl +` / `ctrl =`, and the root Esc handler under GPUI's context precedence | every binding in gui-logic's table fires on a Linux layout |
| S12 | theming: a throwaway `theme::Def` mapped onto Kit's `Theme` + a `Look`; Kit `Button`, `Tabs`, `Input`, `Tooltip`, `Popover` restyled to the prototype's values (`.fhead` tabs, chips, the filter box) through theme + `Styled`; a runtime switch to a second definition | the five reach the prototype or each has a named gap (→ the `gpui-base` fallback for that control); both a Kit component and a bespoke canvas repaint on the switch; which Kit slots our components read |

**Exit:** a short review with the user over the findings. A failed S1–S4 or
S5 changes the spec before phase 2 starts.

**As built (2026-09-30).** The findings are in spec §12 (*Findings*).
Where the work departed from the text above:

- **Probes are ignored tests, not examples.** gui-new is a binary crate,
  and the spikes that answer by pixels need its modules (the meter, the
  theme, the image cache). So they are `#[ignore]`d tests that save PNGs
  under `WOWDPS_SHOTS_DIR`: `meter::render_probe` and `probes::{s6, s7,
  s8, s12}`. S11 is an ordinary test, `keys::tests`. S4 is the overlay
  itself, which takes its output from `WOWDPS_OVERLAY_OUTPUT` until phase 2
  chooses the game's monitor.
- **Several spikes left production code behind.**
  - S5: `testkit::headless`.
  - S7: `images.rs`.
  - S11: `keys.rs`, plus gui-logic's `ZOOM_CHORDS`.
  - S12: gui-logic's `theme::{Color, Def, GOLD, FROST}`, its colour
    arithmetic, and gui-new's `theme.rs`. The iced GUI's tokens and colour
    maths now read from gui-logic, moved; its guard and design shots hold.
  - S4: gui-logic's `output`.
- **Open spikes.** S2 is folded into 2.2. S1's fullscreen-game check and
  S3's numbers wait for the game to run. Neither blocks 2.1 (the meter's
  rows, no surface work).
- **The exit review.** The findings are written for the user to review.
  The work goes on into phase 2 meanwhile, since nothing in them changes
  the spec's design.

## Phase 2 — the overlay

Each step reproduces the guard states it names as `TestWindowExt`
interaction tests (green on every `cargo test`) and, at 2.6, as guard renders.
Colours come from the theme (spec §6.1) from the first step: 2.1 creates
gui-logic's `theme::Def` with the built-in `gold` definition, whose overlay
palette is `Look::OVERLAY`'s values (wave B moves those values and the
shared token helpers), and the mapping onto Kit's `Theme` and our `Look`.
The overlay's options card, view menu and tooltips are Kit components
restyled to the overlay palette; its meter, strip and graphs are bespoke.

| Step | Builds | Guard states |
| --- | --- | --- |
| 2.1 | meter rows (rank, class icon, bar under, numbers), header, footer, view-name cycle, view menu, options card (ranks, realms), both closing when the pointer leaves | meter, hover (first, second), options, view menu, enemies, interrupts |
| 2.2 | the surface per S3: edge tab ↔ expanded, the grip telling a click from a drag, grip drag + persisted offset, edge change, wheel-over-header zoom, hidden + `SetVisible`, single-instance takeover (gui-logic), output choice + the 30 s `GAME_STARTING` wait, `follow_game`, `focus: false`, the surface's `Closed` event | collapsed (the others are drawn expanded) |
| 2.3 | instance strip Σ–①─②─③–⚑ (wave B: `timeline` helpers), block stepping, chip scrub, wheel over the strip (`StripScroll`), Live re-pin, discard trash, the Σ split's second `Session` | live, split, arena |
| 2.4 | drill: R26 rollups (`tree_open`), spell drill, attackers, drill graph + drag-zoom, the time cursor shared by every graph (`GraphProbe`), right-click backing out, Taken / Deaths / Enemies drills, mitigation line (wave B: `compare.rs`'s `view_draws`, `view_draws_mark`, `for_view`, `kinds_shown`, `view_window`, `curve`, `peak_of`) | drill, drill hover, spell drill, tree drill (shut, all open), taken / deaths / enemies drill |
| 2.5 | compare: class-icon picks, two tables + one-scale graphs, the per-second / cumulative toggle (a click — the overlay has no keyboard), range, spell hover lit in both lists, a marker lighting every use of its item (`CompareHover`), a spell row drilling both sides (`CompareSpell`), right-click clearing the pair, growth to `COMPARE_MIN` | compare, zoomed |
| 2.6 | the render guard (`crates/gui-new/snapshots/overlay/`), every `WOWDPS_OVERLAY_*` env var, side-by-side review against the iced guard PNGs at 1:1; then `gui_binary = "wowdps-gui-new"` on the dev machine | all |

2.2 is the only step that needs the real compositor for acceptance; its
interaction tests cover the state machine (collapsed/expanded/hidden,
offset arithmetic, input-region rectangles) and a manual checklist covers
the surface on Hyprland with the game running.

Every step also ticks its `Message` variants off the checklist derived from
`overlay.rs`'s enum (spec §7.1): a test, or a named reason for none.

**As built (2026-09-30), step by step.**

- **2.1.** `overlay/panel.rs` is the `Overlay` entity; `ov.rs` holds the
  zoom, the palette and the text helpers, `rows.rs` the rows and badges.
  The surface opens through GPUI's `cx.open_window`, not Kit's: Kit's
  Root adds a 20 px shadow inset on a layer surface. A bar eases to a
  new value over 280 ms (a gpui-base `transition`, keyed by player), the
  first delight; reduce-motion settles it at once, so a test or a shot
  draws the parity pixels.
- **2.3.** `overlay/instance.rs` draws the strip from gui-logic's
  `timeline` (items, the wipe collapse, `cascade_xs`'s fan with the
  watched element raised). The chip's ‹ › and the wheel over the strip
  scrub the visit (`timeline::scrub`; half notches add up). The footer Σ
  toggles the split, remembered by itself through `Config::store`. The
  split's connection is a second `Session`, made on first want through an
  `AuxMaker`: a `Window`-kind `DaemonClient` in the app (so `SetVisible`
  never reaches it), a second mock in the tests. Its clock feeds the
  header's instance clock.
- **2.4.** Wave B moved the graph's model into gui-logic `graph`: the
  marks, the curves, the window, the wording, and the plot's geometry as
  `Plot`. iced's `Graph` holds a `Plot` and derefs to it. A second move,
  gui-logic `drill`, took the drill's words and the overlay drill's grid.
  The iced overlay guard and all 66 iced design shots are byte-identical
  across both moves.
  - `overlay/drill.rs` draws the lines: the crumb, stat cards and
    targets of an ability; the caption; R26's tree; a death's recap; an
    enemy's attackers.
  - `overlay/graph.rs` is the graph, one canvas painting the iced
    layers in order. Its gestures are window mouse listeners. Each graph
    keeps what it last said in a `Local`, so two graphs sharing one echo
    never argue. GPUI exports no round line join, so a miter limit of 1
    bevels the bends. A receding marker icon is the panel's ground laid
    over it at the opacity it loses, because `paint_image` takes no
    opacity.
  - **A deliberate change:** a right press anywhere backs out one level
    (a comparison first, then an ability, then the drill), and a right
    press on the graph only resets its zoom. In iced the body's
    comparison-clearing handler swallowed a right press, so the drill
    backed out only from the header or the footer.
  - **Shots and the cursor.** `overlay_shots` now builds each state
    over its own fixture (`tree.txt`, `taken.txt`, `arena.txt`, the
    live mock). Every drill state, live and arena matches the iced
    guard's pictures. The graph's cursor reads its gesture: a crosshair
    over the curve, a hand over a marker, a resize arrow mid-drag.
  - **Gap:** iced draws a scrollbar on an overflowing list. gui-new does
    not yet, and step 2.6 adds one.
- **2.2.** `overlay/panel/surface.rs` puts the overlay on its edge
  strip. The surface spans the edge, anchored to the edge and both
  neighbours. The content sits at the offset, and the input region is
  exactly the content.
  - **The grip** (header, tab) is a click or a drag beyond 5 px. A
    window-wide listener follows the drag off the content. The offset is
    remembered by itself.
  - **A new edge.** Under Hyprland, a tab dropped near another edge
    recreates the surface there (gui-logic `surface::nearest_edge`,
    moved). The new surface opens before the old one goes.
  - **Hidden** is a 1 px strip with an empty input region: the daemon's
    `SetVisible` composed with `follow_game`.
  - **Output and lifetime.** The output is chosen before the app starts
    (`overlay::choose_output`: `WOWDPS_OVERLAY_OUTPUT`, `monitor`, the
    game's monitor with the 30 s spawn wait). Closing the last surface
    quits.
  - **Finding: never ask for a zero length.** A zero length, which
    layer-shell reads as "stretch", kills the connection. GPUI hands every
    size to the surface's viewport, and a zero there is the protocol
    error "Size was <= 0". The surface opens with the edge's real length:
    Hyprland's `monitor_named`, which accounts for rotation and scale,
    else GPUI's display bounds.
  - **Verified** on a headless Hyprland output, with
    `WOWDPS_OVERLAY_OUTPUT` and `WAYLAND_DEBUG=1`.
  - **Still to do with the game running:** S1 (fullscreen compositing)
    and S3's cost of a full-length transparent surface.
- **2.5.** `overlay/panel/compare.rs` draws the pair from the same
  gui-logic `graph` model: two panes over one legend, one scale, the
  two-reading probe, an ability drilled on both sides, the "hit by"
  table and the mitigation line on Taken. Its words moved first
  (`graph::table_words`, `waiting_words`; `drill::avg_text`,
  `crit_text`).
  - The surface's size is decided in `render`, from the state, so it
    grows to the zoomed `COMPARE_MIN` and gives the room back however the
    pair began or ended. Only a change is sent to the compositor.
  - The shot matches iced's to within a few pixels.
- **2.6.** The overlay's render guard is `overlay_render_guard`. It
  checks every state in `overlay_shots`' table (20, with live, arena,
  every drill and the comparison) against `snapshots/overlay/<state>.png`,
  within `guard`'s tolerance.
  - **No game art.** The guard renders without the art caches
    (`images::without_art`), as a machine without them draws, so the
    committed pictures hold no extracted Blizzard art. The faces are the
    machine's Noto Sans, so run it where it was blessed. A picture with
    no state fails too.
  - **The scrollbar.** iced's is a square 10 px rail with its thumb in
    the list's lane. `overlay/scrollbar.rs` draws it over a
    `ScrollHandle`, with a draggable thumb: on the body list and on both
    comparison tables. Kit's thin, self-hiding bar was not the
    overlay's.
  - **Debug aids.** The `WOWDPS_OVERLAY_*` aids (`DEBUG`, `AUTOSEG`,
    `AUTOVIEW`, `AUTODRILL`, `AUTOCOMPARE`, `AUTOTOGGLE`) work as iced's
    do (`panel/autos.rs`). They were checked live on a headless output
    over the real log.
  - **Left for the raid week:** switching the dev machine to
    `gui_binary = "wowdps-gui-new"`, which needs the user and the game.

**Phase gate:** 2.6 merged. The raid week with gui-new's overlay as
`gui_binary` runs IN PARALLEL with phase 3 — it gates phase 5, not the
window work; every regression found is fixed or recorded as a cutover
blocker.

## Phase 3 — the window

In the redesign's own order, so each step can be checked against its
decision record and the prototype. Every step adds its states to the
design-shot test and reviews them against the reference renders at the
three sizes (1440×900, 960×880, 460×860).

| Step | Builds | Moves to gui-logic (wave B) |
| --- | --- | --- |
| 3.1 Look | the window's half of `gold` (the prototype's Tokens) and class chrome as an accent override on any theme; the theme-switch test; the icon decision (glyph strokes vs SVG assets beside Lucide); fonts, frame, top bar (wordmark, places, jump box, live pill, character picker, gear, help — Kit `Button` / `Popover` restyled), footer status, the two breakpoints | the window's token values, `chrome_base`, `class_text`, `Accent` |
| 3.2 Header | fight header + stat line + "you" chip, the ribbon (R25 canvas: raid rate, lust wash, skulls, crosshair), view tabs (Kit `Tabs` restyled to `.fhead`, with reveal), the meter table (column sets, sort, filter box on Kit `Input`, pinned total), the Deaths table; the chrome-budget test | table column sets / sort / filter, `fight_head::Seen`, Deaths ordering |
| 3.3 Inspector | follow-selection, numbers, actions, the plot (10 s buckets, spline, death hatch, zoom, reset), lanes, list tabs, the R26 tree with parts, the stacked graph and slot seating, Taken + Stacks matrices, the Deaths recap and chips, Enemies, the comparison pair, the narrow push, `Held` dimming | level-0 derivation, lane and stack seating |
| 3.4 Rail | tonight + stored nights (`Earlier` pages), visits, drawer ≤ 1180 px with j/k/Enter, `[` `]` across nights with paging, stored pulls as `Stored` sessions, `p` pin, refused reads | night / visit grouping and order |
| 3.5 Home | week derivation, night card, rank slope chart, the three panels, scope chips, store-state words | Home's derivations, `standing` |
| 3.6 Over everything | command palette (Kit `Dialog` + `Input` with our search, or Kit `Command` if it carries our sections and ranking — decided here), `?` sheet keyed on surface (`Kbd` hints), row filter + `/`, the Esc walk, options card, character menu, toasts (`Notification`) — all restyled Kit components | palette search and ranking |
| 3.7 Review | full design-shot set + real-log chrome budget; keybinding completeness against gui-logic's table | — |

**As built (2026-10-01), step by step.**

- **Wave B for 3.1 and 3.2** (`e334c1b`). The measures (`theme::Sizes` and
  `Pitches` on every `Def`, the breakpoints, the "you" alphas, the
  shadows), the line icons as data (`glyph`), the window's words
  (`labels`, `fight_head`, `deaths`), the table's grid, column sets, total
  and row filter (`table`), the tick steps (`axis`), the ribbon's
  arithmetic (`ribbon`) and the tab strip's (`reveal`). The iced design
  shots stayed byte-identical: 66 over the fixture, 87 over the real-log
  slice.
- **3.1.**
  - `Gui` is the window's root entity. It holds the log's `Session`, the
    place on show, the open cards (`Cards`: options, sheet, palette,
    picker and the rail's drawer, flags that 3.4 and 3.6 draw) and `Seen`.
    `window/w.rs` is the render context: the tokens, sizes, pitches,
    accent and faces, the zoom (`w.z(…)`: GPUI has no app scale factor)
    and the width at zoom 1, which every breakpoint reads.
  - Breakpoints are the CSS's, inclusive at both. The iced fight header
    is strict at 820 (finding D1); gui-new is not, on purpose.
  - **The icon decision.** Strokes, not SVG assets. `window/paint.rs`
    strokes gui-logic's glyph table. GPUI does not re-export lyon's caps
    and joins, so each segment is its own sub-path and a disc of the
    stroke's width sits at every end. `glyph_ink` paints in its parent's
    text colour, so an icon brightens with its control's hover: finding
    D10, fixed.
  - The top bar (`window/top_bar.rs`) is the iced one. The jump box keeps
    a constant 14 px margin rather than measuring its room, a difference
    only below 219 px of room. Until the rail and Home read the store,
    the picker names the stage's owner.
  - Keys. The Meter bindings fire on `Meter && !Input`: GPUI's `!` reads
    the whole context stack, so no Meter key fires while a Kit `Input`
    has focus. `keys::tests::a_text_field_keeps_the_meter_s_keys_out`
    types "jk" into a field inside the Meter context and asserts the
    text lands and no action fires. The window-local gestures ride `Go`.
  - Zoom writes one key (`Config::store`), so an overlay drag saved
    since launch survives a zoom (finding D2, fixed).
  - `t` opens the talent viewer on the selected row and asks for the
    logged loadout; its `Close` gives the keys back. The root drops its
    Meter context while the viewer holds the window.
  - Delights: the live dot pulses, as the prototype's `.pulse` does (a
    keyframed ring; the dot alone under reduced motion).
  - Tests: `window::tests` (places, cards, the live pill, zoom, `t` and
    Esc, the shared keymap, the rail docking above 1180 only, and a
    theme switch repainting the window's own quads); the
    `window_shots` design-shot test (ignored, scale 2).
- **3.2.**
  - The stage (`Gui::stage`) is the fight header, the ribbon, the view
    tabs, then the meter or the Deaths table beside the inspector's seat
    (520 px wide, 410 in a tile; the whole stage when a narrow window
    pushes it). 3.3 fills that seat.
  - The fight header (`window/fight_head.rs`) lays out gui-logic's
    `fight_head` words. Wide, the stat line is one line ending in the
    "you" chip; narrower, it wraps. The chip's press selects the owner,
    and a filter that hid them gives way, the field's own text with it:
    a Kit `Input` keeps its text, so the window clears both.
  - The ribbon (`window/ribbon.rs`) is one canvas: the rate as a
    gradient area under a Catmull-Rom line, the lust wash, a skull per
    death on its red hairline, the labels set on plates where they fit,
    and the tooltip. The pointer is read through the canvas's painted
    bounds (a `Cell`), so a press on a skull opens that death.
  - The view tabs (`window/tabs.rs`) reveal the active tab whenever the
    view or the width changes, and a wheel scrolls the strip. The row
    filter is a Kit `Input` in a `Filter` context: its Esc
    (`Filter > Input`) clears it and gives the keys back.
  - The meter (`window/table.rs`) draws gui-logic's column sets. A
    heading sorts descending, then ascending, then gives back the
    daemon's order. Rows keep the daemon's index, so a sorted or
    filtered row keeps its rank, share and click target. The selected
    name is a new token, `WindowTokens::name_lit` (`.trow.sel .nm`).
    The total pins under the list.
  - The Deaths table (`window/deaths.rs`) lists the deaths in the order
    they happened. The blow's after-run gives way before the blow does.
  - Keys walk what is drawn. On a filtered or sorted meter, `j`/`k` step
    through the drawn rows, using gui-logic's `table::meter_step`, which
    moved there from the iced window (`168b5a3`). On the Deaths table
    they walk the deaths in order, and each step is that death's recap.
  - The list follows its selection past the fold. A step, the chip and
    an opened death set `Gui::reveal`. The list's next layout scrolls the
    least that shows the whole row (`ScrollHandle::scroll_to_item`). The
    iced window brings in the row's text but not its bar, so gui-new
    scrolls a few pixels further.
  - The synthetic raid moved to `gui_logic::raid` (`0232c32`), so both
    GUIs measure their chrome over the same 25 players.
  - Delights, each settling to the parity pixels under reduced motion
    (the shots run with it on):
    - the tabs' underline glides to the picked tab (a 300 ms spring);
    - the strip's edges fade where it scrolls;
    - the filter widens from 92 to 140 px while it has focus;
    - a row's bar eases to its new length (280 ms) and brightens when
      selected;
    - the ribbon's crosshair glows, with a dot where it meets the curve.
  - **The chrome budget holds.** `the_chrome_leaves_a_raid_its_rows`
    runs in real fonts on every `cargo test`: the first row starts
    285.5 px down at 1440×900 (the budget is 290), 18 rows show, the
    total sits flush with the bottom, and the owner's chip says "dps".
  - Parity gaps, all small:
    - the Deaths table's trailing pad is a constant 16 px;
    - in a narrow window the Deaths player column grows (0.8) rather
      than taking the iced window's measured share;
    - `window_alpha` translucency is not drawn yet;
    - the picker's characters come from the stage alone until 3.4 and
      3.5 read the store;
    - the lists drew no scrollbar (since fixed: one shared scrollbar,
      `0884c6b`, below);
    - the title's ★ for a pinned card waits for `p` and the store (3.4).
  - Kit's `Input` pads its field by its size, so the filter sets its own
    padding to zero, and its text starts where the iced window's does.
  - The design shots select the owner the way the keys get there, as the
    iced shots' `in_view` does. They also make their directory.
  - Tests: the filter keeps the meter's keys while it has focus (types
    "j", which lands in the field and moves nobody); the list following
    a step past its fold; a sorted meter's keys; the Deaths table's keys;
    tabs; sort; a row's
    press, and in a narrow window the push; the chip; a death's line; a
    skull's press (`ribbon::tests`); the chrome budget. `testkit::NullLink`
    holds a hand-built state still.
- **3.3, the graph, the matrices and the chips.** These are three
  components the inspector wires (`window/inspector/{plot,matrix,chips}.rs`).
  - **Wave B.** The graph's layout is gui-logic's `inspect::geometry`,
    moved from iced's `inspector/plot.rs` (`cddb709`). The R21 matrices'
    level-0 derivation and heat are `inspect::matrix`, moved from iced's
    `taken.rs`. Iced draws through both.
    - The geometry covers every constant, x and y, span rectangles and
      hit tests, hover snapping, the drag window, and the tooltip's words
      and placement. It also covers the hatch, scale, tick and lane labels
      with their tooltip culling, curve points and stacked bands, the
      clamped Catmull-Rom spline and the 45° stripes.
    - A renderer brings only a text measure and its paint.
    - Three new tokens hold iced's literals for GOLD and FROST:
      `WindowTokens::lane_track` (white at .028), `drag_fill` (white at
      .10) and `span_lit` (white). Iced reads them too.
    - The 87 iced design shots over the frozen night are byte-identical
      before and after, and the overlay guard is unchanged.
  - **The plot** (`plot(id, Input, &W)`, with `.on_range(OnRange)` and an
    optional `.state(Entity<State>)`) is one canvas painted in iced's
    order at zoom 1 × the window's zoom. It measures text as it paints
    it: shaped at the zoomed size, divided back.
    - Hover and drag live in keyed state; the last frame's window and
      tooltip are recorded for the owner and for tests.
    - Its height is `Input::height()`.
    - Gestures are window mouse listeners from paint, as the overlay's
      graph uses: a drag scrubs past the canvas, held to the plot.
    - A span's edge and the tooltip's frame are quads grown half the edge
      each way, which matches iced's centred strokes.
  - **The matrix** (`matrix(id, &[Matrix], dropped, &W, &App)`) and **the
    chips** (`chips(&[DeathWindow], shown, dropped, &W, OnPick)`) are
    divs. GPUI adds a border outside the padding where iced draws it
    inside, so their padding gives the border back. Each sets the
    window's 1.3 line height itself (GPUI's default is φ).
  - **Parity, measured.** `inspector_plot_shots` (gui-new, ignored) and
    iced's `plot_shots` / `taken_shots` (ignored) render the same
    `geometry::samples` states (alone, lanes, zoomed, a plot hover, a span
    hover, a pair, same-class twins dashed, a ghost, the stack, a total),
    plus the matrices and the chips, at the same frame and scale. Diffed
    with ImageMagick (`compare -metric AE -fuzz 12%`):
    - the ten plot states differ in 0.12–0.33 % of pixels;
    - the matrix differs in 1.1 % and the chips in 0.6 %;
    - all of it is glyph antialiasing, plus GPUI snapping a text baseline
      to the device pixel where iced keeps the fraction (a lane label
      sits ≤ 1 device px lower).
  - **Delights**, each gone under reduced motion so the shots are the
    parity pictures:
    - a zoom glides: the axis, curves, hatches and spans slide to the new
      window over 220 ms, ease-out, through Kit's `transition`;
    - the crosshair glows, the ribbon's glow, with a dot on a ring of the
      panel where it meets each curve;
    - a drag in flight shows gold edges and the window it would select
      ("0:42–3:03");
    - each matrix cell stands on a 13 % wash of its heat, and a row lights
      under the pointer;
    - a cell's tooltip says how many hits its average is over (always on);
    - a death chip brightens and takes the floating edge under the
      pointer (always on).
    - `WOWDPS_SHOTS_DELIGHT=1` adds the motion-on pictures to the shots.
  - **Tests.**
    - gui-logic: geometry (11, moved and new), matrix (2), the samples'
      pointers (1).
    - gui-new: Kit-harness tests:
      - a drag publishes its window, a click or a 2 px wander does not,
        a drag off the canvas holds at its edge, a right press resets,
        and a press on the axis starts nothing;
      - an unzoomable graph takes no drag;
      - hover reads the plot, then Power Infusion over the Heroism, then
        nothing;
      - the glide's first, middle and settled frames, and the jump under
        reduced motion;
      - the height follows the lanes;
      - the matrix's cells;
      - a chip's press, and when the strip shows at all.
  - **Parity gaps.**
    - GPUI rounds the plot's height to the device pixel (194.5 against
      194.4).
    - The curves bevel their joins (miter limit 1, as the overlay graph
      does) where iced rounds them. This is invisible on the C1 spline.
- **3.3, the inspector.** `window/inspector/model.rs` builds `Insp`, owned
  data, once a frame from the fight's `ClientState` (`Insp::of(&Ctx)`);
  `view.rs`, `list.rs` and `recap.rs` draw it, and the three components
  above take their seats (`graph`, `death_chips`, `stack_matrix` in
  `inspector.rs`). `Held` keeps the last player's body, dimmed, until the
  next breakdown lands.
  - **Wave B** (`5fd1735`, `031a54a`, `377a150`): the roster, lanes,
    stack seating and curve data, the numbers, curves and recap words,
    and the lists' columns, hues and foe sphere moved to
    `gui_logic::inspect`.
  - **Keys** (`9c5d02f`), iced's order: Enter hands the keys over; `j`/`k`
    walk the ability tree's lines (a group's included) or a sorted list
    as drawn; Enter on a group folds it; → ← open and shut a fold or climb
    to the line that holds it; on a Deaths drill ← → step the death
    windows, swallowed with one; Tab walks into the Stacks tab, which
    stands only where a ledger makes a matrix. Beside the inspector the
    Deaths table keeps its keys; a narrow window's Enter pushes the
    recap.
  - **The keyed line stays in sight.** The inspector scrolls as a whole,
    its lists deep inside, where `ScrollHandle::scroll_to_item` reaches
    only direct children. A step lays a probe canvas on the keyed line
    (`inspector::list::Keep`), which reads its bounds after layout and
    scrolls the least that shows it; the palette reuses it.
  - **Three iced drifts found against the shots and fixed** (`8911219`):
    iced spaces no void element, so a top-level tree line has no gap
    before its caret; the figures wear their column's ink (amount and
    rate in ink, the rest ink 2); GPUI draws a border outside the
    padding where iced draws it inside, so a button pads 1 px less.
  - **One scrollbar** (`0884c6b`): the overlay's moved to
    `crate::scrollbar` and takes a `Style` — the overlay's square thumb
    on its rail, the window's iced-default thumb (radius 2) on a clear
    lane (`W::scrollbar`). The inspector keeps the lane iced keeps for
    it; the meter and the Deaths table wear the bar (`chrome::scrolled`).
  - **Parity, measured** over the real-log night against the current
    iced set: about 1 % of pixels differ at 12 % fuzz across Damage,
    Healing, Taken and Deaths at the wide and tile frames, most of it
    the rail's earlier nights (the gui-new shots load no history yet).
    One behaviour differs on purpose: a meter row past the fold comes
    into sight whole, where iced leaves its bar cut.
  - Tests: Enter and the tree's walk, the arrows' folds, a caret and a
    line's press, the tabs and the actions, the keyed line in sight in a
    short window (the inspector scrolled), the Deaths table's keys and a
    narrow push swallowing ←.
- **3.4 and 3.5, the rail and Home** (`window/{history,rail,home}.rs`,
  `home/{panels,charts}.rs`).
  - **Wave B.** These moved from the iced window into gui-logic, and iced
    draws through them:
    - the rail's model went to `rail` (`363d0cc`, `07330a5`): nights by
      local date with the 06:00 cutover, visits, pulls, the walk
      (`Rail::step`), the trash rule (`Line::shown`), the log's visits,
      the words and the reveal rules (`near_offset`, `open_offset`);
    - Home went to `home`: the week's paging, `Season`, `derive`,
      `standing`, the panels' data, the remembered characters
      (`remember`), and the words and measures;
    - the charts' geometry went to `home::chart`.

    After each move the iced design shots were byte-identical (66 over
    the fixture, 87 over the real-log slice with the frozen store), and
    the overlay guard was unchanged.
  - **Readers.** `Hist` is one `Gui` field:
    - `Store` holds the rail's `Earlier` pages, a stored pull's own
      `Stored` state and Home's paging. Each asks through
      `Session::request`, is answered from `Reply`, and keeps one read in
      flight.
    - `Hist` owns its subscriptions and a 250 ms tick that re-asks a
      refused read.
    - It reads the store's state from `Status`.
    - `Gui::fight` is the stage's pull, and `Gui::act` acts on it, so a
      gesture on a stored pull becomes its `GetFight`. The stage, the
      header and the inspector read it.
  - **The rail.**
    - Docked at 236 px above 1180. At 1180 and under it is a 280 px
      drawer over a scrim, with its shadow.
    - `rail_key` takes the keys first. While the drawer is open, j/k walk
      a highlight, Enter opens a pull and Esc closes the drawer. Outside
      it, `[` `]` and the header's ‹ › walk the rail's drawn order, stored
      nights included, paging the store past the last card in hand.
    - A row is placed by iced's rules: a step leaves 40 px under it, and
      the drawer opening stands the night's heading at the top. When the
      last layout was a different list, the first frame uses GPUI's
      least scroll.
    - Its thumb is iced's: 6 px, in a 10 px lane.
    - The ★ goes in the row's gutter. A stored card of the tailed log is
      listed, and opens, as the log's pull.
  - **Home.**
    - Scope chips: a chip writes `character` alone.
    - The night card: tiles plus the rank slope.
    - Panels: keys with par bars; boss rows with a dot per pull; key
      throughput. Each words an empty week. An off or cold store says so,
      and no panel is drawn.
    - A grid row stretches its panels to equal height.
    - Widths come from the window's width less the docked rail.
    - Narrow, the page's bar takes a lane while the page overflows.
    - Home opens at launch when nothing is live, and stands aside when a
      pull starts.
  - **A stored pull's header** shows its card's wipe %, a ★ when pinned,
    its night when it is not tonight's, and the reading or gone words.
    `t` adopts the build the stored answer carried. The picker names the
    store's newest owner.
  - **Delight** (static, so reduced motion has nothing to settle): each
    chart dot is an element of its own, the size of the press radius. It
    wears the pointer and its tooltip names the pull ("Ula'tek, 10th of
    18"; "Tranqlock, 292.6k on Saturday"). Iced's charts say nothing
    under the pointer.
  - **Parity, measured** (gui-new's `window_shots` against iced's
    `design_shots`, the same real-log night and frozen store, scale 2;
    `compare -metric AE -fuzz 12%`):
    - home: 1.7 % wide, 2.0 % tile, 3.0 % narrow;
    - rail-open: 0.8 to 1.8 %;
    - hide-trash: 0.8 to 1.9 %;
    - rail-earlier: 0.8 to 1.3 %, the drawer standing where iced's does;
    - stored: 0.5 to 0.7 %.

    The differences are glyph antialiasing and Home's night card standing
    about 2 px taller (line boxes), which moves the panels under it.
  - **Gaps.**
    - On a stored pull, Compare and the Enemies tab are not dimmed and
      refused as iced's are (the inspector's and the tabs').
    - A wipe tile's "at 56%" stays while the name gives way. Iced gives
      the tail up whole first.
    - The Home night card stands 2 px tall.
    - A pull starting under Home is covered by code only: the mock never
      goes live.
  - **Tests.** gui-logic: the rail (7), Home (18) and the charts (2).
    gui-new:
    - `window::history` (4);
    - Kit tests over the mock with a stored history:
      - rail (10): lists, Hide trash, the drawer at 1180 and 1181, drawer
        keys, `[` `]` paging across nights, Show older nights, the live
        pill off a stored pull, a stored pull's own gestures, the
        header's night and ★, and the 40 px reveal;
      - Home (7): its week and panels, a chip remembered, jump points,
        keys, at launch, an off store, one column narrow.

- **3.6, the command palette** (`window/palette.rs`). Its model moved to
  `gui_logic::palette` (`09b0af3`) with its tests; the fixtures ride
  `test-support` for both GUIs' gesture tests, and iced's palette shots
  stayed byte-identical. The card is over the whole window; its field
  (a Kit `Input` in a `Palette` context) holds the focus, so the meter's
  keymap is silent under it, and `Palette > Input` binds the arrows,
  Ctrl N / Ctrl P, Esc and Ctrl K. Measured against iced's palette
  shot over the frozen night: 0.49 % of pixels differ at 12 % fuzz.
  - **One themed field** (`window/field.rs`, `8fdf706`). Kit's styled
    `Input` paints every placeholder in the theme's one
    `muted_foreground` (ink 2, which Kit's tooltips, tabs and keycaps
    also read) and forces `text_sm` at every zoom, so the row filter sat
    at 12.7 px whatever its size said. `Field` composes Kit's unstyled
    gpui-base `Input` on the same state and projects the window's tokens
    onto its editor style each render: placeholder in ink 3's text grade,
    caret, selection and ink; caret, IME, scrolling and clipping stay
    Kit's. A second opinion chose it over a placeholder drawn per site
    (wrong under IME composition, zoom and clipping) and over recolouring
    `muted_foreground` globally. The palette and the row filter use it;
    the talent viewer's framed field stays on Kit's styled `Input`.
  - Tests: Ctrl K opens it with the keys in its field and closes it; a
    query narrows and Enter runs; the arrows step and stop; Esc gives the
    keys back; a press runs a line, one on the card's heading holds, one
    on the scrim closes; a player's line runs to Damage; nothing matching
    runs nothing.
- **3.6, the cards (the palette is its own).**
  - Moved to gui-logic (`429091f`), with iced delegating:
    - `keys::{sheet_groups, keycaps, Surface::of, inert_keys}`;
    - `home::{FOLLOW, FOLLOW_NOTE, played_note}`;
    - a new `toast` module: the words, `TOAST_FOR`, `pinned_player` and
      `stored_refusal`.
  - `window/cards.rs` holds the Gui's side of the cards. One file each
    draws the `?` sheet, the ⚙ card, the character menu and the toast.
  - **Menus are modal for the keys.** While one is up the root leaves
    the `Meter` context for `Modal`, and its key-down listener closes
    the menu on any key, which does nothing else. The zoom chords
    (bound with no context) still zoom, and Ctrl K (bound in `Modal`)
    closes the menus for the palette. One menu at a time. For the
    pointer:
    - the sheet hangs over an occluding scrim, and a press anywhere on
      it closes it;
    - the character menu has a clear scrim;
    - the ⚙ card closes when the pointer leaves it.
  - The ⚙ card writes each choice with `Config::store`, one key at a
    time.
  - The Esc walk follows the iced window's order: the palette (a stub
    the palette fills), the rail's drawer, Home (where it ends), then on
    the stage the filter's text, the inspector's ability or keys, the
    comparison, and Home. The talent viewer and the menus answer their
    own Esc first.
  - The toasts:
    - a pin's word, which the pair forming takes back;
    - `p`'s NO_CARD, and the store's PINNED or UNPINNED;
    - the follow item's note;
    - a stored pull's refusals (`v`, the enemies' view, Enter on an
      ability).
  - The picker's menu takes characters from the rail's pages as well as
    Home's answers, as iced's does.
  - Decisions:
    - **Not Kit's `Kbd`.** It capitalises single letters, so `j` and
      `J`, `k` and `K` (Deaths) would read alike, and it prints a chord
      as one cap. The sheet uses the window's `kbd`.
    - **Not Kit's `Notification`.** Its stack stands in a corner with a
      close button; the prototype's toast is centred, brief and takes
      no pointer.
    - iced's sheet lists only what works on the surface and dims the
      keys the pull cannot answer. There is no "elsewhere" section, and
      gui-new follows iced.
    - A new token, `WindowTokens::check`, is the tick on a checked box
      (iced's `primary.strong.text`, measured at #3B2D0C).
  - **Delight:** a card enters with a fade and a 6 px rise over 160 ms.
    Under reduced motion it rests from its first frame, at the iced
    pixels.
  - **Measured against iced** (`plot-after`, the real log with the frozen
    store, each card cropped at each size):
    - pixels differing by more than 6 %: options 1.2–1.4 %, picker
      1.4–1.5 %, the sheet 0.6–1.4 %;
    - all of it is text antialiasing, the tick's shape (within 1 px) and
      the arrow keycaps. GPUI's font fallback finds a narrower ← → than
      iced's cosmic-text does (DejaVu Sans): each cap is 5.5 px narrower;
    - iced draws a container's border inside its padding and GPUI
      outside it, so the cards pad by the iced value less their 1 px
      edge (`cards::BORDER`). Before that fix the ⚙ card stood 5 px
      wider and 4 px taller.
  - Tests (`window::cards::tests`):
    - each menu opened where a reader opens it, then closed by a key
      (which does nothing else) or by a press;
    - zoom under a menu, and Ctrl K replacing it;
    - the options written one key at a time (a zoom saved meanwhile
      survives);
    - the pointer leaving the ⚙ card;
    - the character menu: the follow item, the scope remembered,
      `hide_realms`;
    - a toast passing, `p` with no card, and a pin's word taken back;
    - a stored pull's refusals;
    - the Esc walk, and the drawer going first;
    - the entrance animating, and resting at once under reduced motion;
    - the sheet's columns at each width.
  - The design shots read a real store (`WOWDPS_SHOTS_HISTORY`), share
    one parsed mock across their windows, pin "tonight" to the log's
    newest night, use the iced shots' display keys, and photograph the
    three cards.

**Phase gate:** every SHOTS.md state reproduced and reviewed by the user.

## Phase 4 — the talent viewer

Panes from `proto::talents` + `talents.json`; art from `talent-art.bin`
(plain panels without it); node shapes, rank pills, lit paths; the import
field; SimC paste (gui-logic `simc`) with loadout chips, inventory tab and
per-character persistence; `GetLoadout` adoption for a meter row's player
("from combat log"). S8's answer decides whether the background painting is
a stacked image under the canvas (as in iced) or drawn in it. The
`real_dataset_lays_out_every_spec` test is ported.

**As built (2026-10-01), ahead of phase 3.** The viewer is a self-contained
entity that phase 3's window opens; the window itself is not wired yet.

- **Wave B.** Everything the iced viewer computed without drawing is
  gui-logic's `talents`: the layout (`model`), the editing state machine
  (`viewer`, iced's `TalentsUi`), and the `geometry`. The geometry covers
  hit-testing, the picker's tiles, octagons, arrowheads, carets, badges,
  the tooltip's lines as `Tone` roles with their placement, and the
  inventory's words. The iced viewer re-exports and draws it. A pane's
  `Retained` slot holds iced's tessellation caches. The iced talent design
  shots are byte-identical across the move. The colours are
  `Def::talents` (`TalentTokens`): `gold` holds iced's literals, and
  `frost` recolours paths, frames and plates. A `test-support` fixture
  (dataset, minted strings, pastes, `Sandbox`) serves all three crates'
  tests.
- **The viewer** (`gui-new/src/talents.rs` + `talents/`). S8 answered:
  each pane is ONE canvas painted in iced's layer order, with the
  background painting a cover-fit canvas under the trees and no stacked
  widgets.
  - The pointer is mapped through a hitbox, per pane, as iced's canvases
    did.
  - The tooltip lays out gui-logic's lines in a box at gui-logic's
    origin.
  - `GetLoadout` goes through `Session::request`; its answer comes back
    as a new `Reply` event (one-shot answers: `Loadout`, `History`,
    `Fight`, `HistoryChanged`), matched by `req_id`.
  - Esc and Tab are `CloseTalents` / `FlipTab` in a `Talents` key
    context.
  - The import field is Kit's `Input` at `.small()`. Root's rem is the
    theme's 14.5 px, which leaves `Medium`'s text box shorter than its
    1.25 rem line, so its descenders clip; the window's inputs will meet
    this too.
- **Parity.** At 1440 × 900 on the coiled-altar night the GPUI render
  matches iced's `wide-talents.png` to within a few pixels: the same
  tree geometry, the area 3 px lower, the import field a little
  shorter. Iced's painting overflows its canvas into the 10 px margin;
  here the area is full-bleed on purpose, so the two agree.
- **Delights**, each settling to the parity pixels under reduced motion:
  - a lit path grows from its parent (240 ms), its arrowhead arriving
    over the last fifth;
  - a node a press takes or ranks up sends out a fading gold ripple
    (520 ms);
  - an opening choice picker fans its options out from the node
    (170 ms);
  - a tooltip arrives with a short fade and a 4 px rise (140 ms);
  - the trees scale to fit a narrow window, down to 70%, before they
    scroll. They scroll from their left edge, with Kit's scrollbars,
    where iced cut a centred row;
  - "copy string" answers "copied ✓" for 1.4 s;
  - a SimulationCraft export file dropped on the viewer is read as a
    paste (the drop zone lights in the talent gold);
  - the theme recolours the tree (`frost`).
- **Tests.** gui-logic: 23 logic tests, the real-dataset layout, and a
  contrast test for every theme's talent words. gui-new: 11 harness
  tests (presses at a node's own offset on the pane canvases, hover and
  tooltip, keys, clipboard, a dropped file, typing + Enter, the loadout
  over a `Session`, the fit, a ripple ending), plus two ignored ones.
  `real_dataset_draws_every_spec` opens and draws all 40 specs and takes
  a root in each. `talent_shots` writes the fixture states, and with
  `WOWDPS_SHOTS_LOG` the owner's logged build at iced's three sizes.

## Phase 5 — cutover

**Readiness (2026-10-01), against spec §10's four criteria.**

1. **Every state reproduced and reviewed.** Reproduced and measured: the
   45 window states (three sizes) and the 20 overlay guard states, each
   beside iced's, in a local review page
   (`~/.local/share/wowdps/design-shots/gui-new-r2/review.html`, kept off
   the repository and off any service: the shots hold real player names).
   At 12 % fuzz the wide and tile frames differ in 0.4–2.1 % of pixels,
   the overlay in 0.2–2.4 %, and four narrow states (Damage,
   Healing, options, picker) in 3–4 %, from the
   one deliberate difference (a selected row past the fold comes into
   sight whole). Every overlay `Message` variant has a test or a named
   reason; every key the `?` sheet advertises is bound. **The user's
   review is the open half.**
2. **A raid week with gui-new's overlay as `gui_binary`.** Open. The user
   no longer raids; whether this criterion stands, is replaced by a
   replayed log, or is waived is the user's call. The replay is ready:
   [`tools/overlay-replay.sh`](../tools/overlay-replay.sh) streams a night's
   log at speed into an isolated daemon (its own runtime, config, data and
   state; the dev daemon untouched) while an overlay follows it on a
   headless output. The coiled-altar night (38 min of play, 1.2 M lines)
   at 4× ran through gui-new's overlay with no panic and no stderr, its
   memory flat, its picture iced's frame for frame.
3. **Cost no worse than iced's**, release builds on this machine. The
   window against the same running daemon, on the headless output (two
   runs each, 30 s idle after an 8 s settle):

   | Measure | iced | gui-new |
   | --- | --- | --- |
   | Window mapped after launch | 0.19–0.27 s | 0.20–0.27 s |
   | Idle CPU | 1.9–2.0 % | 0.36–0.40 % |
   | RSS | 214 MiB | 199–200 MiB |
   | PSS | 105–117 MiB | 106–119 MiB |

   The first gui-new run idled at 5.7 %: the live dot's endless pulse
   redrew the whole window about 28 times a second while the log's newest
   pull stayed open. It now pulses three times per live pull
   (`f429dec`).

   The overlay through the replay above (30 s samples):

   | Measure | iced | gui-new |
   | --- | --- | --- |
   | Idle before any data | 0.56 % | 0.23 % |
   | While the log streams (mean) | 0.89 % | 1.24 % |
   | Idle after (the staleness radar) | 1.66 % | 1.30 % |
   | RSS | 245 MiB | 195 MiB |

   Its first run streamed at 4–9.5 %: every meter bar eased 280 ms
   toward values that change ten times a second, so the overlay redrew at
   display rate as long as a pull went; bars now step while live
   (`b162d1e`). The remaining 0.35 points while streaming are GPUI's
   per-frame cost (a `perf` profile: allocation, taffy layout, style
   refinement, atlas lookups — no hotspot of ours) at the ~4 renders a
   second the snapshots drive; caching the overlay's static parts as
   views is the lever if the user wants parity there too.
4. **The user's sign-off.** Open. What the review has changed so far:
   - **The scrollbars** (2026-10-01): a thumb dragged a few pixels and
     stopped, because the bar kept its grip in itself and every frame
     builds it anew; a press on the rail did nothing. The grip lives in
     `scrollbar::Scroll` beside the list's handle, a press on the rail
     pages toward itself, and held, keeps paging.
   - **The picker names the pick** (2026-10-01, the user's call; a
     departure from iced and the prototype, where it always names the
     character played last and a pick only scopes Home, which read as
     stuck). It names the config's `character` (Home's scope) once the
     window knows them, else the character played. The menu's follow item
     stands only while a character is picked (with none it did nothing),
     and a press on it lets the pick go (Home, when up, on every
     character). The "you" marks, the class chrome and the rail's owner
     still follow the character played (`Gui::played`). The cutover's
     rewrite of CLAUDE.md says so.

When spec §10's four criteria hold, one PR:

1. delete `crates/gui`; `git mv crates/gui-new crates/gui`; package and
   binary become `wowdps-gui` (the fonts already live in gui-logic, 0.2.7);
2. `gui_binary` defaults to `wowdps-gui` again (the key stays);
3. the flake, home-manager and NixOS modules, dev unit and wrappers drop the
   second package;
4. CONTRACT.md's `gui:` clause loses iced; CLAUDE.md's GUI sections are
   rewritten for GPUI; the knowledge bundle's crate docs and the
   `gui-on-gpui` record are updated; SHOTS.md describes the new loop;
5. the release build is deployed with the dev-unit restart, and the user's
   config drops `gui_binary`.

## Cross-cutting

- **Measure as you go.** From 2.1 on, every phase records idle CPU at the
  10 Hz push rate, RSS, startup to first frame, and (overlay) frame time
  under a live pull, against the iced build on the same machine — the
  numbers phase 5's criterion 3 needs.
- **GPUI bumps** only between steps, as their own PR (spec §3).
- **Upstream.** If S3 keeps the edge strip, open a Zed PR adding a runtime
  `set_margin` for layer surfaces; adopt it in a later bump, never via a
  patch.
- **Keep the iced GUI alive.** Fixes to it during phases 1–4 go through
  gui-logic where the code lives there; view-only fixes are made in
  `crates/gui` and noted for the gui-new step that ports that view.

## Risks

| Risk | Mitigation |
| --- | --- |
| GPUI's API moves weekly | pin exactly; bump deliberately; the mirror and digests make diffs reviewable |
| The edge strip costs the game frames | S3 measures before phase 2 depends on it; recreate-on-release is the fallback |
| Headless renders differ run to run | the guard compares with a tolerance from the start (weekly lock updates move Mesa); S5 decides whether a pinned Mesa input earns exact hashes too |
| A Kit component's built-in spacing, size or animation fights the prototype | restyle through theme + `Styled` first; S12 names the gaps early; per-control fallback to the `gpui-base` primitive, noted in the step |
| The styled layer churns more than the primitives on a bump | the theme mapping is one function and the restyles live with each control, so a bump's breakage is local; the render guard and design shots catch visual drift |
| A colour literal sneaks past the theme | review rule (spec §6.1), and the theme-switch test samples both a Kit component and a bespoke surface |
| Text-dependent tests pass on fake metrics | they run in `HeadlessAppContext` with a real text system, never on `TestPlatform`'s stub |
| A UI that stops updating on some message kinds | `pump` notifies on any non-empty drain; a test per message kind the overlay draws (`CompareSnapshot`, `SegmentList`) |
| The dev unit restarting the raid daemon on a gui-new build | it watches only the configured `gui_binary` (0.3) |
| The harness is three weeks old | `VisualTestContext` underneath; contribute fixes upstream to Kit |
| Text metrics differ from iced's (cosmic-text either way, but a different layout engine) | the chrome budget is a test from 3.2; Tokens are sizes, not pixels |
| Workspace lints vs GPUI idioms | S10; scoped, justified allows only |
| Compile time | the crane dependency layer in CI; dev opt-levels; S9's numbers decide |
| Two GUIs drifting during the transition | one gui-logic, moved not copied; keybind parity over one table |

## Review log (devil's advocate, 2026-09-30)

An independent pass checked both documents against `crates/gui`, the
daemon, the TUI's tests and the GPUI mirror (`docs/gpui/src`). One blocker,
ten majors, seven minors; all are folded in above. What changed:

| # | Sev | Finding | Fix |
| --- | --- | --- | --- |
| 1 | blocker | wave A's "whole" modules import non-moving code (`config` → `theme::{Chrome, Density, class_named}`, `history` → `home::PAGE`, `tree` → `table::{Col, sorted}`, `list::split_pet`) | 0.2 step 0 moves those pieces first; `Density`'s sizes stay as a GUI extension trait |
| 2 | major | `keybind_parity.rs` greps `action_for`'s match arms, which the plan kept in `crates/gui` while pointing the test at gui-logic | a structured chord table in gui-logic; the parity test iterates it compiled; iced mapping unit-tested |
| 3 | major | notifying on `snapshot_gen` alone misses `CompareSnapshot`, `SegmentList`, `LoadFailed`, `Fatal` | `pump` notifies on any non-empty drain, plus a 1 s live clock |
| 4 | major | `#[gpui_kit::test]` runs on `NoopTextSystem` (0.6 em glyphs, ignores `add_fonts`), so the chrome budget would measure fake text | text-dependent tests run in `HeadlessAppContext` with a real text system |
| 5 | major | `RenderImage::new` takes `image::Frame` (not re-exported); `CosmicTextSystem` lives in un-re-exported `gpui_wgpu` | `image` named in the policy (no codecs); S5 finds a text route, else a `gpui-pre-wgpu` dev-dependency |
| 6 | major | "`Status` names the binary" is a wire change | dropped; `Failed` already names the path |
| 7 | major | the `gui_binary` rule lost the `$PATH` fallback the nix modules use | sibling, else `$PATH`; `/` means a path |
| 8 | major | the dev unit would restart the raid daemon on every gui-new build, and its pkill misses `wowdps-gui-new --overlay` | watch only the configured binary; widen the pkill |
| 9 | major | rebuilding iced image handles per call re-uploads textures every frame, invisible to the guard | a generic `Tiles<K, H>` cache moves instead |
| 10 | major | weekly lockfile updates move Mesa, so lavapipe hashes would churn | tolerance comparison from the start |
| 11 | major | `compare.rs`'s pure helpers the overlay needs were unscheduled, inviting copies | wave B at step 2.4 |
| 12 | major | matching monitors by bounds is wrong at fractional scale and under rotation | recompute the display's UUIDv5 from the output name; geometry from the configure size |
| 13 | minor | the strip need not be as wide as the widest state; edge-change drags rely on an implicit grab | width follows the state via `resize`; 1 px hidden; S3 tests the grab |
| 14 | minor | a perpetual drain timer never parks the test executor | tests call `pump`; no timer under test |
| 15 | minor | the overlay inventory missed eight behaviours; `g` is a click; "expanded" is no guard state | inventory derived from the `Message` enum; table fixed |
| 16 | minor | the key contexts named don't match `keys::Surface`; no spike for shifted / `ctrl +` keys | real names; S11 |
| 17 | minor | CI and build costs (GPUI on every PR, two wgpu / cosmic-text majors, canary apt list, `image` feature unification) | folded into S9; per-package builds |
| 18 | minor | layer `Closed` event, `focus: false` / no `app_id`, the raid week blocking phase 3, OKF and CLAUDE.md paths going stale, `use gpui_kit::*` shadowing `#[test]`, `ElementId` tuple syntax | each folded in where it applies |

Claims the review verified as correct: no runtime margin / anchor / layer
setter; `set_input_region` semantics and immediate commit; `display_id` →
`wl_output` and fractional-scale support; `current_headless_renderer()` on
Linux; `TestWindowExt`'s API and that it needs only gpui-base;
`open_window`'s `base::Root`; `add_fonts`; `RenderImage` is BGRA; scene
order follows paint order; layer surfaces can parent popups; the takeover
socket is unversioned; `wowdps gui-new` dispatches; every line count.

## Revision (2026-09-30, after the review)

The first draft took `gpui-kit` with `default-features = false` (only
`gpui-base`, Kit's unstyled layer), arguing that our Tokens draw every pixel
and that the styled crate's tree bought nothing. Checked against the
sources, the tree argument was wrong (`gpui-base` already carries markdown,
html5ever, lsp-types, ropey, schemars and chrono; the styled layer adds
rust-i18n, resvg, notify, uuid and the icon set) and the pixels argument was
overstated (134 component types implement `Styled`; the `Theme` has 134
colour slots plus fonts, radii, shadow and focus ring). The user chose the
styled layer for its styling range, with several themes planned: spec §3,
§4 and §11 now take `gpui-component` and `gpui-kit-assets`, §6.1 makes one
theme definition feed both Kit's `Theme` and our `Look`, S12 proves the
restyling and the switch, and steps 2.1, 3.1, 3.2 and 3.6 build the generic
controls from restyled Kit components.
