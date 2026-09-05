# Role pivots, step 3a — implementation plan (R19 support, the healing split, healing received: engine only)

Scope after the devil's-advocate review (log at the end): the **engine
side** of step 3 — ruling R19 (support attribution), the R2 amendment (the
healing split and healing received), `Spec::support`, the `support.txt`
fixture with goldens, CONTRACT rows and FORMAT-NOTES. **No wire change, no
record change**: `PROTO_VERSION` stays 22; the card fields, the rows-tier
`support[]`, the `stored_fight` / MCP / DuckDB side and the bump to 23 are
**step 3b**, its own plan and PR. Branch `role-pivots-4` off
`role-pivots-3` (PR #17).

## Real-log facts (2026-09-04; raid `090226_172412`, dummy session `080126_225759` with an Augmentation, 13 821 support lines)

- Families: `SPELL_DAMAGE_SUPPORT`, `SPELL_PERIODIC_DAMAGE_SUPPORT`,
  `RANGE_DAMAGE_SUPPORT` (fixture only), `SWING_DAMAGE_LANDED_SUPPORT` (the
  melee support event — there is no `SWING_DAMAGE_SUPPORT`),
  `SPELL_HEAL_SUPPORT`, `SPELL_PERIODIC_HEAL_SUPPORT`; and
  `SPELL_ABSORBED_SUPPORT` (8 lines in 137 MB), which stays `Other` (§0).
- Shape: the underlying family's line **with a 3-field spell block that is
  the buff** (Ebon Might 395152, Prescience 410089, Shifting Sands 413984,
  Bombardments 434481, Fate Mirror 413786) and the **supporter's bare guid
  as the last field** in place of the `ST`/`AOE` trailer. So a melee
  support line is SPELL-shaped (42 fields), not swing-shaped (38): a
  `SWING_` prefix with the swing offsets would read the spell id as the
  amount. Heal-support lines are 36 + 1.
- The amount is the **buff's share** (Ebon Might 21 of a 4 593 Void Ray;
  Ebon Might 1 401 + Prescience 16 908 on a 163 102 Eradicate — shares are
  additive and far under the hit); a proc the Evoker owns outright
  (Bombardments, Fate Mirror) carries the whole hit (7 506 = 7 506).
- **The Evoker's own procs are logged twice**: `SPELL_DAMAGE src = Evoker`
  *and* `SPELL_DAMAGE_SUPPORT supporter = Evoker`, same amount. R1 already
  counts the first; a naive `damage + given` counts it again.
- Support `src` is often a pet or a guardian (3 508 Creature-src + 73
  Pet-src lines; every sampled one has a `SPELL_SUMMON` / owner hint), so
  received must fold onto the owner. `nil` supporters: none in either log.
- Every support line directly follows its hit; none precede a pull's
  first hit (the passive gate stays, for parity, not for a real case).
- The dummy log has 73 NPC-sourced `SPELL_HEAL` lines on players.
- `check.awk`'s `absorbheal` is the absorber-credited `SPELL_ABSORBED` total
  folded exactly as the meter folds — an honest golden for `absorbed`.

## 0. Rulings as they will be written into CONTRACT

**R19 — support attribution.**

- The six families become `Event::Support { src, dst, spell /* the buff */,
  supporter: String, amount, healing: bool }`: the parser pops the trailing
  supporter guid (it must be a guid, else `Other`) and dispatches the rest
  on the base family with the spell-block prefix, so the existing damage /
  heal suffix code reads the amount (`amount + absorbed` for damage, R1's
  convention; `amount − overheal` for heals). Any
  other `_SUPPORT` name (`SPELL_ABSORBED_SUPPORT`: its spell block is the
  buff, the underlying shield is unknowable, so the `NON_HEALING_ABSORBS`
  exclusion cannot be applied) stays a duplicate → `Other`.
- Per segment, per player, raw-guid keyed and folded onto owners at read
  time (a buffed pet's support is its owner's received): `given.damage /
  given.healing` where `supporter` = the player, `received.*` where `src`
  = the player, and per supporter `targets` (by buffed player *name*:
  damage, healing). A support line goes through the passive gate
  (`open_segment_for_passive`), never `ensure_combat`, never `last_ms`,
  never an R8 signal, never a mark. R1 / R2 / R3 do not move.
- **One number for everyone: `effective = damage − received.damage +
  given.damage`.** For a peer it is their net; for an Augmentation with
  no received it is its contribution; a self-supported proc (given and
  received by the same player) cancels, so it is counted once, by R1; and
  Σ effective over a segment = Σ damage — a true partition of the raid's
  damage. There is no `contribution` and no `net` field: `effective` is
  derived by readers from `damage` and the two scalars (the 2b rule:
  derived, never travels). `Spec::support()` (Augmentation) is a flag for
  the card and the trend default only; grading needs no support branch.

**R2 amendment — the healing split and healing received.**

- `overheal` per player = the Healing row's `extra` (R2). `absorbed` per
  player = the absorber-credited R3 total, a dedicated counter written at
  the credit site *after* the `NON_HEALING_ABSORBS` early return and into
  the segment `record` just chose (it may gap-split), so `absorbed ≤
  healing` holds and it equals awk's `absorbheal`.
- `healed_received` = R2 effective healing (`amount − overheal`) landing
  on a player **from any source** (NPC heals included — symmetric with R17
  counting NPC attackers), the `NON_HEALING_ABSORBS` family excluded as R2
  excludes it; **absorbs are not received healing** (a consumed shield is
  damage prevented, already in the R17 record's `absorbed`). `self_healed`
  = the subset with `src.guid == dst.guid`. A heal on a pet is its owner's
  received (raw-keyed, folded at read). Held in a per-player `Healed`
  record beside the R17 record — *not* on `Mitigation`, whose wire codec
  would change (3b decides whether to fold them in under the bump).
- Identity, asserted on every fixture: Σ `healed_received` from player
  sources + Σ absorbs credited on friendly targets = Σ Healing `by_target`
  over friendly names (the Healing rows carry both heals and absorb
  credit, and only player actors have rows).

## 1. Model / parser (foundation)

```rust
Spec::support(self) -> bool                            // Augmentation only
pub struct Support { given_damage, given_healing, received_damage, received_healing: u64 }  // Default, merge
pub struct Healed  { received: u64, self_healed: u64 }                                       // Default, merge
Event::Support { src: Unit, dst: Unit, spell: Spell, supporter: String, amount: u64, healing: bool }
parser::is_support_event(ev) -> bool                    // the six; SPELL_ABSORBED_SUPPORT is not one
```

Parser tests: every family incl. `SWING_DAMAGE_LANDED_SUPPORT` at 42
fields (amount read from the spell suffix, not the swing's), heal support
37 fields, `nil` / non-guid supporter → `Other`, `SPELL_ABSORBED_SUPPORT`
→ `Other`, a self-support line, the fixture's `RANGE_DAMAGE_SUPPORT`.
FORMAT-NOTES: a `_SUPPORT` section (the buff-not-hit spell block, the
share rule, the whole-hit procs, the `_LANDED_` exception, widths) and the
"identical base_amount" sentence corrected.

## 2. Meter

- `Segment.support: HashMap<raw, Support>`, `Segment.support_targets:
  HashMap<raw supporter, HashMap<name, (u64, u64)>>`, `Segment.healed:
  HashMap<raw, Healed>`, `Segment.absorbed_credit: HashMap<raw, u64>`; read
  accessors folding like `mitigation`: `support(guid)`,
  `support_targets(guid) -> Vec<Row>`, `healed(guid)`,
  `absorbed_healing(guid)`, `effective(guid) -> u64`; `absorb` merges all.
- Support branch (passive gate; `learn` both units; supporter named at
  read time through `names`). Heal branch: after the R2 record, on a
  friendly `dst`, `healed_mut(dst raw).received += effective`, `self_healed`
  when `src.guid == dst.guid`. Absorbed branch: the counter after the
  early return.
- Tests (`tests/support.rs` + in-crate): goldens; Σ effective = Σ damage
  and Σ given = Σ received per segment; the healing identity; pet folding
  both ways (buffed pet → owner received; supporter's pet never); the
  self-support proc counted once; NPC-sourced heal counted as received;
  a support line before the first hit attributes to nowhere, full = lazy;
  lazy = full = checkpoint for every new number; scanner byte-identical;
  `sample.txt`'s support pair yields given = received = 29 400 (the golden
  gains the new rows; the awk change regenerates every expected TSV,
  pre-existing metrics byte-identical).

## 3. Fixture `crates/core/fixtures/support.txt`

~80 lines, one kill. Augmentation Evoker (1473) with Ebon Might on a Fire
Mage and Prescience on an Arms Warrior; a Holy Priest (257); the Mage's
elemental. Lines: Ebon Might shares on Fireballs (a small share — the
expected file says the meter *reads* shares and never computes them),
Prescience shares, `SWING_DAMAGE_LANDED_SUPPORT` on the Warrior's swings
(with the plain swing + `_LANDED` twin), a periodic share, one
Bombardments proc logged twice (the double-count case), a Fate Mirror
`SPELL_HEAL_SUPPORT`, the elemental buffed (pet → owner), one
`SPELL_ABSORBED_SUPPORT` (must change nothing). Priest: Power Word: Shield
credited (non-zero `absorbheal`), Flash Heals with overheal, a Renew tick
on herself, one NPC-sourced heal on the Warrior, a Stagger-family absorb
(exclusion gated). Goldens `support.expected.md/.tsv`; `check.awk` gains
`support_given`, `support_received`, `support_given_heal`,
`support_received_heal`, `healed_received`, `self_healed`, `effective` for
every player row; all expected TSVs regenerate, pre-existing metrics
byte-identical; `verify.sh` covers it.

## 4. Real-log gate (ignored, `WOWDPS_REAL_LOG`, the dummy session)

Every support `src` folds to a player (orphans = 0); per `(ts, src, dst)`
Σ support shares ≤ Σ hits (group sums — single pairing is impossible);
Σ effective = Σ damage per segment; the healing identity; a census by
family; parse-time delta.

## 5. Order and agents

1. Foundation (delegated): model + parser + tests + FORMAT-NOTES +
   CONTRACT rulings.
2. Parallel: (A) meter + core tests + real-log gate; (B) fixture + goldens
   + awk.
3. Adversarial diff review, fixes, PR stacked on #17.

Estimate: 2–3 k lines (engine + fixture only).

## Review log (devil's advocate, 2026-09-04)

Verdict *rethink one ruling, then ship with changes; split 3a/3b*.
Blocking: `contribution = damage + given` double-counted the Evoker's own
procs, which the log writes as both damage and support — replaced by one
formula for everyone, `effective = damage − received + given`, derived
and never stored (B1); melee support lines are SPELL-shaped, so the swing
offsets would have read the spell id as the amount — the parser pops the
supporter and dispatches on the base family with the spell prefix (B2);
the healing identity was wrong because Healing rows carry absorb credit
and NPC heals have no rows — ruled: any source counts, identity restated
with the absorb term (B3). Should-fix: `SPELL_ABSORBED_SUPPORT` stays
`Other` (S1); split into 3a engine / 3b records (S2); no `support_targets`
on `Breakdown` (S3); card growth measured at ~262 B/player today — 3b
carries the four scalars only (S4); the absorbed counter's exact write
site (S5); the real-log gate asserts falsifiable things, not Σ given = Σ
received (S6); facts corrected — no nil supporters, no pre-pull support
lines (S7); the fixture's share is a read value, not a computed one (S8);
healing received in its own record, its own SQL columns (S9, amended here
for the wire).
