# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What this is

`wowdps` is a World of Warcraft combat-log damage meter with a client/server split: a headless **daemon** owns the whole pipeline (tail → index → parse → meter → snapshots) and every frontend is a pure rendering client speaking a hand-rolled binary protocol over a unix socket. Crates: `crates/model` (zero-dep domain types), `crates/core` (the engine: parser/meter/index/tail), `crates/proto` (wire codec + `DaemonClient` + `ClientState`), `crates/daemon` (hub, loader pool, game watcher, overlay supervisor, index cache), `crates/tui` (binary `wowdps` = daemon + launcher + TUI client), `crates/gui` (binary `wowdps-gui` = window or wlr-layer-shell overlay via `--overlay`; depends on model+proto only, so it *cannot* parse a log), `crates/mcp` (binary `wowdps-mcp`, reached as `wowdps mcp` via the dispatcher's external-command lookup: an MCP stdio server exposing fight data as tools — `status`, `list_fights`, `fight`, `breakdown`, `compare` — plus talent tools — `talent_tree`, `decode_talents`, `encode_talents`, answered from the per-machine talent dataset (R14), never the daemon — hand-rolled JSON, model+proto only, so it too cannot parse a log; repo `.mcp.json` registers it for Claude Code via `cargo run`).

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
cargo run --bin wowdps -- status   # daemon state incl. overlay spawn failures
cargo run --bin wowdps -- stop
# No args = daemon follows config `logs_dir`; when unset it discovers the
# install itself ($WOWDPS_WOW_DIR, else a Steam compatdata scan picking the
# newest .build.info — crates/core/src/cli.rs default_logs_dir), erroring
# only when nothing is found. `wowdps daemon [--linger]`
# runs the daemon in the foreground (what systemd and self-spawn use).
# `wowdps-gui` takes no source flags — the daemon owns the log.

# Perf gates against a real log
WOWDPS_REAL_LOG=/path/to/WoWCombatLog-*.txt cargo test --release -p wowdps-core -- --ignored real_log --nocapture
WOWDPS_REAL_LOG=/path/to/WoWCombatLog-*.txt cargo test --release -p wowdps-daemon -- --ignored real_log --nocapture

# Regenerate the generated game-data tables (once per game patch, needs the
# install + network for schemas/keys)
tools/gen-class-spells.sh      # class_spells.rs   (R8)
tools/gen-keystone-timers.sh   # keystone_timers.rs (R10)
tools/gen-item-spells.sh       # item_spells.rs    (R12; SpellEffect is big, be patient)
tools/gen-icons.sh             # ~/.local/share/wowdps/class-icons.bin (class crests +
                               # spec icons, BLP-decoded, circle-masked, per-machine cache
                               # — extracted Blizzard art never lands in the repo)
tools/gen-spell-icons.sh       # ~/.local/share/wowdps/spell-icons.bin: EVERY spell's
                               # icon (~58 MiB, per-machine cache, never committed);
                               # gui reads it lazily for ability icons on by-spell rows
                               # (Row.spell_id, wire v9) and draws none when absent
tools/gen-talent-trees.sh      # ~/.local/share/wowdps/talents.json (R14): every class's
                               # full trait tree — nodes, edges, choice entries, hero
                               # subtrees, spec gating, spell names + icon names — for
                               # the mcp talent tools and external viewers; per-machine
                               # cache, never committed

# Parser-independent fixture check (gawk recomputes golden totals)
crates/core/fixtures/verify.sh                # sample.txt vs sample.expected.tsv
crates/core/fixtures/verify.sh crates/core/fixtures/corrupt.txt   # negative control: must FAIL

# DB2 extractor parity gate (dev-time, network): decode raw client tables
# locally and compare against wago.tools' export of the same build
tools/extract/verify.sh                       # latest live build (or pass one)
tools/extract/verify.sh --game "$WOW_DIR"     # tables read from the install's own
                                              # CASC storage (WOW_DIR holds .build.info)
# wowdps-extract fetch pulls any file from local CASC storage by FileDataID
# (network-free); see tools/extract/src/main.rs for the full CLI
```

Cargo works system-wide, but building/running the **GUI** needs the flake dev shell (`nix develop`) for pkg-config/libxkbcommon at build time and the `LD_LIBRARY_PATH` (wayland, vulkan-loader, libGL) at runtime — this is NixOS. `devenv.nix` is a twin of that shell (auto-entered via devenv's cd hook after `devenv allow`); keep both in sync, and keep `devenv.yaml`'s nixpkgs pin matching `flake.lock`. The flake also packages the daemon/TUI binary (`nix build .#wowdps`, pure Rust) and exports `homeManagerModules.default` and `nixosModules.default`, each installing the same systemd user unit (`wowdps daemon --linger`, gated hard on `graphical-session.target`); the two modules live in `nix/` and must stay in lockstep.

Dependency policy (from CONTRACT.md): model zero-dep; core, proto, daemon stdlib only. Approved: ratatui + crossterm (tui); iced + iced_layershell + serde/toml (gui). No chrono (timestamps are hand-parsed), no tokio (threads + channels), no serde outside the gui.

## Architecture

**CONTRACT.md is the binding interface spec.** It fixes the public signatures of `parser`, `meter`, and `index`, semantic rulings R1–R9 (what counts as damage/healing, absorb attribution, segment boundaries and duration semantics, pet attribution, mid-log `COMBAT_LOG_VERSION` reset, class/spec inference, the death recap), and the wire protocol surface (`PROTO_VERSION`, frame layout, message tags, ordering/id guarantees). Fixture expected values are computed from the rulings; the golden-byte tests in `crates/proto/tests/codec.rs` pin the encodings — changing either means changing CONTRACT.md, the fixtures/golden bytes, and the code together, and a wire-shape change means bumping `PROTO_VERSION` (which renames the socket).

**`crates/core`** — the engine (only the daemon runs it):

- `parser.rs` — one combat-log line → `LogLine`/`Event`. Unknown events become `Event::Other`, never an error.
- `meter.rs` — `Meter::feed` aggregates lines into `Segment`s (Encounter or Trash) and produces `Row`s per `View` plus per-player breakdowns. R10: `ZONE_CHANGE`/`CHALLENGE_MODE_*` events track instance *visits* (suspend/resume on zoning, new key = new visit); segments carry their visit's ordinal, and `Meter::overall(ordinal)` merges a visit's members into a synthetic `SegmentKind::Overall` segment (duration = sum of member durations).
- `index.rs` — fast structural scan (segment boundaries + byte ranges, no per-event parsing) so a 300 MB+ log lists its segments in <1 s; a segment is fully parsed only when opened (`load_segment` + fresh `Meter`), seeded with earlier `SPELL_SUMMON`/`COMBATANT_INFO`/`COMBAT_LOG_VERSION` lines so lazy parsing exactly matches full replay (fixture-gated). The scanner mirrors `Meter::feed`'s segmentation; keep them in lockstep. `Index::checkpoint`/`scan_from` make scans resumable — the daemon's index cache persists checkpoints so restarts rescan only the tail.
- `item_spells.rs` — GENERATED spell-id → `ItemKind` table (regenerate with
  `tools/gen-item-spells.sh`, once per game patch: Item + ItemEffect +
  ItemXItemEffect out of the local install, plus a two-level chase through
  `SpellEffect.EffectTriggerSpell` so trinket *procs* — never the item's own
  listed spell — are covered; rules in `tools/extract/src/itemgen.rs`). Backs
  ruling R12: `Segment::timeline` bucket damage on a 1s grid and marks trinket
  uses, trinket procs and consumables on it. The chase is generous and also
  claims some class spells, so `class_spells` is consulted first and wins.
- `tail.rs` — `Tailer` following a file or the newest log in a directory (poll ~200 ms, rotation-aware). On open: `Switched` → `Index` (one scan, injectable via `with_scan` for the cache) → `Lines` from `live_offset`; `CaughtUp` separates backlog replay from fresh combat.
- `class_spells.rs` — GENERATED spell-id → class/spec table (regenerate with `tools/gen-class-spells.sh`, once per game patch: it reads the eight source tables straight out of the local install via `tools/extract` — the `wowdps-extract` workspace crate, stdlib-only — whose pipeline is WDC5 `.db2` + WoWDBDefs `.dbd` → CSV plus a full local-install CASC reader (`fetch`: .build.info → build config → .idx/archives → BLTE with hand-rolled inflate + Salsa20 → encoding → root manifest), proven byte-identical to wago.tools' raw files and parity-gated by `tools/extract/verify.sh` (which also takes `--game`); attribution rules live in `tools/extract/src/classgen.rs`, network is only touched for schemas/keys, and output is deterministic per build; `tools/gen-keystone-timers.sh` regenerates `keystone_timers.rs` (R10 par timers from MapChallengeMode.db2) the same way). Backs ruling R8: out of instances COMBATANT_INFO never fires, so the meter infers a player's class/spec from their casts — segment-local only (never carried forward), COMBATANT_INFO overwrites it, and it must never open a segment, or lazy/full parity breaks.

**`crates/proto`** — `wire.rs` (LE primitives + `u32 len | u8 tag | body` frames, decode never panics), `msg.rs` (`ClientMsg`/`DaemonMsg`; a `Watch` declares a `Cursor` — the list, or a segment+view with optional drill — and the daemon pushes snapshots for exactly that, plus an unsolicited `SegmentList` broadcast whenever the segment id table changes shape, so off-list navigation always resolves ids), `client.rs` (`socket_path()` embeds `PROTO_VERSION`; `ensure_daemon` spawns on demand; `DaemonClient`'s reader thread coalesces stale snapshots), `state.rs` (`ClientState`: the old `App` accessor surface for renderers; `apply`/`on_msg` return requests to send; held-key `j`/`k` clamps against the cached snapshot and never round-trips).

**`crates/daemon`** — `engine.rs` (live meter + index with daemon-lifetime-monotonic `SegmentId`s + LRU of ≤16 parsed segments; liveness from observation + the game-process signal, not mtime), `hub.rs` (session table; 10 Hz changed-only pushes; immediate reply on `Watch`), `loader.rs` (historical parses off the hub thread), `server.rs` (accept/reader/writer threads; lockfile taken before the stale socket is unlinked), `game.rs` (3 s /proc sweep for `game_process`), `overlay.rs` (supervisor: spawn `wowdps-gui --overlay` on game start, `SetVisible` on exit, exit-grace termination, manual-hide stickiness, spawn stderr surfaced in `Status`), `cache.rs` (index checkpoints under `$XDG_CACHE_HOME/wowdps/index`; never parsed meters), `config.rs` (section-aware toml-subset reader of `~/.config/wowdps/config.toml`: `logs_dir`, `game_process`, `auto_overlay`, `overlay_exit_grace_secs`), `mock.rs` (in-process fake daemon over the real engine + fixture, driving `ClientState` synchronously — what `testkit` was to the old `App`).

**Fixtures** (`crates/core/fixtures/`): `sample.txt` is a synthetic advanced-format log (2 encounters + trash inside one raid visit, 3 players + 1 pet, every modeled event type) with hand-computed golden totals in `sample.expected.md`/`.tsv`, verified independently of the parser by `check.awk`; `corrupt.txt` is the negative control; `instance.txt` exercises R10 (a completed key, suspend/resume, city combat between visits — gated by `crates/core/tests/instance.rs`); `arena.txt` exercises R13 (arena matches as named win/loss Encounter segments — arenas zone in at difficulty 0, so the match's segment is titled from the last ZONE_CHANGE at *any* difficulty; gated by `crates/core/tests/arena.rs`). `FORMAT-NOTES.md` documents the log format itself.

Meter rows wear the game's own art, all from PER-MACHINE caches under
`~/.local/share/wowdps/` — extracted Blizzard artwork never lands in the
repository, and a machine without the caches renders fine. `class-icons.bin`
(`tools/gen-icons.sh`: classicon_* crests + ChrSpecialization spec icons,
decoded by `tools/extract/src/blp.rs` — BLP2: DXT1/3/5, palettized, raw —
32px, circle-masked; read whole by `gui/src/icons.rs`, ~200 KiB) and
`spell-icons.bin` (`tools/gen-spell-icons.sh`: every spell id via SpellMisc,
~58 MiB; `gui/src/spell_icons.rs` loads the index once and reads tiles on
demand). `compare::class_icon` prefers the spec icon, falls back to the class
crest, then to the drawn class-colored disc; ability icons on by-spell rows
simply vanish without their cache. iced's "image" feature exists solely for
this; no image files are decoded at runtime.

**R12 comparison** (GUI only): clicking a meter row's class icon picks that
player; the second pick opens `Screen::Compare`, which renders two per-spell
tables (hits / crit% / average) each over a timeline graph — rolling DPS or
cumulative (`g`), with vertical bars for trinket uses, trinket procs and
consumables. Shared render code is `gui/src/compare.rs` (pure, message-free,
so `window.rs` and `overlay.rs` both use it; the overlay grows its surface to
`COMPARE_MIN` while comparing). Both graphs share one y-scale and one x-range
— per-side scaling would make every pair look identical, which is the one
thing a comparison must not do.

**Frontends** are thin clients: the TUI (`ui.rs` renders `ClientState`, TestBackend tests against `daemon::mock`; `tests/no_engine.rs` greps that tui sources never name engine modules) and the GUI (`window.rs` / `overlay.rs` sharing `view.rs`; config persisted at `~/.config/wowdps/config.toml`). GUI keybinds mirror the TUI's. The overlay is single-instance (`gui/src/single.rs`): a new `--overlay` launch evicts the running one via an unversioned takeover socket, so orphans can't stack surfaces or respawn daemons. Under Hyprland the overlay follows the game's workspace (`gui/src/hypr.rs`; config keys `follow_game`/`game_match`) — layer-shell has no unmap, so "hidden" is a 1×1 click-through surface; the daemon's `SetVisible` wish composes with it. Inside an instance visit the overlay anchors its frame on the *visit*: `gui/src/timeline.rs` groups the segment list into blocks (a visit's Σ + members, or a stray segment) and renders the clickable Σ–①─②─③–⚑ strip; the footer ◀▶ steps whole blocks while the strip and its chip line scrub members, a new pull re-pins Live (unless parked on the live visit's Σ), and the footer Σ toggle (`overlay_split` in config) appends the visit's overall rows via a second, `Window`-kind daemon connection.

## Debugging

`docs/tracing.md` covers: the daemon-mode workflow (`wowdps status`, `wowdps stop`, `$XDG_STATE_HOME/wowdps/daemon.log`, cache location, source-conflict errors), overlay debug env vars (`WOWDPS_OVERLAY_DEBUG=1` input tracing, `WOWDPS_OVERLAY_START_EXPANDED`, `WOWDPS_OVERLAY_AUTOTOGGLE`), a headless Hyprland workflow for screenshotting/verifying the overlay without a real game, and two iced_layershell 0.19 upstream bugs deliberately worked around in `overlay.rs` (bare `SizeChange` dropped; custom `scale_factor` breaks hit-testing — the overlay renders at scale 1.0 and applies its own zoom).

Also note: the game flushes combat-log writes in multi-minute bursts (anti-overlay countermeasure), so a "frozen" meter is usually just an unflushed buffer — the daemon's liveness verdict uses the game-process signal for exactly this reason; check the log file's mtime before debugging the tail path.
