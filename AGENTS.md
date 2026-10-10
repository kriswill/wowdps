# AGENTS.md

How to work in this repository. Each area has its own `AGENTS.md`, loaded
when you touch that directory; descriptions of how things are built live in
the knowledge bundle (`docs/OKF/`) and the design docs it links.

## What wowdps is

A World of Warcraft combat-log damage meter with a client/server split. A
headless daemon owns the whole pipeline (tail → index → parse → meter →
snapshots, plus the history store of every closed fight). Every frontend
(the TUI, the GUI's window and overlay, the MCP server) is a pure rendering
client speaking a hand-rolled binary protocol over a unix socket.
`CONTRACT.md` is the binding spec: public signatures, the rulings, the wire
and the dependency policy.

## Map

- `crates/model/`: zero-dependency domain types. Read
  `crates/model/AGENTS.md` before changing a type.
- `crates/core/`: the engine (parser, meter, index, tail, replay cut,
  generated tables, fixtures). Read `crates/core/AGENTS.md` before changing
  parsing, metering, segmentation or a fixture.
- `crates/proto/`: the wire, `DaemonClient`, `ClientState`, the history
  codecs and binary tiers. Read `crates/proto/AGENTS.md` before changing a
  message, the client state or a stored format.
- `crates/daemon/`: hub, engine, loaders, history store, overlay
  supervisor, addon, config, mock. Read `crates/daemon/AGENTS.md` before
  changing the daemon, the store or `addon/`.
- `crates/tui/`: binary `wowdps` (dispatcher, launcher, TUI). Read
  `crates/tui/AGENTS.md` before changing the TUI or its keys.
- `crates/gui/`: binary `wowdps-gui`, the window and the `--overlay` on
  GPUI. Read `crates/gui/AGENTS.md` before any GUI change, and
  `crates/gui/src/window/AGENTS.md` before changing a window surface or
  the talent viewer.
- `crates/gui-logic/`: the GUI's framework-free half. Read
  `crates/gui-logic/AGENTS.md` before changing it.
- `crates/mcp/`: binary `wowdps-mcp` (`wowdps mcp`), fight data as MCP
  tools. Read `crates/mcp/AGENTS.md` before changing a tool.
- `crates/history/`: binary `wowdps-history` (`wowdps history`), DuckDB
  over the stored fights. Read `crates/history/AGENTS.md` before changing a
  view or a subcommand.
- `crates/encounter-rubric/`: the seasonal encounter rubric and the
  replay's art formats. Read `crates/encounter-rubric/AGENTS.md` before
  changing it.
- `tools/`: game-data generators, the extractor, the dev unit. Read
  `tools/AGENTS.md` before running or changing a tool.
- `nix/`, `flake.nix`, `devenv.nix`, `devenv.yaml`: the dev shells and
  packages. Read `nix/AGENTS.md` before changing them.
- `.github/`: CI. Read `.github/AGENTS.md` before changing a workflow.
- `docs/OKF/`: the knowledge bundle. Read `docs/OKF/AGENTS.md` before
  editing it.

## The live daemon

On a dev machine the live daemon is the `wowdps-dev` systemd user unit
(`tools/dev-unit.sh`). The user may be in a raid while you work.

- Never run `wowdps daemon --linger` by hand, and never rely on a
  self-spawned daemon: it idle-exits ~10 s after its last client and takes
  the overlay with it.
- Restart only with `systemctl --user restart wowdps-dev`, and ask first.
  A restart mid-pull drops the live meter.
- A build of the active profile's `wowdps` or of the configured overlay GUI
  restarts the daemon and overlay a few seconds later. Warn the user before
  such a build if they may be mid-pull. A stopped unit stays stopped.
- Never run `wowdps status` while the unit restarts: a one-shot client
  spawns its own daemon and races the unit.
- A PROTO_VERSION bump renames the socket. Deploy it with
  `docs/OKF/playbooks/bump-proto-version.md`.

## Rules

- **Contract.** A change to a public signature, a ruling, a fixture golden
  or the wire changes CONTRACT.md, the goldens and the code together. A
  wire-shape change bumps `PROTO_VERSION`.
- **Dependencies.** CONTRACT.md §Dependencies is the policy: model
  zero-dep; core, proto, daemon and mcp stdlib only; no chrono, no tokio, no
  serde outside gui, gui-logic and encounter-rubric. Anything new needs a
  sign-off.
- **No panics in production code.** Clippy denies `unwrap`, `expect`,
  indexing, `panic!` and friends (`Cargo.toml` lints); tests are exempt
  (`clippy.toml`).
- **Commits.** Follow @CC.md (Conventional Commits). A docs-only commit carries
  `[skip ci]`; the tag anywhere in a head commit's message skips PR CI, so
  never write it, even quoted, in a code commit. Stage explicit paths:
  another agent may share the checkout.
- **Knowledge bundle.** After adding a crate, ruling, fixture or generator,
  or making a decision worth a long commit body, update `docs/OKF/` in the
  same change (the `knowledge-bundle` skill); `okf validate` must exit 0.
- **No real data in the repo.** No real player names, guild names or
  Warcraft Logs report codes in fixtures, tests, docs or commits; inputs
  cut from real logs stay under `~/.local/share/wowdps/`. Extracted game
  art stays in per-machine caches.
- **Writing.** American English (color, center, gray). Plain and specific.
  Never name third-party replay tools or echo their UI words; describe prior
  art generically.
- **Worktrees.** Never share `CARGO_TARGET_DIR` between worktrees: cargo
  keys path dependencies by their workspace-relative path and picks up the
  other tree's stale crate.

## Toolchain and shell

Nightly, from `rust-toolchain.toml`; the tree must still build on stable,
so no `#![feature]`. Build in the dev shell (devenv, entered by its cd hook,
or `nix develop`): the GUI and `wowdps-history` link libraries only it
provides. In the shell `wowdps`, `wowdps-history`, `wowdps-mcp` and
`wowdps-gui` are wrappers that rebuild release from this checkout before
they run (`WOWDPS_NO_BUILD=1` skips the build).

## Commands

```sh
cargo test                          # the workspace: fixture parity, IPC suites
cargo test -p wowdps-core           # one crate
cargo test -p wowdps-core meter::   # tests matching a substring
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt
wowdps status                       # daemon state, overlay, history store
wowdps stop                         # stops a daemon; the live one is the unit's
cargo run --bin wowdps -- --file crates/core/fixtures/sample.txt
tools/dev-unit.sh status            # the unit's profile, overlay GUI, state
nix develop -c okf validate         # the bundle (okf is on the shell's PATH)
coderabbit review --agent           # alias cr; --base main reviews the branch
```

- `--file` refuses to attach to a daemon following another source. With
  the unit up, ask before stopping it to run a fixture.
- CodeRabbit: `.claude/settings.json`'s SessionStart hook puts the dev
  shell's `coderabbit` on PATH. Never `coderabbit update`: the store is
  read-only and the weekly lock update moves it. `/code-rabbit-review` is
  CodeRabbit's skill; `/code-review` is the plugin's.
- The CodeRabbit skills in `.claude/skills/` are vendored from upstream.
  Never edit them alone: a local fix is a patch in
  `.claude/skills/patches/`, and `tools/skills-update.sh` refreshes them and
  reapplies every patch (never `bunx skills update`).
- Debugging: `docs/tracing.md`. A frozen meter is usually the game's
  multi-minute log flush; check the log file's mtime before suspecting the
  tail.
