# `forever.txt` — expected values (R8 per edition: a Forever log)

Authoritative expected output for the editions fixture. The totals are
**computed independently of the Rust implementation** by `check.awk` (the
validator's own reading of the log grammar); the machine-readable form is
`forever.expected.tsv`. The classes below are read by hand off the generated
class tables, Forever's `crates/core/src/forever/class_spells.rs` and, for the
contrast, Retail's `crates/core/src/class_spells.rs`; `check.awk` names no
class, so `tests/forever.rs` gates them.

Regenerate / check:

```sh
./verify.sh                                     # every gated fixture, forever.txt among them: PASS
./verify.sh forever.txt forever.expected.tsv    # PASS
cargo test -p wowdps-core --test forever        # classes, editions, lazy and resumed parity, the cut
cargo test -p wowdps-core --test fixture_totals forever
```

## The log

A Forever client's log as the client writes it: the header is
`COMBAT_LOG_VERSION,22,ADVANCED_LOG_ENABLED,0,BUILD_VERSION,1.60.1,PROJECT_ID,18`
while every line still carries the 19-field advanced block, whose map field
reads 0 inside the dungeon: the client has no UiMap there (R29), so the replay
cut keeps each post on map 0 over the instance's map 48 (see "The replay cut"
below). There is no `COMBATANT_INFO`: R8's inference is the only class source.
One boss pull,
`ENCOUNTER_START,2914,"Geilhast",1,5,48` to its `ENCOUNTER_END` 21 s later, a
kill, after a door into map 48 at difficulty 1.

| guid | player | R8 evidence, in order | Forever's table | Retail's table (same lines, `PROJECT_ID,1`) |
|---|---|---|---|---|
| `…3C01` | Velkris | aura 8647 Expose Armor (on the boss), damage 1752 Sinister Strike | **Rogue**, no spec (8647 is a Rogue's) | Monk (8647 is a Monk's there, and the first evidence wins) |
| `…3C02` | Brindlewick | damage 133 Fireball (twice) | **Mage**, no spec | Mage, Fire (63) |
| `…3C03` | Tharokk | aura 71 Defensive Stance, aura 871 Shield Wall, damage 78 Heroic Strike | **Warrior**, no spec | Warrior, Protection (73) |
| `…3C04` | Elowyn | aura 17 Power Word: Shield, its absorb, damage 585 Smite, heal 2050 Lesser Heal | **Priest**, no spec | Priest |
| `…3C05` | Hollin | damage 17364 Stormstrike, damage 29470 Flametongue Attack | **none**: neither id is in Forever's table | Shaman (17364 is a Shaman's there) |

Under a header naming an edition with no table (a Classic client's id, an
unlisted one) nobody has a class, and the amounts are the same under every
header.

## Forever's own tables, and the ones it has none of

Forever has its own absorb table (`crates/core/src/forever/absorb_spells.rs`,
R20) and role-span table (`crates/core/src/forever/role_spells.rs`, R18), and
no proc or placed table, so `check.awk` reads its Forever copies (`SHIELD_F`,
`ROLE_F`) for a Forever log and its Retail copies (`ROLE`, `SHIELD`, `PLACE`,
`TELL`) only while the version line names Retail (`edition_tables`):

- Power Word: Shield (17, in Forever's absorb table) opens an R20 shield of
  known size on Elowyn's ledger: applied 250 at 16.0 s, 200 of it absorbed at
  17.0 s (her healing, `absorbheal` 200, R2), and the 50 left at its removal at
  25.0 s wasted: `absorb_applied` 250, `absorb_wasted` 50, `shields_unknown` 0.
- Shield Wall (871, a defensive in Forever's role table) opens an R18 span
  on Tharokk from 14.0 s (4 000 ms into the pull) to its removal at 26.0 s:
  `spans` 1; a defensive is no active mitigation, so `am_uptime_ms` stays 0.
  Its removal is logged a second time at 30.5 s, after the span closed: the
  segment-start rule fires only for a key the segment opened no span for,
  so it opens nothing (no `[0, 20 500]` span; `spans` stays 1).
- The cut keeps Shield Wall's two buff rows, a role buff going on and coming
  off a player (`replay_buff_applied` 1, `replay_buff_removed` 1); Tharokk is
  one of its units by his posts already (`replay_units` 6, "The replay cut"
  below). Power Word: Shield is no role buff, so it keeps none.

## Totals (segment 1, the pull, 21 000 ms of combat)

| player | damage | how |
|---|---|---|
| Brindlewick | 670 | 310 + (410 − 50 overkill) |
| Hollin | 205 | 160 + 45: the id no table knows counted like any other |
| Velkris | 180 | 120 + a 60 swing (its `_LANDED` twin is no second hit) |
| Tharokk | 140 | |
| Elowyn | 95 | |
| **Σ** | **1 290** | the boss's health, exactly |

Casts (`casts`, a `SPELL_CAST_SUCCESS` each; no cast moves an amount): Velkris
1 (Expose Armor), Brindlewick 2 (Fireball at 12.5 s and 29.5 s), Elowyn 1
(Smite at 17.8 s), Hollin 1 (Stormstrike at 19.0 s), Tharokk none. None is a
role signal (R30) and none an R8 evidence.

Power (R28, each second's last reading of the player's own block, after the
cast's cost at block offset 13): Velkris energy 35 at second 1 (60 − 25) and
35 at second 10 (his swing's block): sum 70, max 100; Tharokk rage 120 and
130 (seconds 7 and 12): 250, max 1000; Brindlewick mana 1370 (1500 − 130,
second 2) and 1080 (1210 − 130, second 19): 2450, max 1800; Elowyn mana 1560
(1650 − 90, second 7), max 1900; Hollin mana 820 (880 − 60, second 9), max
1100.

Healing: Elowyn 280 = 80 effective (110 − 30 overheal) + the 200 absorbed.
Taken: Tharokk 290 = 90 + 200 absorbed (`reduced` 10: 300 unmitigated − 290).
Velkris's Expose Armor stands on the boss 19 000 ms (`dot_uptime_ms`).

## Roles (R30)

Forever's log carries no spec, so each player's role is inferred from the
pull's own lines (`roles.expected.md` has the rule). 21 000 ms of combat
judges it; the group is all five (each dealt damage); the pull's five casts
(Expose Armor, two Fireballs, a Smite, a Stormstrike) are none of the kit and
none a heal, so the kit signal is 0 for everyone and a caster's heal casts
are 0 of their casts; the boss's only hit on a player is a spell, so the group
took no melee swing and the swing signal is 0 for everyone.

| player | damage × 5 / 1 290 | low damage | other signals | tank | healer | role |
|---|---:|---:|---|---:|---:|---|
| Brindlewick | 2.597 | 0 | two casts, no signal | 0 | 0 | dps (3) |
| Hollin | 0.795 | 0.205 | one cast, no signal | 0.020 | 0.051 | dps (3) |
| Velkris | 0.698 | 0.302 | one cast, no signal | 0.030 | 0.076 | dps (3) |
| Tharokk | 0.543 | 0.457 | Defensive Stance from 13.5 s to the end: form 17 500 / 21 000 = 0.833 | 0.2 × 0.833 + 0.1 × 0.457 = 0.212 | 0.114 | dps (3) |
| Elowyn | 0.368 | 0.632 | all 280 of the healing on others: heal share 1; one cast, a Smite: heal casts 0 / 1 | 0.063 | 0.4 + 0.25 × 0.632 = 0.558 | **healer (2)** |

The Warrior's stance alone is not a tank's evidence: he took no swing and
cast nothing of the kit, so his role is the one the lines show.

## The replay cut (R29)

The client names no UiMap inside the dungeon and writes 0 in every block's
map field, its x, y and facing still the unit's own; in a Forever log that 0
is a place (`Tables::map_zero_is_a_place`), so every block with health posts
its unit on map 0, NO UiMap: x and y on the instance's map, the head's 48
(ENCOUNTER_START's instance id). The cut's floor is 0, the one floor the
players stood on. The same lines under any other header (Retail's, a log
naming none, a Classic client's, an unlisted id) read 0 as no place and post
nobody (`tests/forever.rs`).

| t (ms from 10.0 s) | line | the block describes | post |
|---:|---|---|---|
| 1 000 | Velkris's Expose Armor cast | Velkris (its caster) | −505.20, −9.80 |
| 2 000 | Velkris's Sinister Strike | Geilhast (its target) | −507.79, −11.67 |
| 2 500 | Brindlewick's Fireball cast | Brindlewick | −512.40, −14.20 |
| 3 000 | Brindlewick's Fireball | Geilhast | |
| 5 000 | Tharokk's Heroic Strike | Geilhast | |
| 7 000 | Geilhast's Arcing Smash | Tharokk (its target) | −506.10, −10.40 |
| 7 800 | Elowyn's Smite cast | Elowyn | −514.60, −6.30 |
| 8 000 | Elowyn's Smite | Geilhast | |
| 9 000 | Hollin's Stormstrike cast | Hollin | −506.90, −12.10 |
| 9 000 | Hollin's Stormstrike, Flametongue Attack | Geilhast, twice | |
| 10 000 | Velkris's swing | Velkris (its source) | −505.20, −9.80 |
| 10 000 | its `_LANDED` twin | Geilhast | |
| 12 000 | Elowyn's Lesser Heal | Tharokk (its target) | −506.10, −10.40 |
| 19 500 | Brindlewick's Fireball cast | Brindlewick | −512.40, −14.20 |
| 20 000 | Brindlewick's killing Fireball | Geilhast, at 0 health | |

That is 16 posts on map 0 (`replay_map_posts` 16, `replay_posts` 16):
Geilhast 8, every one at −507.79, −11.67; Velkris 2, Brindlewick 2, Tharokk 2,
Elowyn 1, Hollin 1. The SPELL_ABSORBED line has no block.

- `replay_units` 6: the five players and the boss — Velkris by his cast's
  post at t 1 000, Geilhast as that cast's target, then Brindlewick, Tharokk,
  Elowyn and Hollin by their first posts.
- `replay_hit` 1: Arcing Smash on Tharokk, kept because his own block places
  him (under another header, with no post, it is no row).
- `replay_pcast_success` 5: the five casts. `replay_debuff_applied` and
  `_removed` 1 each: Velkris's Expose Armor on the boss (a player's debuff on
  an NPC). `replay_npc_died` 1, `replay_boss_engaged` and `_killed` 1 each.
  `replay_buff_applied` and `_removed` 1 each: Tharokk's Shield Wall, a role
  buff in Forever's role table; Power Word: Shield is none, so no row.
- `replay_absorb` 100: Tharokk's block says 50 of shield at each of his two
  posts (the Power Word: Shield); every other block says 0.
- `replay_pools` 8 and `replay_pool_sum` 5 150: one pool on each player post
  (the boss's block names a pool with max 0: none): Velkris's energy 35 and
  35, Tharokk's rage 120 and 130, Brindlewick's mana 1 370 and 1 080, Elowyn's
  1 560, Hollin's 820 — each after its cast's cost (format 5), as R28 reads
  them above.
