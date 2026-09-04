# Role pivots, step 2b — implementation plan (Taken in the store, MCP and DuckDB)

Scope: the store, MCP and SQL side of R17, deferred from step 2a
(`docs/plan-role-pivots-step2.md`, PR #16), revised after the
devil's-advocate review (log at the end). After this step a tank's fights
are gradable from stored records alone: the mitigation record and both
Taken drills on every stored fight, tank measures on the card so `history`
/ `trend` / the `me` block answer without opening rows, `view: "taken"` in
the MCP, and DuckDB views that open on an un-regraded lake, with the
parity gate widened. Branch `role-pivots-3` off `role-pivots-2`.

Real store facts (`~/.local/share/wowdps/history/v1`, 389 fights, measured
by the review): a stored `Row` is ~265 B (every field written, nulls
included); the rows tier is 32.5 MB and 122 615 of its rows are recaps —
recaps *are* the tier; the p90 file is a 25-player raid pull at 208 KB; a
real boss pull has ~9 taken abilities and ~5 attackers per player; details
are written **only on kills** (`history.rs:1334`), pins and bests only keep
an existing file; 0 of 389 rows files carry `views.taken` yet (only
`regrade` rewrites an existing fight).

## 0. Decisions

- **Wire bump to 22**, carrying only what has a consumer here: `CardPlayer`
  + `taken`, `mitigated`, `prevented`, `dtps` (the `me` block and the
  `history` list read cards from `HistoryAnswer::Fights`; JSON-only fields
  would cost N `GetFight` round trips and fail on card-only tiers);
  `HistoryQuery::Trend`: `measure: TrendMeasure { Dps, Hps, Dtps,
  MitigatedPct }` **replaces** `view` (same byte count, re-bless the pinned
  golden); `HistoryQuery::Fights` + trailing `role: Option<Role>` (a `Role`
  u8 code is new in `msg.rs`). Step 4 bumps again for marks with `src`;
  accepted.
- **Card measures**: `taken`, `mitigated`, `prevented` (= absorbed_full +
  blocked_full), `dtps` (per-second over R7 duration, the same path as
  `dps` in `extract()`'s roster loop). **`mitigated_pct` is derived**, the
  way `role` is: `CardPlayer::mitigated_pct()` = `mitigated / (taken +
  prevented)` × 100 (one helper shared with `Mitigation::mitigated_pct`),
  written to JSON for DuckDB, ignored on read. Old cards read 0 / 0.0;
  `regrade` fills them; `stats` reports `cards_without_taken` and
  `rows_without_mitigation`.
- **Rows tier, every fight, rows-only** (no details copy — details exist
  only on kills, where rows already hold the same list):

  ```
  FightRows + mitigation: Vec<PlayerMitigation>
  PlayerMitigation { guid, record: Mitigation,
                     taken_spells: Vec<Row>,        // top 16 by amount, the meter's rows
                     other: TakenOther { amount, extra, count, n },   // the rest, rolled up; n = abilities folded
                     taken_sources: Vec<Row> }      // by attacker name, uncapped (~5 per player)
  ```

  The rollup is a **struct, not a fake `Row`** (a `Row` with `spell_id 0`
  and an empty key would collide with Melee and double count in SQL). Σ
  `taken_spells.amount` + `other.amount` = the Taken row's amount is the
  stated identity; `n > 0` tells a reader the list was capped. The cap
  bites only Σ records (keys / overalls with 60+ abilities); on a boss
  pull nothing is folded. Growth at the p90 raid file: (9 + 5) rows × 25
  players × 265 B ≈ +93 KB (+45 %); lake-wide roughly +20 MB. Accepted:
  wipes are where tanks die and recaps already cost more.
- **Protected set**: the owner's best `mitigated_pct` per (group, spec)
  for Tank specs, kills only, beside best dps / hps — and a floor for all
  three: a measure of 0 protects nothing (today every DPS spec protects an
  arbitrary card for "best hps = 0.0"; that quirk goes too) and aborted
  fights never qualify.
- **`me` / `peer`**: every row gains `taken`, `mitigated`, `prevented`,
  `mitigated_pct`, `dtps`; a Tank subject gets `tank_pair: [{name, key,
  spec, taken, mitigated, mitigated_pct, dtps}]` over the fight's friendly
  tanks sorted by taken; tanks stay unranked.
- **`Trend { measure }`**: `TrendPoint.amount` = taken (Dtps) or mitigated
  (MitigatedPct), `per_sec` = the measure's value (dtps, or the pct). A
  day / week bucket folds `per_sec` as a running mean — a mean of per-fight
  pcts, exactly as Dps-by-day is already a mean of rates; the doc and
  CONTRACT say so. The MCP names the JSON field by measure.
- **`Fights { role }`** = the subject's role (`guid`, else the owner); no
  subject (owner uninferred and no guid) → the filter is a no-op and the
  MCP says so in the answer. No `roles` on the card (`FightCard::roles()`
  derives it in memory).
- **MCP**: `view: "taken"` reachable on `fight` / `breakdown` /
  `stored_fight` (drill keys **`by_ability` / `by_target`** — `by_target`
  is what every view emits; no third spelling), plus `mitigation` object
  {record fields, `mitigated`, `prevented`, `mitigated_pct`, `misses:
  {kind: n}`, `other` when capped}; `trend { measure }` (own arg, default
  by role like the spec's table); `history { role }`.
- **DuckDB**: views `taken`, `mitigation`, `taken_spells`, `taken_sources`,
  each defined **only after a `LIMIT 0` probe** shows the field exists in
  the lake (the `role` precedent, `lib.rs:236`), so an un-regraded or
  mixed lake opens; `players` gains the card measures by unnest with
  `mitigated_pct` as a CASE over `mitigated / (taken + prevented)`;
  `role_ranks` unchanged. Parity: `Trend { Dtps }` / `{ MitigatedPct }`
  (`bucket: None`) = SQL; per player Σ `taken_spells` + `other` = Σ
  `taken_sources` = `taken.amount`; SQL `mitigated_pct` = the card's JSON
  one; a mixed lake (pre-2b + post-2b files) opens and answers.

## 1. Order and agents

1. **Foundation (me)**: records + JSON goldens (`CardPlayer`, `Mitigation`,
   `PlayerMitigation`, `TakenOther`, `FightRows.mitigation`), `PROTO_VERSION`
   22 (card fields, `TrendMeasure`, `Role` code, `Fights.role`), CONTRACT
   v22 row and `HistoryQuery` prose.
2. **Parallel**: (A) daemon — `extract()` (roster loop gains Taken for
   `dtps`; mitigation lists with the cap), `stored_fight` / `derived_fight`
   Taken arm from the rows tier for every tier, `trend(measure)`,
   `fights(role)`, protected set with the floor, `stats`, tests (real store
   over `taken.txt`; cap + rollup arithmetic on a synthetic 20-ability
   segment; regrade back-fill; `Trend { Dtps }`; `Fights { role: Tank }`;
   protected set keeps a tank's best pct and drops the zero-hps quirk);
   (B) `crates/history` — probed views, `stats`, parity incl. the mixed
   lake; (C) MCP — `view: "taken"`, `trend { measure }`, `history { role }`,
   tank block, tests over `MockDaemon::fixture_at(taken.txt)` with the
   Protection Warrior (tank subject, `tank_pair` of two) and the Fire Mage
   as owners, `fight` = `stored_fight` byte-equal on the Taken drill.
3. Adversarial diff review, fixes, PR stacked on #16.

Estimate: 3–4 k lines (2a landed 4.5 k against 3–5 k).

## Review log (devil's advocate, 2026-09-03)

Verdict *ship with changes*; all taken. Blocking: the DuckDB views would
have failed to open on the real, un-regraded lake — 0 of 389 rows files
carry `views.taken` and `define_views` is all-or-nothing (B1: probe first,
mixed-lake parity case); the rows budget was ~2× low (a `Row` is 265 B,
not 150) and the details copy was dead weight since details exist only on
kills (B2: rows-only, cap 16). Should-fix: the protected set needed a zero
/ aborted floor (S1); store `prevented` and derive `mitigated_pct` like
`role` (S2); the `(other)` `Row` was a double-count trap — a struct now
(S3); `arg_view` could not say "taken" and `trend` needs a `measure` arg
(S4); `Trend { MitigatedPct }` by day is a mean of pcts, said so (S5);
`Fights { role }` with no subject defined (S6); estimate raised (S7). Nits:
`by_target` not `by_attacker`; the roster loop change touches `content_id`
for a dodged-only player — noted for the regrade test; re-bless the Trend
golden.
