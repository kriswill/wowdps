# gui-new — specification (the GUI on GPUI)

Status: **draft**, 2026-09-30. The decisions in §11 were taken with the user
on 2026-09-30; everything marked **spike** is unproven and is settled by the
plan's phase 1 (`docs/plan-gui-new.md`) before the work that depends on it.
The GPUI reference this spec cites is `docs/gpui/README.md` (regenerate its
mirror with `tools/fetch-gpui-docs.sh`); `docs/gpui/src/…` paths are into
that mirror.

## 1. Intent

Re-implement both GUI surfaces — the **window** (`wowdps-gui`) and the
**overlay** (`wowdps-gui --overlay`) — on Zed's GPUI, through GPUI Kit, as a
new crate `crates/gui-new` (binary `wowdps-gui-new`). It replaces
`crates/gui` (iced 0.14 + iced_layershell 0.19, 57 k lines) once it reaches
parity. Nothing a user sees is meant to change in kind: the window is the
window redesign (`docs/design/window-redesign.html`, its Tokens and its
decision records), the overlay is today's overlay. The engine changes.

What the move buys, and what this spec has to prove it keeps:

- a retained, entity-based UI with real focus, key contexts and action
  dispatch instead of one message enum and a hand-routed keymap;
- a text system and layout engine (taffy) that ellipsise, measure and wrap
  natively — `ellipsis.rs` and the `responsive` measuring tricks go away;
- an in-process click-through test harness with element identities
  (`gpui_kit::test::TestWindowExt`), so interaction tests stop needing
  screenshots, and headless renders where pixels matter;
- the same native wlr-layer-shell surface the overlay needs, in stock GPUI;
- **styling range**: Kit's styled components override through `Styled` and
  read a runtime `Theme`, so the Tokens become one theme among several the
  app can ship later (§6.1). Keeping that range open is a reason to choose
  Kit, not a side effect of it.

## 2. Non-negotiables

1. **A pure client.** `wowdps-gui-new` links `wowdps-model`, `wowdps-proto`,
   the new `wowdps-gui-logic` (§5) and GPUI Kit — never `wowdps-core`. It
   cannot open a combat log; the compiler enforces it, as for `crates/gui`.
2. **No forked or patched GPUI.** Stock `gpui-pre` through `gpui-kit`, pinned
   exactly; no `[patch]` of any `gpui-pre*` crate, no `gpui-ce`. A capability
   GPUI lacks is designed around (§7.2) or contributed upstream, never vendored.
3. **The overlay in use never regresses.** The iced overlay stays the one the
   daemon spawns until cutover (§10); gui-new's overlay is opt-in by config.
4. **No wire change is required.** gui-new speaks `PROTO_VERSION` as it
   stands, through `ClientState`, as the window and overlay do today
   (`ClientKind::Window` / `Overlay`). If a phase discovers it needs one, it
   goes through the usual CONTRACT.md + golden-bytes + bump process.
5. **One writer per file on disk.** Both GUIs read and write
   `~/.config/wowdps/config.toml`, `~/.local/share/wowdps/simc/` and the
   overlay's takeover socket while they coexist; each of those has ONE
   implementation, in `wowdps-gui-logic`.
6. **House rules hold:** the workspace clippy denies (`unwrap_used`,
   `expect_used`, `indexing_slicing`, `panic`, …), `cargo fmt`, nightly
   toolchain with the stable floor, CI PR-only.
7. **The TUI is untouched**, and keeps every keybinding the GUIs have
   (`crates/tui/tests/keybind_parity.rs`).

## 3. Platform: which GPUI

`gpui-kit = "=0.7.0"` with its default features, `component` and `assets`
(no `tree-sitter*`, no `decimal`, no `inspector` outside debugging). Kit
pins `gpui-pre =0.3.7` (a crates.io publication of a recorded Zed commit;
the library is still named `gpui`) and brings: `gpui-pre` (the framework),
`gpui-pre-platform` (`application()`, `headless()`,
`current_headless_renderer()`), `gpui-pre-linux` (Wayland via
`wayland-client` with `dlopen`, X11 via `x11rb`), `gpui-pre-wgpu` (renderer,
cosmic-text + swash); **`gpui-base`**, Kit's unstyled behaviour layer
(primitives: tabs, tooltip, popover, dialog, sheet, toast, input, select,
scrollbar, tree, table, virtual list, plot, focus trap, Root; the test
harness's observation, `gpui_base::test_support`); **`gpui-component`**, the
styled library over it (Root with its dialog / sheet / notification layers,
Button, Tabs, Input, Popover, Menu, Tooltip, Kbd, Tag, Notification,
Command, Sheet, Scrollable …, 134 types implementing `Styled`) and its
`Theme` (134 colour slots, font family and size, mono font, radii, shadow,
focus ring, list / sheet / notification settings) with the `ThemeRegistry`
that loads `ThemeConfig` files; and **`gpui-kit-assets`**, the Lucide icon
set the components draw their chevrons, checks and close glyphs from.

**Why the styled layer (user, 2026-09-30).** Styling range is the point:
every component overrides through `Styled`, every colour it draws is a
`Theme` slot that changes at runtime, and the app will want several themes.
Building the generic controls on bare primitives would mean re-creating
that range by hand. What it adds over `gpui-base` in crates is modest —
`gpui-base` already carries markdown, html5ever, lsp-types, ropey, schemars
and chrono; the styled layer adds rust-i18n, resvg, notify, uuid and the
icon set. **Per control:** a generic control (tab, chip, toggle, menu,
popover, tooltip, input, dialog, sheet, toast, kbd hint, scrollbar) is
Kit's component restyled through the theme and `Styled`; it falls back to
the `gpui-base` primitive plus our styling only where the component cannot
reach the prototype's values, and that fallback is noted in the step that
makes it. Bespoke surfaces (the meter grid, ribbon, plots, lanes, stacked
graph, recap, rail rows, Home charts, the overlay) are built from GPUI's
`div` / `canvas` / `uniform_list` and read their colours from the theme.

Not `gpui` 0.2.2 (docs.rs/gpui/latest): published 2025-10, and it has no
layer-shell windows at all.

**Bumps are deliberate.** gpui-pre snapshots arrive about weekly; Kit
follows. A bump is its own commit: `tools/fetch-gpui-docs.sh <kit-version>`,
read Kit's release notes, `cargo update -p gpui-kit`, the whole test suite
plus the render guard (§9), re-bless only for an intended change.

**Platform scope:** Linux, Wayland, Hyprland first — the only platform the
overlay targets. X11 and other compositors must not crash (the window opens
as a normal window; the overlay reports `LayerShellNotSupportedError` and
exits with a clear message the daemon surfaces in `Status`). macOS/Windows
builds are not a goal; layer-shell code sits behind
`cfg(target_os = "linux")` so they stay possible.

## 4. Dependencies (CONTRACT.md amendment)

The `gui:` clause of CONTRACT.md §Dependencies as amended — signed off and
adopted 2026-09-30 (plan step 0.1); CONTRACT.md is the binding text, and
this is its rationale:

> gui: iced + iced_layershell + serde/toml **(until gui-new's cutover)**.
> gui-new: `gpui-kit` pinned exactly, with its default features
> (`component`, `assets`) and no `tree-sitter*` feature — the `gpui-pre`
> family, `gpui-base`, `gpui-component` and `gpui-kit-assets` it pins are
> accepted as one unit, including what they pull transitively
> (serde/serde_json, smol, futures, chrono, regex, image, wgpu,
> cosmic-text, taffy, rust-i18n, resvg, notify, uuid …), none of which our
> own code may name except serde/toml and `image` — plus serde/toml, and
> `image` with `default-features = false` at the version `gpui-pre-wgpu`
> locks, named only to build `RenderImage` frames (`RenderImage::new` takes
> `image::Frame`; gpui does not re-export the crate). No `[patch]` of any
> `gpui-pre*` or Kit crate. gui-logic: model + proto + serde/toml.
> Dev-dependencies: `gpui-kit` with `test-support`, the in-repo
> `wowdps-daemon` (its `mock`) and `wowdps-core` — for gui-new and
> gui-logic alike (`history.rs`'s tests drive `MockDaemon`) — and, if S5
> finds no other route to a real text system for `HeadlessAppContext`,
> `gpui-pre-wgpu` (its `CosmicTextSystem` is not re-exported by Kit).

One more name is left to a spike: `uuid` (S4 — matching a Hyprland monitor
to a `DisplayId`; already in the tree through `gpui-pre-linux`; the
alternative is a hand-rolled SHA-1 for UUIDv5 in gui-logic). A new direct
name is a policy line, never a quiet `use`.

"No chrono / no tokio / no serde outside the gui" keep their meaning for our
code: GPUI needs no tokio; chrono arrives transitively and is never named.

## 5. The shared crate: `crates/gui-logic` (`wowdps-gui-logic`)

Framework-free GUI logic both GUIs use while they coexist, and gui-new keeps
after cutover. Depends on model + proto + serde + toml. **Code moves, it is
never copied:** each item leaves `crates/gui` for gui-logic in a
behaviour-neutral commit, `crates/gui` re-imports it, and the iced overlay's
snapshot guard plus the existing tests prove nothing moved but the code.

Wave A (plan phase 0, before any GPUI code) — whole modules that are
iced-free or nearly so. "Whole" needs a few iced-free pieces of OTHER
modules to move first, or the moves do not compile (review finding 1):
`theme::{Chrome, Density}` as name-only enums with `class_named` (config
reads them; `Density::row_h` / `pad` stay in the GUI as an extension trait
over `theme::pitch`), `home::PAGE` (history pages by it), `table::{Col,
sorted}` and `inspector::list::split_pet` (the ability tree uses them).
history.rs's one test that needs `fight_head::badge` stays in `crates/gui`.

| Today | Lines | Moves as |
| --- | --- | --- |
| `config.rs` | 619 | whole — the one reader/writer of `config.toml`, `store_*` casual-gesture writes |
| `hypr.rs` | 900 | whole — Hyprland IPC, game window/monitor/workspace lookup |
| `single.rs` | 88 | whole — the overlay's unversioned takeover socket (so either GUI's overlay evicts the other's) |
| `fold.rs` | 224 | whole — accent folding for the filter and palette |
| `simc.rs` | 434 | whole — SimulationCraft export parser + per-character persistence |
| `history.rs` | 1 059 | whole — `Earlier` pages, `Stored` pulls' synthetic snapshots from `GetFight` |
| `inspector/tree.rs` | 638 | whole — R26 ability-tree lines |
| `keys.rs` | 513 | a STRUCTURED chord table (`Chord` → action, plus `BINDINGS`' display rows and `Surface`) replaces `action_for`'s match arms; `crates/gui`'s `action_for` becomes iced key → `Chord` → table lookup, unit-tested there. `keybind_parity.rs` stops grepping source and iterates the table (the TUI gains gui-logic as a dev-dependency), so both GUIs and the TUI are held by one compiled test |
| `icons.rs`, `spell_icons.rs`, `talent_art.rs`, `lazy_tiles.rs` | 876 | the cache readers decode to RGBA tiles, and the tile CACHE moves generic — `Tiles<K, H: Clone>` with a `make: FnOnce(Vec<u8>) -> H` — so each GUI caches its own handle type (iced `Handle`, GPUI `Arc<RenderImage>`). Rebuilding a handle per frame would give iced a new image id and re-upload the texture every frame, with identical pixels the guard cannot catch |
| `fonts/` + `theme::FONTS` | — | the OFL files and their README move to `crates/gui-logic/fonts/`; `FONTS` becomes a `&[&[u8]]` both GUIs load |

Wave B (moved by the phase that first needs it, never earlier):
`theme.rs`'s token VALUES and WCAG helpers (`chrome_base`, `class_text`,
`Accent`) as plain RGB; `timeline::{blocks, block_of, scrub, collapse,
is_live}`; the rail's night/visit grouping and ordering; Home's week
derivation and `standing`; the palette's search and ranking; the table's
column sets, sort and filter; `taken.rs`'s per-debuff level-0 derivation;
the Deaths ordering; the lanes' and stack's slot seating; `fight_head::Seen`;
and `compare.rs`'s pure functions the overlay's drill and comparison need
(`view_draws`, `view_draws_mark`, `for_view`, `kinds_shown`, `view_window`,
`curve`, `peak_of`) — scheduled at plan step 2.4, so they are moved, not
copied, when the overlay first draws a graph.
Each is a function over model/proto types today, wrapped in view code; the
wave splits the function out. Anything still drawing stays in its GUI.

## 6. Architecture of gui-new

```
crates/gui-new/src/
  main.rs            args (--overlay), application().run, fonts, keymap, open the surface
  session.rs         Session entity: one daemon link + its ClientState
  keys.rs            gpui actions + KeyBindings generated from gui-logic's table
  theme.rs           the active gui-logic theme::Def → Kit Theme + our Look global (§6.1)
  images.rs          RenderImage cache over gui-logic's decoded tiles
  widgets/           table, plot primitives (area, line, spline, ticks), line icons, chips
  overlay/           mod (view), layer (surface, input region, output), strip, footer,
                     drill, compare, menu, options
  window/            mod (root), top_bar, rail, stage, fight_head, ribbon, tabs, meter,
                     deaths, inspector/{mod, plot, lanes, list, tree, stack, recap, taken},
                     home/{mod, charts, panels}, palette, sheet, talents/{mod, art}
```

**Sessions.** A `Session` entity owns one daemon link and its `ClientState`
— the window's stage, each stored pull (`history::Stored`), the overlay, and
the overlay's Σ-split second connection are each a `Session`, exactly the
`ClientState`s that exist today. The link is a trait with two impls: the real
`DaemonClient` (`try_reconnect`, never a blocking wait on the UI thread — the
628-daemons lesson) and the daemon's `MockDaemon` for tests. All the work
is one method, `Session::pump(cx)`: drain the link, apply each message to
`ClientState`, send the requests it returns, and `cx.notify()` when the
drain was non-empty. NOT on `snapshot_gen` alone — it moves only on
`Snapshot`, so `CompareSnapshot`, `SegmentList`, `LoadFailed` and `Fatal`
would leave the comparison and the strip stale (review finding 3); the
daemon already pushes only changes, at 10 Hz. In the app a foreground task
calls `pump` every `window::TICK` (100 ms) on a background-executor timer,
and a 1 s clock notifies while a live pull is on screen (its duration
ticks). Under test no timer is spawned: tests call `pump` themselves, since
`run_until_parked` advances the test clock to the next timer and a
perpetual loop never parks. The overlay's 33 ms animation tick is a
separate task that runs only while an animation is in flight.

**Views are entities, renderers are functions.** Each pane that owns state
(rail, stage, inspector, palette, talents, overlay) is an entity
implementing `Render`; stateless pieces (a meter row, a recap entry, a chip)
are `RenderOnce` components or plain functions returning elements. Panes talk
through events (`EventEmitter` + `cx.subscribe`) and the shared `Session`,
never by reaching into each other.

**Keys.** gui-logic's binding table becomes gpui `actions!` and
`KeyBinding`s registered at start, each in the key context of the
`keys::Surface` it names (`Meter`, `Drill`, `Ability`, `Compare`, `Home`,
`Talents`, `Rail`), plus contexts for the modal layers that swallow keys
today (`Palette`, `Filter`, `Sheet`, `Menu`). GPUI's context precedence does
what `Gui::surface` and the "keys swallowed while X has focus" rules do by
hand today; the Esc walk (talents → menus → drawer → filter → inspector →
comparison → Home) stays one explicit `Escape` handler at the window root,
because its order is a product rule, not a focus fact. The overlay has no
keyboard (`KeyboardInteractivity::None`), as now.

**Ids are the test surface.** Every element a user can click, and every
region a test asserts on, gets a stable `ElementId` keyed by identity, not
position — a row by its guid (`ElementId::NamedChild` under a `"row"` name;
there is no `(&str, String)` conversion) — and opts into observation with
`.test_support()`: the same ids the harness clicks (§9). Test modules import
Kit's items explicitly: with `test-support`, `use gpui_kit::*` brings GPUI's
`test` macro and shadows `#[test]`.

### 6.1 Themes

Several themes are coming, so no surface reads a colour, size or font
literal; everything draws from the active theme.

- **One definition.** A wowdps theme is a framework-free value in gui-logic
  (`theme::Def`): the prototype's surfaces (ground, surface, raise, line,
  edge), three inks, gold / gold-dim / gold-ink, good, bad, legendary, the
  overlay's palette (`Look::OVERLAY`'s values), font families, type sizes,
  row pitches and radii. The built-in `gold` definition IS the prototype's
  Tokens. Class chrome (`chrome = "class"`) is not a second theme but a
  runtime accent override on any theme: the owner's class colour, moved
  along its hue until ink on it clears WCAG AA, as today.
- **Two targets, one mapping.** gui-new turns the active definition into
  (a) Kit's `Theme` through `Theme::update`: `background` ← ground,
  `foreground` ← ink 1, `muted_foreground` ← ink 3, `border` ← line,
  `accent` / `primary` / `ring` ← the chrome accent, `popover` ← raise,
  `list_hover` / `list_active` / `tab_active` / `selection` from the hover
  and selection styles, `danger` / `success` ← bad / good, plus font family,
  size, radius, and shadow off, so every Kit component follows; and (b) our
  `Look` global for what Kit has no slot for (ink levels, gold-dim,
  legendary, type sizes, row pitches, the overlay palette). One function
  maps definition → both. A Kit slot we leave unmapped keeps Kit's dark
  default, so the mapping is tested to cover every slot a component we use
  reads.
- **Class colours are data**, not theme (`Class::rgb` on a bar, exactly);
  `class_text`'s AA lift is computed against the ACTIVE theme's surface.
- **Switching** is `Theme::update` + replacing `Look` + `cx.refresh_windows()`.
  A test switches between two definitions and checks that a Kit component
  and a bespoke surface both changed.
- **Later.** More built-in definitions (light, high-contrast, …) and user
  themes from `~/.config/wowdps/themes/`. Likely Kit `ThemeConfig` JSON
  carrying our extra tokens beside Kit's slots, hot-reloaded through
  `ThemeRegistry::watch_dir`; the format is decided when the first extra
  theme is built. A config key `theme` joins `chrome`. The phases ship
  `gold` with `chrome = gold | class`, today's choices, and nothing more —
  but nothing they build may assume there is only one.
- **The overlay** draws from the theme's overlay palette. The built-in
  definition's overlay palette is today's, so "same overlay, new engine"
  holds, and a future theme can restyle it.

## 7. The overlay

### 7.1 Parity target

Today's overlay, screen for screen. Its snapshot guard names the states that
define it, and each becomes a gui-new interaction test plus a guard render:
meter, hover (first / second row), drill, drill with hover, spell drill,
tree drill (shut / every group open), compare, compare zoomed, collapsed,
Σ split, options card, view menu, Taken / Deaths / Enemies drills, Enemies
meter, interrupts, live, arena, expanded. Behaviour beyond pixels, from
`crates/gui/src/overlay.rs`: edge tab ↔ expanded panel, grip drag along the
edge (offset persisted), zoom, the instance strip (Σ–①─②─③–⚑, block
stepping, chip scrub, re-pin Live on a new pull), the footer view name
(left-click cycles, right-click opens the view menu, which leaves an open
drill alone), the Σ toggle (`overlay_split`, a second `Window`-kind
connection), class-icon picks into Compare (grown to `COMPARE_MIN`),
per-spell hover lit in both lists, drag-zoom on the drill graph,
R26 rollups in the drill (`Overlay::tree_open`), the options card (ranks,
realms), discard-trash, every `WOWDPS_OVERLAY_*` debug/auto env var, the
`WOWDPS_OVERLAY_GAME_STARTING` 30 s wait for the game window, Hyprland
workspace following (`follow_game`, `game_match`), birth on the game's
monitor unless `monitor` is set, single-instance takeover, and the daemon's
`SetVisible` composing with the user's manual hide. Also, from the
`Message` enum (`overlay.rs` 454–551), which is the checklist's real source
— every variant gets a test or a named reason it has none: the time cursor
shared across every graph (`GraphProbe`), a compare marker lighting every
use of its item (`CompareHover`), a compare spell row drilling both sides
(`CompareSpell`), the wheel over the strip scrubbing members
(`StripScroll`), the wheel over the header zooming, right-click backing out
of a drill or clearing the pair, the grip telling a click from a drag, and
the options card and view menu closing when the pointer leaves them. The
overlay has no keyboard, so every toggle (`g` in the window) is a click
here.

Look: `Look::OVERLAY` — the overlay's palette, monospace numbers and yellow
— carried as values into gui-new's theme. It cannot match iced byte for
byte (different rasteriser and text stack); it must match by eye at 1:1
against the iced guard's PNGs (`WOWDPS_GUARD_PNG`), and gets its own hashes.

### 7.2 The surface

`WindowKind::LayerShell(LayerShellOptions { namespace: "wowdps", layer:
Layer::Overlay, anchor, margin, exclusive_zone: None, keyboard_interactivity:
KeyboardInteractivity::None })` with `WindowBackgroundAppearance::Transparent`
(`docs/gpui/src/gpui-pre-0.3.7/examples/layer_shell.rs`).

**The edge strip (spike S3).** gpui-pre sets a layer surface's margin, anchor
and layer only at creation — there is no runtime `set_margin` — and today's
grip drag moves the surface along its edge by changing the margin live. The
design that needs no fork: the surface spans its edge's LENGTH (anchored to
the edge and both of its neighbours, e.g. `RIGHT | TOP | BOTTOM`), while
its WIDTH follows the state through `Window::resize` (which a layer surface
honours as `set_size`): the tab's width collapsed, the panel's expanded,
`COMPARE_MIN` while comparing. It is fully transparent outside the content;
the content sits inside it at the configured offset, and
`Window::set_input_region` restricts input to the content's bounds every
time they change (it commits immediately), so the rest of the strip passes
every click to the game. A drag moves content and input region, not the
surface. Changing EDGE (left ↔ right) recreates the surface: rare, may
flicker, and the drag that does it leaves the strip, so it relies on
Wayland's implicit pointer grab (S3 tests it). The spike measures what this
costs (a full-length transparent layer over a fullscreen game: compositor
load, frame time, whether Hyprland still tears or scans out as it does now)
against recreating the surface on drag release, and checks the stretch with
a non-zero initial length; an upstream `set_margin` for layer surfaces is
worth proposing to Zed either way.

**Hidden** = an empty input region (`set_input_region(Some(&[]))`) over a
1 px-wide frame that draws nothing — layer-shell has no unmap, as today's
1×1 surface works around. **Opening:** `focus: false` and no `app_id`, so
GPUI asks for no xdg-activation; a layer surface's `Closed` event (its
output went away) ends the process cleanly, and the supervisor's restart
rules take it from there. **Output:** `WindowOptions::display_id` becomes
the layer surface's `wl_output`. `PlatformDisplay` has no name accessor,
but its `uuid()` is `Uuid::new_v5(NAMESPACE_DNS, output_name)`
(`gpui-pre-linux` `wayland/display.rs:36`), so gui-logic's
`hypr::game_monitor` name maps to a display EXACTLY by recomputing that
UUID (S4). Display bounds are not used for anything: GPUI derives them from
the unrotated mode divided by the integer output scale, wrong at fractional
scale and under a transform (DP-1 is rotated). The overlay's geometry comes
from the layer surface's own configure size.
**Scale:** iced_layershell's two bugs that `overlay.rs` works around (bare
`SizeChange`, custom `scale_factor` breaking hit-testing) are that crate's;
the overlay's zoom becomes a plain multiplier on its layout sizes, and S4
checks hit-testing at the monitor's real scale.

## 8. The window

The window redesign is the spec: `docs/design/window-redesign.html` (CSS
values and JS behaviour), its Tokens, the decision records in
`docs/OKF/decisions/` (`window-redesign` and the steps it links:`gold-chrome-and-a-look-per-surface`,
`one-fight-header-over-the-meter`, `inspector-beside-the-meter`,
`one-pull-rail`, `raid-timeline-and-mine-on-the-wire`,
`home-derives-from-fights`, `ability-tree-in-the-inspector`,
`cooldown-windows-per-view`) and CLAUDE.md's GUI sections, which describe the
window as built. gui-new rebuilds that window; it does not redesign it.
Reference renders: `~/.local/share/wowdps/design-shots/reference/`.

| Area (today) | Lines | gui-new |
| --- | --- | --- |
| `window.rs` (state, messages, drain, Esc, stored pulls) | 7 491 | root entity + `Session`s; message plumbing becomes entity methods and actions |
| `view.rs`, `table.rs`, `reveal.rs`, `ellipsis.rs` | 5 572 | one `widgets::table` over taffy grid/flex; `text_ellipsis()` / `truncate()` replace `ellipsis.rs`; tab reveal is a scroll-into-view on the active tab |
| `top_bar.rs`, `fight_head.rs`, `ribbon.rs` | 3 761 | entities over the stage `Session`; ribbon is one `canvas()` (area + line paths, skull glyphs, hover crosshair via a hitbox) |
| `inspector.rs` + `plot`, `lanes`, `list`, `tree`, `stack` | 8 235 | `inspector/`: plot = `canvas()` + `PathBuilder` (Catmull-Rom → cubic Béziers), drag-zoom on mouse handlers; lanes a canvas; lists `uniform_list`; the tree's lines from gui-logic |
| `deaths.rs`, `taken.rs`, `compare.rs` | 4 765 | Deaths table and recap, R21 matrices, the pair — renderers over gui-logic derivations |
| `rail.rs`, `history.rs`, `timeline.rs` | 4 799 | rail entity + drawer (Kit `Sheet` below 1180 px); grouping from gui-logic |
| `home.rs` + `charts`, `panels` | 2 681 | Home entity; charts are canvases; panel grid is flex-wrap at the 330 px minimum |
| `palette.rs`, `nav.rs` (menus, chips, filter box) | 2 600 | Kit components restyled: the palette on Kit `Dialog` + `Input` with our search (or Kit `Command`, if it can carry our sections and ranking — decided at step 3.6), menus on `Popover` / `Menu`, chips on `Tag` / `Toggle`, the filter on `Input`, toasts on `Notification`, `?` sheet keys on `Kbd` |
| `talents.rs`, `talent_art.rs`, `simc.rs` | 4 760 | phase 4; see §8.1 |
| `theme.rs`, `line_icons.rs`, `gauge.rs` | 1 913 | the theme definition and its mapping (§6.1); the prototype's 16-unit glyphs as `PathBuilder` strokes, or registered as SVG assets beside Kit's Lucide set (GPUI's `svg()`, which can recolour them per theme) — decided at step 3.1 |

Breakpoints (`TILE_WINDOW` 1180, `NARROW_WINDOW` 820) are read from the
window's viewport on resize, as `Gui::fit` does. Type: the bundled OFL fonts
(gui-logic's `FONTS`, §5) load through `cx.text_system().add_fonts` before the
first window opens; the tabular digits are baked into the family, so no
OpenType feature is needed. The chrome budget (first meter row ≤ 290 px down
at 1440×900, 18 rows without scrolling) is a test from the first phase that
draws a meter.

### 8.1 Talent viewer

Last to move. The iced viewer has to draw the background painting as a
stacked image under its canvas because iced composites a frame's images
above its vectors; GPUI's scene orders primitives by paint order within a
layer (spike S8 confirms), which may let the tree draw in one canvas. The
dataset, import-string codec and logged-loadout path are `proto::talents`
and unchanged.

## 9. Testing

| Layer | Tool | Runs |
| --- | --- | --- |
| gui-logic | plain `#[test]`, moved with the code | every `cargo test` |
| interaction | `#[gpui_kit::test]` + `TestWindowExt` (`find`, `within`, `click`, `click_at`, `right_click`, `hover`, `press`, `input`, `scroll`, `drag`, `drag_to`; snapshots of role / label / value / selected / expanded / bounds / visible / focused) over `MockDaemon::fixture_at(…)` through the `Session` link trait | every `cargo test`; no GPU, no display |
| layout with real text | the same harness inside `HeadlessAppContext` with a real text system — `#[gpui_kit::test]`'s `TestPlatform` uses `NoopTextSystem`, which ignores `add_fonts` and makes every glyph 0.6 em, so any test whose answer depends on text metrics (the chrome budget, truncation, the tab reveal, column widths) runs here. Opening its headless window needs a wgpu adapter even when nothing is captured (found at the cutover): the GPU, or Mesa's lavapipe in CI and the nix sandbox | every `cargo test` |
| render guard | `HeadlessAppContext` + `gpui_platform::current_headless_renderer()` (`WgpuHeadlessRenderer` on Linux) → PNG per overlay state under `crates/gui/snapshots/overlay/` (`crates/gui-new/…` before the cutover), compared with a TOLERANCE (per-channel delta and differing-pixel budget) from the start, not SHA-256: lavapipe is Mesa, and the weekly `nix flake update` moves Mesa, so exact hashes would churn every Monday. S5 decides whether a separately pinned Mesa input makes exact hashes worth adding | ignored test, run alone, needs a wgpu adapter (GPU, or lavapipe) |
| design shots | the same renderer over the window states SHOTS.md lists, at the prototype's three sizes | ignored test, manual review against the reference renders |
| chrome budget | `bounds` of the first meter row, in the real-text context above | every `cargo test` (synthetic 25-player raid), plus the real-log variant |
| keybind parity | `keybind_parity.rs` reading gui-logic's table | every `cargo test` |
| no engine | gui-new's manifest has no `wowdps-core` outside dev-deps | the compiler |

Harness limits that shape the code: there is no visible-text getter (assert
through an `aria_label` that already means something, or by reading the
entity), and a canvas is one target (wrap it in an id'd `.test_support()`
element, `click_at` an offset, assert the entity's state). The harness is
young (in Kit since 0.6.6, first commit 2026-09-08) but it is how Kit tests
itself (278 tests); where it falls short, gpui-pre's `VisualTestContext`
(`simulate_click`, `simulate_keystrokes`, `dispatch_action`,
`debug_selector` + `debug_bounds`) is underneath and usable directly.

## 10. Coexistence and cutover

**While both exist.** The daemon spawns `wowdps-gui` unless config
`gui_binary` names another. Resolution keeps today's rule
(`crates/daemon/src/lib.rs` 109–114) for any bare name: a sibling of the
daemon binary when one exists there, else the name on `$PATH` — the
home-manager and NixOS modules depend on that `$PATH` step — and a value
containing `/` is a path. `gui_binary = "wowdps-gui-new"` makes gui-new's
overlay the one that follows the game. No wire change: `Status` is not
extended to name the binary (that would be a `PROTO_VERSION` bump); a spawn
failure already names the path in the overlay's `Failed` state.
`wowdps gui-new` launches the new window through the dispatcher's existing
external-command lookup. The dev unit stamps and watches only `wowdps` and
the CONFIGURED `gui_binary` — watching both GUIs would bounce the live
daemon and its overlay on every gui-new build — and its stop path kills
either overlay (`wowdps-gui(-new)? --overlay`), so an orphan cannot respawn a
daemon while it reconnects. The flake gains `.#wowdps-gui-new` (and its
crane dependency layer) beside `.#wowdps-gui`, and the modules a way to put
it on the service's `$PATH`. Both overlays already share one takeover socket
(it is unversioned, and gui-logic holds the one implementation), so
launching either evicts the other — never two surfaces at once.

**Cutover** when every one of these holds:
1. every overlay guard state and every design-shot state is reproduced and
   reviewed, and the feature inventory in §7.1 and CLAUDE.md's window
   sections is ticked off;
2. gui-new's overlay has been the configured `gui_binary` through at least
   one week of real raiding with no open regression;
3. idle CPU, memory and startup time are no worse than the iced build's
   (measured, release, same machine);
4. the user signs off.

Then, in one change: `crates/gui` is deleted, `crates/gui-new` becomes
`crates/gui` (package and binary `wowdps-gui`), `gui_binary` defaults back to
`wowdps-gui`, the flake/home-manager/NixOS modules/dev unit drop the second
package, CONTRACT.md's `gui:` clause loses iced, CLAUDE.md and the knowledge
bundle follow, and the iced overlay guard's hashes go with their crate.

## 11. Decisions

| Decision | Why |
| --- | --- |
| Side by side, then rename (user, 2026-09-30) | The overlay in use keeps working the whole way; the switch is a config key, and the rename is mechanical at the end |
| Extract a shared crate, move don't copy (user) | One config writer, one takeover socket, one keymap while two GUIs run; the iced guard proves each move |
| Same overlay, new engine (user) | The overlay was never part of the redesign and is played with every night; a restyle is a separate design question |
| Overlay first (user) | Smallest surface, the in-game one, and it exercises the riskiest platform code (layer-shell, input region, output choice) before the window's months of work |
| No GPUI forks (user) | gpui-pre moves weekly; a carried patch is a tax on every bump |
| Kit's styled layer (`gpui-component`), restyled per control; `gpui-base` primitives only where a component can't reach the prototype (user, 2026-09-30) | Styling range: `Styled` overrides on every component and a runtime `Theme` are what several themes need; it adds little over `gpui-base`'s tree |
| One theme definition feeds Kit's `Theme` and our `Look`; no literals | Themes will multiply; a colour that bypasses the definition is a colour the next theme cannot change |
| Kit's `TestWindowExt` as the interaction harness | In the crate we depend on, the vendor's own, no fork; third-party GPUI drivers all need patches or hand-built trees (`docs/gpui/README.md`) |
| The overlay surface spans its edge's length, its width follows the state; the input region follows the content | Grip drag without a runtime margin setter, no fork (spike S3 may overturn) |
| A display is found by recomputing its UUID, never by bounds | GPUI's UUID is v5 of the output name — exact; its bounds are wrong at fractional scale and under rotation |
| Interaction tests that depend on text metrics run in `HeadlessAppContext` | the default test platform's text system is a stub |
| The render guard compares with a tolerance | Mesa moves weekly under the lockfile update; exact hashes would churn |

## 12. Spikes (plan phase 1)

| | Question | Exit criterion |
| --- | --- | --- |
| S1 | Layer surface on Hyprland: namespace, layer, anchor, margin, transparency | an overlay-layer surface over a fullscreen game, transparent, no focus stolen |
| S2 | `set_input_region`: empty and partial regions | clicks pass through outside the region, land inside it |
| S3 | The edge strip vs recreate-on-release | a decision with numbers: frame time, compositor CPU, game frame pacing |
| S4 | Output choice + fractional scale | the game's monitor found by recomputed UUIDv5 of its Hyprland name (`uuid` named, or SHA-1 in gui-logic — decided here); born on it; hit-testing right at its scale and on the rotated DP-1 |
| S5 | Headless renders and real text on Linux | `render_to_image` works under lavapipe in the nix shell; a route to a real text system for `HeadlessAppContext` without naming `gpui-pre-wgpu` (`gpui_kit::platform::current_platform(true).text_system()`), or that dev-dependency; the guard's tolerance set; whether a pinned Mesa input makes runs byte-identical |
| S6 | Fonts | Barlow Semi Condensed Tabular and Marcellus load and render; the overlay's ⚙ Σ ☠ ● resolve |
| S7 | Images | an RGBA tile becomes `RenderImage` through `image::Frame` (BGRA order — swap in the decoder, once); lazy spell tiles stay lazy; the generic `Tiles` cache holds `Arc<RenderImage>` |
| S8 | Canvas | area + spline paths, text in a canvas, an image painted over a path (paint order follows the bounds tree, `scene.rs`, so this should pass) |
| S9 | Build | nix shell libs, a RUNPATH bake like `crates/gui/build.rs`, clean-build time, binary size, `[profile.dev.package]` opt-levels; CI cost — the `ci` job's clippy and tests now build GPUI on every PR, and iced's wgpu 27 / cosmic-text 0.15 build beside GPUI's wgpu 29 / cosmic-text 0.19 until cutover; the stable canary's apt list; `image` feature unification leaking codecs into the iced binary in workspace-wide builds (per-package builds avoid it) |
| S10 | Lints | the workspace clippy denies hold over GPUI idioms (`actions!`, `cx.listener`, `open_window`'s `Result`) |
| S11 | Keys | shifted bindings (`K T E ? ~ H`) and `ctrl +` / `ctrl =` parse and fire through GPUI's keystroke parser on Linux layouts; the Esc walk at the root does not fight context precedence |
| S12 | Theming (§6.1) | a `theme::Def` mapped onto Kit's `Theme` and a `Look`; Kit `Button`, `Tabs`, `Input`, `Tooltip` and `Popover` restyled to the prototype's values, each reaching them or with its gap named (→ the `gpui-base` fallback for that control); a runtime switch to a second definition repaints a Kit component and a bespoke canvas alike |

### Findings (2026-09-30, plan step 1.3)

The probes are `gui-new`'s ignored tests (`meter::render_probe`,
`probes::*`, `keys::tests`), run with `WOWDPS_SHOTS_DIR` set to keep their
PNGs. No spike failed its exit criterion. Three are partly blocked on the
running game.

**S1 — met, except over a running game.**
- A `Layer::Overlay` surface with `KeyboardInteractivity::None` and a
  transparent background mapped on Hyprland. Its corners showed the
  wallpaper through, and the active window kept focus.
- **It must not wear Kit's Root.** On a client-decorated surface (a
  layer surface always is), Root adds a 20 px shadow border, which it also
  sets as the client inset, and paints the theme's ground. A 28 × 96 tab
  mapped as 68 × 136. The overlay opens through `cx.open_window`.
- Not yet checked: over a fullscreen game. That waits for the game to
  run.

**S2 — moved to step 2.2.** Proving click-through needs a pointer, and
the real seat is never driven. The input region is built and tested with
the overlay's surface in 2.2.

**S3 — open until the game runs.** The edge strip (full edge length,
input region following the content) stays the design. Its cost to frame
pacing is measured with the game running, during 2.2's live checks or
the raid week, against recreate-on-release.

**S4 — met.**
- `gui_logic::output::output_uuid` is RFC 4122 UUIDv5 over a 20-line
  SHA-1, checked against the FIPS vectors and Python's
  `uuid5(NAMESPACE_DNS, "python.org")`. It finds a display by comparing
  bytes with `display.uuid()?.as_bytes()`, so no `uuid` dependency.
- **Wayland names its outputs only after the event loop turns:**
  `cx.displays()` is empty at launch. The overlay therefore opens from a
  task that waits up to a second for the named display.
- On HEADLESS-1 the tab was born on the named output and flush with its
  right edge, at scale 1, at fractional scale 1.5 (crisp at the higher
  density), and rotated 90°.

**S5 — met.**
- `gpui_kit::platform::current_platform(true).text_system()` is the
  real `CosmicTextSystem` with no `gpui-pre-wgpu` dependency, and
  `HeadlessAppContext::capture_screenshot` renders through wgpu.
- One frame captured twice is byte-identical. On Mesa's lavapipe the
  same frame differed from the GPU's in about 2 pixels by one 8-bit
  level, and a 1% fuzz makes them equal.
- **The guard's tolerance is a per-channel delta** of a couple of levels
  with a small pixel budget, not exact hashes. A pinned Mesa is
  unnecessary.

**S6 — met.**
- The bundled faces load by `add_fonts` and render by family name, with
  tabular digits.
- ⚙ Σ ☠ ● ⚑ ① ◀ ▶ resolve by fallback in every family.
- **GPUI resolves no generic family name**: `sans-serif` and `monospace`
  fall to its default. So a theme names its faces. The overlay's are
  Noto Sans and Noto Sans Mono: what iced's cosmic-text draws it in
  live. cosmic-text asks for Open Sans and Noto Sans Mono by default; no
  Open Sans is installed here, so it falls back to Noto Sans. The iced
  guard's PNGs are drawn in Fira Sans, which iced_test loads itself.

**S7 — met.**
- gui-logic's cache readers take `Arc<RenderImage>` handles, built by
  `images::make` (RGBA → BGRA, one `image::Frame`). One tile is one Arc
  ever after, so it uploads once.
- Crests, spec icons and spell icons paint in their true colours.

**S8 — met, beyond the criterion.** One canvas painted, in written
order:
- a gradient-filled area under a Catmull-Rom spline (as cubic Béziers);
- a dashed rule;
- shaped text;
- an image OVER the line.
iced composited all images above all vectors. Here the inspector and
the talent tree can paint in one canvas, and the talent pane needs no
stacked image under it.

**S9 — measured.**
- The binary links libxkbcommon(-x11), libxcb and fontconfig, which the
  dev shell provides, and dlopens wayland-client, vulkan and EGL.
- A debug build takes about 30 s, a first release build about 45 s. The
  release binary is 50 MB, with a 116 MiB closure.
- The lockfile goes from 611 to 1,081 packages.
- CI builds gui-new per package; its dev opt-levels are Kit's set plus
  linux, wgpu, base and swash.

**S10 — met.** The workspace denies hold over `actions!` and data
actions (`#[action(no_json)]`), `cx.listener`, `open_window`'s `Result`
and `Window` / `App` closures, with no allows. The one `expect` waits for
code phase 2 uses.

**S11 — met.**
- Every chord in gui-logic's tables, typed as Linux delivers it, fires
  its action (`keys::tests`). A typed character binds as itself:
  Shift+/ arrives as `/` with key_char `?`, and GPUI matches the
  key_char.
- The meter's actions ride one data action, `Do(Action)`.
- Esc's root walk is built in 3.6.

**S12 — met.**
- One gui-logic `theme::Def` maps onto Kit's `Theme` (every slot a
  control reads: shadcn's `primary` and `ring` take the chrome, `accent`
  the hover wash; buttons read `button_*` slots of their own) and onto
  `Look`.
- Kit's Button, TabBar (underline variant, overridden to `.vtab`'s 37 px
  and 9 px), Input and Tooltip reach the prototype's values; a menu
  panel is `div`s in the popover tokens.
- A live switch from `gold` to `frost` repainted a Kit control and a
  bespoke canvas alike on the next frame.

Any spike that fails its exit criterion stops the plan and comes back to the
user with options before work that depends on it starts.

## 13. Out of scope

A visual redesign of either surface; macOS/Windows support; GPUI Shell
(Kit's JS plugin runtime); WebView; any change to the daemon beyond the
`gui_binary` key; the TUI; wire or store changes.
