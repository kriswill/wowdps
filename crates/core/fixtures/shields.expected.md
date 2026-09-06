# `shields.txt` — expected values (R20: the shield ledger, `absorb_wasted`)

Authoritative expected output for the R20 fixture. **Computed independently of
the Rust implementation** by `check.awk`'s shield ledger (the validator's own
reading of the log grammar under CONTRACT.md R20 as written in
`docs/spec-role-pivots.md` §4.4 and `docs/plan-role-pivots-step5.md` §0). The
machine-readable form is `shields.expected.tsv`; this file is the same numbers
with every derivation shown, shield by shield.

Regenerate / check:

```sh
./verify.sh                                     # sample, taken, support, spans AND shields: PASS
./verify.sh shields.txt shields.expected.tsv    # PASS
./verify.sh corrupt.txt sample.expected.tsv     # must FAIL (negative control)
gawk -v SHIELDS=1 -f check.awk shields.txt shields.txt > /dev/null   # the ledger, key by key, on stderr
```

TSV columns: `segment kind name result dur_ms enc_id difficulty player metric
value`. Every (segment, player) row carries **37 metrics in a fixed order,
always emitted**: the 34 of `spans.expected.md`, then the three new ones:

| metric | definition (per player, per segment) |
|---|---|
| `absorb_applied` | Σ `applied` over the player's shields (as ABSORBER) that **closed** inside the segment with a known applied amount |
| `absorb_wasted` | Σ `wasted` over the player's closed shields whose waste is known; **blank** (not 0) when no such shield exists — the meter's `absorb_wasted = None` |
| `shields_unknown` | the count of the player's shields whose applied amount was unknown: closed shields first seen by an absorb, plus **every** shield still open when the segment closed |

`absorbheal` (already emitted, `Segment::absorbed_healing`) is Σ `consumed`
over the same rows — the gated identity: every absorb that credits healing
enters exactly one ledger key, so **Σ rows.consumed = absorbheal, exactly.**

## The ledger

One state machine per key `(segment, target, shield spell, caster)` — raw
guids, exactly R18's span key — whose row lands on the **absorber** (the aura's
`src`, which the log's `SPELL_ABSORBED` names as the absorber; the real-log
census found 0 mismatches). A shield is `{applied, consumed, remaining, wasted}`
with two knowledge flags, *applied known* and *waste known*:

| line | transition |
|---|---|
| `SPELL_AURA_APPLIED` with a trailer `a` | opens `applied = remaining = a`, known; while the key is already OPEN the old shield closes first with `wasted = remaining` (when known) |
| `SPELL_AURA_APPLIED` without a trailer | opens unknown-applied |
| `SPELL_AURA_REFRESH` with a trailer `r` | **`r` is the new running total, never a delta**: `r > remaining` → `applied += r − remaining`; `r < remaining` → `wasted += remaining − r` (a refresh DOWN overwrites); then `remaining = r`. No trailer, or no open shield: no-op |
| `SPELL_ABSORBED` naming the shield | `consumed += amount`; an **over-absorb** (`amount > remaining`) raises `applied` by the excess and `remaining` → 0, so `applied = consumed + wasted` holds by construction; on a key not open it opens an unknown-applied shield with `consumed = amount` |
| `SPELL_AURA_REMOVED` with a trailer `w` | `wasted += w` (waste known); **self-check: `w == remaining` whenever both are known** — a mismatch fails `verify.sh` |
| `SPELL_AURA_REMOVED` without a trailer | `wasted += remaining` when known, else the waste stays unknown; closes. No open shield: no-op |
| segment close | every open shield folds with its `consumed` and count ONLY — applied and wasted dropped, `unknown += 1` |

**The gate.** An aura line ledgers only when its spell is an absorb spell —
`check.awk`'s `SHIELD` is the fixture's three (17 Power Word: Shield, 11426 Ice
Barrier, 77535 Blood Shield), a hand-coded subset of the generated
`crates/core/src/absorb_spells.rs` — never on the trailer alone: **29838 Second
Wind** (`BUFF,0,0`, the 15-field shape, l.80) and **195181 Bone Shield** (a
7 500 trailer on APPLIED and REMOVED, l.81–82) carry trailers and open nothing.
The `NON_HEALING_ABSORBS` (**115069 Stagger**, l.49) never enter the ledger —
R17's `stagger`, as before. Every aura line is passive: one after
`ENCOUNTER_END` (l.94–95) or in a trash segment's dead zone (l.103–104) lands
nowhere, like a span.

## Roster

- `P` = `Player-1168-0A1B2C41` "Serenya-Nebula-US", `0x514` — **Discipline
  Priest** (COMBATANT_INFO spec 256). Casts every Power Word: Shield.
- `W` = `Player-1168-0A1B2C42` "Bulwark-Nebula-US", `0x511` — **Protection
  Warrior** (73). Shielded by P; Second Wind.
- `M` = `Player-1168-0A1B2C43` "Pyrelle-Nebula-US", `0x514` — **Fire Mage** (63).
  Shielded by P; her own Ice Barrier, re-applied while open.
- `K` = `Player-1168-0A1B2C44` "Brewmoon-Nebula-US", `0x514` — **Brewmaster
  Monk** (268). Shielded by P (the refresh-down and the shield open at the
  kill); Stagger.
- `D` = `Player-1168-0A1B2C45` "Morthane-Nebula-US", `0x514` — **Blood Death
  Knight** (250). Shielded by P (the pre-pull shield and the over-absorb); his
  own Blood Shield with two running-total refreshes; Bone Shield.
- boss `Creature-…-218000-0000AE01` "Shields Test Boss", `0xa48`, raid flag
  `0x80`, max HP 400 000 (driven to exactly 0 at l.91).
- `Creature-0-4232-2552-0-1985-0000CC04` "Wandering Boar", max HP 42 000 — the
  trash tail in Dornogal.

No pets, no `_SUPPORT` lines, no misses, no externals or support buffs. Two
of D's buffs ARE in R18's role table as `ActiveMitigation` — Blood Shield
77535 (l.66–78, 20:05:34 → 20:05:40 = 6 000 ms) and Bone Shield 195181
(l.81–82, 1 000 ms) — so D carries **2 spans and `am_uptime_ms` 7 000**; every
other span metric is 0. Blood Shield is thus both an R18 span and an R20 shield
(the two rulings share a key shape and nothing else); Bone Shield is a span and
never a shield. Every `SPELL_ABSORBED` is paired with the damage line it rode on,
whose `absorbed` field equals the absorb amount, so R17's `taken` / `absorbed`
are consistent with the ledger. Both `SPELL_ABSORBED` arities appear: **22
fields** under a boss spell (attacker, defender, damage spell, absorber, shield,
amount, total) and **19 fields** under a boss swing (no damage-spell block).

**Expected segment count: 2**, in order: Encounter "Shields Test Boss" (id
3148, difficulty 16, **kill**, 60.000 s = 20:05:00.000 → 20:06:00.000, R4), then
Trash (8.000 s, R7: 20:06:10 → 20:06:18). The pre-pull `SPELL_AURA_APPLIED` at
20:04:55 (l.4) is passive and opens nothing, so no pre-pull Trash exists. Times
below are `m:ss` offsets from `ENCOUNTER_START`; line numbers are
`shields.txt`'s (1-based).

---

## Segment 1 — Encounter "Shields Test Boss" — KILL, 60.000 s, enc 3148 / diff 16

### Damage dealt (R1)

| player | lines | hits | total |
|---|---|---|---|
| M | 20, 37, 60, 84, 91 | Fireball 40 000 + Pyroblast 60 000 (crit) + Fireball 35 000 + Pyroblast 45 000 + Fireball 6 000 (the kill) | **186 000** |
| D | 30, 79, 89 | Heart Strike 25 000 + 28 000 + 20 000 | **73 000** |
| W | 13, 43, 83 | Shield Slam 30 000 + Revenge 12 000 + Shield Slam 24 000 | **66 000** |
| K | 24, 50, 52, 86 | Tiger Palm 15 000 + **Stagger tick 1 500** (self, R1 has no self-damage exclusion) + Keg Smash 20 000 + Tiger Palm 14 000 | **50 500** |
| P | 33, 65, 90 | Smite 9 000 + Penance 12 000 + Smite 5 000 | **26 000** |

Σ = 401 500 (the boss's 400 000 + K's 1 500 self-tick). dps over 60 s: M
3 100.00, D 1 216.67, W 1 100.00, K 841.67, P 433.33; pct: 46.33, 18.18, 16.44,
12.58, 6.48. Overkill 0 everywhere (the kill at l.91 logs overkill 0). Row
order: M, D, W, K, P.

### Healing (R2) and the absorb credit (R3)

l.41 Penance on W 30 000 / overheal 6 000 → 24 000 (**W healed_received
24 000**). Absorbs credit the ABSORBER as healing: P's Power Word: Shield
absorbs (l.12, 18, 22, 26, 29, 35, 39, 46, 63, 88) sum to **65 000**, M's Ice
Barrier (l.55, 58) **11 000**, D's Blood Shield (l.69, 73, 77) **9 000**; the
Stagger absorb at l.49 is NON_HEALING and credits nobody.

| player | heal | overheal | absorbheal |
|---|---|---|---|
| P | 24 000 + 65 000 = **89 000** | 6 000 | **65 000** |
| M | **11 000** | 0 | **11 000** |
| D | **9 000** | 0 | **9 000** |

### Damage taken (R17)

`taken` = base + absorbed on every boss hit; the Stagger absorb is `stagger`
and the tick is `stagger_ticked` (never `taken`).

| player | hits (base + absorbed) | taken | absorbed | taken10_0 |
|---|---|---|---|---|
| D | l.11 3 000+4 000, l.62 1 000+7 000, l.67 6 000+2 000, l.71 5 000+3 000, l.75 4 000+4 000 | **39 000** | 20 000 | 7 000 |
| M | l.21 1 000+9 000, l.34 2 000+6 000, l.54 2 000+3 000, l.57 500+8 000 | **31 500** | 26 000 | 10 000 |
| P | l.25 2 000+5 000, l.38 1 000+15 000 | **23 000** | 20 000 | 7 000 |
| W | l.16 8 000+6 000, l.27 4 000+4 000 | **22 000** | 10 000 | 14 000 |
| K | l.45 3 000+6 000, l.47 5 000+3 000, l.87 2 000+3 000 | **22 000** | 12 000 | 0 |

**K stagger 3 000** (l.49), **stagger_ticked 1 500** (l.50). Identity: Σ boss
by_target 137 500 = Σ taken 137 500.

### The shield ledger (R20) — every shield, in order of closing

Ten shields close in the segment and one folds open at the kill. Each row
below is one `(target, spell, caster)` key's lifetime; "→" lines are the
transitions in log order.

| # | key | lines | transitions | applied | consumed | wasted | note |
|---|---|---|---|---:|---:|---:|---|
| 1 | D / PW:S / P | 4 (nowhere), 11–12, 14 | APPLIED 6 000 at 20:04:55 — no segment is open, **lands nowhere** → ABSORBED 4 000 at 0:01 opens **unknown-applied**, consumed 4 000 → REMOVED **2 000** at 0:03: wasted 2 000 (known — the trailer is authoritative even when applied is not) | **?** | 4 000 | 2 000 | the pre-pull shield: `unknown` 1, its waste still KNOWN (S2) |
| 2 | W / PW:S / P | 15, 18, 29, 31 | APPLIED 20 000 → ABSORBED 6 000 (rem 14 000) → ABSORBED 4 000 (rem 10 000) → REMOVED 10 000 = rem ✓ | 20 000 | 10 000 | 10 000 | plain: applied = consumed + wasted |
| 3 | M / PW:S / P | 19, 22, 35, 36 | APPLIED 15 000 → ABSORBED 9 000 (rem 6 000) → ABSORBED 6 000 (rem 0) → REMOVED 0 = rem ✓ | 15 000 | 15 000 | 0 | fully consumed |
| 4 | P / PW:S / P | 23, 26, 32, 39, 40 | APPLIED 12 000 → ABSORBED 5 000 (rem 7 000) → **REFRESH 18 000**: running total, delta +11 000 → applied 23 000, rem 18 000 → ABSORBED 15 000 (rem 3 000) → REMOVED 3 000 = rem ✓ | 23 000 | 20 000 | 3 000 | refresh UP; 23 000 = 20 000 + 3 000 ✓ (B3) |
| 5 | K / PW:S / P | 42, 44, 46, 51 | APPLIED 10 000 → **REFRESH 6 000** < rem 10 000: wasted 4 000, rem 6 000 → ABSORBED 6 000 (rem 0) → REMOVED 0 = rem ✓ | 10 000 | 6 000 | 4 000 | refresh DOWN is waste (S1) |
| 6 | M / Ice Barrier / M | 53, 55, 56 | APPLIED 8 000 → ABSORBED 3 000 (rem 5 000) → **APPLIED again while open**: the old shield closes with wasted = rem 5 000 | 8 000 | 3 000 | 5 000 | re-apply without a removal |
| 7 | M / Ice Barrier / M | 56, 58, 59 | (the new shield) APPLIED 8 000 → ABSORBED 8 000 (rem 0) → REMOVED 0 = rem ✓ | 8 000 | 8 000 | 0 | |
| 8 | D / PW:S / P | 61, 63, 64 | APPLIED 5 000 → ABSORBED **7 000 > rem 5 000**: applied raised by the excess 2 000 → 7 000, rem 0 → REMOVED 0 = rem ✓ | 7 000 | 7 000 | 0 | the over-absorb (B2): the identity holds by construction |
| 9 | D / Blood Shield / D | 66, 69, 70, 73, 74, 77, 78 | APPLIED 5 000 → ABSORBED 2 000 (rem 3 000) → REFRESH 8 000: +5 000 → applied 10 000, rem 8 000 → ABSORBED 3 000 (rem 5 000) → REFRESH 9 000: +4 000 → applied 14 000, rem 9 000 → ABSORBED 4 000 (rem 5 000) → REMOVED 5 000 = rem ✓ | 14 000 | 9 000 | 5 000 | two running-total refreshes (the Aug 1 finding: 84 753 → 127 428 → 170 173 were totals, not deltas) |
| 10 | K / PW:S / P | 85, 88, (kill) | APPLIED 9 000 → ABSORBED 3 000 (rem 6 000) → **still open at ENCOUNTER_END**: folds with consumed 3 000 and count 1 only; the 9 000 applied and the 6 000 remaining are DROPPED, `unknown` += 1 | — | 3 000 | — | open at the kill (S3) |

Not in the ledger: l.49 Stagger (NON_HEALING_ABSORBS → `stagger`), l.80 Second
Wind `BUFF,0,0`, l.81–82 Bone Shield 7 500 (not absorb spells — no row, no
count), l.94–95 a Power Word: Shield applied AND removed with 11 000 after
`ENCOUNTER_END` (no segment is open: nowhere — a removal on a key with no open
shield is a no-op, never a shield).

#### Rows (per absorber, per spell — `Segment::shields`)

| absorber | spell | count | applied | consumed | wasted | unknown | from shields |
|---|---|---:|---:|---:|---:|---:|---|
| P | Power Word: Shield | 7 | **75 000** | **65 000** | **19 000** | **2** | #1, 2, 3, 4, 5, 8, 10 |
| M | Ice Barrier | 2 | **16 000** | **11 000** | **5 000** | 0 | #6, 7 |
| D | Blood Shield | 1 | **14 000** | **9 000** | **5 000** | 0 | #9 |

P's applied = 20 000 + 15 000 + 23 000 + 10 000 + 7 000 (#1 unknown and #10
dropped); wasted = 2 000 + 10 000 + 0 + 3 000 + 4 000 + 0 (#10 dropped);
consumed = 4 000 + 10 000 + 15 000 + 20 000 + 6 000 + 7 000 + 3 000 = 65 000 =
P's `absorbheal`; unknown = #1 (applied unknown) + #10 (open at close).

#### Per-player metrics

| player | absorb_applied | absorb_wasted | shields_unknown | absorbheal (= Σ consumed) | absorb_efficiency (derived) |
|---|---:|---:|---:|---:|---:|
| P | **75 000** | **19 000** | **2** | 65 000 | 65 000 / 84 000 = 77.4 % |
| M | **16 000** | **5 000** | 0 | 11 000 | 68.75 % |
| D | **14 000** | **5 000** | 0 | 9 000 | 64.3 % |
| W | 0 | *(blank)* | 0 | 0 | — |
| K | 0 | *(blank)* | 0 | 0 | — |

W and K absorb nothing for anyone (they are shield TARGETS, never absorbers),
so their waste is unknown: `absorb_wasted` is blank, the meter's `None` — not 0,
which would claim a perfect efficiency.

### The identities

1. **Σ rows.consumed = absorbheal** for every player: P 65 000, M 11 000, D 9 000
   (and 0 = 0 for W, K). Exact, because an open shield folds its consumed.
2. **applied = consumed + wasted on every closed shield with a known applied**:
   #2 20 000 = 10 000 + 10 000; #3 15 000 = 15 000 + 0; #4 23 000 = 20 000 +
   3 000; #5 10 000 = 6 000 + 4 000; #6 8 000 = 3 000 + 5 000; #7 8 000 = 8 000 +
   0; #8 7 000 = 7 000 + 0; #9 14 000 = 9 000 + 5 000. Per player: P 75 000 =
   (65 000 − 4 000 − 3 000) + (19 000 − 2 000) = 58 000 + 17 000 ✓ (#1's
   consumed and waste, #10's consumed are outside the known-applied set); M
   16 000 = 11 000 + 5 000 ✓; D 14 000 = 9 000 + 5 000 ✓.
3. **remaining == REMOVED trailer** on every removal where both are known (#2,
   3, 4, 5, 7, 8, 9) — `check.awk` asserts it and `verify.sh` fails otherwise.
4. R17 unchanged: Σ taken 137 500 = Σ boss by_target; K's stagger 3 000 is not
   in taken, its tick 1 500 is dealt, not taken.
5. R18 unchanged: D's Blood Shield and Bone Shield are `ActiveMitigation`
   spans (2 spans, `am_uptime_ms` 7 000 — `check.awk`'s `ROLE` subset carries
   both); every other span metric is 0.

---

## Segment 2 — Trash, 8.000 s (20:06:10 → 20:06:18)

l.97 the boar swings on W at 1:10 for 1 500 (opens the Trash segment; **W taken
1 500, taken10_0 1 500**); l.99 W swings the boar for 6 000 at 1:13; l.101 M's
Fireball kills the boar at 1:18 (36 000, overkill 0), `UNIT_DIED` l.102.

| player | damage | dps | pct | taken | absorb_applied | absorb_wasted | shields_unknown |
|---|---|---|---|---|---|---|---|
| M | 36 000 | 4 500.00 | 85.71 | 0 | 0 | *(blank)* | 0 |
| W | 6 000 | 750.00 | 14.29 | 1 500 | 0 | *(blank)* | 0 |

**The dead zone**: l.103 Power Word: Shield APPLIED 5 000 at 20:07:30 and l.104
REMOVED 5 000 at 20:07:45 come 72 s / 87 s after the trash's last combat line
(20:06:18) — past the R4 60 s gap. An aura is passive and never opens, extends or
splits a segment, so both **land nowhere**: P has no row in the trash at all.
The lines after `ENCOUNTER_END` (l.94–95) land nowhere for the same reason: no
segment was open.

---

## Edge shapes deliberately present

| shape | line(s) | expected behaviour |
|---|---|---|
| APPLIED before `ENCOUNTER_START` | 4 | nowhere; the shield is later seen only by its absorb (#1, unknown-applied) |
| REMOVED with a trailer on an unknown-applied shield | 14 | wasted KNOWN (2 000), applied unknown — `shields_unknown` counts it, `absorb_wasted` still sums it |
| `SPELL_ABSORBED` 19-field (swing, no damage-spell block) | 18, 29, 49, 69, 73, 77 | absorber at 9–12, shield at 13–15, amount 16 |
| `SPELL_ABSORBED` 22-field (under a boss spell) | 12, 22, 26, 35, 39, 46, 55, 58, 63, 88 | absorber at 12–15, shield at 16–18, amount 19 |
| REFRESH with a running total above remaining | 32, 70, 74 | applied += delta |
| REFRESH with a running total below remaining | 44 | wasted += the difference |
| APPLIED while the key is open | 56 | the old shield closes with wasted = remaining, a new one opens |
| absorb larger than remaining | 63 | applied raised by the excess, remaining 0 |
| Stagger absorb + tick | 49–50 | `stagger` 3 000 / `stagger_ticked` 1 500; no ledger row |
| `BUFF,0,0` (15-field APPLIED) | 80 | not an absorb spell: nothing |
| non-shield buff with a nonzero trailer | 81–82 | not an absorb spell: nothing |
| shield open at the kill | 85, 88 | folds consumed 3 000 + count, `unknown` += 1; applied/wasted dropped |
| APPLIED + REMOVED after `ENCOUNTER_END` | 94–95 | nowhere |
| APPLIED + REMOVED in the trash dead zone | 103–104 | nowhere |
