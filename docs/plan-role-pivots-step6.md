# Role pivots, step 6 — implementation plan (R21 stacked-debuff conditioning: "what did X hit for at N stacks of Y")

Scope: the coach's friction report 18 (2026-09-05, Ruby Life Pools +14,
`~/Documents/wow-coach/wowdps-history-mcp-retest-18-2026-09-05.md`). Three
group wipes on one Deepstone Earthshaper pack; the player asked whether
Tectonic Strike's three stacks made the tank vulnerable; nothing in the MCP
surface could answer it, and the coach published a wrong call ("a
2k-per-tick non-factor") before hand-rolling the answer in awk: Crushing
Smash on the tank went from a 231k mean at 0 stacks to 455k at 3, with a
622k max against a ~630k pool — a one-shot. The signal is *exactly* what a
whole-pull mean flattens.

This step adds one ruling, R21, in the shape of R17–R20: parse the `_DOSE`
events the engine currently drops, keep a per-victim stack ledger for
hostile debuffs, bucket every Taken hit by the level of each debuff open on
the victim at the moment it lands, and expose the buckets on the drill, the
rows tier and SQL. Two PRs: **6a** engine + fixture + CONTRACT (no wire
change), **6b** store + `PROTO_VERSION` 27 + MCP + DuckDB + parity. Branch
`role-pivots-10` off `main` (step 5 has merged).

## Facts (the code and the live log, 2026-09-05)

- **The stack dimension is absent from the whole pipeline, not just the
  store.** `parser.rs` has no `_DOSE` arm: `SPELL_AURA_APPLIED_DOSE` and
  `SPELL_AURA_REMOVED_DOSE` fall through to `Event::Other`. No model type
  carries a level; `UptimeCell` is `{spell_id, label, kind, src, count,
  total_ms}`; R18 spans are open/close intervals on *Buff* auras in the
  curated role table; R20 keys on the absorb table. R17's Taken by-ability
  rows are one mean per `(victim, damage spell)` over the segment —
  `Row` has `amount` and `hits`, no `max`. `FORMAT-NOTES.md` :238 already
  says stacks "only appear on `_DOSE` events" and that APPLIED's trailer
  is an absorb amount, never a stack count (the R20 rule the parser
  comments at :1185 / :1199 restate).
- **Census, tonight's live log** (`WoWCombatLog-090526_174144.txt`, the
  Ruby Life Pools session, Steam prefix): 1899 `SPELL_AURA_APPLIED_DOSE`
  and 1080 `SPELL_AURA_REMOVED_DOSE`, **every one 14 fields** — the 13
  aura fields plus a trailing integer. No `_DOSE` line carries an absorb
  trailer (0 of 1899 wider than 14). The trailer is the **new running
  total**, on both events: a Hellbent Commander buff `REMOVED_DOSE …,3` →
  `…,2` → `…,1` counts down, `APPLIED_DOSE` counts up (Tectonic Strike
  `DEBUFF,2` then `DEBUFF,3` 1.6 s later). Max observed level 60 (a
  buff). 1698 of the 1899 applied doses land on players; **92 are
  DEBUFFs, 11 of them on players — 9 Tectonic Strike, 2 Cold Claws**. So
  the conditioning population is tiny per pull and the buff side is the
  bulk, which the ruling ignores.
- **Tectonic Strike's own sequence on the tank** (one application): the
  Earthshaper's `SPELL_CAST_SUCCESS`, then `SPELL_AURA_APPLIED …,DEBUFF`
  at 17:45:23.682 and its `SPELL_ABSORBED` lines at the **same
  millisecond, after the apply**; the `_DOSE` for level 2 follows 2.4 s
  later. Log order within a timestamp is application first, damage second
  — so a stacking hit lands at its *new* level. Over the session the spell
  produced 15 APPLIED, 9 APPLIED_DOSE, 3 REFRESH, 15 REMOVED, 20 DAMAGE,
  19 MISSED, 30 ABSORBED: REFRESH at the cap happens (3 of them), and
  every application on the tank was eventually removed, so a level ledger
  balances.
- No committed fixture carries a `_DOSE` line (0 in all nine).
- Meter template: R18's `open_spans: HashMap<SpanKey, OpenSpan>` keyed
  `(raw target, spell id, raw caster)` with `note_span` / `close_span`
  (:2155 / :2208), the retroactive open-at-segment-start rule for a
  refresh or removal with no open key, the passive gate on every call
  site, raw-guid keying with the owner fold at read time, `SPAN_CAP` with
  newest-dropped. R17's Taken arm is where every hit on a friendly
  destination is recorded a second time — the one place a hit can consult
  the victim's ledger.
- Rows tier (`proto/src/history.rs` `FightRows`): `mitigation`, `support`,
  `uptime`, `coarse`, `shields` — each "one entry per player, empty on an
  older rows file, `regrade` fills it". The DuckDB side defines each view
  only after a probe proves the field exists and is typed; `stats` reports
  `rows_without_<x>`. `tests/parity.rs` is the gate. `PROTO_VERSION` is 26.
- MCP (`mcp/src/tools.rs`): the Taken drill on `fight` / `breakdown` /
  `stored_fight` returns `by_ability` (capped at 16 stored, rest rolled
  up), `by_target`, a `mitigation` object and, since v25, a `timeline`
  with marks. Tool descriptions are the coach's only documentation.

## 0. Decisions

- **Admission is a data property, not a table.** R21 conditions on
  *every DEBUFF aura on a friendly player whose source is not a player or
  a player's pet* (a hostile NPC, or an environment/nil source). No
  generated table, no curated list, no census gate: stacking
  tank-vulnerability debuffs are new every patch and the coach cannot
  wait for a regeneration. The class veto is irrelevant (auras are never
  class signals, R8) and the R18/R20 tables are not consulted — a debuff
  that happens to be in neither is exactly the point. Player-sourced
  debuffs on players (arena, mind control, a Warlock's own Soulburn) are
  out: the question is "what did the *enemy's* debuff do to us".
- **The ledger.** Per segment, `debuffs: HashMap<(raw victim, spell id),
  DebuffState { level: u16, src: String, max_level: u16, changes: u32 }>`
  and the per-victim *stack cells* below. Level transitions, in log
  order: `AuraApplied` (Debuff) → 1; `AuraDose { stacks: n }` (either
  `_DOSE` family) → n; `AuraRefresh` → unchanged; `AuraRemoved` → 0
  (entry dropped). **A dose, refresh or removal with no open entry opens
  one at its stated level** (a refresh: level 1) — the debuff predated
  the segment; R18's retroactive rule, minus the "at segment start" span
  since R21 keeps no intervals. Every arm goes through the passive gate
  (`open_segment_for_passive`), so an aura after a segment's end lands
  nowhere and lazy = full. The ledger is **raw-guid keyed on the victim**
  and folded onto the owner at read time like everything else on the
  destination side; a debuff on a pet conditions the pet's hits, which R17
  already folds onto the owner — stated, and the fixture has one.
- **The cells.** In R17's Taken arm, after the hit is recorded, for each
  open ledger entry on the raw destination: `stack_cells[(victim)]
  [(damage spell id, aura spell id, level)] += {hits: 1, sum: amount, max:
  max(amount)}`, `amount` being R17's Taken amount (R1's amount +
  absorbed — the shield-eaten part is still a hit that landed). A `MISSED`
  line is not a hit and records nothing (a dodge at 3 stacks is R17's
  business). A hit with no open debuff on the victim records no cell.
  The damage-spell label follows R17's by-ability key (swings are
  "Melee", pets' sources fold as R5 says). Per-victim cap
  `STACK_CELL_CAP = 512` distinct cells, newest-dropped with a `dropped`
  counter (the R12/R18 rule, stated in CONTRACT); 92 debuff doses in a
  whole session says a real pull is tens of cells, never hundreds.
- **Level 0 is derived, never recorded.** For a given `(victim, damage
  spell, aura)`, the level-0 row is the unconditioned Taken by-ability
  row minus the sum of the level ≥ 1 cells: `hits` and `sum` exactly;
  **`max` at level 0 is `None`** — the meter does not know which
  unconditioned hits fell outside the aura, and a silent maximum-of-all
  would be the wrong number in the one row it must not be wrong (the R20
  precedent: `absorb_wasted` is `None`, never 0). The coach's table needs
  the max at the *top* level, which is exact. A follow-up could record a
  per-`(victim, damage spell, aura)` "not open" cell by iterating the
  victim's seen-aura set on every hit; it costs a set walk per hit for a
  row nobody has asked for yet.
- **What the drill shows.** A debuff is *stacking* when its `max_level ≥
  2` on that victim in the segment. The Taken drill lists every stacking
  debuff on the player as `stacking_debuffs[] { spell_id, label, src,
  max_level, hits_conditioned }` so the coach can see what is askable;
  asked with `conditioned_on: <spell id | label>`, each `by_ability` row
  gains `stacks[] { level, hits, mean, max }` for levels 0..max_level
  (level 0 derived, `max: null`), rows with no conditioned hit omitted.
  A non-stacking debuff (max level 1) is still conditionable — level 0
  vs 1 is a real question for Hellbent Commander-style debuffs — it just
  is not *listed*; passing its id works.
- **Timeline marks are untouched.** Nothing on the graph. A stack level
  series on the coarse timeline is the GUI item (roadmap §2), not this.
- **Parser.** `SPELL_AURA_APPLIED_DOSE` and `SPELL_AURA_REMOVED_DOSE` →
  `Event::AuraDose { src, dst, spell, aura_type, stacks: u16, removed:
  bool }`: 14 fields, `aura_type` at 12, `stacks` at 13 parsed as an
  integer, `Other` when absent or non-numeric (never gate on width — the
  R20 lesson, and the census says 14 always, but a 15-field line with an
  absorb trailer would still read its stacks at 13). `removed` is kept
  for the record; the meter treats both as "level is now n". `Event::
  Other` for the buff side is NOT the shape: the parser emits every
  dose and the meter's Debuff-on-friendly filter decides — a parser
  filter would make the fixture's negative controls untestable.
- **Wire (6b): `PROTO_VERSION` 27.** The `Breakdown` drill reply gains,
  for `View::Taken`, `stacking: Vec<StackingDebuff>` and the by-ability
  rows' `stacks: Vec<StackCell>` populated only when the `Watch`/`Drill`
  cursor carries `conditioned_on: Option<u32>` (spell id; the MCP resolves
  a label to an id from `stacking_debuffs` first). Golden bytes in
  `proto/tests/codec.rs`; the socket renames.
- **Store (6b).** `FightRows.stacks: Vec<PlayerStacks { guid, dropped,
  debuffs[] { spell_id, label, src, max_level }, cells[] { damage_spell_id,
  damage_label, aura_spell_id, level, hits, sum, max } }>` — the raw
  per-level cells, never the derived level 0, so SQL derives it the same
  way the daemon does. Empty on a pre-6b rows file; `regrade` fills it.
  No card field: nothing to rank by. `stored_fight { player, view:
  "taken", conditioned_on }` answers from the rows tier exactly like the
  live drill.
- **DuckDB (6b).** View `stacks` (fight × victim × damage spell × aura ×
  level → hits, sum, max) defined after the typed-field probe (LIST of
  STRUCT; reject any `JSON`-typed shape, the 4b lesson); `stats` reports
  `rows_without_stacks`. `docs/history-queries.md` gains the coach's
  question verbatim as a recipe (level-0 row derived with a `coalesce`
  against `taken_spells`). `tests/parity.rs`: for every stored fight and
  player, the daemon's `stacks[]` per `(damage spell, aura, level)` equals
  SQL's, hits/sum/max.
- **The coach's secondary note** (raw-log damage offset): `FORMAT-NOTES.md`
  gains a "reading amounts by hand" paragraph naming the end-indexed
  offsets the parser uses, so any awk fallback stops guessing. Docs only.
- **No CONTRACT change beyond R21 + the `Event` enum + the drill/rows
  shapes.** R17's totals, R18's spans, R20's ledger and every existing
  golden are untouched: the fixture parity gates prove it (all nine
  fixtures unchanged byte-for-byte in expected values).

## 1. Ruling text (for CONTRACT.md's table, R21 "Stacked-debuff conditioning")

**The call:** every Taken hit on a friendly is bucketed, per hostile
DEBUFF open on the victim at that moment, by that debuff's level;
`_DOSE` sets the level, APPLIED = 1, REMOVED = 0, REFRESH keeps it, a
dose/refresh/removal with no open entry opens one at its level; cells are
`(victim, damage spell, aura, level) → {hits, sum, max}`, per-victim cap
512 newest-dropped; level 0 is derived (`max` unknown); nothing on the
timeline, the card, or R17's totals.

**A conflicting change would:** hide a one-shot at 3 stacks under a
whole-pull mean, or make the level-0 max a number the meter never
measured.

## 2. Fixture — `stacks.txt` (goldens `stacks.expected.md` / `.tsv`, `check.awk`)

One dungeon trash segment plus a dead zone, three players + a pet:

1. A tank taking Crushing Smash at 0 stacks (two hits), then Tectonic
   Strike APPLIED (level 1, one hit), `_DOSE 2` (one hit), `_DOSE 3` (two
   hits, one of them the max), REFRESH at 3 (level unchanged, one hit),
   `REMOVED_DOSE 2` (one hit), REMOVED (one hit at 0 again). Goldens: per
   level hits/sum/max, and the derived level-0 row = by-ability total
   minus levels 1–3.
2. Tectonic Strike's *own* damage conditioned on itself — the hit that
   accompanies the `_DOSE` lands at the **new** level (log order).
3. A second stacking debuff open at the same time on the same tank (two
   ledger entries → one hit lands in two cells; Σ over auras ≠ Σ hits,
   stated in the goldens).
4. A `_DOSE 4` with no prior APPLIED on a healer (a pre-pull debuff): the
   entry opens at 4; the healer's next hit is a level-4 cell.
5. A player-sourced DEBUFF on a player (a Mage's Slow) with a `_DOSE`:
   never a ledger entry, never a cell — the negative control for admission.
6. A stacking **BUFF** with `_DOSE` (Hellbent Commander-shaped, 3 → 2 →
   1 on removal doses): no cell, and `REMOVED_DOSE` on a debuff counts
   *down* elsewhere in the file so the direction is covered.
7. A debuff on the **pet** and a hit on the pet: the cell folds onto the
   owner at read time.
8. A `SPELL_MISSED` (dodge) at 3 stacks: no cell, R17's mitigation still
   counts it.
9. A hit with a nonzero `absorbed` at 2 stacks: the cell's `sum` uses
   amount + absorbed (R17's amount).
10. Auras and doses after the kill / in the dead zone: land nowhere.

`check.awk` runs the same per-`(victim, aura)` level machine and emits
`stack` rows; it fails on a `_DOSE` whose trailer is non-numeric or on a
REMOVED that leaves the level ≠ 0 after the ledger says it dropped.
`tests/stacks.rs` gates lazy = full over the file; `real_log_stacks.rs`
(ignored, `WOWDPS_REAL_LOG`) asserts the invariants on a real log: for
every `(victim, damage spell, aura)`, Σ level ≥ 1 hits ≤ the by-ability
hits and Σ sums ≤ its amount (the derived level 0 is never negative);
every ledger entry that received a `REMOVED` sits at 0; every `_DOSE`
line parsed (count of `Event::AuraDose` = grep count).

## 3. Order and agents

**6a — engine (PR 1, no wire change):**

1. Parser: `Event::AuraDose`, the two `_DOSE` arms, unit tests for 14 /
   15 fields and a non-numeric trailer; `FORMAT-NOTES.md` (the `_DOSE`
   shape, the running-total semantics, the by-hand amount offsets).
2. Meter: `debuffs` ledger + `stack_cells` through the passive gate, in
   R17's Taken arm; `Segment::stacking_debuffs(guid)` and
   `Segment::stack_cells(guid, aura)` read-time folds (owner fold, level-0
   derivation, the cap's `dropped`); `merge` for Overall (sum cells; a
   ledger never merges — Overall's cells are the members' cells summed,
   stated).
3. Fixture + `check.awk` + goldens + `tests/stacks.rs` +
   `real_log_stacks.rs`; CONTRACT R21 + the `Event` surface. `cargo test`
   whole workspace: every other fixture's goldens unchanged.

**6b — store, wire, MCP, SQL (PR 2):**

4. `PROTO_VERSION` 27: `Cursor::Drill`'s `conditioned_on`, the
   `Breakdown` reply's `stacking` + per-row `stacks`, codec goldens.
   `ClientState` passes it through untouched (no frontend renders it —
   the TUI's `no_engine` test and the GUI's headless renders stay green).
5. Rows tier `stacks` + the daemon's `Closed` writer + `regrade`;
   `MemBackend` mock feeds it.
6. MCP: `fight` / `breakdown` / `stored_fight` with `view: "taken"` gain
   `stacking_debuffs[]` always and `conditioned_on` (id or label);
   `by_ability[].stacks[]` with the derived level 0 (`max: null`); tool
   descriptions rewritten so the coach finds it — "ask this when a player
   asks about a stacking debuff".
7. DuckDB `stacks` view + probe + `stats` + the history-queries recipe;
   `tests/parity.rs` extended.
8. `docs/spec-role-pivots.md` §4.6 (R21) + §6/§7/§9 rows; `docs/roadmap.md`
   §1a's order gains "(6) R21 stacked-debuff conditioning"; the OKF
   bundle (once #27 merges): `rulings/r21.md` scaffolds from the table,
   `fixtures/stacks.md` by hand.

One agent per numbered item is the step-5 shape; 1–3 serialize (the
fixture needs the parser, the goldens need the meter), 4 and 5 can run
side by side after 3, 6 and 7 after 5.

## 4. Open questions for review

- **Environment-sourced debuffs** (nil src, e.g. a puddle) — admitted by
  "source is not a player or pet"; is a nil `src` string acceptable on
  the wire, or should it read `"Environment"` like R17's by-target does?
- **`max_level ≥ 2` as the listing threshold** — the alternative is to
  list every conditionable debuff and let the coach filter, at the cost
  of a long list on a pull with twenty one-stack debuffs.
- **Cap 512** — generous by a factor of ten on tonight's evidence; the
  cost is the `dropped` bookkeeping either way.
