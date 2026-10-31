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
  shape, so off-list navigation always resolves ids. `ImportReplay` (v46)
  hands the daemon a fight from outside this machine's logs, answered
  `ImportedReplay` or `Refused`; `HistoryQuery::Fights`' `OriginFilter`
  lists imports only when asked, so every older client reads what it read.
- `client.rs` — `socket_path()` embeds `PROTO_VERSION`. `ensure_daemon`
  spawns on demand and waits, for one-shot clients.
  `DaemonClient::try_reconnect` is the tick-driven clients' path (window,
  overlay, TUI): one connect attempt, a spawn at most once per doubling
  backoff, never a wait — a window that blocked its UI thread 3 s per tick
  on the old path spawned 628 daemons in half an hour and the compositor
  called it unresponsive. `DaemonClient`'s reader thread coalesces stale
  snapshots. Without `/proc` (macOS) the socket dir's ownership check
  learns our uid once per process, so every caller agrees on the dir, from
  a probe file created exclusively in the temp dir under a name no other
  try in the process takes (pid plus a counter, which steps past a crashed
  process's leftover); a temp dir where no probe can be created reads as
  uid 0 and fails the check closed.
  The handshake ignores only macOS's EINVAL from clearing its read timeout
  after the peer has closed.
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
  ids and the loadout hash. A card's `Origin` (v46) is written only for an
  import (`"origin": {"kind": "imported", "source": …}`, last), so a local
  document is byte for byte the older one; `ImportedCard` is what an import
  knows and `ImportedCard::card` the card the store keeps, its id
  `imported_id` ([Imported Fights Live In The History Store](../decisions/imported-fights-in-the-store.md)).
  v48: each `ImportedPlayer` carries the totals the importer reckoned and
  its 1 s damage and healing curves; `card` derives the rates with
  `wowdps_model::rate` over the duration (the card's `combat_ms`),
  `details` makes the details tier from the curves, and `check_curves`
  is the store's refusal when a curve does not sum to its total or runs
  past the fight ([An Import Carries Its Meter](../decisions/an-import-carries-its-meter.md)).
  A card's `KeyInfo::affixes` (v47) is written the same way: last in its
  `key`, only when the key names some; on the wire the key carries the
  list always, embedded
  ([Keep A Keystone Run's Affixes](../decisions/keep-keystone-affixes.md)).
  The daemon writes them and every reader parses
  them here; `addon_table` reads the wowdps addon's `WOWDPS_DATA` once and
  `Affiliation::from_addon_table` turns its players into records.
- `creatures.rs` — the codec of `creatures.tsv`, the addon's NPC
  classifications at the store's root: `Creature`, `parse` (refuses another
  or a newer header, skips a row it cannot read), `render` (the golden
  bytes the replay reads), `merge` (newest `seen` per creature id; absent
  rows stay) and `Creature::from_addon_table` over the SavedVariables
  section ([Creature Classifications Come From The Addon](../decisions/creature-classes-come-from-the-addon.md)).
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
- `placed.rs` — the generated table of what players place in the room
  (`placed/placed_spells.rs`, written by gen-placed-spells with its
  review twin beside it): each spell's `Found`, `Form`, `Purpose`, reach
  and duration, and the lookups (`all`, `placed`, `tells`,
  `Placed::telling`). The daemon's replay cut keeps the lines its spells
  write, through [wowdps-core](core.md)'s `PlacedTable`
  ([R29](../rulings/r29.md)), and the replay draws them by it, so it
  lives where both a daemon and a pure client read it. It was the
  encounter rubric's until the rubric left wowdps with the replay
  (2026-10-30).
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

The file is `WDRP` | format 6 | a section index | strings, head, units,
posts, events, placed, markers, spells, maps, absorb, pools (`encode`,
`decode`, `format_of`; decode never panics, and allocates in proportion to
the bytes: no power type at or above `POOL_KINDS`, no pools list past
`MAX_POOLS`, no affix count past the head's bytes). Format 6 is format 5's
sections but the head, whose key adds its affixes after its level (a
count, then each id), so the rewrite queue recuts every older file from
its log; an older file reads with none, and `pull.txt` says `affixes = …`
when the key names some. Format 5 is format 4's codec but the head's number, moved
with the cut's pool readings (a cast's after its cost, an energize's gain on
the pool it fills: the pools section lists the posts an energize set as it
lists a cast's second pool) so the rewrite queue recuts every older file,
and `replay::csv`'s `tracks.csv` writes `power` and `pools` that way.
Format 4 is format 3's codec but the head's number, moved
with the cut's aura rules so the rewrite queue recuts every format-3 file
too; `encode` writes each post's pools tidied (`next_pools`), so what it
writes decodes. Format 3 keeps each post's
shields (the `absorb` section: each track's changes at their post indices)
and every pool kind (the `pools` section lists only the posts whose pools
are not their last's with their own pool set in — a cast's second pool;
every format reads the rest by that rule), and the buff rows (kinds 16–18);
its posts section is format 2's, byte for byte, and format 2 still reads,
with no shield. Format 2 keeps every floor's posts — the
`maps` section codes each post's floor as the changes of the tracks that
leave the head's, so a one-floor pull pays four bytes — and the boss rows
(a row naming no unit is `has` bit 5); format 1 (the head's floor alone)
still reads, every post on that floor, and the daemon's rewrite queue
recuts it, as it recuts formats 2 and 3. `replay::csv` writes a `Cut` column for
column as the extractor's old cutter did, plus `tracks.csv`'s trailing
`map_id`, `absorb` and `pools`, the boss rows and the buff rows: `units.tsv`, `tracks.csv`, `events.csv`, `placed.csv`,
`markers.csv`, `raid.csv` (R25's series, never stored twice) and
`pull.txt`.

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
the encounter rubric (the replay's, since 2026-10-30) resolves its text
sidecars through it.

## Contract

Public signatures and dependency policy: [`CONTRACT.md`](../../../CONTRACT.md).
