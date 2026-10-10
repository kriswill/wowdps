# crates/daemon (wowdps-daemon)

One process owns tail → index → parse → meter → snapshots for every
client, plus the history store, the overlay supervisor and the addon.
Threads and channels, no async runtime. Module map and the store tier by
tier: `docs/OKF/crates/daemon.md`. Operations: `docs/tracing.md`.

## Rules

- **Tests never touch the live daemon.** The suites run real daemons on
  temp sockets through `DaemonOptions` (every path and grace injectable);
  never point one at the production socket. The live daemon is the
  `wowdps-dev` unit (root `AGENTS.md`, "The live daemon").
- **`wowdps stop` is the shutdown.** The daemon takes no signals; the unit's
  `ExecStop` runs it.
- **Liveness is observation plus the game process** (`game.rs`), never the
  log's mtime: the game flushes in multi-minute bursts.
- **Nothing closes on the game-process signal.** Closing is file-derived: a
  tailer that switches logs sends `HistoryReq::Retire` for the one it left.
- **The hub never parses history.** Historical parses run on the loader
  pool. The hub pushes changed-only at 10 Hz and answers a `Watch` at once;
  `SegmentId`s are monotonic for the daemon's life; at most 16 parsed
  segments stay in the LRU.
- **Build per-client extras only for the kinds that read them:** the raid
  timeline for `Window` and `Mcp` (`engine::wants_raid`), stacked series for
  `Window` (`engine::wants_series`).
- **One durability primitive.** Every file the daemon writes goes through
  `cache.rs`'s `write_atomic`. Index checkpoints are cached; parsed meters
  never are. The lockfile is taken before a stale socket is unlinked.

## The history store

- `Store<B: Backend>`: `DirBackend` in production, `MemBackend` for the
  mock and tests. Write store tests against `MemBackend`.
- **Retention runs after every write.** The protected set is kept whole:
  boss kills, timed keys, pinned fights and progression wipes until the
  first kill at that difficulty. The caps count the unprotected alone; never
  demote or evict a protected fight.
- **Stored equals live.** A stored drill, window, stack or comparison must
  equal the live answer over the same seconds (`tests/series.rs`): both go
  through one function (`series::window_rows`, one `answer`). Never fork a
  stored-only path.
- **Formats move forward from the logs.** A tier format change leaves old
  files readable; the rewrite queue (`Store::rewrites`, `Store::recuts`)
  rewrites them from their logs while idle, and never touches a newer
  format.
- **Affiliations are joined when answered,** never stored on a card.
- **A late replay is broadcast:** a replay that lands after its card sends
  `HistoryChanged`.
- **A ruling that moves stored numbers** needs `wowdps history regrade`
  after deploy (pins kept); say so in the PR.
- Config: the flat `history_*` keys in `config.rs`; a new key's default
  keeps today's behavior.

## The overlay supervisor

- It spawns `<gui_binary> --overlay` when the game appears and sets
  `WOWDPS_OVERLAY_GAME_STARTING`. Config `gui_binary` (default
  `wowdps-gui`) is resolved once by `Config::gui_bin`: a bare name is the
  daemon binary's sibling, else found on `$PATH`; a value with `/` is a path.
- `Status` never names the binary (that would be a wire change); a failed
  spawn's error does.
- A killed overlay is not respawned. Relaunch `wowdps-gui --overlay`; it is
  single-instance, so a new launch evicts an old one.

## The addon (`addon/`)

- Embedded in the daemon. `wowdps addon install` installs it, `wowdps addon`
  reports it. The daemon rewrites a stale copy on start and never installs a
  missing one.
- It writes raid members' guilds (which the combat log never carries) into
  its SavedVariables on logout; they lag a logout. The daemon reads them on
  start and on a 30 s idle poll.
- The TOC's interface number comes from the install's `.build.info`; never
  hardcode it.

## The mock

`mock.rs` is an in-process daemon over the real engine and a fixture,
driving `ClientState` synchronously; the GUI's and the TUI's tests run on
it. Keep it answering as the hub does.

```sh
cargo test -p wowdps-daemon
WOWDPS_REAL_LOG=/path/to/WoWCombatLog-*.txt cargo test --release -p wowdps-daemon -- --ignored real_log --nocapture
```
