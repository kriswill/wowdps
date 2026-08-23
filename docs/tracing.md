# Instrumentation & debugging

The GUI ships with permanent, env-gated debug rigging. All of it is zero-cost
when the variables are unset; none of it changes behavior except where stated.

## Daemon-mode workflow

Every frontend is a client of the `wowdps` daemon; the daemon owns the log.
To exercise a frontend against the fixture, point the *daemon* at it:

```sh
wowdps --daemon --file crates/core/fixtures/sample.txt &   # or rely on
wowdps-gui --overlay                                       # auto-spawn via
wowdps                                                     # `wowdps --file …`
```

- `wowdps --status` — what the daemon follows, client count, game detection,
  overlay supervisor state (including retained stderr of a failed overlay
  spawn — the first thing to check when auto-launch "did nothing").
- `wowdps --stop` — clean shutdown; without `--linger` it also idle-exits
  ~10 s after the last watching client disconnects.
- `$XDG_STATE_HOME/wowdps/daemon.log` — startup/shutdown/failure trail of
  daemons running with null stdio (self-spawned or systemd).
- Index checkpoints cache under `$XDG_CACHE_HOME/wowdps/index/`; delete the
  directory to force cold full scans.
- A client with `--file`/`--logs` refuses to attach to a daemon following a
  different source (it says so and suggests `--stop`) — remember that when a
  fixture run "won't start" while a real-log daemon lingers.

## Overlay debug environment variables

| Variable | Effect |
|---|---|
| `WOWDPS_OVERLAY_DEBUG=1` | Trace input on stderr: raw mouse events that widgets ignored, grip presses (with cursor position along the drag axis), expand/collapse toggles, and Hyprland workspace show/hide flips (`game workspace visible=…`) — all stamped `[    ms]` since process start. |
| `WOWDPS_OVERLAY_START_EXPANDED=1` | Start with the panel open instead of the tab. For screenshots and layout work on outputs nothing can click. |
| `WOWDPS_OVERLAY_AUTOTOGGLE=1` | Fire one expand/collapse toggle ~2 s after launch. Verifies the resize path end-to-end without any pointer. |
| `WOWDPS_OVERLAY_AUTODRILL=1` | Drill into the top meter row as soon as one exists; `=2` descends once more into the top ability when the by-spell rows arrive. For screenshotting the drilldowns without any pointer (combine with `START_EXPANDED`). |
| `WOWDPS_OVERLAY_AUTOCOMPARE=1` | R12: pick the top two meter rows as soon as both exist, opening the comparison (which also grows the surface to `COMPARE_MIN`); `=half` picks only the top row, for the badged-but-waiting meter state. For screenshotting the comparison without any pointer (combine with `START_EXPANDED`). |
| `WOWDPS_OVERLAY_AUTOVIEW=deaths` | Start on that view (`damage`/`healing`/`interrupts`/`cc`/`dispels`/`deaths`). With `AUTODRILL`, screenshots view-specific drilldowns like the death recap. |

Typical capture:

```sh
WOWDPS_OVERLAY_DEBUG=1 wowdps-gui --overlay 2>overlay-trace.log
```

Reading the trace: `mouse ButtonPressed/Released` lines come from
`iced::event::listen()`, which only yields events **ignored** by widgets — so
a click that shows up raw is a click that *missed* every `mouse_area`, while a
working click shows up as `grip pressed` + `toggle` with no raw lines. That
asymmetry is the primary diagnostic (it is how the scale-factor hit-test bug
below was found).

## Headless verification workflow (Hyprland)

Verify rendering and layer-shell behavior without touching the real desktop
or a running game. Works for both the window and the overlay.

```sh
hyprctl output create headless                  # creates HEADLESS-n
hyprctl eval "hl.monitor({ output='HEADLESS-n', mode='1920x1080', position='4880x0', scale=1 })"

# point the overlay at it with a scratch config (never the real one); the
# fixture data comes from the daemon, which the overlay auto-spawns — or
# start one explicitly first: wowdps --daemon --file crates/core/fixtures/sample.txt &
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
# hidden is w=1 h=1 (layer-shell has no unmap); shown is the real tab/panel size
```

## Known upstream bugs worked around (iced_layershell 0.19)

1. **Bare `SizeChange` is dropped.** Only `AnchorSizeChange` reliably resizes
   the surface — it is also the only variant upstream's own examples use. The
   overlay's `toggle()` therefore always re-asserts anchor + size together.
2. **Custom `scale_factor` breaks pointer hit-testing.** Layout scales, pointer
   coordinates do not, so with any scale ≠ 1.0 clicks land on a grid smaller
   than the visible UI and appear to work "randomly". The overlay renders at
   surface scale 1.0 and multiplies its own font/row/tab sizes by the config's
   `zoom` instead (`bar_row`'s `scale` parameter). The windowed frontend keeps
   real `scale_factor` — winit handles it correctly.

If either is fixed upstream (waycrate/exwlshelleventloop), the workarounds can
be retired; both are commented at their use sites in `crates/gui/src/overlay.rs`.

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
