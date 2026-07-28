# `sample.txt` — expected values

Authoritative expected output for the canonical fixture. **Computed independently of
the Rust implementation** by `check.awk` (this validator's own reading of the log
grammar). The machine-readable form is `sample.expected.tsv`; this file is the same
numbers with the derivations shown.

Regenerate / check:

```sh
./verify.sh sample.txt sample.expected.tsv    # PASS
./verify.sh corrupt.txt sample.expected.tsv   # must FAIL (negative control)
```

## Method

- Semantics are CONTRACT.md **R1–R6**. Field offsets are `FORMAT-NOTES.md`, verified
  against a real retail log (build 12.0.7, 15910 lines).
- **Damage** (R1) = per-event `base_amount + absorbed` field. `SWING_DAMAGE` counted,
  `SWING_DAMAGE_LANDED` **not** (same swing). `*_SUPPORT` and `DAMAGE_SPLIT` excluded.
  `extra` = overkill clamped to ≥ 0 (raw value is `-1` on non-killing blows).
- **Healing** (R2) = effective (`amount − overheal`), where `amount` at offset 32
  *includes* overheal. `extra` = overheal. `SPELL_ABSORBED` credits the **absorber**
  with healing and has no overheal component. Stagger / cheat-death shield IDs
  (114556, 31850, 31230, 115069) are excluded from healing.
- **Pets** roll into their owner's row; `pet dmg` below is the portion of the owner's
  total contributed by the pet (shown for cross-checking, not a separate meter row).
- **DPS** = damage ÷ segment duration. For encounters, duration is
  `ENCOUNTER_END ts − ENCOUNTER_START ts` exactly (R4, no DoT-tail grace window):
  **60.000 s** for encounter 1, **45.000 s** for encounter 2.
- **pct** is of that segment's total damage across all meter rows.

Players: `P1` = `Player-1168-0A1B2C01` "Thraxx-Nebula-US" (warrior),
`P2` = `Player-1168-0A1B2C02` "Mírelle-Nebula-US" (priest),
`P3` = `Player-1168-0A1B2C03` "Kael'thar-Nebula-US" (hunter),
pet `Pet-…-0201A1B2C3` "Sharptooth" → owned by **P3**.

**Expected segment count: 4**, in order: Trash, Encounter (kill), Trash, Encounter (wipe).

---

## Segment 1 — Trash

Duration 18.000 s (first→last combat event, 20:04:02 → 20:04:20). Trash duration is
**advisory, not gated** — the contract does not pin a trash segment's start instant.
Damage totals for trash **are** gated.

| player | damage | pet dmg | DPS | pct |
|---|---:|---:|---:|---:|
| P1 Thraxx | **36 300** | 0 | 2016.67 | 51.86 |
| P3 Kael'thar | **26 300** | 4 300 | 1461.11 | 37.57 |
| P2 Mírelle | **7 400** | 0 | 411.11 | 10.57 |

Segment total damage **70 000**.
P1 = 9 800 swing + 26 500 Mortal Strike. P3 = 22 000 Aimed Shot + 4 300 pet swing.

> The pet swing at 20:04:10 lands **before** its `SPELL_SUMMON` at 20:04:12. It is
> still attributed to P3, via the `owner_guid` in the swing's advanced block (the block
> describes the *source* on `SWING_DAMAGE`). An implementation that only reads
> `SPELL_SUMMON` must buffer and retroactively reassign to get 26 300 here.

## Segment 2 — Encounter "The Ashen Warden" — **KILL**, 60.000 s

| player | damage | overkill | pet dmg | DPS | pct |
|---|---:|---:|---:|---:|---:|
| P1 Thraxx | **185 370** | 5 200 | 0 | 3089.50 | 50.83 |
| P3 Kael'thar | **167 200** | 0 | 30 450 | 2786.67 | 45.85 |
| P2 Mírelle | **12 100** | 0 | 0 | 201.67 | 3.32 |

Segment total damage **364 670**.

Derivations:
- **P1 185 370** = 12 500 + 11 800 + 13 020 + 6 600 (swings; the 6 600 is the off-hand
  swing carrying the optional 39th field) + 34 200 Mortal Strike + 51 000 Execute
  + 47 800 Execute (killing blow, overkill 5 200) + 4 100 + 4 350 Deep Wounds.
- **P3 167 200** = own 136 750 (28 700 + 30 100 + 29 400 Aimed Shot, 41 200
  `"Kill Shot, Empowered"`, 3 600 + 3 750 Serpent Sting) + pet 30 450 (5 200 + 5 450 +
  5 600 swings, 7 300 Bite, 6 900 Claw).
- **P2 12 100** = 8 900 Smite + 3 200 Shadow Word: Pain.

Healing, interrupts, CC, dispels, deaths:

| player | healing | overheal | of which absorb | interrupts | CC | dispels | deaths |
|---|---:|---:|---:|---:|---:|---:|---:|
| P1 Thraxx | 0 | 0 | 0 | **1** | **1** | 0 | 0 |
| P3 Kael'thar | 0 | 0 | 0 | **1** | **1** | 0 | 0 |
| P2 Mírelle | **149 800** | **27 300** | 22 800 | 0 | 0 | **1** | **1** |

- **P2 healing 149 800** = effective 127 000 + absorb 22 800.
  Effective = raw 154 300 (42 000 + 38 000 Flash Heal, 9 500 + 9 800 Renew,
  55 000 Radiance) − overheal 27 300 (0 + 12 000 + 9 500 + 1 300 + 4 500). The
  9 500 Renew tick is **100 % overheal → contributes 0** effective healing.
- **Absorb 22 800** = 15 600 (22-field `SPELL_ABSORBED`, shield on P1) + 7 200
  (19-field self-shield on P2). The third `SPELL_ABSORBED` at 20:05:53 is **Stagger
  (115069) for 5 000 and is excluded by R2** — if you see 27 800 here, the exclusion
  list is not being applied.
- **CC = 1 each for P1 and P3** (Intimidating Shout 5246, Binding Shot 117526). The
  `Shadow Word: Pain` DEBUFF at 20:05:32.5 and the `Power Word: Shield` BUFF at
  20:05:16.5 must **not** count.
- **Deaths = 1** (P2 at 20:05:55). The boss and the add dying are not player deaths.

## Segment 3 — Trash

Duration 11.000 s (20:07:05 → 20:07:16), advisory.

| player | damage | pet dmg | DPS | pct |
|---|---:|---:|---:|---:|
| P3 Kael'thar | **27 700** | 6 200 | 2518.18 | 73.28 |
| P1 Thraxx | **10 100** | 0 | 918.18 | 26.72 |

Segment total damage **37 800**. This segment exists because the gap from
`ENCOUNTER_END` (20:06:00) to the next combat event (20:07:05) is 65 s > 60 s.

## Segment 4 — Encounter "Verkath the Hollow" — **WIPE**, 45.000 s

| player | damage | pet dmg | DPS | pct |
|---|---:|---:|---:|---:|
| P3 Kael'thar | **73 300** | 12 000 | 1628.89 | 52.51 |
| P1 Thraxx | **58 200** | 0 | 1293.33 | 41.69 |
| P2 Mírelle | **8 100** | 0 | 180.00 | 5.80 |

Segment total damage **139 600**.

- **P3 73 300** = own 61 300 + pet 12 000 (4 900 swing + 7 100 Bite).
  Own = 26 900 Aimed Shot + 3 400 Serpent Sting + **31 000** Aimed Shot, where that
  31 000 is `base_amount 27 600 + absorbed 3 400` per **R1**. If you see 69 900 here,
  the `absorbed` field is not being added.
- **P1 58 200** = 11 200 + 12 100 swings + 31 000 Mortal Strike + 3 900 Deep Wounds.

| player | healing | overheal | of which absorb | interrupts | CC | dispels | deaths |
|---|---:|---:|---:|---:|---:|---:|---:|
| P3 Kael'thar | 0 | 0 | 0 | **1** | 0 | 0 | **1** |
| P1 Thraxx | 0 | 0 | 0 | 0 | **1** | 0 | **1** |
| P2 Mírelle | **56 400** | **38 200** | 9 400 | 0 | 0 | **1** | **1** |

- **P2 healing 56 400** = effective 47 000 (raw 85 200 − overheal 38 200) + absorb 9 400.
  The 36 000 Flash Heal at 20:08:26 is **100 % overheal**.
- **Deaths = 3** (P1, P3, P2). **Sharptooth the pet dies at 20:08:33 and must NOT be
  counted** — a pet death is not a player death. If you see 4, the flag check is wrong.
- The 22 000 `SPELL_PERIODIC_DAMAGE` at 20:08:28 has a **nil source**
  (`0000000000000000,nil,0x80000000`). It gets no meter row and must not crash the
  parser.
- The 6 000 `DAMAGE_SPLIT` at 20:08:21 is excluded from offensive totals (R1).

---

## Edge shapes deliberately present

Every one of these is in `sample.txt`; the totals above already account for them.

| shape | line (ts) | expected behaviour |
|---|---|---|
| `SWING_DAMAGE_LANDED` twin of every swing | throughout | counted **once** (R1) |
| `RANGE_DAMAGE_SUPPORT` duplicate | 20:05:47 | ignored → `Other` (R1) |
| `DAMAGE_SPLIT` | 20:08:21 | excluded from damage |
| damage with non-zero `absorbed` | 20:08:20.5 | `+3 400` added to damage (R1) |
| Stagger self-absorb (115069) | 20:05:53 | excluded from healing (R2) |
| 100 % overheal | 20:05:22, 20:08:26 | 0 effective healing |
| overkill `-1` | most damage lines | clamped to 0 |
| killing blow overkill 5 200 | 20:05:58 | `extra` = 5 200 |
| quoted comma in spell name | 20:05:09.5 `"Kill Shot, Empowered"` | 41 200 credited to P3 |
| apostrophe + non-ASCII names | throughout | `Kael'thar`, `Mírelle` parse intact |
| off-hand swing (39th field) | 20:05:45.5 | 6 600 counted |
| nil source unit | 20:08:28 | no row, no error |
| pet acts before `SPELL_SUMMON` | 20:04:10 vs :12 | attributed to P3 |
| pet death | 20:08:33 | **not** a player death |
| non-CC DEBUFF | 20:05:32.5 | not counted as CC |
| BUFF with optional amount field | 20:05:16.5 | not counted as CC |
| unknown event type | 20:05:43 `WOWDPS_SYNTHETIC_EVENT` | `Other`, never an error |
| truncated line (6 fields) | 20:05:46 | `parse_line` → `None` |
| blank line | after 20:05:47 | `parse_line` → `None` |
| unmodelled real events | `SPELL_CAST_SUCCESS`, `SPELL_ENERGIZE`, `SWING_MISSED`, `SPELL_MISSED`, `SPELL_AURA_REMOVED`, `ZONE_CHANGE`, `MAP_CHANGE` | `Other` |

## Known coverage gaps (stated, not hidden)

These are **not** exercised by this fixture. They are real rules or shapes I could not
validate here, and I am flagging them rather than implying coverage:

1. **R6 mid-log `COMBAT_LOG_VERSION`** (hard boundary, reset pet-owner map) — not in
   this fixture. Needs a separate small fixture; the segment maths here would become
   ambiguous if bolted on.
2. **`SPELL_DISPEL` 16-field layout is spec-only.** Zero dispels occurred in the real
   log I verified against, so its offsets are unconfirmed by ground truth.
3. **`*_SUPPORT` layout is spec-only** — no Augmentation Evoker in the real log.
4. **Off-hand swing (39-field) is spec-only** — all 269 real swings were 38-field.
5. **`COMBATANT_INFO` is structurally short here** (41–52 fields vs 461–508 in the real
   log). It carries nested bracket/paren arrays with embedded commas, so the CSV stress
   is present in kind, but not at real scale. Only offset 1 (`player_guid`) is
   contracted, so this is low risk.
6. Trash segment **duration/DPS** is advisory — the contract does not define the start
   instant of a trash segment. Trash **damage totals** are gated.
