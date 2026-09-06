# Role pivots, step 4b — implementation plan (R18 in the store, MCP and DuckDB)

Scope: the records side of step 4, deferred from 4a-ii (`docs/plan-role-
pivots-step4.md`, PR #23), revised after the devil's-advocate review (log
at the end). After this a tank's active-mitigation uptime, a healer's
externals and a supporter's per-target uptime are gradable from stored
records, the coarse taken / healing series and the mark list ride the
rows tier (store spec §14 items 1 and 2), and `stored_fight`'s Taken
drill answers with a timeline again. Branch `role-pivots-8` off
`role-pivots-7`.

## Facts (explorers + the real logs and store, 2026-09-04)

- Engine (4a-ii, `crates/core/src/meter.rs`): `spans(guid) -> Vec<Mark>`
  (capped `SPAN_CAP = 256`, private const, newest dropped), `uptime(guid)
  -> Vec<UptimeRow { spell_id, label, kind: MarkKind, src, count, total_ms
  }>` (uncapped, per (spell, caster), keyed by TARGET, sorted by kind code
  / spell / src), `am_uptime_ms(guid) -> i64` (union, clamped at `close_ms`
  except on Overall), `externals_given / externals_received(guid) -> (u32,
  i64)`, `support_uptime(guid) -> Vec<(target owner guid, spell_id,
  total_ms)>`, `taken_timeline(guid)`, `heal_timeline(guid)`, and
  `Timeline::coarsen(factor)` (u64 chunk sums, a partial tail kept, marks
  cloned). `marks_for` (private) merges item marks and `spans()` sorted by
  `at_ms`; every drill's marks are that list. There is **no
  `am_uptime_pct` anywhere**: callers divide by `duration_ms(now)`.
  `UptimeRow` is used outside `meter.rs` only by `tests/spans.rs`; none of
  proto / daemon / mcp / history names `uptime` or `coarse`.
  `support_uptime` is a projection of `uptime` rows (kind `SupportBuff`,
  the target's rows where `src` = the supporter). `max_ms` (spec §6) is not
  computed. `MarkKind` has codes but **no `name()`**; details' timeline
  marks write `kind` as the code.
- Records (`crates/proto/src/history.rs`): `CardPlayer` has 22 stored
  fields; derived values are *methods* emitted only by `to_json_in`
  (`role`, `mitigated_pct`, `effective_dps`) and skipped by `from_json`,
  which defaults every added field with `unwrap_or(0)` — the whole "old
  cards read zeros" mechanism. `FightRows { views, recaps, mitigation,
  support }`; `PlayerDetail` stores full `damage_timeline` /
  `heal_timeline` with marks (`src` already written) and **no taken
  timeline**. Goldens and strip-key tests: `crates/proto/tests/history.rs`
  (`golden_without_support` is the 3b template).
- **Wire**: `put_card_player` (`msg.rs:1170`) writes every `CardPlayer`
  field in order with no length prefix; `FightCard` rides
  `HistoryAnswer::Fights` and `StoredFight.card`; the `me` / `tank_pair`
  grade runs in the MCP over that answer. So a card field IS a wire change
  — 2b (v22) and 3b (v23) each bumped for exactly this. `StoredFight {
  card, rows, breakdown: Option<Breakdown>, tier, has_recap, loadout,
  support }`; the stored Taken drill's `Breakdown.timeline` is `None`
  since v21 (CONTRACT's v24 row: "until the coarse series lands on the
  rows tier (step 4b)"); **the stored Healing drill on a tier-3 fight
  serves the details tier's 1 s `heal_timeline` today** (`history.rs:1898`,
  same in `derived_fight`). `TrendMeasure` codes 0–4. The MCP's `curve()`
  (`tools.rs:2310`) divides each chunk by `chunk.len() × bucket_ms`.
- Daemon (`crates/daemon/src/history.rs`): `extract()` (:2094) builds the
  roster from the four views + supporters and fills the card per guid from
  `seg.*`; `stored_fight` / `derived_fight` must stay byte-identical;
  `trend` matches the measure at :1793; protected set measures 0/1/2 (dps,
  hps, mitigated_pct); regrade = `store_impl(force)`.
- MCP: `grade.rs` `Measure { Dps, Hps, Effective }`; `graded_row` (:1401)
  builds me/peer with `tank_pair` (:1456); `measure_for_role`;
  `stored_fight` support block (:1181); `mark_json` already emits `caster`
  / `active_secs`. The coach reference (`~/.claude/skills/wow-coach/
  references/mcp-tools.md`) says the Taken timeline is "live only until 4b".
- DuckDB (`crates/history/src/lib.rs`): card-field probes (`players_have_*`
  via `LIMIT 0` over the recursive unnest) with `CAST(0.0 AS DOUBLE)`
  fallbacks (`pct_sql`), rows-tier views through `probe_view(name, sql,
  typed)` (CREATE + DESCRIBE, every typed column `!= "JSON"` exactly),
  `stats` counters, `materialize` over `self.views`. **An all-empty LIST
  column types as `JSON[]`** (verified on duckdb 1.5.4), which the exact
  comparison passes; a `::BIGINT[]` cast works on `JSON[]`. Parity
  (`tests/parity.rs`) starts a real daemon over a fixture and compares
  Fights / Trend / role_ranks to SQL; mixed-lake tests assert the exact
  `views()` array (unchanged for pre-4b lakes); downgraders `pre_2b_*` /
  `pre_3b_*`.
- Real store (408 fights, verified read-only): **0 cards carry `taken` or
  `role`, 0 rows carry `mitigation` or `support`, 0 details carry `src`**
  (the lake predates step 1). Rows p90 207 203 B (biggest 26 players),
  cards p90 7 192 B.
- **Measured budget** (gawk over the 1.34 GB raid log, 31 pulls, 20
  players, and a 32-min +14 key; role-table ids only): a raid pull ≥ 120 s
  averages 63 (target, spell, caster) cells and 158 spans, worst 111 /
  290; per player p90 ≤ 7 cells and ≤ 25 spans, a tank ≤ 70; the key
  16–22 cells and 130–238 spans (tank 53–93). `SPAN_CAP` is never reached.
- Fixture goldens (`spans.expected.tsv`): the Warrior's AM union 27 000 ms
  of 60 000 on the kill, 5 000 of 8 000 on Trash; the Priest gives 3
  externals / 38 000 ms; the Mage gives 3 / 120 000 ms and receives 2 /
  60 000; the Evoker's support uptime 48 000 ms; `taken10_0` 22 000 for the
  Warrior; span counts 8/1/0/7.

## 0. Decisions

- **`PROTO_VERSION` 25, not "no bump".** The 4a plan called 4b a store
  change only, written before the wire was checked: every card field is
  encoded and the `me` / `tank_pair` grade is computed in the MCP over the
  `Fights` answer, so `am_uptime_pct` there, `trend { measure: am_uptime }`
  and the stored uptime drill cannot exist without a bump. A files-only
  design read through `history_sql` leaves the coach's default path blind
  (that tool is registered only where the binary exists). One bump
  carries everything below; step 5 bumps again (R20 card field +
  `RoleNight`).
- **Card** (`CardPlayer` tail, wire + JSON): `am_uptime_ms: u64`,
  `externals_given: u32`, `externals_given_ms: u64`, `externals_received:
  u32`, `externals_received_ms: u64` — raw scalars like `taken` /
  `mitigated`, so the pct is never stored twice. Derived, written for SQL
  and ignored on read: **`am_uptime_pct()` = `am_uptime_ms as f64 * 100.0
  / duration_ms as f64`** (0.0 at duration 0), emitted by `to_json_in`
  beside `mitigated_pct`. Consumers: `am_uptime_pct` → `tank_pair`, the
  `me` / `peer` row, trend, the SQL scatter (spec §9); externals → the
  `healers` block and the row; ms → "how long", the SQL recipe. Old cards
  read zeros, always emitted (0 is the honest stored value, like `taken`);
  `stats.cards_without_am_uptime` counts them and the `trend` / row
  descriptions say a pre-4b card reads 0 % until `regrade_fights`, like
  `mitigated_pct`'s. Trash cards store the clamped union (engine rule).
- **Rows tier**: `FightRows.uptime: Vec<PlayerUptime { guid, cells:
  Vec<UptimeCell { spell_id, label, kind: MarkKind, src, count, total_ms
  }> }>` — the rollup as the engine gives it, keyed by TARGET, uncapped,
  one block per FRIENDLY player with any cell; `kind` is written as the
  NAME (`MarkKind::name()` / `from_name()`, new — `active_mitigation` …
  like `RoleSpellKind::name`) so spec §9's `kind = 'external'` recipe
  works; details' timeline marks keep writing the code. `support_uptime`
  is NOT stored (derived from the target's `support_buff` cells by `src`).
  `FightRows.coarse: Vec<PlayerCoarse { guid, taken10: Vec<u64>, heal10:
  Vec<u64>, marks: Vec<Mark> }>` — `taken_timeline(guid).coarsen(10)`,
  `heal_timeline(guid).coarsen(10)` buckets, and `timeline(guid).marks`
  (item marks + role spans, one list; `Mark.kind` tells them apart and
  every drill's marks are this list by construction); friendly players
  only, a block for every player with a nonzero bucket or any mark;
  `bucket_ms` fixed at 10 000, not stored. `max_ms` cut (no engine value,
  no consumer); no details-tier taken grid or full span list (spec §6
  amended: the live drill has the 1 s taken series and the coarse list is
  never capped in practice). Budget from the measurement: per raid pull
  uptime 7–12 KB, marks 15–28 KB, buckets 5–13 KB → **+30–50 KB on a
  207 KB p90 rows file**, the same order as 2b's mitigation.
- **Wire v25**: `CardPlayer` + u64, u32, u64, u32, u64 trailing (32 B);
  `TrendMeasure::AmUptime` (code 5; amount = `am_uptime_ms`, `per_sec` =
  pct, so the value field is `am_uptime_pct`); `StoredFight + uptime:
  Vec<StoredUptime { target: String, cell: UptimeCell }>` trailing — **both
  halves for the drilled player: every cell where the player is the TARGET
  and every cell on any other target where the player is the `src`** (a
  self-cast appears once), so "externals given, to whom" and a supporter's
  per-target uptime are answerable over the wire; empty without a drill.
  `UptimeCell` on the wire: `u32 spell | string label | u8 kind | string
  src | u32 count | i64 total_ms`. The stored **Taken** drill's
  `Breakdown.timeline` = the coarse taken series with the marks
  (`bucket_ms` 10 000) — the existing slot. The stored **Healing** drill
  keeps the details tier's 1 s `heal_timeline` on tier 3 (today's answer,
  unchanged) and falls back to `heal10` on tier 2 (new; today `None`).
  `derived_fight` builds the same from the segment through the same
  `coarsen`, so stored = derived stays byte-equal. Live drills stay 1 s.
- **Engine**: `UptimeRow` moves to `model` as `UptimeCell` (zero-dep
  types; `meter.rs` keeps `pub use UptimeCell as UptimeRow` for
  `tests/spans.rs`); `MarkKind::name()` / `from_name()`. No other change.
- **Grading / MCP**: no new `Measure` (tanks stay unranked). `graded_row`:
  `am_uptime_pct`, `externals_given: {count, secs}`, `externals_received:
  {count, secs}` on every row; `tank_pair` gains `am_uptime_pct`; a
  **`healers` block for healer-role subjects** `[{name, hps, overheal_pct,
  externals_given: {count, secs}}]` (spec §7 minus `absorb_efficiency`,
  which is R20; `overheal_pct` = overheal × 100 / (healing + overheal)).
  `trend { measure: am_uptime }` (value field `am_uptime_pct`); the tank
  default stays `mitigated_pct`. `stored_fight { player }` gains `uptime:
  [{target, spell, name, kind, caster, count, secs}]` and its `view: taken`
  (and tier-2 `healing`) drill carries `timeline` (`bucket_secs: 10`,
  `marks`) from the rows tier — **`curve()` divides the LAST bucket by
  `duration_ms − bucket_ms × (n − 1)`**, not a full bucket, so the trailing
  partial point reads as the live drill does. Descriptions updated;
  `history_sql`'s points at `docs/history-queries.md`; the coach
  reference's "live only until 4b" line and the v25 key list.
- **DuckDB**: `players` gains the five scalars, the stored `am_uptime_pct`
  and `am_uptime_pct_sql` = `CASE WHEN duration_ms > 0 THEN CAST(coalesce(
  am_uptime_ms, 0) AS DOUBLE) * 100.0 / duration_ms ELSE 0.0 END` (DOUBLE
  first — the 3b DECIMAL trap), probed like `pct_sql`; view **`uptime`**
  (fight × guid × `spell_id`, `label`, `kind`, `src`, `count`, `total_ms`)
  probed on `["guid","spell_id","total_ms"]`; view **`coarse`** (fight ×
  guid × `taken10`, `heal10`, `marks` lists — unnest per query) selecting
  `c.taken10::BIGINT[]` / `c.heal10::BIGINT[]` and probed on
  `["guid","taken10"]` with **`probe_view`'s rule tightened to
  `starts_with("JSON")`** (the exact comparison would pass `JSON[]`);
  `stats.cards_without_am_uptime` / `rows_without_uptime`; USAGE prose; the
  spec §9 recipes as a first `docs/history-queries.md`. Parity: stored
  `am_uptime_pct` = `am_uptime_pct_sql` = the model on `spans.txt`; Σ
  `uptime.total_ms` per (fight, src, kind external) = the caster's card
  `externals_given_ms`; `Trend { AmUptime }` = SQL; the mixed / pre-4b
  lake opens with the view list unchanged and every new column NULL or 0.
- **Regrade** back-fills; tested by the strip-key golden and a daemon
  regrade test over `spans.txt`.
- **Protected set**: unchanged (AM uptime is not a graded measure).
- **Spec**: §6 amended for the details-tier deviation and `max_ms`; §7 for
  the v25 carriage; §9 gets its recipe file.

## 1. Order and agents

1. **Foundation (delegated)**: `model::UptimeCell` + `MarkKind::name`
   (+ core alias and `uptime()` returning it), records (`CardPlayer` five +
   `am_uptime_pct()`, `PlayerUptime`, `PlayerCoarse`, `FightRows.uptime/
   coarse`, goldens, `golden_without_spans`), v25 (card tail, `AmUptime`,
   `StoredFight.uptime` + `StoredUptime`/`UptimeCell` codec + golden
   bytes), CONTRACT v25 row + R18 "4b" prose.
2. **Parallel**: (A) daemon — `extract()`, `stored_fight` / `derived_fight`
   drills + `uptime`, `trend(AmUptime)`, `spans.txt` through the real store,
   regrade; (B) `crates/history` — probe rule, columns, views, stats, USAGE,
   parity, `docs/history-queries.md`; (C) MCP — rows, `tank_pair`,
   `healers`, trend, `stored_fight` uptime + timeline + the last-bucket
   divisor, descriptions, coach reference; spec amendments.
3. Adversarial diff review, fixes, PR stacked on #23.

Estimate: ~3.5 k lines (3b landed 4.1 k).

## Review log (devil's advocate, 2026-09-04)

Verdict *rethink three shapes, then ship*; all taken. Blocking: the wire
`uptime` block as drafted carried only target-side cells, so a healer's
"externals given, to whom" and a supporter's per-target uptime — the
step's headline — were unanswerable over the wire (B1: `StoredUptime {
target, cell }`, both halves); DuckDB types an all-empty LIST column as
`JSON[]`, which `probe_view`'s exact `!= "JSON"` passes, so `coarse` would
define untyped on an empty lake (B2: `starts_with("JSON")` + `::BIGINT[]`);
the stored Healing drill already serves the details tier's 1 s series on
kills and "the existing slot" would have degraded it to 10 s (B3: details
when present, `heal10` on tier 2). Should-fix: the MCP's `curve()` divides
a trailing partial bucket by a full 10 s (S1: divide the last by the
remainder); `kind` stored as a code with no `MarkKind::name` breaks the
spec's recipe (S2: the name); `UptimeRow` is imported by `tests/spans.rs`
(S3: alias); the budget prose was wrong in both directions — "~20 cells"
3× high, "~30 marks" 2–3× low for tanks — replaced by the gawk measurement
(S4); spec §6 amended, `coarse` friendly-only, `history_sql` description
(S5). Nits: the grade is MCP-side, not the daemon's; the un-regraded 0 %
said in the descriptions; `overheal_pct` defined.

## Second review log (adversarial diff review, 2026-09-04)

Verdict *not mergeable as-is; mergeable after fixes*, all applied.
Blocking: the MCP's `curve()` sized its LAST point by `duration −
bucket_ms × (n − 1)`, but the engine sizes the grid by the last EVENT, not
the fight — a tank whose last hit landed at 120 s of a 300 s kill stores
13 taken10 buckets and the 13th was divided by 180 s; on a live open
segment the last point decayed every poll; on Trash a tiny positive tail
inflated (B1: the tail is capped at the bucket's own width, the "< 1 ms →
chunk span" fallback kept; the 25 s / 3-bucket partial-tail answer still
holds). Should-fix: `extract()` filled the five v25 card scalars for
enemy roster players while `uptime[]` / `coarse[]` are friendly-only, so
on an arena card Σ `uptime.total_ms` per caster ≠ the enemy healer's card
externals (S1: enemies store zeros so the Σ identity holds on arena lakes
— stated in CONTRACT's v25 row and the `history` USAGE, gated by an
arena.txt copy carrying an enemy-cast Pain Suppression); `history {
players: all }` roster rows lacked `am_uptime_pct` / `externals_given` /
`externals_received`, so the coach reference's "every me / peer / roster
row" was false (S2: the three keys, the same helpers as `graded_row`);
the stored Healing drill fell back to `heal10` whenever the details tier
lacked the guid, even with the file present (S3: tier-3 fallback only
when details are absent, `details.is_none()`; a present-but-silent file
answers None as before 4b); the USAGE did not say that
`cards_without_am_uptime` counts cards with a SPECCED player lacking the
key, spec-less cards in neither count, so 376 ≠ 408 on a real lake read
as corruption (S4); CONTRACT's v24 row still said the Taken timeline
differs live vs stored "until the coarse series lands" (S5: "landed in
v25"). Nits: `stored_fight`'s `uptime` is "absent when empty (a pre-4b
record is always empty until regrade_fights)"; spec §7's heading is now
`PROTO_VERSION` 21 → 25. The real 408-fight lake was opened read-only on
this branch: `views` unchanged, `stats` `cards_without_am_uptime` 376 /
`rows_without_uptime` 408, every `players` row `am_uptime_pct_sql` = 0
(un-regraded, as designed). The pre-existing `real_log_support` failure
on the 2026-09-02 raid log (the share-pairing assert, reproduced on
role-pivots-7 unchanged) is not this step's.
