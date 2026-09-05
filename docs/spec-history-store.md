# History store — specification (roadmap item 1)

Status: **implemented and hardened**, 2026-09-03 — every §13 step landed,
delivered as one branch / PR (#12, `history-store`) rather than six PRs: the
feature commit, then a code-review pass and fourteen rounds with the coaching
session that exercises the MCP tools over a real store (26 logs, ~390
fights). The sections below describe the tree as shipped; where the design
moved during implementation the section says so. The short list of what
differs from the 09-02 draft:

- **Wire (still `PROTO_VERSION` 20 — the branch is unmerged, so v20 absorbed
  every later field).** Beyond §8's table: `ImportLog 0x0B` and `Regrade 0x0C`;
  `GetFight` has a trailing `boss`; `SegmentList` carries the tailed log's
  `log_id`; `HistoryQuery::Fights` pages with `after_id` and its answer carries
  `total`; `Progression` / `Trend` take `local_cutover_hour`; `Night` gained
  `kills` and `tz_min`, `TrendPoint` `tz_min`; `StoredFight` reports `tier`,
  `has_recap` and the drilled player's `loadout`; `HistoryAnswer::Regraded`.
- **Records.** The card carries `best_pct` (R16, no longer reserved) and, on
  keys, `bosses[]`. Pins survive the one legitimate rewrite. `regrade`
  rewrites a card in place from its log.
- **Rules learned on real logs.** A key's member boss is never a pull of its
  own — even when its START predates the log (keystone difficulty 8 marks
  it); an older log's open *visit* (the night's last key) is imported too; an
  abandoned key is `aborted`; a suspended visit reads live only while the game
  runs; a provisional log identity (half-written header) is never memoized.
- **R16** ended up stricter than §4 sketched: per-NPC health, the boss is the
  largest-max NPC (councils = unique creature ids within half of it, never an
  add pack), "down" is under 0.1 % because the game parks a boss at 1 HP, the
  grade is the lowest *surviving* boss, and a kill is 0 by definition.
- **DuckDB** is nixpkgs 1.5.4 (crate `=1.10504.0`; §3 measured 1.5.5). A
  reading lake fences file access to its own directories (§3), takes bound
  parameters, and `wowdps history` reads config `history_dir`; the crate links
  the daemon crate for that config reader (§10).
- **Daemon.** Pending imports hold a non-lingering daemon open; the loader's
  reply to the history thread blocks rather than riding the lossy channel; the
  production test suite sandboxes `XDG_DATA_HOME`.

Supersedes the sketch in `docs/roadmap.md` §1.

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
  to the history thread; a full channel drops the write and reports it. (The
  loader pool's *reply* to the history thread is the one blocking send: a lost
  `Loaded` would wedge the import queue, and the pool has a worker to spare.)
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
Extension autoinstall and autoload are off, the extension repository and
directory point inside the lake (an explicit `INSTALL` would otherwise still
reach the network), and `lock_configuration` is on; JSON, Parquet and ICU are
statically linked in the nixpkgs build, so the engine never touches the
network. A **reading** lake (`Lake::open`) additionally fences file access to
the lake's own five data directories (`allowed_directories` +
`enable_external_access = false`, then locked): `history_sql` hands an LLM's
SQL to it verbatim, and without the fence `COPY … TO` and `read_text()` could
read or write anything on the machine. Simply disabling external access does
not work — the views re-read their JSON on every query. `materialize` opens
writable for its `ATTACH`. Queries take bound parameters (`Lake::sql_with`,
`--params`, the tool's `params`) so a string literal never crosses a quoting
layer. This dependency was signed off (DuckDB 1.5.4 in the flake).

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

4. **Boss health (R16, built).** `best_pct: Option<u16>` on the fight card
   is written, and `Progression` answers each night's lowest. The rule as
   shipped is stricter than the "min observed hp fraction" first sketched
   here, because real logs broke the naive version three times: the parser's
   `HpHint` carries the described unit's flags and only a hostile-reaction
   `Creature-`/`Vehicle-` counts (a friendly totem is a Creature too); health
   is tracked per NPC and the boss is the largest-max NPC, every NPC within
   half of it being a council member *only if its creature id spawned once*
   (Coiled Altar's eighteen 223M Manifestations are an add pack); "down" is
   under 0.1 % because the game parks a boss it will not let die yet at 1 HP;
   the grade is the lowest *surviving* boss (a fallen council member is
   progress, not the number); and a kill is 0 by definition (a scripted death
   lands no 0/max report). `Segment::boss_health` exposes the per-NPC
   observations. `wowdps history regrade` re-derives stored cards under the
   current rule.

## 5. Fight identity

```
fight_id = "<log:016x>-<start_ms>"    # a pull
fight_id = "<log:016x>-<start_ms>s"   # a visit's Σ (key or Overall): a visit and its
                                      # first member can share a millisecond, so the Σ
                                      # carries a mark (Store::open renames older Σ cards)
log      = fnv64(first complete line of the file)   // the COMBAT_LOG_VERSION header,
                                                     // unique per session (ms timestamp + build)
           else fnv64(file name)                     // a log begun mid-session
start_ms = Segment.start_ms == SegmentMeta.start_ms  // identical by the parity tests
```

- Computed lazily at first store, never at `Switched` — the daemon retargets
  to a new log the moment it appears, when it may hold half a line. A
  provisional identity (header line not yet whole → file-name hash) is never
  memoized: it is re-read on every use until the header lands, or the same
  fight would be stored twice under two ids on the next start.
- Not a byte offset: the live meter has none (`LogLine` and `TailEvent::Lines`
  are CONTRACT-fixed and the tailer strips line endings), and two segments
  cannot start on the same millisecond in one file. `byte_range` is stored as
  provenance when the index has it.
- Not `(dev, ino)`: a log copied out of the prefix must import to the same id.
- A keyed visit's Overall uses the visit's `start_ms`; an arena match its
  segment's.
- **Idempotent.** The write path is insert-if-absent on `fight_id`. Restarts,
  rescans and `--file` replays of a stored log write nothing. A record is
  rewritten in three cases only: its `schema` is older than the daemon's; it
  was `aborted` and the same fight closes for real (its END arriving after a
  restart); or a `Regrade` asked for it. Every rewrite carries `pinned`
  forward — a pin is the user's decision — and annotations are separate
  files, never touched.
- **Derived, not primary:** `content_id = fnv64(encounter id, difficulty,
  start epoch second, group_size, sorted friendly guids)`. Two people's logs
  of the same pull share a `content_id` but keep separate records — their
  numbers differ. It exists for export and annotation addressing.

## 6. What is stored

Kinds: `Encounter` segments (raid bosses and arena matches), keyed visits'
`Overall`, and plain visits' `Overall` (a raid night's Σ — stored whether or
not the visit ever closed, see §8 Import). Trash only under
`history_store_trash`. `noise` never. A keyed visit's members are stored as
their own records only under the trash switch; the Σ is what a key's history
means — and a boss pulled at keystone difficulty (8) is a member even when the
key's START predates the log, so a daemon that attached mid-run never
promotes it to a pull. The key's card lists its members as `bosses[]` (name,
encounter, start, duration, verdict, pull order), and `GetFight { boss }`
parses a member from the log on demand (§8) — per-boss grading without
per-boss records.

Aborted fights (an encounter closed by a version seam, a rotation or daemon
exit with no `ENCOUNTER_END`) are stored with `success: null` and `aborted:
true`. They are listed but never count as pulls.
An aborted keystone Σ (left, or the log ended inside it) has no END to clock
it: its stored `duration_ms` is combat time up to its last hit, and its
per-second numbers are over that. The live row for the same visit runs the
key clock while the visit is open, so the two can differ until the record is
written; a regrade of such a record can therefore change its duration.

Per fight, three files in two tiers:

| File | Tier | Contents | Size (20 players) |
| --- | --- | --- | --- |
| `fights/<id>.json` | card, always | identity, encounter, visit/key facts, `start_local_ms`, `tz_min`, `start_utc_ms`, `duration_ms`, `official_ms`, `pars_ms`, `success`, `aborted`, `build`, `project_id`, `log_version`, `owner`, `byte_range`, `pinned`, `best_pct`, `players[]`, `bosses[]` (keys) | ~400 B + 60 B per player |
| `rows/<id>.json` | rows, always | the six `View`s' meter rows (all players, no top-n), per-player death recaps (event and attacker rows) | 12–20 KB |
| `details/<id>.json` | detail: written for kills and for wipes of at least `history_details_min_wipe_secs` (60 s; never for aborted fights); retention then keeps bests / pinned and caps the rest | per-player by-spell and by-target breakdowns for Damage and Healing, per-player damage and healing timelines (1 s buckets + marks) | 60–120 KB, ~10 KB per timeline on a 35 min key |

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

- Every file is one JSON document written to a uniquely named `.tmp` sibling
  then renamed (`cache::write_atomic`, shared with the index cache — unique so
  the tail thread and the start-up sweep can checkpoint the same index without
  sharing a temp file), so a reader never sees a partial file and DuckDB never
  needs `ignore_errors`. Per-fight files were chosen over monthly NDJSON for
  this reason and because eviction is then an `unlink`; DuckDB showed no
  penalty for 5 000 small files.
- Every document carries `schema: u16` (`HISTORY_SCHEMA`, independent of
  `PROTO_VERSION`). Within `v1/`, fields are only added, and readers tolerate
  missing ones. A breaking change is `v2/` plus a one-shot migrator that reads
  old and writes new. Never in-place edits.
- There is no manifest. The daemon's index is rebuilt from `fights/` at start
  (5 000 × 400 B, milliseconds). The `tier` of a fight is the existence of its
  details file; `GetFight` answers from the card alone when the rows are gone
  and reports `tier` (card / rows / details) so a reader knows what it could
  not serve rather than receiving a partial document.
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
| `history_details_min_wipe_secs` | `60` | a wipe at least this long gets a details file at write time (kills always do; aborted fights and shorter wipes never) |
| `history_characters` | `""` | "Name-Realm, …" that are "me" (§9); a bare "Name" matches any realm |

- Eviction runs on the history thread after every write and never touches
  the **protected set**: pinned fights, annotated fights, the fastest kill per
  (encounter, difficulty), and the owner's highest `per_sec` per (encounter,
  difficulty, spec) for Damage and Healing. The set is recomputed at eviction
  time. Everything else is oldest-first.
- The details cap counts every details file in the group — a long wipe's as
  much as a kill's — so under pressure wipes' details go first: a kill is
  protected as the fastest (or the owner's best) far more often than a wipe
  is, and the oldest unprotected file is always the one unlinked. A wipe
  shorter than `history_details_min_wipe_secs` never had details; a reader
  that finds none applies the same rule to the card to say "never written"
  rather than "demoted" (`stored_fight`'s error text does).
- Unwritable directory or ENOSPC: the write fails soft, `Status` reports it,
  the daemon lives.

## 8. Daemon: write path, index, wire

**Detection.** In `Engine::on_tail`'s `Lines` arm, after `feed`, a
`closed_seen` vector parallel to `live_ids` turns each `end_ms: None →
Some` transition into `EngineEvent::Closed(SegmentId)`; visits likewise. Only
after `CaughtUp` — backlog goes through import. A visit's Σ closing when the
daemon attached mid-visit needs the scanned prefix resident: if nobody watched
the Σ (or the LRU evicted it) the hub requests the prefix load and stores the
fight when it lands (`Engine::history_pending`) rather than dropping the key.
A visit that zoned out is only *suspended* (R10); its Σ row reads live while a
member is being fought, the game runs, or lines arrive — a stale log's last
key is not a fight happening now.

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
each closed index segment not yet present, plus, for every log but the tailed
one, an aborted record for a segment still open at EOF *and the open visit*:
zoning out only suspends a visit, so the night's last key — and the raid
itself — is still open when the player logs off and exists only as the
index's `open_visit` (a keyed run whose END fired is a finished run; a key
without one is `aborted`; a plain visit's Σ is stored as is). The first sweep
without this lost 12 of 36 keys. `wowdps history import <log|dir>` does the
same on demand; liveness is judged from the daemon's own source, not the
argument, so a hand-imported older file gets its open visit too. Loads run
through the loader pool as `LoadReply::History` so they bypass the LRU; one
job outstanding at a time, so a watching client always finds a worker free.
Pending imports hold a non-lingering daemon open (a one-shot `import` used to
spawn a daemon that quit ten seconds later with the queue undispatched).
`Regrade` rides the same queue: the log is found by its identity among the
source's files, the fight by its start in a fresh cached scan (a Σ card
matches Overall metas only — a visit and its first segment can start on the
same line), and the card is rewritten in place.

**Wire (v20, as shipped).** Trailing `encounter` on `SegmentInfo` /
`ListRow`; trailing `log_id: Option<u64>` on `SegmentList` (the tailed log's
identity once its header is whole — with a closed row's `start_ms` it names
the row's fight id, so `list_fights` rows carry `history_id` and the fight
tools accept `fight_id`); `Status` carries `HistoryStatus`. Then:

| Tag | Message | Answer |
| --- | --- | --- |
| `0x08` | `GetHistory { req_id, query: HistoryQuery }` | `0x8B History { req_id, answer }` |
| `0x09` | `GetFight { req_id, fight_id, view, drill, boss }` | `0x8C Fight { req_id, fight: Option<StoredFight> }` — with `boss`, a key member parsed from the log on demand, answered when the load lands |
| `0x0A` | `PinFight { req_id, fight_id, pinned }` | `0x8B History` with `Pinned` |
| `0x0B` | `ImportLog { req_id, path }` | `0x8B History` with `Imported { queued }` — the count of LOGS queued for scanning, answered before any is read; the history thread scans one file between messages so a directory of gigabytes never holds the mailbox shut |
| `0x0C` | `Regrade { req_id, fight_id, encounter, difficulty, kind }` | `0x8B History` with `Regraded { queued }`; the rewrites ride the import queue |
| — | unsolicited | `0x8D HistoryChanged { fight_id }` on every store, like `SegmentList` |

```
enum HistoryQuery {
  Fights      { encounter, difficulty, guid, since_utc_ms, kind, sort: Newest|Fastest|OwnerPerSec, limit, after_id },
  Progression { encounter, difficulty, local_cutover_hour },
  Trend       { guid, spec, encounter, difficulty, view, bucket: None|Day|Week, since_utc_ms, limit, local_cutover_hour },
}
enum HistoryAnswer {
  Fights { cards: Vec<FightCard>, total },
  Progression { pulls, kills, first_kill: Option<FightCard>, nights: Vec<{day_utc_ms, pulls, kill, best_pct, kills, tz_min}>, median_kill_ms },
  Trend(Vec<{bucket_utc_ms, fight_id, spec, amount, per_sec, duration_ms, n, tz_min}>),
  Pinned { fight_id, pinned },
  Imported { queued },
  Regraded { queued },
}
StoredFight { card, rows, breakdown, tier: 1 card | 2 rows | 3 details, has_recap, loadout }
```

`after_id` pages in the answer's order (an unknown cursor starts over).
`local_cutover_hour` buckets nights (and trend days / weeks) by each card's
own log timezone as local days starting at that hour, so a raid evening past
local midnight is one night; `None` = UTC calendar days, the default.
`Fights` stamps `owner` at answer time, so cards written before
`history_characters` was set still name the owner.

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
"=1.10504.0"` (system-linked to nixpkgs' 1.5.4), plus the daemon crate for
its config reader alone — `--dir`, else config `history_dir`, else the XDG
default, so SQL always reads the lake the daemon writes (the draft's "model +
proto only" lost that). Opens with `threads = 2`, `memory_limit = 256MB`,
offline and fenced as §3 says. Defines views over the lake: `fights` from
`read_json('v1/fights/*.json')`, `players` by unnesting `players[]`, `rows`,
`details`, `loadouts`, `annotations`. Subcommands: `sql <query> [--params
<json array>] [--json|--objects]` (`?` placeholders), `best-kill`,
`progression`, `trend`, `materialize` (writes `cache.duckdb`, which only this
binary ever opens, so the single-writer lock never crosses a process),
`import` and `regrade <fight_id | --encounter N [--difficulty D] | --kind K>`
(thin clients of the daemon's `ImportLog` / `Regrade`; `regrade` waits for
the queue to drain and prints before → after per card), `export <fight_id>`,
`stats`, `views`. `tests/parity.rs` is the lake parity gate and also gates the
read-only fence and bound parameters.

**MCP** stays stdlib. Tools:

| Tool | Backed by |
| --- | --- |
| `history` (filters incl. difficulty by name, sort, limit, `after_id` → `total` + `next_after_id`; `players: me / none / all / <name>` — the owner's row as `me` with `rank_dps` / `dps_count` / `dps_median` / `dps_share` among DPS-role players, zero-output players excluded, a named player as `peer`) | `GetHistory::Fights` |
| `progression` (`bucket: "local"` + `cutover_hour`; `first_kill` / `best_kill` as references; nights carry `kill`, `kills`, `best_pct`, `night_local`) | `GetHistory::Progression` |
| `trend` (with `bucket`, `local`; points carry the spec name and `date_local`) | `GetHistory::Trend` |
| `stored_fight` (`boss` for a key member; answers `tier` + `available_views`, and every gap — not stored, details demoted, no recap — is an error) | `GetFight`; same JSON shape as `fight` / `breakdown`, so the coach rubric needs no second path |
| `pin_fight` | `PinFight` |
| `regrade_fights` (`fight_id` \| `encounter` + `difficulty` \| `kind`) | `Regrade` |
| `history_sql` (`params` for `?` placeholders) | spawns `wowdps-history sql --json`; absent binary = tool not registered |

The live tools grew to meet the store: `list_fights` rows and the `fight`
headers carry `encounter { id, difficulty, difficulty_name, group_size }` and
`history_id`; `fight` / `breakdown` / `compare` / `loadout` accept `fight_id`
(resolved against the daemon's list, an older night pointing at
`stored_fight`); `loadout { fight_id, player }` answers a stored fight from
the loadouts tier; player rows carry `role`; difficulty arguments accept names
("Heroic", "Mythic Keystone"). Tool descriptions state that the store holds
every raid member's name and loadout from the user's own log, and what
`success` / `owner` mean on the SQL views.

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
| Boss health | R16 proposal | assumed tracked | not tracked anywhere | built as R16 (§4.4), refined three times on real logs |

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

## 13. Delivery order — planned, and as it happened

Planned as six PRs: (1) core prerequisites, (2) the codec, (3) the daemon
store, (4) wire queries + MCP tools, (5) `crates/history` + the parity gate,
(6) R16. Delivered as **one branch, PR #12**: the six steps landed together in
the feature commit (signing blocked committing them as they were finished),
then a code-review pass fixed eight defects and three nits, then fourteen
rounds with the coaching session — which tests the MCP tools over a real
store and talks to the development session directly over the cross-session
channel, with the retest / response files under `~/Documents/wow-coach/` as
the durable record — produced the paging, the `me` / `peer` grade, roles,
difficulty names, one id everywhere, `stored_fight` tiers, `loadout` by
stored id, pin durability, the regrade command, local nights, the keyed-boss
drill, and the R16 refinements. Additional tests that came out of it and
should not be lost: the production suite sandboxes `XDG_DATA_HOME` (its
sweep once imported the fixture into the user's real store); a real-daemon
regrade test pins, tampers, regrades and checks; a real-daemon drill test
over the keyed fixture; the open-visit, abandoned-key and keystone-difficulty
member tests; the parity gate's read-only fence and bound-parameter checks.

## 14. What the coach reads, and what the store could hold next

The wow-coach report for 2026-09-02 (`~/Documents/wow-coach/tranqster-raid-
report-2026-09-02.html`: a Normal clear, a Heroic re-run, two keys) is the
best specimen so far of what the store is *for*. Every claim in it traces to
a source; this section records which sources the store holds, which it only
half-holds, and what would let the next report be sharper. Written 2026-09-03
after the store's first real night; the items are candidates, not commitments,
and the ones that matter most are marked.

**What the report is made of, per boss pull:** kill time, the owner's DPS,
rank among DPS specs and ratio to the DPS-spec median (the store's `me`
block); a key-ability cast count ("Stars" = Collapsing Star casts, from the
by-ability rows); potion use *and its timing relative to Bloodlust*; deaths
with a grade (unavoidable / defensible / closed) built from the death recap's
hp slide and damage sources; a 10-second damage timeline with the lust window
drawn as a band and the first Metamorphosis marked ("first Meta at 0:30,
peers burst at 0:10" — the single most repeated finding); the same timeline
for two same-spec peers; the build per pull (hero tree, one node's presence)
and gear receipts between pulls (item id, ilvl, slot); for keys the same line
per boss plus the whole-key share and trash-death counts; a death ledger; a
"what closed" list citing earlier findings; ranked homework with evidence; a
retraction record.

**Held today, and served:** `me` / `peer` rank, count, median and share
(§10); by-ability rows (cast counts) and by-target; 1 s damage / healing
timelines with item, consumable and external (lust) marks — for kills, bests
and pins; death recaps with hp per event for every stored fight; the logged
loadout per pull (talents with tiered-node splits, gear by slot with ilvl);
`best_pct`; keys' `bosses[]` and the on-demand boss drill; local nights.

**Half-held — the refinements, most valuable first:**

1. **Marks and a coarse timeline on the rows tier, for every fight.** The
   report grades potion timing, lust alignment and the opener on wipes as
   much as on kills, but timelines and marks live in `details/`, which
   retention keeps only for kills, bests and pins. A per-player list of marks
   (trinket use / proc, consumable, external, and the two kinds below) plus a
   10 s damage series per player is a few hundred bytes per player — put it
   on `rows/` so a wipe's opener and potion are gradable forever. The 1 s
   grids stay in `details/`.
2. **Major-cooldown and defensive marks.** "First Meta at 0:30" was computed
   by re-reading the log; Metamorphosis is a class cast, and `Cast` is
   deliberately not a mark source (R12). A generated per-spec table of
   *major cooldowns* (SpellCooldowns / SpellCategories: base cooldown ≥ 60 s,
   or the spec's known burst window) and *personal defensives* (the R9 recap
   already wants "defensives used") would give two new `MarkKind`s from the
   same lookup path `item_spells` uses, and the opener / defensive-under-
   pressure findings become one timeline read. Both tables are `tools/gen-*`
   outputs like `class_spells.rs`, regenerated per patch.
3. **Annotations, written (roadmap item 4).** The store reserved
   `annotations/<id>.ndjson` and the eviction rule already protects annotated
   fights, but nothing writes them; the coach's grades, findings, homework
   and retractions live in its session memory. A `grade_fight` / `note` pair
   (an `Annotate` message + MCP tools; `stored_fight` and `history` return
   them) makes "what closed since last time" a query over stored evidence
   instead of a memory lookup — the report's "What closed tonight" and the
   retraction paragraph are exactly that shape.
4. **Item names.** Gear rows carry item ids only; the report says "item
   268211, Baleful Hexblade" because the name came from a website. A
   per-machine `item-names.bin` (ItemSparse.db2 → id → name, like the icon
   caches; never committed) read by the MCP and the GUI's inventory tab makes
   every gear receipt legible. Same generator family as `gen-item-spells.sh`.
5. **Hero tree on the card's player rows** (`hero: "Annihilator"`). The
   report pivots on it (Annihilator vs Void-Scarred pulls); it is derivable
   from the stored loadout through the dataset's subtree table, so the MCP
   can fill it on the `me` / `peer` / `players[]` rows from the loadouts tier
   without a store change. Cheap, and it settles the "which build was this"
   question the retraction turned on.
6. **Damage-taken breakdown per player in `details/`.** The soak analysis
   ("zero buckets at 2:40, 3:20, 4:00 shared by all three: Guillotine
   soaks") and the death coaching ("Mutilated Gash and Venomous Surge ticks")
   need taken-by-ability per player; today only the dead player's recap has
   it. Adding `taken_spells` to `PlayerDetail` costs one more row list per
   player on kills; roadmap item 2's avoidable-damage table then has data to
   mark against.
7. **A `loadout_diff` view** (two fight ids, one player → slots that
   changed, node picks that changed) so gear receipts and mid-night respecs
   are one call rather than two loadouts compared by hand.
8. **Difficulty names from the install.** `difficulty_name` is a hand table;
   id 250 (a 39-player Venomous Abyss visit) is still unnamed. Difficulty.db2
   through `wowdps-extract` (its FileDataID is what is missing) makes the
   table generated like the others.

**Not worth storing:** peer or raid-median timelines (derivable from what
kills already hold), spell-of-a-spell timelines (§6's exclusion stands),
anything the log does not carry (the guide prose, the usage snippets — those
are the coach's research, cited in the report's footer, and belong in
annotations if anywhere).
