---
type: Crate
title: wowdps-daemon
description: 'Headless wowdps daemon: tails the combat log and serves meter snapshots over a unix socket.'
resource: crates/daemon
tags: [crate]
status: stable
generated: { by: okflight/0.4.0, at: 2026-09-06T18:29:58-07:00 }
---

The wowdps daemon: one process owns tail → index → parse → meter → snapshots for every client. Threads + channels, no async runtime. `run` is the whole daemon; `DaemonOptions` makes every path and grace injectable so the integration suite can run real daemons on temp sockets against the fixtures.


## Modules

- `engine.rs` — the live meter and index, with daemon-lifetime-monotonic
  `SegmentId`s and an LRU of at most 16 parsed segments; liveness from
  observation plus the game-process signal, never mtime. It builds a
  segment's raid timeline only for the client kinds that read it
  (`engine::wants_raid`: `Window` and `Mcp`) and the drill's stacked series
  only for `Window` sessions (`engine::wants_series`).
- `hub.rs` — the session table; 10 Hz changed-only pushes; an immediate
  reply on `Watch`.
- `loader.rs` — historical parses off the hub thread.
- `server.rs` — accept, reader and writer threads; the lockfile is taken
  before the stale socket is unlinked.
- `game.rs` — a 3 s `/proc` sweep for `game_process`.
- `overlay.rs` — the supervisor (below): spawns `<gui_binary> --overlay` on
  game start, `SetVisible` on exit, exit-grace termination, manual-hide
  stickiness, spawn stderr surfaced in `Status`.
- `cache.rs` — index checkpoints under `$XDG_CACHE_HOME/wowdps/index`, never
  parsed meters; `write_atomic` is the daemon's one durability primitive.
- `addon.rs` — the wowdps addon's life on disk (v31): `product_dir` derives
  `<install>/_retail_` from the tailed logs dir and validates the install,
  `interface_version` reads `.build.info`'s Version into the TOC's
  `## Interface:` number, `install` writes `Interface/AddOns/wowdps/`
  atomically, `inspect` tells Current from Stale byte for byte,
  `ensure_current` is the start-up rule — rewrite a stale copy, leave a
  missing one missing — and `saved_variables` lists every account's
  `wowdps.lua` ([Guilds Come From An Addon](../decisions/guilds-come-from-an-addon.md)).
- `history.rs` — the history store (below).
- `mine.rs` — whose rows are the reader's (below).
- `replay.rs` — core's `PlacedTable` over the rubric's generated table.
- `config.rs` — a section-aware TOML-subset reader of
  `~/.config/wowdps/config.toml`: `logs_dir`, `game_process`,
  `auto_overlay`, `overlay_exit_grace_secs`, `gui_binary`, and the flat
  `history_*` keys (v45 added `history_keep_progression_whole` and
  `history_replay_mb`).
- `mock.rs` — an in-process fake daemon over the real engine and a fixture,
  driving `ClientState` synchronously (what `testkit` was to the old `App`);
  it feeds every `Closed` into a `MemBackend` store and cuts a fight's
  replay lazily on the first `GetReplay` for it.

## The history store, tier by tier

A thread owning `$XDG_DATA_HOME/wowdps/history/v1/` and an in-memory index
of the cards (roadmap item 1, [spec](../../spec-history-store.md)).

- **Intake.** The hub hands it one `Segment` clone per
  `EngineEvent::Closed` over a bounded `try_send` and forwards the tailed
  log's index for import; a start-up sweep imports older logs through the
  loader pool via `LoadReply::History`, one job at a time; a `Retire` for the
  log the tailer left rescans it as an older log (below).
- **Affiliations** (v31). `affiliations/<guid>.json` from the addon's
  SavedVariables, read on start and on a 30 s idle poll, newest sighting per
  guid, JOINED onto a card's players when it is answered and never stored on
  the card; the addon's own-character set names the owner before the
  COMBATANT_INFO intersection does.
- **Backends.** `Store<B: Backend>` is generic — `DirBackend` in
  production, `MemBackend` for the mock and tests; retention and the
  protected set run after every write.
- **The series tier** (v39, format 2 since v42). `series/<id>.bin` is
  written beside the details for a kill, a key or a pinned fight
  (`Retention::wants_series`), unlinked with them or when a wipe's pin goes,
  kept in an in-memory set, and read per player through
  `Backend::read_range`. It windows a stored Damage or Healing drill
  through the model's `series::window_rows` — the live drill's own
  function, so `tests/series.rs` holds stored = live window for window.
  Format 2 adds each Damage ability's targets second by second, every
  ability's whole-fight targets and the 1 s damage taken, and the details
  tier the count views' drills (`counts[]`), so a stored drill stacks
  (`model::series::stack`, the live ranking), opens an ability and compares
  (`Ask` carries view, drill, death, range, spell, pair and `stacked`; one
  `answer` serves the stored and the derived path) exactly as live
  ([A Series Tier For Stored Windows](../decisions/a-series-tier-for-stored-windows.md)).
  Pinning a fight that earns the tier but lacks it queues its rewrite.
- **Kept whole.** Every boss kill and timed key (`Retention::keeps_whole`,
  protected; config `history_keep_kills_whole`, on by default) and every
  PROGRESSION wipe — a boss pull lost at an (encounter id, difficulty) no
  stored card killed — until the first kill there (`Store::is_progression`,
  config `history_keep_progression_whole`, on by default; a Heroic kill says
  nothing of Mythic). The caps count the unprotected alone; `HistoryStatus`
  counts the kept set by why
  ([Stored Pulls Kept Whole](../decisions/stored-pulls-kept-whole.md)).
- **The replay tier** (v45, R29). Every boss pull and keystone run (never
  an aborted one) is cut into `replay/<id>.bin` (`daemon::replay` over
  core's cut and the rubric's `placed` table): beside the meter on import
  and regrade (`CutJob::Also`, one parse), as a cut alone after a live close
  once idle (`CutJob::Only`), and by the rewrite queue (`Store::recuts`) for
  a slot lacking a current file — missing, or an older format (format 1,
  the head's floor alone, recut as format 2). A replay is kept while protected, else
  among the newest `history_keep_details_per_encounter` of its group
  (`Store::replay_slots`); past `history_replay_mb` (default 4096) the
  oldest unprotected goes first, and at the cap the rewrite queue cuts only
  a fight newer than the oldest unprotected replay held. `GetReplay`
  answers it.
- **The rewrite queue.** Each series file's format is read once at open;
  after its first status the thread queues `Store::rewrites` — a kept fight
  short of its details, a series file missing or in an OLDER format (never
  a newer one) — and rewrites them from their logs, a log's fights at a
  time, while idle.
- `HistoryStatus` rides in `Status`.

## History store: the retire, mine and the replay tier

`history.rs` is a thread owning the fight lake: the hub hands it one clone
per closed segment, the tailed log's index for backlog, and — since PR #56 —
a `Retire` for the log the tailer just left, which it rescans as an older
session so an abandoned pull and the night's Σ import without a restart
([why the trigger is the file switch and never the game process](../decisions/retire-a-log-on-tail-switch.md)).

Whose rows are the reader's is the history thread's to say: `mine.rs`'s
`Mine` (every character of the account the store knows, by guid, plus the
configured names) is published through `HistoryLink::mine`, and the engine
marks each snapshot's rows, drill and raid deaths from it before they go
out; the store marks a stored fight's when it answers
([the Wire decision](../decisions/raid-timeline-and-mine-on-the-wire.md)).

Since v45 the store keeps the REPLAY tier too ([R29](../rulings/r29.md)):
`replay.rs` implements core's `PlacedTable` over the encounter rubric's
generated table, the loader cuts a segment beside its meter from one parse
(`CutJob`), a live close queues a cut alone, and retention keeps a replay in
the details tier's slots and all season for the protected set
([why](../decisions/replay-tier-in-the-store.md)) — which since v45 holds
every progression wipe ([A Progression Wipe Is Kept Whole](../decisions/progression-kept-whole.md)).
A replay that lands after its card (a live close, a pin) is broadcast as
`HistoryChanged` on its own, so a client told `None` asks again; adding one
trims by size alone over one walk of the protected set, and a backfill batch
scans its log once for its rewrites and its cuts together. `MemBackend`
answers a size and a range without reading a read-through file whole.

## Overlay supervisor

`overlay.rs` spawns `<gui_binary> --overlay` when the game appears. The
config key `gui_binary` (default `wowdps-gui`, the [GUI](gui.md)) is unset
in an ordinary config; it let [gui-new's](../decisions/gui-on-gpui.md)
overlay follow the game while two GUIs existed, and still lets another
build stand in. `Config::gui_bin` resolves it once, at start:

- **A bare name** is the daemon binary's sibling when one exists. That is a
  dev build's own GUI.
- **Otherwise** it is the name itself, for the spawn to find on `$PATH`.
  The home-manager and NixOS modules depend on that step: the daemon's
  package holds no GUI, and their `guiPackage` option puts one on the
  service's `PATH`.
- **A value with a `/`** is a path, never re-rooted.

`Status` does not name the binary, because naming it would be a wire
change. A failed spawn already does, in `Failed`'s
`spawning <path>: …`. The dev unit (`tools/dev-unit.sh`) stamps the daemon
and the configured GUI only, so a build of the other GUI never restarts the
live daemon.

## Source

- Manifest: [`crates/daemon/Cargo.toml`](../../../crates/daemon/Cargo.toml)
- Root: [`crates/daemon/src/lib.rs`](../../../crates/daemon/src/lib.rs)

## Contract

Public signatures and dependency policy: [`CONTRACT.md`](../../../CONTRACT.md).
