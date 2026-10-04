# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What this is

`wowdps` is a World of Warcraft combat-log damage meter with a client/server split: a headless **daemon** owns the whole pipeline (tail → index → parse → meter → snapshots) and every frontend is a pure rendering client speaking a hand-rolled binary protocol over a unix socket. Crates: `crates/model` (zero-dep domain types), `crates/core` (the engine: parser/meter/index/tail), `crates/proto` (wire codec + `DaemonClient` + `ClientState`, plus the shared client extras `json`/`talents` — the hand-rolled JSON value and the R14 talent dataset + import-string codec, used by mcp and the GUI's talent viewer), `crates/daemon` (hub, loader pool, game watcher, overlay supervisor, index cache), `crates/tui` (binary `wowdps` = daemon + launcher + TUI client), `crates/gui` (binary `wowdps-gui` = window or wlr-layer-shell overlay via `--overlay`, on Zed's GPUI through GPUI Kit; depends on model+proto and `crates/gui-logic` — the framework-free GUI logic: config, Hyprland IPC, the keymap, history pages, cache readers, every layout model — only, so it *cannot* parse a log), `crates/mcp` (binary `wowdps-mcp`, reached as `wowdps mcp` via the dispatcher's external-command lookup: an MCP stdio server exposing fight data as tools — `status`, `list_fights`, `fight` (v35: with `raid`, R25's raid rate per 10 s, the deaths in order and the lust; rows carry `mine`; v40: the fight object carries `combat` / `combat_ms`, the clock `per_sec` divides by, and a key's rows add `run_per_sec` over the key timer — `stored_fight` too), `breakdown` (v35: a recap event's `offset_secs`; v36: damage and healing rows carry R26's `group`, `casts` / `avg_cast` and `parts`, with `ability_groups` beside them — `stored_fight` too), `compare`, `loadout` (v19: a player's logged COMBATANT_INFO talents + gear, talents named via the dataset), the history store's `history` / `progression` / `trend` / `stored_fight` / `pin_fight` / `regrade_fights` (v20: the daemon's fixed questions over stored fights, answered from its card index — `stored_fight` reuses `fight`/`breakdown`'s row shapes and takes `boss` for a key's member, parsed from the log on demand, and (v39) `from_secs` / `to_secs` for a player's damage or healing drill over a zoom window, answered from the series tier; `history` carries the owner's grade as `me` — role-relative since roadmap 1a step 1: a healer ranks among healers by HPS, a DPS among DPS, tanks unranked but (step 2b) carrying `taken` / `mitigated` / `prevented` / `mitigated_pct` / `dtps` and a `tank_pair`; `history { role }` filters by the SUBJECT's role; `trend { measure: dps|hps|dtps|mitigated_pct }` defaults by role and names its value field by measure (`per_sec` kept as an alias for the coach); `view: "taken"` on `fight` / `breakdown` / `stored_fight` adds a `mitigation` object to the drill, and a stored by-ability list is capped at 16 with the rest rolled up; step 3b: the DPS role is graded by `effective_dps` (R19: damage − received + given, one label) while the legacy `rank_dps` / `dps_*` block keeps raw dps (the block an Augmentation's buffs inflate), rows carry the healing split and support scalars, `stored_fight { player }` on a supporter returns its `support` block with targets, and `trend` defaults every DPS subject to `effective_dps`; step 4b (v25): every row carries `am_uptime_pct` (R18, derived from the stored `am_uptime_ms` over the card's duration) and `externals_given` / `externals_received` as `{count, secs}`, `tank_pair` carries `am_uptime_pct`, a healer subject gets a `healers` block, `trend { measure: am_uptime }`, and `stored_fight { player }` returns `uptime[]` — BOTH halves, the cells where the player is the target and those on other targets where the player is the caster, so "externals given, to whom" is one call — while its `view: "taken"` drill now carries the 10 s coarse timeline with marks (the Healing drill keeps the details tier's 1 s series on kills, `heal10` otherwise); step 5 (v26, R20): healer rows carry `absorb_wasted` (null = unknown), `shields_unknown` and `absorb_efficiency_pct`, `stored_fight { player }` returns `shields[]` per shield spell, `trend { measure: absorb_efficiency }` skips unknown points, and `role_night { encounter, difficulty, night | date }` is the daemon's fixed question for one night's roster by role — tanks side by side, healers and DPS ranked; `regrade_fights` rewrites cards from their logs, pins kept) — plus talent tools — `talent_tree`, `decode_talents`, `encode_talents`, answered from the per-machine talent dataset (R14), never the daemon — hand-rolled JSON, model+proto only, so it too cannot parse a log; repo `.mcp.json` registers it for Claude Code via `cargo run`; `history_sql` shells out to `wowdps-history` and is registered only where that binary exists), `crates/history` (binary `wowdps-history`, reached as `wowdps history`: DuckDB — the one non-stdlib dependency, SYSTEM-linked to nixpkgs' libduckdb, never bundled — over the history store's JSON files as views `fights`/`players` (with `role`, derived by spec id so un-regraded lakes answer)/`rows`/`details`/`loadouts`/`annotations`/`role_ranks` (the mcp grader's role-relative rank, floors included, in SQL) and, from roadmap 1a step 2b, `taken` / `mitigation` / `taken_spells` / `taken_sources` (R17 on the rows tier — each defined only after a probe proves the field exists AND is typed, because DuckDB types an all-empty nested field as JSON and a JSON column answers struct references with more JSON instead of erroring; so an un-regraded or mixed lake still opens and `stats` reports `cards_without_taken` / `rows_without_mitigation`) and, from step 3b, `support` / `support_targets` plus `players.effective_dps_sql` (recomputed with a coalesce and a clamp so a pre-3b card ranks exactly as before — `role_ranks` ranks the DPS role by it under one label) and a derived `support` flag and, from step 4b, `uptime` (rows.uptime[] — fight × target × spell × caster, `kind` stored as its name) / `coarse` (the 10 s `taken10` / `heal10` lists cast to `BIGINT[]`, because an all-empty LIST column types as `JSON[]` and the probe now rejects any type starting with JSON — plus the merged mark list) and `players.am_uptime_pct_sql` (coalesced, DOUBLE first; `stats` reports `cards_without_am_uptime` / `rows_without_uptime`), with `docs/history-queries.md` the recipe list every parity run executes, and, from step 5, `shields` (rows.shields[] per absorber × spell: applied / consumed / wasted / count / unknown), `players.absorb_wasted` / `shields_unknown` / `absorb_efficiency_sql` (NULL is the honest pre-5 and unknown value, never 0) and the `role-night` subcommand (the SQL twin of `RoleNight`, UTC nights); offline by construction; `import` is a thin client of the daemon's `ImportLog`; `tests/parity.rs` is the lake parity gate — the daemon's fixed answers must equal SQL's over the same files).

## Knowledge bundle (docs/OKF)

`docs/OKF/` is an Open Knowledge Format v0.2 bundle — the graph over the
crates, CONTRACT.md's rulings, the fixtures, the game-data tools, and the
decisions/patterns/playbooks behind them — maintained with `okf`
(github:kriswill/okflight; on the dev-shell PATH, else
`nix run .#okf -- <cmd>`). Crate, Ruling and Tool docs are scaffolded from
the sources by `docs/OKF/_okflight/scripts/`; the rest is hand-authored.
The `knowledge-bundle` skill (`.claude/skills/`) is the maintenance loop:
after adding a crate, ruling, fixture or generator, or making a decision
worth a long commit body, update the bundle in the same change and run
`okf validate` (must exit 0). Conventions: `docs/OKF/okf-profile.md`.

## The GUI is GPUI

`crates/gui` (`wowdps-gui`, `wowdps gui`) draws the window and the overlay on
Zed's GPUI through GPUI Kit. It was built as `crates/gui-new` beside the iced
GUI, reproducing every overlay guard state and every design-shot state, and
took its place at the cutover (`docs/plan-gui-new.md` phase 5). The rules it
was built under still hold:

- **Shared logic lives in `crates/gui-logic`.** Anything the GUI computes
  without drawing — config, Hyprland IPC, the chord table, the rail's nights,
  Home's week, the inspector's geometry, the talent layout, every word — is
  gui-logic's, with its tests; the GUI brings a text measure and its paint.
  The TUI's keybind parity test reads gui-logic's chord table too.
- **Dependencies.** `gpui-kit` is pinned exactly (`=0.7.0`), and its styled
  `gpui-component` layer is restyled per control. No forked or patched GPUI:
  a capability GPUI lacks is designed around or contributed upstream.
- **Themes.** One theme definition (gui-logic's `theme::Def`: `onyx`, the
  default (`theme::default_def`: no `theme` key, an unknown name, a user
  theme with no `base`); `navy` — the prototype's Tokens and the overlay's
  palette, called `gold` until there were themes, a name config still reads
  (so a config that says `gold` keeps the old look); and `frost`; config
  `theme`, chosen in the window's ⚙ card) feeds both Kit's `Theme` and the app's `Look`
  (`crates/gui/src/theme.rs`). No surface draws a literal colour, face,
  corner or effect: each comes from the `Def`, and config.toml's
  `[themes.<name>]` tables override any built-in's tokens or define a theme
  of the user's own (gui-logic's `theme::Registry`; `wowdps-gui
  --print-theme <name>` prints every key). Navy's pixels are the prototype's:
  a new token, effect or corner role must leave them unmoved (the overlay
  render guard and the Navy window shots are the check).
- **Tests.** Kit's `gpui_kit::test::TestWindowExt` is the click-through
  harness, over the daemon's mock through the `Session` link (`testkit.rs`).
  Its default `TestAppContext` has a stub text system (every glyph 0.6 em,
  fonts ignored), so any test whose answer depends on text metrics (heights,
  widths, truncation, the chrome budget) runs in `testkit::headless()`, a
  `HeadlessAppContext` over the platform's real cosmic-text system.
- **Kit's Root is for the window.** On a layer surface it adds a 20 px
  shadow border and paints the theme's ground, so the overlay opens its
  view through GPUI (`cx.open_window`), not `gpui_kit::open_window`.
- **Every animation is finite.** GPUI's animation frame re-renders the whole
  view that asked for it, and the window is one root view: an endless pulse
  on the live dot idled it at 5.7 % of a core (three pulses per live pull:
  0.4 %). An animation is keyed to what triggered it and settles to static
  pixels; something that must keep moving (the overlay's staleness radar)
  runs on a timer at the slowest rate that still reads as motion.
- **Delights settle to parity under reduced motion.** Motion beyond the
  prototype (a bar easing to its new length, the tab underline gliding, a
  zoom gliding, a card's entrance, the talent ripples) is gone when
  `cx.reduce_motion()` is set — what the platform reports, and what every
  shot test sets — so the settled pixels are the reviewed ones.

The documents:

- **Spec:** `docs/spec-gui-new.md` (the GUI's design: platform, themes,
  surfaces, testing, the cutover criteria).
- **Plan:** `docs/plan-gui-new.md`. Phases 0–5 with *As built* notes per
  step (every parity gap and deliberate difference is named there), spikes
  S1–S12, and a devil's-advocate review log of traps to remember.
- **Policy:** the `gui:` line in CONTRACT.md §Dependencies.
- **Rationale:** `docs/OKF/decisions/gui-on-gpui.md` and `no-gpui-forks.md`.
- **GPUI reference:** `docs/gpui/README.md`. Read it before any GPUI work.

GPUI Kit pins `gpui-pre` (a crates.io snapshot of Zed's GPUI whose library
is still `gpui`), not crates.io's stale `gpui` 0.2.2. The reference's
mirror (Kit's pages, API digests rendered from docs.rs rustdoc JSON, the
exact crate sources) is gitignored. Regenerate it with
`tools/fetch-gpui-docs.sh [kit-version]`. Ripgrep skips it unless you
pass the path; `grep -rn … docs/gpui/src/` works. A GPUI bump is its own
deliberate change, between other work.

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
coderabbit review --agent         # CodeRabbit's review of local changes (`cr` for short;
                                  # `--base main` for the branch); both dev shells carry it
                                  # (`coderabbit auth login` once). Never `coderabbit update`:
                                  # the store is read-only, the weekly lock update moves it.
                                  # In Claude Code here, .claude/settings.json's SessionStart
                                  # hook puts that same build on the Bash tool's PATH, and the
                                  # project's CodeRabbit skills (.claude/skills/code-review and
                                  # autofix, from coderabbitai/skills, skills-lock.json) replace
                                  # the built-in /code-review; the code-review plugin is off.
                                  # Never edit those files alone: a local fix is a patch in
                                  # .claude/skills/patches/, and tools/skills-update.sh
                                  # refreshes them and re-applies every patch (never
                                  # `bunx skills update`: it symlinks them for every agent)

# Inside the flake/devenv shell the workspace's own binaries are on PATH as
# `wowdps` / `wowdps-history` / `wowdps-mcp` / `wowdps-gui` —
# thin wrappers that `cargo build --release --bin <it>` from the live checkout and then
# `exec` the real binary, so a shell can never hand you a stale build (the
# `exec` also keeps `current_exe` in target/release, which is how the
# dispatcher finds its siblings). `WOWDPS_NO_BUILD=1` skips the build; the
# wrappers refuse to run outside this checkout. Everything below can be read
# as either `wowdps <cmd>` or the `cargo run` form.

# Run against the committed fixture log (the client forwards the source to
# the daemon it spawns; the daemon idle-exits ~10s after the last client)
cargo run --bin wowdps -- --file crates/core/fixtures/sample.txt
cargo run --bin wowdps -- status   # daemon state incl. overlay spawn failures + the history store
cargo run --bin wowdps -- stop
wowdps addon install               # the wowdps ADDON (addon/, embedded in the daemon): a few
                                   # lines of Lua the game runs that write every raid member's
                                   # guild — which the combat log never carries — into its
                                   # SavedVariables on logout; the daemon reads them into the
                                   # history store's affiliations/ and rewrites a stale copy on
                                   # start (never installs one). `wowdps addon` reports it.

# The history store (roadmap item 1): the daemon writes every closed fight
# as JSON under $XDG_DATA_HOME/wowdps/history/v1/ and imports older logs on
# start; `wowdps history` (crates/history, binary wowdps-history) is DuckDB
# over those files — needs the flake/devenv shell (DUCKDB_LIB_DIR etc.)
cargo run --bin wowdps-history -- sql "select name, duration_ms from fights order by start_utc_ms desc"
cargo run --bin wowdps-history -- best-kill 3130 15   # progression / trend / export / stats / materialize too
wowdps history regrade --kind key   # rewrite stored cards from their logs (pins kept); also <fight_id> / --encounter N
cargo run --bin wowdps-history -- import ~/Games/wow/Logs   # asks the daemon to sweep a log or dir
# No args = daemon follows config `logs_dir`; when unset it discovers the
# install itself ($WOWDPS_WOW_DIR, else a Steam compatdata scan picking the
# newest .build.info — crates/core/src/cli.rs default_logs_dir), erroring
# only when nothing is found. `wowdps daemon [--linger]`
# runs the daemon in the foreground (what systemd and self-spawn use).
# `wowdps-gui` takes no source flags — the daemon owns the log.

# THE LIVE DAEMON ON A DEV MACHINE is the `wowdps-dev` systemd user unit
# (tools/dev-unit.sh; templates in tools/dev-unit/, installed into
# ~/.config/systemd/user with the checkout path baked in). Never run
# `wowdps daemon --linger` by hand and never rely on a self-spawned daemon:
# a self-spawned one is `linger: no` and idle-exits ~10 s after its last
# client (an mcp dying with a claude session took the overlay down mid-game
# on 2026-09-20). The unit runs target/<profile>/wowdps --linger and wins the
# socket by asking any running daemon to stop first.
tools/dev-unit.sh install          # write + enable + start (--no-start to defer)
tools/dev-unit.sh status           # profile, overlay GUI, both units, `wowdps status`
tools/dev-unit.sh profile debug    # or release: switch builds, restarts if running
systemctl --user restart wowdps-dev   # THE way to restart after a test stopped it
# Rebuilds restart it for you: wowdps-dev.path watches
# target/{debug,release}/wowdps{,-gui} and the reload oneshot restarts the
# service only when the ACTIVE profile's daemon or CONFIGURED overlay GUI
# changed (config `gui_binary`, default wowdps-gui; stamped at start) — so
# `cargo build --release --bin wowdps` or `--bin wowdps-gui` bounces the
# daemon AND its supervised overlay a few seconds later, while a build of a
# GUI it does not spawn bounces nothing; warn the user if they are mid-pull
# (docs/tracing.md). A `systemctl --user stop wowdps-dev` stays
# stopped through rebuilds; the wrappers' own `cargo build` counts as a
# rebuild only when it actually writes a new binary. The debug profile needs
# a debug build of the configured GUI beside the daemon, or the overlay
# spawns from $PATH if at all.

# Perf gates against a real log
WOWDPS_REAL_LOG=/path/to/WoWCombatLog-*.txt cargo test --release -p wowdps-core -- --ignored real_log --nocapture
WOWDPS_REAL_LOG=/path/to/WoWCombatLog-*.txt cargo test --release -p wowdps-daemon -- --ignored real_log --nocapture

# Regenerate the generated game-data tables (once per game patch, needs the
# install + network for schemas/keys)
tools/gen-class-spells.sh      # class_spells.rs   (R8)
tools/gen-keystone-timers.sh   # keystone_timers.rs (R10)
tools/gen-item-spells.sh       # item_spells.rs    (R12; SpellEffect is big, be patient)
tools/gen-proc-spells.sh       # proc_spells.rs    (R26 step 4: CURATED talent proc -> driver,
                               # proven by the client's text + tools/census-proc-spells.sh,
                               # whose CSV is committed; re-run the census when curating)
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

# The GUI's headless review loop (crates/gui/SHOTS.md): ignored tests that
# render through GPUI's headless renderer (a wgpu adapter: the GPU, or Mesa's
# lavapipe) over the daemon's mock — no window on screen, no daemon, debug
# builds, so nothing is launched beside a running game
WOWDPS_SHOTS_DIR=/tmp/shots cargo test -p wowdps-gui window_shots -- --ignored --nocapture
WOWDPS_SHOTS_DIR=/tmp/shots cargo test -p wowdps-gui overlay_shots -- --ignored
cargo test -p wowdps-gui overlay_render_guard -- --ignored   # by name; WOWDPS_BLESS=1 re-blesses
# also talent_shots, inspector_plot_shots, the_chrome_budget_holds_on_the_log

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

The toolchain is **nightly**, declared once in `rust-toolchain.toml` (channel + components); the flake's dev shell and package and `devenv.nix` all build it from that file through rust-overlay, whose locked rev pins the nightly date (so `nix flake update` moves it). Cargo.toml's `rust-version` remains the stable floor — no `#![feature]`; CI's non-blocking canary proves the tree still builds on stable. Building the **GUI** needs the flake dev shell (`nix develop`) for pkg-config and the libraries GPUI links (libxkbcommon, libxcb, fontconfig) — this is NixOS; the dlopened runtime libraries (wayland-client, vulkan-loader, EGL/libGL) are baked into the binary's RUNPATH by `crates/gui/build.rs` from the shell's `LD_LIBRARY_PATH` at link time, so a GUI built in the shell runs from anywhere (the daemon's overlay supervisor, a plain terminal); one built outside it cannot find its Wayland or Vulkan libraries. `devenv.nix` is a twin of that shell (auto-entered via devenv's cd hook after `devenv allow`) — both `import ./nix/dev`, which IS the environment, so the two can no longer drift; each file adds only what it alone plumbs, its Rust toolchain and the CLIs it reaches through its own inputs: `okf`, and `coderabbit` (alias `cr`, the CodeRabbit review CLI — prebuilt and unfree, not in nixpkgs — from numtide's `llm-agents.nix`, which keeps its own nixpkgs because following ours its package set does not evaluate). That directory is parcelled by concern — `wrappers.nix` (the `wowdps-gen-*` generators and the four workspace-binary wrappers, both resolved against the live checkout), `env.nix` (`DUCKDB_*` plus the dlopened libraries behind `LD_LIBRARY_PATH`), `contract.nix` (what a shell must deliver, built as the runnable `wowdps-dev-contract` from the same command list that builds the wrappers) and `default.nix` assembling them. `devenv test` IS that contract; the flake half is `nix develop -c wowdps-dev-contract`. Keep `devenv.yaml`'s nixpkgs, rust-overlay, okf and llm-agents pins matching `flake.lock` (`update-locks.yml` does it weekly). The flake also packages the daemon/TUI binary (`nix build .#wowdps`, pure Rust, built with crane as a dependency layer `.#wowdps-deps` keyed on Cargo.lock plus the workspace crates on top over a `lib.fileset`-filtered source, so CI downloads the dependency compile from FlakeHub Cache and a docs edit rebuilds nothing) and the GUI the same way (`.#wowdps-gui` over `.#wowdps-gui-deps`, wrapped so the dlopened libraries are on its `LD_LIBRARY_PATH`; its check phase runs the real-text tests on Mesa's lavapipe, the sandbox having no GPU) and exports `homeManagerModules.default` and `nixosModules.default`, each installing the same systemd user unit (`wowdps daemon --linger`, gated hard on `graphical-session.target`, with `guiPackage` — the flake's `.#wowdps-gui` by default — on the service PATH for the overlay supervisor); the two modules live in `nix/` beside `dev/` and must stay in lockstep.

Dependency policy (from CONTRACT.md): model zero-dep; core, proto, daemon stdlib only. Approved: ratatui + crossterm (tui); for the gui, `gpui-kit` pinned exactly with its default features and no `tree-sitter*` (the `gpui-pre` family, `gpui-base`, `gpui-component` and `gpui-kit-assets` accepted as ONE unit, transitive crates included, none of which our code may name except serde/toml and `image`), plus `image` (`default-features = false`, GPUI's locked version) only to build `RenderImage` frames; no `[patch]` and no fork of any GPUI or Kit crate; serde/toml serve gui-logic's config; the window's bundled OFL fonts (`crates/gui-logic/fonts/`) are assets, not dependencies, and its line icons are strokes from gui-logic's `glyph` table, not SVG assets. No chrono (timestamps are hand-parsed), no tokio (threads + channels), no serde outside the gui crates (gui, gui-logic) — rules for our own code; GPUI pulling them transitively is accepted with it. Dev-dependencies are exempt within reason: the gui's tests drive every surface through Kit's `TestWindowExt` and the real-text `HeadlessAppContext` (Kit's `test-support`, plus `image`'s png codec for the pictures), building realistic state from `wowdps-daemon`'s mock over the fixtures (`crates/gui/src/testkit.rs`) — run `cargo llvm-cov --workspace` after a full `cargo clean` when the toolchain changed.

## Architecture

**CONTRACT.md is the binding interface spec.** It fixes the public signatures of `parser`, `meter`, and `index`, semantic rulings R1–R9 (what counts as damage/healing, absorb attribution, segment boundaries and duration semantics, pet attribution, mid-log `COMBAT_LOG_VERSION` reset, class/spec inference, the death recap), and the wire protocol surface (`PROTO_VERSION`, frame layout, message tags, ordering/id guarantees). Fixture expected values are computed from the rulings; the golden-byte tests in `crates/proto/tests/codec.rs` pin the encodings — changing either means changing CONTRACT.md, the fixtures/golden bytes, and the code together, and a wire-shape change means bumping `PROTO_VERSION` (which renames the socket).

**`crates/core`** — the engine (only the daemon runs it):

- `parser.rs` — one combat-log line → `LogLine`/`Event`. Unknown events become `Event::Other`, never an error.
- `meter.rs` — `Meter::feed` aggregates lines into `Segment`s (Encounter or Trash) and produces `Row`s per `View` plus per-player breakdowns. GAME-METER PARITY (2026-10-02, v40, reconciled against the game's built-in meter on a real +15): R1's amount is `amount + absorbed − overkill` on every ledger (overkill stays in the Damage row's `extra`; the R9 recap keeps the raw hit); every rate divides by `Segment::combat_ms` — an encounter's START..END, a trash segment's ENGAGEMENT clock (stretches between group-vs-enemy DAMAGE lines, a quiet one over `ENGAGE_QUIET_MS` = 30 s left out; misses and friendly fire never run it), an Overall's Σ members' (a key's too: its `duration_ms` stays the key timer, and its run rate is a reader's `wowdps_model::rate(amount, duration_ms)` — the window's stat line, the overlay's chip, the TUI header and mcp's `run_per_sec` say it); and R22 holds FRIENDLY FIRE off the Damage row too (a hit on another `Player-`/`Pet-` from its own side by the flags, or from a unit a player on the victim's side summoned — a totem's last lines read `0xa28` — Spirit Link Totem's redistribution; an arena enemy's summon hitting us stays the enemy's damage), tallied as `Segment::friendly_fire` and kept in the victim's Taken. R22: damage an actor deals to ITSELF (own pets folded — a Brewmaster's Stagger self-ticks and his Niuzao's own were 21% of his Damage row on a real +14) never reaches a Damage row, drill, timeline or DPS; it is `Segment::self_harm` (`total` + the `on_friendly` subset whose destination guid is `Player-`/`Pet-`), raw-keyed and folded at read; "itself" means the same raw guid or a unit it SUMMONED (`Meter::summon_fold`, never the ownership map R4 folds with — a charmed mob writes to that one, and folding through it would swallow a Priest's damage to a mob they mind-controlled), and the R17 identity restates as Σ dealt to friendlies + Σ self_harm_on_friendly + Σ friendly_fire = Σ Taken + Σ stagger_ticked — the `on_friendly` half because a guardian summoned as a `Creature-` unit (Niuzao) folds onto its owner offensively but never earns a Taken row (segmentation untouched: these lines still open and extend segments, or the scanner desyncs). R17: every damage event is recorded a second time on its DESTINATION as `View::Taken` (amount = R1's amount + absorbed, by-target = attacker name) and every `*_MISSED` line as a count with its prevented amount in a per-player `Mitigation` record (an ABSORB — a hit a shield took whole — is R1's hit since 2026-09-30: amount 0 + absorbed on the attacker's Damage, R24 and Taken alike, through the passive gate; `prevented` is then the full blocks alone) (`Segment::mitigation`, raw-guid keyed, folded onto owners at read time); stagger is taken once on the hit and its self-ticks tallied apart (see R22 above for the identity's current form) — nothing in Taken opens or extends a segment. R19: the six `*_SUPPORT` families are `Event::Support` (the parser pops the trailing supporter guid and dispatches on the base family with the BUFF's spell-block prefix); the meter keeps per player, raw-keyed and folded at read, `support` given/received, `support_targets`, `healed` (received from any source, absorbs excluded, self-healed) and `absorbed_healing` (the absorber-credit counter), all through the passive gate; `Segment::effective` = damage − received + given is one number for everyone, derived and never stored, so Σ effective = Σ damage. R18: every aura in the curated role-spell table (`role_spells.rs`, generated by `tools/gen-role-spells.sh` from a hand list the generator validates — name, an APPLY_AURA effect, a committed real-log census) opens a span on its target with the caster on it, checked before the class veto and bypassing the trinket dedupe; v34: the table covers every spec's major offensive cooldown (Summon Demonic Tyrant, Dark Transformation, Berserk, Shadow Dance …), long defensive, self-shield and raid-wide wall, and a SIXTH kind `HealingCooldown` (Tranquility, Divine Hymn, Apotheosis, Tree of Life, Resto's Ascendance — a hybrid's Avenging Wrath stays `Cooldown`), proven against an eight-log census with `census_exempt` for specs no committed log holds; the GUI's graphs draw only the marks their VIEW is about (gui-logic's `graph::view_draws`, per mark `view_draws_mark`: items, externals and deaths everywhere — except a Healthstone, which stays off the Damage graph alone, cooldowns and support on Damage and Healing, healing cooldowns on Healing alone, mitigation and defensives on Taken alone — so a cooldown's window and its duration read off the damage curve, and a wall's off the taken curve); a refresh or removal with no open span opens one at the segment start, every mark call site goes through the passive gate, and an open role span closes at READ time (an open trinket proc still reads 0); spans have their own `SPAN_CAP` list and an uncapped per-(spell, caster) `uptime` rollup, `am_uptime_ms` is an exact union, externals are given/received by count and ms, `taken_timeline` is the 1 s taken series. R20: a Buff aura whose spell is in the generated absorb table (`absorb_spells.rs`, every SCHOOL_ABSORB effect) opens a shield keyed (target, spell, caster) with its trailing amount as `applied`; a refresh's trailer is the NEW RUNNING TOTAL (up = more applied, down = waste), a `SPELL_ABSORBED` on the key is `consumed` (an over-absorb raises `applied`; an absorb with no open key opens an unknown-applied shield, so no healing is lost), a removal's trailer is `wasted` (else the known remainder), and a shield still open at READ time folds consumed + count + unknown only — so Σ `shields().consumed` = `absorbed_healing` exactly, `applied = consumed + wasted` on every known row, and `absorb_wasted` is `None`, never a silent 0, when no waste was ever known. R21: the two `_DOSE` families parse to `Event::AuraDose` (14 fields, the trailer = the aura's new running stack total, both families); a Debuff on a friendly from a source the group does not control (no table) drives a per-(raw victim, spell) level ledger — applied 1, dose n, refresh unchanged, removed gone, an orphan dose/refresh/removal opens at its level — and every Taken hit lands in one cell per open debuff at its level (`(damage label+id, aura, level) → {hits, sum, max}`, per-victim cap 512 newest-dropped); `stacking_debuffs` / `stack_cells` / `stacks_dropped` fold onto owners at read, level 0 is the READER's derivation from the by-ability row (sum exact, count an upper bound because a Taken row counts R17's events, max unknown), the ledger never merges (an Overall's cells are Σ members'). R23: a player's UNIT_DIED opens a DEATH SPAN, closed by the
`SPELL_RESURRECT` that raised them (battle rez, Soulstone, Ankh, Mass
Resurrection — now `Event::Resurrect`, passive so segmentation is untouched)
or, failing that, by their first SIGHT ALIVE — a line whose advanced block
reports them above 0 health (a heal or hit on them, their own cast or swing),
or a cast — through the passive gate; damage they deal is no proof (their
imps implode and projectiles land after they die); a Feign Death (UNIT_DIED's
trailing `unconsciousOnDeath` 1) is no death at all (R9: the parser makes it
`Other`, the scanner no combat). One still open at the end reads to the
fight's close and is marked `open` (v41); an Overall carries a member's open
death into the next member, which ends it at its first sight of them
(`Segment::first_alive`) — a wipe's deaths never "end" at the wipe. It rides
every timeline as a `MarkKind::Death` mark (`Segment::death_spans`, its own
map so R18's `spans`/`uptime`/`am_uptime` never see it), labelled
with the rez spell and carrying the rezzer as its caster — so a flat stretch
of dtps says why it is flat. R24: every damage event whose destination is an
ENEMY (a `Creature-`/`Vehicle-` guid that is not ours — a summoned guardian is
ours) and whose source is ours (a friendly guid, or a unit whose owner is a
player as known at the hit) is recorded a THIRD time on the destination as
`View::EnemyTaken` — the game's own "Enemy Damage Taken" pane: one row per
enemy NAME (every hostile guid wearing it folds; the name is the row's key),
by-spell = the ability, by-target = the attacker's owner GUID, folded at read
(pets under their masters, class and spec on the row); an enemy row has no
class and is never `enemy`; Σ EnemyTaken = Σ friendly units' Damage
by_target over hostile names (`tests/taken.rs`); Shift-E everywhere,
`enemy_taken` in the mcp tools, wire code 7 under PROTO_VERSION 32 — and v33: a
zoom window dragged on the drill's graph rides `Cursor::Segment.range` and
scopes the attackers and one attacker's abilities to it (snapped to whole
seconds, echoed on the `Breakdown`) — and never STORED (`VIEW_KEYS` is a fixed
seven, `View::is_stored`, so a card offers no ☠ tab and the stored tools
refuse the view by name). R25 (v35): `Segment::raid_timeline(view)` is the
whole group's fight at READ time — the view's raid series (`View::raid_series`:
healing, damage taken on Taken/Deaths, else damage) summed over the actors the
friendly rows fold (Σ series = the rows' total, `tests/raid.rs`), every kept R9
death window in time order with its killing blow (the recap's newest damage)
and its R23 rez (death spans now keep the resurrect's spell and rezzer), and
the lust windows (`LUST_SPELLS`' R18 spans unioned) — riding every meter
`Snapshot`; each recap Row carries `offset_ms`, its time before the death. R26 (v36): `Segment::spell_tree(player, view)` nests a player's Damage and Healing by-ability rows at READ time — a pet's rows under the spell that SUMMONED it (the first SPELL_SUMMON per (summoner, pet name), now parsed; a hunter's "Call Pet N" or no summon seen → the pet's own name), a spell one trinket owns under the item (`item_spells::trinket_of`, `class_spells` vetoing first), and every row split into `(spell id, periodic)` parts that sum to it (Wither's two ids; Shadow Word: Pain's one id landing both ways) — plus each row's CASTS, `SPELL_CAST_SUCCESS` counted per caster per spell name through the passive gate (a precast, a post-kill cast, the trash dead zone and hostile casts land nowhere); rides `Breakdown.tree` and the details tier's `damage_tree`/`heal_tree`. Step 2: Healing keeps a per-spell series too (`heal_spell_series`), `SpellTree::entries` (model) is the ONE definition of a top-level entry, and Step 3: each row's MISSES (every `*_MISSED` but an ABSORB by the player and their pets under its name — the attacker's side R17 never counted) and, on the player's own row, a DoT's UPTIME — the union of their debuff of that name over every enemy (`Segment::dots`, closed at read time), so two adds at once count once. `Segment::ability_series` / `target_series` give the drill graph its stack — the six largest entries (Σ every entry = the player's curve), or an open ability's six largest enemy targets from R24's per-unit series — on `Breakdown.ability_series` / `target_series`, built for `Window` sessions only (`engine::wants_series`). Step 4 (v37): a talent proc hangs under the spell that DRIVES it — `proc_spells.rs`, GENERATED from a curated list in `tools/extract/src/procgen.rs` whose every entry the client's own text (or a trigger effect) and a real-log census (`tools/census-proc-spells.sh`) must both prove; the driver's row is found by name (the player's own, else a pet's), the proc joins its group or the two form `GroupKind::Spell` ("spell:Wither" holding Wither and Blackened Soul). R10: `ZONE_CHANGE`/`CHALLENGE_MODE_*` events track instance *visits* (suspend/resume on zoning, new key = new visit); segments carry their visit's ordinal, and `Meter::overall(ordinal)` merges a visit's members into a synthetic `SegmentKind::Overall` segment (duration = sum of member durations).
- `index.rs` — fast structural scan (segment boundaries + byte ranges, no per-event parsing) so a 300 MB+ log lists its segments in <1 s; a segment is fully parsed only when opened (`load_segment` + fresh `Meter`), seeded with earlier `SPELL_SUMMON`/`COMBATANT_INFO`/`COMBAT_LOG_VERSION` lines so lazy parsing exactly matches full replay (fixture-gated). The scanner mirrors `Meter::feed`'s segmentation; keep them in lockstep. `Index::checkpoint`/`scan_from` make scans resumable — the daemon's index cache persists checkpoints so restarts rescan only the tail.
- `item_spells.rs` — GENERATED spell-id → `ItemKind` table (regenerate with
  `tools/gen-item-spells.sh`, once per game patch: Item + ItemEffect +
  ItemXItemEffect out of the local install, plus a two-level chase through
  `SpellEffect.EffectTriggerSpell` so trinket *procs* — never the item's own
  listed spell — are covered; rules in `tools/extract/src/itemgen.rs`). Backs
  ruling R12: `Segment::timeline` bucket damage on a 1s grid and marks trinket
  uses, trinket procs and consumables on it. The chase is generous and also
  claims some class spells, so `class_spells` is consulted first and wins. R26: it also names each
  trinket spell's ONE owning item (`trinket_of`, ItemSparse names; a proc two
  differently named trinkets share has none) — the ability tree's item groups.
- `tail.rs` — `Tailer` following a file or the newest log in a directory (poll ~200 ms, rotation-aware). On open: `Switched` → `Index` (one scan, injectable via `with_scan` for the cache) → `Lines` from `live_offset`; `CaughtUp` separates backlog replay from fresh combat.
- `class_spells.rs` — GENERATED spell-id → class/spec table (regenerate with `tools/gen-class-spells.sh`, once per game patch: it reads the eight source tables straight out of the local install via `tools/extract` — the `wowdps-extract` workspace crate, stdlib-only — whose pipeline is WDC5 `.db2` + WoWDBDefs `.dbd` → CSV plus a full local-install CASC reader (`fetch`: .build.info → build config → .idx/archives → BLTE with hand-rolled inflate + Salsa20 → encoding → root manifest), proven byte-identical to wago.tools' raw files and parity-gated by `tools/extract/verify.sh` (which also takes `--game`); attribution rules live in `tools/extract/src/classgen.rs`, network is only touched for schemas/keys, and output is deterministic per build; `tools/gen-keystone-timers.sh` regenerates `keystone_timers.rs` (R10 par timers from MapChallengeMode.db2) the same way). Backs ruling R8: out of instances COMBATANT_INFO never fires, so the meter infers a player's class/spec from their casts — segment-local only (never carried forward), COMBATANT_INFO overwrites it, and it must never open a segment, or lazy/full parity breaks.

**`crates/proto`** — `wire.rs` (LE primitives + `u32 len | u8 tag | body` frames, decode never panics), `msg.rs` (`ClientMsg`/`DaemonMsg`; a `Watch` declares a `Cursor` — the list, or a segment+view with optional drill — and the daemon pushes snapshots for exactly that, plus an unsolicited `SegmentList` broadcast whenever the segment id table changes shape, so off-list navigation always resolves ids), `client.rs` (`socket_path()` embeds `PROTO_VERSION`; `ensure_daemon` spawns on demand and waits, for one-shot clients; `DaemonClient::try_reconnect` is the tick-driven clients' path — window, overlay, TUI — one connect attempt, a spawn at most once per doubling backoff, never a wait, because a window that blocked its UI thread 3 s per tick on the old path spawned 628 daemons in half an hour and the compositor called it unresponsive; `DaemonClient`'s reader thread coalesces stale snapshots), `state.rs` (`ClientState`: the old `App` accessor surface for renderers; `apply`/`on_msg` return requests to send; held-key `j`/`k` clamps against the cached snapshot and never round-trips; `log_id` is the tailed log's identity, from `SegmentList`, which with a row's start names its stored card), `json.rs` + `talents.rs` (the hand-rolled JSON value and the R14 talent dataset + import-string codec — shared by the mcp tools, which re-export them, and the GUI's talent viewer), `history.rs` (the history store's on-disk record codec — `FightCard`/`FightRows`/`FightDetails`/`StoredLoadout`/`Annotation`/`Affiliation` as one-line JSON documents, `HISTORY_SCHEMA`, fight/log/content ids and the loadout hash; the daemon writes them, every reader parses them here — `Affiliation::read_saved_variables` turns the wowdps addon's `WOWDPS_DATA` into records), `lua.rs` (a stdlib reader of the Lua the game writes to `SavedVariables/*.lua`: `NAME = value` globals, tables with bracketed / bare / positional keys, every escape the serializer emits, `1/0`-style floats; a reader of serializer output, never an interpreter), `series.rs` (v39: the history store's one BINARY tier, `series/<id>.bin` — per player the Damage and Healing abilities and the damage targets second by second as `model::series::SeriesRow`s, runs of seconds and varints behind a (guid, offset, length) index so `read_player` reads one player's block alone; decode never panics or allocates past the bytes in hand).

**`crates/daemon`** — `engine.rs` (live meter + index with daemon-lifetime-monotonic `SegmentId`s + LRU of ≤16 parsed segments; liveness from observation + the game-process signal, not mtime), `hub.rs` (session table; 10 Hz changed-only pushes; immediate reply on `Watch`), `loader.rs` (historical parses off the hub thread), `server.rs` (accept/reader/writer threads; lockfile taken before the stale socket is unlinked), `game.rs` (3 s /proc sweep for `game_process`), `overlay.rs` (supervisor: spawn `<gui_binary> --overlay` on game start, `SetVisible` on exit, exit-grace termination, manual-hide stickiness, spawn stderr surfaced in `Status`; config `gui_binary`, default `wowdps-gui`, is resolved by `Config::gui_bin`: a bare name is the daemon binary's sibling, else found on `$PATH`, and a value with a `/` is a path), `cache.rs` (index checkpoints under `$XDG_CACHE_HOME/wowdps/index`; never parsed meters; `write_atomic` is the daemon's one durability primitive), `addon.rs` (the wowdps addon's life on disk — v31: `product_dir` derives `<install>/_retail_` from the tailed logs dir and validates the install, `interface_version` reads `.build.info`'s Version into the TOC's `## Interface:` number, `install` writes `Interface/AddOns/wowdps/` atomically, `inspect` tells Current from Stale byte-for-byte, `ensure_current` is the start-up rule — rewrite a stale copy, leave a missing one missing — and `saved_variables` lists every account's `wowdps.lua`), `history.rs` (the history store, roadmap item 1: a thread owning `$XDG_DATA_HOME/wowdps/history/v1/` and an in-memory index of the cards — plus, v31, `affiliations/<guid>.json` from the addon's SavedVariables, read on start and on a 30 s idle poll, newest sighting per guid, JOINED onto a card's players when it is answered and never stored on the card; the addon's own-character set names the owner before the COMBATANT_INFO intersection does; the hub hands it one `Segment` clone per `EngineEvent::Closed` over a bounded `try_send` and forwards the tailed log's index for import; a start-up sweep imports older logs through the loader pool via `LoadReply::History`, one job at a time; when the tailer SWITCHES to a newer log the hub sends `HistoryReq::Retire` for the one left behind, and the thread rescans it as an older log so its open tail — an abandoned pull, the raid night's Σ — imports without a restart (file-derived on purpose: the game-process signal never closes anything); `Store<B: Backend>` is generic — `DirBackend` in production, `MemBackend` for the mock and tests; retention + the protected set run after every write; v39: the series tier (`series/<id>.bin`) is written beside the details for a kill, a key or a pinned fight (`Retention::wants_series`), unlinked with them or when a wipe's pin goes, kept in an in-memory set, read per player through `Backend::read_range`, and windows a stored Damage or Healing drill through the model's `series::window_rows` — the live drill's own function, so `tests/series.rs` holds stored = live window for window; pinning a fight that earns it but lacks it queues its rewrite from the log; v42: format 2 adds each Damage ability's targets second by second, every ability's whole-fight targets and the 1 s damage taken, and the details tier the count views' drills (`counts[]`), so a stored drill stacks (`model::series::stack`, the live ranking), opens an ability and compares (`Ask` carries view, drill, death, range, spell, pair and `stacked`; one `answer` serves the stored and the derived path) exactly as live; every boss kill and timed key is kept whole (`Retention::keeps_whole`, protected; config `history_keep_kills_whole`, on by default) and the caps count the unprotected alone; each series file's format is read once at open, and after its first status the thread queues `Store::rewrites` — a kept fight short of its details, a series file missing or in an OLDER format (never a newer one) — and rewrites them from their logs a log's fights at a time while idle; `HistoryStatus` rides in `Status`), `config.rs` (section-aware toml-subset reader of `~/.config/wowdps/config.toml`: `logs_dir`, `game_process`, `auto_overlay`, `overlay_exit_grace_secs`, `gui_binary`, and the flat `history_*` keys), `mock.rs` (in-process fake daemon over the real engine + fixture, driving `ClientState` synchronously — what `testkit` was to the old `App`; also feeds every `Closed` into a `MemBackend` store).

**Fixtures** (`crates/core/fixtures/`): `sample.txt` is a synthetic advanced-format log (2 encounters + trash inside one raid visit, 3 players + 1 pet, every modeled event type) with hand-computed golden totals in `sample.expected.md`/`.tsv`, verified independently of the parser by `check.awk`; `corrupt.txt` is the negative control; `instance.txt` exercises R10 (a completed key, suspend/resume, city combat between visits — gated by `crates/core/tests/instance.rs`); `arena.txt` exercises R13 (arena matches as named win/loss Encounter segments — arenas zone in at difficulty 0, so the match's segment is titled from the last ZONE_CHANGE at *any* difficulty; gated by `crates/core/tests/arena.rs`; its win also holds the other team's death, which R25's raid timeline flags `enemy` — gated in `crates/core/tests/raid.rs`); `taken.txt` exercises R17 + R22 (three players, every miss kind, staggered hits, a pet hit before its summon, and a Niuzao — the Brewmaster's ox cooldown, a `Creature-` guid guardian that stomps for real damage and takes a share of the stagger — summoned mid-pull to hit the boss once and stagger ITSELF once, the R22 fold with its two halves apart; goldens in `taken.expected.md`/`.tsv`, recomputed by `check.awk`'s destination-side metrics — which is why `sample.expected.tsv` carries `taken` … `stagger_ticked` rows too; gated by `crates/core/tests/taken.rs` and the ignored real-log gate `real_log_taken.rs`); `support.txt` exercises R19 + the R2 amendment (an Augmentation Evoker buffing a Mage, a Warrior and a pet — shares, a self-supported proc the log writes twice, a melee support line — and a Holy Priest with shields, overheal, a self-heal and an NPC-sourced heal; goldens recomputed by `check.awk`'s seven support/healing metrics; gated by `tests/support.rs` and `real_log_support.rs`); `spans.txt` exercises R18 (Shield Block spans incl. one refreshed with no apply and one open at the kill, Shield Wall + Pain Suppression overlapping for the union, externals given by a Priest and a Mage's Time Warp, an Evoker's support buffs, a cooldown and a defensive, a trinket proc proving R12 untouched, a pre-pull aura in the trash dead zone that lands nowhere; `check.awk` computes the union as a per-second bitmap; gated by `tests/spans.rs` and `real_log_spans.rs`); `shields.txt` exercises R20 (a Discipline Priest's Power Word: Shield ledger — partial waste on removal, a running-total refresh up and a refresh-down, a re-apply while open, an over-absorb, a pre-pull shield seen only by its absorb, one open at the kill — a Mage's Ice Barrier, a Blood DK's Blood Shield, a Brewmaster's stagger excluded, a non-shield buff carrying a trailer that never becomes a row, and auras after the kill and in the trash dead zone that land nowhere; `check.awk` runs the same per-key state machine and fails on a `remaining ≠ REMOVED trailer` mismatch; gated by `tests/shields.rs` and `real_log_shields.rs`; the aura admission table is the GENERATED `core/src/absorb_spells.rs`, `tools/gen-absorb-spells.sh`, every spell with a SCHOOL_ABSORB effect); `stacks.txt` exercises R21 (a tank's Tectonic Strike ladder 0 → 1 → 2 → 3 → refresh → 2 → 0 under Crushing Smash with the max at 3, the debuff's own hit at its new level, two debuffs open at once, an orphan dose opening at 4, a player-sourced Slow and a stacking BUFF as negative controls, a pet's hit folding, a dodge at 3 stacks, an absorbed hit, an aura after the kill and a dose in the dead zone landing nowhere; `check.awk` runs its own level machine and emits the five `stack_*` metrics; the per-cell table is `tests/stacks.rs`'s, the real-log gate `real_log_stacks.rs`). `tree.txt` exercises R26 (a Destruction Warlock's Sayaad and Infernal, Wither's two ids, two trinkets, every cast the passive gate refuses; a Priest's one-id hit-and-tick and Renew; `check.awk` emits `casts` / `damage_periodic` / `heal_periodic` / `misses_dealt` / `dot_uptime_ms` for every fixture; structure in `tests/tree.rs`). `FORMAT-NOTES.md` documents the log format itself.

Meter rows wear the game's own art, all from PER-MACHINE caches under
`~/.local/share/wowdps/` — extracted Blizzard artwork never lands in the
repository, and a machine without the caches renders fine. `class-icons.bin`
(`tools/gen-icons.sh`: classicon_* crests + ChrSpecialization spec icons,
decoded by `tools/extract/src/blp.rs` — BLP2: DXT1/3/5, palettized, raw —
32px, circle-masked; read whole by `gui-logic/src/icons.rs`, ~200 KiB) and
`spell-icons.bin` (`tools/gen-spell-icons.sh`: every spell id via SpellMisc,
~58 MiB; `gui-logic/src/spell_icons.rs` loads the index once and reads tiles on
demand). The readers are generic over the image handle a GUI makes of a tile
(`lazy_tiles::Tiles<K, H>`); the GUI's handle is an `Arc<RenderImage>` made on
a tile's first use and cloned ever after (`crates/gui/src/images.rs`), so GPUI
uploads each texture once. A player's disc (`window/chrome.rs`'s and the
overlay's `rows.rs`'s `class_icon`) prefers the spec icon, falls back to the
class crest, then to a disc in the class colour wearing its two-letter tag;
ability icons on by-spell rows simply vanish without their cache. The `image`
crate is named only to build those `RenderImage` frames; no image file is
decoded at runtime.

**The GUI** (`crates/gui`, binary `wowdps-gui`) is two frontends, each over
its own `ClientState` held in a `Session` (`session.rs`): the **window**
(`window.rs`, its pieces under `window/`) and the **overlay** (`overlay.rs`
and `overlay/`, `--overlay`) — thin clients like the TUI. A `Session` is one
daemon link (the `Link` trait: the `DaemonClient` in the app, the daemon's
mock under test) and its `ClientState` as a GPUI entity. All its work is
`pump` — reconnect if the link died (one attempt, never a wait), drain, apply
each message, send what the state asks for, and notify on ANY non-empty drain
(not on `snapshot_gen`, which `CompareSnapshot` and `SegmentList` never move)
— on a 100 ms `TICK`, with `GetStatus` once a second because the daemon never
broadcasts it; tests call `pump` themselves and run no timer, so the executor
parks. Every UI change goes through `Session::act`, and a one-shot answer
(`Loadout`, `History`, `Fight`, `HistoryChanged`) comes back as a `Reply`
event, matched by `req_id`, to whoever asked through `Session::request`.
`main.rs` makes the daemon link BEFORE GPUI starts (`DaemonClient::connect`
may spawn a daemon and wait, a wait no frame may take), registers the keymap
and the bundled faces, applies the theme and opens one surface. Config lives
at `~/.config/wowdps/config.toml` (`crates/gui-logic/src/config.rs`, the one
writer: every save atomic, and a gesture's key written alone through
`Config::store` / `store_character` / `store_character_class`, never a
launch-time copy over an overlay drag). The two surfaces share no renderer:
the overlay draws the iced overlay's pixels in the theme's overlay palette,
the window the redesign's Tokens (*Chrome and type*). **The design record is
`docs/design/window-redesign.html`**: an interactive prototype built from a
real Heroic Coiled Altar kill (25 players, six deaths, one Heroism), the
critique that drove it (ten findings ranked by what each costs after a pull),
the six-step build plan (Look, Header, Inspector, Rail, Wire, Home) and the
Tokens section every window colour and size comes from. Its CSS values and JS
behaviour are the spec — window code cites its selectors (`.fhead`, `.rail`,
`.insp`, `.pal` …) — and each step's rationale is a decision in the knowledge
bundle (`docs/OKF/decisions/window-redesign.md` links them all). Reference
renders of the prototype live outside the repository, under
`~/.local/share/wowdps/design-shots/reference/` (`wide-*` 1440×900, `tile-*`
960×880, `narrow-*` 460×860, captured at 1.25×).

**The TUI** (`ui.rs` renders `ClientState`, TestBackend tests against
`daemon::mock`; `tests/no_engine.rs` greps that tui sources never name engine
modules) keeps every pre-redesign semantic: each `ClientState` capability the
window added is OPT-IN (`set_follow`, `open_death`, `select_player`) and the
TUI calls none. GUI keybinds mirror the TUI's through ONE chord table,
gui-logic's `keys::ACTIONS` (`Chord` → `Action`), and
`crates/tui/tests/keybind_parity.rs` iterates that table against the TUI's
match arms and fails on a binding the TUI lacks. The GUI registers the table
as GPUI `KeyBinding`s once at start (`crates/gui/src/keys.rs`): every meter
action rides ONE action, `keys::Do(Action)`, in the `Meter && !Input` context
— GPUI's `!` reads the whole context stack, so no meter key fires while a
text field has the keys — and the zoom chords ride `ZoomTo`, window-wide. A
typed character binds as itself (`?`, `~`, `+`; `K` is Shift+K). Every
window-only gesture (`t`, `p`, Ctrl K, `/`, `m`, `~`, `H`, `?`, `f`) is a
`keys::Go(Gesture)` the window answers itself, never through the table, and
is listed in `keys::BINDINGS` with `window_local: true`. Two window-local rows
are extra READINGS of keys the table also answers: ← → (older/newer segment
there; the window's second meaning steps a recap's deaths) and the Deaths
table's `j`/`k` (down/up there; the window walks the deaths in order, each
step its recap). The window's other key contexts: `Modal` (a menu is up),
`Filter` (the row filter), `Palette` (the command palette) and `Talents` (the
talent viewer), each below.

**R12 comparison** (GUI only): the pair is `ClientState`'s (at most two
picked players, a third replaces the older). The OVERLAY draws it as
`Screen::Compare` (`overlay/panel/compare.rs`): clicking a meter row's class
icon picks that player, the second pick opens two per-spell tables (hits /
crit% / average) each over a timeline graph (`overlay/graph.rs`, from
gui-logic's `graph` model) — rolling DPS or cumulative (a click: the overlay
has no keyboard), with vertical bars for trinket uses, trinket procs and
consumables — the surface grown to `COMPARE_MIN` while comparing. The WINDOW
draws the same pair inside its inspector (below): `v` or a class icon pins,
the next move makes the pair. Either way both curves share one y-scale and
one x-range — per-side scaling would make every pair look identical, which is
the one thing a comparison must not do. v29: a comparison is about the VIEW it
was opened from (`Cursor::Compare.view`, echoed by `CompareSnapshot` and read
back as `ClientState::compare_view`) — on Taken the tables list the abilities
that HIT them ("hit by"), the header's rate is dtps, the curve is
`Segment::taken_timeline` (so consumables, externals and mitigation spans
still mark it) and each side's R17 mitigation record sits under its table;
the view keys re-ask for the same pair. The drag-zoom window scopes the
tables on Damage and (v38) Healing — `Segment::spells_in` reads their sparse
per-spell series — and the daemon echoes `range: None` on other views rather
than pairing a zoomed graph with full-fight tables. Hovering a spell row lights the SAME ability in BOTH lists
(`Overlay::spell_hover`, the window's `InspState::spell_hover`), matched by
by-spell KEY because the two lists are sorted independently. Every list that
answers the mouse wears its surface's one hover fill (the window's `hover`
token), fainter than the selection's mark and borderless, so a hover can sit
on the selected row without arguing with it. The window keeps the pointer's
position in `Gui::row_hover` (`RowHover::Meter` / `Death(i)` on the Deaths
table) and the inspector's in `InspState::hover` (pane, line — a pair draws
two lists side by side, so the pane is part of the answer); it is drawn and
never sent anywhere.

**The window's layout.** A top bar over a body: the pull rail beside (or, at
1180 px and under, over) either Home or the STAGE — a pull's fight header,
ribbon, view tabs, meter and the inspector beside it. `Gui` (`window.rs`) is
the root entity: the log's `Session`, the place on show, the open cards, the
filter and sort, the pointer, the inspector's state and the history reads.
The breakpoints are the prototype's, gui-logic's `theme::TILE_WINDOW` (1180)
and `theme::NARROW_WINDOW` (820), inclusive at both and read at zoom 1
(`Gui::width`, as the last frame laid it out; `Fit::of`). GPUI has no app
scale factor, so the window zooms by hand: every window renderer draws
through `window/w.rs`'s `W` — the tokens, type scale, pitches, accent, faces,
the zoom (a size is `w.z(…)`) and the width — and Ctrl = / Ctrl - / Ctrl 0
step 0.1 over 0.5–3.0, written alone through `Config::store`. A pull is
drawn from the stage's `ClientState` — `Gui::fight()`, the tailed log's or a
stored pull's own, and `Gui::act` acts on it — so one set of renderers draws
both. Home, the talent viewer, the command palette, the `?` sheet and the
rail are WINDOW-LOCAL: no `Screen` variant, so `ClientState` (and with it the
TUI and the overlay) never learns they exist.

**Top bar** (`window/top_bar.rs`, the prototype's `.top`): the wordmark (in
Marcellus), two places — Home and Fights, the active one underlined in the
accent — the jump box ("Jump to a pull, player or view": a press, like Ctrl K
from anywhere, opens the command palette), the live pill (the log's newest
pull, "Live, Trash 11:43" with a red dot while it goes — the dot pulses three
times per live pull, never forever — "Latest, …" with a ring once over; a
press pins it as `m` does), the character picker, the gear (the ⚙ options
card: row ranks, hide realm names, the chrome) and help (`?`); at 820 px and
under the wordmark goes, the jump box is a glyph, the pill keeps its dot and
clock and the picker its icon. The picker is a spec icon + the
class-colored name of the character picked — config `character`, Home's
scope — else of the character played last (`Gui::picked`; whose window it is
stays `Gui::played`), opening the character menu (`window/cards/menu.rs`) at
the window root (hover per row, `hide_realms` honoured): a character row
picks them and scopes Home, and while one is picked a "Follow the character
I'm playing" item over the rows lets the pick go (Home, when up, on every
character; iced's picker named only the character played, which read as
stuck on a pick). The frame recites no keys:
the footer carries only the daemon's status line, and the `?` sheet
(`window/cards/sheet.rs`) is keyed on the surface (gui-logic's
`keys::Surface::of`, from the window-local screens first, through
`Gui::surface`): it lists every binding that works on this surface in the
prototype's four groups — Move, Views, Inspector, Go to — and dims the keys
the pull on the stage cannot answer (`Gui::inert_keys`). Its keycaps are the
window's own `kbd`, not Kit's `Kbd`, which capitalises a single letter (`j`
and `J` would read alike).

**Pull rail** (`window/rail.rs`, its model gui-logic's `rail`): the window
draws no fight list and no History screen — ONE PULL RAIL lists tonight's log
and every stored night, beside Home or the stage, 236 px at the left above
1180 px (`RAIL_W`), and at 1180 and under a 280 px drawer over a scrim
(`DRAWER_W`; the fight header's list button or `H` opens it; the scrim, Esc —
before anything under it —, Enter or a pick closes it; while it is open
`Gui::rail_key` takes the keys first: j, k and the arrows walk a highlight
over its rows for Enter to open, and `[` `]` step the stage leaving it open
on the row they reach — the `?` sheet's "pull list" surface). Nights by their
LOCAL date with a 06:00 cutover so a raid past midnight is one night
("Tonight" for the night it is now, in the store's newest card's timezone, or
any night with a pull still going; else "Saturday, Sep 26"), then visits (an
instance and its difficulty wearing the logger's class dot; a night's keys
and dungeon runs as one "Mythic+ keys" visit, a key's zone-in visit left out;
a delve; a raid visit's stored Σ), then pulls newest first with the visit's Σ
("Whole visit") last: ✓ kill/timed, ✕ wipe/over, a dash for trash, Σ, a red
dot while live, a ★ in the gutter on a pinned card, and at the right a wipe's
best %, a key's +N or "over", a character dot where one visit holds several
characters, the duration; a "Hide trash" toggle; "Show older nights" pages
the store. A step leaves 40 px under the row it reaches, and the drawer
opening stands the night's heading at the top. Tonight is the daemon's
`SegmentList` grouped by `timeline::blocks` (a segment it filed under no
visit but inside one's span is that visit's); earlier nights are
`HistoryQuery::Fights` pages (gui-logic's `history::Earlier`: every
character's, `home::PAGE` cards — under the store's `FIGHTS_CAP` — newest
first, one request in flight, the newest 20 cards re-asked and merged after
`HistoryChanged`; a refused read — no cards and a total of 0 while cards are
in hand — is asked again after a pause and never taken for the store's word).
`window/history.rs` routes every read (`Hist`: the rail's `Earlier` pages, a
stored pull, Home's week, the store's state from `Status`, a 250 ms tick that
re-asks a refused read). A stored card of the tailed log that its list does
not hold joins the log's visit it followed; a card with no visit (an arena,
the open world) joins no raid visit; a raid pull no visit claims is named for
its instance (the log's visit that night at its difficulty, else any Σ card
of its map), "Raid, Heroic" only when nothing names it. A stored card that is
also a segment of the tailed log (`ClientState::log_id` + the row's
`start_ms` is its fight id, a Σ with its mark) is listed once, as the log's,
lending it the card's best % and owner — and Home's links to it open the
log's segment. `[` `]` (and ← →) walk the rail's drawn order, stored nights
included — past the last card in hand `[` asks for the next page — as the
header's ‹ › do; a move keeps the view and the inspected player. `m` pins the
log's live pull; the window never shows the `ClientState` List screen (the
TUI still does): with no pull on the stage the log's newest takes it. A
STORED PULL opens in the same workspace (gui-logic's `history::Stored`): a
`ClientState` of the pull's own, fed synthetic snapshots built from
`GetFight` answers, whose every `Watch` becomes the `GetFight` for its view,
drill and death window — one read in flight for the whole window, a pull
stepped onto while the last one's read is out waiting for its answer, and an
empty answer asked again after a pause before the pull is called gone; its
header names its night when it is not tonight's, shows the card's wipe % and
a ★ when pinned, and its "you" is the card's owner. v42: a stored pull is
KEPT WHOLE where the store kept its tiers — every boss kill, timed key and
pinned pull (`Retention::keeps_whole`, never demoted or evicted) — so its
drill stacks by ability, Enter opens an ability (its curve, its targets,
their stack: `GetFight.spell`) and `v` compares (`GetFight.pair`, answered
as `StoredFight.pair` and fed to its state as a `CompareSnapshot`), each
byte for byte the live answer over the same seconds (`tests/series.rs`).
What the store keeps no answer for is refused with a toast rather than
asked (gui-logic's `toast::stored_refusal`, over the last answer's
`history::Kept`, everything offered before the first answer —
`Stored::offered`): a new pin where it kept the rows alone (`v` always
lets a pin go), an ability opened where it kept no ability's targets (a
pre-v42 file whose log is gone, `StoredFight::abilities`), and the
enemies' view anywhere; a drill the store kept no breakdown for says so in
the inspector. `p` pins or lets go the stage's
stored card (window-local; a log pull's too once the rail holds its card).
`Hist::owner` holds whose window it is — the character the store's newest
card was played on, as Home's answers (else the rail's pages) name them;
never Home's scope.

**Fight header and ribbon.** The window's meter wears ONE fight header
(`window/fight_head.rs` over gui-logic's `fight_head` words, window-only —
the overlay keeps its instance strip): a title line (led by the rail's list
button where the rail is a drawer; the encounter in Marcellus, difficulty and
size, the outcome badge, the duration, ‹ older / › newer pull buttons) and a
stat line of the view's raid figures as label/value pairs with full commas,
ending in the owner's "you" chip (their place in their ROLE on Damage, the
rate or count elsewhere; "died 5:45" on Deaths; a press selects their row,
and a filter that hid them gives way, its field's text with it) — with
"Deaths 6" on the Damage and Enemies lines and "First 1:10", "Battle rezzes
2" on Deaths, all from the snapshot's raid timeline — and, on a keystone
run's Σ, "Run dps" after the raid rate (the fold over the key timer; every
row rates over combat time, as the game's meter does) and "In combat 26:50"
last (v40, gui-logic's `fight_head::RunClocks`). Wide, the stat line is
one line; narrower, it wraps. The owner is the row the DAEMON marked `mine`
(v35, `Gui::owner_of`: its owner resolution — the addon's own characters,
every card's owner, the configured names — so an alt is "you" too and the
window matches no names; the character played last, then
`history_characters`, only when nothing is marked). While a view's answer is
on its way (`ClientState::view_answered`, a loading placeholder) the stat
line says nothing rather than a zero. Between the stat line and the tabs the
**ribbon** (`window/ribbon.rs`, one canvas, 86 px, 74 narrow, its arithmetic
gui-logic's `ribbon`) draws the raid timeline (R25): the view's raid rate in
10 s steps (finer in a short pull) as an ink area under a Catmull-Rom line,
"Raid dps, peak 10.7M", minute ticks, the lust as a faint wash (its name in
the tooltip while the pointer is in it — a word on the ribbon hid the crest
or was struck out by the curve), a skull per death in the class colour on a
hairline (`death_line`; "you" over the owner's; an arena enemy's in
outline), labels set on plates where they fit, an accent crosshair that
glows, with a dot where it meets the curve, and a tooltip on hover; the
pointer is read through the canvas's painted bounds, and a press on a skull
is `Gui::open_death` → `ClientState::open_death` (opt-in: the Deaths view
drilled into that death window, pushed in a narrow window). A stored pull
wears the store's rebuild of its timeline (R25 STORED: a 1 s series from the
details tier, the coarse 10 s one, or none once the details are demoted — the
axis, the lust and the deaths alone); only a card-only answer has no ribbon.
The ribbon and the inspector's graph span ONE axis, gui-logic's
`ribbon::fight_span`: a pull's `duration_ms` (its wall clock) or further
where its data runs; a visit's Σ spans what it holds alone, since its
`duration_ms` is no wall clock (a raid's Σ of members' combat, a key's
timer with its death penalties — 35:24 on a run that ended 31:32 in), and
falls back to that clock only with no series to measure. The daemon builds the timeline only for the client KINDS that use it
(`engine::wants_raid`: `Window` and `Mcp`) — so the overlay's Σ-split
connection, which is a `Window` kind, is sent one it never reads (a known
waste, pixel-neutral to fix with a kind or a flag). The view tabs follow
(`window/tabs.rs`: line icons, the prototype's order and names; the active
one's underline glides to it), and the strip keeps its ACTIVE tab whole in
sight at any width (gui-logic's `reveal`, asked whenever the view or the
width changes; a wheel scrolls it, and its edges fade where it scrolls). The
chrome budget this bought is measured, not eyeballed: at 1440×900 the
featured raid's first meter row starts under 290 px down with 18 rows
showing (`window::tests::the_chrome_leaves_a_raid_its_rows`, 285.5 px, on
every `cargo test`).

**The meter** (`window/table.rs`). Every list of `Row`s is drawn from ONE
column model (gui-logic's `table`): a column set (`table::meter_set` per view
— amount, rate, share and the view's own fourth column, crit / overheal /
absorbed; a count view's count and share; amount and rate at 820 px and under
— and the inspector's lists on `Grid::Abilities` / `Targets` / `Pair`), the
heading line over it and the total row under it come from the same list, so
they cannot drift; the live meter stands on the prototype's own grid per view
(`table::Grid::Meter`: `.v-num4` / `.v-enemy` / `.v-count` widths at a 12 px
gap). Every numeric heading sorts (desc → asc → the daemon's order,
`Gui::sort`); sorting and filtering change what is DRAWN and never what a
row's numbers mean (each row keeps the daemon's index, so ranks, shares, the
bar's scale and click targets hold, and `j`/`k` walk the drawn order through
`table::meter_step`); only the live meter's total follows a filter, the drawn
rows' fold with no share, as the prototype's does. The fight's workspace —
header, tabs, headings, rows, total — runs edge to edge; a short list takes
its own height and the total follows its last row, a long one scrolls and
the total pins under it. The list follows its selection past the fold: a key
step, the "you" chip and an opened death set `Gui::reveal`, and the list's
next layout scrolls the least that shows the WHOLE row, bar included
(`ScrollHandle::scroll_to_item`). The owner's row wears a "you" tag; a row's
bar eases to its new length (280 ms) and brightens when selected. The row
filter is a compact field at the end of the view tabs, widening from 92 to
140 px while it has the keys. On the Deaths view the meter is the deaths IN
THE ORDER THEY HAPPENED (`window/deaths.rs`, its words gui-logic's `deaths`:
time, player, killing blow then ONE quieter run "source, rezzed m:ss" that
gives way before the blow loses a letter, hit, overkill in red; j/k walk
them, each step that death's recap, and beside the inspector the table always
holds the keys); the count table stands only where no timeline came (a
card-only stored pull). The meter and the Deaths table wear the window's
scrollbar (`crate::scrollbar`, iced's thumb on a clear lane), never Kit's
self-hiding one.

**Inspector** (`window/inspector.rs`, window-only; the prototype's `.insp`):
the window's drill is master and detail — beside the meter above 820 px (520
px, 410 px in a 821–1180 tile — the most of the prototype's `minmax`, which
the grid always gives it), pushed over the whole stage with a back button at
820 px and under. `inspector/model.rs` builds `Insp`, owned data, once a
frame from the fight's `ClientState` and the window's `InspState`; `view.rs`,
`list.rs` and `recap.rs` draw it, and three components take their seats: the
graph (`plot.rs`), the stack matrix (`matrix.rs`) and the death chips
(`chips.rs`). It FOLLOWS the selection through `ClientState`'s opt-in
follow-selection (`set_follow`, which the window turns on and the TUI never
does): every move (`select_row`, j/k) re-watches `Cursor::Segment` with the
selected row as the drill, so one snapshot carries rows and breakdown; the
drill is keyed by guid so a re-sorting snapshot moves the highlight with the
player, an answered view with nobody in it drops the drill, and a filter that
hides the selection moves it to the first drawn row. Enter hands the keys to
the inspector (`inspecting`, what a narrow window draws as the push) where
j/k walk its list and Enter opens the ability inside it. It shows the player
(34 px disc, name, "Spec Class, you, died m:ss"), the view's numbers four or
two across, the actions (Compare `v`, Talents and gear `t`, Death recap, Per
second/Cumulative `g`), the graph and its LANES, then the lists. The graph
is one canvas painted in the iced window's layer order from gui-logic's
`inspect::geometry` — every constant, the axis, span rectangles and hit
tests, hover snapping, the drag window, the tooltip's words and placement,
the deaths' rules, the ticks and lane labels with their tooltip culling, curve points
and stacked bands, the clamped Catmull-Rom spline (the prototype's
`smooth()`) — while the component brings only a text measure (shaped at the
zoomed size, measured back at zoom 1) and its paint (`plot/{curves, deaths,
lanes, tip}.rs`): the curve as an area, the rate in the prototype's 10 s
buckets (finer only for a stretch too short to hold forty), minute ticks over
the fight's span, a death as two dashed rules — red where they died, green
where they were alive again (no green while `open`: they never came back) —
with the stretch between left clear and the words in a band the curves peak
under (deaths placed before returns, so a return gives way), the plot
starting at the lanes' track only when there are lanes — the gutter that
leaves is its scale, the peak and 0. Its gestures are window mouse
listeners from paint, so a drag scrubs past the canvas, held to the plot: a drag selects a zoom window (gold edges and the
window's words while in flight), a right press resets it, and a zoom glides
to its new window (220 ms). The LANES under it (rows 19.6 px apart:
cooldowns, items, externals, defensives) draw each span in its CASTER's class
colour via gui-logic's `inspect::Roster`, which also names a drill's target
rows — they wear the drilled player's class on the wire; the lanes take every
mark, NOT `view_draws_mark`'s per-view set, since a lane is its own row and
never washes the curve. The lists (`inspector/list.rs`, behind two tabs — Tab
walks them): pets dimmed after the ability, the pair ending in ONE ellipsis;
a 2 px class bar; the keys' line an accent edge kept in sight (the inspector
scrolls as a whole and its lists sit deep inside, out of
`scroll_to_item`'s reach, so a probe on the keyed line, `list::Keep`, reads
its bounds after layout and scrolls the least that shows it); the ability
list is the throughput table — amount, share, hits, average, crit. On Damage
and Healing that list is R26's TREE (gui-logic's `tree`, the lines the list
draws and the keys walk): groups of two or more and rows with parts fold,
shut until opened (`InspState::tree_open`, session-wide), a group of one
drawn as its row with the pet's or trinket's name after it, a part led by
what it is ("Direct", "Over time"); j/k walk LINES (the window keeps the line
in `InspState::tree_cursor` when it is a group or a part — `ClientState`
still thinks in row indices), Enter on a group folds it, → ← open and shut a
fold or climb to the line that holds it while the keys are in the list
(swallowed there, like a recap's), and the opened ability's numbers gain
Casts, Avg cast, Miss and Uptime. Over it the graph STACKS (gui-logic's
`inspect::stack`): a band per series in six hues validated for the surface by
the data-viz checks (no ochre, no red), stacked in slot order with a neutral
"Other" on top, a colour seated once per key (`InspState::stack_slots`) so a
live re-sort repaints nobody, the list's bars wearing the hues (the list is
the legend) and the tooltip a swatch per band; the switch beside Per second
("By ability" / "By target", "Total" pressed) is `InspState::stack_graph`.
Taken adds R17's mitigation line and keeps the R21 ledger behind a third tab,
"Hit by | Attackers | Stacks" (standing only where a ledger makes a matrix),
its matrices in the list's place (`inspector/matrix.rs`, the derivation
gui-logic's `inspect::matrix`): one matrix per debuff, level 0 derived PER
DEBUFF (the baseline less that debuff's own cells — exact under overlap,
empty rather than invented without a baseline), each cell on a faint wash of
its heat, its tooltip saying how many hits its average is over. The Deaths
view's inspector is the recap as the prototype's `recapPanel` draws it
(`inspector/recap.rs`: oldest first to the tinted killing blow; a time column
from each entry's `offset_ms`, the signed change, the event with its source
quiet after it — "yours" for the owner's own — and the health after it as a
6 px bar, amber under 15 %, red under 3 %, a dash where none was reported;
an insight line after the list when the player hurt themselves) and wears a
chip per death window (`inspector/chips.rs`); clicks, and ← → while the keys
are in the inspector (on the meter they step pulls), select through
`ClientState::select_death` (proto: `death` rides the Watch, reset with the
drill/view; `drill_breakdown` / `deaths` / `drill_stacks` are the
accessors), and the attacker list words its amounts as damage; the Enemies
view's is its numbers straight onto the attackers. `v` pins the selection and
the next move makes the pair: the inspector overlays both curves on one plot
and one scale (the second dashed when one colour would draw both — a shared
class — as the prototype's `graphBlock`), both players' lanes (each lane
split, the first's spans over the second's), their numbers, a legend and the
two ability lists (side by side in a wide window, stacked in a tile or a
narrow one), the meter still beside it (its rows as they stood — the
comparison's cursor carries none, so a view switch mid-pair re-asks for the
meter before the pair re-forms, and a live pull's meter says "paused while
comparing"; the pinned row wears a gold-dim "A" and its partner a "B", and a
pin says so in a toast until the pair forms). A move of the selection drops
the breakdown in hand; the window holds the last player's body
(`inspector::model::Held`, re-taken on every snapshot —
`ClientState::snapshot_gen`) and draws it dimmed until the next one's lands.
Enter on a pair beside the meter does nothing; Tab and `g` in a narrow window
push the inspector they change. A drag across the graph (v38) scopes the
lists to the window where the daemon keeps a clock for them
(`View::windows_drill` / `windows_targets`: Damage's abilities and targets,
Healing's abilities, the enemies' attackers): the window rides
`Cursor::Segment.range`, the rows come back windowed (`Segment::spells_in`,
`damage_targets_in`; the tree keeps its groups alone, `SpellTree::windowed`),
the list's heading names the window ("Ability, 0:42–1:13") or says it kept
the whole fight ("Target, whole fight"), and the graph's top line gives the
window's rate. A STORED pull windows the same way when its fight keeps the
history store's series tier (v39: kills, keys and pinned fights while their
details last, a boss kill's and a timed key's all season since v42 —
`StoredFight::series`, which turns its state's
`set_drill_windows` on, and `GetFight.range`); any other's zoom stays the
graph's and its lists say "whole pull". WIDENED (the corner button at
the head's right, `f`; `InspState::wide`, session-wide, nothing in a narrow
window): the inspector takes the whole stage under the view tabs, the meter
set aside while j/k still walk its players (`Seat::Wide`,
`inspector::model::Layout::Wide`, its rules gui-logic's `inspect::wide`): the
head on one line with the actions beside the name, the numbers in one line,
the graph 180 px tall, and the panes the tabs would switch side by side 3:2
on a stage of 1100 px or more (else one over the other) — a drill's
abilities beside its targets ("Hit by" heads Taken's), a recap beside its
attackers, R21's matrices under them rather than behind a tab — every list
with room for the rate and the view's last word (`inspect::list::Room::Wide`;
a rate no row was given is dropped), a pair's halves with rate, hits and
crit. Tab walks the two lists; Esc narrows it before Home.

**Home** (`window/home.rs`, its panels in `home/panels.rs` and its charts in
`home/charts.rs`; the derivations gui-logic's `home` and `home::chart`;
window-local like the talent viewer): the prototype's `.home`, "You, this
week", derived client-side from `HistoryQuery::Fights` answers — each card's
"me" is the scoped character's row, or scoped to all of them its owner's —
read one page in flight at a time until the week (the seven days back from
the store's newest card) is in hand, never more than `MAX_PAGES`. It opens at
launch when nothing is live (`home_on_start`) and stands aside when a pull
starts; `~` or the top bar's Home reaches it, and keys on Home never reach
the meter under it. Scope is a chip row — All characters, then each character
the window knows you play, with a class dot — never a lock: a chip (or a pick
of the picker's menu, which opens Home scoped) is remembered as config
`character` (`Config::store_character`), the scope Home opens on and the
character the picker names, and nothing else — whose window it is, the chrome
and the "you" follow the character played last. The night card ("Last night you played", "Tonight…" for
tonight's) is the scope's newest night: the place (a raid and its difficulty
from its visit's Σ card, else a Σ on its map, else — the daemon stores a
visit's Σ only once it closes — the tailed log's own visit,
`rail::log_instances`, else "Heroic raid", never a placeholder name; "Mythic+
keys"), the date, the character, a tile per pull (outcome glyph, "at 56%"
after a wipe, "17th of 19, 149.3k" — the place among the ROLE by effective
dps, a healer's by hps, `home::standing`), and a rank slope chart of each
pull's percentile in the role. Under it a grid of panels (the prototype's
330 px minimum, three wide, one at 820 px and under, widths from the window's
width less the docked rail): keys against their timers (par bars with ticks
at +3, +2 and par, "+2" or "over"), one panel per raid and difficulty (per
boss "Killed in 7:02" or "Best 2%" — never an unobserved 0 % — and a dot per
pull, filled on a kill, ringed in the character's colour on a wipe) and
effective dps on keys (a dot per run in its character's colour, each
character's best of the week ringed in legendary orange) — always those
three, in that order, each in its own words when its week is empty, so the
dashboard keeps its shape; never more columns than panels, a grid row's
panels stretched to one height. Every tile, row and dot is a jump point into
its pull, and each chart dot is an element of its own, the size of its press
radius, whose tooltip names the pull ("Ula'tek, 10th of 18"). It shows no
season score, no `N / 8` boss denominator and no ladder percentile because no
card carries them, and words a disabled store, a cold store and a degraded
one (the daemon's `Status`) apart rather than showing the same confident
nothing for all three — off or not answered yet, the night card ("Your
week") carries the state's words and no panel is drawn — refreshing `Status`
on open and on a debounced store change, as the daemon never broadcasts it.
Config keys: `season_start` / `season_end` (UTC `YYYY-MM-DD`, hand-parsed —
no chrono; Home reads nothing before the start; the old `season_label`
round-trips untouched and draws nothing), `density`, `home_on_start`,
`character` (Home's scope).

**Command palette, cards, filter and Esc.** The command palette
(`window/palette.rs`, its model gui-logic's `palette`; window-local; Ctrl K or
the jump box) is a card over a scrim: its field holds the focus in a
`Palette` context, so the meter's keymap is silent under it, and `Palette >
Input` binds the arrows, Ctrl N / Ctrl P, Esc and Ctrl K. It searches the
rail's pulls (the newest four before anything is typed — a live one, a boss,
a key, never the trash between), the players of the pull on the stage (a
count view's or the enemies' pull still lists who fought, from
`fight_head::Seen`, and running one switches to Damage and selects them by
key through `ClientState::select_player`, opt-in, the TUI never calls it),
the views with their key (`keys::key_for`) and the screens (Home, the live
pull, the earlier nights, the sheet, the talent viewer, and — once a word is
typed — Home's scopes), title and words folded together through `fold`;
Enter runs, a press runs a line, one on the card's heading holds and one on
the scrim closes. The selection is the one line lit (RAISE with the accent's
2 px edge, the rail's current-pull mark); the pointer lights none, as the
prototype's `.pal-it` has no hover. The other cards (`window/cards.rs`: the
`?` sheet, the ⚙ card, the character menu and the toast) are MODAL for the
keys: while one is up the root leaves the `Meter` context for `Modal`, where
any key closes the menu and does nothing else, the zoom chords still zoom and
Ctrl K replaces it with the palette; one menu at a time. For the pointer the
sheet hangs over an occluding scrim a press anywhere on closes, the character
menu over a clear one, and the ⚙ card closes when the pointer leaves it; it
writes each choice alone through `Config::store`. A card enters with a fade
and a 6 px rise (160 ms). The toast (`cards/toast.rs`, its words gui-logic's
`toast`) is the window's own, not Kit's `Notification`: centred at the
stage's foot, brief, taking no pointer. Esc walks one level up: the palette,
the talent viewer and the menus answer their own Esc first, then the rail's
drawer, then on the stage the filter's text, the inspector's ability or keys
(a narrow window's push), the comparison, a widened inspector, and Home,
where the chain ends.
`/` focuses the row filter; its Esc (`Filter > Input`) clears it and gives
the keys back, Enter keeps the text and gives them back. The filter narrows
what is drawn by label, class, spec or role name (`Class::name` /
`Spec::name` / `Role::name`, case-insensitive substring, accent-folded
through `gui-logic/src/fold.rs` so "akanos" finds Akanôs and the accented
spelling still works — Latin-1 and Latin Extended-A only, non-Latin scripts
deliberately untransliterated), and never renumbers: a filtered row keeps its
rank, its share and the index a click sends back — and `j`/`k` step over what
it hides, so the highlight is always on a drawn row. It is a PLAYER filter and
stops at the player list: the inspector's lists are never narrowed by it, and
it is drawn wherever the meter is — beside the inspector, under a comparison,
never over a narrow window's pushed inspector. The palette and the row filter
are the window's themed `Field` (`window/field.rs`): Kit's styled `Input`
paints every placeholder in the theme's one `muted_foreground` and forces
`text_sm` at every zoom, so `Field` composes Kit's unstyled gpui-base `Input`
on the same `InputState` and projects the window's tokens onto its editor
style each render (placeholder in ink 3's grade, caret, selection, ink),
leaving caret, IME, scrolling and clipping Kit's; the talent viewer's framed
import field stays Kit's styled `Input`.

**Chrome and type.** gui-logic's `theme` is the one definition: a `Def`
(`NAVY`, `ONYX`, `FROST`, each in its own file under `theme/`; config
`theme`; an unknown name reads the default, `onyx`, and the old `gold`
reads `navy`) holds the
window's `WindowTokens`, the overlay's `OverlayTokens`, the talent viewer's
`TalentTokens`, the data hues (`DataTokens`: the stacked bands, the
lettered squares, the foe, the timeline marks), the `Faces`, the type scale
(`Sizes`), the row pitches (`Pitches`), the corners (`Shape`: every radius
times `scale`, a pill capped at `chip`, read through `w.r` / `w.pill`,
`ov.r`, `p.r`, `pen.r`), the `Bars` (how much class colour a bar shows), the
`Effects` and the `Shadows`. Every token group
is written by `theme::tokens!`, so each field is a config key by the same
name: `[themes.<name>.window|overlay|talents|data|faces|size|pitch|shape|bars|
effects]`, plus `base`, `label`, `accent_label` and `dark` — `theme::Registry`
lays a config's tables over the built-ins (a table named for one overrides
it; any other is a theme of the user's own, from its `base`, the default when
unsaid), owns what it builds — a theme's words are `theme::Text`, a
literal or a string shared from the config; the built-ins are lazily built
statics; the active `Look` holds its `Def` by an `Arc`, so a theme switched
away from is freed — and words every
mistake (`did you mean "accent"?`) for stderr and the ⚙ card. The window
switches theme from the ⚙ card (`Gui::set_theme`, one key through
`Config::store`, the chrome kept); the overlay polls the config's mtime once
a second and follows (`Overlay::take_theme`). **Onyx** is a black-dial
chronograph: true black, lume ink, a grey ramp, white chrome (its chrome
chip says "White"), Saira Tabular (Saira at width 80, the measure of
Barlow Semi Condensed) and Michroma, corners at
0.3 and squared chips, and the four `Effects` `navy` leaves off — `glass`
(what floats — the cards, the palette, the sheet, the toast, the tooltips,
the overlay panel — is the `glass` fill under a sheen with an inset
specular rim: `W::float` / `.floating`, `paint::paint_float` on canvases,
`Ov::glass`, `Paint::tip_face`), `brackets` (reticle corners round the
ribbon and the inspector's graph), `dial` (`window/instruments.rs`: the
inspector's crest in a 60-tick bezel, the player's meter bar wrapped round
it in their class colour, to a hand) and `fine_ticks` (the ribbon's 10 s chapter ring). Two more
switches restyle what navy fills: `quiet_press` (a pressed action — "Stop
comparing" — is a raised key with an accent hairline and a lit bar along its
foot, not a block of the accent) and the data token `stack_other_edge` (the
stack's rest, `Ink::StackRest`, drawn as graphite under a steel line: the
player's whole curve). No colour of navy's is baked anywhere else: a class
chrome that knows no class yet wears the theme's own accent (the old
`NEUTRAL` blue is gone), and the graph samples take a theme's hues
(`samples::all_in`).
`crates/gui/src/theme.rs` `apply`s it to two targets: Kit's `Theme`, slot by
slot (the chrome goes to `primary` and `ring`, the prototype's raise to
`accent` — Kit's slot names are shadcn's), and the `Look` global every
bespoke surface reads, so `apply` with another definition repaints
everything (a test samples a Kit component and a bespoke surface across a
switch). No surface draws a literal colour: a new colour is a token on every
`Def`. `navy` wears the prototype's Tokens — ground, surface, raise,
line, edge, three inks, accent / label ink / accent ink (its gold, gold-dim and
gold-ink), good, bad, legendary (a
personal best, and nothing else), hover, name-lit, the tick on a checked box
— under one rule: gold is the interface, class colours are people, green and
red are outcomes, and no colour is semantic yellow (a live pull is a red dot
and its word; crit is ink). The chrome is `theme::Chrome`, config `chrome =
"theme"` (the default, the theme's own accent; the old `"gold"` reads
as it) or `"class"`, a plain string like `density` so a typo reads the
theme's. A class chrome wears the OWNER's class, learned once from the
owner's row and held (`Gui::learn_class`) — rows resort on every snapshot, so
tinting from the selection would re-colour the whole window on its own — and
is right on the first frame because the window writes that class whenever it
learns it (`character_class`, one key through `Config::store_character_class`)
and `main.rs` reads it before the first frame. The accent is drawn only as an
underline under the active tab or place, a pressed chip's edge and a
selection's edge — never a fill; `theme::chrome_base` may move a class colour
along its own hue until ink on it clears WCAG AA (Shaman blue is the one that
moves), and `Accent`'s light/dark ink split is WCAG's crossover luminance so
all thirteen class colours stay legible. A class colour as TEXT is lifted
toward white until it clears AA on the surface (`theme::class_text_on`); a
BAR keeps `Class::rgb` exactly, because the bar is data. Every bar in every
list is a narrow bar UNDER the row's text, the text on the panel in its own
ink, so no name or number ever sits on its class colour: the meter's 3 px
(`window/table.rs`), the inspector's 2 px (`inspector/list.rs`), the
overlay's 3z (`overlay/rows.rs`). The window's TYPE is bundled: Barlow Semi
Condensed 400/500/600 with its tabular figures baked into the default digits
(the family renamed "Barlow Semi Condensed Tabular" so an installed
proportional copy is never the face picked) for names and numbers alike, and
Marcellus for encounter titles and the wordmark alone — OFL files under
`crates/gui-logic/fonts/` (provenance, pinned upstream commits and the
reproducible fonttools bake in its `README.md`), `include_bytes!`d as
gui-logic's `fonts::FONTS` and registered with GPUI's text system in
`main.rs` — every built-in's faces, whatever the theme; `navy`'s overlay
draws in the system UI face and a monospace for its numbers
(`overlay/ov.rs`), `onyx`'s in Saira, whose digits are baked tabular the
same way. Fonts are assets, not dependencies.
Sizes are the `Def`'s `Sizes` (the Tokens specimens: encounter 27, 22
narrow; names 15; every figure 14.5; column heads and stat labels 13.5 in
the label ink; a theme with a wider title face sets its titles smaller) and `Pitches` (a meter row 32, the top bar 44, by `density`); no
overlay code names either. The window speaks the prototype's words (gui-logic's
`labels`): sentence case everywhere (`labels::sentence` turns the source's
"KILL" into "Kill"), the views in the prototype's order and names
(`labels::WINDOW_VIEWS`, `window_view_name`; `View::ALL` and
`fmt::view_name` stay the TUI's and the overlay's), `hide_realms` honoured in
every pane (`labels::realmless` strips only what reads as a player's
"Name-Realm-Region", since creatures' names hold hyphens). The line icons are
strokes, not SVG assets: `window/paint.rs` strokes gui-logic's `glyph` table
(GPUI exports no round caps, so each segment is its own sub-path with a disc
of the stroke's width at every end), and `glyph_ink` paints in its parent's
text colour so an icon brightens with its control's hover. A label ends in
"…" through GPUI's own `text_ellipsis` / `truncate`. Two GPUI traps shape the
window's pixels: GPUI draws a border OUTSIDE the padding (iced drew it
inside), so a card or a button pads by the design value less its edge
(`cards::BORDER`); and its default line height is φ, so a piece that must
stand on the window's 1.3 (the matrix, the death chips) sets it itself.

**Talent viewer** (`t`, `talents.rs` and `talents/`, window-local; its logic
gui-logic's `talents`: the layout `model`, the editing state machine
`viewer`, the `geometry`). `t` opens it on the selected row and asks for the
logged loadout; the root drops its `Meter` context while the viewer holds
the window, and its keys live in a `Talents` context (Esc closes, Tab flips
the tab). It decodes in-game import strings through `proto::talents` against
the per-machine `talents.json` and draws the panes the way the game does:
class pane left, spec pane right (split at the posX midpoint), the picked
hero tree between them under its medallion + golden ring
(`gui-logic/src/talent_art.rs` reads `talent-art.bin` — pane background
paintings included; absent cache = plain panels). Node frames follow the
game's shapes — square = active ability (entryType 1), circle = passive,
octagon = choice with side carets — with gold borders, rank pills and lit
gold paths for taken talents; icons come shaped/desaturated from
`spell_icons::styled`. Each pane is ONE canvas painted in the game's layer
order (`talents/pane.rs`) — GPUI keeps paint order, images and paths
interleaved as painted (spike S8), where iced composited every image above
every path and needed stacked widgets — with the background painting a
cover-fit canvas under the trees, and the pointer mapped back through a hitbox
per pane. A pasted SimulationCraft addon export (`gui-logic/src/simc.rs`,
stdlib parser), or the export's file dropped on the viewer, also brings saved
loadouts (chips switch between them), equipped gear, bag items and
currencies (`talents/inventory.rs`); pastes persist per character under
`~/.local/share/wowdps/simc/`, so reopening the viewer on that player's
meter row restores their build. v19: opening on a row also sends
`GetLoadout` through `Session::request` (the row supplies name, spec id AND
guid); the daemon answers with the player's COMBATANT_INFO loadout — the
actual talents + equipped gear from the log — which wins over a stored paste
(`adopt_logged`: picks → `proto::talents::picks_to_selections` → `encode` →
the normal adopt path, so validation, "copy string" and the warnings pane all
just work; a "from combat log" marker shows, gear renders on the inventory
tab as honest `item {id}` rows in slot order, simc loadout chips stay one
click away, and logged builds are never persisted — the daemon re-answers on
every open). Its delights settle under reduced motion: a lit path grows from
its parent, a taken node sends out a gold ripple, an opening choice fans its
options out, a tooltip arrives with a fade and a rise, the trees scale to fit
a narrow window down to 70 % before they scroll, and "copy string" answers
"copied ✓" for 1.4 s. The ignored `real_dataset_draws_every_spec`
(`cargo test -p wowdps-gui real_dataset_draws_every_spec -- --ignored`)
opens and draws every spec of the real dataset and takes a root in each.

**Overlay** (`overlay.rs` opens the surface; `overlay/panel.rs` is the
`Overlay` entity — the collapsed tab and the panel's header, rows and footer,
with the options card and the view menu; `panel/surface.rs` its place on the
edge; `rows.rs`, `instance.rs`, `drill.rs`, `graph.rs`, `panel/compare.rs`;
`ov.rs` its zoom, palette, faces and small pieces). It draws what the iced
overlay drew — the same states, its palette (`OverlayTokens`), monospace
numbers and yellow — and its pixels are held by the render guard below. It is
a GPUI layer surface (`WindowKind::LayerShell`: namespace `wowdps`, the
overlay layer, `KeyboardInteractivity::None`, transparent), opened through
`cx.open_window`, never Kit's Root. It is single-instance
(`gui-logic/src/single.rs`): a new `--overlay` launch evicts the running one
via an unversioned takeover socket, so orphans can't stack surfaces or
respawn daemons. Its output is chosen before the app starts
(`overlay::choose_output`), because a layer surface never changes output:
`WOWDPS_OVERLAY_OUTPUT`, else config `monitor`, else under Hyprland with
`follow_game` the game's monitor (`hypr::game_monitor`: the game window's
workspace, then that workspace's monitor from the `workspaces` reply — so a
game parked off screen still resolves), else the compositor's choice. The
daemon spawns on the game PROCESS and the window maps seconds later under
Proton, so a daemon-spawned overlay — marked by the
`WOWDPS_OVERLAY_GAME_STARTING` env the supervisor sets — polls for up to 30 s
(`GAME_WINDOW_WAIT`) before choosing; a hand launch asks once. The display is
found by recomputing GPUI's UUIDv5 of the output name
(`gui_logic::output::output_uuid`), never by bounds, which are wrong at
fractional scale and under rotation. **The edge strip**: gpui-pre sets a
layer surface's margin only when it is created, so the surface spans its
edge's whole LENGTH (anchored to the edge and both neighbours), its
THICKNESS follows the state (`Window::resize`, sized in `render`, only a
change sent), the content sits at the configured offset inside it, and the
input region is exactly the content (`Window::set_input_region`), the rest of
the strip click-through (`overlay/strip.rs` is the arithmetic). The grip (the
tab, the header) tells a click from a drag beyond 5 px, a window-wide
listener follows a drag off the content, and the offset is remembered by
itself; under Hyprland a tab dropped near another edge recreates the surface
there (gui-logic's `surface::nearest_edge`), the new one opened before the
old one goes. It never asks for a zero length (layer-shell's "stretch" is a
viewport protocol error under GPUI). "Hidden" is a 1 px strip with an empty
input region: the daemon's `SetVisible` composed with `follow_game` (the
game's workspace on screen, `gui-logic/src/hypr.rs`; `game_match`). Closing
its last surface quits, and the supervisor takes it from there. It zooms by
hand (`ov.z(…)`), and a wheel over the header zooms. Inside an instance visit
the overlay anchors its frame on the *visit*: gui-logic's `timeline` groups
the segment list into blocks (a visit's Σ + members, or a stray segment —
the window's rail groups tonight by the same `timeline::blocks`) and
`overlay/instance.rs` draws the clickable Σ–①─②─③–⚑ strip; the footer ◀▶
steps whole blocks while the strip, its chip's ‹ › and a wheel over it scrub
members, a new pull re-pins Live (unless parked on the live visit's Σ), and
the footer Σ toggle (`overlay_split` in config) appends the visit's overall
rows through a second `Session`, made on first want by an `AuxMaker` (a
`Window`-kind `DaemonClient`, so `SetVisible` never reaches it). The overlay
has no keyboard, so the footer's view name carries both switch gestures:
left-click cycles (`View::next`), right-click opens a view menu card that
jumps straight to any view. A right press anywhere else backs out one level
(a comparison first, then an ability, then the drill), and on a graph it only
resets the zoom. Its drill (`overlay/drill.rs`) is still the screen it
replaces — and, on Damage and Healing, R26's rollups through gui-logic's
`tree::lines`, groups (a pet's abilities under its summon, a trinket's under
the item, a proc under its driver) shut until a press opens one
(`Overlay::tree_open`, session-only), a press on an ability still drilling
into it, and no (spell id, periodic) parts; every line of a grouped drill
keeps a caret and an art column, a drill with no groups stays the flat list
— and its graph (`overlay/graph.rs`) shares one time cursor across every
graph on the panel. Its lists wear the overlay's square scrollbar
(`crate::scrollbar`, a 10 px rail with a draggable thumb), and its staleness
radar sweeps on a 100 ms timer only while it shows (`RADAR_FRAME`). The
daemon sends an `Overlay` session no raid timeline (its Σ split's
`Window`-kind connection still gets one, unread — see the ribbon above). The
`WOWDPS_OVERLAY_*` debug aids live in `panel/autos.rs` (`docs/tracing.md`).

**The headless review loop** (`crates/gui/SHOTS.md` is the manual). The GUI
is reviewed without launching it: ignored tests open the real views in a
`HeadlessAppContext` over the daemon's mock and capture frames through GPUI's
headless renderer (a wgpu adapter: the GPU, or Mesa's lavapipe — CI and the
nix sandbox run none of them), motion reduced so every picture is the
settled pixels. `window::shots::window_shots` renders the window's 22 states
at the prototype's three sizes, each reached by the gestures a user makes,
over the fixture or `WOWDPS_SHOTS_LOG` (+ `_FIGHT`, `_OWNER`, a read-only
`_HISTORY` store, `_ONLY`); `overlay::panel::tests::overlay_shots` the
overlay's 20 states, each over its own fixture; `talents::shots::talent_shots`
the viewer; `window::inspector::plot::shots::inspector_plot_shots` the graph
over gui-logic's shared samples, the matrix and the chips
(`WOWDPS_SHOTS_DELIGHT=1` adds motion-on pictures); all write PNGs under
`WOWDPS_SHOTS_DIR` when it is set. The inputs a comparison uses — a real-log
night slice and a FROZEN copy of the history store — live under
`~/.local/share/wowdps/design-shots/`, never in the repository: they hold
real player names, and so do their shots.
`window::shots::the_chrome_budget_holds_on_the_log` turns the header's
acceptance into a number on a real log (first meter row ≤ 290 px down at
1440×900, 18 rows without a scroll), held on every `cargo test` by
`window::tests::the_chrome_leaves_a_raid_its_rows` over a synthetic 25-player
raid in the window's real fonts. `overlay::panel::tests::overlay_render_guard`
renders every overlay state WITHOUT the art caches (so the committed pictures
hold no extracted game art) and compares each with
`crates/gui/snapshots/overlay/<state>.png` within a TOLERANCE (`guard.rs`: a
channel may move by 3, 0.05 % of the pixels may differ — Mesa moves under the
weekly lock update, so hashes would churn); a missing picture or one no state
draws fails, `WOWDPS_GUARD_PNG=<dir>` saves the pictures, and an intended
overlay change re-blesses with `WOWDPS_BLESS=1` and commits the new PNGs. The
overlay draws in the machine's system face, so run the guard by name where it
was blessed, before merging anything that touches the overlay or gui-logic's
overlay models.

## Debugging

`docs/tracing.md` covers: the daemon-mode workflow (`wowdps status`, `wowdps stop`, `$XDG_STATE_HOME/wowdps/daemon.log`, cache location, source-conflict errors), overlay debug env vars (`WOWDPS_OVERLAY_DEBUG=1` input tracing, `WOWDPS_OVERLAY_OUTPUT`, `WOWDPS_OVERLAY_START_EXPANDED`, `WOWDPS_OVERLAY_AUTOTOGGLE` and the other AUTO* aids; `WAYLAND_DEBUG=1` for what the surface asks of the compositor), a headless Hyprland workflow for screenshotting/verifying the overlay without a real game (for the WINDOW's design, prefer the compositor-free shot tests of `crates/gui/SHOTS.md`), and the layer-surface constraints GPUI imposes (no runtime margin, hence the edge strip; never a zero length; Kit's Root kept off the surface; zoom by hand).

Also note: the game flushes combat-log writes in multi-minute bursts (anti-overlay countermeasure), so a "frozen" meter is usually just an unflushed buffer — the daemon's liveness verdict uses the game-process signal for exactly this reason; check the log file's mtime before debugging the tail path.
