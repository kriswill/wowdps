# Log

## 2026-10-12

- **Update** — macOS support across [core](crates/core.md)
  (install discovery checks the Applications roots),
  [daemon](crates/daemon.md) (game detection runs `pgrep -fil`; the
  default `game_process` is per platform) and [gui](crates/gui.md) (the
  overlay is a floating non-activating panel, content-sized, re-anchored
  on every size change; monitor choice takes a display index). Unchanged:
  the TUI, the tail, the wire, [hypr.rs](crates/gui-logic.md) (absent
  Hyprland degrades to no-ops).

## 2026-10-09

- **Update** — the [OKF Profile](okf-profile.md): the working instructions
  are the `AGENTS.md` files (the repository root's and one per area) since
  `CLAUDE.md` was split; `AGENTS.md` joins the reserved files
  (`okflight.toml [profile]`), so the bundle's own rules file is no concept.

## 2026-09-05

- **Creation** — bundle initialized by `okf init` / `okf setup`; the
  [OKF Profile](okf-profile.md); scaffold passes for crates, rulings and
  tools (`_okflight/scripts/`); the `decisions/`, `fixtures/`,
  `patterns/` and `playbooks/` directories.
