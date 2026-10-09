---
type: Crate
title: wowdps-proto
description: 'Wire codec, daemon client and shared client state for wowdps.'
resource: crates/proto
tags: [crate]
status: stable
generated: { by: okflight/0.4.0, at: 2026-09-06T15:50:50-07:00 }
---

The wowdps wire protocol: hand-rolled, zero-dependency, binary, length-prefixed frames over a unix socket, plus the client library that speaks it. Depends on `wowdps-model` only — never on the engine. Also home to the shared client-side extras every frontend may need without touching the daemon: the hand-rolled JSON value (`json`) and the talent dataset + import-string codec (`talents`, ruling R14) — here rather than in one frontend so mcp and gui read the same code. `history` is the on-disk record codec of the history store (roadmap item 1): the daemon writes these documents, the readers parse them.

## Modules

- `wire.rs` — little-endian primitives and `u32 len | u8 tag | body`
  frames; decode never panics.
- `msg.rs` — `ClientMsg` / `DaemonMsg` and `PROTO_VERSION`. A `Watch`
  declares a `Cursor` (the list, or a segment and view with an optional
  drill) and the daemon pushes snapshots for exactly that, plus an
  unsolicited `SegmentList` broadcast whenever the segment id table changes
  shape, so off-list navigation always resolves ids.
- `client.rs` — `socket_path()` embeds `PROTO_VERSION`. `ensure_daemon`
  spawns on demand and waits, for one-shot clients.
  `DaemonClient::try_reconnect` is the tick-driven clients' path (window,
  overlay, TUI): one connect attempt, a spawn at most once per doubling
  backoff, never a wait — a window that blocked its UI thread 3 s per tick
  on the old path spawned 628 daemons in half an hour and the compositor
  called it unresponsive. `DaemonClient`'s reader thread coalesces stale
  snapshots.
- `state.rs` — `ClientState`, the accessor surface renderers draw from
  (the old `App`'s); `apply` / `on_msg` return the requests to send; a held
  `j`/`k` clamps against the cached snapshot and never round-trips; `log_id`
  (below).
- `json.rs`, `talents.rs` — the hand-rolled JSON value and the R14 talent
  dataset and import-string codec, shared by the mcp tools (which re-export
  them) and the GUI's talent viewer.
- `history.rs` — the history store's on-disk record codec: `FightCard`,
  `FightRows`, `FightDetails`, `StoredLoadout`, `Annotation`, `Affiliation`
  as one-line JSON documents, `HISTORY_SCHEMA`, the fight, log and content
  ids and the loadout hash. The daemon writes them and every reader parses
  them here; `Affiliation::read_saved_variables` turns the wowdps addon's
  `WOWDPS_DATA` into records.
- `lua.rs` — a stdlib reader of the Lua the game writes to
  `SavedVariables/*.lua`: `NAME = value` globals, tables with bracketed,
  bare and positional keys, every escape the serializer emits, `1/0`-style
  floats. A reader of serializer output, never an interpreter.
- `series.rs` (v39) — the history store's first binary tier,
  `series/<id>.bin`: per player the Damage and Healing abilities and the
  damage targets second by second as `model::series::SeriesRow`s, runs of
  seconds and varints behind a (guid, offset, length) index so
  `read_player` reads one player's block alone; decode never panics or
  allocates past the bytes in hand.
- `replay.rs`, `varint.rs`, `dirs.rs` — below.

## Source

- Manifest: [`crates/proto/Cargo.toml`](../../../crates/proto/Cargo.toml)
- Root: [`crates/proto/src/lib.rs`](../../../crates/proto/src/lib.rs)

## The replay tier

`replay.rs` (v45, [R29](../rulings/r29.md)) is the history store's second
binary tier, `replay/<id>.bin` (`WDRP`, a section index, each post against
its unit's last), and `replay::csv` the one writer of the seven files a
replay reads; `GetReplay` / `Replay` carry it
([why](../decisions/replay-tier-in-the-store.md)). Both binary tiers code
through one crate-private `varint` module (varints, zigzag, strings, the
bounds-checked cursor), so the two decoders share their refusals.

The file is `WDRP` | format 1 | a section index | strings, head, units,
posts, events, placed, markers, spells (`encode`, `decode`, `format_of`;
decode never panics). `replay::csv` writes a `Cut` column for column as the
extractor's old cutter did: `units.tsv`, `tracks.csv`, `events.csv`,
`placed.csv`, `markers.csv`, `raid.csv` (R25's series, never stored twice)
and `pull.txt`.

## Seams

`ClientState` is every frontend's state machine, and a frontend changes its
semantics only by opting in: `set_follow(true)` — the window's
master-and-detail, never the TUI's — makes the drill follow the meter
selection (each move re-watches the segment with the row as the drill,
`v` pins one half of a pair); off, a drill is the screen it always was
([the Inspector decision](../decisions/inspector-beside-the-meter.md)).
Every capability the window added is opt-in this way (`set_follow`,
`open_death`, `select_player`), and the TUI calls none of them.
`log_id` hands on what `SegmentList` names, the tailed log's identity,
which with a row's start is its stored card's fight id — additive, read
by the window's pull rail alone
([the Rail decision](../decisions/one-pull-rail.md)).
`raid()` holds the snapshot's raid timeline for the segment, and
`open_death` — opt-in like following, the TUI never calls it — drills the
Deaths view into one death window in one Watch
([the Wire decision](../decisions/raid-timeline-and-mine-on-the-wire.md)).
`select_player` — opt-in too — selects a player by key rather than by row,
for the window's command palette, which names a player the chart in hand
may not hold yet (a view switch still on its way)
([the Redesign decision](../decisions/window-redesign.md)).

`dirs` resolves the XDG base directories once (`data_home`, `cache_home`,
`state_home`, `config_home`, `data_path`): an empty or relative variable
counts as unset, as the spec words it, and `Base::resolve` is the rule as a
pure function so its tests never touch the process environment.
`talents::data_path` delegates to it, and
[`wowdps-encounter-rubric`](encounter-rubric.md) resolves its text sidecars
through it.

## Contract

Public signatures and dependency policy: [`CONTRACT.md`](../../../CONTRACT.md).
