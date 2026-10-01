---
type: Tool
title: overlay-replay
description: Replays a real combat log, at speed, into an isolated daemon while an overlay follows it on a headless Hyprland output — the stand-in for a raid night without the game, and the measure of an overlay's cost under live load.
resource: tools/overlay-replay.sh
tags: [tool, gui, cost]
status: stable
generated: { by: claude-code/opus-5.5, at: 2026-10-01T12:20:00-07:00 }
sources:
  - id: plan
    resource: ../../plan-gui-new.md
    title: gui-new plan — Phase 5's readiness note, criteria 2 and 3
---

Streams a night's `WoWCombatLog-*.txt` in 250 ms chunks into a daemon of
its own while a GUI's `--overlay` follows it on a headless output, and
samples the overlay's CPU and memory every 30 s, with idle readings before
the first line and after the last, the overlay's stderr and panics, and
screenshots. It is how [wowdps-gui-new](../crates/gui-new.md)'s overlay was
held against the iced one ([wowdps-gui](../crates/gui.md)) for the
cutover's cost criterion, and the proposed stand-in for the raid week the
spec asks for when nobody raids[^plan].

## Seams

- **Isolation is by directory, not by flag.** The daemon is the GUI
  binary's sibling `wowdps`, started with its own `XDG_RUNTIME_DIR`,
  config, data, cache and state, so [the dev daemon](dev-unit.md), its
  socket, its history store and the user's config are untouched. The
  runtime dir is a short `/tmp/wdr.*` path because a unix socket's path
  must fit `SUN_LEN` (a scratch path under `/tmp/claude-*` did not); it
  links the session's Wayland and Hyprland sockets so the overlay still
  reaches the compositor. Only processes the script started are stopped:
  the dev daemon has the very same command line, so a pattern kill would
  take it down.
- **The surface must land on the headless output** (an existing
  `HEADLESS-*`, else one created and removed at the end); one that lands
  elsewhere is killed at once. The script refuses to run while the game
  does, since hot-plugging an output glitched it.
- **What it found.** Its first run showed gui-new's overlay at 4–9.5 % of
  a core while live against iced's 0.8–1.0 %: every meter bar eased 280 ms
  toward a value that changes ten times a second, so the overlay redrew at
  display rate for as long as a pull went (fixed in `b162d1e`, 1.24 %
  against 0.89 % after).

[^plan]: The plan's Phase 5 readiness note records the runs and the numbers.

## Source

- Script: [`tools/overlay-replay.sh`](../../../tools/overlay-replay.sh)
