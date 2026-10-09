# `tree.txt` — expected values (R26: the ability tree)

Authoritative expected output for the R26 fixture. The per-player totals are
**computed independently of the Rust implementation** by `check.awk` (its
`SPELL_CAST_SUCCESS` arm and the periodic halves of its damage and heal arms);
the machine-readable form is `tree.expected.tsv`. The tree's STRUCTURE — which
rows hang under which group, and each row's parts and casts — is not a
per-player total, so it is laid out by hand below and held by
`crates/core/tests/tree.rs`.

Regenerate / check:

```sh
./verify.sh                                  # every gated fixture: PASS
./verify.sh tree.txt tree.expected.tsv       # PASS
```

Every (segment, player) row of every fixture carries the pre-existing metrics,
then five R26 ones, always emitted (zeros included):

| metric | R26 definition (per player, per segment; pets fold onto owners) |
|---|---|
| `casts` | `SPELL_CAST_SUCCESS` lines by the player or their pets that the passive gate admits: an open segment, not past the trash gap |
| `damage_periodic` | Σ `amount + absorbed` of the player's `SPELL_PERIODIC_DAMAGE` (self-harm excluded, as R22 excludes it from `damage`) |
| `heal_periodic` | Σ effective healing (`amount − overheal`) of the player's `SPELL_PERIODIC_HEAL` |
| `misses_dealt` | `*_MISSED` lines by the player or their pets, not against themselves, through the passive gate |
| `dot_uptime_ms` | Σ over the player's debuffs (by name) on enemies of each one's UNION of time up on any enemy — applied or refreshed opens a target, removed closes it, one still open closes at the segment's close |

## Roster

| who | guid | notes |
|---|---|---|
| Vexxa | `Player-1168-0A1B2C61` | Destruction Warlock |
| Lumen | `Player-1168-0A1B2C62` | Priest |
| Sayaad | `Pet-…-0101A1B261` | summoned by "Summon Sayaad" (366222) at 22:05:01 |
| Infernal | `Creature-…-0000B26101` | a guardian, summoned by "Summon Infernal" (1122) at 22:05:10.1 |
| Tree Test Boss | `Creature-…-0000AC61` | encounter 3160, Mythic |
| Grove Cultist | two hostile units | the trash |

The spell ids are the game's own (SpellName in 12.1.0): both Wither ids are
named "Wither"; 1232797 and 1232802 are both "Araz's Ritual Forge" (item
242402), 1233384 is "Eradicating Arcanocore" (item 242394).

## Segment 1 — Tree Test Boss (kill, 30 000 ms)

### Vexxa — damage 1 108 000, periodic 98 000, casts 6

| row | amount | parts (id, periodic: amount / hits) | casts | group |
|---|---:|---|---:|---|
| Chaos Bolt | 780 000 | one part (116858) | 2 | — |
| Wither | 110 000 | 445468 direct: 50 000 / 1 · 445474 tick: 60 000 / 2 (one crit) | 1 | spell "Wither" |
| Blackened Soul | 20 000 | one part (445736) | 0 | spell "Wither" |
| Araz's Ritual Forge | 80 000 | 1232797 direct: 60 000 / 1 · 1232802 tick: 20 000 / 1 | 0 | item 242402 "Araz's Ritual Forge" |
| Eradicating Arcanocore | 45 000 | one part | 0 | item 242394 "Eradicating Arcanocore" |
| Lash of Pain (Sayaad) | 36 000 | one part | 2 | summon "Summon Sayaad" |
| Melee (Sayaad) | 12 000 | one part | 0 | summon "Summon Sayaad" |
| Immolation (Infernal) | 18 000 | one part (19483, ticks) | 0 | summon "Summon Infernal" |
| Melee (Infernal) | 7 000 | one part | 0 | summon "Summon Infernal" |

- Blackened Soul (22:05:17, on the boss while Wither is up) is Wither's
  Hellcaller proc: `proc_spells` names Wither its driver, so the two form one
  group labelled "Wither" (kind `spell`) — R26 step 4.
- `petdamage` 73 000 = the Sayaad's 48 000 + the Infernal's 25 000.
- `damage_periodic` 98 000 = Wither's ticks 60 000 + the Forge's tick 20 000
  + Immolation 18 000.
- `casts` 6 = Chaos Bolt × 2 (22:05:02, 22:05:22), Wither, Summon Infernal
  (no damage row of its own: in the total, on no row) and the Sayaad's Lash of
  Pain × 2. NOT counted: the precast Chaos Bolt at 22:04:58 (no segment open)
  and the one at 22:05:31, after the kill.
- `misses_dealt` 2: the Sayaad's swing DODGEd at 22:05:08.8 (on its Melee
  row) and a Chaos Bolt the boss was IMMUNE to at 22:05:23 (on Chaos Bolt:
  1 miss against 2 hits, Miss 33.3 %).
- `dot_uptime_ms` 24 800: Wither on the boss 22:05:03.2 → 25.0 (refreshed at
  15.0, which changes nothing) and on a Grove Cultist 20.0 → 28.0 — one union,
  3.2 → 28.0. The Wither applied at 22:05:31, after the kill, lands nowhere.

### Lumen — damage 31 000, periodic 16 000; healing 73 000 (overheal 7 000), periodic 18 000; casts 3

| row | amount | parts | casts |
|---|---:|---|---:|
| Shadow Word: Pain (Damage) | 31 000 | 589 direct: 15 000 / 1 · 589 tick: 16 000 / 2 — ONE id, landing both ways | 1 |
| Renew (Healing) | 38 000 | 139 direct: 20 000 / 1 · 139 tick: 18 000 / 2 (2 000 overheal) | 1 |
| Flash Heal (Healing) | 35 000 | one part (5 000 overheal, a crit) | 1 |

The Grove Cultist's Shadow Bolt cast at 22:05:20 is nobody's: a hostile unit.
Lumen's `dot_uptime_ms` is 25 900: Shadow Word: Pain on the boss from 22:05:04.1,
still up at the kill, closed at the segment's end (22:05:30). Renew is a BUFF on
Vexxa — no enemy, no uptime.

## Segments 2 and 3 — Trash (1 500 ms each)

Vexxa's Incinerate opens each (two hits of 30 000). `casts` 1 in each: the
cast at 22:06:59.5 comes before the first trash hit (nothing open), and the
one at 22:08:30 falls 88.5 s after the last hit — past the 60 s gap, the dead
zone — so each trash pull keeps only the cast after its opening hit.

## Σ (the visit's Overall)

Vexxa's casts 8 (6 + 1 + 1), Incinerate's row 2; Wither's parts and every
group as in segment 1. Wither's uptime 24 800, the pull's own.

## 2026-10-08: casts that began (`cast_starts`)

Nine `SPELL_CAST_START` lines join the log. `check.awk` counts one by one of
ours exactly as it counts a cast — passive, so it never opens, extends or
splits a segment — and emits `cast_starts` right after `casts` for every
fixture (0 wherever no start was logged):

| ts | who | spell | lands |
|---|---|---|---|
| 22:04:56.0 | Vexxa | Chaos Bolt | nowhere — before the pull (nothing open) |
| 22:05:01.5 | Vexxa | Chaos Bolt | the pull (its cast at 02.0 went off) |
| 22:05:18.0 | Lumen | Flash Heal | the pull (cast at 19.0) |
| 22:05:18.5 | Grove Cultist | Shadow Bolt | nobody's — an NPC |
| 22:05:20.5 | Vexxa | Chaos Bolt | the pull (cast at 22.0) |
| 22:05:24.0 | Vexxa | Chaos Bolt | the pull — and no cast follows: it never went off |
| 22:05:30.5 | Vexxa | Chaos Bolt | nowhere — after ENCOUNTER_END |
| 22:07:00.2 | Vexxa | Incinerate | trash 1 (after its opening hit) |
| 22:08:29.0 | Vexxa | Incinerate | nowhere — 87.5 s past the last hit, the dead zone |

So segment 1: **Vexxa 3** (Chaos Bolt's row: 3 starts, 2 casts — one
cancelled), **Lumen 1**; segment 2: **Vexxa 1**; segment 3: 0. No other
number moves and the segment list is the same three.

## 2026-10-08: resources (`energize_gained` / `energize_wasted`, R27)

Eight `SPELL_ENERGIZE` / `SPELL_PERIODIC_ENERGIZE` lines join the log, each
with its advanced block. A line counts on the PLAYER it lands on, passive
like a cast:

| ts | on | power | amount / over | lands |
|---|---|---|---|---|
| 22:04:57.5 | Vexxa | soul shards (7) | 2 / 0 | nowhere — before the pull |
| 22:05:05.3 | Vexxa | soul shards | 1 / 0 | the pull |
| 22:05:06.7 | Sayaad (pet) | energy (3) | 10 / 0 | nobody — a pet's pool is its own |
| 22:05:07.3 | Vexxa | soul shards | 0 / 1 (capped) | the pull |
| 22:05:10.5 | Lumen | mana (0) | 2 500 / 0 | the pull |
| 22:05:12.5 | Vexxa | soul shards | 0.5 / 0 | the pull |
| 22:07:00.6 | Vexxa | soul shards | 1.5 / 0.5 | trash 1 |
| 22:08:29.5 | Vexxa | soul shards | 2 / 0 | nowhere — the dead zone |

So segment 1: **Vexxa 1.5 gained, 1.0 wasted** (40 % of 2.5 generated),
**Lumen 2 500 / 0**; segment 2: **Vexxa 1.5 / 0.5**; segment 3: nothing. The
visit's Σ sums Vexxa to 3.0 / 1.5 over 4 lines.

## 2026-10-09: empowered spells (`empower_stage1`..`4`, `empower_cancelled`, R26)

A Preservation-style Evoker, Ember (`0A1B2C63`), joins the pull: nineteen
lines of Fire Breath (released 357208, its hits 357209) and Dream Breath
(released 355936, its heal 355941). Starts count nothing; a release counts
under the stage it trails, a cancel whatever it trails; passive like a cast:

| ts | line | stage | lands |
|---|---|---|---|
| 22:04:59.0 | Fire Breath END | 2 | nowhere — before the pull |
| 22:05:03.4 | Fire Breath END (+ its cast, its hit, a tick at 05.5) | 3 | the pull |
| 22:05:11.9 | Fire Breath INTERRUPT | (0) | the pull — a cancel |
| 22:05:13.8 | Fire Breath END (+ its cast and a crit) | 1 | the pull |
| 22:05:17.2 | Dream Breath END (+ its cast, a 30 000 heal on Vexxa, 10 000 over) | 4 | the pull |
| 22:05:20.1 | Dream Breath INTERRUPT | (1) | the pull — a cancel |
| 22:05:30.8 | Fire Breath END | 2 | nowhere — after the kill |

So segment 1: **Ember `empower_stage1` 1, `stage3` 1, `stage4` 1,
`empower_cancelled` 2**; her Fire Breath row carries stages `[1, 0, 1, 0]`
and one cancel (mean stage 2.0, two casts), her Dream Breath row `[0, 0, 0,
1]` and one cancel. She deals 112 000 (Fire Breath 60 000 + 12 000 tick +
40 000 crit) and heals 20 000, casts 3; the pull's `pct` column moves with
her damage (Vexxa 88.57, Lumen 2.48, Ember 8.95) and Vexxa's
`healed_received` gains the Dream Breath's 20 000 (93 000). Nothing else
moves and the segment list is the same three.

## 2026-10-09: power (`power_seconds` / `power_sum` / `power_max`, R28)

Every block describing a player writes their second of that type. Vexxa
reports mana (0 of 250 000 max) on her casts and the hits landing on her,
and soul shards (type 7, max 50) on her energize lines — two series, so her
`power_max` is 250 050 and the pull's Σ 1 350 066 over twelve seconds;
Ember reports mana on her three casts (230 000, 210 000, 190 000: Σ
630 000); Lumen's mana on four seconds (Σ 600 000). The trash pulls read
Vexxa alone (2 seconds and 1).
