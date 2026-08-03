# wowdps client/server rearchitecture

## Context

Today all three frontends (TUI, GUI window, overlay) each own the whole pipeline: they spawn their own `tail::spawn` reader, drain `TailEvent`s into a client-side `App`, and service segment loads synchronously from the filesystem. This means N processes tailing the same file, no shared state, and no way to auto-launch the overlay when the game starts. The goal: a single headless daemon owns log processing (tail + index + segment loading + wow.exe detection) and TUI/GUI/overlay become IPC clients — child processes of the daemon or launched by the user — opening the door to future clients (e.g. MCP).

**Decisions:**
- **Thin daemon**: daemon streams `TailEvent`s and serves segment loads (raw lines); each client keeps its own `App`/`Meter`, replaying lines via the existing parser. `app.rs`, `ui.rs`, `view.rs`, and all 23 TUI TestBackend tests stay unchanged; each client gets independent view/selection for free.
- **Wire format**: hand-rolled, zero-dependency, **binary** protocol (gRPC/protobuf-style length-prefixed frames), in a new crate. No serde in core (CONTRACT) or in the new crates.
- **Binary layout**: `wowdps` = daemon + launcher + TUI client; `wowdps-gui` stays a separate binary (iced/wayland deps isolated from the daemon build) and becomes a pure client for both window and `--overlay` modes.
- **Lifetime**: any active client connection keeps the daemon alive; it exits after the last client disconnects (grace period). `--linger` disables idle-exit. Ship a systemd user unit + flake/home-manager output.

## Crate layout

```
crates/core      wowdps-core    (deps: none — unchanged)   one tiny addition to tail.rs
crates/proto     wowdps-proto   (deps: wowdps-core only)   NEW: wire codec + client library
crates/daemon    wowdps-daemon  (deps: core, proto)        NEW: headless daemon
crates/tui       wowdps-tui, bin `wowdps` (adds proto, daemon deps)
crates/gui       wowdps-gui     (adds proto dep)           becomes an IPC client
```

Dependency direction: `core ← proto ← daemon`; tui depends on core+proto+daemon; gui on core+proto. Core stays zero-dep; proto and daemon are stdlib-only. proto depends on core so it encodes `TailEvent`/`Index`/`SegmentMeta` directly — no mirror types, no drift. Keep the crate named `wowdps-tui` (renaming is churn with no payoff).

New files:
- `crates/proto/src/lib.rs` — constants, re-exports
- `crates/proto/src/wire.rs` — primitive encode/decode
- `crates/proto/src/msg.rs` — `ClientMsg`/`DaemonMsg` + core-type codecs
- `crates/proto/src/client.rs` — `DaemonClient`, `Feed`, `ensure_daemon`, `socket_path`
- `crates/daemon/src/{lib,hub,server,game,overlay,config}.rs`
- `crates/daemon/tests/ipc.rs` — integration test against `crates/core/fixtures/sample.txt`
- `nix/home-manager.nix` — systemd user unit module

## Wire protocol (`wowdps-proto`)

Transport: Unix socket `$XDG_RUNTIME_DIR/wowdps/wowdps.sock` (fallback `/tmp/wowdps-<uid>/wowdps.sock`; dir 0700, verify ownership). `proto::client::socket_path()` is the single source of truth.

**Framing** — all integers fixed-width little-endian:

```
frame := u32 len | u8 tag | body     (len covers tag+body; MAX_FRAME = 64 MiB)
```

Primitives (`wire.rs`): u8/u16/u32/u64/i64 LE; bool as u8; string = u32 len + UTF-8; `Option<T>` = presence byte + T; `Vec<T>` = u32 count + items; `PathBuf` as string. Decode returns `Result<T, DecodeError>` (`UnexpectedEof | BadTag | BadBool | BadUtf8 | FrameTooLarge`) — never panics.

**Messages** (`msg.rs`), `PROTO_VERSION: u16 = 1`:

```rust
enum ClientMsg {                                    // tags 0x01..
    Hello { proto: u16, client: String, pid: u32 }, // client = "tui"/"gui"/"overlay"/"mcp"
    Subscribe,
    LoadSegment { req_id: u32, meta: SegmentMeta }, // meta, not index position (stateless loads)
    GetStatus { req_id: u32 },
    Shutdown,                                       // used by `wowdps --stop`
}
enum DaemonMsg {                                    // tags 0x81..
    HelloAck { proto: u16, version: String },
    Tail(TailEvent),                                // sub-tag: Lines/Switched/CaughtUp/Index/Waiting/Error
    LoadResult { req_id: u32, result: Result<Vec<String>, String> },
    Status { req_id: u32, game_running: bool, source: Option<PathBuf>, clients: u32, linger: bool },
    Fatal(String),                                  // sent before daemon closes the connection
}
```

Handshake: `Hello` → `HelloAck` on exact `PROTO_VERSION` match, else `Fatal("protocol vX vs vY — restart the daemon: wowdps --stop")` + close. Bump `PROTO_VERSION` whenever an encoded core type changes shape; a golden-bytes test forces the conscious bump.

Proto tests: roundtrip every variant (empty vecs, non-ASCII, `None`s); truncation fuzz (every strict prefix of every encoded message must `Err`, never panic); oversized-len and bad-tag rejection; golden bytes.

## CLI + `wowdps` dispatch

`core/src/cli.rs::parse_args` returns a struct (tests updated; `--file/--logs` semantics preserved):

```rust
pub struct Args {
    pub source: Option<SourceSpec>, // Some => explicit --file/--logs => embedded standalone mode
    pub daemon: bool, pub gui: bool, pub linger: bool, pub stop: bool, pub standalone: bool,
}
```

- `--file/--logs` ⇒ **embedded mode** — today's architecture, byte-for-byte. This keeps `cargo run --bin wowdps -- --file fixtures/sample.txt` and all fixture workflows working, and is the no-daemon escape hatch. `--standalone` = embedded with `DEFAULT_LOGS_DIR`.
- `wowdps --daemon [--linger]` → run daemon in foreground (systemd target; also what self-spawn launches).
- `wowdps` in a TTY → `ensure_daemon()` then run TUI as IPC client.
- `wowdps` **not** a TTY, no flags → `ensure_daemon()` (spawn detached if absent), print status, exit 0 — "when run it starts itself in the background".
- `wowdps --gui` → `ensure_daemon()`, spawn sibling `wowdps-gui` detached, exit 0.
- `wowdps --stop` → connect, send `Shutdown`.

`ensure_daemon(daemon_bin) -> io::Result<UnixStream>` (in `proto::client`): try connect; else `Command::new(daemon_bin).arg("--daemon")` detached (`process_group(0)`, null stdio — no libc fork), retry connect with backoff (≤3 s). `wowdps` passes `current_exe()`; `wowdps-gui` passes the sibling `wowdps` path (PATH fallback), so launching the GUI directly also works.

`gui/src/main.rs` keeps pre-stripping `--overlay`; `--file/--logs` ⇒ embedded, otherwise ⇒ daemon client; rejects daemon-only flags.

## Client library (`proto/src/client.rs`)

```rust
pub enum Feed {
    Embedded { rx: mpsc::Receiver<TailEvent> },  // wraps today's tail::spawn
    Daemon(DaemonClient),
}
impl Feed {
    fn embedded(spec: SourceSpec) -> Feed;
    fn connect(daemon_bin: &Path) -> io::Result<Feed>;   // ensure_daemon + Hello + Subscribe
    fn try_recv(&mut self) -> Result<TailEvent, TryRecvError>;  // drop-in for Receiver
    fn load(&mut self, meta: &SegmentMeta, source_path: &Path) -> Result<Vec<String>, String>;
    fn reconnect_if_dead(&mut self);            // called on tick; retry every ~2s, no respawn
}
```

`DaemonClient`: one connection; one reader thread demuxes frames — `Tail` → events mpsc, `LoadResult` → loads mpsc; EOF/error → synthetic `TailEvent::Error("daemon disconnected")`. Writes go over a `try_clone()`d writer half. `load()` is a synchronous round-trip (`recv_timeout(10s)`, req_id matched defensively — App has at most one `load_pending`, FIFO is already correct). Simplest correct answer to reentrancy: no second connection, no async.

**Frontend diffs (deliberately tiny):**
- `tui/src/main.rs`: `run(feed)`; drain loop calls `feed.try_recv()`; `service_loads` swaps `index::load_segment(&path, &meta)` for `feed.load(&meta, &path)` + existing `meter_from_lines` + `install_loaded`.
- `gui/src/window.rs`: `Gui.lines: Receiver<TailEvent>` → `Gui.feed: Feed`; `drain_tail`/`service_loads` keep their shapes (shared with the overlay — one edit covers both GUI modes).
- `gui/src/overlay.rs`: same substitution; `hypr.rs` untouched (workspace-following stays a display concern of the overlay client).
- Reconnect v1: status line (existing `TailEvent::Error` → `app.status`) + `reconnect_if_dead()` on tick; replay-on-connect resets the App via `Switched` (existing behavior).

`app.rs`, `ui.rs`, `view.rs`, `keys.rs`, `testkit.rs`, all TUI render tests: untouched.

## Daemon internals (`wowdps-daemon`) — threads + channels, no tokio

- **Hub thread** (`hub.rs`): owns a `Tailer` directly (not `tail::spawn` — its mpsc is single-consumer; we need fan-out) + client table `Vec<Client { id, subscribed, tx: SyncSender<DaemonMsg> }>`. Loop: `msg_rx.recv_timeout(200ms)`; on timeout/after each message, `tailer.poll()` → broadcast to subscribers. `HubMsg { Register, Request(ClientId, ClientMsg), Gone, Game(bool) }`.
- **Server** (`server.rs`): single instance — try connect to socket; success ⇒ "already running", exit; else unlink stale socket + bind. Per connection: reader thread (handshake, then forwards `ClientMsg`s to hub) + writer thread draining a `sync_channel(256)`. Backpressure: `try_send` full ⇒ drop the client (it reconnects and resyncs cheaply). Hub never blocks on a client.
- **Replay-on-connect** (on `Subscribe`): (a) drain tailer to EOF, broadcasting normally — pins "delivered" to end-of-file; (b) fresh `index::scan` of the current file (<1 s per perf gate); (c) send the new client `Switched(path)` → `Index { fresh, file_age_ms from mtime }` → one `Lines` batch read from `[fresh.live_offset, delivered_offset)` → `CaughtUp`; (d) mark subscribed. No line caching; fresh `file_age_ms` means a mid-fight joiner lands on the live meter via App's existing `ACTIVE_FILE_MS` logic. Requires one small core addition: `Tailer::current() -> Option<(&Path, u64 offset, usize buffered_partial)>` (tail.rs is prose-only in CONTRACT — free to extend); `delivered = offset - buffered`. If still `Waiting`, send `Waiting`.
- **Load servicing**: `LoadSegment{meta}` → `index::load_segment(current_path, &meta)` on the hub thread (same blocking cost frontends pay today; worker-offload is a noted future improvement) → `LoadResult` to that client only. Sending `SegmentMeta` (not a position) makes loads stateless and immune to index-sync races; rotation mid-load surfaces via the existing `app.load_failed`.
- **Game watcher** (`game.rs`): thread, every 3 s scans `/proc/[0-9]*/{comm,cmdline}` for case-insensitive substring `cfg.game_process` (default `"wow.exe"` — matches wine's `Z:\...\Wow.exe` cmdline); sends `HubMsg::Game(bool)` on transitions. Pure `game_running(pattern) -> bool` unit-tested against fake cmdlines.
- **Overlay child** (`overlay.rs`): on `Game(true)` + `auto_overlay` + no live child: spawn `wowdps-gui --overlay` (sibling of `current_exe()`, else PATH), null stdio; reap via `try_wait()` on hub ticks. On `Game(false)`: kill only the auto-spawned child; user-launched overlays are ordinary clients.
- **Idle-exit**: last subscriber gone ⇒ 10 s grace ⇒ exit 0 + unlink socket, unless `--linger` or (`game_running && auto_overlay` — the overlay client is imminent). `Shutdown` ⇒ immediate clean exit.
- `DaemonOptions { socket, source, linger, idle_grace, auto_overlay override }` — injectable for the integration test (temp socket, fixture file, 100 ms grace).

## Config

Extend the gui `Config` (serde, `#[serde(default)]` keeps old files valid): `auto_overlay: bool` (**default true** — the headline behavior; the key exists to turn it off), `game_process: String` (default `"wow.exe"`), `logs_dir: Option<String>`.

Daemon reads its three keys via a tiny hand-rolled toml-subset reader (`daemon/src/config.rs`, ~60 lines: strip comments, match `key = value` for bare bools and double-quoted strings, ignore the rest). Keeps the daemon stdlib-only; the gui's `toml` output is trivially within the subset. Tests: real gui-written file, missing file, garbage tolerance.

## systemd + flake

- `flake.nix`: `packages.${system}.default` builds **only the `wowdps` binary** (`cargoBuildFlags -p wowdps-tui` — pure Rust, no GUI native deps). `wowdps-gui` packaging (wayland build inputs + LD_LIBRARY_PATH wrapper) noted as follow-up; until then auto_overlay needs `wowdps-gui` on PATH.
- `homeManagerModules.default` (`nix/home-manager.nix`): `services.wowdps.enable` → package + `systemd.user.services.wowdps` (`ExecStart=… --daemon --linger`, `Restart=on-failure`, `WantedBy=default.target`). Darwin: guarded `launchd.agents` stub, documented untested.
- `devenv.nix`: no new packages needed (proto/daemon are stdlib); keep the sync note scoped to the dev shell.

## Implementation phases (each leaves `cargo test` green)

1. **proto crate** — workspace member, `wire.rs`/`msg.rs`/`socket_path`, full unit suite. Nothing else touched.
2. **daemon crate + core accessor** — `Tailer::current()` (+ test); `game.rs`, `config.rs`, `server.rs`, `hub.rs`, `run()`; `daemon/tests/ipc.rs`: handshake; Switched→Index→Lines→CaughtUp ordering; two concurrent clients; `LoadSegment` bytes == direct `load_segment`; live-append reaches both; idle-exit; `Shutdown`; stale-socket recovery. Binaries still on the old path.
3. **CLI + `wowdps` dispatch + TUI client** — `Args` rework (cli tests updated); `proto/src/client.rs`; `tui/src/main.rs` dispatch + `Feed`-based run loop. Embedded mode intact ⇒ existing workflows/tests pass.
4. **GUI clients** — `gui/src/main.rs` mode selection; `window.rs`/`overlay.rs` swap to `Feed`; reconnect-on-tick.
5. **auto_overlay + config keys** — Config fields, daemon subset reader, spawn/reap wiring, idle-exit nuance.
6. **packaging + docs** — flake package + hm module; update CLAUDE.md (commands/architecture), CONTRACT.md (rewrite stale tail/app prose; add pinned proto/daemon sections: PROTO_VERSION, frame layout, tags, ordering guarantee; extend dep policy "proto/daemon: stdlib only"), docs/tracing.md daemon-mode workflow.

## Verification

- `cargo test` — proto suite, daemon integration suite, game-matcher/config tests, plus existing 153 core + 23 TUI + 17 gui tests unchanged.
- Manual: run daemon on the fixture (`wowdps --daemon` with logs_dir override or `--file`), attach `wowdps` (TTY) + `nix develop -c cargo run --bin wowdps-gui` simultaneously — identical rows; kill daemon → clients show status → restart → clients resync; `wowdps --stop`; idle-exit after last client; headless-Hyprland overlay workflow from `docs/tracing.md` with the overlay as a daemon client.
- Perf gate: existing `WOWDPS_REAL_LOG=… --ignored real_log` unchanged; add an `--ignored` daemon test asserting time-to-Index-frame < 1 s on the real log and `LoadSegment` round-trip comparable to direct `load_segment`.

## Risks / gotchas

- Two tailers on one file (embedded client + daemon) is read-only-safe but shows independent meters — document that `--file/--logs` bypasses the daemon by design.
- Socket hygiene: 0700 dir, connect-test-then-unlink for stale sockets, ownership check on the /tmp fallback.
- Version skew: exact `PROTO_VERSION` handshake with a human-readable `Fatal`; golden-bytes test forces conscious bumps.
- Replay-on-connect correctness depends on draining the tailer to EOF before the fresh scan — encoded as an ordering assertion in the integration test.
- CONTRACT.md update lands with Phase 6; the new proto section becomes pinned surface.
