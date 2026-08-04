# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What this is

`wowdps` is a World of Warcraft combat-log damage meter with a client/server split: a headless **daemon** owns the whole pipeline (tail → index → parse → meter → snapshots) and every frontend is a pure rendering client speaking a hand-rolled binary protocol over a unix socket. Crates: `crates/model` (zero-dep domain types), `crates/core` (the engine: parser/meter/index/tail), `crates/proto` (wire codec + `DaemonClient` + `ClientState`), `crates/daemon` (hub, loader pool, game watcher, overlay supervisor, index cache), `crates/tui` (binary `wowdps` = daemon + launcher + TUI client), `crates/gui` (binary `wowdps-gui` = window or wlr-layer-shell overlay via `--overlay`; depends on model+proto only, so it *cannot* parse a log).

## Commands

```sh
cargo test                        # whole workspace (fixture parity + IPC suites included)
cargo test -p wowdps-core         # one crate
cargo test -p wowdps-core meter:: # tests matching a substring
cargo build --release
cargo clippy && cargo fmt

# Run against the committed fixture log (the client forwards the source to
# the daemon it spawns; the daemon idle-exits ~10s after the last client)
cargo run --bin wowdps -- --file crates/core/fixtures/sample.txt
cargo run --bin wowdps -- --status   # daemon state incl. overlay spawn failures
cargo run --bin wowdps -- --stop
# No args = daemon follows config `logs_dir`, defaulting to the Proton path
# in crates/core/src/cli.rs (DEFAULT_LOGS_DIR). `wowdps --daemon [--linger]`
# runs the daemon in the foreground (what systemd and self-spawn use).
# `wowdps-gui` takes no source flags — the daemon owns the log.

# Perf gates against a real log
WOWDPS_REAL_LOG=/path/to/WoWCombatLog-*.txt cargo test --release -p wowdps-core -- --ignored real_log --nocapture
WOWDPS_REAL_LOG=/path/to/WoWCombatLog-*.txt cargo test --release -p wowdps-daemon -- --ignored real_log --nocapture

# Parser-independent fixture check (gawk recomputes golden totals)
crates/core/fixtures/verify.sh                # sample.txt vs sample.expected.tsv
crates/core/fixtures/verify.sh crates/core/fixtures/corrupt.txt   # negative control: must FAIL
```

Cargo works system-wide, but building/running the **GUI** needs the flake dev shell (`nix develop`) for pkg-config/libxkbcommon at build time and the `LD_LIBRARY_PATH` (wayland, vulkan-loader, libGL) at runtime — this is NixOS. `devenv.nix` is a twin of that shell (auto-entered via devenv's cd hook after `devenv allow`); keep both in sync, and keep `devenv.yaml`'s nixpkgs pin matching `flake.lock`. The flake also packages the daemon/TUI binary (`nix build .#wowdps`, pure Rust) and exports `homeManagerModules.default` with a systemd user unit (`wowdps --daemon --linger`).

Dependency policy (from CONTRACT.md): model zero-dep; core, proto, daemon stdlib only. Approved: ratatui + crossterm (tui); iced + iced_layershell + serde/toml (gui). No chrono (timestamps are hand-parsed), no tokio (threads + channels), no serde outside the gui.

## Architecture

**CONTRACT.md is the binding interface spec.** It fixes the public signatures of `parser`, `meter`, and `index`, semantic rulings R1–R7 (what counts as damage/healing, absorb attribution, segment boundaries and duration semantics, pet attribution, mid-log `COMBAT_LOG_VERSION` reset), and the wire protocol surface (`PROTO_VERSION`, frame layout, message tags, ordering/id guarantees). Fixture expected values are computed from the rulings; the golden-byte tests in `crates/proto/tests/codec.rs` pin the encodings — changing either means changing CONTRACT.md, the fixtures/golden bytes, and the code together, and a wire-shape change means bumping `PROTO_VERSION` (which renames the socket).

**`crates/core`** — the engine (only the daemon runs it):

- `parser.rs` — one combat-log line → `LogLine`/`Event`. Unknown events become `Event::Other`, never an error.
- `meter.rs` — `Meter::feed` aggregates lines into `Segment`s (Encounter or Trash) and produces `Row`s per `View` plus per-player breakdowns.
- `index.rs` — fast structural scan (segment boundaries + byte ranges, no per-event parsing) so a 300 MB+ log lists its segments in <1 s; a segment is fully parsed only when opened (`load_segment` + fresh `Meter`), seeded with earlier `SPELL_SUMMON`/`COMBATANT_INFO`/`COMBAT_LOG_VERSION` lines so lazy parsing exactly matches full replay (fixture-gated). The scanner mirrors `Meter::feed`'s segmentation; keep them in lockstep. `Index::checkpoint`/`scan_from` make scans resumable — the daemon's index cache persists checkpoints so restarts rescan only the tail.
- `tail.rs` — `Tailer` following a file or the newest log in a directory (poll ~200 ms, rotation-aware). On open: `Switched` → `Index` (one scan, injectable via `with_scan` for the cache) → `Lines` from `live_offset`; `CaughtUp` separates backlog replay from fresh combat.
- `class_spells.rs` — GENERATED spell-id → class/spec table (regenerate with `python3 tools/gen-class-spells.py`, sources wago.tools DB2 exports). Backs ruling R8: out of instances COMBATANT_INFO never fires, so the meter infers a player's class/spec from their casts — segment-local only (never carried forward), COMBATANT_INFO overwrites it, and it must never open a segment, or lazy/full parity breaks.

**`crates/proto`** — `wire.rs` (LE primitives + `u32 len | u8 tag | body` frames, decode never panics), `msg.rs` (`ClientMsg`/`DaemonMsg`; a `Watch` declares a `Cursor` — the list, or a segment+view with optional drill — and the daemon pushes snapshots for exactly that), `client.rs` (`socket_path()` embeds `PROTO_VERSION`; `ensure_daemon` spawns on demand; `DaemonClient`'s reader thread coalesces stale snapshots), `state.rs` (`ClientState`: the old `App` accessor surface for renderers; `apply`/`on_msg` return requests to send; held-key `j`/`k` clamps against the cached snapshot and never round-trips).

**`crates/daemon`** — `engine.rs` (live meter + index with daemon-lifetime-monotonic `SegmentId`s + LRU of ≤16 parsed segments; liveness from observation + the game-process signal, not mtime), `hub.rs` (session table; 10 Hz changed-only pushes; immediate reply on `Watch`), `loader.rs` (historical parses off the hub thread), `server.rs` (accept/reader/writer threads; lockfile taken before the stale socket is unlinked), `game.rs` (3 s /proc sweep for `game_process`), `overlay.rs` (supervisor: spawn `wowdps-gui --overlay` on game start, `SetVisible` on exit, exit-grace termination, manual-hide stickiness, spawn stderr surfaced in `Status`), `cache.rs` (index checkpoints under `$XDG_CACHE_HOME/wowdps/index`; never parsed meters), `config.rs` (section-aware toml-subset reader of `~/.config/wowdps/config.toml`: `logs_dir`, `game_process`, `auto_overlay`, `overlay_exit_grace_secs`), `mock.rs` (in-process fake daemon over the real engine + fixture, driving `ClientState` synchronously — what `testkit` was to the old `App`).

**Fixtures** (`crates/core/fixtures/`): `sample.txt` is a synthetic advanced-format log (2 encounters + trash, 3 players + 1 pet, every modeled event type) with hand-computed golden totals in `sample.expected.md`/`.tsv`, verified independently of the parser by `check.awk`; `corrupt.txt` is the negative control. `FORMAT-NOTES.md` documents the log format itself.

**Frontends** are thin clients: the TUI (`ui.rs` renders `ClientState`, TestBackend tests against `daemon::mock`; `tests/no_engine.rs` greps that tui sources never name engine modules) and the GUI (`window.rs` / `overlay.rs` sharing `view.rs`; config persisted at `~/.config/wowdps/config.toml`). GUI keybinds mirror the TUI's. Under Hyprland the overlay follows the game's workspace (`gui/src/hypr.rs`; config keys `follow_game`/`game_match`) — layer-shell has no unmap, so "hidden" is a 1×1 click-through surface; the daemon's `SetVisible` wish composes with it.

## Debugging

`docs/tracing.md` covers: the daemon-mode workflow (`--status`, `--stop`, `$XDG_STATE_HOME/wowdps/daemon.log`, cache location, source-conflict errors), overlay debug env vars (`WOWDPS_OVERLAY_DEBUG=1` input tracing, `WOWDPS_OVERLAY_START_EXPANDED`, `WOWDPS_OVERLAY_AUTOTOGGLE`), a headless Hyprland workflow for screenshotting/verifying the overlay without a real game, and two iced_layershell 0.19 upstream bugs deliberately worked around in `overlay.rs` (bare `SizeChange` dropped; custom `scale_factor` breaks hit-testing — the overlay renders at scale 1.0 and applies its own zoom).

Also note: the game flushes combat-log writes in multi-minute bursts (anti-overlay countermeasure), so a "frozen" meter is usually just an unflushed buffer — the daemon's liveness verdict uses the game-process signal for exactly this reason; check the log file's mtime before debugging the tail path.
