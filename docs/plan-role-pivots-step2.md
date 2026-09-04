# Role pivots, step 2a — implementation plan (R17 Taken, `PROTO_VERSION` 21)

Scope, after the devil's-advocate review (log at the end): ruling **R17,
damage taken and mitigation** in the engine, a seventh `View::Taken`, the
wire bump to **21** that carries it, the three frontends showing it, and an
R17-only fixture with hand-computed goldens plus a real-log gate. The
store, MCP and DuckDB side is **step 2b** (its own plan and PR, after this
one merges); everything that needs a further wire field (`Fights { role }`,
`Trend { measure }`, `tank_pair`, `RoleNight`) rides step 4's bump, which
the aura-span marks need anyway. Branch `role-pivots-2` off `role-pivots-1`.

Real-log facts (`WoWCombatLog-090226_172412.txt`, 1.5 GB, Heroic raid,
sampled 2026-09-03), confirmed by the review:

- `SWING_MISSED` = 9 prefix + `missType, isOffHand` (11), `+ amount` for
  BLOCK (12), `+ amountMissed, unmitigated, critical` for ABSORB (14).
  `SPELL_*_MISSED` = prefix + spell block + the same tail **+ a trailer
  token that is always present and is `ST` or `AOE`** (15 / 16 / 18);
  `RANGE_MISSED` has the tail and **no trailer** (14 / 15 / 17). Parse
  forward from `missType`; never index from the end.
- Observed kinds: PARRY, ABSORB, MISS, DODGE, IMMUNE, BLOCK, DEFLECT,
  EVADE, REFLECT. RESIST never appears in 5.3 M lines (modeled anyway).
  `DAMAGE_SHIELD_MISSED`: none observed, parsed anyway.
- `blocked` is at suffix offset `s+5` on every damage family (a partial
  block reads `…,-1,1,0,60693,5355,nil` → blocked 60693, absorbed 5355).
- Real boss pull, 22 players: 191 (player, ability) taken pairs, 104
  (player, attacker *name*) pairs, 420 by attacker *guid*.

## 0. Ruling R17 as it will be written into CONTRACT

- **Taken amount = R1's convention: `amount + absorbed`** per damage event
  whose destination is a player or pet (pets fold onto owners, R4), from
  any source. `extra` = the event's `absorbed`. The log's `amount` is
  already post-block, so blocked is *not* added — this is what makes the
  identity exact: per segment, Σ over every actor's Damage `by_target`
  for friendly names = Σ Taken row amounts + Σ `stagger_ticked` (a tick is
  still R1 damage done by the monk; it is only not Taken). (The Damage view records every
  source and target, `meter.rs:1862`; NPC actors exist in `actors`.)
- **Full misses are not damage.** A `*_MISSED` line has no damage twin; its
  BLOCK amount or ABSORB `amountMissed` is *prevented* damage in the
  mitigation record. R3's `SPELL_ABSORBED` is never read by Taken.
- **Stagger (and the other `NON_HEALING_ABSORBS`) are ruled explicitly.**
  R3's premise is that a `SPELL_ABSORBED` amount is already inside the
  paired damage line's `absorbed` field, so a staggered hit is Taken in
  full on the hit (absorbed part included). The staggered portion then
  re-lands as self-sourced `SPELL_PERIODIC_DAMAGE` "Stagger" (124255)
  ticks: **those ticks are excluded from Taken** (they re-deal damage
  already counted) and tallied as `Mitigation.stagger_ticked` for
  information. `Mitigation.stagger` = the `NON_HEALING_ABSORBS` amounts
  consumed on the player, reported and **never added to `mitigated`**,
  since it is a subset of `absorbed`. Purified stagger is therefore
  mitigation that shows as the difference `stagger − stagger_ticked`.
- **Misses count.** A miss is `count += 1, amount 0` on the Taken row and
  on its by-ability and by-attacker rows; the kind split lives in the
  mitigation record. A Taken row is listed when `count > 0` (a player who
  was only dodged still has a row; `rows()`'s `amount == 0 && extra == 0`
  skip does not apply to Taken).
- **`mitigated = absorbed + blocked + absorbed_full + blocked_full`;
  `mitigated_pct = mitigated / (taken + absorbed_full + blocked_full)`**
  (the denominator is everything that was swung with an amount; dodges and
  parries carry none and are counts only).
- **Attacker key = attacker name**, like every other view's `by_target`
  (R5's pet-by-name precedent, `meter.rs:838`); a boss's identity is R16's
  and is joined at extract time (step 2b), never by guid in a row key.
- **Environmental and nil sources**: `ENVIRONMENTAL_DAMAGE` is labeled by
  its `envType` ("Falling", "Lava", …) and a nil source is named
  "Environment"; today both are invisible and would otherwise read "Melee
  from nil".
- **Taken never opens or extends a segment.** The scanner ignores
  `*_MISSED` (`index.rs:840-887`) and keeps doing so; the meter's Missed
  path writes into the open segment only when `end_ms.is_none()` (the
  R16 guard, `meter.rs:1742`), never touching `last_ms`. Damage events
  already opened the segment before their Taken record.
- Taken is a rate view (`is_rate` → DTPS). `Row.extra`'s CONTRACT doc
  line gains "absorbed for Taken".
- Receiver-side healing (`self_healed`, `healed_received`) is **not** R17;
  it goes with step 3's healing split, where "does received include
  consumed absorbs" is ruled.

## 1. Model (`crates/model`)

```rust
pub enum View { …, Deaths, Taken }             // COUNT = 7, index 6, is_rate = true
pub enum MissKind { Dodge, Parry, Block, Miss, Absorb, Immune, Deflect, Evade, Reflect, Resist }
impl MissKind { pub const ALL: [MissKind; 10]; pub fn parse(&str) -> Option<Self>; pub fn name(self) -> &'static str; pub fn index(self) -> usize }
#[derive(Default …)]
pub struct Mitigation {
    pub absorbed: u64,        // partial, from damage events (subset: stagger)
    pub blocked: u64,         // partial, from damage events
    pub absorbed_full: u64,   // ABSORB misses' amountMissed
    pub blocked_full: u64,    // BLOCK misses' amount
    pub overkill: u64,
    pub stagger: u64,         // NON_HEALING_ABSORBS consumed on the player
    pub stagger_ticked: u64,  // Stagger ticks excluded from Taken
    pub misses: [u32; MissKind::COUNT],
}
impl Mitigation { pub fn mitigated(&self) -> u64; pub fn mitigated_pct(&self, taken: u64) -> f64; pub fn merge(&mut self, &Self); pub fn misses(&self) -> u32 }
```

No `taken` / `hits` / `crits` on the record — they are the row's `amount` /
`count` / `crits`. `fmt::view_name` gains `"Taken"`; enum self-tests grow.

## 2. Parser (`crates/core/src/parser.rs`)

- `Event::Damage` gains `blocked: u64` (`s+5`).
- `Event::Missed { src, dst, spell: Option<Spell>, kind: MissKind, off_hand:
  bool, prevented: u64 }` for the five `*_MISSED` families. `prevented` is
  the BLOCK amount or the ABSORB `amountMissed`, else 0; the ABSORB tail's
  `unmitigated` and `critical` are dropped (documented). Unknown
  `missType` → `Other`. Field indexes forward from `missType`; the `ST` /
  `AOE` trailer and the trailer-less `RANGE_MISSED` are both fine that way.
- `ENVIRONMENTAL_DAMAGE`: `spell` stays `None`; a new `env: Option<String>`
  on `Event::Damage`? No — carry the `envType` as a synthetic `Spell { id:
  0, name: envType, school }` so no signature grows and the label rule is
  "spell name, else Melee". Decide in review of the diff; the plan takes
  the synthetic spell.
- `AuraRemoved.absorb` is **not** parsed now (no ruling behind it; the
  parser is not versioned).
- Unit tests: every sampled shape (11/12/14 swing; 15/16/18 spell with
  `ST` and `AOE`; 14/15/17 range; a quoted comma in an NPC name; `nil` and
  `1` off-hand; `critical = 1`), `blocked` on swing and spell,
  `DAMAGE_SHIELD_MISSED`, unknown kind → `Other`, environmental label.
- `FORMAT-NOTES.md` gains a `*_MISSED` section with widths and offsets and
  the trailer rule.

## 3. Meter (`crates/core/src/meter.rs`)

- **Taken is the seventh `ViewStats` slot on the destination actor**,
  written through `Segment::record(dst_guid, View::Taken, label, spell_id,
  school, attacker_name, amount, overkill, crit)` right beside the R1
  record in the damage branch — `Segment::record`, not `Meter::record`
  (`ensure_combat` is what opens segments; the damage event already did).
  `rows`, `breakdown`, `finish_rows` and `absorb` then work unchanged
  except: `rows(Taken)` lists on `count > 0`; enemy flag as R13.
- The Missed branch: when a segment is open (`end_ms.is_none()`), record
  `(dst, Taken, label, …, count 1, amount 0)` and bump the kind counter.
  Never `ensure_combat`, never `last_ms`.
- Stagger ticks: in the damage branch, `spell.id == 124255 && src == dst`
  → no Taken record, `stagger_ticked += amount`. The R1 Damage record is
  untouched (R1 is not reopened: whether a self-tick is "damage done" is
  today's behavior and stays).
- `Segment.mitigation: HashMap<String, Mitigation>` keyed by the **raw**
  destination guid; `Segment::mitigation(guid)` folds pets onto owners at
  read time exactly like `rows` (`resolve_owner`), and `absorb` merges
  raw-keyed entries. Fed by the damage branch (absorbed, blocked,
  overkill), the Missed branch (kinds, full amounts), and the Absorbed
  branch's `NON_HEALING_ABSORBS` early return (`stagger`).
- `engine.render`'s timeline match returns `None` for Taken (the coarse
  taken series is step 4).
- Tests (`crates/core/tests/taken.rs` + in-crate): the fixture goldens;
  the identity over `sample.txt`, `instance.txt`, `arena.txt`, `taken.txt`
  (every segment, Σ Damage by_target[friendly] = Σ Taken); the
  pet-before-summon fold (`mitigation(owner)` includes the pet's misses);
  a miss after `ENCOUNTER_END` changes nothing; lazy = full = checkpoint
  for Taken and mitigation (view loops at `index.rs:1318`,
  `instance.rs:203`, `fixture_totals.rs:20`); scanner output byte-identical
  before/after; `only_recordable_events_open_segments` gains a MISSED
  line; `Overall` sums members.

## 4. Fixture `crates/core/fixtures/taken.txt` (R17 only)

Small enough to hand-compute honestly: ~70 lines, one raid visit, one
encounter (kill), a short trash tail. Three players: a Protection Warrior
(partial block, full BLOCK miss, PARRY, DODGE, MISS), a Brewmaster Monk
(two staggered hits with their `SPELL_ABSORBED` 115069 lines and the
damage lines' `absorbed`, two Stagger ticks, one purify gap), a Fire Mage
(IMMUNE via Ice Block, a full ABSORB miss with its `SPELL_ABSORBED` twin, a
partial absorb, DEFLECT, EVADE, REFLECT, RESIST from an add, one
`ENVIRONMENTAL_DAMAGE`), plus the Mage's water elemental pet taking a hit
*before* its `SPELL_SUMMON` (the B2 case). One boss with health reports
and one add. Every `MissKind` once against a friendly target.

Goldens: `taken.expected.md` (per-ruling derivation with the identity
written out) and `taken.expected.tsv`. `check.awk` gains destination-side
metrics `taken`, `absorbed`, `blocked`, `prevented`, `misses`, `stagger`,
`stagger_ticked` for every player row (fixed shape, always emitted), with
`SWING_MISSED` / `SPELL_*_MISSED` / `RANGE_MISSED` arms; **the sample
golden is regenerated** (review B3: `sample.txt:105` already has a
friendly-destination tick, so Thraxx's `taken` is non-zero) and its diff
reviewed line by line; `instance` / `arena` goldens likewise if they carry
friendly-destination damage. `verify.sh taken.txt` joins the gate; the
`sample.txt` negative control still fails on `corrupt.txt`.

The spec's five-player `support.txt` is retired: steps 3–5 each add their
own small fixture with goldens computed under their own ruling.

## 5. Wire (`crates/proto`) — `PROTO_VERSION` 21

- `view_code` / `view_from`: code 6 = Taken; `enums.rs` list.
- `Breakdown` gains trailing `mitigation: Option<Mitigation>` (embedded, so
  written unconditionally with its presence byte; `put_opt` already does).
- `Mitigation` codec: the six u64 amounts + `misses` as ten u32.
- Nothing else. `CardPlayer` fields, `Fights { role }`, `Trend { measure }`
  wait for 2b / step 4 (review S7/S8).
- CONTRACT: header `PROTO_VERSION = 21`, the v21 version-history row, the
  `Event` / `Segment` / `View` signature blocks, `Row.extra` doc, the R17
  ruling row (§0). Goldens: `codec.rs:751` → 21; view lists; a `Breakdown`
  with `Some(mitigation)` round-trip plus pinned hex; fuzz unchanged.

## 6. Daemon (engine only, in this step)

- `engine.render`: Taken rows/breakdown through the existing calls;
  attach `mitigation` to the drill `Breakdown` when the view is Taken.
- `history.rs extract()`: the seventh view lands in `FightRows.views` via
  `VIEW_KEYS` — add `(View::Taken, "taken")` so the rows tier carries the
  Taken *rows* now (cheap: one `Row` per player) and 2b adds the record and
  the drills. `stored_fight`'s `match view` gets a Taken arm returning rows
  with no drill (2b fills it).
- Tests: `many_clients.rs` view list; a Taken watch over the mock.

## 7. Frontends

- Key **`T`** in both keymaps (the `K`-for-Deaths precedent; `t` is the
  GUI's talent viewer). TUI `extra_tag` `"ab"`, rate view. GUI
  `meter_captions` `("(absorbed)", "taken", "dtps")`, drill caption, the
  `hps`/`dps` label sites read the view. Overlay: `CycleView` ring, name
  table, header/rate labels. Tests loop seven views; the mock gains
  `Mock::fixture_at(path)` so the frontends render `taken.txt` headless.

## 8. Real-log gates (`WOWDPS_REAL_LOG`, ignored)

Conservation over every encounter of the 1.5 GB log; miss-line width
census (every `*_MISSED` line parses to `Missed` or, for unknown kinds,
`Other` — count both); parse-time delta before/after (< 5 %).

## 9. Order of work and agent split

1. **Foundation (me):** model, parser, FORMAT-NOTES, CONTRACT R17 row +
   signature blocks; compiles with stub arms.
2. **Parallel:** (A) meter + core tests; (B) fixture + goldens +
   `check.awk` + regenerated sample golden; (C) wire v21 + proto goldens +
   CONTRACT wire section.
3. **Parallel:** (D) daemon engine + rows-tier view key + tests; (E)
   TUI/GUI/overlay + headless tests.
4. Adversarial diff review, fixes, full workspace under `nix develop`, the
   real-log gates run once with numbers in the PR.

Estimate, given step 1 landed 3× its estimate: **3–5 k lines** for 2a.

## Review log (devil's advocate on the first draft, 2026-09-03)

Verdict was *rethink*; every finding was taken. Blocking: stagger was
double-counted (`absorbed` already holds it and the ticks re-deal it) —
now ruled in §0 (B1); the mitigation map was keyed at write time while
everything else folds at read time, breaking the pet-before-summon case —
now raw-keyed (B2); `sample.txt` does carry a friendly-destination tick,
so the golden regenerates (B3); one 3–4 k PR was not honest against step
1's 2.3 k actual — split into 2a/2b and re-estimated (B4). Should-fix:
attacker rows keyed by name, not guid (S1: 104 vs 420 pairs on a real
pull); rows-tier budget is 10× the spec's figure — 2b decides what lands
on rows vs details (S2); environmental / nil labels (S3); the miss
trailer is always present and is `ST` or `AOE` (S4); a post-END miss must
not write into the closed segment (S5); miss-only players need a row (S6);
`Trend { measure }` vs `view` and the "one bump" claim — dropped from this
step, step 4 bumps again anyway (S7, S8); `mitigated_pct` denominator ruled
(S9). Q3: compact misses array, no duplicated row fields, healing received
moved to step 3. Q5: `T`. Q6: no `boss_guid` on the card; `tank_pair`
deferred. The five-player fixture was retired for an R17-only one.

## Real-log gate results (2026-09-03, `WoWCombatLog-090226_172412.txt`, release)

`cargo test --release -p wowdps-core --test real_log_taken -- --ignored`:
27 boss pulls, the identity dealt = taken + stagger_ticked exact on every
one (Σ taken 4 677 753 005); 93 997 `*_MISSED` lines, all parsed to
`Missed`, none `Other` (no unknown kind in the log); parse + meter of the
27 pulls 5 726 ms.
