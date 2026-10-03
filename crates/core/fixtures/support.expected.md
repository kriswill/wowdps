# `support.txt` — expected values (R19: support attribution; R2 amendment: the healing split and healing received)

Authoritative expected output for the R19 fixture. **Computed independently of the
Rust implementation** by `check.awk`'s support arms (the validator's own reading of
the log grammar under CONTRACT.md R19 and the R2 amendment, as written in
`docs/plan-role-pivots-step3.md` §0). The machine-readable form is
`support.expected.tsv`; this file is the same numbers with every derivation shown,
line by line.

Regenerate / check:

```sh
./verify.sh                                   # sample, taken AND support: PASS
./verify.sh support.txt support.expected.tsv  # PASS
./verify.sh corrupt.txt sample.expected.tsv   # must FAIL (negative control)
```

TSV columns: `segment kind name result dur_ms enc_id difficulty player metric
value`. Every (segment, player) row carries **26 metrics in a fixed order, always
emitted (zeros included)**: the twelve original ones (`damage overkill petdamage
dps pct heal overheal absorbheal interrupts cc dispels deaths`), the seven R17
ones (`taken absorbed blocked prevented misses stagger stagger_ticked`), then the
seven new ones:

| metric | definition (per player, per segment; pets fold onto owners) |
|---|---|
| `support_given` | Σ share amounts over the four damage-support families whose trailing **supporter guid** is the player (R19) |
| `support_received` | Σ share amounts over the same families whose **source** is the player or their pet (R19) |
| `support_given_heal` | as `support_given`, over `SPELL_HEAL_SUPPORT` / `SPELL_PERIODIC_HEAL_SUPPORT` (`amount − overheal`) |
| `support_received_heal` | as `support_received`, over the heal-support families |
| `healed_received` | Σ R2 effective healing (`amount − overheal`) landing on the player or their pet from **any** source — NPC heals included; `NON_HEALING_ABSORBS` excluded; **absorbs are not received healing**; heal-support shares are not received healing |
| `self_healed` | the subset of `healed_received` with `src guid == dst guid` |
| `effective` | **derived**: `damage − support_received + support_given` — never stored; readers compute it |

**The share is a read value.** A support line's amount is whatever the game
logged as the buff's share of the hit; the meter (and `check.awk`) READ it and
never compute it from the hit. This fixture uses round shares (Ebon Might 1 % of
the Fireballs, Prescience 8 % of the Mortal Strike and of the Execute) purely so
the arithmetic is checkable by eye — real ratios are ~0.5–1 % and ~7–10 % on
crits, and nothing in the rulings depends on the ratio. Every support line but
one carries `absorbed` 0: the share of the Fireball the boss's shield took
whole (l.38) is logged the way a real log writes it, amount 0 + `absorbed` 200,
so an implementation that drops a support line's `absorbed` field (which R1
adds for a hit) loses those 200 from E's given and M's received.

## Roster

- `E` = `Player-1168-0A1B2C21` "Vessyra-Nebula-US", flags `0x511` — **Augmentation
  Evoker** (COMBATANT_INFO spec 1473). The supporter on every `_SUPPORT` line.
- `M` = `Player-1168-0A1B2C22` "Ignatia-Nebula-US", `0x514` — **Fire Mage** (63)
- pet `Pet-0-4232-2662-31585-78116-0401A1B2E5` "Water Elemental", `0x1114` → owned
  by **M** (`SPELL_SUMMON` at 22:05:01, l.9 — before its first act this time)
- `W` = `Player-1168-0A1B2C23` "Brakkar-Nebula-US", `0x514` — **Arms Warrior** (71)
- `H` = `Player-1168-0A1B2C24` "Seraphíne-Nebula-US", `0x514` — **Holy Priest** (257)
- boss `Creature-…-216000-0000AC01` "Support Test Boss", `0xa48`, raid flag `0x80`,
  max HP 500 000 (its health reports are in the players' target blocks, R16; a
  support line repeats its hit's report — a share is part of the hit, not more
  damage, so the boss does not lose HP twice)
- add `Creature-…-216010-0000AC02` "Support Test Add", `0xa48`, max HP 80 000
- `Creature-…-216020-0000AC03` "Earthen Ward", `0xa18` (Outsider + Friendly +
  NPCControlled + NPC) — a friendly NPC with no owner: nobody's meter row
- `Creature-0-4232-2552-0-1985-0000CC02` "Wandering Boar" — the trash tail

**Expected segment count: 2**, in order: Encounter "Support Test Boss" (id 3146,
difficulty 16, **kill**, 60.000 s = 22:05:00 → 22:06:00, R4), then Trash (2.000 s,
R7: 22:10:00 → 22:10:02). Nothing fires between the `ZONE_CHANGE` in and
`ENCOUNTER_START`, so no pre-pull Trash exists. Every support line directly
follows its hit at the same timestamp (as in real logs); none precedes a pull's
first hit.

Line numbers below are `support.txt`'s (1-based).

---

## Segment 1 — Encounter "Support Test Boss" — KILL, 60.000 s, enc 3146 / diff 16

### Damage dealt (R1) — unchanged by R19

| player | damage | overkill | pet dmg | DPS | pct |
|---|---:|---:|---:|---:|---:|
| M Ignatia | **291 000** | 0 | 9 000 | 4850.00 | 48.50 |
| W Brakkar | **239 500** | 2 500 | 0 | 3991.67 | 39.92 |
| E Vessyra | **69 500** | 0 | 0 | 1158.33 | 11.58 |
| H Seraphíne | 0 | 0 | 0 | 0.00 | 0.00 |

Segment total damage **600 000** (R1 since 2026-10-02: W's two killing blows'
2 500 overkill is out of the amount and stays in `overkill`; it was 602 500).

- **M 291 000** = 40 000 + 42 000 + 44 000 Fireball on the boss (l.14, 26, 33)
  + **20 000 Fireball the boss's shield took whole** (l.37, a `SPELL_MISSED`
  ABSORB — R1 counts it as a hit, amount 0 + absorbed 20 000) + 6 000 Ignite (l.24) + 30 000 Fireball on the add (l.55) + 100 000 Pyroblast
  (l.61) + pet 9 000 Waterbolt (l.22).
- **W 239 500** = 12 000 + 13 000 swings (l.17, 30; their `_LANDED` twins l.18,
  31 are the same swings) + 35 000 Mortal Strike (l.20) + 50 000 Mortal Strike on
  the add (l.56, the killing blow's 52 000 less its overkill 2 000) + 129 500
  Execute (l.62, the boss's killing blow's 130 000 less its overkill 500).
  Overkill 2 500, reported apart.
- **E 69 500** = 30 000 + 32 000 Eruption (l.16, 60) + **7 500 Bombardments
  (l.28)** — the plain `SPELL_DAMAGE` half of the twice-logged proc. R1 counts
  it here, once; see the self-support case below.
- **Every `*_SUPPORT` line contributes 0 to `damage`** (R1: `Other` for the
  Damage view). If M reads 292 850, W 254 250 or E 77 000, the support shares
  are being added to damage — R1 has not moved.
- Interrupts, CC, dispels, deaths: 0 for everyone (the add's and the boss's
  `UNIT_DIED`, l.57 / l.65, are not player deaths).

### Support (R19) — every support line, in order

| line | ts | event | src → dst | buff (the spell block) | share | supporter | → given | → received |
|---|---|---|---|---|---:|---|---|---|
| 15 | :03 | `SPELL_DAMAGE_SUPPORT` | M → boss | Ebon Might 395152 | 400 | E | E +400 | M +400 |
| 19 | :05 | `SWING_DAMAGE_LANDED_SUPPORT` (**42 fields**, spell-shaped) | W → boss | Ebon Might | 120 | E | E +120 | W +120 |
| 21 | :06 | `SPELL_DAMAGE_SUPPORT` | W → boss | Prescience 410089 | 2 800 | E | E +2 800 | W +2 800 |
| 23 | :07 | `SPELL_DAMAGE_SUPPORT` | **pet** → boss | Ebon Might | 90 | E | E +90 | **M** +90 |
| 25 | :08 | `SPELL_PERIODIC_DAMAGE_SUPPORT` | M → boss | Shifting Sands 413984 | 300 | E | E +300 | M +300 |
| 27 | :09 | `SPELL_DAMAGE_SUPPORT` | M → boss | Ebon Might | 420 | E | E +420 | M +420 |
| 29 | :10 | `SPELL_DAMAGE_SUPPORT` | **E** → boss | Bombardments 434481 | 7 500 | **E** | E +7 500 | **E** +7 500 |
| 32 | :11 | `SWING_DAMAGE_LANDED_SUPPORT` | W → boss | Ebon Might | 130 | E | E +130 | W +130 |
| 34 | :12 | `SPELL_DAMAGE_SUPPORT` | M → boss | Ebon Might | 440 | E | E +440 | M +440 |
| 36 | :12 | `SPELL_DAMAGE_SUPPORT` | **Earthen Ward (NPC)** → boss | Ebon Might | 600 | E | **nobody** | **nobody** |
| 38 | :12 | `SPELL_DAMAGE_SUPPORT` (amount 0, absorbed 200) | M → boss | Ebon Might | 200 | E | E +200 | M +200 |
| 63 | :55 | `SPELL_DAMAGE_SUPPORT` | W → boss | Ebon Might | 1 300 | E | E +1 300 | W +1 300 |
| 64 | :55 | `SPELL_DAMAGE_SUPPORT` | W → boss | Prescience | 10 400 | E | E +10 400 | W +10 400 |

| player | support_given | support_received | **effective** = damage − received + given |
|---|---:|---:|---:|
| E Vessyra | **24 100** | **7 500** | 69 500 − 7 500 + 24 100 = **86 100** |
| M Ignatia | 0 | **1 850** | 291 000 − 1 850 = **289 150** |
| W Brakkar | 0 | **14 750** | 239 500 − 14 750 = **224 750** |
| H Seraphíne | 0 | 0 | **0** |

- **E given 24 100** = 400 + 120 + 2 800 + 90 + 300 + 420 + 7 500 + 130 + 440
  + 200 + 1 300 + 10 400 — the Earthen Ward's 600 is not in it (below).
- **M received 1 850** = 400 + 90 + 300 + 420 + 440 + 200 — the **90 is the Water
  Elemental's** (l.23: `src` is the pet, flags `0x1114`, and the support line's
  advanced block describes the *target*, so its `owner_guid` is zero — ownership
  comes from the `SPELL_SUMMON` at l.9, folded at read time). If M reads 1 760
  the buffed pet's share is being lost; if a "Water Elemental" row appears, pets
  are not folding.
- **W received 14 750** = 120 + 2 800 + 130 + 1 300 + 10 400. The two swings'
  shares (l.19, 32) are `SWING_DAMAGE_LANDED_SUPPORT` lines: **42 fields, the
  amount at the SPELL offset (31)**, not the swing offset (28). Read at the fixed
  swing offset they yield the advanced block's `ui_map_id` (2287); read through
  this parser's swing path (which probes f[9] for the advanced block and finds
  the buff's spell id, not a guid) they yield that spell id, 395152 — never 120
  and 130. Each has its
  plain `SWING_DAMAGE` + `SWING_DAMAGE_LANDED` twin, as R1 demands; the twins
  stay one 12 000 / 13 000 hit each.
- **l.63 + l.64 — two shares on one hit.** The Execute (130 000) carries an Ebon
  Might share AND a Prescience share; shares are additive (1 300 + 10 400 =
  11 700, far under the hit) and each is read as logged.
- **l.28 + l.29 — the self-support case.** Bombardments is a proc the Evoker
  owns outright, and the game logs it TWICE: a plain `SPELL_DAMAGE` from E
  (7 500 → R1 damage) and a `SPELL_DAMAGE_SUPPORT` whose `src` AND supporter are
  both E for the same 7 500. Under R19 that is `given` +7 500 and `received`
  +7 500 on the same player — they cancel in `effective`, so the proc is counted
  **once, by R1**. A naive `damage + given` gives E 93 600 and breaks the
  partition below. E's `effective` 86 100 = own 62 000 (Eruptions) + 7 500 (the
  proc, once) + 16 600 (shares on others' hits).
- **l.35 + l.36 — an NPC ally's share lands on nobody** (R19). The Earthen
  Ward, an unowned friendly NPC, slams the boss for 30 000 and the Ebon Might on
  it logs a 600 share naming E. Its hit is on no Damage row, so crediting the
  share would break the partition: it is neither E's given nor anyone's
  received. If E reads 24 700 given, it is being credited.
- **l.37 + l.38 — a hit the boss's shield took whole (R1).** The Fireball is
  logged as `SPELL_MISSED … ABSORB,nil,20000,20000,1,ST`, and the game still
  shares it: the Ebon Might line carries amount 0 and absorbed 200, the shape a
  real log writes. R1 counts the hit as M's damage — 20 000, a crit, and not a
  miss (`misses_dealt` stays 0) — so the 200 received nets against damage that
  is on the row. If M reads 271 000 the hit was dropped and the partition
  below is off by 20 000 − 200.
- **l.50 `SPELL_ABSORBED_SUPPORT` (20 fields) changes nothing**: no row, no
  metric, no healing, no stagger. Its spell block is the buff (Shifting Sands),
  the underlying shield is unknowable, so it stays `Other`. If any of W's or E's
  numbers moves by 500, that line is being read.
- **Unsupported hits exist**: the Fireball on the add (l.55) lands after Ebon
  Might fell off M (`SPELL_AURA_REMOVED`, l.51) and has no support line; the
  Eruptions, the Pyroblast and the add Mortal Strike have none either. Support
  is per line, never inferred from an aura.
- Support lines never open, extend or split a segment and never fire an R8
  signal or a marker; E's spec comes from `COMBATANT_INFO` (1473), not from a
  support line.

### Healing (R2 / R3) and healing received (the R2 amendment)

| line | ts | event | src → dst | amount | overheal | effective | → H heal | → received |
|---|---|---|---|---:|---:|---:|---|---|
| 40 | :14 | `SPELL_ABSORBED` (19 fields; absorber **H**, defender W) | boss / W / H, Power Word: Shield 17 | 15 000 | — | 15 000 | +15 000 (absorbheal) | **not received** (R3) |
| 42 | :15 | `SPELL_HEAL` Flash Heal | H → W | 30 000 | 5 000 | 25 000 | +25 000 | W +25 000 |
| 43 | :15 | `SPELL_HEAL_SUPPORT` Fate Mirror 413786 (**37 fields**) | H → W, supporter E | 2 000 | 0 | 2 000 | — | E given_heal +2 000, H received_heal +2 000 (received is keyed by the line's SOURCE — the healer whose heal was amplified — never the heal's target) |
| 44 | :17 | `SPELL_PERIODIC_HEAL` Renew | H → **H** | 8 000 | 0 | 8 000 | +8 000 | H +8 000, **self** +8 000 |
| 45 | :17 | `SPELL_PERIODIC_HEAL_SUPPORT` Shifting Sands (37 fields) | H → H, supporter E | 100 | 0 | 100 | — | E given_heal +100, H received_heal +100 |
| 46 | :18 | `SPELL_HEAL` "Earthen Mending" | **Earthen Ward (NPC)** → W | 6 000 | 1 000 | 5 000 | **no row** | W +5 000 |
| 47 | :19 | `SPELL_ABSORBED` (19 fields) **115069 Stagger** | boss / W / W | 4 000 | — | — | **excluded** (R2) | not received; R17 `stagger` W +4 000 |
| 52 | :21 | `SPELL_HEAL` Flash Heal | H → W | 28 000 | 8 000 | 20 000 | +20 000 | W +20 000 |
| 53 | :22 | `SPELL_HEAL` Flash Heal | H → **pet** | 5 000 | 0 | 5 000 | +5 000 | **M** +5 000 |
| 58 | :30 | `SPELL_PERIODIC_HEAL` Renew | H → H | 8 000 | 3 000 | 5 000 | +5 000 | H +5 000, self +5 000 |
| 59 | :31 | `SPELL_HEAL` Flash Heal | H → E | 10 000 | 0 | 10 000 | +10 000 | E +10 000 |

| player | heal | overheal | absorbheal | support_given_heal | support_received_heal | healed_received | self_healed |
|---|---:|---:|---:|---:|---:|---:|---:|
| H Seraphíne | **88 000** | **16 000** | **15 000** | 0 | **2 100** | **13 000** | **13 000** |
| W Brakkar | 0 | 0 | 0 | 0 | 0 | **50 000** | 0 |
| M Ignatia | 0 | 0 | 0 | 0 | 0 | **5 000** | 0 |
| E Vessyra | 0 | 0 | 0 | **2 100** | 0 | **10 000** | 0 |

- **H heal 88 000** = effective 73 000 (25 000 + 8 000 + 20 000 + 5 000 + 5 000
  + 10 000) + absorb 15 000 (l.40, absorber = H — a 19-field line whose absorber
  is NOT the defender: arity is discriminated by width, never by absorber
  identity). **overheal 16 000** = 5 000 + 8 000 + 3 000. **absorbheal 15 000**
  — the absorber-credited R3 total, what the amendment calls `absorbed` per
  player (≤ healing holds: 15 000 ≤ 88 000). The Stagger absorb (l.47) is
  excluded: if H's heal reads 92 000 the exclusion list is not applied.
- **W healed_received 50 000** = 25 000 + 20 000 from H **+ 5 000 from the
  Earthen Ward** (l.46): an NPC heal on a player counts, symmetric with R17
  counting NPC attackers; the NPC itself earns no row (`0xa18` is neither a
  player nor an owned pet). The 15 000 PWS absorb is **not** received healing (a
  consumed shield is damage prevented, already in W's R17 `absorbed`), and
  neither is the 2 000 Fate Mirror share (it is the supporter's share of the
  Flash Heal already counted at l.42). If W reads 45 000 the NPC heal is
  dropped; 65 000 means absorbs are being received; 52 000 means heal-support
  shares are.
- **H healed_received 13 000 = self_healed 13 000**: both Renew ticks (l.44,
  l.58) are `src == dst`; the second is 8 000 − 3 000 overheal. The Shifting
  Sands heal share (l.45, 100) is `support_received_heal`, not received healing.
- **M healed_received 5 000** is the Flash Heal on the Water Elemental (l.53):
  a heal on a pet is its owner's received (raw-keyed, folded at read). Not
  self-healed (H ≠ pet).
- **E given_heal 2 100** = 2 000 (Fate Mirror, l.43) + 100 (Shifting Sands,
  l.45) = **Σ received_heal** = H 2 100 (both heal shares ride her own heals; the Warrior, the Fate Mirror heal's TARGET, receives nothing — `received` is keyed by the line's source). (In real logs a Fate Mirror
  heal-support line's `src` is the Prescience target, as here: the share rides
  the buffed player's own heal.)

### Damage taken (R17) — unchanged by R19

| player | taken | absorbed | blocked | prevented | misses | stagger | stagger_ticked |
|---|---:|---:|---:|---:|---:|---:|---:|
| W Brakkar | **64 000** | **19 000** | 0 | 0 | 0 | **4 000** | 0 |
| M Ignatia | **4 000** | 0 | 0 | 0 | 0 | 0 | 0 |
| E, H | 0 | 0 | 0 | 0 | 0 | 0 | 0 |

- W: Cinder Lash (l.41) 25 000 + absorbed 15 000 = 40 000; the swing (l.48)
  20 000 + absorbed 4 000 = 24 000 → 64 000; absorbed 19 000; the 19-field
  Stagger `SPELL_ABSORBED` (l.47) → stagger 4 000 (a subset of absorbed, never
  added). M: the boss's Cinder Lash on the pet (l.54) 4 000, folded. Support
  lines are never taken (their `dst` is the boss anyway).

### The identities

**1. Σ effective = Σ damage** (R19: a true partition of the raid's damage):

```
effective:  E 86 100 + M 289 150 + W 224 750 + H 0 = 600 000
damage:     E 69 500 + M 291 000 + W 239 500 + H 0 = 600 000   ✓
```

**2. Σ given = Σ received**, damage and healing separately (a share whose `src`
folds to no player — the Earthen Ward's — is on neither side, so nothing leaks):

```
damage:   given  E 24 100                          = 24 100
          received  E 7 500 + M 1 850 + W 14 750   = 24 100   ✓
healing:  given  E 2 100                           =  2 100
          received  W 2 000 + H 100                =  2 100   ✓
```

(1 follows from 2: Σ effective = Σ damage − Σ received + Σ given.)

**3. Σ healed_received from PLAYER sources + Σ absorbs credited on friendly
targets = Σ Healing `by_target` over friendly names** (the Healing rows carry
heals AND absorb credit; only player actors have rows):

```
healed_received from player sources:
  W  25 000 + 20 000            =  45 000   (H's Flash Heals; the 5 000 Earthen Ward heal is NPC-sourced: out)
  H  8 000 + 5 000              =  13 000
  M  5 000                      =   5 000   (the pet, folded)
  E  10 000                     =  10 000
                                  -------
                                   73 000
absorbs credited on friendly targets (R3):
  l.40 PWS on W                 =  15 000   (l.47 Stagger: excluded from healing, not a credit)
                                  -------
                                   88 000

H's Healing by_target:  Brakkar 60 000 (25 000 + 20 000 + 15 000 absorb)
                      + Seraphíne 13 000 + Water Elemental 5 000 + Vessyra 10 000
                                = 88 000   ✓  (= H's heal row, 88 000)
```

The NPC heal is the one term where `healed_received` (W 50 000, all sources)
and the identity's player-source sum (W 45 000) differ, by ruling.

## Segment 2 — Trash, 2.000 s (22:10:00 → 22:10:02)

Out of the raid (`ZONE_CHANGE` to Dornogal, difficulty 0, l.67).

| player | damage | DPS | pct | support_given | support_received | effective | taken |
|---|---:|---:|---:|---:|---:|---:|---:|
| M Ignatia | **8 000** | 4000.00 | 57.14 | 0 | **80** | **7 920** | 0 |
| W Brakkar | **6 000** | 3000.00 | 42.86 | 0 | 0 | **6 000** | **1 500** |
| E Vessyra | **0** | 0.00 | 0.00 | **80** | 0 | **80** | 0 |

- l.68 W swings the boar (opens the Trash at 22:10:00); l.70 M's Fireball 8 000
  with an Ebon Might share of 80 (l.71); l.72 the boar hits W for 1 500 → taken.
- **E has a row with no damage**: a supporter whose only presence in a segment
  is `given` still gets a (segment, player) row in the TSV — damage 0, dps 0,
  pct 0, `effective` 80. The same shape appears in `sample.expected.tsv`
  (`Player-1168-0A1B2C04`, segment 2, given 29 400) — see its addendum.
- Σ effective 7 920 + 6 000 + 80 = 14 000 = Σ damage ✓; given 80 = received 80 ✓.

---

## Edge shapes deliberately present

| shape | line | expected behaviour |
|---|---|---|
| Ebon Might share on a `SPELL_DAMAGE` | 15, 27, 34, 71 | given E / received M, read as logged |
| an NPC ally's hit with an Ebon Might share | 35 + 36 | the share lands on nobody (R19); no row for the NPC |
| a hit the target's shield took whole (`SPELL_MISSED` ABSORB, crit) with its share (amount 0 + absorbed) | 37 + 38 | M damage +20 000, a hit not a miss (R1); share 200 given E / received M |
| Prescience share on a `SPELL_DAMAGE` (crit) | 21 | given E 2 800 / received W |
| `SWING_DAMAGE_LANDED_SUPPORT` — 42 fields, SPELL-shaped, with its swing + `_LANDED` twins | 17–19, 30–32 | share at $32: 120 / 130; the swing counted once |
| `SPELL_PERIODIC_DAMAGE_SUPPORT` (Shifting Sands on an Ignite tick) | 25 | given E 300 / received M |
| support `src` = a pet (block's owner_guid zero) | 23 | received folds to M via `SPELL_SUMMON` |
| the twice-logged proc: plain `SPELL_DAMAGE` + self-supported `SPELL_DAMAGE_SUPPORT` | 28 + 29 | damage 7 500 once (R1); given = received = 7 500 on E; effective unchanged |
| two shares on one hit (Ebon Might + Prescience) | 63 + 64 | additive, 11 700 on W |
| `SPELL_HEAL_SUPPORT` (37 fields, Fate Mirror) | 43 | given_heal E 2 000 / received_heal H (the source, not the target W); NOT healed_received |
| `SPELL_PERIODIC_HEAL_SUPPORT` (37 fields, Shifting Sands, src = dst) | 45 | given_heal E 100 / received_heal H; NOT self_healed |
| `SPELL_ABSORBED_SUPPORT` (20 fields) | 50 | **nothing changes** |
| 19-field `SPELL_ABSORBED` with absorber ≠ defender (PWS, absorber H on W) | 40 | H absorbheal 15 000; W: not received healing |
| Stagger-family `SPELL_ABSORBED` on a warrior | 47 | excluded from healing; W stagger 4 000 |
| NPC-sourced `SPELL_HEAL` on a player | 46 | W healed_received +5 000; no row for the NPC |
| heal on a pet | 53 | M healed_received +5 000 |
| self-heal with overheal | 58 | H self_healed +5 000 (8 000 − 3 000) |
| Flash Heals with overheal | 42, 52 | H overheal 13 000 of the 16 000 |
| `SPELL_AURA_REMOVED` Ebon Might before an unsupported Fireball | 51, 55 | support is per line, never inferred |
| killing blows with overkill | 56, 62 | W overkill 2 500; boss best_pct 0 on the kill (R16) |
| supporter-only row in a segment | 71 (Trash) | E: damage 0, given 80, effective 80 |

## Ambiguities resolved here (assumptions, stated)

1. **Support amount and the `absorbed` field.** `check.awk` reads `base_amount
   + absorbed` on a damage-support line, as R1 does for a hit. One line here
   (l.38, the share of a hit the shield took whole) carries amount 0 and
   absorbed 200, as real logs write that share, so the goldens depend on it.
2. **The supporter guid is taken raw** (`$NF`), never through the pet-owner
   map: the ruling says the supporter is the player. A `nil`/zero supporter is
   skipped (the parser turns such a line into `Other`); none occur here.
3. **Heal-support effective share = `amount − overheal`**, as R2 reads a heal;
   every heal-support line here has overheal 0.
4. **Fate Mirror's `src`** is the buffed player (the real-log shape), so the
   heal-support line rides H's heal on W rather than being a heal from E — there
   is no plain-heal twin from E in this fixture, unlike Bombardments.
5. **`healed_received` counts `SPELL_HEAL` / `SPELL_PERIODIC_HEAL` only**: not
   absorbs (R3), not heal-support shares (they duplicate a counted heal), and
   not the `NON_HEALING_ABSORBS` family (R2).
6. **Segment clock**: a support line is passive — it never opens, extends or
   splits a segment. Here every support line shares its hit's timestamp, so no
   duration depends on that; the trash still ends at the boar's hit (22:10:02).
