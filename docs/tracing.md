# Instrumentation & debugging

The GUI ships with permanent, env-gated debug rigging. All of it is zero-cost
when the variables are unset; none of it changes behavior except where stated.

## Daemon-mode workflow

Every frontend is a client of the `wowdps` daemon; the daemon owns the log.
To exercise a frontend against the fixture, point the *daemon* at it:

```sh
wowdps daemon --file crates/core/fixtures/sample.txt &   # or rely on
wowdps-gui --overlay                                       # auto-spawn via
wowdps                                                     # `wowdps --file …`
```

- `wowdps status` — what the daemon follows, client count, game detection,
  overlay supervisor state (including retained stderr of a failed overlay
  spawn — the first thing to check when auto-launch "did nothing").
- `wowdps stop` — clean shutdown; without `--linger` it also idle-exits
  ~10 s after the last watching client disconnects.
- `$XDG_STATE_HOME/wowdps/daemon.log` — startup/shutdown/failure trail of
  daemons running with null stdio (self-spawned or systemd).
- Index checkpoints cache under `$XDG_CACHE_HOME/wowdps/index/`; delete the
  directory to force cold full scans.
- The history store lives under `$XDG_DATA_HOME/wowdps/history/v1/` (one JSON
  document per fight in `fights/`, `rows/`, `details/`, `loadouts/`).
  `wowdps status` prints its line — fight count, imports in flight, dropped
  writes, and the latest write/read error. The files are the truth: delete any
  of them and the daemon re-imports what the logs still hold. `history_enabled
  = false` in the config turns it off.
- A client with `--file`/`--logs` refuses to attach to a daemon following a
  different source (it says so and suggests `wowdps stop`) — remember that when a
  fixture run "won't start" while a real-log daemon lingers.

## Overlay debug environment variables

| Variable | Effect |
|---|---|
| `WOWDPS_OVERLAY_DEBUG=1` | Trace input on stderr: grip presses and releases (moved or not, and the offset), expand/collapse toggles, the daemon's `SetVisible`, and Hyprland workspace show/hide flips (`game workspace visible=…`) — all stamped `[    ms]` since process start. |
| `WOWDPS_OVERLAY_OUTPUT=<name>` | Open on that output, ahead of config `monitor` and the game's monitor. Use it to put the overlay on a headless output for a screenshot. |
| `WOWDPS_OVERLAY_START_EXPANDED=1` | Start with the panel open instead of the tab. For screenshots and layout work on outputs nothing can click. |
| `WOWDPS_OVERLAY_AUTOTOGGLE=1` | Fire one expand/collapse toggle ~2 s after launch. Verifies the resize path end-to-end without any pointer. |
| `WOWDPS_OVERLAY_AUTODRILL=1` | Drill into the top meter row as soon as one exists; `=2` descends once more into the top ability when the by-spell rows arrive. For screenshotting the drilldowns without any pointer (combine with `START_EXPANDED`). |
| `WOWDPS_OVERLAY_AUTOCOMPARE=1` | R12: pick the top two meter rows as soon as both exist, opening the comparison (which also grows the surface to `COMPARE_MIN`); `=half` picks only the top row, for the badged-but-waiting meter state. For screenshotting the comparison without any pointer (combine with `START_EXPANDED`). |
| `WOWDPS_OVERLAY_AUTOVIEW=deaths` | Start on that view (`damage`/`healing`/`interrupts`/`cc`/`dispels`/`deaths`/`taken`/`enemy`). With `AUTODRILL`, screenshots view-specific drilldowns like the death recap. |
| `WOWDPS_OVERLAY_AUTOSEG=17` | Park the frame on that combined-list position (the TUI's segment list, 0-based) once the list arrives, instead of pinning Live. The other AUTO* aids hold their fire until it lands, so they act on the parked segment. For screenshotting any historical pull without a pointer. |

Typical capture:

```sh
WOWDPS_OVERLAY_DEBUG=1 wowdps-gui --overlay 2>overlay-trace.log
```

The aids live in `crates/gui/src/overlay/panel/autos.rs`. GPUI has no
stream of "events no element took", so the trace has no raw-event lines (the
iced overlay's did): a click that never shows up as `grip pressed` or a
toggle missed the content, or the input region.

To see what the surface asks of the compositor, run it with
`WAYLAND_DEBUG=1`. That trace found the zero-length layer surface that
GPUI turns into a viewport protocol error ("Size was <= 0").

## Headless verification workflow (Hyprland)

Verify rendering and layer-shell behavior without touching the real desktop
or a running game. Works for both the window and the overlay.

```sh
hyprctl output create headless                  # creates HEADLESS-n
hyprctl eval "hl.monitor({ output='HEADLESS-n', mode='1920x1080', position='4880x0', scale=1 })"

# point the overlay at it with a scratch config (never the real one) or
# WOWDPS_OVERLAY_OUTPUT=HEADLESS-n; the fixture data comes from the daemon,
# which the overlay auto-spawns — or start one explicitly first:
# wowdps daemon --file crates/core/fixtures/sample.txt &
mkdir -p /tmp/xdg/wowdps
printf 'edge = "right"\nmonitor = "HEADLESS-n"\n' > /tmp/xdg/wowdps/config.toml
XDG_CONFIG_HOME=/tmp/xdg wowdps-gui --overlay

# inspect and screenshot
hyprctl layers -j | jq '.["HEADLESS-n"].levels["3"]'   # overlay layer: geometry, namespace
grim -g "<x>,<y> <w>x<h>" shot.png

hyprctl output remove HEADLESS-n                # cleanup
```

Notes:
- Fresh headless outputs default to scale 2; set scale 1 or coordinates in
  `hyprctl layers` (logical) will not match `grim -g` pixels.
- For the *windowed* app on a hidden output, add a runtime rule so it does not
  tile over your desktop:
  `hyprctl eval "_G.r = hl.window_rule({ name='shot', match={ title='^wowdps' }, workspace='<ws> silent', float=true, size='460 640', move='100 100' })"`
  (`move` is monitor-relative; disable later with `_G.r:set_enabled(false)`).
- Keys can be sent to an unfocused *window* (not a layer surface) without
  stealing focus:
  `hyprctl eval "hl.dispatch(hl.dsp.send_shortcut({ mods='', key='Return', window='address:0x…' }))"`.

## Exercising workspace tracking without the game (Hyprland)

With `follow_game` on, the overlay hides whenever the `game_match` window's
workspace is not displayed on any monitor. Both transitions can be driven
without WoW: point `game_match` at a fake title in a scratch config, then
spawn and close such a window on a workspace nothing displays:

```sh
hyprctl eval "hl.exec_cmd('ghostty --title=wowdps-fake-game', { workspace = '99 silent' })"
# → overlay hides (debug trace: `game workspace visible=false`)
pkill -f wowdps-fake-game    # → overlay restores (careful: -f also matches a shell quoting it)
hyprctl layers -j | jq '[.. | objects | select(.namespace? == "wowdps")] | map({pid, w, h})'
# the overlay's surface spans its whole edge (the edge strip): the tab or
# panel sits at the offset inside it and the input region is exactly the
# content; hidden is a 1 px strip with an empty input region
```

## Layer-surface constraints (GPUI)

The overlay is a GPUI layer surface (`crates/gui/src/overlay.rs`,
`overlay/strip.rs`, `overlay/panel/surface.rs`), and GPUI's layer-shell
support shapes it:

1. **No runtime margin, anchor or layer setter.** A surface cannot move along
   its edge, so it spans the edge's whole length and the content moves inside
   it; a grip drag moves the content, and the input region follows the
   content, so the rest of the strip is click-through. A tab dropped near
   another edge (Hyprland) recreates the surface there, the new one opened
   before the old one goes.
2. **Never ask for a zero length.** Layer-shell reads 0 as "stretch", but
   GPUI hands every size to the surface's viewport, and a zero there is the
   protocol error "Size was <= 0", which kills the connection. The surface
   opens with the edge's real length (Hyprland's `monitor_named`, which
   accounts for rotation and scale, else GPUI's display bounds).
3. **Kit's Root is for the window.** On a client-decorated surface (a layer
   surface always is) it adds a 20 px shadow inset and paints the theme's
   ground, so the overlay opens its view through GPUI's `cx.open_window`.
4. **The overlay zooms by hand.** GPUI has no app scale factor; the overlay
   multiplies its own sizes by the config's `zoom` (`overlay/ov.rs`), as the
   window does through its `W` (`w.z(…)`).

A runtime `set_margin` for layer surfaces would retire the strip; it is to be
contributed upstream, never patched in (`docs/OKF/decisions/no-gpui-forks.md`).

## Real-log gates

`crates/core` has an ignored perf test that runs the scanner + a full parse
against any real combat log:

```sh
WOWDPS_REAL_LOG=/path/to/WoWCombatLog-*.txt cargo test --release -p wowdps-core -- --ignored real_log --nocapture
```

It asserts a sub-second scan and sub-second biggest-encounter load, and prints
segment counts — a quick health check when Blizzard changes the log format.
Note it expects at least one *closed* segment, so a log captured mid-first-pull
fails its non-empty assertion by design.

## Combat-log flush latency (context for "is it frozen?")

The game buffers combat-log writes and flushes in large bursts — measured on
2026-08-01: ~2 m 48 s between flushes, 6.3 MB in one burst, during active
combat. This is Blizzard's post-2023 countermeasure against real-time helper
overlays; nothing external sees events sooner. Both frontends surface it as
"no events for Ns" instead of pretending to be real-time. When diagnosing
"the meter stopped", check the file first:

```sh
stat -c '%y' "$LOG"; tail -1 "$LOG" | cut -d' ' -f1-2   # mtime vs last event ts
```

If mtime is old but the game is up, the buffer simply has not flushed —
or combat logging is off (`/combatlog` resets every session).

## The dev daemon unit (`tools/dev-unit.sh`)

On a dev machine the daemon should be the build you just made, and it should
stay up between game launches. `tools/dev-unit.sh install` writes three
systemd user units from `tools/dev-unit/` with the checkout path baked in:

- `wowdps-dev.service` — `target/<profile>/wowdps daemon --linger`, where
  the profile (`debug` | `release`) lives in `~/.config/wowdps/dev-unit.env`
  and is switched with `tools/dev-unit.sh profile debug` (restarts the
  daemon if it is running). On start it stops any daemon already answering
  the socket, so a self-spawned one is replaced rather than fought over the
  lockfile. `ExecStop` is `wowdps stop` — the daemon takes no signals.
- `wowdps-dev.path` — watches `target/{debug,release}/wowdps{,-gui}` and
  fires `wowdps-dev-reload.service`, which waits for cargo's writes to settle
  and restarts the daemon only when the ACTIVE profile's stamp changed: the
  daemon and the GUI it spawns as the overlay, config `gui_binary` (default
  `wowdps-gui`), each stamped at start by path, inode, size and mtime. A
  build of a GUI the daemon does not spawn fires a reload that restarts
  nothing, and a stopped service stays stopped.
- `tools/dev-unit.sh status` shows the profile, the overlay's GUI, both
  units and `wowdps status`; `uninstall` removes everything.

The overlay follows: a restart terminates the supervised overlay and the
new daemon respawns it while the game is running. A debug profile needs a
debug build of the configured GUI beside the daemon, or the daemon spawns
the name from `$PATH`, and failing that the overlay cannot spawn (the
failure surfaces in `wowdps status`). Standing another build in for the
overlay is a config edit and a restart: `gui_binary = "<name or path>"`,
then `systemctl --user restart wowdps-dev`. A `gui_binary` holding a `/` is
a path; one outside this checkout's `target/` is stamped but not watched, so
restart the unit after rebuilding it. An orphaned `wowdps-gui --overlay` is
killed on stop. The packaged twin for non-dev machines is the flake's
home-manager/NixOS module.
