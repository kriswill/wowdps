# Shots, the render guard and the chrome budget

The GUI is reviewed without launching it. Its pictures are ignored tests
that open the real views in a `HeadlessAppContext` (`testkit::headless()`:
the Linux platform's real cosmic-text system, Kit's assets and GPUI's
headless renderer), over the daemon's in-process mock (`MockDaemon`), and
capture each frame with `capture_screenshot`. No window reaches the
compositor and no daemon runs, so nothing is launched beside a running
game. They are debug-profile runs.

What they need: a **wgpu adapter** (`current_headless_renderer()` is
`WgpuHeadlessRenderer` on Linux: a GPU, or Mesa's lavapipe). CI and the nix
sandbox have none, so none of these runs there; every ordinary `cargo test`
still runs the interaction tests (Kit's `TestWindowExt`) and the real-text
layout tests, which need no GPU.

Every picture is taken with motion reduced (`cx.set_reduce_motion(true)`):
each delight settles at once, so a shot is the settled pixels, never a
frame mid-glide. Run each test by its name:

| Test | What it renders | Writes |
| --- | --- | --- |
| `window::shots::window_shots` | the window's states at the prototype's three sizes | `<size>-<state>.png` |
| `window::shots::the_chrome_budget_holds_on_the_log` | (measures; no picture) the chrome budget over a real log | — |
| `overlay::panel::tests::overlay_shots` | the overlay's states, with this machine's art caches | `overlay-<state>.png` |
| `overlay::panel::tests::overlay_render_guard` | the same states, without the art, against the committed pictures | (checks) |
| `talents::shots::talent_shots` | the talent viewer's states | `talents-<state>.png` |
| `window::inspector::plot::shots::inspector_plot_shots` | the inspector's graph over the shared samples, the matrix, the chips | `plot-<sample>.png`, `matrix.png`, `chips.png` |

```sh
WOWDPS_SHOTS_DIR=/tmp/s cargo test -p wowdps-gui window_shots -- --ignored --nocapture
WOWDPS_SHOTS_DIR=/tmp/s cargo test -p wowdps-gui overlay_shots -- --ignored
cargo test -p wowdps-gui overlay_render_guard -- --ignored      # by name; WOWDPS_BLESS=1 re-blesses
WOWDPS_SHOTS_DIR=/tmp/s cargo test -p wowdps-gui talent_shots -- --ignored
WOWDPS_SHOTS_DIR=/tmp/s cargo test -p wowdps-gui inspector_plot_shots -- --ignored --nocapture
WOWDPS_SHOTS_LOG=… cargo test -p wowdps-gui the_chrome_budget_holds_on_the_log -- --ignored --nocapture
```

`WOWDPS_SHOTS_DIR` is where the PNGs go; it is optional everywhere. Without
it the shot tests still render every state (a crash or a panic is still a
failure) and save nothing. A run writes only its own file names and deletes
nothing.

`WOWDPS_SHOTS_THEME=<name>` draws every shot set in that built-in theme
(`navy`, the default; `onyx`, `frost`) — the window's, the overlay's, the
talent viewer's and the graph's — so a theme is reviewed state by state
without a window on screen. Give each theme its own `WOWDPS_SHOTS_DIR`: the
file names do not carry it. The render guard never reads it: the blessed
pictures are `navy`'s.

## Window shots — `window::shots::window_shots`

The window's states, each reached from a fresh `Gui` the way a user reaches
it (keys dispatched as `keys::Do` / `keys::Go`, places and cards opened
through the window's own methods), at the prototype's three frames, zoom 1,
in `navy` (or `WOWDPS_SHOTS_THEME`'s), with the bundled faces loaded:

| frame  | logical  |
|--------|----------|
| wide   | 1440×900 |
| tile   | 960×880  |
| narrow | 460×860  |

The reference renders (`~/.local/share/wowdps/design-shots/reference/`,
`wide-*` / `tile-*` / `narrow-*`, captured at 1.25×) are the prototype's;
compare in logical pixels.

States (15, so 45 pictures):

- `damage`, `healing`, `taken`, `deaths`, `interrupts`, `enemies` — the
  fight on that view, the owner's row selected by stepping the keys to it,
  so the inspector beside the meter (above 820 px) is on the owner;
- `options`, `keys`, `picker` — the ⚙ card, the `?` sheet and the
  character menu over the Damage meter;
- `home` — Home;
- `rail-open` — the pull rail's drawer open over the fight (the wide frame
  docks the rail beside the stage in every shot);
- `hide-trash` — that drawer with "Hide trash" pressed;
- `rail-earlier` — a kill from the deepest of the first four earlier
  nights opened on the stage, the drawer open at its row;
- `stored` — the rail's newest stored kill of an earlier night, opened on
  the stage from the history store, the owner's row selected;
- `palette` — the command palette (Ctrl K) over the meter, nothing typed.

A state the input cannot reach (no earlier night in the store, no stored
kill) draws what the window shows instead; nothing panics for it.

"Tonight" is pinned to the night of the log's newest segment, so no heading
depends on the day the shots are taken.

### Inputs and variables

- Over the committed fixture (`crates/core/fixtures/sample.txt`) by
  default.
- `WOWDPS_SHOTS_LOG=<log>` — a combat log the mock reads instead. The
  window is landed on the newest pull of `WOWDPS_SHOTS_FIGHT` (default
  "The Coiled Altar"), with `WOWDPS_SHOTS_OWNER` (default "Tranqlock")
  named in the config's `history_characters` so the "you" marks find them,
  and the display keys the iced shots used: realms hidden, ranks shown,
  comfortable density.
- `WOWDPS_SHOTS_HISTORY=<store v1 dir>` — a history store read through
  READ-ONLY under the log's own stored fights, for Home, the rail's earlier
  nights and a stored pull. Only with `WOWDPS_SHOTS_LOG`.
- `WOWDPS_SHOTS_ONLY=home,rail` — only the states whose name contains one
  of its comma-separated words.

The inputs every compared set uses live outside the repository, under
`~/.local/share/wowdps/design-shots/`, because real logs and stores hold
real player names, and so do their shots:

```sh
S=~/.local/share/wowdps/design-shots
WOWDPS_SHOTS_DIR=$S/after \
WOWDPS_SHOTS_LOG=$S/coiled-altar-night.txt \
WOWDPS_SHOTS_HISTORY=$S/history-v1 \
  cargo test -p wowdps-gui window_shots -- --ignored --nocapture
```

- **The log**, `coiled-altar-night.txt`: the night's Venomous Abyss visit
  from the log's start through the Coiled Altar kill, the trash after it,
  both Ula'tek pulls and the trash after that kill, cut where the raid left
  for Silvermoon. The Coiled Altar is a past pull while the visit's trash
  is live, as the reference frames it.
- **The store**, `history-v1`: a FROZEN copy of the machine's history store
  as of the slice's last line. Never point a run at the live store
  (`~/.local/share/wowdps/history/v1`): the daemon writes it while the user
  plays, and a before/after pair would mix data with design.

Compare two sets only when they were made from the same inputs. The iced
GUI's last sets over these inputs (`before/`, `plot-after/`,
`overlay-iced/`, …) are kept there too; `gui-new-r2/review.html` lays the
GPUI window's and overlay's pictures beside iced's (`docs/plan-gui-new.md`,
phase 5's readiness).

## Chrome budget — `window::shots::the_chrome_budget_holds_on_the_log`

The fight header's acceptance as a number over a real log: at 1440×900 the
featured raid's first meter row (`meter-list`'s top) starts no more than
290 px down, and at least 18 rows (or every row, when fewer) show without a
scroll. It reads the same `WOWDPS_SHOTS_LOG` / `_FIGHT` / `_OWNER` /
`_HISTORY` as the window shots, and without a log it says so and passes.

Every `cargo test` holds the same budget over a synthetic 25-player Heroic
kill (`gui_logic::raid`) in the window's real fonts:
`window::tests::the_chrome_leaves_a_raid_its_rows` (285.5 px, 18 rows, the
total flush with the window's bottom, the owner's chip on the header).

## Overlay shots — `overlay::panel::tests::overlay_shots`

The overlay's states, each a fresh overlay over its own fixture, reached as
a user reaches it, on a dark backdrop (the panel is translucent), with this
machine's art caches when it has them. The panel is 410 × 460 at zoom 1.25
(the defaults); `zoomed` is four notches up with the panel grown with it,
`compare` is the surface grown to `COMPARE_MIN`, and `collapsed` is the tab
alone.

States (20): `meter`, `hover`, `options`, `view-menu`, `enemies`,
`interrupts`, `split` (the footer Σ's second session), `live` (the mock's
live pull), `arena` (`arena.txt`), `drill`, `drill-hover`, `spell-drill`,
`taken-drill` (`taken.txt`), `deaths-drill`, `enemies-drill`, `compare`,
`tree-drill` and `tree-drill-open` (`tree.txt`: R26's groups shut, then
every group open), `zoomed`, `collapsed`. `WOWDPS_SHOTS_ONLY=meter,drill`
takes the named states only (exact names here).

## Overlay render guard — `overlay::panel::tests::overlay_render_guard`

Every state of `overlay_shots`' table against its blessed picture in
`crates/gui/snapshots/overlay/<state>.png`, compared with a TOLERANCE, not a
hash (`guard.rs`): a pixel differs when any channel moves by more than 3,
and a state passes while at most 0.05 % of its pixels differ. The GPU and
Mesa's lavapipe draw a frame within a pixel or two of one 8-bit level, and
the weekly lock update moves Mesa, so exact hashes would churn.

- **No game art.** The guard renders without the art caches
  (`images::without_art`), as a machine without them draws, so the
  committed pictures hold no extracted Blizzard art.
- **The machine's faces.** The overlay draws in the system UI face (Noto
  Sans here), so run the guard where it was blessed.
- **Both directions fail.** A state with no picture, and a picture no state
  draws, are failures.
- `WOWDPS_GUARD_PNG=<dir>` saves every picture it takes, blessed or not.
- `WOWDPS_BLESS=1` writes the current pictures as the new blessed ones
  instead of comparing. Bless only an intended overlay change, and commit
  the new pictures with it.

It keeps its switches per thread (the art, the config path), so it may share
a run with other tests; by name, alone, is still how it is run before a
merge that touches the overlay or gui-logic's overlay models.

## Talent shots — `talents::shots::talent_shots`

The viewer's states as `talents-<state>.png`:

- over gui-logic's synthetic fixture (no art, a sandboxed dataset): the
  logged build, its tooltip, a choice's open picker, the inventory tab, and
  the `frost` theme;
- with `WOWDPS_SHOTS_LOG` as well: the owner's logged build from the newest
  pull of `WOWDPS_SHOTS_FIGHT`, owner `WOWDPS_SHOTS_OWNER`, against this
  machine's real `talents.json` and talent art, at the three frames.

Its twin, `talents::shots::real_dataset_draws_every_spec`, needs no GPU but
this machine's `talents.json` (`tools/gen-talent-trees.sh`): every spec
opens on its empty tree, draws both panes, and a press takes one of its
roots.

## Graph shots — `window::inspector::plot::shots::inspector_plot_shots`

Every state of gui-logic's shared samples (`inspect::geometry::samples`:
alone, lanes, zoomed, a plot hover, a span hover, a pair, same-class twins
dashed, a ghost, the stack, a total) rendered on the inspector's surface,
16 px around, as `plot-<sample>.png`, plus the R21 stack matrix
(`matrix.png`) and the death chips (`chips.png`). With
`WOWDPS_SHOTS_DELIGHT=1`, three more with motion on, for what reduced motion
leaves out: `plot-hover-plot-delight` (the crosshair's glow and its dots),
`plot-pair-delight` and `plot-drag-delight` (a drag in flight, its gold
edges and its window's words).

## Probes

`probes::{s6_fonts, s7_images, s8_canvas, s12_theming}` and
`meter::render_probe` are phase 1's spikes
kept as ignored tests (fonts and the overlay's symbol glyphs, images, the
canvas's paint order, theming). They save what they drew under
`WOWDPS_SHOTS_DIR` and are run only when the question they answered comes
back (a GPUI bump).
