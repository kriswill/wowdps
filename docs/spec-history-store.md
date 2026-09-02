# History store — specification (roadmap item 1)

Status: **proposed**, 2026-09-02. Supersedes the sketch in `docs/roadmap.md`
§1. Nothing here is implemented.

How this was written: an architect draft, a visionary pass on what the store
should make possible later, a researcher's measured comparison of storage
engines, and an adversarial review that checked every claim against the tree
at f1fb0ac. §11 records where they disagreed and what was decided. The full
reports are not in the repo; the conclusions are.

## 1. Intent

The game writes a fresh `WoWCombatLog-*.txt` per session and the daemon tails
only the newest, so today history ends at the last login. The store makes
fights persist across sessions and answers, for a year of play:

- best kill per boss and difficulty, overall and per character + spec;
- pulls-to-kill progression per boss, per night;
- DPS / HPS trend per character + spec over time, bucketed by day or week,
  scoped to a game build ("since the nerf");
- keystone time trends per dungeon and level;
- ad hoc questions the fixed tools did not anticipate, asked in SQL by a person
  or by the coach.

It also becomes the substrate for roadmap item 4 (coach grades and notes) and
item 2's history screens.

## 2. Non-negotiables

- **Summaries, never events.** Nothing is stored that `Meter` cannot re-derive
  from the log, and nothing is keyed per event. CONTRACT.md's daemon section
  ("the only persistence is the index-checkpoint cache … never parsed
  meters") is amended in the same PR to permit *derivable fight summaries*
  and to state that boundary.
- **The daemon stays stdlib-only.** It writes plain files with the existing
  `proto::json` value type and answers the fixed questions from an in-memory
  index. No engine lives in the daemon, ever.
- **The files are the truth.** Any index, cache or materialized table is
  rebuildable from them and may be deleted at any time.
- **Decode never panics.** A torn, foreign or corrupt file is skipped and
  reported once in `Status`.
- **A live meter is never delayed by history.** The hub only ever `try_send`s
  to the history thread; a full channel drops the write and reports it.
- **Written now, impossible to retrofit:** the game build, a timezone-correct
  epoch, and content-addressed loadouts. Every record carries them from v1.

## 3. Storage technology

Decision: **a local data lake.** The daemon writes NDJSON / JSON files that
DuckDB reads directly; DuckDB lives only in a new binary crate.

The researcher measured the candidates on this machine (24 cores; CI will be
slower):

| Option | Crates | Cold build | Binary delta | Analytical power | Could live in the daemon? |
| --- | --- | --- | --- | --- | --- |
| DuckDB, system `libduckdb` (nixpkgs 1.5.5) | 111 | 19 s | +1.5 MB, +75 MB shared lib | full OLAP SQL over JSON/CSV/Parquet globs | no |
| DuckDB, `bundled` | 112 | 70 s here, est. 10–20 min on CI | +44 MB | same | no |
| SQLite via rusqlite `bundled` | 15 | 16 s | +1.9 MB | window/JSON/CTE; planner needs hand-held indexes | policy change, but the lightest |
| DataFusion 55 / Polars 0.55 / Lance | 237–277 | ~70 s | tens of MB | full | no (tokio, API churn) |
| redb / sled | 2 | 3 s | +1.1 MB | none (KV) | no need |
| Pure stdlib | 0 | 0 | 0 | fixed queries only | yes |

Measured over a synthetic lake of 5 000 fights (20 MB of headers, 65 MB of
rows): DuckDB answers a best-kill group-by over 5 000 separate JSON files in
0.5 s including process start, a weekly trend join in 0.44 s, and the same
trend in 35 ms once materialized; idle in-process RSS is 36 MB. SQLite answers
the trend in 0.11 s without an index and 15.7 s with the wrong one.

Why the lake and not a database owned by the daemon:

- DuckDB's file lock is one read-write process *or* N read-only; a daemon
  holding `history.duckdb` open would lock out the MCP server and the GUI.
  Lake files have no lock.
- DuckDB 2.0 ships in September 2026 with a new default storage format and a
  reworked C API; the Rust crate pins one DuckDB minor. A `.duckdb` file as the
  source of truth would tie the user's data to that churn. JSON does not
  churn.
- The daemon cannot take the dependency, and the fixed questions need only
  the ~300 B fight cards, which stdlib Rust answers from memory in
  microseconds.

**Fallback engine:** rusqlite `bundled` inside the same binary, same files,
`materialize` loads them into `history.sqlite` in WAL mode. **Fallback for the
MCP tool alone:** exec the nixpkgs `duckdb` CLI with no Rust crate.

Consequences for policy: CONTRACT's Dependencies paragraph gains one line,
`history: model + proto + duckdb (system-linked, pinned to the nixpkgs
version; never bundled)`. The flake sets `DUCKDB_LIB_DIR` / `DUCKDB_INCLUDE_DIR`
from `pkgs.duckdb.lib` / `.dev` (nixpkgs ships no `.pc`) and wraps the binary
with the library on its rpath, as it does for the GUI's wayland libraries.
Extension autoinstall and autoload are off, `lock_configuration` on; JSON,
Parquet and ICU are statically linked in the nixpkgs build, so the engine
never touches the network. This dependency is the one item in the spec that
needs explicit sign-off.

## 4. Prerequisite core changes

Three small changes to `crates/core`, each a CONTRACT edit with fixture
parity, shipped before the store:

1. **Encounter identity.** `Segment`, `SegmentMeta`, `SegmentInfo` and
   `ListRow` gain `encounter: Option<Encounter { id: u32, difficulty: u32,
   group_size: u32 }>` from `ENCOUNTER_START`. Today only the name survives,
   and Heroic and Mythic share a name. The scanner mirrors it; `cache.rs`
   bumps its MAGIC byte (one full rescan). Wire: trailing field, part of the
   `PROTO_VERSION` bump in §8.
2. **Game build.** `Event::Version` gains `build: (u16, u16, u16)` and
   `project_id: u8` from the `COMBAT_LOG_VERSION` line's `BUILD_VERSION` and
   `PROJECT_ID` fields, which the parser currently discards. It is already a
   seed line (R6), so lazy loads carry it for free. The meter exposes the
   current build on the segment.
3. **Timezone offset.** Log timestamps already carry a full date and a
   `-Z` offset; the parser reads the date into `ts_ms` (a local-time epoch)
   and deliberately drops the offset. Add `parser::tz_offset_min(line) ->
   Option<i16>`; the store applies it once per log. Legacy `M/D` lines with no
   year store `tz_min = None` and are flagged. No CONTRACT signature changes.

Reserved, not built: `best_pct: Option<u16>` on the fight card. Boss health is
not observed anywhere today (the advanced block's hp feeds only the death
recap rings). Ruling R15 — min observed hp fraction over hostile non-pet
sources while an encounter is open — is its own CONTRACT PR with its own
fixture lines. Until it lands, "best-percent progression" is not promised.

## 5. Fight identity

```
fight_id = "<log:016x>-<start_ms>"
log      = fnv64(first complete line of the file)   // the COMBAT_LOG_VERSION header,
                                                     // unique per session (ms timestamp + build)
           else fnv64(file name)                     // a log begun mid-session
start_ms = Segment.start_ms == SegmentMeta.start_ms  // identical by the parity tests
```

- Computed lazily at first store, never at `Switched` — the daemon retargets
  to a new log the moment it appears, when it may hold half a line.
- Not a byte offset: the live meter has none (`LogLine` and `TailEvent::Lines`
  are CONTRACT-fixed and the tailer strips line endings), and two segments
  cannot start on the same millisecond in one file. `byte_range` is stored as
  provenance when the index has it.
- Not `(dev, ino)`: a log copied out of the prefix must import to the same id.
- A keyed visit's Overall uses the visit's `start_ms`; an arena match its
  segment's.
- **Idempotent.** The write path is insert-if-absent on `fight_id`. Restarts,
  rescans and `--file` replays of a stored log write nothing. A record is
  rewritten only when its `schema` is older than the daemon's.
- **Derived, not primary:** `content_id = fnv64(encounter id, difficulty,
  start epoch second, group_size, sorted friendly guids)`. Two people's logs
  of the same pull share a `content_id` but keep separate records — their
  numbers differ. It exists for export and annotation addressing.

## 6. What is stored

Kinds: `Encounter` segments (raid bosses and arena matches), keyed visits'
`Overall`. Trash only under `history_store_trash`. `noise` never. A keyed
visit's members are stored as their own records only under the trash switch;
the Σ is what a key's history means.

Aborted fights (an encounter closed by a version seam, a rotation or daemon
exit with no `ENCOUNTER_END`) are stored with `success: null` and `aborted:
true`. They are listed but never count as pulls.

Per fight, three files in two tiers:

| File | Tier | Contents | Size (20 players) |
| --- | --- | --- | --- |
| `fights/<id>.json` | card, always | identity, encounter, visit/key facts, `start_local_ms`, `tz_min`, `start_utc_ms`, `duration_ms`, `official_ms`, `pars_ms`, `success`, `aborted`, `build`, `project_id`, `log_version`, `owner_guid`, `players[]` | ~400 B + 60 B per player |
| `rows/<id>.json` | rows, always | the six `View`s' meter rows (all players, no top-n), per-player death recaps (event and attacker rows) | 12–20 KB |
| `details/<id>.json` | detail, kills / bests / pinned | per-player by-spell and by-target breakdowns for Damage and Healing, per-player damage and healing timelines (1 s buckets + marks) | 60–120 KB, ~10 KB per timeline on a 35 min key |

`players[]` on the card carries per player: `guid`, `name`, `class`, `spec`,
`loadout_hash`, `enemy`, and the top-line `amount` and `per_sec` for Damage
and Healing plus a death count. That denormalization is what lets trend and
best-per-player queries run without opening a rows file.

Side tables: `loadouts/<hash>.json`, content-addressed by fnv64 of the
loadout's wire encoding (most pulls in a night share one; a collision shows a
wrong build, never loses data, and the write compares bytes first).
`annotations/<id>.ndjson`, append-only records `{ts, kind, author, rubric,
body, tags}` reserved for item 4; no tool writes them yet, but the codec and
the eviction rule exist from v1.

Excluded on purpose: raw events, spell-of-a-spell timelines and spell target
lists (derive by reopening the log while it exists), compare windows
(computed from stored by-spell rows), anything about players not in the
fight. `per_sec` and `pct` are stored as computed, not recomputed.

Budget: 5 000 fights ≈ 100 MB of cards and rows; details are capped by
retention (§7).

## 7. On-disk layout, durability, retention

```
$XDG_DATA_HOME/wowdps/history/v1/
  fights/<fight_id>.json
  rows/<fight_id>.json
  details/<fight_id>.json
  loadouts/<hash>.json
  annotations/<fight_id>.ndjson
```

- Every file is one JSON document written to `.tmp` then renamed, exactly as
  `cache.rs` does, so a reader never sees a partial file and DuckDB never
  needs `ignore_errors`. Per-fight files were chosen over monthly NDJSON for
  this reason and because eviction is then an `unlink`; DuckDB showed no
  penalty for 5 000 small files.
- Every document carries `schema: u16` (`HISTORY_SCHEMA`, independent of
  `PROTO_VERSION`). Within `v1/`, fields are only added, and readers tolerate
  missing ones. A breaking change is `v2/` plus a one-shot migrator that reads
  old and writes new. Never in-place edits.
- There is no manifest. The daemon's index is rebuilt from `fights/` at start
  (5 000 × 400 B, milliseconds). The `tier` of a fight is the existence of its
  details file.
- Retention keys, top-level in `~/.config/wowdps/config.toml` (the daemon's
  reader ignores `[sections]` and has no list type, so keys stay flat and the
  character list is one comma-separated string):

| Key | Default | Meaning |
| --- | --- | --- |
| `history_enabled` | `true` | write at all |
| `history_dir` | XDG default | override |
| `history_store_trash` | `false` | §6 |
| `history_keep_per_encounter` | `200` | cards + rows kept per (encounter id, difficulty) |
| `history_keep_details_per_encounter` | `10` | details kept per (encounter id, difficulty); demotion is an unlink |
| `history_characters` | `""` | "Name-Realm, …" that are "me" (§9) |

- Eviction runs on the history thread after every write and never touches
  the **protected set**: pinned fights, annotated fights, the fastest kill per
  (encounter, difficulty), and the owner's highest `per_sec` per (encounter,
  difficulty, spec) for Damage and Healing. The set is recomputed at eviction
  time. Everything else is oldest-first.
- Unwritable directory or ENOSPC: the write fails soft, `Status` reports it,
  the daemon lives.

## 8. Daemon: write path, index, wire

**Detection.** In `Engine::on_tail`'s `Lines` arm, after `feed`, a
`closed_seen` vector parallel to `live_ids` turns each `end_ms: None →
Some` transition into `EngineEvent::Closed(SegmentId)`; visits likewise. Only
after `CaughtUp` — backlog goes through import.

**Extraction.** The hub clones the closed `Segment` (one allocation;
loadouts are `Arc`) and hands `Box<Segment>` plus the log identity to the
history thread. Row, recap, breakdown and timeline computation all happen
there. The hub thread does no history work beyond the clone and a
`try_send` on a bounded (64) channel.

**History thread** (`daemon/src/history.rs`, spawned beside the loader pool):
owns the directory, the in-memory index (`Vec<FightCard>` plus
`by_encounter`, `by_key`, `by_guid` maps), writes, eviction, the fixed
queries, and replies through `HubMsg::History` so the hub forwards with
control-message ordering. `history::Store` is generic over a backend
(directory or in-memory) so `daemon::mock` can drive it synchronously.

**Import.** On start, sweep every log in `logs_dir` newest-first and store
each closed index segment not yet present, plus an aborted record for a
segment still open at EOF. `wowdps history import <log|dir>` does the same on
demand. Loads run through the loader pool with `LoadReq.purpose = History`
so they bypass the LRU; one outstanding load while a client is watching,
unlimited when idle.

**Wire (v20).** Trailing `encounter` on `SegmentInfo` / `ListRow`, then:

| Tag | Message | Answer |
| --- | --- | --- |
| `0x08` | `GetHistory { req_id, query: HistoryQuery }` | `0x8B History { req_id, answer }` |
| `0x09` | `GetFight { req_id, fight_id, view, drill }` | `0x8C Fight { req_id, fight: Option<StoredFight> }` |
| `0x0A` | `PinFight { req_id, fight_id, pinned }` | `0x8B History` with `Pinned` |
| — | unsolicited | `0x8D HistoryChanged { fight_id }` on every store, like `SegmentList` |

```
enum HistoryQuery {
  Fights      { encounter, difficulty, guid, since_utc_ms, kind, sort: Newest|Fastest|OwnerPerSec, limit },
  Progression { encounter, difficulty },
  Trend       { guid, spec, encounter, difficulty, view, bucket: None|Day|Week, since_utc_ms, limit },
}
enum HistoryAnswer {
  Fights(Vec<FightCard>),
  Progression { pulls, kills, first_kill: Option<FightCard>, nights: Vec<{date, pulls, kill}>, median_kill_ms },
  Trend(Vec<{bucket_utc_ms, fight_id, spec, amount, per_sec, duration_ms, n}>),
  Pinned { fight_id, pinned },
}
```

Best kill is `Fights` with `sort: Fastest, limit: 1`; key times are `Fights`
with `kind: Key`. All are one-shots with `GetLoadout` semantics: always
answered, never an error; a disabled store answers empty and `Status` says
why. `Cursor` is untouched — a stored fight never changes, so nothing about
it is watchable. `ClientState` gains a passthrough only; history screens are
item 2's, window-local like the talent viewer.

## 9. Who is "me"

`history_characters` is the source of truth. When empty, the owner is
inferred as the intersection of `COMBATANT_INFO` guids across *all* stored
logs — the logger is in every log they write, guildmates are not — and
`Status` marks it `inferred`. Never inferred per log, and never from meter
rows: the row builder drops zero-output actors, so a logger who died early
would vanish from a per-log intersection.

## 10. Readers

**`crates/history` → binary `wowdps-history`,** reached as `wowdps history …`
through the existing external dispatch. model + proto + `duckdb =
"~1.10505.0"`. Opens with `threads = 2`, `memory_limit = 256MB`. Defines
views over the lake: `fights` from `read_json('v1/fights/*.json')`, `players`
by unnesting `players[]`, `rows` from `rows/*.json`, `loadouts`,
`annotations`. Subcommands: `sql <query> [--json]`, `best-kill`,
`progression`, `trend`, `materialize` (writes `cache.duckdb`, which only this
binary ever opens, so the single-writer lock never crosses a process),
`import`, `export <fight_id>` (card + rows + details + annotations as one
self-contained file), `stats`.

**MCP** stays stdlib. Tools:

| Tool | Backed by |
| --- | --- |
| `history` (filters, sort, limit) | `GetHistory::Fights` |
| `progression` | `GetHistory::Progression` |
| `trend` (with `bucket`) | `GetHistory::Trend` |
| `stored_fight` | `GetFight`; same JSON shape as `fight` / `breakdown`, so the coach rubric needs no second path |
| `pin_fight` | `PinFight` |
| `history_sql` | spawns `wowdps-history sql --json`; absent binary = tool not registered |

Tool descriptions state that the store holds every raid member's name and
loadout from the user's own log.

**GUI** takes fixed screens from the daemon in item 2 and shells out to
`wowdps history` for anything richer. The GUI stays model + proto.

**Privacy.** v1 stores what the log already holds, on the same disk. Names
are inline in row labels because that is the only row encoder there is;
redaction belongs to the export path, where a label rewrite is a rendering
concern.

## 11. Decisions where the personas disagreed

| Question | Architect | Visionary | Adversary / researcher | Decided |
| --- | --- | --- | --- | --- |
| Date source | filename + midnight rollover | line offset | parser already reads the date; offset is parsed then dropped | §4.3 |
| Primary key | `(log, byte offset)` | content hash | offsets unreachable on the live path; content hash collides across two loggers | `(log, start_ms)` primary, `content_id` derived |
| Log identity | fnv of first 4 KiB | — | file may be half a line when first seen | fnv of first complete line, lazily |
| Format | wire codec, byte-identical rows | — | no external-tool story | JSON via `proto::json`; DuckDB reads it |
| Detail demotion | rewrite the fight file | — | second writer, second decoder | separate file, unlink |
| Config | `[history]` table | — | daemon reader skips sections | flat `history_*` keys |
| "Me" | guid in every fight of the log | — | zero-output actors have no rows | config, else COMBATANT_INFO intersection across logs |
| Hub-thread cost | ~1 ms clone of rows | — | tens of ms for a keyed Σ with timelines | clone the Segment, compute on the history thread |
| Tools | 7 | 2 + sugar | fold best/keys into `history` | 5 fixed + `history_sql` |
| Gear-delta tool | — | proposed | n = 1, causal-looking noise | cut; data kept, coach reasons over it |
| Affixes on keys | — | proposed | parser + scanner + fixture change for no asker | deferred |
| Annotations | — | first-class record | zero cost now, expensive later | reserved in v1 |
| Boss health | R15 proposal | assumed tracked | not tracked anywhere | reserved field, R15 separate |

## 12. Testing

- **Core:** encounter identity, build and tz helpers on `sample.txt`,
  `instance.txt`, `arena.txt` with scanner/meter parity and the expected
  files gaining id / difficulty columns.
- **Codec (`proto/tests/history.rs`):** golden JSON for every record type,
  round-trip, missing-field tolerance, a v1 document decoding after a field
  is added, fuzz (truncate at every byte) never panics.
- **Store (`daemon/tests/history.rs`, tempdir):** `sample.txt` closes two
  encounters and the raid Σ → three cards, three rows files, no trash;
  trash switch on → trash present. Restart on the same dir, same fixture →
  zero new files. Corrupt card → skipped, reported, others served. Retention
  and the protected set. Details demoted by unlink. `instance.txt` keyed Σ
  with `official_ms` and verdict, members absent. `arena.txt` match WIN /
  LOSS, enemy rows kept. Aborted record for a segment open at EOF. CRLF copy
  of a log imports to the same `fight_id`. Unwritable dir fails soft. No
  history traffic before `CaughtUp`. Restart mid-fight, then the
  `ENCOUNTER_END` arrives on the fresh daemon → stored once via import.
- **Hub:** each message answered with control ordering; full channel drops
  the write, sets `Status`, keeps the 10 Hz cadence.
- **Analytics:** a generated 5 000-card index; answers asserted and each
  query under 20 ms in release.
- **Lake parity gate:** the daemon's `Fights`/`Progression`/`Trend` answers
  over `sample.txt` equal `wowdps-history`'s SQL over the files the same run
  wrote. This is the test that keeps two readers honest.
- **MCP:** every tool over the mock bridge; `stored_fight` byte-equal to
  `fight` for the same fixture fight.
- **Perf gate:** the ignored `real_log` test imports a real 300 MB log and
  reports fights, wall time and bytes per fight.
- **GUI/TUI seam:** `daemon::mock` over the in-memory backend so item 2's
  screens render headless.

## 13. Delivery order

1. Core prerequisites (§4) with fixture parity. One PR.
2. `proto::history` codec + golden JSON. One PR.
3. Daemon store: thread, write path, import, retention, `Status`; CONTRACT
   amendment; config keys. One PR, `PROTO_VERSION` 20.
4. Wire queries + MCP tools. One PR.
5. `crates/history` with DuckDB, flake wiring, the lake parity gate. One PR,
   after the dependency sign-off.
6. R15 boss health as its own CONTRACT PR, unblocking best-percent
   progression.
