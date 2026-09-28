# Design shots and the overlay guard

Two ignored tests render the GUI headless (iced_test's tiny-skia
Simulator: no window, no GPU, no daemon), so a design change can be looked
at, and the overlay proven untouched, without launching anything. Both are
debug-profile runs; neither needs `--release`.

## Design shots — `window::shots::design_shots`

Renders the window's screens to PNG through the code the running window
draws with (`view::view`, `window::settings()`, the window's own theme,
painted through its `style` the way the app paints its background — but
opaque, where the live window is translucent at `window_alpha`), each
reached from a fresh window the way a user reaches it, at the prototype's
three sizes:

| size   | logical  | shot (scale 2) | reference (scale 1.25) |
|--------|----------|----------------|------------------------|
| wide   | 1440×900 | 2880×1800      | 1800×1125              |
| tile   | 960×880  | 1920×1760      | 1200×1100              |
| narrow | 460×860  | 920×1720       | 681×1075 (a ~460 px frame in a wider capture) |

To measure one against the other, bring both to CSS/logical px: harness
px ÷ 2, reference px ÷ 1.25. Scale outside the repository if a picture
must be resized (no image crate is added for it).

States: `list damage healing taken deaths enemies drill taken-drill
deaths-drill enemies-drill compare home history`, saved as
`<size>-<state>.png` beside a `manifest.txt`. A state the log cannot
produce (no damage taken, no second player) is skipped with a line in the
manifest, never a panic.

### The inputs every run uses

Every set that will be compared with another is made from the same three
inputs, all outside the repository (real logs and stores hold real player
names — keep them, and their shots, out of it):

```sh
S=~/.local/share/wowdps/design-shots
WOWDPS_SHOTS_DIR=$S/after \
WOWDPS_SHOTS_LOG=$S/coiled-altar-night.txt \
WOWDPS_SHOTS_HISTORY=$S/history-v1 \
WOWDPS_SHOTS_FIGHT='The Coiled Altar' \
WOWDPS_SHOTS_OWNER=Tranqlock \
  cargo test -p wowdps-gui design_shots -- --ignored --nocapture
```

A run over the night takes about two minutes (debug build).

**The log**, `coiled-altar-night.txt`: the night's Venomous Abyss visit
from the log's start through the Coiled Altar kill, the trash after it,
both Ula'tek pulls and the trash after the kill, cut where the raid left
for Silvermoon — the reference's "Tonight". The Coiled Altar is then a
past pull while the visit's trash is live, as the reference frames it:
the manifest says `following live: no`. It was cut once:

```sh
LOG=".../_retail_/Logs/WoWCombatLog-092726_185854.txt"   # the night's log
kill=$(grep -n 'ENCOUNTER_END,3492,' "$LOG" | awk -F, '$(NF-1)==1' | tail -1 | cut -d: -f1)
out=$(awk -v k="$kill" 'NR>k && /ZONE_CHANGE/ {print NR; exit}' "$LOG")
head -n $((out - 1)) "$LOG" > ~/.local/share/wowdps/design-shots/coiled-altar-night.txt
```

(`coiled-altar.txt`, the first slice, ends 6 ms after the kill: over it
the fight is the log's newest segment, opening it pins Live, and every
fight shot wears live chrome the references do not.)

**The store**, `history-v1`: a FROZEN copy of this machine's history store,
as of the slice's last line. The live daemon writes
`~/.local/share/wowdps/history/v1` while the user plays, so a run over it
shows whatever was pulled since — other pulls, counts, week totals and
chips — and a before/after pair of Home or History would mix data with
design. Never point a run at the live store. The copy was made once,
dropping every fight that starts after the slice (a card's file name ends
in its start on the log's clock):

```sh
S=~/.local/share/wowdps/design-shots
cp -a ~/.local/share/wowdps/history/v1 "$S/history-v1"
cut=$(( $(date -u -d '2026-09-27 19:36:36' +%s) * 1000 ))   # the slice's last line, log clock
for f in "$S"/history-v1/{fights,rows,details}/*.json; do
  t=${f##*-}; t=${t%%[!0-9]*}
  [ "$t" -gt "$cut" ] && rm "$f"
done
```

**The settings**: the fight pinned by name and the owner by name, so a
change to the log's contents cannot move either.

`before/` is the baseline: these inputs, drawn by the tree the redesign
started from (0a7e7cf plus this harness). Compare an `after/` with it only
when both manifests list the same `log`, `history` (fingerprint included),
`fight`, `owner`, `display` and `cache` lines.

### The variables

- `WOWDPS_SHOTS_DIR` (required; without it the test returns at once):
  where the PNGs and `manifest.txt` go. A run first deletes the PNGs the
  previous `manifest.txt` lists and every name it may write, and a
  panicked run's `.render/`; nothing else in the directory is touched. It
  refuses a directory that holds pictures under its names but no
  `manifest.txt` of its own (first line `rev:`): the prototype's
  `reference/` shares several names (`wide-damage`, `wide-home`, …) and
  would otherwise be overwritten. Pair pictures by the table below, never
  by name.
- `WOWDPS_SHOTS_LOG` (default `crates/core/fixtures/sample.txt`): the combat
  log the mock daemon parses. Its segments are the fight list — a
  whole-night log is what fills a list (and the redesign's Pulls rail)
  the way the reference's does.
- `WOWDPS_SHOTS_HISTORY` (optional): a history store's `v1` directory
  that Home and History answer from, with the log's own cards on top.
  Without it they see only the log's cards, which is no way to judge
  their density, grouping or charts. The store is read through, never
  written: its files are read into the mock's in-memory store
  (`MemBackend::over_dir`), whose retention and migrations stay in
  memory; no `DirBackend` is ever opened on it.
- `WOWDPS_SHOTS_OWNER` (default `Tranqlock`): the owner, "Name" or
  "Name-Realm". The window gets their row label as `history_characters` and
  their guid as `character`, as the user's config names them; the mock's
  history store stamps them as the owner of the log's cards. Over the
  committed fixture use `Thraxx`.
- `WOWDPS_SHOTS_FIGHT` (default: the log's first boss kill): the featured
  fight by name — its first kill, else its first pull.

The window's config is the shipping one at zoom 1 with the display keys
the prototype's "Look" row assumes and the user's config sets —
`hide_realms = true`, `show_ranks = true`, `density = "comfortable"` — so
names are drawn without their realms in every pane. Home is offered at
launch (a live pull replaces it); a window that never saw it there visits
Home by its tab and comes back, so what Home tells a window — the owner,
for the tab strip's character picker — is there on every screen, as for
any window that has been to Home once.

`manifest.txt` records where a set came from: `rev` (the commit, read from
`.git` without running git), `src` (a fingerprint of the crates' sources —
what tells two dirty trees at one commit apart), the icon caches' size and
mtime, the log, the history store with a fingerprint of its `fights/`
(names and sizes) and its card count, the newest card (what Home and
History are "as of"), the fight, **`following live`**, the owner, the
`display` keys, the timings, and every file written — with its logical
size and its pixel size — or state skipped.

`following live: yes` means the featured fight is the log's newest segment,
so opening it pinned Live — the window's own rule — and every fight shot
wears live chrome (the live tab lit, the list's newest row live). The
references show their fight as a past pull; over the night slice this
reads `no`.

### Reference → harness

The prototype's renders (`design-shots/reference/`) and the state each is
compared with:

| reference              | harness                    |
|------------------------|----------------------------|
| `wide-home`            | `wide-home`                |
| `wide-damage`          | `wide-drill` (owner selected, inspector open) and `wide-damage` |
| `tile-damage`          | `tile-drill`, `tile-damage`|
| `narrow-meter`         | `narrow-damage`            |
| `narrow-inspector`     | `narrow-drill`             |
| `wide-healing`         | `wide-healing`             |
| `wide-taken-mehna`     | `wide-taken-drill` (the top of Taken; Mehna in the reference) |
| `wide-deaths`          | `wide-deaths-drill` (the owner's death recap) |
| `wide-enemies`         | `wide-enemies-drill` (the top enemy's attackers) |
| `wide-compare`         | `wide-compare`             |
| `tile-rail-open`       | none yet — add `rail-open` when the Pulls rail exists |
| `wide-palette`         | none yet — add `palette` when the command palette exists |

## Overlay guard — `overlay::guard::overlay_snapshot_guard`

Renders the overlay over the committed fixtures — the kill's meter, its
top row's drill, R17's Taken drill, the comparison, the Deaths recap, the
ability drill, the Enemy Taken meter and its attackers, the Interrupts
count view, a hovered meter row and drill row, an arena match's team
divider, a live pull followed, the footer's ⚙ options card, its view menu
with a row hovered, the Σ split rows, a wheel-zoomed panel, and the
collapsed tab — at the surface size the overlay would ask for, with the
overlay's own settings (`overlay::settings`), and checks each against a
SHA-256 in `crates/gui/snapshots/overlay/`. The window redesign forks
window-only paths instead of changing renderers the overlay shares; this
test is what says it worked.

```sh
cargo test -p wowdps-gui overlay_snapshot_guard -- --ignored
```

It proves the pixels at iced_test's scale of 2 only. The running overlay
draws at 1 and applies its own zoom, so a change that only shifts 1x
rounding or pixel snapping can pass; the `zoomed` state, at an off-grid
zoom, is what exercises zoom-dependent sizes.

Run it ALONE, with exactly that filter — never under
`cargo test -- --include-ignored`. iced's font system is process-global:
a window test (or `design_shots`) that loads the window's own fonts
earlier in the same process changes the fallback cosmic-text picks for
glyphs the system fonts lack (the overlay draws ⚙ Σ ☠ ●). The guard checks
for any face in memory that iced did not load itself before it touches a
hash — so a bless in such a process fails before it deletes or writes
anything — and again after, naming the fonts.

It fails, too, when a state has no `<state>-tiny-skia.sha256` (iced_test
would otherwise write the missing file and call it a match) and when the
run added or removed a file in the directory. `WOWDPS_GUARD_PNG=<dir>`
also saves each state's picture there (`<state>-tiny-skia.png`), to look
at a state that moved or a new one before it is blessed.

Ignored by default because the hashes are of pixels, and pixels depend on
the machine: its fonts, and the per-machine icon caches under
`~/.local/share/wowdps/` (`tools/gen-icons.sh`, `tools/gen-spell-icons.sh`).
Run it on the machine that blessed it. When an overlay change is intended —
or the caches were regenerated — re-bless and commit the new hashes (the
directory is rewritten to exactly the guard's states):

```sh
WOWDPS_BLESS=1 cargo test -p wowdps-gui overlay_snapshot_guard -- --ignored
```
