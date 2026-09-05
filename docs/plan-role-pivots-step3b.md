# Role pivots, step 3b — implementation plan (support and the healing split in the store, MCP and DuckDB)

Scope: the records side of step 3, deferred from 3a (`docs/plan-role-
pivots-step3.md`, PR #19), revised after the devil's-advocate review (log
at the end). After this a support spec's fights and a healer's efficiency
are gradable from stored records: the healing split and the support
scalars on the card, the supporter's target table on the rows tier,
`effective_dps` in grading, trend and SQL, and the bump to 23 that carries
the card fields. Branch `role-pivots-5` off `role-pivots-4`.

Engine surface (3a): `Segment::support(guid)`, `support_targets(guid) ->
Vec<Row>` (key = buffed owner guid, amount = damage shares, extra = healing
shares, count = lines), `healed(guid)`, `absorbed_healing(guid)`,
`effective(guid)`, `Spec::support()`, `model::effective`. Verified by the
review: `effective()` folds exactly as the card's Damage row does, and the
card's `duration_ms` is the denominator `finish_rows` uses, so
`model::effective(damage, received, given) / duration` from card scalars
reproduces the meter to 6e-11 on the real lake — **provided every
supporter is on the card** (today the roster is the union of the four
views' rows; a supporter with no row must be added).

Real store (396 fights): 264 B per card player today (the v22 shape ~330
B); the six scalars + the derived `effective_dps` add ~170 B → ~500 B per
player, ~13 KB for a 25-player card, ~3.5 MB lake-wide. The lake is
entirely pre-step-1 (0 cards carry `role`), so every 3b column must read
as its old value, never NULL, on it.

## 0. Decisions

- **Card** (`CardPlayer`, wire tail + JSON, `PROTO_VERSION` 23): six u64 —
  `overheal`, `absorbed`, `support_given`, `support_received` (damage
  shares; healing shares stay on the rows tier), `healed_received`,
  `self_healed` — each with a named consumer: `overheal` / `absorbed` the
  healer's efficiency (`me` row, SQL), `support_*` the `effective`
  arithmetic, `self_healed` and `healed_received` the **tank pair** (a
  tank's own healing and the external healing it needed, side by side —
  the spec §1 tank question) and the `me` row. Plus one **derived, written
  for SQL, ignored on read**: `effective_dps`. **No `support` flag on the
  card**: like `role` it would be written and ignored; SQL derives it with
  a CASE on spec 1473 beside `role_case()`, the MCP from `Spec::support()`.
  Old cards read zeros; `regrade` fills them; `stats` reports
  `cards_without_overheal`.
- **`effective_dps` needs the duration**: `CardPlayer::effective_dps(
  duration_ms) -> f64` = `model::effective(damage, support_received,
  support_given) as f64 / (duration_ms as f64 / 1000.0)` — the same
  expression `finish_rows` uses for `dps` — and a test asserts
  `effective_dps(d) == dps` **bit-for-bit** on every player of the
  `sample.txt` / `taken.txt` cards (no support → identical), which is what
  lets grading rank it with no predicate.
- **Rows tier**: `FightRows.support: Vec<PlayerSupport { guid, given:
  {damage, healing}, received: {damage, healing}, targets: Vec<Row> }>` for
  every friendly player with any support (empty without an Augmentation;
  ≤ raid-size rows per supporter × ~265 B ≈ 6.6 KB per supporter, no cap).
  `Healed` is on the card only.
- **Wire v23**: `CardPlayer` + 6×u64 (48 B); `TrendMeasure + EffectiveDps`
  (code 4); `StoredFight + support: Option<PlayerSupport>` trailing (the
  drilled player's block from the rows tier — the coach reads
  `stored_fight`; `history_sql` exists only where the binary does). No
  other message changes; step 4 bumps again, as 2b accepted.
- **Grading**: `Measure::Effective` in `grade.rs` with `of(p, duration)`;
  `grade()` ranks the DPS role by `effective_dps` **always** —
  `rank_measure: "effective_dps"` for every DPS-role player, one label, no
  "fight has support" predicate anywhere (values on Aug-less fights do not
  move). `dps_pool` keeps the legacy `rank_dps` / `dps_*` block on raw
  `dps`, and the tool description says that block is the one an
  Augmentation's buffs inflate. `share` = effective / Σ effective (= Σ
  damage except the clamp case, which the fixture does not have — said in
  the test).
- **Trend**: `EffectiveDps` (`amount` = effective, `per_sec` = per duration);
  the MCP default for **every DPS-role subject** is `effective_dps` (equal
  to `dps` on Aug-less fights, so nothing old changes; a plain Mage's
  trend is no longer confounded by whether an Evoker was in the raid);
  `dps` stays raw and reachable. Spec §3's table is amended.
- **MCP**: `me` / `peer` / roster rows gain the six scalars, `effective_dps`
  and `support: true` (derived); `tank_pair` gains `self_healed` /
  `healed_received`; `stored_fight { player }` on a supporter returns
  `support: {given: {damage, healing}, received: {…}, targets: [{name, key,
  spec, damage, healing, lines}]}`; `trend { measure: effective_dps }`;
  descriptions; **the wow-coach skill's `references/mcp-tools.md` is
  updated to read `rank` / `rank_share` / `effective_dps`** beside the
  legacy keys (the coach's recipes read the raw block today).
- **DuckDB**: `players` gains the six scalars and the stored `effective_dps`
  by unnest, a derived `support` CASE, and `effective_dps_sql` =
  `greatest(0, coalesce(damage,0) − coalesce(support_received,0) +
  coalesce(support_given,0)) * 1000.0 / duration_ms` — the coalesce is what
  makes an old card read `dps` and the clamp is R19's ruling;
  `role_ranks`' measure for the DPS role is `effective_dps_sql` (so a
  pre-3b card ranks exactly as under v22 — a parity case says so); a
  probed view `support_targets` (fight × supporter × target with columns
  named `damage`, `healing`, `lines` — never `extra`/`count` on
  damage-shaped rows); parity: Σ effective = Σ damage per fight in SQL
  over the fixture lake; `role_ranks` = the grader for every player of
  `support.txt` (the Evoker ranks by its contribution, the Mage below its
  raw dps); `Trend { EffectiveDps }` = SQL; stored `effective_dps` =
  `effective_dps_sql`; mixed and pre-3b lakes open and rank unchanged.
- **Regrade** back-fills; test by stripping the keys like earlier steps.

## 1. Order and agents

1. **Foundation (delegated)**: records + JSON goldens (`CardPlayer` six +
   `effective_dps(duration)` + the bit-for-bit test, `PlayerSupport`,
   `FightRows.support`), v23 (card tail, `EffectiveDps`,
   `StoredFight.support` + `PlayerSupport` wire codec), CONTRACT v23 row +
   store prose, `Measure::Effective` signature change in `grade.rs` (mcp)
   only as far as it must compile.
2. **Parallel**: (A) daemon — `extract()` (roster gains support-only guids;
   overheal from the Healing row's `extra`; the accessors), `stored_fight`
   / `derived_fight` `support`, `trend(EffectiveDps)`, regrade test,
   `support.txt` through the real store; (B) `crates/history` — columns,
   `support_targets` view, `role_ranks` measure, `stats`, parity incl. the
   pre-3b-ranks-unchanged case; (C) MCP — grading on `Effective`, rows,
   `tank_pair`, `support` block, trend measure + default, descriptions,
   the coach skill reference, tests with the Evoker and the Mage as owners.
3. Adversarial diff review, fixes, PR stacked on #19.

Estimate: 3.5–4.5 k lines (2b landed 4.2 k, 3a 4.2 k).

## Review log (devil's advocate, 2026-09-04)

Verdict *ship with changes*. Blocking: `effective_dps_sql` without a
coalesce is NULL on every pre-3b card and `role_ranks` drops NULL
measures — the whole real lake would have lost its DPS ranking on day
one (B1: coalesce + clamp, a parity case); `CardPlayer::effective_dps()`
cannot exist without the duration (B2: `effective_dps(duration_ms)`, the
bit-for-bit equality test). Should-fix, all taken: one `rank_measure`
label (S1); trend default `effective_dps` for the whole DPS role, `dps`
kept raw (S2); the coach's recipes read the legacy block — update the
skill reference and say which block is inflated (S3); the roster must
include supporters with no rows or Σ effective(card) < Σ damage (S4); a
consumer named for every scalar — `healed_received` joins the tank pair
(S5); estimate raised (S6); v23 now confirmed right (S7). Nits: view
columns named `healing` / `lines`; the `support` flag cut as dead weight.
