# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What this is

`wowdps` is a World of Warcraft combat-log damage meter with a client/server split: a headless **daemon** owns the whole pipeline (tail → index → parse → meter → snapshots) and every frontend is a pure rendering client speaking a hand-rolled binary protocol over a unix socket. Crates: `crates/model` (zero-dep domain types), `crates/core` (the engine: parser/meter/index/tail), `crates/proto` (wire codec + `DaemonClient` + `ClientState`, plus the shared client extras `json`/`talents` — the hand-rolled JSON value and the R14 talent dataset + import-string codec, used by mcp and the GUI's talent viewer), `crates/daemon` (hub, loader pool, game watcher, overlay supervisor, index cache), `crates/tui` (binary `wowdps` = daemon + launcher + TUI client), `crates/gui` (binary `wowdps-gui` = window or wlr-layer-shell overlay via `--overlay`; depends on model+proto only, so it *cannot* parse a log), `crates/mcp` (binary `wowdps-mcp`, reached as `wowdps mcp` via the dispatcher's external-command lookup: an MCP stdio server exposing fight data as tools — `status`, `list_fights`, `fight`, `breakdown`, `compare`, `loadout` (v19: a player's logged COMBATANT_INFO talents + gear, talents named via the dataset), the history store's `history` / `progression` / `trend` / `stored_fight` / `pin_fight` / `regrade_fights` (v20: the daemon's fixed questions over stored fights, answered from its card index — `stored_fight` reuses `fight`/`breakdown`'s row shapes and takes `boss` for a key's member, parsed from the log on demand; `history` carries the owner's grade as `me` — role-relative since roadmap 1a step 1: a healer ranks among healers by HPS, a DPS among DPS, tanks unranked but (step 2b) carrying `taken` / `mitigated` / `prevented` / `mitigated_pct` / `dtps` and a `tank_pair`; `history { role }` filters by the SUBJECT's role; `trend { measure: dps|hps|dtps|mitigated_pct }` defaults by role and names its value field by measure (`per_sec` kept as an alias for the coach); `view: "taken"` on `fight` / `breakdown` / `stored_fight` adds a `mitigation` object to the drill, and a stored by-ability list is capped at 16 with the rest rolled up; step 3b: the DPS role is graded by `effective_dps` (R19: damage − received + given, one label) while the legacy `rank_dps` / `dps_*` block keeps raw dps (the block an Augmentation's buffs inflate), rows carry the healing split and support scalars, `stored_fight { player }` on a supporter returns its `support` block with targets, and `trend` defaults every DPS subject to `effective_dps`; step 4b (v25): every row carries `am_uptime_pct` (R18, derived from the stored `am_uptime_ms` over the card's duration) and `externals_given` / `externals_received` as `{count, secs}`, `tank_pair` carries `am_uptime_pct`, a healer subject gets a `healers` block, `trend { measure: am_uptime }`, and `stored_fight { player }` returns `uptime[]` — BOTH halves, the cells where the player is the target and those on other targets where the player is the caster, so "externals given, to whom" is one call — while its `view: "taken"` drill now carries the 10 s coarse timeline with marks (the Healing drill keeps the details tier's 1 s series on kills, `heal10` otherwise); `regrade_fights` rewrites cards from their logs, pins kept) — plus talent tools — `talent_tree`, `decode_talents`, `encode_talents`, answered from the per-machine talent dataset (R14), never the daemon — hand-rolled JSON, model+proto only, so it too cannot parse a log; repo `.mcp.json` registers it for Claude Code via `cargo run`; `history_sql` shells out to `wowdps-history` and is registered only where that binary exists), `crates/history` (binary `wowdps-history`, reached as `wowdps history`: DuckDB — the one non-stdlib dependency, SYSTEM-linked to nixpkgs' libduckdb, never bundled — over the history store's JSON files as views `fights`/`players` (with `role`, derived by spec id so un-regraded lakes answer)/`rows`/`details`/`loadouts`/`annotations`/`role_ranks` (the mcp grader's role-relative rank, floors included, in SQL) and, from roadmap 1a step 2b, `taken` / `mitigation` / `taken_spells` / `taken_sources` (R17 on the rows tier — each defined only after a probe proves the field exists AND is typed, because DuckDB types an all-empty nested field as JSON and a JSON column answers struct references with more JSON instead of erroring; so an un-regraded or mixed lake still opens and `stats` reports `cards_without_taken` / `rows_without_mitigation`) and, from step 3b, `support` / `support_targets` plus `players.effective_dps_sql` (recomputed with a coalesce and a clamp so a pre-3b card ranks exactly as before — `role_ranks` ranks the DPS role by it under one label) and a derived `support` flag and, from step 4b, `uptime` (rows.uptime[] — fight × target × spell × caster, `kind` stored as its name) / `coarse` (the 10 s `taken10` / `heal10` lists cast to `BIGINT[]`, because an all-empty LIST column types as `JSON[]` and the probe now rejects any type starting with JSON — plus the merged mark list) and `players.am_uptime_pct_sql` (coalesced, DOUBLE first; `stats` reports `cards_without_am_uptime` / `rows_without_uptime`), with `docs/history-queries.md` the recipe list every parity run executes; offline by construction; `import` is a thin client of the daemon's `ImportLog`; `tests/parity.rs` is the lake parity gate — the daemon's fixed answers must equal SQL's over the same files).

## Commit messages

Commits follow the Conventional Commits convention in @CC.md.

For documentation-only commits, add `[skip ci]` to the commit message so the expensive CI build doesn't run.

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
cargo run --bin wowdps -- status   # daemon state incl. overlay spawn failures + the history store
cargo run --bin wowdps -- stop

# The history store (roadmap item 1): the daemon writes every closed fight
# as JSON under $XDG_DATA_HOME/wowdps/history/v1/ and imports older logs on
# start; `wowdps history` (crates/history, binary wowdps-history) is DuckDB
# over those files — needs the flake/devenv shell (DUCKDB_LIB_DIR etc.)
cargo run --bin wowdps-history -- sql "select name, duration_ms from fights order by start_utc_ms desc"
cargo run --bin wowdps-history -- best-kill 3130 15   # progression / trend / export / stats / materialize too
cargo run --bin wowdps-history -- regrade --kind key  # rewrite stored cards from their logs (pins kept); also <fight_id> / --encounter N
cargo run --bin wowdps-history -- import ~/Games/wow/Logs   # asks the daemon to sweep a log or dir
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
                               # subtrees, spec gating, spell names + icon names, plus
                               # tooltip lines (Spell.db2 descriptions with their
                               # $-tokens substituted by tools/extract/src/spelltip.rs,
                               # cost/range/cast) and per-currency point caps at max
                               # level (TraitCurrencySource: 34/34/13) — for the mcp
                               # talent tools and the GUI's talent viewer (both through
                               # proto::talents); per-machine cache, never committed
tools/gen-talent-art.sh        # ~/.local/share/wowdps/talent-art.bin (~60 MiB): the
                               # talent UI's own artwork cropped from the client's
                               # UiTextureAtlas sheets — per-spec pane background
                               # paintings, hero-tree medallions (via TraitSubTree's
                               # UiTextureAtlasElementID), the golden medallion ring;
                               # the GUI's talent viewer reads it lazily and renders
                               # plain panels without it

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

The toolchain is **nightly**, declared once in `rust-toolchain.toml` (channel + components); the flake's dev shell and package and `devenv.nix` all build it from that file through rust-overlay, whose locked rev pins the nightly date (so `nix flake update` moves it). Cargo.toml's `rust-version` remains the stable floor — no `#![feature]`; CI's non-blocking canary proves the tree still builds on stable. Building/running the **GUI** needs the flake dev shell (`nix develop`) for pkg-config/libxkbcommon at build time and the `LD_LIBRARY_PATH` (wayland, vulkan-loader, libGL) at runtime — this is NixOS. `devenv.nix` is a twin of that shell (auto-entered via devenv's cd hook after `devenv allow`); keep both in sync, and keep `devenv.yaml`'s nixpkgs and rust-overlay pins matching `flake.lock`. The flake also packages the daemon/TUI binary (`nix build .#wowdps`, pure Rust) and exports `homeManagerModules.default` and `nixosModules.default`, each installing the same systemd user unit (`wowdps daemon --linger`, gated hard on `graphical-session.target`); the two modules live in `nix/` and must stay in lockstep.

Dependency policy (from CONTRACT.md): model zero-dep; core, proto, daemon stdlib only. Approved: ratatui + crossterm (tui); iced + iced_layershell + serde/toml (gui). No chrono (timestamps are hand-parsed), no tokio (threads + channels), no serde outside the gui. Dev-dependencies are exempt within reason: the gui's tests render every screen and canvas headless through `iced_test` + `iced_tiny_skia` and build realistic state from `wowdps-daemon`'s mock over the fixture (`window::testkit`, `Overlay::for_test`, `talents::seam`), so GUI rendering is no longer a coverage blind spot — run `cargo llvm-cov --workspace` after a full `cargo clean` when the toolchain changed.

## Architecture

**CONTRACT.md is the binding interface spec.** It fixes the public signatures of `parser`, `meter`, and `index`, semantic rulings R1–R9 (what counts as damage/healing, absorb attribution, segment boundaries and duration semantics, pet attribution, mid-log `COMBAT_LOG_VERSION` reset, class/spec inference, the death recap), and the wire protocol surface (`PROTO_VERSION`, frame layout, message tags, ordering/id guarantees). Fixture expected values are computed from the rulings; the golden-byte tests in `crates/proto/tests/codec.rs` pin the encodings — changing either means changing CONTRACT.md, the fixtures/golden bytes, and the code together, and a wire-shape change means bumping `PROTO_VERSION` (which renames the socket).

**`crates/core`** — the engine (only the daemon runs it):

- `parser.rs` — one combat-log line → `LogLine`/`Event`. Unknown events become `Event::Other`, never an error.
- `meter.rs` — `Meter::feed` aggregates lines into `Segment`s (Encounter or Trash) and produces `Row`s per `View` plus per-player breakdowns. R17: every damage event is recorded a second time on its DESTINATION as `View::Taken` (amount = R1's amount + absorbed, by-target = attacker name) and every `*_MISSED` line as a count with its prevented amount in a per-player `Mitigation` record (`Segment::mitigation`, raw-guid keyed, folded onto owners at read time); stagger is taken once on the hit and its self-ticks tallied apart, so Σ dealt to friendlies = Σ Taken + Σ stagger_ticked per segment, exactly — nothing in Taken opens or extends a segment. R19: the six `*_SUPPORT` families are `Event::Support` (the parser pops the trailing supporter guid and dispatches on the base family with the BUFF's spell-block prefix); the meter keeps per player, raw-keyed and folded at read, `support` given/received, `support_targets`, `healed` (received from any source, absorbs excluded, self-healed) and `absorbed_healing` (the absorber-credit counter), all through the passive gate; `Segment::effective` = damage − received + given is one number for everyone, derived and never stored, so Σ effective = Σ damage. R18: every aura in the curated role-spell table (`role_spells.rs`, generated by `tools/gen-role-spells.sh` from a hand list the generator validates — name, an APPLY_AURA effect, a committed real-log census) opens a span on its target with the caster on it, checked before the class veto and bypassing the trinket dedupe; a refresh or removal with no open span opens one at the segment start, every mark call site goes through the passive gate, and an open role span closes at READ time (an open trinket proc still reads 0); spans have their own `SPAN_CAP` list and an uncapped per-(spell, caster) `uptime` rollup, `am_uptime_ms` is an exact union, externals are given/received by count and ms, `taken_timeline` is the 1 s taken series. R10: `ZONE_CHANGE`/`CHALLENGE_MODE_*` events track instance *visits* (suspend/resume on zoning, new key = new visit); segments carry their visit's ordinal, and `Meter::overall(ordinal)` merges a visit's members into a synthetic `SegmentKind::Overall` segment (duration = sum of member durations).
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

**`crates/proto`** — `wire.rs` (LE primitives + `u32 len | u8 tag | body` frames, decode never panics), `msg.rs` (`ClientMsg`/`DaemonMsg`; a `Watch` declares a `Cursor` — the list, or a segment+view with optional drill — and the daemon pushes snapshots for exactly that, plus an unsolicited `SegmentList` broadcast whenever the segment id table changes shape, so off-list navigation always resolves ids), `client.rs` (`socket_path()` embeds `PROTO_VERSION`; `ensure_daemon` spawns on demand; `DaemonClient`'s reader thread coalesces stale snapshots), `state.rs` (`ClientState`: the old `App` accessor surface for renderers; `apply`/`on_msg` return requests to send; held-key `j`/`k` clamps against the cached snapshot and never round-trips), `json.rs` + `talents.rs` (the hand-rolled JSON value and the R14 talent dataset + import-string codec — shared by the mcp tools, which re-export them, and the GUI's talent viewer), `history.rs` (the history store's on-disk record codec — `FightCard`/`FightRows`/`FightDetails`/`StoredLoadout`/`Annotation` as one-line JSON documents, `HISTORY_SCHEMA`, fight/log/content ids and the loadout hash; the daemon writes them, every reader parses them here).

**`crates/daemon`** — `engine.rs` (live meter + index with daemon-lifetime-monotonic `SegmentId`s + LRU of ≤16 parsed segments; liveness from observation + the game-process signal, not mtime), `hub.rs` (session table; 10 Hz changed-only pushes; immediate reply on `Watch`), `loader.rs` (historical parses off the hub thread), `server.rs` (accept/reader/writer threads; lockfile taken before the stale socket is unlinked), `game.rs` (3 s /proc sweep for `game_process`), `overlay.rs` (supervisor: spawn `wowdps-gui --overlay` on game start, `SetVisible` on exit, exit-grace termination, manual-hide stickiness, spawn stderr surfaced in `Status`), `cache.rs` (index checkpoints under `$XDG_CACHE_HOME/wowdps/index`; never parsed meters; `write_atomic` is the daemon's one durability primitive), `history.rs` (the history store, roadmap item 1: a thread owning `$XDG_DATA_HOME/wowdps/history/v1/` and an in-memory index of the cards; the hub hands it one `Segment` clone per `EngineEvent::Closed` over a bounded `try_send` and forwards the tailed log's index for import; a start-up sweep imports older logs through the loader pool via `LoadReply::History`, one job at a time; `Store<B: Backend>` is generic — `DirBackend` in production, `MemBackend` for the mock and tests; retention + the protected set run after every write; `HistoryStatus` rides in `Status`), `config.rs` (section-aware toml-subset reader of `~/.config/wowdps/config.toml`: `logs_dir`, `game_process`, `auto_overlay`, `overlay_exit_grace_secs`, and the flat `history_*` keys), `mock.rs` (in-process fake daemon over the real engine + fixture, driving `ClientState` synchronously — what `testkit` was to the old `App`; also feeds every `Closed` into a `MemBackend` store).

**Fixtures** (`crates/core/fixtures/`): `sample.txt` is a synthetic advanced-format log (2 encounters + trash inside one raid visit, 3 players + 1 pet, every modeled event type) with hand-computed golden totals in `sample.expected.md`/`.tsv`, verified independently of the parser by `check.awk`; `corrupt.txt` is the negative control; `instance.txt` exercises R10 (a completed key, suspend/resume, city combat between visits — gated by `crates/core/tests/instance.rs`); `arena.txt` exercises R13 (arena matches as named win/loss Encounter segments — arenas zone in at difficulty 0, so the match's segment is titled from the last ZONE_CHANGE at *any* difficulty; gated by `crates/core/tests/arena.rs`); `taken.txt` exercises R17 (three players, every miss kind, staggered hits, a pet hit before its summon; goldens in `taken.expected.md`/`.tsv`, recomputed by `check.awk`'s destination-side metrics — which is why `sample.expected.tsv` carries `taken` … `stagger_ticked` rows too; gated by `crates/core/tests/taken.rs` and the ignored real-log gate `real_log_taken.rs`); `support.txt` exercises R19 + the R2 amendment (an Augmentation Evoker buffing a Mage, a Warrior and a pet — shares, a self-supported proc the log writes twice, a melee support line — and a Holy Priest with shields, overheal, a self-heal and an NPC-sourced heal; goldens recomputed by `check.awk`'s seven support/healing metrics; gated by `tests/support.rs` and `real_log_support.rs`); `spans.txt` exercises R18 (Shield Block spans incl. one refreshed with no apply and one open at the kill, Shield Wall + Pain Suppression overlapping for the union, externals given by a Priest and a Mage's Time Warp, an Evoker's support buffs, a cooldown and a defensive, a trinket proc proving R12 untouched, a pre-pull aura in the trash dead zone that lands nowhere; `check.awk` computes the union as a per-second bitmap; gated by `tests/spans.rs` and `real_log_spans.rs`). `FORMAT-NOTES.md` documents the log format itself.

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

**Frontends** are thin clients: the TUI (`ui.rs` renders `ClientState`, TestBackend tests against `daemon::mock`; `tests/no_engine.rs` greps that tui sources never name engine modules) and the GUI (`window.rs` / `overlay.rs` sharing `view.rs`; config persisted at `~/.config/wowdps/config.toml`). GUI keybinds mirror the TUI's, plus one window-only extra: `t` opens the **talent viewer** (`gui/src/talents.rs`), a window-local screen (never a model `Screen` variant — the `ClientState` machine doesn't know it exists; Esc closes, and the meter keymap is swallowed while it is open so the text input is typable). It decodes in-game import strings through `proto::talents` against the per-machine `talents.json` and draws the panes the way the game does: class pane left, spec pane right (split at the posX midpoint), the picked hero tree between them under its medallion + golden ring (`gui/src/talent_art.rs` reads `talent-art.bin` — pane background paintings included; absent cache = plain panels). Node frames follow the game's shapes — square = active ability (entryType 1), circle = passive, octagon = choice with side carets — with gold borders, rank pills and lit gold paths for taken talents; icons come shaped/desaturated from `spell_icons::styled`. One iced trap is load-bearing: a canvas `Frame` composites ALL images above ALL vector paths (text above both), so the background painting is a stacked `image` widget UNDER the canvas, never drawn inside it, and nothing vector may need to sit on top of an icon tile. A pasted SimulationCraft addon export (`gui/src/simc.rs`, stdlib parser) also brings saved loadouts (chips switch between them), equipped gear, bag items and currencies (inventory tab); pastes persist per character under `~/.local/share/wowdps/simc/`, so reopening the viewer on that player's meter row restores their build. v19: opening on a row also sends `GetLoadout` (the row supplies name, spec id AND guid); the daemon answers with the player's COMBATANT_INFO loadout — the actual talents + equipped gear from the log — which wins over a stored paste (`adopt_logged`: picks → `proto::talents::picks_to_selections` → `encode` → the normal adopt path, so validation, "copy string" and the warnings pane all just work; a "from combat log" marker shows, gear renders on the inventory tab as honest `item {id}` rows in slot order, simc loadout chips stay one click away, and logged builds are never persisted — the daemon re-answers on every open). The env-gated `real_dataset_lays_out_every_spec` test (`cargo test -p wowdps-gui -- --ignored`) checks every spec of the real dataset lays out. The overlay is single-instance (`gui/src/single.rs`): a new `--overlay` launch evicts the running one via an unversioned takeover socket, so orphans can't stack surfaces or respawn daemons. Under Hyprland the overlay follows the game's workspace (`gui/src/hypr.rs`; config keys `follow_game`/`game_match`) — layer-shell has no unmap, so "hidden" is a 1×1 click-through surface; the daemon's `SetVisible` wish composes with it. Inside an instance visit the overlay anchors its frame on the *visit*: `gui/src/timeline.rs` groups the segment list into blocks (a visit's Σ + members, or a stray segment) and renders the clickable Σ–①─②─③–⚑ strip; the footer ◀▶ steps whole blocks while the strip and its chip line scrub members, a new pull re-pins Live (unless parked on the live visit's Σ), and the footer Σ toggle (`overlay_split` in config) appends the visit's overall rows via a second, `Window`-kind daemon connection.

## Debugging

`docs/tracing.md` covers: the daemon-mode workflow (`wowdps status`, `wowdps stop`, `$XDG_STATE_HOME/wowdps/daemon.log`, cache location, source-conflict errors), overlay debug env vars (`WOWDPS_OVERLAY_DEBUG=1` input tracing, `WOWDPS_OVERLAY_START_EXPANDED`, `WOWDPS_OVERLAY_AUTOTOGGLE`), a headless Hyprland workflow for screenshotting/verifying the overlay without a real game, and two iced_layershell 0.19 upstream bugs deliberately worked around in `overlay.rs` (bare `SizeChange` dropped; custom `scale_factor` breaks hit-testing — the overlay renders at scale 1.0 and applies its own zoom).

Also note: the game flushes combat-log writes in multi-minute bursts (anti-overlay countermeasure), so a "frozen" meter is usually just an unflushed buffer — the daemon's liveness verdict uses the game-process signal for exactly this reason; check the log file's mtime before debugging the tail path.
