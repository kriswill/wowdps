# crates/daemon (wowdps-daemon)

One process owns tail → index → parse → meter → snapshots for every
client, plus the history store, the overlay supervisor and the addon.
Threads and channels, no async runtime. Operations: `docs/tracing.md`.

## Rules

- **Tests never touch the live daemon.** The suites run real daemons on
  temp sockets through `DaemonOptions` (every path and grace injectable);
  never point one at the production socket. The live daemon is the
  `wowdps-dev` unit (root `AGENTS.md`, "The live daemon").
- **`wowdps stop` is the shutdown.** The daemon takes no signals; the unit's
  `ExecStop` runs it.
- **Liveness is observation plus the game process** (`game.rs`), never the
  log's mtime: the game flushes in multi-minute bursts. A process is the
  game by its program alone (comm or argv[0], never a later argument: a
  shell or editor that mentions `wow.exe` is not the game). The watcher
  names each running edition by the product folder in its argv[0]; the
  overlay is shown while any runs.
- **Nothing closes on the game-process signal.** Closing is file-derived: a
  tailer that switches logs sends `HistoryReq::Retire` for the one it left.
- **The hub never parses history.** Historical parses run on the loader
  pool. The hub pushes changed-only at 10 Hz and answers a `Watch` at once;
  `SegmentId`s are monotonic for the daemon's life, from one counter
  (`engine::Ids`) every engine shares; at most 16 parsed segments stay in
  each engine's LRU.
- **One source and one engine per edition** (`sources.rs`, CONTRACT.md's
  editions paragraph). Every answer goes through `Sources`: an id to the
  engine holding it, `Live` and the list to the session's edition's, the
  live edition by its rule there. A session no source serves acts on
  nothing. The live edition is one rule, `Sources::choose_live` (its
  words are in `sources.rs` and CONTRACT.md): the started source whose log
  shows the newest line time (`Engine::line_ms`), held by its open pull or
  a move under `HANDOFF_QUIET`, the watcher naming one game overriding,
  modified time for a source with no timed line. It is chosen on the tick
  and on game reports, never on a tail event. Keep it one argmax: a new
  reason to move belongs in the rule's words first. Tests stand in for the
  clock with `Sources::age_for_test` and time their lines on one clock.
  Over the hub's one-source scenario a daemon of one source queues the
  wire bytes it queued before editions were followed:
  `one_source_frames_are_the_daemon_s_before_editions` holds them to
  `tests/golden/one_source_frames.txt`, made by the daemon before. Re-bless
  it (`WOWDPS_BLESS=1`, which rewrites it and fails) only for a change every
  Retail client should see, and say so in the commit.
  Discovery follows `wowdps_core::cli::FOLLOWED` (Retail and Forever):
  a source is named by its folder only once a second one holds a log, so
  an install with no Forever log is held to the same golden
  (`one_source_scenario` over `two_editions()`). Tests name other
  editions' folders through `DaemonOptions::editions`.
- **Build per-client extras only for the kinds that read them:** the raid
  timeline for `Window` and `Mcp` (`engine::wants_raid`), stacked series for
  `Window` (`engine::wants_series`).
- **One durability primitive.** Every file the daemon writes goes through
  `cache.rs`'s `write_atomic`. Index checkpoints are cached, the newest
  eight logs' per edition (each entry wears its log's mtime, so scan order
  never decides); parsed meters never are. The lockfile is taken before a stale socket is unlinked.

## The history store

- `Store<B: Backend>`: `DirBackend` in production, `MemBackend` for the
  mock and tests. Write store tests against `MemBackend`.
- **A start reads only what is new** (`swept.rs`; CONTRACT.md, the
  persistence paragraph). The manifest beside the index checkpoints
  records each older log read to its end, once every import it queued has
  landed with no load or write failing; the start-up sweep passes over it
  while its inode, length and mtime hold, every fight it held is still a
  card, and the store's `store.id` and the rules are the manifest's.
  Never record a tailed log (its open tail is live). The rules fingerprint
  the sources that decide what a read stores, so no version needs bumping
  by hand: `build.rs` lists them by module (a split stays covered), leaves
  out what decides nothing (`LEFT_OUT`, test code), and fails the build
  when a listed unit or exclusion names nothing or a definition in
  `DECIDING` is outside the hashed set. A new function that decides what
  a read yields or keeps joins `DECIDING`, and so does a helper of a
  `DECIDING` function that moves out of the hashed set (or the module it
  moves to joins `SOURCES`); code moving into a module the list does not
  cover joins `SOURCES`. A test module stays its file's last item, or it
  is hashed. `tests/sweep.rs` holds the rule,
  `tests/fingerprint.rs` the fingerprint; the ignored `measured` test
  prints what three starts over forty logs cost.
- **Retention runs after every write.** The protected set is kept whole:
  boss kills, timed keys, pinned fights and progression wipes until the
  first kill in that edition at that difficulty. The caps count the
  unprotected alone, per edition (`Edition::counts_as`); never demote or
  evict a protected fight.
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
- The daemon's exit, by any way out of `hub::run`, terminates the overlay
  it spawned (`Supervisor::shutdown`, which `Drop` also runs, so a panic
  unwinding out of `run` is covered): an orphaned overlay's reconnect
  respawns daemons. A user-launched overlay is only a session; it stays.

## The addon (`addon/`)

- Embedded in the daemon. `wowdps addon install` installs it, `wowdps addon`
  reports it. The daemon rewrites a stale copy on start and never installs a
  missing one.
- It writes raid members' guilds (which the combat log never carries) into
  its SavedVariables on logout; they lag a logout. The daemon reads them on
  start and on a 30 s idle poll.
- It also records each NPC's classification and lieutenant flag by creature
  id (`WOWDPS_DATA.creatures`). The poll merges them into
  `<history dir>/creatures.tsv`, a format the replay reads (CONTRACT.md,
  the addon paragraph; codec `proto::creatures`). The newest `seen` per id
  wins and a row the addon pruned stays. Never rewrite a file the codec
  refuses, and never write a player's name into it.
- Any change under `addon/` makes every installed copy stale (the check is
  byte for byte), so the daemon rewrites it on its next start. Bump the
  Lua's `SCHEMA` when the table's shape changes.
- The TOC's interface number comes from the install's `.build.info`; never
  hardcode it.
- One copy for now: the first source's product's (`DaemonOptions::production`'s
  `addon_dir`). An edition followed beside it has its copy and SavedVariables
  untouched until the addon per product lands.

## The mock

`mock.rs` is an in-process daemon over the real engine and a fixture,
driving `ClientState` synchronously; the GUI's and the TUI's tests run on
it. Keep it answering as the hub does.

```sh
cargo test -p wowdps-daemon
WOWDPS_REAL_LOG=/path/to/WoWCombatLog-*.txt cargo test --release -p wowdps-daemon -- --ignored real_log --nocapture   # Retail's, name-free; a Forever log is passed over
WOWDPS_REAL_LOG_FOREVER=/path/to/_forever_/Logs/WoWCombatLog-*.txt cargo test --release -p wowdps-daemon --test real_log_placed -- --ignored --nocapture   # Forever's placed rows, name-free
```
