# crates/proto (wowdps-proto)

The wire codec, `DaemonClient`, `ClientState`, the history store's record
codecs and binary tiers, the generated `placed` table, and the shared client
extras (`json`, `talents`, `lua`, `dirs`). Depends on model alone, never the engine. Module map:
`docs/OKF/crates/proto.md`. Wire spec: CONTRACT.md, "Wire protocol".

## Rules

- **Decode never panics.** Truncation, bad tags, bad bools, bad UTF-8 and
  lying counts are errors, never a panic or an attacker-sized allocation
  (`tests/fuzz.rs`). The same holds for every binary tier and for `json`.
- **A wire shape change bumps `PROTO_VERSION`.** Change `msg.rs`, the
  golden bytes in `tests/codec.rs` and CONTRACT.md's wire section, with the
  number in its heading, in one change; bump once per branch, however many
  shapes move. A diff in the goldens is a wire change, never a fixture to
  refresh. The socket and lockfile names embed the version, so deploy with
  `docs/OKF/playbooks/bump-proto-version.md`.
- **`ClientState` changes only by opt-in.** `set_follow`, `open_death` and
  `select_player` are the window's; the TUI calls none of them. A new
  capability for one frontend is opt-in too, and every other frontend keeps
  its semantics.
- **Held keys never round-trip.** `j`/`k` clamp against the cached
  snapshot.
- **Tick-driven clients never wait.** The window, overlay and TUI reconnect
  through `DaemonClient::try_reconnect`: one attempt, a spawn at most once
  per doubling backoff. `ensure_daemon` (spawn and wait) is for one-shot
  clients only; a window that blocked on it spawned 628 daemons in half an
  hour.
- **One codec per stored format.** `history.rs` is the history store's
  record codec (one-line JSON documents): the daemon writes them, every
  reader parses them here. A new field must read as absent from every older
  file. `creatures.rs` is the codec of `creatures.tsv` (the addon's NPC
  classifications, read by the replay): its header names `FORMAT` 1, and
  `tests/creatures.rs` pins its bytes.
- **Binary tiers keep their formats.** `series.rs` (`FORMAT` 2) and
  `replay.rs` (`FORMAT` 6) each carry a format number read by `format_of`;
  a reader takes older formats and refuses newer ones, and a format bump
  needs the daemon's rewrite queue to bring old files forward. Both code
  through the crate-private `varint.rs`.
- **`placed` is generated.** `placed/placed_spells.rs` and its review twin
  come from `tools/gen-placed-spells.sh` (`placegen.rs` in
  `tools/extract`); never hand-edit them. The daemon's replay cut keeps
  the lines of its spells (core's `PlacedTable`) and the replay draws
  them by it, so a new entry changes the cut (`tools/AGENTS.md`). It is a
  stored table, not a message: changing it moves no `PROTO_VERSION`.
- **`lua.rs` reads serializer output** (the game's `SavedVariables`); it is
  never an interpreter.
- **`dirs.rs` is the one XDG resolver.** An empty or relative variable
  counts as unset. Resolve every per-machine path through it.

```sh
cargo test -p wowdps-proto           # codec goldens, fuzz, state, history, client
```
