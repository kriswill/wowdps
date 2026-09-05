# Role pivots — specification (roadmap item 1a)

Status: **planned**, 2026-09-03; step 1 of §11 is on branch `role-pivots-1`
(`docs/plan-role-pivots-step1.md`), the rest is unbuilt. It is
the fast follow to the history store (`docs/spec-history-store.md`, shipped
in PR #12) and expands its parsing, storage, faceting, testing and DuckDB
surface so the store answers questions about **support roles**: tanks,
healers, and the support DPS whose value is what other people do while
buffed — Augmentation Evoker today, plus every spec that gives externals,
raid buffs or damage amplification. Supersedes the sketch in `docs/roadmap.md`
§1a and the "role pivots" table in `docs/history-store-design.md` §9, both
of which now point here.

The problem in one line: every rank, grade, trend and best the store or the
coach produces is a DPS-role number over `damage` / `dps`, and a tank, a
healer or an Augmentation Evoker measured that way is measured wrong. Half
the fix is a **role dimension** and role-relative measures on records that
already exist; the other half is a **parser gap** — damage taken, misses and
blocks, shield lifetimes and buff spans are not modeled at all — and that
half is a set of new CONTRACT rulings with fixture parity, exactly like the
rulings the store itself needed (§4 of the store spec).

## 1. Intent

- Every stored fight carries, per player, **what role they played**, and
  every ranking, median, share and trend the daemon or SQL produces can be
  asked **within a role**: healer among healers, tank against the fight's
  other tank, support DPS by contribution rather than by own hits.
- A **healer** can ask: effective vs overheal, absorbs given vs absorbs
  wasted, externals given (to whom, when), uptime of the buffs they own,
  and how their HPS ranks among the fight's healers.
- A **tank** can ask: damage taken by ability and by attacker, how much of
  it was mitigated and by what (absorb, block, dodge, parry, miss, stagger),
  active-mitigation uptime, self-healing, and — the nearest thing the log
  has to threat — which tank the boss was hitting, and when it swapped.
- A **support DPS** (Augmentation) can ask: damage contributed through
  others, uptime of the buffs they maintain on each target, and a DPS rank
  that counts contribution — while the buffed players' rank is not inflated
  by it.
- All of it for **every fight, wipes included** — tanks die on wipes and
  healers are graded on wipes — so the new measures live on the card and
  the rows tier, never only in details.
- All of it in **both readers**: the daemon's fixed questions and DuckDB
  over the same files must agree (the lake parity gate widens).

## 2. Non-negotiables

Inherited from the store spec §2 (the files are the truth, records are
append-only fields within `v1/`, the daemon writes and everything else
reads, offline, no new dependency) plus:

1. **Nothing here opens, extends or splits a segment.** Miss events, aura
   spans, shield bookkeeping and support attribution are all segment-local
   state like R8/R9/R12; the scanner stays untouched and lazy loads
   reproduce every number exactly. This is gated the same way R12 is.
2. **R1, R2 and R3 do not move.** `damage` and `healing` on every existing
   row keep their meaning and their fixture totals. Every new measure is a
   new field or a new view; a reader on today's rows sees today's numbers.
   Augmentation's contribution is *not* folded into anyone's `damage`.
3. **Role is written into the file at write time, from the spec, and is
   never a second copy in memory.** The spec is the truth; `role` on the
   card is a projection for readers that cannot call `Spec::role` (DuckDB
   and anything else over the files). The daemon and the codec derive it
   from `spec` on demand; the SQL `players` view derives it by spec id when
   the column is missing, so an un-regraded lake answers the same; `regrade`
   rewrites old cards so the bytes carry it too. Nobody guesses a role from
   a class.
4. **Conservation holds and is tested.** Over a fight, the friendly side's
   damage *taken* from enemies equals the enemy side's damage *dealt* to
   friendlies, under the same amount convention (R1's `amount + absorbed`).
   This is the invariant that keeps the Taken view honest, and it is a
   fixture expectation and a SQL parity check.
5. **Curated spell tables are generated, never hand-edited, and never
   committed if they carry Blizzard data beyond ids and names.** The role
   spell table (§5) is a `tools/gen-*` output like `class_spells.rs`.
6. **Schema stays `HISTORY_SCHEMA = 1`.** Fields are only added. A card or
   rows file from PR #12 decodes unchanged and reads as "role unknown, no
   Taken data"; the DuckDB views tolerate the missing columns through
   `union_by_name`.

## 3. Roles

`model::Role` is the game's group-finder classification (Tank / Healer /
Dps) and stays that; it is what the card's `role` says. A second, orthogonal
flag captures **support**: a spec whose logged output is partly what others
do while buffed. Today that set has one member.

```
Spec::role(self) -> Role                 // exists
Spec::support(self) -> bool              // new; true for Augmentation only (hand table
                                         // in model, one line per spec; revisited per patch)
```

Why a flag and not a fourth `Role`: the game ranks Augmentation with DPS,
the group finder slots it as DPS, and every "DPS-role players" filter today
(the coach's `me` block, `dps_count`) must keep including it — the fix is
what number it is ranked *by*, not which bucket it is in. Healer externals
and tank raid buffs are support *acts*, recorded per act (§5); they do not
make the spec a support spec.

Role-relative measures, per role, decided here so the daemon and SQL rank
by the same number:

| Role | Rank measure | Also on the card | Trend default |
| --- | --- | --- | --- |
| Dps | `effective_dps` = damage − received + given (§4.3; equals `dps` when nobody gave support, so the DPS pool ranks it always under one label — step 3b) | `support.received` | `effective_dps` (a plain DPS's trend is then not confounded by whether an Evoker was in the raid; `dps` stays raw and reachable) |
| Dps, support | the same `effective_dps` (its contribution, since it receives nothing) | `support.given` | `effective_dps` |
| Healer | `hps` (R2 effective, absorbs included as today) | `overheal`, `absorbed`, `absorb_wasted`, `externals_given` | `hps` |
| Tank | none ranked; graded on `mitigated_pct` and `dtps` | `taken`, `mitigated`, `dtps`, `self_healed`, `am_uptime_pct` | `mitigated_pct` |

A tank is not ranked against the other tank on a raw number — the boss
chooses who takes the damage — so the `me` block reports the tank pair side
by side (§7) instead of a rank.

## 4. Parser and meter requirements

Four new pieces of segment-local state in `crates/core`, each a CONTRACT
ruling (numbered after R16) with expected values in a new fixture (§8).

### 4.1 Damage taken and mitigation — ruling R17

**Parser.** Three event families become modeled instead of `Event::Other`:

- `SWING_MISSED`, `SPELL_MISSED`, `SPELL_PERIODIC_MISSED`, `RANGE_MISSED`,
  `DAMAGE_SHIELD_MISSED` → `Event::Missed { src, dst, spell: Option<Spell>,
  kind: MissKind, off_hand: bool, amount: u64 }`. `MissKind` is the log's
  `missType`: `Dodge | Parry | Block | Miss | Absorb | Immune | Deflect |
  Evade | Reflect | Resist`. `amount` is the trailing `amountMissed` where
  present (a full absorb or a full block carries it; a dodge does not); a
  `BLOCK` miss is a *full* block, an `ABSORB` miss is a fully absorbed hit
  — the latter is the one case where a `SPELL_ABSORBED` line is also
  written, so R3 keeps its "one source per direction" and the Taken grain
  reads the miss, never both.
- The `blocked` field on every damage event (`SWING_DAMAGE`, `SPELL_DAMAGE`,
  `SPELL_PERIODIC_DAMAGE`, `RANGE_DAMAGE`, `DAMAGE_SHIELD`,
  `ENVIRONMENTAL_DAMAGE`), today parsed and dropped, lands on
  `Event::Damage.blocked: u64` (partial block). `absorbed` is already there.
- `SWING_DAMAGE_LANDED` stays `Other` (R1 double-count trap); the target's
  advanced block on it is not needed because `SWING_DAMAGE` already names
  the target.

**Meter** (as ruled in CONTRACT R17 and built in step 2a; the first draft
of this section said `+ blocked` and a separate `taken_rows` API — both
were changed by the step 2 review, `docs/plan-role-pivots-step2.md`).
Taken is the **seventh `View`**, recorded on the *destination* actor's
view slot beside R1's record on the source, so `rows`, `breakdown`,
`finish_rows` and the R10 `absorb` merge serve it unchanged:

```
Taken row (View::Taken):   amount = Σ (amount + absorbed)   // R1's convention exactly: the log's amount is
                                                            // post-block, so blocked is NOT added — and then
                                                            // Σ every actor's Damage by_target over friendly
                                                            // names == Σ Taken rows + Σ stagger_ticked, exactly
                           extra  = Σ absorbed               // partial absorbs
                           count  = hits + ticks + misses    // a miss is count 1, amount 0
                           crits  = crits
by-spell  = taken by ability;  by-target = taken by ATTACKER NAME (R5's pet-by-name precedent, never a guid)
Mitigation record, per player (model::Mitigation, raw-guid keyed, folded at read time like rows):
                           absorbed, blocked (partial, from damage events)
                           absorbed_full, blocked_full (ABSORB / BLOCK misses' prevented amounts — never Taken)
                           overkill, stagger, stagger_ticked, misses[MissKind]
                           mitigated()     = absorbed + blocked + absorbed_full + blocked_full
                           mitigated_pct() = mitigated / (taken + absorbed_full + blocked_full)
```

- **Stagger, cheat-death and the other `NON_HEALING_ABSORBS`.** R3's
  premise is that a `SPELL_ABSORBED` amount is already inside the paired
  damage line's `absorbed`, so a staggered hit is Taken in full on the hit.
  `stagger` = those amounts consumed on the player — a *subset* of
  `absorbed`, reported and never added again. The staggered portion then
  re-lands as self-sourced `SPELL_PERIODIC_DAMAGE` "Stagger" (124255)
  ticks, which are **excluded from Taken** (they re-deal damage already
  counted) and tallied as `stagger_ticked`; purified stagger is the
  difference. R2 is unchanged.
- **A full miss is prevented damage, not taken.** A `*_MISSED` line has no
  damage twin; R3's `SPELL_ABSORBED` is never read by Taken.
- **Miss-only players have a row** (Taken lists on `count > 0`).
- **Environmental and nil sources** label by `envType` / "Environment".
- **Receiver-side healing** (`self_healed`, `healed_received`) moved to
  step 3 with the healing split, where "does received include consumed
  absorbs" is ruled.
- **Segment API.** `Segment::rows(View::Taken)`, `Segment::breakdown(guid,
  View::Taken)`, `Segment::mitigation(guid) -> Option<Mitigation>`. `View::
  COUNT = 7`; the rows tier stores the view in the same `views` slot table
  — this is the `PROTO_VERSION` 21 bump (§7).
- **Overall (R10).** `absorb` merges the slot and the mitigation map; a
  key's Σ Taken is the sum over members.
- **Recap (R9)** is unchanged; its ring still carries damage *without* the
  absorbed part, because the recap answers "what killed me", not "what was
  swung at me".
- **Taken never opens or extends a segment**: the scanner ignores
  `*_MISSED`; the meter records a miss or a stagger absorb only into a segment that is
  open and not past the trash gap (a passive line after a lull, or before a
  pull's first hit, is attributed to nowhere — the parity-safe reading,
  since the scanner's byte ranges split at that hit) and never touches
  `last_ms`.

### 4.2 Aura spans with caster and target — ruling R18

As ruled after the step 4 review (`docs/plan-role-pivots-step4.md`); the
first draft's aura-effect heuristics and class gate were dropped. R12
already records marks with a duration for a curated item table plus a
hand-picked external list; R18 widens the *source* of marks to a curated
role-spell table and adds the *caster* to every mark, so one mechanism
serves three roles' questions.

**A generated table, `crates/core/src/role_spells.rs`** (`tools/gen-role-
spells.sh` → `tools/extract/src/rolegen.rs`): its **membership is
curated** in the generator source — `(aura id, expected name, kind)`, the
`EXTERNAL_BUFFS` precedent grown to five kinds — and the generator
**validates** each entry against the install: the name must match, and
the id must have an APPLY_AURA `SpellEffect` row (a cast id whose buff is
logged under another id — Metamorphosis 191427 vs its buff 162264 — is a
build failure, which a name check alone cannot give). Only ids exercised
by a committed census of real logs ship; `role_spells.expected.md` lists
every entry with its name, kind and observed counts. No class/spec gate:
nothing reads one (an external lands on its target regardless of class).

| Kind | What | Who reads it |
| --- | --- | --- |
| `ActiveMitigation` | a tank's rotational mitigation buff (Shield Block, Shield of the Righteous, Ironfur, Demon Spikes, Bone Shield, Shuffle, Blood Shield) | `am_uptime_pct` |
| `Defensive` | a personal damage-reduction cooldown, any spec | the R9 "defensives used"; death coaching |
| `External` | a buff cast on someone else (the Bloodlust family, Power Infusion, Pain Suppression, Guardian Spirit, Ironbark, Life Cocoon, Blessings, Innervate, Lay on Hands, Time Dilation, Rescue) | healers' `externals_given`; everyone's `externals_received` |
| `SupportBuff` | a buff whose value is the *target's* output, on a player (Ebon Might, Prescience, Shifting Sands — debuffs on enemies such as Chaos Brand have no span under a target-is-a-player rule and are out) | support uptime per target |
| `Cooldown` | a major offensive cooldown's buff (Metamorphosis, Avatar, Combustion, Dragonrage …) | the compare graph's burst bar, the "first Meta" finding |

**Meter.** A Buff `AuraApplied` or `AuraRefresh` on a player whose spell
is in the table opens a **span** keyed by the target with the caster as
`src`, consulted only for auras (never casts), **before** the class-spells
veto (the table takes `EXTERNAL_BUFFS`' slot) and **bypassing the item
dedupe rules** (own-cast-within-2 s and same-label-within-500 ms are
trinket semantics); a re-apply while open is a refresh. `AuraRemoved`
closes the newest open span. **A refresh or removal with no open span
opens one at the segment's start** — the buff predated the segment, the
only way a refresh can precede an apply inside it (on boss pulls it never
fires; before a trash segment it does). **Every mark call site goes
through the passive gate** (`open_segment_for_passive`), so an aura after
a segment's end lands nowhere and lazy = full. **The close at segment end
is computed at read time**, kind-branched: a role span still open reads
`min(end, now) − at`; an item mark still reads 0 (a proc that never
dropped is not a span, and no R12 golden moves). Spans have their own
list under `SPAN_CAP = 256`, inheriting R12's newest-dropped rule (stated
in CONTRACT), so a tank's spans cannot evict a trinket proc; **`uptime`,
an uncapped rollup per target per `(spell, src)` of `{count, total_ms}`**,
is the fixture-gated measure, so a fifty-minute key's Shield Block uptime
is exact after the list wrapped. `SPELL_AURA_REFRESH` is parsed as
`Event::AuraRefresh` (the same 13-field shape as applied) and matters only
here. `Segment::timeline()`'s marks gain `src` (trailing on the wire) and
the new kinds; `MarkKind` grows `ActiveMitigation | Defensive |
SupportBuff | Cooldown`. R8 is unchanged: an aura is never a class signal.

**Derived measures** (on the segment in 4a-ii, onto the card in 4b):
`am_uptime_pct` = the per-millisecond union of `ActiveMitigation` spans on
the player over the segment's `duration_ms` (the same duration the card
writes — a key Overall's is the timer), so overlapping buffs never exceed
100 %; `externals_given` = count **and** total ms of `External` spans with
`src` = the player, `externals_received` likewise by target;
`support_uptime` per `(spell, target)` for support specs, from the rollup.
The per-spell rollup is the drill behind the union headline.

### 4.3 Support attribution — ruling R19

As ruled after the step 3 review (`docs/plan-role-pivots-step3.md`); the
first draft's `contribution` / `net` pair was wrong because the log writes
an Augmentation's own procs twice.

The six `*_SUPPORT` families (`SPELL_DAMAGE_SUPPORT`,
`SPELL_PERIODIC_DAMAGE_SUPPORT`, `RANGE_DAMAGE_SUPPORT`,
`SWING_DAMAGE_LANDED_SUPPORT` — the melee support event; there is no
`SWING_DAMAGE_SUPPORT` — `SPELL_HEAL_SUPPORT`, `SPELL_PERIODIC_HEAL_SUPPORT`)
are the underlying family's line with a 3-field spell block that is the
**buff** (Ebon Might, Prescience, Shifting Sands, Bombardments, Fate
Mirror) and the **supporter's bare guid as the last field**. The amount
is the buff's attributable share — an Ebon Might line carries 21 of a
4 593 Void Ray, shares are additive and never exceed the hit — and a proc
the Evoker owns outright (Bombardments, Fate Mirror) carries the whole
hit and is logged **twice**: a plain damage line from the Evoker and a
support line with the Evoker as supporter.

- `Event::Support { src: the buffed unit, dst, spell: the buff, supporter:
  guid, amount, healing }`. The parser pops the trailing guid (a `nil` or
  non-guid last field → `Other`) and parses the rest as the base family
  with the spell-block prefix — a melee support line is SPELL-shaped (42
  fields), so the swing offsets would read the spell id as the amount.
  `SPELL_ABSORBED_SUPPORT` stays `Other`: its spell block is the buff, the
  underlying shield is unknowable, so R2's `NON_HEALING_ABSORBS` exclusion
  cannot be applied (8 lines in 137 MB of logs).
- Per segment, per player, raw-guid keyed and folded onto owners at read
  time (a buffed pet's support is its owner's received): `given.damage /
  given.healing` where `supporter` = the player, `received.*` where `src`
  = the player, and per supporter `targets` by buffed player name. A
  support line goes through the passive gate (R17's rule), never opens or
  extends a segment, is never an R8 signal, never marks a timeline. R1 /
  R2 / R3 do not move: no support amount enters anyone's `damage` or
  `healing`.
- **One number for everyone: `effective = damage − received.damage +
  given.damage`.** A peer's is their net; an Augmentation's with nothing
  received is its contribution; a self-supported proc (given and received
  by the same player) cancels and is counted once, by R1; Σ effective over
  a segment = Σ damage, a true partition of the raid's damage. There is no
  `contribution` and no `net` field — `effective` is derived by readers
  from `damage` and the two scalars (the store rule: derived values never
  travel). Grading needs no support branch: **(step 3b)** the DPS pool ranks
  `effective` always — it equals `dps` whenever nobody gave support, so
  one label, `effective_dps`, and no "fight has support" predicate — and
  the legacy `rank_dps` keys keep ranking raw `dps` (the block an
  Augmentation's buffs inflate).
- `Spec::support()` is true for Augmentation and is a flag only: the card
  does NOT store it (SQL derives it by spec id like `role`, the MCP from
  `Spec::support()`), and `trend` defaults every DPS-role subject to
  `effective_dps` — step 3b; 3a is the engine only.

**R2 amendment — the healing split and healing received** (step 3a):
`overheal` per player is the Healing row's `extra`; `absorbed` is the
absorber-credited R3 total, a counter written at the credit site after
the `NON_HEALING_ABSORBS` return (so `absorbed ≤ healing`, and it equals
`check.awk`'s `absorbheal`). `healed_received` is R2 effective healing
landing on the player **from any source** (NPC heals included, symmetric
with R17 counting NPC attackers), the stagger family excluded as R2
excludes it, and **absorbs are not received healing** — a consumed shield
is damage prevented, already in the R17 record. `self_healed` is the
subset with `src.guid == dst.guid`; a heal on a pet is its owner's
received. Both live in a per-player `Healed` record beside the R17 one
(not on `Mitigation`, whose wire codec would change before 3b's bump).
Identity, asserted on every fixture in the form the test can compute: Σ
`healed_received` over every source + Σ absorb credit − Σ `SPELL_ABSORBED`
on non-friendly victims = Σ Healing `by_target` over friendly names across
every actor's drill (NPC healers included) — on `support.txt` 78 000 +
15 000 − 0 = 93 000; dropping the NPC healer from both sides gives the
player-sources form, 73 000 + 15 000 = 88 000.

### 4.4 Shield ledger — ruling R20

Absorbs given are R2/R3 healing credited to the absorber when consumed.
What no ruling holds is a shield's **applied** value and what **expired
unconsumed**, which is the healer's waste number.

- `SPELL_AURA_APPLIED` / `SPELL_AURA_REFRESH` on a Buff carry an optional
  trailing absorb amount (FORMAT-NOTES correction 5: field 13 is the absorb,
  not a stack count); `SPELL_AURA_REMOVED` carries the *remaining* absorb.
  The parser exposes `absorb: Option<u64>` on `AuraApplied`/`AuraRefresh`/
  `AuraRemoved`; `Event::AuraRefresh` is new (today it is `Other`).
- Per open shield `(spell, src, dst)`: `applied` (sum of the applied amount
  and every refresh's delta above the remaining), `consumed` (R3's
  `SPELL_ABSORBED` amounts for that shield, already parsed), `wasted` =
  remaining at removal (the removal line's amount; when it carries none,
  `applied − consumed` clamped at 0). Segment close drops open shields
  as neither consumed nor wasted (a shield outliving the fight is not
  waste), which keeps the ledger segment-local and lazy-parity safe.
- Card measures per player (§6): `absorbed` (= consumed, given), and
  `absorb_wasted`; `absorb_efficiency = consumed / (consumed + wasted)`.
  Per shield spell in details: `shields[]` `{spell, applied, consumed,
  wasted, count}`.
- Rating limits: the stagger / cheat-death list stays out (they are the
  target's own mitigation, §4.1); a shield with no applied amount on its
  line (older logs, some raid-wide absorbs) ledgers `consumed` only and
  `wasted` unknown (`None`, not 0) — the card says so, the SQL column is
  `NULL`.

### 4.5 Coarse timeline on the rows tier

The store spec §14 ranked "marks and a coarse timeline on `rows/`" first;
tanks and healers need it more than DPS do (tank swaps, external timing,
the pre-pull shield). Per player on the rows tier: a **10 s damage-taken
series** and **10 s healing-done series** (R12's 1 s grid summed by ten;
`Segment::timeline_coarse(guid, 10_000)`), plus the player's span and mark
list under the same `SPAN_CAP`. A 35 min key is 210 buckets × a few bytes
per series — hundreds of bytes per player, within the store spec's rows
budget. The 1 s grids stay in details.

## 5. Generated tables

| File | Generator | Source tables | Committed? |
| --- | --- | --- | --- |
| `core/src/role_spells.rs` | `tools/gen-role-spells.sh` → `tools/extract/src/rolegen.rs` (curated membership; the generator validates name + an APPLY_AURA `SpellEffect` row and emits the census-annotated `role_spells.expected.md`) | SpellName, SpellEffect | yes — ids and kind only; like `class_spells.rs` |
| `model` `Spec::support` | hand | — | yes (one line) |

The generator emits, beside the table, a `role_spells.expected.md` listing
every id it selected per kind with the rule that selected it, so a patch's
diff to the committed table is reviewable. `tools/extract/verify.sh` gains
no new step: the inputs are the same DB2s already parity-gated.

## 6. Storage

Every field is additive; `HISTORY_SCHEMA` stays 1. Sizes are for a
20-player raid fight.

**Card, `fights/<id>.json`, `players[]` per player** (+ ~120 B):

```
role: "tank" | "healer" | "dps" | null   // Spec::role at write time (step 1: written, ignored on read; null = unknown spec)
support flag: NOT stored (3b) — SQL derives it by spec id like role, the MCP from Spec::support()
overheal, absorbed, absorb_wasted (null when unknown)        // R2 / R3 / R20
taken, mitigated, prevented, dtps, mitigated_pct              // R17 (step 2b): mitigated = absorbed + blocked + prevented; prevented = full absorbs + full blocks;
                                                             // mitigated_pct is DERIVED (written for SQL, ignored on read, like role); stagger is never added
self_healed, healed_received                                 // step 3 (healing split)
am_uptime_pct, externals_given, externals_received           // R18
support_given, support_received                             // R19 (3b): DAMAGE shares only; healing shares live on rows.support[]
effective_dps (DERIVED: damage − received + given; never stored)   // R19; equals dps when the fight has no support
```

The card does NOT gain `roles` / `has_support` (step 1 and 2b reviews):
`FightCard::roles()` derives the head-count in memory and `has_support`
waits for a consumer.

**Rows, `rows/<id>.json`** (measured, not the 4–8 KB first guessed: a stored
`Row` is ~265 B, so a 25-player raid pull grows by ~90 KB, +45 % of the p90
file; recaps already cost more):

- `views.taken[]` — the seventh view's meter rows (all players) — step 2a.
- `mitigation[]` — per friendly player `{guid, record, taken_spells[] (top
  16 by amount), other: {amount, extra, count, n} (the rest rolled up as a
  STRUCT, never a fake row — n > 0 says the list was capped), taken_sources[]
  (by attacker name, top 16 by amount — a raid Σ listed 74 attackers per
  player, 345 KB of rows — with `other_sources` for the rest)}` — step 2b, rows-only: details exist only on
  kills, where rows already hold the same list, so there is no details copy.
- `support[]` — per player `{guid, given, received, targets[]}` (R19).
- `uptime[]` — per player, per `(spell, kind, src)` `{count, total_ms,
  max_ms}` (R18 rollups); `shields[]` per healer (R20).
- `coarse[]` — per player `{taken10[], heal10[], marks[], spans[]}`
  (§4.5).

**Details, `details/<id>.json`** (kills / bests / pins): `PlayerDetail`
gains `taken_timeline` (1 s), `spans[]` in full (not the coarse cap),
`support_spells[]`, and the `damage_timeline` marks carry `src` and the
new kinds. Retention (§7 of the store spec) is unchanged; the protected
set gains **the owner's best per spec by role measure** — a healer's best
HPS and a tank's best `mitigated_pct` protect a fight the way best DPS does.

**Write path.** All extraction stays on the history thread from the cloned
`Segment`; the hub's clone-and-`try_send` is untouched. `regrade` rewrites
every field above from the log, so a store written by PR #12 is upgraded
by `wowdps history regrade --kind all` and, until then, reads with `role`
derived and every new column `NULL`.

**Index.** The in-memory card index gains a `by_role` bucket per fight (the
`roles` block) so `Fights { role }` filters without scanning players, and
the owner's best-per-spec map is keyed by role measure.

## 7. Wire and fixed questions (`PROTO_VERSION` 21)

One bump, taken once, carrying:

- `View::Taken` (seventh view) — `Watch` cursors, `Snapshot`, `SegmentList`
  row counts and the TUI/GUI keymaps all learn it; `Row` is unchanged.
- `Snapshot` for a Taken cursor with a drill carries `taken_spells` /
  `taken_sources` in the existing drill slots and a trailing `Mitigation`
  record; `Timeline` marks carry a trailing `src`.
- `GetFight` answers gain trailing `mitigation`, `support`, `uptime`,
  `shields`, `coarse` (stored fights) or their live equivalents.
- `HistoryQuery::Fights` gains `role: Option<Role>` — the SUBJECT's role
  (`guid`, else the owner; no subject = no-op) — in v22 (step 2b); `support:
  Option<bool>` when it has a consumer; its `me` / `peer` rows gain the role
  block below.
- `HistoryQuery::Trend`: `measure: TrendMeasure` REPLACES `view` in v22
  (step 2b; `Dps | Hps | Dtps | MitigatedPct` first; a Day/Week bucket folds
  `per_sec` as a running mean — a mean of per-fight values, as DPS-by-day
  already is) and grows to (`Dps | NetDps |
  ContributionDps | Hps | Dtps | MitigatedPct | AmUptime | AbsorbEfficiency
  | SupportGiven | Uptime(spell_id)`); default per role from §3 when
  absent.
- New `HistoryQuery::RoleNight { encounter, difficulty, night }` — the
  night's roster by role with each player's role measure, the tank pair
  side by side and the healer set ranked; what the coach's "healers
  tonight" paragraph needs in one call.

**The `me` / `peer` grade** (`graded_row` in the mcp, backed by the daemon's
`Fights` answer) becomes role-relative:

```
role, rank, count, median, share, excluded     // within role, by the §3 measure, the
                                               //   zero-output floor applied as today
rank_dps / dps_count / dps_median / dps_share  // kept verbatim for DPS-role players (compat)
effective_dps (derived)                        // always: equals dps when nobody gave support
tank_pair: [{name, taken, mitigated_pct, dtps, am_uptime_pct, boss_share}]   // tanks only
healers:   [{name, hps, overheal_pct, absorb_efficiency, externals_given}]   // healers only
```

`boss_share` is the player's share of boss-sourced damage taken among the
tanks (R16's boss identity over `taken_sources`), the "who was tanking"
number.

## 8. Testing

**Fixtures: one small fixture per ruling, not one big one.** The first
draft planned a five-player `support.txt` carrying R17–R20 lines at once;
the step 2 review retired it — five players × every miss kind × lines
whose rulings are not yet written is not hand-computable honestly, and
the R18–R20 lines would be rewritten when those rulings firm up. Instead:

- **`taken.txt` (R17, step 2a):** three players — a Protection Warrior
  (partial block, full BLOCK miss, PARRY, DODGE, MISS, a partial absorb), a
  Brewmaster Monk (two staggered hits with their `SPELL_ABSORBED` 115069
  lines and the damage lines' `absorbed`, two Stagger self-ticks), a Fire
  Mage (IMMUNE via Ice Block, a full ABSORB miss with its `SPELL_ABSORBED`
  twin, DEFLECT, REFLECT, RESIST, one `ENVIRONMENTAL_DAMAGE`) plus the
  Mage's elemental taking a hit *before* its `SPELL_SUMMON` (the read-time
  fold); one boss with health reports, one add that EVADEs (a miss on an
  NPC, proving it is not Taken). Every kind once; EVADE on an NPC by design
  (the log never writes an NPC→player EVADE). `taken.expected.md`
  / `.tsv` hand-derived under R17 with the identity written out; `check.awk`
  gains destination-side metrics (`taken`, `absorbed`, `blocked`,
  `prevented`, `misses`, `stagger`, `stagger_ticked`) on every player row,
  which **regenerates `sample.expected.tsv`** (its line 105 is a
  friendly-destination tick) — the ten pre-existing metrics stay
  byte-identical and the diff is reviewed.
- **Step 3 (R19)** adds `support.txt` with an Augmentation Evoker and two
  buffed DPS; **step 4 (R18)** a spans fixture; **step 5 (R20)** a shield
  fixture. Each computes its goldens under its own ruling only.
- **Real-log gates** (`WOWDPS_REAL_LOG`, ignored): the taken = dealt
  identity over every encounter of a 1.5 GB Heroic log, a miss-line width
  census, and the parse-time delta.

**Core** (`crates/core/tests/taken.rs` and per-step siblings): expected
values; full replay = lazy load = checkpoint resume for every new number
(the R12 gate pattern); scanner output byte-identical before and after
(nothing opened a segment); the identity over every fixture; `Overall`
sums members; later steps add the role table's committed ids stable (a
test lists them so a regenerate shows in review) and `Spec::support` /
`Spec::role` total over `Spec::ALL`.

**Codec** (`proto/tests/history.rs`): goldens for every new field; a PR #12
card / rows / details document decodes to `role: None` and empty new lists;
truncation fuzz; the wire goldens for v21 frames (`proto/tests/codec.rs`).

**Store** (`daemon/tests/history.rs`): `support.txt` closes one encounter →
card with `roles {2,1,2}` and `has_support`, rows with all four new lists;
`regrade` of a card written without the fields fills them (write a PR #12-
shaped card by hand, regrade, compare to a fresh write); the protected set
keeps a healer's best HPS and a tank's best `mitigated_pct`; `Fights {
role: Healer }` returns exactly the priest; `Trend { Hps }` for the priest
and `Trend { MitigatedPct }` for the warrior; `RoleNight` over the fixture
night.

**Lake parity** (`history/tests/parity.rs`): the daemon's role-relative
`me` grade for each of the five players equals a SQL window rank over
`players` partitioned by `(fight_id, role)` ordered by the role measure;
`Trend { measure }` equals SQL for every measure; the two conservation
identities hold in SQL over the fixture's files; every new view (§9)
selects without error on a store with and without the new columns.

**MCP**: `history { role }`, `trend { measure }`, `role_night`, and
`stored_fight` carrying `mitigation` / `support` / `uptime` / `shields`,
each over the mock bridge; `fight` = `stored_fight` byte-equal for the
fixture fight including the new blocks.

**GUI/TUI**: the Taken view renders headless (iced_test / TestBackend) over
`daemon::mock` on `support.txt`; the keymap reaches it; `tests/no_engine.rs`
still passes.

**Perf gates**: the ignored `real_log` tests report the added parse cost
(target: under 5 % on a 300 MB log — the new events are a small fraction
of lines) and the rows-tier growth per fight.

## 9. DuckDB: views and the queries they enable

New views in `crates/history/src/lib.rs`, defined only when the files
exist, all read-only and fenced like the rest:

| View | From | Grain |
| --- | --- | --- |
| `players` | (exists) | gains `role`, `support`, `overheal`, `absorbed`, `absorb_wasted`, `taken`, `mitigated`, `prevented`, `dtps`, `mitigated_pct` (a CASE over the three, so SQL and the card agree), `self_healed`, `healed_received`, `am_uptime_pct`, `externals_given`, `externals_received`, `support_given`, `support_received`, the stored `effective_dps` and `effective_dps_sql` (recomputed: `greatest(0, coalesce(damage,0) − coalesce(support_received,0) + coalesce(support_given,0))` over the duration, so a pre-3b card reads its `dps`; `role_ranks` ranks the DPS role by it under one label), a derived `support` flag — flattened by the recursive unnest, `NULL` on old cards |
| `taken` | `rows.views.taken` unnested | fight × player: the Taken meter row — every 2b view is defined only after a `LIMIT 0` probe shows the field exists, so an un-regraded or mixed lake still opens |
| `mitigation` | `rows.mitigation` unnested | fight × player: the R17 record |
| `taken_spells` / `taken_sources` | `rows.mitigation[].taken_spells / taken_sources` | fight × player × ability (capped; `mitigation.other` holds the rest) / attacker |
| `support` / `support_targets` | `rows.support` | fight × supporter (× target) |
| `uptime` | `rows.uptime` | fight × player × spell × caster: `count`, `total_ms`, `max_ms`, `kind` |
| `shields` | `rows.shields` | fight × healer × shield spell: applied, consumed, wasted |
| `coarse` | `rows.coarse` | fight × player: the 10 s arrays (`taken10`, `heal10`) and span / mark lists — unnest per query |
| `role_ranks` | window over `players` | fight × player: `rank`, `count`, `median` within `(fight_id, role)` by the §3 measure — the SQL twin of the daemon's `me` grade, and what the parity gate compares |

Worked queries the views make one statement each (kept in
`docs/history-queries.md` as the recipe list the MCP's `history_sql`
description points at):

- **Healer rank trend across a tier:** `role_ranks` filtered to the owner,
  `role = 'healer'`, joined to `fights` for the night.
- **Absorb efficiency by boss:** `shields` summed per `(encounter_id,
  difficulty)`, `consumed / (consumed + wasted)`.
- **Externals given, to whom, how early:** `uptime` where `kind =
  'External'` and `src = me`, joined to `players` on the target guid for
  role and name; `coarse.spans` for the timing.
- **Tank swap and boss target share:** `taken_sources` where the source
  guid is the card's R16 boss, pivoted by tank; `coarse.taken10` per tank
  for the swap points.
- **Active-mitigation uptime vs damage taken:** `mitigation` joined to
  `players.am_uptime_pct`, per fight, the scatter the coach wants for
  "is the tank pressing their button".
- **Augmentation contribution per target:** `support_targets` joined to
  `players` for the target's spec — "Prescience went to the Fire Mage 70 %
  of the time; the Assassination Rogue was 40 % ahead".
- **Effective DPS for the buffed:** `players.effective_dps` vs `dps`, for every fight
  with `has_support`.
- **Damage taken by ability, avoidable share:** `taken_spells` joined to a
  reader-supplied avoidable list (roadmap item 2) — the store holds the
  facts, not the verdict.

`materialize` pre-unnests the new views into `cache.duckdb` like the rest.
`export <fight_id>` includes the new blocks. `stats` reports how many cards
carry `role` so an un-regraded store is visible.

## 10. Decisions

| Question | Options | Decided |
| --- | --- | --- |
| Augmentation: a fourth `Role` or a flag? | `Role::Support` keeps grading paths simple; a flag keeps the game's own classification and every "DPS-role" filter | **Flag** (`Spec::support`). The rank bucket is DPS; the rank measure is contribution. |
| Where does Taken live? | a parallel list on rows only; a seventh `View` | **Seventh `View`** — tanks want it live, and one slot table serves meter and store. Costs the `PROTO_VERSION` bump this item was going to take anyway. |
| Taken amount convention | landed-on-hp only; R1's `amount + absorbed` | **R1's**, so taken = dealt and the invariant is testable; `extra` = absorbed. |
| Do `_SUPPORT` amounts fold into anyone's `damage`? | WCL-style rewrite of everyone's number; keep R1, add measures | **Keep R1**; `contribution` and `net` beside it. No golden moves, in-game meters still agree. |
| Aura spans: store every span or rollups? | spans only (cap loses uptime); rollups only (no timing) | **Both**: capped spans + uncapped rollups per `(spell, src)`. |
| Wasted absorb when the removal line carries no remainder | assume 0; `applied − consumed`; unknown | **`applied − consumed` clamped, else unknown (`NULL`)**; never a silent 0. |
| Coarse timeline bucket | 5 s / 10 s / 30 s | **10 s** — matches the coach's report and keeps a key under 1 KB per series. |
| Tank rank | rank by `mitigated_pct` | **No rank**; the pair side by side with `boss_share`. A tank who was not tanking is not "worse". |

## 11. Delivery order

Five shippable steps, each a PR with its fixture parity green, in the
order that front-loads coach value:

1. **Role in the card's JSON, role-relative `me` / `peer` grading in the
   MCP (a healer ranked among healers by HPS; tanks unranked), `players.role`
   in SQL derived by spec id, `role_ranks` with the grader's floors, the
   parity gate widened to the grade, regrade back-fill.** No parser change
   and **no wire change**: `Fights { role }`, `Trend { measure }`, the
   `roles` / `has_support` card fields and the tank block wait for the bump
   in step 2. A healer trend already exists as `Trend { view: Healing }`.
   Plan: `docs/plan-role-pivots-step1.md`. Ships the healer rank the coach
   has been faking with a spec lookup.
2. **R17 Taken**: parser events, `View::Taken`, the mitigation record,
   by-source rows, the rows-tier lists and views, the `PROTO_VERSION` 21
   bump — which also carries `Fights { role }`, `Trend { measure }`, the
   `roles` / `has_support` card fields and the `tank_pair` block with its
   R17 columns — TUI/GUI view. `support.txt` lands here with its goldens
   (the R18–R20 lines are present from the start so later steps only add
   expectations).
3. **R19 Support** (small: `Spec::support`, parser + card + `contribution`/`net` +
   `support` views) and the **card's healing split** (`overheal`,
   `absorbed`, both already computed by the meter).
4. **R18 Aura spans** with the generated role table, uptime rollups,
   `am_uptime_pct`, externals given/received, the new `MarkKind`s, and the
   coarse timeline on rows (§4.5) — which also closes the store spec's §14
   items 1 and 2.
5. **R20 Shield ledger** and `absorb_wasted`; `RoleNight`.

CONTRACT.md gains R17–R20 in its rulings table when each lands; the store
spec's §14 and the design document's §9 table get a "shipped in 1a step N"
column at the end.
