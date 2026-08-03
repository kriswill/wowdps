# wowdps client/server rearchitecture

## Context

Today all three frontends (TUI, GUI window, overlay) each own the whole pipeline: they
spawn their own `tail::spawn` reader, drain `TailEvent`s into a client-side `App`, and
service segment loads synchronously from the filesystem. N processes tail the same file,
parse it independently, share nothing, and nothing can auto-launch the overlay when the
game starts.

The goal: a single headless daemon owns everything from bytes to rendered rows — tail,
index, parse, meter, `wow.exe` detection, overlay lifecycle — and TUI/GUI/overlay become
**pure rendering clients**. Clients never open the log, never parse a line, and never
link the parser. They receive domain data and turn it into pixels.

**Decisions:**

- **Fat daemon.** The daemon owns the `Meter`. The wire carries `Row`/`ListRow`/segment
  metadata — the view model — not log lines. Clients hold selection, screen, and
  drilldown position only. Parse cost drops from N× to 1×; the overlay stops needing a
  parser at all.
- **Query + snapshot protocol**, not a stream replay. A client declares what it is
  looking at (`Watch`, a cursor: the segment list, or a segment+view optionally drilled
  into one actor) and the daemon pushes snapshots for exactly that — including the
  drilldown and the list, so a drilldown open on a live fight and a list row's ticking
  duration update at the same cadence as the meter rows. Snapshots are idempotent, so a
  lagging client is caught up by dropping stale ones — never by dropping the client.
- **Wire format**: hand-rolled, zero-dependency, binary, length-prefixed frames, in a new
  crate. No serde in `model`/`proto`/`daemon`.
- **Socket is version-namespaced** (`wowdps-v<PROTO_VERSION>.sock`). Version skew is
  structurally impossible rather than diagnosed: a new client never sees an old daemon's
  socket, spawns its own, and the old one idle-exits. No handshake-mismatch recovery
  path to get wrong.
- **No embedded mode.** Clients cannot tail. If `ensure_daemon` fails, the client exits
  with an error — no fallback pipeline duplicated across three frontends.
- **The daemon is a session service.** It owns the overlay end to end: launches it on
  `wow.exe`, hides it when the game exits, relaunches/reveals on return, decays to a
  clean exit if the game stays gone.
- **Binary layout**: `wowdps` = daemon + launcher + TUI client; `wowdps-gui` stays a
  separate binary (iced/wayland deps isolated from the daemon build), a pure client for
  both window and `--overlay` modes.
- **Long-term storage is out of scope.** No Parquet, no DuckDB, no analytics store. The
  only persistence is a compact binary cache of the structural index (below), which
  exists to skip rescans — not to become an event store. The advanced-analytics GUI and
  its storage layer are a separate, later project.

## Crate layout

```
crates/model     wowdps-model    (deps: none)              NEW: domain types, zero-dep
crates/core      wowdps-core     (deps: model)             parser/meter/index/tail — engine
crates/proto     wowdps-proto    (deps: model)             NEW: wire codec + client library
crates/daemon    wowdps-daemon   (deps: model, core, proto) NEW: headless daemon
crates/tui       wowdps-tui, bin `wowdps` (model, proto, daemon)
crates/gui       wowdps-gui      (deps: model, proto)      pure client
```

`wowdps-model` is the split that makes "clients don't parse" enforceable by the compiler
rather than by discipline: `View`, `Row`, `Class`, `SegmentKind`, `SegmentId`,
`SegmentInfo`, `ListRow`, `Screen`, `Pane`, `Drill`, `Action`. It has no I/O and no
parser. `wowdps-core` keeps `parser`/`meter`/`index`/`tail` and re-exports model types so
its internals are unchanged; `model.rs` stops being a re-export shim and becomes the
crate boundary it always described itself as.

`proto` depends on **model only**, never on core. `wowdps-gui` therefore cannot parse a
log line even by accident.

The one compromise: the `wowdps` binary is both daemon and TUI client, so it links core
transitively. Enforce by convention plus a test — nothing under `crates/tui/src/` may
`use wowdps_core::{parser, meter, index, tail}`; a source grep in the tui test suite
keeps that honest.

`SegmentMeta`'s `byte_range` and `seeds` stay daemon-side and never reach the wire. That
also removes the quadratic seed-vector cost the raw-line design would have paid on every
connect (seeds accumulate monotonically across a session; index.rs:174-191).

New files:

- `crates/model/src/lib.rs` — domain types moved out of `meter.rs`/`index.rs`/`app.rs`
- `crates/proto/src/{lib,wire,msg,client}.rs`
- `crates/proto/src/mock.rs` (feature `mock`) — in-process fake daemon for client tests
- `crates/daemon/src/{lib,hub,server,session,loader,game,overlay,cache,config}.rs`
- `crates/daemon/tests/ipc.rs` — integration suite against `crates/core/fixtures/sample.txt`
- `nix/home-manager.nix` — systemd user unit module

## Wire protocol (`wowdps-proto`)

Transport: Unix socket `$XDG_RUNTIME_DIR/wowdps/wowdps-v<PROTO_VERSION>.sock` (fallback
`/tmp/wowdps-<uid>/`; dir 0700, ownership verified). `proto::client::socket_path()` is the
single source of truth and embeds `PROTO_VERSION`.

**Framing** — all integers fixed-width little-endian:

```
frame := u32 len | u8 tag | body     (len covers tag+body; MAX_FRAME = 16 MiB)
```

Primitives (`wire.rs`): u8/u16/u32/u64/i64/f64 LE; bool as u8; string = u32 len + UTF-8;
`Option<T>` = presence byte + T; `Vec<T>` = u32 count + items. Decode returns
`Result<T, DecodeError>` (`UnexpectedEof | BadTag | BadBool | BadUtf8 | FrameTooLarge`) —
never panics.

16 MiB is generous for every message the protocol can produce: snapshots are bounded by
`top_n`, breakdowns by one actor's spell list, segment lists by a session's segment
count (hundreds of small rows on a long night). Nothing
unbounded crosses the wire, which is the point of the query model. A message that would
exceed the cap is a bug, not a condition to handle at runtime — assert it in tests.

**Messages** (`msg.rs`), `PROTO_VERSION: u16 = 1`:

```rust
enum ClientMsg {                                   // tags 0x01..
    Hello { proto: u16, client: ClientKind, pid: u32 },   // Tui | Window | Overlay | Mcp
    /// The client's cursor: what it is currently rendering. Replaces any prior Watch.
    Watch(Cursor),
    GetStatus { req_id: u32 },
    /// Overlay only: the user hid/showed it locally, so the daemon's supervisor agrees.
    VisibilityChanged { visible: bool },
    Shutdown,                                      // `wowdps --stop`
}

/// What a client is rendering. Everything a screen shows is covered by a
/// cursor, so everything a screen shows stays live-updating under push.
enum Cursor {
    /// The segment list screen: the daemon pushes `SegmentList` snapshots
    /// (durations tick, segments open/close, live flags flip).
    List,
    /// A segment's meter, optionally drilled into one actor. `drill` set means
    /// snapshots carry the breakdown too — a drilldown open on a live fight
    /// updates at the same cadence as the rows behind it, never frozen at
    /// open time.
    Segment { segment: SegmentRef, view: View, top_n: Option<u32>, drill: Option<String> },
}

enum DaemonMsg {                                   // tags 0x81..
    HelloAck { proto: u16, version: String },
    Snapshot {
        seq: u64,
        segment: SegmentRef,          // echoes the cursor this answers
        view: View,
        info: SegmentInfo,            // kind, name, start_ms, duration_ms, success, live
        rows: Vec<Row>,
        total_rows: u32,              // > rows.len() when top_n truncated
        breakdown: Option<Breakdown>, // by_spell/by_target; present iff the cursor drills
        segment_count: u32,
        source: Option<String>,       // log file name, for the header
        status: Option<String>,       // daemon-side error/notice for the footer
    },
    /// Pushed to `Cursor::List` watchers: the full list, oldest first. Bounded
    /// in practice (a long raid night is a few hundred small rows); coalesced
    /// by `seq` like snapshots. Each row carries its stable `SegmentId` — the
    /// id "open this row" turns into a `Cursor::Segment`.
    SegmentList { seq: u64, entries: Vec<ListEntry>, source: Option<String> },
    /// A new segment just opened on live combat — the client decides whether to snap.
    SegmentOpened { id: SegmentId },
    LoadFailed { segment: SegmentId, error: LoadError },  // NotFound | Rotated | Io(String)
    Status { req_id: u32, game_running: bool, source: Option<PathBuf>, clients: u32,
             linger: bool, overlay: OverlayState },
    /// Overlay lifecycle command from the supervisor.
    SetVisible(bool),
    Fatal(String),
}

struct Breakdown { by_spell: Vec<Row>, by_target: Vec<Row> }

enum SegmentRef { Live, Id(SegmentId) }            // stable id, never an index position
```

`SegmentRef::Id` is a stable `SegmentId`, monotonic for the **daemon's lifetime** and
never reused — not per file. Across a rotation a stale id resolves to
`LoadFailed(NotFound | Rotated)` by construction, never to another file's fight — the
failure mode the raw-byte-range design had. Clients additionally treat a changed
`source` on any snapshot as "the log rotated": drop cached snapshots and selections and
re-`Watch`. There is no explicit reset message; the source echo is the signal.

Handshake: `Hello` → `HelloAck`. Because the socket path carries the version, a mismatch
means a hand-rolled client got the path wrong; `Fatal` + close is sufficient. `Shutdown`
is accepted pre-handshake so `--stop` always works.

Proto tests: roundtrip every variant (empty vecs, non-ASCII, `None`s, f64 edge values);
truncation fuzz (every strict prefix of every encoded message must `Err`, never panic);
oversized-len and bad-tag rejection; golden bytes forcing a conscious `PROTO_VERSION`
bump whenever an encoded type changes shape.

## Client library (`proto/src/client.rs`)

```rust
pub struct DaemonClient { /* one connection, reader thread, writer half */ }

impl DaemonClient {
    pub fn connect(daemon_bin: &Path) -> io::Result<Self>;  // ensure_daemon + Hello
    pub fn watch(&mut self, cursor: Cursor);    // replaces any prior cursor
    pub fn poll(&mut self) -> Vec<DaemonMsg>;   // non-blocking drain, stale snapshots coalesced
    pub fn reconnect_if_dead(&mut self) -> bool;
}
```

The reader thread demuxes frames into a mutex-guarded inbox. **Snapshot coalescing lives
here**: only the newest `Snapshot` per `(segment, view)` and the newest `SegmentList`
are retained, so a client that misses ticks catches up instead of falling behind.
Everything else queues in order. A snapshot that doesn't match the current cursor
(sent before a `Watch` change landed) is discarded on arrival.

`ensure_daemon(daemon_bin, source_override) -> io::Result<UnixStream>`: try connect; else
`Command::new(daemon_bin).arg("--daemon")` detached (`process_group(0)`, null stdio),
forwarding `--file`/`--logs` when given, then retry connect with backoff (≤3 s). Failure
is fatal to the client — there is no embedded fallback. `wowdps` passes `current_exe()`;
`wowdps-gui` passes its sibling `wowdps` (PATH fallback).

### Client-side state (`ClientState`)

Everything the current `App` holds that is *not* derived from the log stays client-side:
`screen`, `view`, `row_sel`, `list_sel`, `drill`, `follow_live`, plus **the last snapshot,
cached for local clamping**. `Action` handling and `keys.rs` are unchanged.

`ClientState` deliberately exposes the same accessor surface `ui.rs` and `view.rs`
already call — `rows()`, `list_rows()`, `breakdown()`, `segment_name()`,
`segment_success()`, `duration_ms()`, `segment_count()`, `segment_index()`,
`following_live()`, `is_live()`, `list_selection()`, `screen`, `view`, `drill`, `row_sel`,
`source`, `status` — so the render code is nearly untouched despite the state moving. The
diff lands in `apply()`, which now emits requests alongside local updates.

Round-trip cost: a `d`→`h` view change or a drilldown open is one unix-socket round trip
(sub-millisecond) — each is just a new `Watch` cursor. Held-key repeat over `j`/`k`
never round-trips — it clamps against the cached snapshot. Opening a drilldown renders
immediately from the cached rows and fills the panes when the drilled snapshot lands.

## Daemon internals (`wowdps-daemon`) — threads + channels, no tokio

- **Tail thread**: owns a `Tailer` (not `tail::spawn` — its mpsc is single-consumer),
  polls at `POLL_INTERVAL`, feeds lines straight into the engine. This is the only thing
  in the system that opens the log.
- **Engine** (hub-owned): the live `Meter`, the current `Index`, and an LRU of loaded
  historical segment `Meter`s. Assigns `SegmentId`s. Everything `app.rs` used to do with
  `index`/`loaded`/`load_pending`/`maybe_snap_live` lives here, once, for all clients.
- **Hub thread** (`hub.rs`): owns the engine and the session table
  `Vec<Session { id, kind, cursor, tx: SyncSender<DaemonMsg> }>`. Loop:
  `msg_rx.recv_timeout(100ms)`; on tick, rebuild snapshots for **watched cursors only**
  — `Cursor::List` watchers get `SegmentList`, `Cursor::Segment` watchers get
  `Snapshot` (with the breakdown when drilled) — and push changed ones. Snapshot rate
  capped at 10 Hz. A new segment opening on live
  combat emits `SegmentOpened`; the client decides whether to snap (preserving today's
  "backing out to the list mid-fight sticks" behavior, which is a per-client preference).
- **Loader workers** (`loader.rs`): a small pool (1–2 threads) servicing historical
  segment parses. **Loads never run on the hub thread** — one client browsing history
  must not freeze the live overlay. Results return via `HubMsg::Loaded`, and the
  requesting client's next snapshot carries the data.
- **Server** (`server.rs`): single instance via a lockfile held for the daemon's
  lifetime (`std::fs::File::try_lock`, stable since 1.89; workspace is on 1.97), acquired
  *before* unlinking a stale socket and binding — this closes the connect-test-then-bind
  TOCTOU that lets two racing daemons orphan each other. Per connection: reader thread
  (handshake, then forwards `ClientMsg`s) + writer thread draining a `sync_channel(64)`.
  Backpressure drops only stale snapshots, never the client.
- **Late joiners**: no replay, no backfill, no offset arithmetic. A joiner sends `Watch`
  and gets the current snapshot. The whole `live_offset`/`delivered`/overlap-race class of
  bug the raw-line design carried simply does not exist here, and `Tailer::current()` is
  no longer needed in core.
- **Liveness**: "is combat live now" is answered by the daemon from what it has actually
  observed (last line arrival, open segment, `game_running`), not by the client guessing
  from file mtime. This matters because the game flushes its log in multi-minute bursts —
  an mtime-based `ACTIVE_FILE_MS` check would put a freshly auto-launched overlay on the
  segment list in the middle of a pull.
- **Game watcher** (`game.rs`): thread, every 3 s scans `/proc/[0-9]*/{comm,cmdline}` for
  case-insensitive substring `cfg.game_process` (default `"wow.exe"` — matches wine's
  `Z:\...\Wow.exe`); sends transitions to the hub. Pure `game_running(pattern) -> bool`
  unit-tested against fake cmdlines. Linux-only, and said so.
- `DaemonOptions { socket, source, linger, idle_grace, overlay overrides }` — injectable
  for tests (temp socket, fixture file, short graces).

### Overlay supervisor (`overlay.rs`)

The daemon owns the overlay's whole life. Layer-shell has no unmap, so "hidden" is a 1×1
click-through surface the overlay process maintains — the daemon cannot hide it, only ask
via `SetVisible`.

```
                 game appears                    game appears
   Absent ──────────────────────► Visible ◄──────────────── Hidden
      ▲                              │                        │
      │      exit_grace elapsed      │ game exits             │
      └──────────────────────────────┴───────────────────────►┘
```

- `Game(true)` + `auto_overlay` + no overlay session ⇒ spawn `wowdps-gui --overlay`.
  If an overlay session already exists (identified by `Hello.client == Overlay`, which is
  what that field is *for*), send `SetVisible(true)` instead — never spawn a second one
  over a user-launched overlay.
- `Game(false)` ⇒ `SetVisible(false)`, start `overlay_exit_grace`. Game returns before it
  elapses ⇒ `SetVisible(true)`, cancel. Grace elapses ⇒ terminate the child and reap.
- A manual hide by the user (`VisibilityChanged{false}`) sticks until the next `Game`
  *transition*, so the daemon never fights the user mid-session.
- Spawn failures are captured and surfaced: the child's stderr is retained and reported
  through `Status`/`OverlayState`, because a Wayland client launched from a systemd user
  unit with null stdio and no `WAYLAND_DISPLAY`/`LD_LIBRARY_PATH` fails silently
  otherwise — the most likely real-world breakage on NixOS. The daemon itself logs to
  `$XDG_STATE_HOME/wowdps/daemon.log` for the same reason.

### Lifetime

The daemon is a **session service** — systemd user unit or Hyprland `exec-once`. That is
what makes `auto_overlay` work at all: detecting the *next* game launch requires a
process that outlives the last one.

- `--linger` (what the unit uses) disables idle-exit entirely.
- Otherwise: last session gone ⇒ `idle_grace` (10 s) ⇒ exit 0 + unlink socket + release
  lock, unless the overlay supervisor is mid-`exit_grace`.
- `Shutdown` ⇒ immediate clean exit.
- A `GetStatus`-only client (a statusline poller) does **not** hold the daemon open;
  only a `Watch`ing session or a live overlay child counts.

### Index cache (`cache.rs`)

The one piece of persistence in scope. A compact binary serialization (reusing
`wire.rs`, no new format) of the structural index at
`$XDG_CACHE_HOME/wowdps/index/<hash-of-path>.bin`, keyed by `(dev, ino, size, mtime)`.

- On daemon start, load the cache for the current log and **rescan only the tail**
  (`[cached.scanned, EOF)`), instead of the full 300 MB scan.
- Per-file entries, newest kept first, older evicted by count — so "load most recent
  first, older on demand" falls out of the cache layout.
- Invalidation is the identity key: any mismatch is a full rescan. Truncation and
  rotation already force a rescan today.

Explicitly **not** cached: parsed segment `Meter`s. They are derivable from the log in
milliseconds via `load_segment`, and serializing per-actor hashmaps is how a cache turns
into an event store by accident. If we later want durable history, that is the Parquet
project, not this file.

## Config

Config moves out of the gui crate into `daemon/src/config.rs` and becomes the single
file both sides read (`~/.config/wowdps/config.toml`, unchanged path, `#[serde(default)]`
semantics preserved so existing files stay valid). The daemon reads it with a hand-rolled
toml-subset reader (~80 lines: track `[section]` headers, match `key = value` for bare
bools, ints, floats and double-quoted strings, ignore the rest), keeping the daemon
stdlib-only; the gui keeps writing it with `toml`.

New keys:

| key | default | owner |
| --- | --- | --- |
| `logs_dir` | `DEFAULT_LOGS_DIR` | daemon — **the source of truth for what to tail** |
| `game_process` | `"wow.exe"` | daemon |
| `auto_overlay` | `true` | daemon |
| `overlay_exit_grace_secs` | `180` | daemon |
| existing gui keys (`edge`, `offset`, `width`, `height`, `zoom`, `monitor`, `follow_game`, `game_match`) | unchanged | gui |

Section-aware parsing matters: a naive `key = value` matcher would happily read
`game_process` out of a future `[overlay]` table. Tests: real gui-written file, missing
file, sectioned file, garbage tolerance.

The daemon reads config at startup; changing it requires a restart. A control message to
re-read or re-target the source is a deliberate follow-up (`ClientMsg::SetSource`), noted
here so the message space leaves room for it.

## CLI + `wowdps` dispatch

`core/src/cli.rs::parse_args` returns a struct (tests updated):

```rust
pub struct Args {
    pub source: Option<SourceSpec>,  // daemon-only override of config `logs_dir`
    pub daemon: bool, pub gui: bool, pub overlay: bool,
    pub linger: bool, pub stop: bool, pub status: bool,
}
```

- `wowdps --daemon [--linger] [--file F | --logs D]` → run the daemon in the foreground
  (systemd target; also what self-spawn launches). **`--file`/`--logs` configure the
  daemon's source**, overriding config `logs_dir`. They no longer mean "embedded mode",
  because there is no embedded mode.
- `wowdps [--file F | --logs D]` → `ensure_daemon()` forwarding the source, then run the
  TUI client. If a daemon is already running against a *different* source, that is a hard
  error naming both paths and suggesting `wowdps --stop` — not a silent surprise.
  This keeps `cargo run --bin wowdps -- --file crates/core/fixtures/sample.txt` a
  one-liner, which is the whole fixture workflow.
- `wowdps --gui` → `ensure_daemon()`, spawn sibling `wowdps-gui` detached, exit 0.
- `wowdps --status` → connect, `GetStatus`, print, exit. `--stop` → `Shutdown`, exit 0
  even if no daemon was running.
- No TTY-sniffing. The old "not a TTY ⇒ start in the background" behavior is now
  `--daemon`/`--status`, which are testable and unsurprising.

`gui/src/main.rs` keeps pre-stripping `--overlay`; it accepts no source flags and rejects
daemon-only flags — the GUI cannot choose what to tail because it cannot tail.

## Testing

The engine tests (153 core) are untouched: `parser`, `meter`, `index`, `tail` and the
fixture-parity invariants all still run against the same code, now inside `wowdps-core`.

The render tests are the ones that move. `testkit.rs` currently builds `App`s by replaying
`fixtures/sample.txt` through the real parser/meter. It becomes `proto::mock` (feature
`mock`): an **in-process fake daemon** that runs the *real* engine over the fixture and
serves the *real* protocol messages into a `DaemonClient` over a channel pair instead of
a socket. The 23 TUI TestBackend tests and the 17 gui tests keep asserting against real
parsed fixture data — they lose nothing — while exercising the client-side state machine
and the snapshot path they will actually run in production.

`daemon/tests/ipc.rs` (real socket, temp path, injected `DaemonOptions`):

- handshake; version-namespaced socket path
- `Watch(Live)` → snapshot rows byte-identical to a direct `Meter` replay of the fixture
- two concurrent clients on *different* cursors, each getting only its own snapshots
- live append reaches both; snapshot `seq` monotonic per cursor
- stale-snapshot coalescing: a stalled reader receives the newest, not a backlog
- a drilled cursor's breakdown matches `Segment::breakdown` directly, and a live append
  refreshes it (the frozen-drilldown regression test)
- `Cursor::List` snapshots match the combined segment list; a segment close and a
  duration tick each push a fresh one
- `SegmentId`s survive rotation without reuse: a stale id after rotation is
  `LoadFailed`, never another file's segment
- a slow load does not stall the live cursor (the loader-worker guarantee)
- `SegmentOpened` fires once per opened segment
- idle-exit, `Shutdown`, lockfile contention (second daemon exits, first survives),
  stale-socket recovery
- overlay supervisor state machine against a stubbed process spawner and a fake game
  watcher: spawn, hide, regrace, reveal, decay-exit, and "don't spawn over an existing
  overlay session"
- index cache: cold scan, warm tail-only rescan, identity-mismatch full rescan

Perf gates (`--ignored`, `WOWDPS_REAL_LOG`): existing `real_log` unchanged; add
time-to-first-snapshot < 1 s on a real log (cold), and < 100 ms warm from the index cache.

## Implementation phases (each leaves `cargo test` green)

1. **`wowdps-model` extraction** — move domain types out of `meter`/`index`/`app`; core
   re-exports so no engine code changes. Pure refactor, no behavior change.
2. **proto crate** — `wire.rs`/`msg.rs`/`socket_path()`, full unit suite incl. truncation
   fuzz and golden bytes. Nothing else touched.
3. **daemon: engine + hub + server** — engine (tail thread, meter, index, loader workers,
   `SegmentId`s), `hub.rs`, `server.rs` with the lockfile, `config.rs`, `game.rs`,
   `cache.rs`, `run()`. `daemon/tests/ipc.rs` minus the overlay cases. Binaries still on
   the old path; the daemon is exercised only by tests.
4. **client library + `ClientState`** — `proto/src/client.rs`, `proto/src/mock.rs`,
   `ClientState` with the `App`-compatible accessor surface, snapshot coalescing.
5. **TUI client** — `Args` rework, dispatch, `main.rs` run loop against `DaemonClient`;
   re-point the 23 render tests at `proto::mock`. `ui.rs`/`keys.rs` diffs stay minimal.
6. **GUI clients** — `gui/src/main.rs` mode selection; `window.rs`/`overlay.rs` swap to
   `DaemonClient`; `hypr.rs` untouched (workspace-following stays a display concern);
   reconnect-on-tick.
7. **Overlay supervisor + game watcher wiring** — `SetVisible`, the state machine,
   graces, spawn-failure surfacing, idle-exit interaction.
8. **Packaging + docs** — flake package (`-p wowdps-tui`, pure Rust, no GUI native deps)
   + `homeManagerModules.default` with the systemd user unit
   (`ExecStart=… --daemon --linger`, `Restart=on-failure`, `WantedBy=default.target`);
   update CLAUDE.md (commands/architecture), CONTRACT.md (rewrite stale `tail`/`app`
   prose; pin proto surface: `PROTO_VERSION`, frame layout, tags, ordering guarantees;
   extend dep policy — "model: zero-dep; proto/daemon: stdlib only"), and
   `docs/tracing.md` with a daemon-mode workflow.

## Risks / gotchas

- **The protocol is now the view model**, so every new column or view is a protocol
  change. Mitigated by shipping daemon and clients as one release and by the versioned
  socket; do not promise wire stability until an out-of-tree client (MCP) exists.
- **The daemon is now the thing that can OOM.** It holds the live meter plus the union of
  every client's historical segments, where `LOADED_CAP = 8` previously bounded each
  frontend independently. Needs an explicit LRU ceiling and a test that N clients
  browsing N different segments stays bounded.
- **Auto-launched overlay on NixOS**: needs `WAYLAND_DISPLAY`, `XDG_RUNTIME_DIR` and the
  `LD_LIBRARY_PATH` wrapper in the systemd user environment, and `wowdps-gui` on the
  *systemd* PATH, not the shell's. Packaging `wowdps-gui` in the flake is a follow-up;
  until then this is the documented sharp edge, and spawn failure must be visible in
  `--status`.
- **Client responsiveness now depends on the snapshot cache.** If `ClientState` ever
  round-trips on key repeat, held `j`/`k` will stutter. Worth a render test that asserts
  no request is emitted for `Up`/`Down`.
- **`--stop` and systemd** — a clean exit 0 will not trip `Restart=on-failure`, which is
  intended; `systemctl --user restart wowdps` is how you bring it back.
- **CONTRACT.md is stale from phase 1 onward** (it still says "single binary crate") and
  is only corrected in phase 8. Accepted, but the model extraction in phase 1 should note
  the pending rewrite so the gap is deliberate.

## Implementation notes (as built — deviations from the plan above)

- **`proto::mock` became `daemon::mock`.** The mock runs the real engine, which lives in
  the daemon crate; proto depending on daemon would be a cycle. Client render tests
  dev-depend on `wowdps-daemon` — dev-deps don't touch the shipped `wowdps-gui`, whose
  prod deps stay model + proto only.
- **`SegmentList` rows are `ListEntry { id, row }`.** A client opens a list row by id;
  rows without ids would have forced positional cursors back in.
- **`Snapshot` carries `id: Option<SegmentId>`** (the id a `Live` cursor resolved to) and
  **`SegmentList` carries `active: bool`** (the daemon's combat-liveness verdict:
  open segment + fresh lines *or* game process running — the mtime heuristic's
  replacement). The first anchors client-side `[`/`]` navigation; the second drives the
  jump-straight-to-live-meter startup.
- **The index cache keys on content, not `(size, mtime)`.** mtime changes on every
  append, which would have made the cache miss exactly when it matters (daemon restart
  mid-session). Instead core's scanner exposes a resumable checkpoint
  (`Index::checkpoint`/`scan_from`, parity-gated), and the cache stores it keyed by
  `(dev, ino)` plus an FNV checksum of the 64 KiB before the checkpoint offset; any
  mismatch (truncation, rewrite, rotation) is a full rescan.
- **`core::app` is gone**, not just bypassed: the engine half lives in
  `daemon::engine`, the UI half in `proto::state::ClientState`, `meter_from_lines`
  moved to `core::meter`, and `testkit` shrank to the fixture path + lines.
- The gui parses its own two flags (`--overlay`, `--help`) instead of using core's
  `parse_args` — it no longer links core at all.
