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
then three R26 ones, always emitted (zeros included):

| metric | R26 definition (per player, per segment; pets fold onto owners) |
|---|---|
| `casts` | `SPELL_CAST_SUCCESS` lines by the player or their pets that the passive gate admits: an open segment, not past the trash gap |
| `damage_periodic` | Σ `amount + absorbed` of the player's `SPELL_PERIODIC_DAMAGE` (self-harm excluded, as R22 excludes it from `damage`) |
| `heal_periodic` | Σ effective healing (`amount − overheal`) of the player's `SPELL_PERIODIC_HEAL` |

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

### Vexxa — damage 1 088 000, periodic 98 000, casts 6

| row | amount | parts (id, periodic: amount / hits) | casts | group |
|---|---:|---|---:|---|
| Chaos Bolt | 780 000 | one part (116858) | 2 | — |
| Wither | 110 000 | 445468 direct: 50 000 / 1 · 445474 tick: 60 000 / 2 (one crit) | 1 | — |
| Araz's Ritual Forge | 80 000 | 1232797 direct: 60 000 / 1 · 1232802 tick: 20 000 / 1 | 0 | item 242402 "Araz's Ritual Forge" |
| Eradicating Arcanocore | 45 000 | one part | 0 | item 242394 "Eradicating Arcanocore" |
| Lash of Pain (Sayaad) | 36 000 | one part | 2 | summon "Summon Sayaad" |
| Melee (Sayaad) | 12 000 | one part | 0 | summon "Summon Sayaad" |
| Immolation (Infernal) | 18 000 | one part (19483, ticks) | 0 | summon "Summon Infernal" |
| Melee (Infernal) | 7 000 | one part | 0 | summon "Summon Infernal" |

- `petdamage` 73 000 = the Sayaad's 48 000 + the Infernal's 25 000.
- `damage_periodic` 98 000 = Wither's ticks 60 000 + the Forge's tick 20 000
  + Immolation 18 000.
- `casts` 6 = Chaos Bolt × 2 (22:05:02, 22:05:22), Wither, Summon Infernal
  (no damage row of its own: in the total, on no row) and the Sayaad's Lash of
  Pain × 2. NOT counted: the precast Chaos Bolt at 22:04:58 (no segment open)
  and the one at 22:05:31, after the kill.

### Lumen — damage 31 000, periodic 16 000; healing 73 000 (overheal 7 000), periodic 18 000; casts 3

| row | amount | parts | casts |
|---|---:|---|---:|
| Shadow Word: Pain (Damage) | 31 000 | 589 direct: 15 000 / 1 · 589 tick: 16 000 / 2 — ONE id, landing both ways | 1 |
| Renew (Healing) | 38 000 | 139 direct: 20 000 / 1 · 139 tick: 18 000 / 2 (2 000 overheal) | 1 |
| Flash Heal (Healing) | 35 000 | one part (5 000 overheal, a crit) | 1 |

The Grove Cultist's Shadow Bolt cast at 22:05:20 is nobody's: a hostile unit.

## Segments 2 and 3 — Trash (1 500 ms each)

Vexxa's Incinerate opens each (two hits of 30 000). `casts` 1 in each: the
cast at 22:06:59.5 comes before the first trash hit (nothing open), and the
one at 22:08:30 falls 88.5 s after the last hit — past the 60 s gap, the dead
zone — so each trash pull keeps only the cast after its opening hit.

## Σ (the visit's Overall)

Vexxa's casts 8 (6 + 1 + 1), Incinerate's row 2; Wither's parts and every
group as in segment 1.
