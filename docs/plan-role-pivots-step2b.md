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
                     taken_sources: Vec<Row>,       // by attacker name, top 16 by amount (a raid Σ had 74 per player)
                     other_sources: TakenOther }    // the rest, rolled up — see "Rows-tier measurement"
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

## Rows-tier measurement (2026-09-03, post-review)

The review's "~5 attackers per player" budget was measured on boss pulls;
`taken_sources` was uncapped, and a Σ record lists every NPC name in the
instance. Measured before deciding: `real_log_rows_tier_taken_lists_measured`
(`crates/daemon/tests/taken.rs`, ignored, `WOWDPS_REAL_LOG`-gated) imports a
real log into a `MemBackend` store and prints, per stored fight, the rows
file's bytes and each Taken list's share of it (the player's JSON with that
list emptied, subtracted). Log: `WoWCombatLog-090226_172412.txt` — a raid
night (23 Venomous Abyss pulls over two visits) plus a Kings' Rest +10 and
an arena; 28 fights stored. Decision rule: cap `taken_sources` if any Σ
record's rows file grows past 64 KB from the two lists.

Before the cap (`taken_spells` at 16, `taken_sources` uncapped):

| fight | players | rows file | `taken_spells` bytes | `taken_sources` bytes | Σ sources rows | max sources / player |
|---|---:|---:|---:|---:|---:|---:|
| Overall, The Venomous Abyss (visit 1) | 25 | 671 889 | 100 587 | **344 793** | 1 438 | **74** |
| Overall, The Venomous Abyss (visit 2) | 24 | 476 581 | 96 766 | 148 106 | 597 | 28 |
| Key, Kings' Rest +10 | 5 | 65 150 | 17 531 | 15 501 | 63 | 17 |
| Encounter, The Coiled Altar (wipe) | 25 | 259 547 | 97 231 | 114 442 | 498 | 32 |
| typical boss pull (e.g. Nek'zali) | 22–24 | 103–160 K | 50–66 K | 26–31 K | 105–126 | 6–7 |

The rule fires: the raid Σ grows 445 KB from the two lists, 345 KB of it
attacker rows, and one boss wipe (32 attackers on every player — adds)
grows 114 KB on its own. `taken_spells` is already bounded (the raid Σ
folds 2 636 abilities into `other`, the 16 kept cost ~100 KB over 25
players); the by-attacker list had no such bound.

**Decision**: `taken_sources` is capped at `TAKEN_SPELLS_CAP` (16) by
amount, the rest rolled up into a second `other_sources: TakenOther` on
`PlayerMitigation` — the same shape and the same identity as `other` (Σ
kept + rollup = the player's Taken row, on amount / extra / count). One
`cap_taken` in `extract()` serves both lists; the SQL `mitigation` view
gains `other_sources_amount` / `_extra` / `_count` / `_n`, the parity
identity is `Σ taken_sources + other_sources = taken`, and the golden in
`proto/tests/history.rs` shows both folds. After the cap, the same log:

| fight | rows file | `taken_spells` bytes | `taken_sources` bytes | Σ sources rows | folded attackers (Σ `other_sources.n`) |
|---|---:|---:|---:|---:|---:|
| Overall, The Venomous Abyss (visit 1) | 426 223 | 100 587 | 97 373 | 400 | 1 038 |
| Overall, The Venomous Abyss (visit 2) | 425 733 | 96 766 | 95 631 | 384 | 213 |
| Key, Kings' Rest +10 | 65 176 | 17 531 | 15 242 | 62 | 1 |
| Encounter, The Coiled Altar (wipe) | 226 528 | 97 231 | 80 009 | 333 | 165 |

The raid Σ's two lists fall from 445 KB to 198 KB (the file from 672 KB to
426 KB) and every list is now ≤ 16 × players × ~245 B; a boss pull with
≤ 16 attackers per player is byte-identical apart from the 4-field rollup
(~55 B per player, `n: 0`). Note the first table's `rows file` column is
also the whole-file size: recaps still dominate a wipe (The Lost Explorers
at 274 KB carries 34 K of Taken lists).

Also measured: `WoWCombatLog-090126_171845.txt` (a completed Den of
Nalorakk +13 whose log ends inside the visit) stored 0 fights through this
harness — the keyed encounters are by design members of a Σ that had not
closed by end of file. Not a rows-tier question; noted for the import
path's EOF handling.

## Second review log (adversarial diff review, 2026-09-03)

No blocking findings; verdict *open after fixes*, all applied. The daemon's
`cards_without_taken` could not tell "absent" from a real 0 after decode
and disagreed with SQL's (389 vs 357 on the real store) — deleted, SQL is
the one source (S1). `history { role }` now answers `role_applied` and a
note when there is no subject (S2). DuckDB HUGEINT sums printed as Debug
text — `value_json` maps them, and the parity casts that hid it are gone
(S3). A capped drill now says so: `by_ability_other` / `by_target_other`
in the `mitigation` object, computed from the row amount since `Breakdown`
has no rollup slot (S4). The by-attacker list was uncapped on the strength
of boss-pull numbers; measured on a raid night's Σ it was 345 KB of rows
and 74 attackers per player, so it is capped at 16 with `other_sources`
(S5, numbers above). Retention prose in CONTRACT, the store spec and the
design doc names the tank measure and the floor (S6). The review also
quantified the floor on the real store: four dead cards lose protection,
nothing is evicted or demoted on the next pass. Kept from the review's
verification: `per_sec` stays on trend points because the wow-coach skill
reads it.
