# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What this is

`wowdps` is a World of Warcraft combat-log damage meter: a Rust workspace with a shared core (`crates/core`), a ratatui TUI (`crates/tui`, binary `wowdps`), and an iced GUI (`crates/gui`, binary `wowdps-gui`) that runs either as a regular window or a wlr-layer-shell overlay (`--overlay`).

## Commands

```sh
cargo test                        # whole workspace (fixture parity tests included)
cargo test -p wowdps-core         # one crate
cargo test -p wowdps-core meter:: # tests matching a substring
cargo build --release
cargo clippy && cargo fmt

# Run against the committed fixture log
cargo run --bin wowdps -- --file crates/core/fixtures/sample.txt
cargo run --bin wowdps-gui -- --file crates/core/fixtures/sample.txt
# No args = follow the newest WoWCombatLog*.txt in the Proton path hardcoded
# in crates/core/src/cli.rs (DEFAULT_LOGS_DIR).

# Perf gate against a real log (asserts sub-second scan + segment load)
WOWDPS_REAL_LOG=/path/to/WoWCombatLog-*.txt cargo test --release -p wowdps-core -- --ignored real_log --nocapture

# Parser-independent fixture check (gawk recomputes golden totals)
crates/core/fixtures/verify.sh                # sample.txt vs sample.expected.tsv
crates/core/fixtures/verify.sh crates/core/fixtures/corrupt.txt   # negative control: must FAIL
```

Cargo works system-wide, but building/running the **GUI** needs the flake dev shell (`nix develop`) for pkg-config/libxkbcommon at build time and the `LD_LIBRARY_PATH` (wayland, vulkan-loader, libGL) at runtime — this is NixOS.

Dependency policy (from CONTRACT.md): stdlib unless justified. Approved: ratatui + crossterm (tui); iced + iced_layershell + serde/toml (gui). No chrono (timestamps are hand-parsed), no tokio in core (threads + channels), no serde in core.

## Architecture

**CONTRACT.md is the binding interface spec.** It fixes the public signatures of `parser`, `meter`, and `index`, plus semantic rulings R1–R7 (what counts as damage/healing, absorb attribution, segment boundaries and duration semantics, pet attribution, mid-log `COMBAT_LOG_VERSION` reset). Fixture expected values are computed from those rulings, so changing meter semantics means changing CONTRACT.md, the fixtures, and the code together.

**`crates/core`** holds everything that is not a screen:

- `parser.rs` — one combat-log line → `LogLine`/`Event`. Unknown events become `Event::Other`, never an error.
- `meter.rs` — `Meter::feed` aggregates lines into `Segment`s (Encounter or Trash) and produces `Row`s per `View` (Damage/Healing/Interrupts/CrowdControl/Dispels/Deaths) plus per-player breakdowns.
- `index.rs` — fast structural scan of a whole log (segment boundaries + byte ranges, no per-event parsing) so a 300 MB+ log shows its segment list in <1 s. A segment is fully parsed only when opened (`load_segment` + fresh `Meter`). The scanner records `seeds` (byte ranges of `SPELL_SUMMON`/`COMBATANT_INFO`/`COMBAT_LOG_VERSION` lines) that `load_segment` prepends, so lazy parsing exactly matches full replay — this parity (same segments, same rows, same classes) is a fixture-gated invariant. The scanner mirrors `Meter::feed`'s segmentation rules; keep them in lockstep.
- `tail.rs` — reader-thread `Source` following a file or the newest log in a directory (poll ~200 ms, rotation-aware). On open it emits `Switched` → `Index` (one scan) → `Lines` from the index's `live_offset`; history is never replayed line by line.
- `app.rs` — the UI-agnostic application state machine both frontends drive: two screens (`List` segment browser, `Meter`), selection, view keys, drilldowns, live-fight detection at startup, and a `wants_load` request that the frontend's main loop services (app.rs itself does no I/O).
- `model.rs` — the single binding point: frontends import domain types from here, never from `meter`/`parser` directly.
- `testkit.rs` (feature `testkit`, used by tui dev-deps) — builds `App`s by replaying `fixtures/sample.txt` through the real parser/meter.

**Fixtures** (`crates/core/fixtures/`): `sample.txt` is a synthetic advanced-format log (2 encounters + trash, 3 players + 1 pet, every modeled event type) with hand-computed golden totals in `sample.expected.md`/`.tsv`, verified independently of the parser by `check.awk`; `corrupt.txt` is the negative control. `FORMAT-NOTES.md` documents the log format itself.

**Frontends** are thin: the TUI (`app`-driven `ui.rs` with TestBackend tests) and the GUI (`window.rs` / `overlay.rs` sharing `view.rs`; config persisted at `~/.config/wowdps/config.toml`) both consume `wowdps_core::app::App` and the shared CLI (`core/src/cli.rs`). GUI keybinds mirror the TUI's.

## Debugging

`docs/tracing.md` covers: overlay debug env vars (`WOWDPS_OVERLAY_DEBUG=1` input tracing, `WOWDPS_OVERLAY_START_EXPANDED`, `WOWDPS_OVERLAY_AUTOTOGGLE`), a headless Hyprland workflow for screenshotting/verifying the overlay without a real game, and two iced_layershell 0.19 upstream bugs that are deliberately worked around in `overlay.rs` (bare `SizeChange` dropped; custom `scale_factor` breaks hit-testing — the overlay renders at scale 1.0 and applies its own zoom).

Also note: the game flushes combat-log writes in multi-minute bursts (anti-overlay countermeasure), so a "frozen" meter is usually just an unflushed buffer — check the log file's mtime before debugging the tail path.
