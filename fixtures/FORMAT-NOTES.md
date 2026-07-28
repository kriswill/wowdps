# Combat log format notes (validator research)

Source of record for `fixtures/sample.txt`. Primary source:
**WoW Combat Log Reference (WowCoach.gg), `spec.json`** — `format_version: 22`,
`verified_against_patch: "12.0+"`, `last_updated: 2026-05-08`.
<https://wowcoach.gg/docs/combat-log/spec.json>

Secondary/cross-check: <https://warcraft.wiki.gg/wiki/COMBAT_LOG_EVENT>.

**The two sources disagree.** Where they do, the fixture follows spec.json, because it
claims verification against real retail logs and is dated 2026-05-08 (2.5 months ago),
while the wiki page is undated and self-inconsistent. The spec even carries a
`docs-disagree` gotcha calling out that older references are wrong about offsets.

**This is a documentary judgement, not a measurement.** The real raid log — when it
lands — is the tiebreaker, and I will re-verify every offset below against it and
loudly correct this file if it is wrong.

---

## Line shape

```
M/D/YYYY HH:MM:SS.fff-Z<TWO SPACES>EVENT,field,field,...
```

- Timestamp and event CSV separated by **two spaces** (tab on some clients).
- CSV with `"` quoting; **commas are legal inside quoted strings** (spell names).
- The literal string `nil` means "no value" in many fields.
- `COMBAT_LOG_VERSION` can reappear mid-file (logger restart) — hard state boundary.

## Header

```
COMBAT_LOG_VERSION,22,ADVANCED_LOG_ENABLED,1,BUILD_VERSION,12.0.0,PROJECT_ID,1
```

## Common header (offsets 0-8)

`event, srcGUID, srcName, srcFlags, srcRaidFlags, dstGUID, dstName, dstFlags, dstRaidFlags`

Spell prefix (offsets 9-11, for `SPELL_*` / `RANGE_*`, **not** `SWING_*`):
`spellId, spellName, spellSchool`

## Advanced block — 19 fields, NOT 17

The wiki lists 17. spec.json lists **19**: there are two always-zero `unknown` fields
between `absorb` and `power_type`. Getting this wrong shifts every damage/heal suffix
by two columns.

```
0 info_guid      10 power_type
1 owner_guid     11 current_power
2 current_hp     12 max_power
3 max_hp         13 power_cost
4 attack_power   14 position_x
5 spell_power    15 position_y
6 armor          16 ui_map_id
7 absorb         17 facing
8 unknown_1 (0)  18 item_level
9 unknown_2 (0)
```

Position: `SPELL_*`/`RANGE_*` → offsets 12-30. `SWING_*` → offsets 9-27.

### Which unit the advanced block describes  ← attribution-critical

| event | block describes |
|---|---|
| `SWING_DAMAGE` | **source** |
| `SWING_DAMAGE_LANDED` | target |
| `SPELL_DAMAGE`, `SPELL_PERIODIC_DAMAGE`, `RANGE_DAMAGE` | target |
| `SPELL_HEAL`, `SPELL_PERIODIC_HEAL` | target |

**Consequence for pet attribution:** `owner_guid` is only usable when the block
describes the pet. A pet's `SWING_DAMAGE` carries `owner_guid` = the player. A pet's
`SPELL_DAMAGE` describes the *target*, so `owner_guid` there is `0000000000000000` and
tells you nothing. Pet spell damage must be attributed via `SPELL_SUMMON` (or a
remembered owner from an earlier swing / the `0x1000` Pet unit flag).

## Suffix layouts

### SPELL_DAMAGE / SPELL_PERIODIC_DAMAGE / RANGE_DAMAGE — 42 fields

| off | field | note |
|---|---|---|
| 31 | `base_amount` | **effective damage post-mitigation — the canonical number** |
| 32 | `raw_amount` | pre-mitigation, diagnostics only |
| 33 | `overkill` | **`-1` when not a killing blow** — clamp to 0 |
| 34 | `school` | |
| 35 | `resisted` | |
| 36 | `blocked` | |
| 37 | `absorbed` | informational — do NOT add to totals |
| 38 | `critical` | `1` / `nil` |
| 39 | `glancing` | legacy, always `nil` |
| 40 | `crushing` | legacy, always `nil` |
| 41 | `ability_hint` | `ST` / `AOE` — undocumented on the wiki |

Note there are **two** amount fields. The wiki's flat "amount, overkill, school, ..."
list is missing the base/raw split, which is a one-column shift on every damage row.

### SWING_DAMAGE / SWING_DAMAGE_LANDED — 38 fields (39 if off-hand)

Advanced block at 9-27, then:

| off | field |
|---|---|
| 28 | `base_amount` |
| 29 | `raw_amount` |
| 30 | `overkill` |
| 31 | `school` (always `1`) |
| 32-34 | `resisted`, `blocked`, `absorbed` |
| 35-37 | `critical`, `glancing`, `crushing` |
| 38 | `is_off_hand` — **field omitted entirely for main-hand swings** |

### SPELL_HEAL / SPELL_PERIODIC_HEAL — 36 fields

| off | field | note |
|---|---|---|
| 31 | `healed_to_hp` | NOT the heal amount |
| 32 | `amount` | **canonical — INCLUDES overheal** |
| 33 | `overheal` | |
| 34 | `absorbed_to_shield` | already inside `amount`, do not subtract |
| 35 | `critical` | |

Effective healing = `amount - overheal`.

### SPELL_ABSORBED — variable arity, 19 or 22 fields

Count fields before parsing.

- **22** (shield on someone other than the defender): attacker 1-4, defender 5-8,
  damage_spell 9-11, absorber 12-15, shield_spell 16-18, amount 19, total 20, crit 21.
- **19** (self-shield, no damage_spell block): attacker 1-4, defender 5-8,
  absorber 9-12, shield_spell 13-15, amount 16, total 17, crit 18.

### Count/flag events

- `SPELL_INTERRUPT` — 15 fields; 12-14 = interrupted spell id/name/school.
- `SPELL_DISPEL` — 16 fields; 12-14 = dispelled spell, 15 = `BUFF`/`DEBUFF`.
- `SPELL_AURA_APPLIED` — 13 or 14; 12 = `BUFF`/`DEBUFF`, 13 = optional absorb amount
  (**not** a stack count — stacks only appear on `_DOSE` events).
- `SPELL_SUMMON` — 12 fields, spell prefix, no advanced block.
- `UNIT_DIED` — nil source (`0000000000000000,nil,0x80000000,0x00000000`), then the
  dying unit, then trailing `recapID, unconsciousOnDeath`.
- `ENCOUNTER_START` — `id, "name", difficultyID, groupSize, instanceID`
- `ENCOUNTER_END` — `id, "name", difficultyID, groupSize, success(1/0), durationMs`

## Unit flags (`0x...`)

affiliation `0x1` Mine / `0x2` Party / `0x4` Raid / `0x8` Outsider;
reaction `0x10` Friendly / `0x40` Hostile;
control `0x100` PlayerControlled / `0x200` NPCControlled;
type `0x400` Player / `0x800` NPC / `0x1000` Pet / `0x2000` Guardian.

So `0x511` = Mine+Friendly+PlayerControlled+Player, `0x514` = Raid+...+Player,
`0x1114` = Raid+Friendly+PlayerControlled+**Pet**, `0xa48` = Outsider+Hostile+NPC.

---

## Double-counting hazards present in `sample.txt`

The fixture deliberately contains all three. Expected totals count each hit **once**.

1. **`SWING_DAMAGE_LANDED` is the same swing as `SWING_DAMAGE`**, re-reported with the
   target's advanced block. Reading both double-counts every melee hit. The fixture
   pairs every swing with its LANDED twin; expected totals count `SWING_DAMAGE` only.
2. **`_SUPPORT` events are not extra damage.** `SPELL_DAMAGE_SUPPORT` (Augmentation
   Evoker) duplicates an underlying `SPELL_DAMAGE` with an identical `base_amount`.
   The fixture contains one such pair. Treating unknown events as `Event::Other` gets
   this right for free; naive `starts_with("SPELL_DAMAGE")` matching does not.
3. **`absorbed` on a damage event vs. the `SPELL_ABSORBED` event** are different
   things. Counting both double-counts partial absorbs.

## Other traps encoded in the fixture

- `overkill = -1` on every non-killing hit.
- A heal that is 100% overheal (effective 0).
- Main-hand swings that omit the trailing `is_off_hand` field entirely, next to one
  off-hand swing that includes it — so field count alone is not a parse key.
- A spell name containing a quoted comma.
- A `nil` source unit on a damage event.
- A pet swing that occurs *before* its `SPELL_SUMMON`.
- A non-CC `DEBUFF` aura (must not count as crowd control) and a `BUFF` aura carrying
  the optional offset-13 amount.
