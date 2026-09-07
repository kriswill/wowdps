# `taken.txt` — expected values (R17: damage taken & mitigation)

Authoritative expected output for the R17 fixture. **Computed independently of the
Rust implementation** by `check.awk`'s destination-side arms (the validator's own
reading of the log grammar under CONTRACT.md R17). The machine-readable form is
`taken.expected.tsv`; this file is the same numbers with every derivation shown,
line by line.

Regenerate / check:

```sh
./verify.sh                                   # sample.txt AND taken.txt: PASS
./verify.sh taken.txt taken.expected.tsv      # PASS
./verify.sh corrupt.txt sample.expected.tsv   # must FAIL (negative control)
```

TSV columns: `segment kind name result dur_ms enc_id difficulty player metric
value`. Every (segment, player) row carries **27 metrics in a fixed order, always
emitted (zeros included)**: the twelve pre-existing ones — `damage overkill
petdamage dps pct heal overheal absorbheal interrupts cc dispels deaths` — then
the seven R17 ones (below), then R22 `self_harm`, then the seven R19 /
healing-received ones defined
in `support.expected.md` (in this log only `effective` = `damage` everywhere
and Zenlí's `healed_received` = `self_healed` = 22 000, the Expel Harm):

| metric | R17 definition (per player, per segment; pets fold onto owners) |
|---|---|
| `taken` | Σ `amount + absorbed` over damage events with the player/pet as DESTINATION — **excluding** 124255 Stagger self-ticks. Blocked is NOT added: the log's amount is already post-block. |
| `absorbed` | Σ the damage events' `absorbed` field (partial absorbs; the Taken row's `extra`) |
| `blocked` | Σ the damage events' `blocked` field (partial blocks) |
| `prevented` | `absorbed_full + blocked_full`: ABSORB misses' `amountMissed` + BLOCK misses' amount |
| `misses` | count of `*_MISSED` lines with a friendly destination, IMMUNE included |
| `stagger` | Σ `SPELL_ABSORBED` amounts whose absorb spell is in `NON_HEALING_ABSORBS` {114556, 31850, 31230, 115069} on the player (a subset of `absorbed`, never added again) |
| `stagger_ticked` | Σ the 124255 self-tick amounts (src = dst) that re-deal staggered damage — the owner's own AND their pets' |
| `self_harm` | R22: Σ `amount + absorbed` over damage events whose FOLDED source equals their folded destination — held off `damage` entirely (the monk's two stagger ticks plus his ox's one). The identity uses the `on_friendly` subset (destination guid `Player-`/`Pet-`), which here is 10 000 of the 12 500 |

`Mitigation.mitigated = absorbed + blocked + prevented`;
`mitigated_pct = mitigated / (taken + prevented)`. Both are derived, not emitted.

## Roster

- `W` = `Player-1168-0A1B2C11` "Durgan-Nebula-US", flags `0x511` — **Protection
  Warrior** (COMBATANT_INFO spec 73)
- `M` = `Player-1168-0A1B2C12` "Zenlí-Nebula-US", `0x514` — **Brewmaster Monk** (268)
- `F` = `Player-1168-0A1B2C13` "Pyralis-Nebula-US", `0x514` — **Fire Mage** (63)
- pet `Pet-0-4232-2662-31585-78116-0301A1B2D4` "Water Elemental", `0x1114` → owned
  by **F** (`SPELL_SUMMON` at 21:05:02, line 10)
- guardian `Creature-0-4232-2662-31585-73967-0301A1B2D5` "Niuzao", `0x1114` →
  owned by **M** (`SPELL_SUMMON` at 21:05:19.5, line 38). The Brewmaster's ox
  cooldown: it **stomps the boss for real damage** (l.40, ordinary pet damage
  folded onto M) and **takes a share of his stagger**, ticking on ITSELF (l.42).
  Its guid is `Creature-`, not `Pet-`, exactly as the game logs a summoned
  guardian — which is what holds R22's two halves apart here: the tick is
  `self_harm` but NOT `stagger_ticked`, because R17's destination universe is a
  friendly GUID. (The ox ticks its own spell, 324393, exactly as the live
  client logs it — 124255 is the monk's.)
- boss `Creature-…-215000-0000AB01` "Taken Test Boss", `0xa48`, raid flag `0x80`,
  max HP 300 000 (its health reports are in the players' target blocks, R16)
- add `Creature-…-215010-0000AB02` "Taken Test Add", `0xa48`, max HP 60 000
  (< half the boss: never a council member, R16)
- `Creature-0-4232-2552-0-1985-0000CC01` "Wandering Boar" — the trash tail

**Expected segment count: 2**, in order: Encounter "Taken Test Boss" (id 3145,
difficulty 16, **kill**, 60.000 s = 21:05:00 → 21:06:00, R4), then Trash (3.000 s,
R7 — see below for why the miss at 21:10:05 does not extend it). Nothing fires
between the `ZONE_CHANGE` in and `ENCOUNTER_START`, so no pre-pull Trash exists.

Line numbers below are `taken.txt`'s (1-based).

---

## Segment 1 — Encounter "Taken Test Boss" — KILL, 60.000 s, enc 3145 / diff 16

### Damage dealt (R1), healing (R2/R3)

| player | damage | overkill | pet dmg | DPS | pct | heal | overheal | absorbheal |
|---|---:|---:|---:|---:|---:|---:|---:|---:|
| F Pyralis | **227 000** | 25 000 | 12 000 | 3783.33 | 68.58 | 26 000 | 0 | 26 000 |
| W Durgan | **71 000** | 0 | 0 | 1183.33 | 21.45 | 12 000 | 0 | 12 000 |
| M Zenlí | **33 000** | 0 | 6 000 | 550.00 | 9.97 | 25 000 | 8 000 | 3 000 |

Segment total damage **331 000** (R22: the 12 500 of self-harm is not in it).

- **F 227 000** = 65 000 Fireball (l.39) + 120 000 Pyroblast (l.59) + 30 000 Fire
  Blast (l.60, the killing blow: overkill 25 000, boss HP report 0) + pet 12 000
  Waterbolt (l.41; the pet's `SPELL_DAMAGE` block describes the *target*, so
  ownership comes from the `SPELL_SUMMON` alone).
- **W 71 000** = 15 000 + 16 000 swings (l.11, l.57) + 40 000 Shield Slam (l.13).
- **M 33 000** = 18 000 Tiger Palm (l.21) + 9 000 swing (l.36) + pet 6 000
  Niuzao's Stomp (l.40; the guardian is summoned mid-pull at l.38 and its hit
  on the boss is ordinary pet damage, folded onto M — `petdamage` 6 000).
  **R22: the three self-ticks are NOT here** — 5 000 + 5 000 the monk deals to
  himself (l.27, l.31) and 2 500 Niuzao deals to itself (l.42, source and
  destination folding onto the same owner). They are `self_harm` 12 500
  instead — of which `on_friendly` is only the monk's 10 000. If you see
  45 500 here, the R22 exclusion is not being applied; if you see 35 500, the
  guardian half of the exclusion is missing (Niuzao's 2 500 back in `damage`)
  and if you see 43 000 the monk's own half is.
- Interrupts, CC, dispels, deaths: **0 for everyone** (the boss's `UNIT_DIED`,
  l.61, is not a player death).
- **Heals.** M: Expel Harm (l.35) amount 30 000 − overheal 8 000 = 22 000
  effective, plus 3 000 Celestial Brew absorb (l.33, 22-field, absorber = M) →
  heal 25 000 / overheal 8 000 / absorbheal 3 000. W: 12 000 Ignore Pain absorb
  (l.22, 22-field, absorber = W) → heal 12 000 = absorbheal. F: 21 000 + 5 000
  Ice Barrier absorbs (l.48, l.54) → 26 000. The two **Stagger** `SPELL_ABSORBED`
  lines (l.24 16 000, l.28 9 000; 19-field) are excluded from healing (R2) — if
  M's heal reads 50 000 the exclusion list is not applied.

### Damage taken (R17) — per player, line by line

#### W Durgan (the tank: block, parry, dodge, miss, partial absorb)

| line | ts | event | dst | amount | absorbed | blocked | → taken | → mitigation |
|---|---|---|---|---:|---:|---:|---:|---|
| 14 | :05.000 | `SWING_DAMAGE` boss→W | W | 42 000 | 0 | **18 000** | +42 000 | blocked +18 000 |
| 16 | :06.000 | `SWING_MISSED` boss→W **BLOCK**,nil,55 000 (12 fields) | W | — | — | — | count only | prevented +55 000, misses +1 |
| 17 | :07.000 | `SWING_MISSED` boss→W **PARRY** | W | — | — | — | count only | misses +1 |
| 18 | :08.000 | `SWING_MISSED` boss→W **DODGE** | W | — | — | — | count only | misses +1 |
| 19 | :09.000 | `SWING_MISSED` add→W **MISS** | W | — | — | — | count only | misses +1 |
| 20 | :09.500 | `RANGE_MISSED` add→W "Ember Shot" **MISS** (no ST/AOE trailer) | W | — | — | — | count only | misses +1 |
| 23 | :11.000 | `SPELL_DAMAGE` boss→W "Cinder Lash" | W | 30 000 | **12 000** | 0 | +42 000 | absorbed +12 000 |

- **taken = 42 000 + (30 000 + 12 000) = 84 000.** The 42 000 swing is post-block:
  the 18 000 blocked is *not* added (raw 60 000 is diagnostics only). If you see
  102 000 the blocked field is being added; if you see 72 000 the absorbed field
  is not.
- **absorbed 12 000**, **blocked 18 000**, **prevented 55 000** (the full BLOCK; the
  swing was the whole 55 000), **misses 5** (BLOCK, PARRY, DODGE, MISS, MISS),
  stagger 0, stagger_ticked 0.
- The 12 000 partial absorb has its R3 twin `SPELL_ABSORBED` (l.22, Ignore Pain,
  absorber = W) — that line credits W with 12 000 *healing* and is **never read on
  the taken side**: taken already holds the 12 000 through the damage line's
  `absorbed` field. Counting both makes taken 96 000.
- Derived: mitigated = 12 000 + 18 000 + 55 000 = 85 000;
  mitigated_pct = 85 000 / (84 000 + 55 000) = 61.15 %.

#### M Zenlí (stagger: the hit is taken in full once; the ticks are not taken)

| line | ts | event | dst | amount | absorbed | → taken | → mitigation |
|---|---|---|---|---:|---:|---:|---|
| 24 | :12.000 | `SPELL_ABSORBED` boss / M / absorber M, **115069 Stagger** 16 000 (19 fields) | M | — | — | not read | stagger +16 000 |
| 25 | :12.000 | `SWING_DAMAGE` boss→M | M | 24 000 | **16 000** | +40 000 | absorbed +16 000 |
| 27 | :13.000 | `SPELL_PERIODIC_DAMAGE` **124255 Stagger, M→M** | M | 5 000 | 0 | **excluded** | stagger_ticked +5 000 |
| 28 | :14.000 | `SPELL_ABSORBED` Stagger 9 000 (19 fields) | M | — | — | not read | stagger +9 000 |
| 29 | :14.000 | `SWING_DAMAGE` boss→M | M | 13 500 | **9 000** | +22 500 | absorbed +9 000 |
| 31 | :15.000 | `SPELL_PERIODIC_DAMAGE` 124255 Stagger, M→M | M | 5 000 | 0 | **excluded** | stagger_ticked +5 000 |
| 32 | :16.000 | `SPELL_DAMAGE` add→M "Ember Spit" (the plain hit) | M | 7 700 | 0 | +7 700 | — |
| 33 | :17.000 | `SPELL_ABSORBED` boss / M / "Smoldering" / absorber M, **322507 Celestial Brew** 3 000 (22 fields) | M | — | — | not read | (R3: heal +3 000, above) |
| 34 | :17.000 | `SPELL_PERIODIC_MISSED` boss→M "Smoldering" **ABSORB**,nil,3 000,3 000,nil,ST (18 fields) | M | — | — | count only | prevented +3 000, misses +1 |
| 42 | :21.500 | `SPELL_PERIODIC_DAMAGE` **324393 Stagger, Niuzao→Niuzao** (the ox's own stagger spell, not the monk's 124255) | guardian | 2 500 | 0 | **excluded** | — (a `Creature-` destination is outside R17; R22 `self_harm` +2 500) |

- **taken = 40 000 + 22 500 + 7 700 = 70 200.** Each staggered swing is taken in
  full on the hit — `amount + absorbed` = 24 000 + 16 000 and 13 500 + 9 000 — and
  the 5 000 ticks that re-deal part of that are **excluded** (they are src = dst,
  spell 124255); Niuzao's own 2 500 never reaches Taken at all. If you see
  80 200 the ticks are being taken twice.
- **absorbed 25 000** = 16 000 + 9 000. **stagger 25 000** = the two 115069
  `SPELL_ABSORBED` amounts: the same 25 000 seen from the shield's side — a subset
  of `absorbed`, reported, never added to anything. **stagger_ticked 10 000** — the monk's two
  5 000 ticks. Niuzao's own 2 500 is NOT here: R17 records on `Player-`/`Pet-`
  destinations, and the ox is a `Creature-` guardian. It is R22 `self_harm` all
  the same, which is why `self_harm` (12 500) exceeds `stagger_ticked`.
- **prevented 3 000** (the fully absorbed dot tick — an `ABSORB` miss on a
  `SPELL_PERIODIC_MISSED`; its `amountMissed` is at +2 after `missType`, indexed
  forward, the `ST` trailer ignored), **misses 1**, blocked 0.
- Derived: mitigated = 25 000 + 0 + 3 000 = 28 000 (stagger is *not* added
  again); mitigated_pct = 28 000 / (70 200 + 3 000) = 38.25 %.

#### F Pyralis (+ Water Elemental): immune, full absorb, deflect, reflect, resist, falling, a partial absorb, the pet

| line | ts | event | dst | amount | absorbed | → taken | → mitigation |
|---|---|---|---|---:|---:|---:|---|
| 8 | :01.000 | `SWING_DAMAGE` add→**pet** — BEFORE the pet's `SPELL_SUMMON` (l.10) | pet | 8 000 | 0 | +8 000 (F) | — |
| 43 | :22.000 | `SPELL_CAST_SUCCESS` F "Ice Block" | — | | | ignored | |
| 44 | :22.000 | `SPELL_AURA_APPLIED` F→F 45438 Ice Block BUFF | — | | | ignored | |
| 45 | :23.000 | `SPELL_MISSED` boss→F "Cinder Lash" **IMMUNE**,nil,ST (15 fields) | F | — | — | count only | misses +1 |
| 46 | :25.000 | `SPELL_AURA_APPLIED` F→F 11426 Ice Barrier BUFF,60 000 (14 fields) | — | | | ignored | |
| 47 | :26.000 | `SPELL_MISSED` boss→F "Ember Bolt" **ABSORB**,nil,21 000,21 000,nil,ST (18 fields) | F | — | — | count only | prevented +21 000, misses +1 |
| 48 | :26.000 | `SPELL_ABSORBED` boss / F / "Ember Bolt" / absorber F, Ice Barrier 21 000 (22-field twin of l.47) | F | — | — | **not read** | (R3: heal +21 000) |
| 49 | :27.000 | `SWING_MISSED` add→F **DEFLECT** | F | — | — | count only | misses +1 |
| 50 | :28.000 | `SPELL_MISSED` **F→add** "Fireball" **EVADE**,nil,ST | **add** | — | — | **nobody** | — |
| 51 | :29.000 | `SPELL_MISSED` add→F "Ember Spit" **REFLECT**,nil,ST | F | — | — | count only | misses +1 |
| 52 | :30.000 | `SPELL_MISSED` add→F "Frost Breath" **RESIST**,nil,ST | F | — | — | count only | misses +1 |
| 53 | :31.000 | `ENVIRONMENTAL_DAMAGE` nil→F **Falling** (39 fields; envType at off28, after the block) | F | 9 000 | 0 | +9 000 | — |
| 54 | :32.000 | `SPELL_ABSORBED` boss / F / "Cinder Lash" / absorber F, Ice Barrier 5 000 (22 fields) | F | — | — | not read | (R3: heal +5 000) |
| 55 | :32.000 | `SPELL_DAMAGE` boss→F "Cinder Lash" | F | 26 000 | **5 000** | +31 000 | absorbed +5 000 |
| 56 | :33.000 | `SPELL_DAMAGE` boss→**pet** "Cinder Lash" (after the summon) | pet | 4 000 | 0 | +4 000 (F) | — |

- **taken = 8 000 + 9 000 + 31 000 + 4 000 = 52 000.** Both pet hits fold onto F.
  The one at l.8 lands **before** the `SPELL_SUMMON` at l.10 and is a `SWING` from
  the *add*, whose advanced block describes the source — there is nothing on the
  line that names an owner. It must still reach F: the mitigation map is keyed by
  the raw destination guid and folded at *read* time, once ownership is known
  (`check.awk` gets the same answer through its two-pass owner map). If you see
  44 000 the pre-summon hit was lost; if you see a "Water Elemental" row, pets are
  not folding.
- The `ENVIRONMENTAL_DAMAGE` has a nil source and no spell block: 9 000 taken,
  labeled "Falling", attacker "Environment" — it deals damage to nobody's Damage
  row (no `actor` for the nil unit). Its layout is prefix 9 + advanced block 19 +
  `envType` + the 10-field damage suffix = 39; reading the amount at the spell
  offset yields the word `Falling`.
- **absorbed 5 000**, **prevented 21 000** (the full ABSORB miss's `amountMissed`;
  its `SPELL_ABSORBED` twin at l.48 is the *healing* side of the same 21 000 — R3
  credits F's Ice Barrier, R17 reads only the miss; taken never moves), blocked 0,
  **misses 5** (IMMUNE, ABSORB, DEFLECT, REFLECT, RESIST), stagger 0,
  stagger_ticked 0.
- **The EVADE at l.50 is the ADD evading F's Fireball**: its destination is an
  NPC, so it is nobody's taken, nobody's miss. It is in the fixture precisely to
  prove a miss is counted on its destination, never its source. If F's misses read
  6, the miss arm attributes to the source.
- Derived: mitigated = 5 000 + 0 + 21 000 = 26 000;
  mitigated_pct = 26 000 / (52 000 + 21 000) = 35.62 %.

### The identity (R17 taken = dealt, in its R22 form)

Σ taken over players = Σ (`amount + absorbed`) over every damage event with a
friendly destination − the Stagger self-ticks:

```
friendly-destination (`Player-` / `Pet-` guid) damage events, amount + absorbed:
  l.14   42 000 +      0  =  42 000   (W, swing, partial block)
  l.23   30 000 + 12 000  =  42 000   (W, Cinder Lash, partial absorb)
  l.25   24 000 + 16 000  =  40 000   (M, staggered swing)
  l.27    5 000 +      0  =   5 000   (M, Stagger tick — self-sourced)
  l.29   13 500 +  9 000  =  22 500   (M, staggered swing)
  l.31    5 000 +      0  =   5 000   (M, Stagger tick — self-sourced)
  l.32    7 700 +      0  =   7 700   (M, Ember Spit)
  l.53    9 000 +      0  =   9 000   (F, Falling)
  l.55   26 000 +  5 000  =  31 000   (F, Cinder Lash, partial absorb)
  l.8     8 000 +      0  =   8 000   (pet → F, pre-summon)
  l.56    4 000 +      0  =   4 000   (pet → F)
  ------------------------------------
  Σ                        216 200
  − Stagger ticks (l.27 + l.31)   − 10 000
  ====================================
                           206 200

(l.42, Niuzao's own tick, is not in this list at all: the ox is a `Creature-`
guardian, so it is not a friendly DESTINATION. R22 still keeps it off the
Damage rows — `self_harm` 12 500 — but only its `on_friendly` 10 000 is part
of this identity.)

Σ taken:  W 84 000 + M 70 200 + F 52 000 = 206 200   ✓
```

Seen from the attackers' Damage `by_target` rows (the meter keeps NPC actors):
boss → Durgan 84 000, boss → Zenlí 62 500, boss → Pyralis 31 000, boss → Water
Elemental 4 000; add → Zenlí 7 700, add → Water Elemental 8 000; Environment →
Pyralis 9 000. **R22: there is no `Zenlí → Zenlí` row any more, nor a
`Niuzao → Niuzao` one** — self-harm never reaches a Damage row at all. Over
friendly names the by-target side sums to 206 200, and the identity balances
with the two tallies that hold what each side excludes:

```
  Σ Damage by_target over friendly names       206 200
+ Σ self_harm_on_friendly (R22, off Damage)    10 000
= 216 200
= Σ Taken rows                                206 200
+ Σ stagger_ticked        (R17, off Taken)     10 000
```

The misses (13 lines against friendly destinations) contribute 0 to both sides.

## Segment 2 — Trash, 3.000 s (21:10:00 → 21:10:03)

Out of the raid (`ZONE_CHANGE` to Dornogal, difficulty 0, l.63). Only W has a row.

| player | damage | DPS | pct | taken | absorbed | blocked | prevented | misses | stagger | stagger_ticked |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| W Durgan | **6 000** | 2000.00 | 100.00 | **1 500** | 0 | 0 | 0 | **1** | 0 | 0 |

- l.64 W swings the boar for 6 000 (opens the Trash at 21:10:00); l.66 the boar
  swings W for 1 500 → taken 1 500.
- l.68 `SWING_MISSED` boar→W **DODGE** at 21:10:05: **misses 1 — and the segment
  still ends at 21:10:03.** A miss records into the open segment but never extends
  it (R17: never touches `last_ms`; the scanner ignores `*_MISSED`). If the trash
  duration reads 5 000 ms, the miss path is touching the segment clock and the
  index scanner and meter have fallen out of lockstep.

---

## Every `MissKind`, and where it lands

| kind | line | src → dst | counted? |
|---|---|---|---|
| DODGE | 18 (boss→W), 65 (boar→W) | friendly dst | yes (W ×2 across segments) |
| PARRY | 17 | boss→W | yes |
| BLOCK | 16 | boss→W, amount 55 000 | yes; prevented |
| MISS | 19 (`SWING_MISSED`), 20 (`RANGE_MISSED`) | add→W | yes ×2 |
| ABSORB | 44 (`SPELL_MISSED`), 34 (`SPELL_PERIODIC_MISSED`) | boss→F, boss→M | yes; prevented 21 000 / 3 000 |
| IMMUNE | 42 | boss→F (Ice Block) | yes |
| DEFLECT | 46 | add→F | yes |
| EVADE | 47 | **F→add** | **no** — NPC destination |
| REFLECT | 48 | add→F | yes |
| RESIST | 49 | add→F | yes |

Total `*_MISSED` lines: 14; with a friendly destination: 13 = W 5 + M 1 + F 5 +
W 1 (trash). Every shape from FORMAT-NOTES is present: `SWING_MISSED` 11 / 12,
`SPELL_MISSED` 15 / 18, `SPELL_PERIODIC_MISSED` 18, `RANGE_MISSED` (no trailer).

## Edge shapes deliberately present

| shape | line | expected behaviour |
|---|---|---|
| pet hit BEFORE its `SPELL_SUMMON`, from a `SWING` whose block is the *source* | 8 vs 10 | taken by F (read-time fold), no pet row |
| partial block (`blocked` 18 000, amount post-block) | 14 | taken 42 000, blocked 18 000 — blocked never added |
| full BLOCK miss with amount (12-field `SWING_MISSED`) | 16 | prevented 55 000, taken +0 |
| partial absorb with its R3 `SPELL_ABSORBED` twin | 22 + 23, 51 + 52 | taken via the damage line's `absorbed` only; the twin is healing |
| 19-field Stagger `SPELL_ABSORBED` + swing with matching `absorbed` | 24 + 25, 28 + 29 | taken in full once; stagger 25 000 |
| 124255 Stagger self-tick (src = dst) | 27, 31 | NOT taken; stagger_ticked; still damage dealt (R1) |
| full ABSORB miss with its 22-field `SPELL_ABSORBED` twin | 44 + 45 | prevented 21 000; healing 21 000; taken +0 |
| `SPELL_PERIODIC_MISSED` ABSORB with `ST` trailer | 34 | amountMissed read forward from missType |
| `RANGE_MISSED` without trailer | 20 | miss counted; no width assumption |
| IMMUNE after `SPELL_CAST_SUCCESS` + `SPELL_AURA_APPLIED` Ice Block | 40–42 | counted as a miss; the cast/aura lines are `Other` for Taken |
| EVADE by an NPC | 47 | counted by nobody |
| `ENVIRONMENTAL_DAMAGE` (39 fields, nil source) | 50 | taken 9 000, labeled Falling / Environment |
| killing blow with overkill | 57 | F overkill 25 000; boss best_pct 0 on the kill (R16) |
| miss after the last combat event of a Trash segment | 65 | counted, duration unchanged (3 000 ms) |
