---
type: Tool
title: dev-unit
description: Installs and drives the wowdps-dev systemd user unit — the dev machine's live daemon, run from this checkout's own build and restarted only when the build it runs (or the overlay GUI it spawns) changes.
resource: tools/dev-unit.sh
tags: [tool, dev]
status: stable
generated: { by: claude-code/opus-5.5, at: 2026-10-01T12:20:00-07:00 }
---

`tools/dev-unit.sh install | profile [debug|release] | status | uninstall`
manages the unit that runs `target/<profile>/wowdps daemon --linger` from
this checkout ([wowdps-daemon](../crates/daemon.md)). A daemon spawned by
a client is `linger: no` and exits about ten seconds after its last client
leaves (an MCP session ending once took the overlay down mid-game), so the
dev machine's live daemon is always this unit, never a hand-run one.

## Seams

- **Stamps, not mtimes.** At start the unit stamps the binaries it runs:
  the daemon and the overlay GUI config `gui_binary` names (default
  `wowdps-gui`). `wowdps-dev.path` fires on any write to
  `target/{debug,release}/wowdps{,-gui,-gui-new}`, and the reload oneshot
  restarts the service only when the ACTIVE profile's stamped binaries
  changed. A debug build never bounces a release daemon, and a build of
  the GUI the daemon does not spawn bounces nothing. A
  `systemctl --user stop wowdps-dev` stays stopped through rebuilds.
- **Winning the socket.** The unit asks any running daemon to stop
  before it starts, so a self-spawned one cannot hold the socket.
- **Leaving it alone in experiments.** Anything that needs a second
  daemon ([overlay-replay](overlay-replay.md)) isolates it by directory and
  stops only the processes it started: the unit's daemon has the same
  command line.

## Source

- Script: [`tools/dev-unit.sh`](../../../tools/dev-unit.sh), templates in
  [`tools/dev-unit/`](../../../tools/dev-unit/).
