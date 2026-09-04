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
| Dps | `dps`; when the fight has support events, `net_dps` (§4.3) beside it | `support.received` | `dps` |
| Dps, support | `contribution_dps` = own + given (§4.3) | `support.given` | `contribution_dps` |
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
  `*_MISSED`; the meter records a miss only into an already-open segment
  (`end_ms.is_none()`) and never touches `last_ms`.

### 4.2 Aura spans with caster and target — ruling R18

R12 already records marks with a duration (`dur_ms` closes on
`AuraRemoved`) for a curated item table plus the `EXTERNAL_BUFFS` list. R18
widens the *source* of marks and adds the *caster* to each mark, so one
mechanism serves three roles' questions.

**A generated table, `crates/core/src/role_spells.rs`** (`tools/gen-role-
spells.sh`, same extractor pipeline as `class_spells.rs`, regenerated per
patch, ids and names only), maps spell id → `RoleSpellKind`:

| Kind | What | Examples | Who reads it |
| --- | --- | --- | --- |
| `ActiveMitigation` | a tank's rotational mitigation buff | Shield Block, Ironfur, Shield of the Righteous, Demon Spikes, Bone Shield (stack), Blackout Combo's Shuffle, Death Strike's Blood Shield is an absorb (R19) | tank `am_uptime_pct` |
| `Defensive` | a personal damage-reduction cooldown, any spec | Shield Wall, Dispersion, Ice Block, Obsidian Scales, Netherwalk, Vampiric Blood | the R9 "defensives used" the recap wanted; item 2's death coaching |
| `External` | a defensive or throughput buff cast on someone else | Pain Suppression, Guardian Spirit, Ironbark, Life Cocoon, Blessing of Sacrifice, Power Infusion, Innervate, Bloodlust family (moves here from `EXTERNAL_BUFFS`) | healer `externals_given`; everyone's `externals_received` |
| `SupportBuff` | a buff whose value is the *target's* output | Ebon Might, Prescience, Blistering Scales, Breath of Eons' debuff side, Hunter's Mark, Chaos Brand, Mystic Touch | support uptime per target |
| `Cooldown` | a major offensive cooldown (base cooldown ≥ 60 s or the spec's burst window) | Metamorphosis, Avatar, Combustion, Dragonrage | the store spec §14 item 2 — "first Meta at 0:30" |

Selection rules live in `tools/extract/src/rolegen.rs` (SpellCategories /
SpellCooldowns / SpellAuraOptions / the aura's effect list, spec gating via
the same tables `classgen.rs` reads) plus a **hand allowlist / denylist per
kind** checked into the generator's source — the tables are generous and a
persistent raid buff (Arcane Intellect) must never become a `SupportBuff`
span, exactly as `EXTERNAL_BUFFS` was hand-picked. The fixture's expected
values are computed from the ruling and the *committed* table, so the
generator cannot silently move a golden.

**Meter.** A Buff `AuraApplied` on a player whose spell is in the table
opens a **span**: `{spell, kind, src: caster guid, dst: target guid,
start_ms, dur_ms}`; `AuraRemoved` closes the newest open span of that
(spell, src, dst); a re-apply while open is a refresh (R12's rule); an
open span at segment close reads `dur_ms = end − start` (R12 leaves item
marks at 0 — here an open mitigation buff at the kill *is* uptime, so the
close is explicit and fixture-gated). Per player, spans are kept twice:

- `spans`, the bounded list (`SPAN_CAP = 256` per player, oldest evicted),
  written to details on kills/bests/pins and to the rows tier as the
  **coarse timeline** below; and
- `uptime`, an *unbounded but small* rollup keyed by `(spell, src)` per
  target: `{count, total_ms, max_ms}` — never evicted, so a fifty-minute
  key's Shield Block uptime is exact even after the span list wrapped.

`Segment::timeline()`'s marks gain `src` (trailing field) and the new
kinds; `MarkKind` grows `ActiveMitigation | Defensive | SupportBuff |
Cooldown` (`External` exists). The class-spells veto (R12) is checked
*after* the role table, like `EXTERNAL_BUFFS` already is. R8 inference is
unchanged: an aura is never a class signal.

**Derived measures** (computed at write time onto the card, §6):
`am_uptime_pct` = Σ `ActiveMitigation` span time on the player, spans
unioned so overlapping buffs do not exceed 100 %, over the fight's duration;
`externals_given` = count of `External` spans with `src` = the player (and
`externals_given_ms`); `support_uptime` per `(spell, dst)` for support
specs, from the rollup.

### 4.3 Support attribution — ruling R19

The `*_SUPPORT` events (`SPELL_DAMAGE_SUPPORT`,
`SPELL_PERIODIC_DAMAGE_SUPPORT`, `RANGE_DAMAGE_SUPPORT`,
`SPELL_HEAL_SUPPORT`, `SPELL_PERIODIC_HEAL_SUPPORT`) re-state a hit or heal
the buffed player already logged, with a **trailing `supporterGUID`** field
naming the buffing player and an `amount` that is the *portion attributable
to the buff* (the game's own split — verified against Warcraft Logs on the
fixture's source log before the golden is set). R1 keeps them out of
`damage`; R19 gives them a home:

- `Event::Support { src: buffed unit, dst, spell, supporter: Unit, amount,
  overheal, absorbed, healing: bool }`. Unknown supporter guid (the log
  writes `nil` for some environmental cases) → `Other`, as today.
- Per segment, per player: `support.given = { damage, healing }` summed
  over events where `supporter` = the player, and `support.received = {
  damage, healing }` where `src` = the player (pets fold onto owners on both
  ends, R4). Both are conserved: Σ given = Σ received over the fight,
  another fixture and SQL invariant.
- **Per-target given** (`support_targets`, a `Row` list keyed by buffed
  guid) so "Prescience on the wrong player" is one read; per-spell given
  (`support_spells`, keyed by the *underlying* hit's spell) is derivable and
  stored only in details.
- **Contribution and net:** `contribution = damage + support.given.damage`;
  `net = damage − support.received.damage`. `contribution_dps` and `net_dps`
  are the per-second forms over R7 duration. Warcraft Logs' "damage done"
  for an Augmentation is `contribution`; its peers' is `net`. The meter
  keeps R1 `damage` (what the in-game meter shows) as the row's `amount`
  and exposes the other two beside it; no existing golden moves.
- A support event **never opens or extends a segment** (the underlying hit
  did), never marks a timeline, and is never an R8 class signal for the
  supporter (Augmentation is inferred from its own casts).

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
| `core/src/role_spells.rs` | `tools/gen-role-spells.sh` → `tools/extract/src/rolegen.rs` | SpellCategories, SpellCooldowns, SpellAuraOptions, SpellEffect (aura effect types 65/87/… for DR, 69 absorb), SpecializationSpells, SkillLineAbility | yes — ids, names, kind, spec gate; like `class_spells.rs` |
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
support: true | absent                 // Spec::support
overheal, absorbed, absorb_wasted (null when unknown)        // R2 / R3 / R20
taken, mitigated, dtps, self_healed, healed_received         // R17; mitigated = absorbed + blocked + Σ full-miss amounts + stagger
am_uptime_pct, externals_given, externals_received           // R18
support: { given: {damage, healing}, received: {damage, healing} }   // R19
contribution_dps, net_dps                                    // R19; equal to dps when the fight has no support events
```

The card also gains, at the fight level, `roles: {tanks: N, healers: N,
dps: N}` and `has_support: bool` so `history` can filter and the `me`
block can pick a grading path without opening rows.

**Rows, `rows/<id>.json`** (+ 4–8 KB):

- `views.taken[]` — the seventh view's meter rows (all players).
- `mitigation[]` — one R17 record per friendly player (`guid` + the
  fields in §4.1), plus `taken_spells[]` and `taken_sources[]` `Row`s.
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
- `HistoryQuery::Fights` gains `role: Option<Role>` and `support:
  Option<bool>` filters; its `me` / `peer` rows gain the role block below.
- `HistoryQuery::Trend` gains `measure: TrendMeasure` (`Dps | NetDps |
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
net_dps, contribution_dps                      // when has_support
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
  NPC, proving it is not Taken). Every `MissKind` once. `taken.expected.md`
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
| `players` | (exists) | gains `role`, `support`, `overheal`, `absorbed`, `absorb_wasted`, `taken`, `mitigated`, `dtps`, `self_healed`, `healed_received`, `am_uptime_pct`, `externals_given`, `externals_received`, `support_given_damage`, `support_received_damage`, `contribution_dps`, `net_dps` — flattened by the recursive unnest, `NULL` on old cards |
| `taken` | `rows.views.taken` unnested | fight × player: the Taken meter row |
| `mitigation` | `rows.mitigation` unnested | fight × player: the R17 record |
| `taken_spells` / `taken_sources` | `rows.mitigation[].taken_spells / taken_sources` | fight × player × ability / attacker |
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
- **Net DPS for the buffed:** `players.net_dps` vs `dps`, for every fight
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
