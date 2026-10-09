# `replay.txt` — expected values (R29: the replay cut)

Authoritative expected output for the R29 fixture. The counts are **computed
independently of the Rust implementation** by `check.awk` (its R29 block:
the floor votes in pass 1, every closed boss pull's posts, event rows,
placed rows, markers and units in pass 2, the placed rows resolved at END);
the machine-readable form is `replay.expected.tsv`, and
`crates/core/tests/replay.rs` holds the cut to it — for this fixture and for
every other gated fixture, whose TSVs carry the same rows. The pull's own
rows are laid out by hand below.

Regenerate / check:

```sh
./verify.sh                                     # every gated fixture: PASS
./verify.sh replay.txt replay.expected.tsv      # PASS
```

Every closed boss pull (an ENCOUNTER_START's segment closed by its END, a
version seam or the next START) of every gated fixture ends the TSV with
twenty-one `*` rows — `replay_units`, `replay_posts`, `replay_floor`, the
thirteen event kinds (`replay_hit` … `replay_rez`), the four placed kinds
(`replay_placed_cast` … `replay_placed_gone`) and `replay_markers` — then a
`replay_posts` row per player posted on the floor, guid order. The placed
spells are the fixtures' own, hard-coded beside check.awk's other tables
(`PLACE`: Demonic Gateway 111771, Wind Rush Totem 192077, Demonic Circle
48018; `TELL`: the gateway's trip 113942, Wind Rush 192082, Anti-Magic Zone
145629, Healing Rain 73921 — a subset of the encounter rubric's generated
`placed` table).

## Roster

| who | guid | notes |
|---|---|---|
| Tank | `Player-1-A` | COMBATANT_INFO 250 (Blood); the owner in `tests/replay.rs` |
| Healer | `Player-1-B` | COMBATANT_INFO 105 (Restoration Druid) |
| Hunt | `Player-1-H` | COMBATANT_INFO 254 (Marksmanship); feigns death |
| Lock | `Player-1-W` | no COMBATANT_INFO: Warlock, Destruction by R8 (Incinerate 29722) |
| Shaman | `Player-1-S` | summons Spirit Link Totem; no other line |
| Ula'tek | `Creature-…-500-AA` | hostile (`0xa48`), the encounter's title: the boss |
| Egg | `Creature-…-501-BB` | hostile, a tenth of the boss's health: an add |
| Coil, the Lesser | `Creature-…-502-CC` | hostile, a comma in its name; goes down unconscious |
| Wind Rush Totem | `Creature-…-97285-WR` | friendly (`0x2111`), summoned, never posted |
| Spirit Link Totem | `Creature-…-53006-SL` | the Shaman's summon; its damage line reads a neutral `0xa28` |
| Demonic Circle | `GameObject-…-191083-DC` | created, never posted |
| Twin Venoms | `Creature-…-600-DD` | the second pull's boss |

## Before the pull: the markers

| t | line | standing on 3004 |
|---|---|---|
| 20:00:02.0 | PLACED 3004 4 (100.50, −20.25) | 4 |
| 20:00:02.5 | PLACED 3004 1 (90.00, −10.00) | 1, 4 |
| 20:00:03.0 | PLACED 9999 2 | 1, 4 (another map) |
| 20:00:03.5 | REMOVED 1 | 4 |
| 20:00:03.6 | PLACED 3004 1 (95.00, −12.00) | 1, 4 |

Every WORLD_MARKER line is an index seed (v45), so a lazily loaded pull
starts with these two standing. A `SPELL_CAST_FAILED` at 20:00:04 is before
the pull: no row.

## Segment 1 — Ula'tek (encounter 3000, Mythic, kill, 30 s)

**The floor.** Player blocks: Tank 20:01:00.5 and 01.0 (2434), Hunt 01.5
(2434), Healer 08.5 and 09.5 (2434 — the second a `DAMAGE_SPLIT`, its block at
off12 behind its spell prefix), Hunt 09.8 (**2435**, a heal finding him on
another floor), Lock 12.0 and 14.5 (2434), Tank 14.2 and Healer 14.25 (2434,
Spirit Link's two halves): 2434 nine votes, 2435 one — **floor 2434**.

**Posts (13).** Tank 3, Hunt 1 (his 2435 post is not kept), Healer 3, Lock
2; Ula'tek 2 (its Spit landing at 02.5, the Incinerate finding it at 16.0),
Egg 1, Coil 1 (a swing — its block at off9 describes the SOURCE — whose
amount, 2434, is the floor's id: no line is read twice for it).

**Events.**

| t | line | row |
|---|---|---|
| 00.5 | Tank's Death Strike at Ula'tek | `pcast_success`, target Ula'tek |
| 01.0 | Ula'tek's Venom on Tank, his block on 2434 | `hit` (base 120) |
| 01.5 | Ula'tek's `RANGE_DAMAGE` Shoot on Hunt | `hit` (any spell-borne damage) |
| 02.0 | Ula'tek begins Spit | `cast_start` |
| 02.5 | Ula'tek lands Spit on Tank | `cast_success` |
| 03.0 | Egg lands Hatch | `cast_success` |
| 03.2 | Coil's swing on Tank | — (melee is no hit row) |
| 03.5 | Tank's Death Strike fails, "Not enough runic power" | `pcast_failed` |
| 03.6 | Hunt begins Aimed Shot | `pcast_start` |
| 04.0 | Ula'tek's Rot on Tank | `debuff_applied` |
| 04.5 | Rot to 2 stacks | `debuff_dose` |
| 05.0 | Helical Toxins on Healer from no one | `debuff_applied` (no source) |
| 05.5 | Healer's Fortitude on Tank | — (a player's aura) |
| 07.0 | Rot off Tank | `debuff_removed` |
| 08.0 | Venom absorbed whole | `hit` (a miss: no position) |
| 08.5 | Falling Rocks on Healer from the nil unit | `hit` — its source names no unit |
| 09.0 | Tank's Mind Freeze on Spit | `interrupt` |
| 09.5 | `DAMAGE_SPLIT` on Healer | — (a post, no row) |
| 10.0 | Egg dies | `npc_died` |
| 10.5 | Hunt's UNIT_DIED with `unconsciousOnDeath` 1 | — (Feign Death is no death) |
| 10.8 | Coil's UNIT_DIED with `unconsciousOnDeath` 1 | `npc_died` (a creature down) |
| 11.0 | Tank dies | `death` |
| 11.5 | Healer's Rebirth on Tank | `rez`, src Healer |
| 12.0 | Lock casts Demonic Gateway | `pcast_success` |
| 14.1 | Shaman summons Spirit Link Totem | — (no placing spell) |
| 14.2 | Spirit Link on Tank from the totem, `0xa28` | — (one of ours by its summon: no hit) |
| 14.25 | Spirit Link's heal on Healer | — (a post, no row) |

`hit` 4, `cast_start` 1, `cast_success` 2, `pcast_start` 1, `pcast_success`
2, `pcast_failed` 1, `interrupt` 1, `debuff_applied` 2, `debuff_removed` 1,
`debuff_dose` 1, `death` 1, `npc_died` 2, `rez` 1.

**Placed (7 rows).** 12.0 the gateway cast (Lock, where he stood); 12.5 the
trip aura on Hunt (a unit numbered: a `touch`); 13.0 Healer's Wind Rush Totem
(a `summon`, the totem numbered here); 13.5 Wind Rush on Tank from the totem
(a `touch`); 14.0 Anti-Magic Zone on a pet nothing numbered — no row; 14.5
Healing Rain on Lock, his block on 2434 (a `touch` with its place); 15.0
Lock's Demonic Circle (`SPELL_CREATE`: a `summon`, the circle numbered here);
15.5 the totem destroyed (`gone`). `placed_cast` 1, `placed_summon` 2,
`placed_touch` 3, `placed_gone` 1.

**Markers (4).** The two standing (1 at 95.00, −12.00; 4) at t 0, 4 removed at
6 000 ms, 7 placed on 3004 at 6 250 ms; the placement on 9999 at 6 300 ms is
another map's.

**Units (9).** Tank, Ula'tek, Hunt, Egg, Coil, Healer, Lock — then the two
only the placed rows name, the totem and the circle. The nil unit is no
unit; the pet is never numbered; Spirit Link Totem, named by no row, is no
unit.

## Segment 2 — Twin Venoms (encounter 3001, Mythic, wipe, 20 s)

Floor 2434 (Tank's one block). Posts 2 (Twin Venoms, Tank). `cast_success`
2 (Twin Venoms' Bite; Egg's Hatch — the Egg's own flags mark it hostile on
the line, a cut's hostility being its own), `hit` 1. Markers 2: 1 and 7
standing (4 came off in the first pull). Units 3.
