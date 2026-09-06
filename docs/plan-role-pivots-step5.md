# Role pivots, step 5 — implementation plan (R20 shield ledger, `absorb_wasted`, `RoleNight`)

Scope: the last step of `docs/spec-role-pivots.md` §11 — ruling R20 (§4.4)
in the engine with its fixture, `absorb_wasted` / `absorb_efficiency` on
the card and `shields[]` on the rows tier, the healer set's efficiency in
the MCP's `healers` block, the DuckDB `shields` view, and the
`HistoryQuery::RoleNight` fixed question (§7). `PROTO_VERSION` 26. Branch
`role-pivots-9` off `role-pivots-8` (4b).

## Facts (explorers + the real logs, 2026-09-04)

- Parser (`crates/core/src/parser.rs`): `AuraApplied` / `AuraRefresh` /
  `AuraRemoved { src, dst, spell, aura_type }` — `AuraRefresh` already
  exists (4a-ii), all three read `aura_type` at index 12 and **deliberately
  skip the trailing amount** at index 13 (comments at :1165 / :1178:
  "never read it as a stack count"). `Event::Absorbed { src, dst, absorber,
  spell: Option<Spell>, absorb_spell, amount }` is end-indexed (19 or 22
  fields). FORMAT-NOTES :236-247: auras are 13/14/15 fields, 13 = optional
  absorb amount; census 32076/590/32 (APPLIED), 29910/240/24 (REFRESH),
  21225/414/30 (REMOVED).
- Meter (`meter.rs`): the `Absorbed` arm (:2815) short-circuits
  `NON_HEALING_ABSORBS` (114556, 31850, 31230, 115069) into `stagger`
  through the passive gate, else records Healing on the absorber and
  `absorbed_credit[guid] += amount` (**via `segments.last_mut()`, not the
  passive gate**). `absorbed_healing(guid)` is the card's `absorbed`. R18's
  span key `(raw target, spell id, raw caster)` with `note_span` /
  `close_span` and the read-time owner fold is the template; `absorb()`
  merges raw-keyed per-player maps by summing.
- **Real log** (the Aug 1 session, 144 MB, CRLF): `REFRESH`'s trailing
  amount is the shield's **new running total**, not a delta (Blood Shield
  84753 → 127428 → 170173); `REMOVED` carries the remaining (a Power Word:
  Shield applied 9588 and removed 9588 fifteen seconds later = fully
  wasted); REMOVED with a trailer is common (Soul Leech 96, Anti-Magic
  Shell 47, Chrono Ward 21); many non-shield buffs also carry a nonzero
  trailer (Feast of Souls, Soul Fragments), so **the ledger keys on
  SPELL_ABSORBED naming the shield, never on the trailer alone**. No
  committed fixture has a REFRESH or REMOVED with an amount; `sample` /
  `taken` / `support` each carry one APPLIED with an amount and 3–6
  `SPELL_ABSORBED` lines. `check.awk` already emits `absorbheal`.
- Store: `Store` is a sorted `Vec<FightCard>` with no role index;
  `progression()` folds nights with `bucket_start(start_utc_ms, tz_min,
  cutover)` keyed by `day_utc_ms`; the MCP's `nights[]` emits `date`,
  `night_local`, `day_utc_ms`, `pulls`, `kill`, `kills`, `best_pct`.
  Query tags 0–2, answer tags 0–5 in use. `Lake::progression()` is UTC-day
  only. The MCP's `tank_pair` lacks the spec's `boss_share`; `healers`
  (4b) lacks `absorb_efficiency`. No frontend renders any of this.
- 4b's `graded_row` blocks are the vocabulary a night roster reuses.

## 0. Decisions

- **Absorb-spell gate (the table)**: the ledger admits a Buff aura only
  when its spell id is in a GENERATED `core/src/absorb_spells.rs`
  (`tools/gen-absorb-spells.sh` → `tools/extract/src/absorbgen.rs`:
  every spell with a `SpellEffect` row whose `EffectAura` is 69,
  SCHOOL_ABSORB, out of the local install — deterministic per build, the
  R8/R12/R18 generator shape with a sortedness test) — never on the
  trailer alone, because Feast of Souls, Soul Fragments and every
  `Second Wind …,BUFF,0,0` carry one. The fixture's shields (17, 11426,
  77535) must resolve or the generator fails; a `SPELL_ABSORBED` naming a
  spell outside the table still ledgers (`known = false` applied, its
  consumed counted) so an un-generated build never loses healing. If the
  install lacks the table (a machine without the client), the committed
  file is what ships, like `role_spells.rs`.
- **R20 ledger (engine)**: per segment, raw-keyed `shields: HashMap<
  ShieldKey = (raw dst, spell id, raw caster), OpenShield { applied,
  consumed, remaining, known_applied: bool }>` and a per-absorber rollup
  `HashMap<raw absorber, HashMap<spell id, ShieldRow>>`. Aura src =
  absorber holds universally (census: 0 mismatches across ~60 shield
  spells in both raid logs), so the key matches `SPELL_ABSORBED`'s
  `(dst, absorb_spell, absorber)`. Transitions, auras through the passive
  gate, absorbs AFTER `record()` in the `Absorbed` arm (an absorb is
  combat to the scanner and opens the segment; the ledger then admits it):
  `AuraApplied` with `Some(a)` opens `applied = remaining = a, known`; with
  no trailer opens unknown; an apply while open first closes the old
  shield with `wasted = remaining` when known (double APPLIED without
  REMOVED: Soul Leech 104, PW:S 4 per log). `AuraRefresh` with `Some(r)`:
  `r > remaining` → `applied += r − remaining`; **`r < remaining` →
  `wasted += remaining − r`** (a refresh-down overwrites shield: 6 of 15
  PW:S refreshes per raid log, 69 544 in one Aug 3 case); then `remaining
  = r`; no trailer = no-op. `Absorbed` on an open key: `consumed +=
  amount`; **an over-absorb (`amount > remaining`) raises `applied` by
  the excess** (Frost Shield, Soul Leech, Reversion under-report; the
  identity `applied = consumed + wasted` holds by construction) and
  `remaining = 0`; on a key not open: opens unknown-applied with `consumed
  = amount` (the pre-pull Disc shield). `AuraRemoved`: `wasted +=` the
  trailer when present (PW:S 617/617 removals carry one, `,0` when empty;
  only Guardian Spirit lacks it), else `remaining` when `known_applied`,
  else nothing and `wasted_unknown`; **a trailer off a known balance is
  raise-only** (second review S1): above it `applied +=` the difference
  (stacking shields — Soul Leech, Yu'lon's Grace, Frost Shield — grow with
  no REFRESH line; a real log removes a Soul Leech applied 843 with 3 171
  remaining), below it `applied` stays and the shield closes as `unknown`
  (First In, Last Out shrinks; the row visibly inconsistent, never quietly
  perfect — no transition ever lowers `applied`); the shield closes into
  the row `{applied, consumed, wasted, count, unknown}` where `unknown`
  counts shields whose APPLIED amount was unknown, plus the shrunk ones
  (the waste is still known whenever a removal trailer exists — S2). **Segment close folds open
  shields into their row with `consumed` and `count` only** (no applied,
  no wasted, `unknown += 1`), so Σ `rows.consumed` = `absorbed_healing`
  EXACTLY per player — the fixture-gated identity — and Fel Armor's
  absorbs appear somewhere. `NON_HEALING_ABSORBS` never enter. R8, the
  scanner and segmentation untouched; lazy = full (segment-local, like
  spans).
- **Accessors**: `Segment::shields(guid) -> Vec<ShieldRow { spell_id,
  label, applied, consumed, wasted, count, unknown }>` (owner-folded,
  consumed desc), `absorb_wasted(guid) -> Option<u64>` = `Some(Σ wasted)`
  when at least one closed shield had a KNOWN waste, `None` otherwise;
  `shields_unknown(guid) -> u32` = Σ `unknown`. `absorb_efficiency` is
  derived: `absorbed / (absorbed + wasted)` when `wasted` is `Some`.
- **Parser**: `absorb: Option<u64>` trailing on the three aura events
  (index 13 on widths ≥ 14; the parser is quote-aware so a comma in a
  name never shifts it); the 15-field shape is `BUFF,0,0` (`Some(0)` on a
  buff — the table gate makes it harmless). FORMAT-NOTES: the
  running-total finding, the src = absorber census, the `BUFF,0,0`
  correction.
- **Fixture `shields.txt`** (~90 lines, one kill + a trash tail): a
  Discipline Priest with Power Word: Shield on a Warrior (applied 20000,
  absorbs 6000 + 4000, removed 10000 → wasted 10000), on a Mage (applied
  15000, absorbs 9000 + 6000, removed 0 → wasted 0), on itself (applied
  12000, absorb 5000 → remaining 7000, refresh 18000 → delta 11000 applied
  23000, absorb 15000 → remaining 3000, removed 3000 → wasted 3000,
  consumed 20000: `applied = consumed + wasted` ✓); a refresh-DOWN case
  (applied 10000, refresh 6000 → wasted 4000, absorb 6000, removed 0); a
  re-apply while open (applied 8000, absorb 3000, APPLIED again 8000 → the
  old closes wasted 5000); an over-absorb (applied 5000, absorb 7000 →
  applied 7000, removed 0); a pre-pull shield seen only by absorb +
  removal (`unknown` 1, consumed 4000, removed 2000 → wasted 2000 KNOWN);
  a shield open at the kill (consumed 3000, folds with `unknown`); a
  Brewmaster Stagger absorb (excluded); a Blood DK Blood Shield with two
  running-total refreshes; a shield applied after `ENCOUNTER_END` (lands
  nowhere); a Feast-of-Souls-style non-shield buff with a trailer (never
  a row). Goldens in `shields.expected.md` / `.tsv`; `check.awk` runs the
  same state machine per `(seg, dst, spell, src)` key beside its existing
  per-key span map and asserts `remaining == REMOVED trailer` whenever
  both are known (the self-check B3 lacked); metrics `absorb_applied`,
  `absorb_wasted` (blank = unknown), `shields_unknown`; every expected
  TSV regenerates, pre-existing metrics byte-identical. Gates:
  `tests/shields.rs` + the ignored `real_log_shields.rs`: Σ rows.consumed
  = `absorbed_healing` on every segment, `applied = consumed + wasted` on
  every known row, a census of over-absorbs and refresh-downs by spell
  with the healer set (PW:S, Divine Aegis, Chi Cocoon, Life Cocoon, Void
  Shield) asserted at 0 over-absorbs, no negatives.
- **Card** (v26, `CardPlayer` tail): `absorb_wasted: Option<u64>`
  (`put_opt_u64`, JSON null when unknown) and `shields_unknown: u32`
  (consumer: the `healers` block's caveat); derived, written for SQL,
  ignored on read: `absorb_efficiency` (f64 or null). Consumers: the
  healer's `me` row, `healers[]`, `RoleNight`, `trend { measure:
  absorb_efficiency }` (`TrendMeasure::AbsorbEfficiency` code 6, `per_sec`
  a percentage like `MitigatedPct`, `None` points skipped — the fold is a
  running mean over the points present), the SQL recipe.
- **Rows tier**: `FightRows.shields: Vec<PlayerShields { guid, rows:
  Vec<ShieldRow> }>` for friendly players with any row.
- **`RoleNight`** (v26): `HistoryQuery::RoleNight { encounter, difficulty,
  night: i64 (the `day_utc_ms` `nights[]` handed back), local_cutover_hour:
  Option<u8> }` tag 3; `HistoryAnswer::RoleNight { night: Night, rows:
  Vec<RoleNightRow { guid, name, spec, role, pulls, measure: f64 (mean of
  the per-pull role measure — the Day-bucket precedent), best, taken,
  dtps, am_uptime_pct, overheal_pct, absorb_efficiency: Option<f64> (Σ
  absorbed / Σ (absorbed + wasted) over pulls with known waste — a ratio
  of sums, not a mean of ratios), externals_given }> }` tag 6; the daemon
  folds the night's non-aborted pulls with the same `bucket_start`; rows
  sorted tank / healer / dps then `measure` desc; spec = the night's
  most-played. MCP tool `role_night { encounter, difficulty, night | date,
  bucket }` renders `tanks[]`, `healers[]`, `dps[]` with 4b's key names;
  CLI `wowdps-history role-night`; SQL `Lake::role_night` (UTC nights like
  `progression`) with parity row by row at cutover `None`. `boss_share`
  deferred with the honest reason: it is computable today from
  `rows.mitigation[].taken_sources` matched to the encounter name (R16's
  boss guid is not on the card), which lives on the rows tier the card
  index does not load — a rows-tier fixed question is a later step.
- **MCP**: `healers[]` and the healer row gain `absorb_efficiency` (null
  when unknown), `absorb_wasted`, `shields_unknown`; `stored_fight {
  player }` gains `shields: [{spell, name, applied, consumed, wasted,
  count, unknown}]` via `StoredFight + shields: Vec<ShieldRow>` trailing;
  `trend { measure: absorb_efficiency }`; descriptions; the coach reference.
- **DuckDB**: `players` + `absorb_wasted` (NULL when unknown or pre-5),
  `shields_unknown`, and `absorb_efficiency_sql` = `CASE WHEN absorb_wasted
  IS NULL THEN NULL WHEN absorbed + absorb_wasted > 0 THEN CAST(absorbed AS
  DOUBLE) / (absorbed + absorb_wasted) END` — NULL is the honest old value
  here, never 0; probed view `shields` (fight × guid × spell); `stats.
  cards_without_shields` (specced healers lacking the KEY, distinct from a
  legitimate null); the "absorb efficiency by boss" recipe uses
  `players.absorbed + absorb_wasted` (the card's definition, open shields
  included) and a second recipe drills `shields` per spell.
- **Protected set**: unchanged. **Regrade** back-fills; strip-key goldens.


## 1. Order and agents

1. **Foundation (delegated)**: parser `absorb` + FORMAT-NOTES; `model::
   ShieldRow`, `RoleNightRow`; records (`absorb_wasted`, `absorb_
   efficiency()`, `PlayerShields`, `FightRows.shields`, goldens, strip-key
   test); v26 (card tail, `AbsorbEfficiency`, `StoredFight.shields`,
   `RoleNight` query/answer, golden bytes); CONTRACT R20 row + v26 row.
2. **Parallel**: (A) engine — the ledger, accessors, `shields.txt` +
   goldens + awk, `tests/shields.rs`, the real-log gate; (B) daemon —
   `extract()`, `stored_fight` / `derived_fight`, `trend`, `role_night`,
   tests; (C) `crates/history` — columns, view, stats, `role_night`,
   parity, recipe; (D) MCP — rows, `healers`, `stored_fight.shields`,
   `trend`, `role_night` tool, descriptions, coach reference.
3. Adversarial diff review, fixes, PR stacked on 4b's.

Estimate: ~4.5 k lines (engine + fixture ≈ 4a-ii's 3.5 k, plus the store
side).

## Review log (devil's advocate, 2026-09-05)

Verdict *not ready as drafted*; all findings taken. Blocking: the ledger
opened on any buff with a trailer, contradicting its own rule — now gated
on a generated absorb-spell table (`EffectAura` 69), with an unlisted
`SPELL_ABSORBED` still ledgered as unknown-applied (B1); the gate
"consumed ≤ applied" fails on real logs (Frost Shield, Soul Leech,
Reversion over-absorb) — an over-absorb raises `applied`, the gate is a
census with the healer set at 0 (B2); the fixture's third Priest shield
did not balance — recomputed, with `check.awk` asserting `remaining ==
REMOVED trailer` (B3). Should-fix: refresh-down is waste (S1); `unknown`
means applied-unknown only, waste is known whenever a removal trailer
exists, `shields_unknown` on the card (S2); open shields fold into rows
with consumed so Σ rows = `absorbed_healing` exactly, and the ledger runs
after `record()` (S3); a night's efficiency is a ratio of sums, the mean
precedent kept for the role measure, `boss_share` deferred for the honest
rows-tier reason (S4); the src = absorber census into FORMAT-NOTES (S5).
Nits: `per_sec` a percentage, a `role-night` CLI, the 15-field shape is
`BUFF,0,0`.

## Second review log (adversarial diff review, 2026-09-05)

Verdict *no blocking findings; mergeable after the should-fixes, all
applied*. Should-fix: a removal trailer off a known balance is RAISE-ONLY
— above it `applied` grows (stacking shields grow with no REFRESH line),
below it `applied` stays and the shield closes as `unknown`, the row
visibly inconsistent rather than quietly corrected; the shrink unit case
(applied 10 000, absorbed 2 000, removed 5 000 → consumed 2 000, wasted
5 000, applied 10 000, unknown 1), the real-log gate at `applied >=
consumed + wasted` on known rows and `=` on every spell the census never
saw shrink (First In, Last Out: 311 shrinks, 364 unknown of 382), `check.awk`
kept stricter with the reason stated, the rule in CONTRACT R20, §0 and
FORMAT-NOTES (S1); `role_night` picks the mode spec FIRST and folds only
the pulls played in that role — one denominator per column, a fully
specless player rostered with role `None`, measure 0, all their pulls —
mirrored in SQL (NULL specs ignored by the mode, a LEFT JOIN keeps the
specless, `n.role IS NOT DISTINCT FROM s.role`, `sum(x) / count(*)`
stating the contract) and gated by a hand-built two-night lake under a
real daemon against SQL, every field by bits, with a spec-swap player, a
specless pull, an enemy and an aborted pull (S2); spec §4.4 rewritten to
the shipped R20 and §7's heading to 21 → 26 (S3); the `RoleNightRow` doc
names `mitigated_pct` as the tank measure (S4); `role_night { date }`
prefers an exact UTC match, falls back to `night_local` only when none
matches and refuses an ambiguous fallback naming both nights (S5);
`gen-absorb-spells.sh`'s header says R20, discovered (every `EffectAura`
69), the fixture shields the fail-loud gate (S6); `healers[]` entries
carry `absorb_wasted` (null when unknown) as §0 promised (S7). Nits: the
history-queries recipe casts with `CAST(... AS DOUBLE)`; the store's
extract folds `shields()` once per player for both the card scalar and
the rows tier; the owner's third `Fights` round trip stays — `Status`
carries only `owner_inferred`, never the guid — and says so; CONTRACT R20
states the pet rule (a `Pet-` shield keys raw and folds at read, so an
aura before its `SPELL_SUMMON` is kept; a `Creature-` guardian's aura
before the summon that names its owner is dropped and its later absorb
opens unknown-applied, consistent with taken.txt's pet rule).

Verified sound: the wire (v26's `CardPlayer` trailer, `TrendMeasure` 6,
`StoredFight.shields`, `HistoryQuery`/`Answer::RoleNight` tags 3 / 6,
golden bytes and the fuzz pass), JSON back-compat (a pre-5 card reads
`None` / 0, a stored `absorb_efficiency` is never read back, a missing
`shields[]` is empty), DuckDB typing (the three synthesized columns NULL /
0 / NULL on a pre-5 lake, `shields` probed before it is defined), the
real lake — 408 fights — opens with its views unchanged and
`cards_without_shields` 376, the ledger's transitions match `check.awk`
line for line on every fixture, Σ `consumed` = `absorbed_healing` for any
guid asked (an NPC absorber included), lazy = full = checkpoint-resume,
and Overall = Σ members.
