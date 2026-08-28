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
| 37 | `absorbed` | **CONTRACT R1 adds this to damage done** (meter convention). spec.json calls it informational; the ruling overrides. Never also count it as healing — that is `SPELL_ABSORBED`'s job (R3). |
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

- **22** — attacker 1-4, defender 5-8, **damage_spell 9-11**, absorber 12-15,
  shield_spell 16-18, amount 19, total 20, crit 21.
- **19** — attacker 1-4, defender 5-8, absorber 9-12, shield_spell 13-15,
  amount 16, total 17, crit 18. **No damage_spell block.**

> **CORRECTION — an earlier version of this file was wrong.** It described the 22-field
> variant as "shield on someone other than the defender" and the 19-field variant as
> "self-shield", repeating spec.json's stated discriminator. **That is false**, caught by
> `core` and confirmed by me against the live log: of 11 586 22-field lines, **9 960 have
> `absorber == defender`** (86 %) and only 1 626 do not; all 19 of the 19-field lines
> also have `absorber == defender`. Absorber identity does **not** discriminate the two
> arities. The real discriminator is the **presence of the damage-spell block** —
> equivalently, the field count. Branch on width (or on the block), never on
> "is the absorber the defender".
>
> `check.awk` was never affected: it branches on `NF == 22` / `NF == 19`, so no expected
> value changes. The error was in this prose only. Reported to spec.json's authors'
> claim, not to the offsets — the offsets above are confirmed correct for both arities.

### Count/flag events

- `SPELL_INTERRUPT` — 15 fields; 12-14 = interrupted spell id/name/school.
- `SPELL_DISPEL` — 16 fields; 12-14 = dispelled spell, 15 = `BUFF`/`DEBUFF`.
- `SPELL_AURA_APPLIED` — 13, 14 **or 15** (see correction 5 below); 12 =
  `BUFF`/`DEBUFF`, 13 = optional absorb amount (**not** a stack count — stacks only
  appear on `_DOSE` events). Read offset 12 and ignore trailing fields.
- `SPELL_SUMMON` — 12 fields, spell prefix, no advanced block.
- `UNIT_DIED` — **10 fields**: nil source (`0000000000000000,nil,0x80000000,0x80000000`),
  then the dying unit, then a single trailing `0`.
- `ENCOUNTER_START` — `id, "name", difficultyID, groupSize, instanceID`
- `ENCOUNTER_END` — `id, "name", difficultyID, groupSize, success(1/0), durationMs`
- `COMBATANT_INFO` — `guid, faction, <22 stat scalars>, currentSpecID(field 25),
  [(traitNodeID,traitNodeEntryID,rank),…], (pvpTalents…), [(itemID,ilvl,
  (enchantIDs),(bonusIDs),(gemIDs)),…], [(auras…)], …`. Field 25 is the LAST
  scalar before the first `[`; a comma split shreds the brackets, so the parser
  scans the raw line bracket-aware (v19). Talent `rank` 0 = a granted/free node
  (matches the import-string codec's "selected but unpurchased"). The gear
  array is positional — the standard 18-slot inventory order (head, neck,
  shoulder, shirt, chest, waist, legs, feet, wrist, hands, finger ×2,
  trinket ×2, back, main hand, off hand, tabard); empty slots log zeroed
  tuples. Real lines run 461–508 fields; ~50–80 talent tuples at max level.
- `ARENA_MATCH_START` — `mapID, unk(0), matchType, teamID` (R13). `matchType` is
  a bare word ("Skirmish", rated brackets, "Rated Solo Shuffle"); **`teamID` is a
  dead constant 0 in real logs** — it is NOT the player's side. Fires at gates,
  ~1 min after the arena's `ZONE_CHANGE` — which carries **difficulty 0**, so
  arenas never open R10 visits. Verified live 2026-08-15.
- `ARENA_MATCH_END` — `winningTeam, matchDurationSecs, newRating1, newRating2`.
  The HOME side comes from the match's own `COMBATANT_INFO` lines: field 2
  ("faction") is the player's arena side (0/1) inside a match, and the game
  re-fires the infos for all six/four/ten players right after the START. Win iff
  `winningTeam` equals the faction of a friendly-flagged (reaction `0x10`)
  player. Verified on two live matches (home 1/winner 0 and home 0/winner 1,
  both losses). Enemy players read `0x548`; a neutral-reaction oddity (`0x528`)
  was observed once — resolution keys on the friendly bit only. The duration
  field tracks START..END timestamps to within a second; the meter uses its own
  clock (R7).

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

---

## Verified against the live raid log (2026-07-27, build 12.0.7)

`WoWCombatLog-072726_205251.txt`, 177 915 lines, read-only. **Ground truth outranks
both spec.json and the wiki.** Result: spec.json confirmed on every offset above;
the wiki's 17-field advanced block and flat damage suffix are wrong.

Field widths observed (zero variance unless noted): `SPELL_DAMAGE` /
`SPELL_PERIODIC_DAMAGE` / `RANGE_DAMAGE` = 42 · `SPELL_HEAL` / `SPELL_PERIODIC_HEAL`
= 36 · `SWING_DAMAGE` / `SWING_DAMAGE_LANDED` = 38 · `SPELL_ABSORBED` = 22 or 19 ·
`SPELL_INTERRUPT` = 15 · `SPELL_SUMMON` = 12 · `UNIT_DIED` = **10** ·
`ENCOUNTER_START` = 6 · `ENCOUNTER_END` = 7 · `COMBAT_LOG_VERSION` = 8 ·
`COMBATANT_INFO` = 461–508.

Parse failures: **4 / 114 275 modeled lines (0.0035 %)**, all one shape (below).

### Corrections the real log forced on this document

1. `UNIT_DIED` is **10** fields — a single trailing `0`, not `recapID` +
   `unconsciousOnDeath`.
2. raidFlags are **`0x80000000`** for "no marker", not `0x0`. That exceeds
   `i32::MAX`: parse flags as `u32`. Unit flags reach 5 hex digits (`0x10a48`).
3. School fields mix formats *on the same line*: `SPELL_INTERRUPT` had
   `spell_school` = `0x1` (hex) and `extraSchool` = `106` (bare decimal).
4. `SPELL_SUMMON` can summon a **`Creature-`** GUID (Efflorescence totem, `0xa28`).
   Detect pets/guardians from flag bits `0x1000`/`0x2000`, never the GUID prefix.
5. `SPELL_AURA_APPLIED` can be **15** fields (`"Second Wind",…,BUFF,0,0`) — two
   trailing optionals. Read `aura_type` at offset 12; never gate on exact width.
   This was the only parse-failure shape in the entire file.
6. A **nil GUID can carry player flags**: 36 `SPELL_DAMAGE` lines had sourceGUID
   `0000000000000000` with sourceFlags `0x514`. Reject the nil GUID *before* testing
   flags, or the meter grows a phantom "unknown player" row.
7. `ENCOUNTER_END`'s `fightTime` field runs 28–56 ms longer than
   (`END ts` − `START ts`) across all five pulls. R4 computes from timestamps; don't
   mix the two sources.

`SPELL_DISPEL`, `*_SUPPORT` and the 39-field off-hand swing did **not** occur in this
log — their layouts remain spec-only and unverified.
