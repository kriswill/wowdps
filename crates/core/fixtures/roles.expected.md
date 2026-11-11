# `roles.txt` — expected values (R30: roles inferred per fight)

Authoritative expected output for the role-inference fixture. The totals and
the roles are **computed independently of the Rust implementation** by
`check.awk` (its R30 block, over a copy of the Forever role-signal ids the
fixture casts); the machine-readable form is `roles.expected.tsv`, whose
`role` row codes the role: 0 none, 1 tank, 2 healer, 3 dps. The arithmetic
below is the ruling's, worked by hand.

Regenerate / check:

```sh
./verify.sh roles.txt roles.expected.tsv          # PASS
gawk -v ROLES=1 -f check.awk roles.txt roles.txt  # each player's scores on stderr
cargo test -p wowdps-core --test roles            # lazy parity, other headers, the Overall
cargo test -p wowdps-core --test fixture_totals roles
```

## The log

A Forever client's log (`COMBAT_LOG_VERSION,22,…,BUILD_VERSION,1.60.1,PROJECT_ID,18`),
no `COMBATANT_INFO`, so no player has a spec and every role is inferred. A door into
Blackfathom Deeps (map 48, difficulty 1) opens one visit holding five fights. Every
name and guid is invented.

| guid | player | class (Forever's table) |
|---|---|---|
| `…2E01` | Garrond | Warrior |
| `…2E02` | Mirela | Shaman |
| `…2E03` | Ashvel | Hunter |
| `…2E04` | Corbin | Paladin |
| `…2E05` | Teskar | Shaman |
| `…2E06` | Fennoa | Druid |
| `…2E07` | Ilsabet | Priest |
| `…2E08` | Orrin | Mage |
| `…2E09` | Quillan | Rogue |
| `…2E0A` | Varsk | Rogue |

## The rule (CONTRACT.md R30)

Per player, from the fight's own lines: SWINGS = their enemy melee swings / the top
player's (0 for all when the group took fewer than 5); KIT = min(casts of a taunt, a
mitigation or a shield cast, 3) / 3; FORM = time under a tanking form / the fight's
span; LOW = 1 − damage × group / Σ damage (clamped to [0, 1]); HEAL SHARE = healing on
other players / the top player's; HEAL CASTS = heals / casts, a heal's cast counting
as a heal only at a friendly unit, at nobody or at themselves (every heal cast here is).

TANK = 0.40 SWINGS + 0.30 KIT + 0.20 FORM + 0.10 LOW;
HEALER = 0.40 HEAL SHARE + 0.35 HEAL CASTS + 0.25 LOW.
Tank at TANK ≥ 0.5 (and ≥ HEALER), else healer at HEALER ≥ 0.5, else dps; none under
10 s of combat or for a player who neither dealt damage, healed nor cast. A cast before
a trash fight's first hit lands nowhere (the passive gate).

## Fight 1 — Twilight Lord Kelris (Encounter 2911, 30 000 ms, a kill)

Group 5, Σ damage 5 200; the boss swung 9 times at the group (6 landed and a dodge and
a parry on Garrond, one on Ashvel); the most healing on others is Mirela's 1 200.
Varsk only kicks the boss's Sleep (a cast and an interrupt): he is on the Interrupts
view and on none of the rows the roster reads (Damage, Healing, Deaths, Taken), so he
is named nothing and is no part of the group, whose fair share stays a fifth.

| player | damage | casts (kit / heals) | swings | heal on others | form | TANK | HEALER | role |
|---|---:|---|---:|---:|---|---:|---:|---|
| Garrond | 500 | 5 (4: Taunt, 2 Shield Block, Revenge / 0) | 8 | 0 | Defensive Stance: removed at 20 s with none open, so 20 s from the start, and taken again at 21 s to the end: 29 000 / 30 000 | 0.4 + 0.3 + 0.193 + 0.052 = **0.945** | 0.130 | **tank (1)** |
| Mirela | 100 | 5 (0 / 4) | 0 | 1 200 (her 150 on herself is not) | — | 0.090 | 0.4 + 0.28 + 0.226 = **0.906** | **healer (2)** |
| Ashvel | 1 700 | 1 | 1 | 0 | — | 0.050 | 0 | dps (3) |
| Corbin | 1 500 | 2 (0 / 1) | 0 | 100 | — | 0 | 0.033 + 0.175 = 0.208 | dps (3) |
| Teskar | 1 400 | 3 (0 / 1) | 0 | 0 (his heal is on himself) | — | 0 | 0.117 | dps (3) |
| Varsk | 0 | 1 | 0 | 0 | — | — | — | **none (0)**: off the roster |

## Fight 2 — Aku'mai (Encounter 2910, 40 000 ms, a kill)

Group 4 (Quillan, swung at once and dead at 7.5 s, did nothing of his own), Σ damage
3 980; 12 swings at the group; the most healing on others is Ilsabet's 1 300 (500 +
400 + 250 healed and a shield's 150 absorbed).

| player | damage | casts (kit / heals) | swings | heal on others | form | TANK | HEALER | role |
|---|---:|---|---:|---:|---|---:|---:|---|
| Fennoa | 600 | 3 (3: Growl, 2 Maul / 0) | 6 | 0 | Bear Form from 1 s to the end: 39 000 / 40 000 | 0.4 + 0.3 + 0.195 + 0.040 = **0.935** | 0.099 | **tank (1)** |
| Garrond | 700 | 3 (3: Taunt, Shield Block, Revenge / 0) | 5 | 0 | none written (his stance is never removed) | 0.333 + 0.3 + 0.030 = **0.663** | 0.074 | **tank (1)** |
| Ilsabet | 80 | 4 (0 / 3) | 0 | 1 300 | — | 0.092 | 0.4 + 0.2625 + 0.230 = **0.892** | **healer (2)** |
| Orrin | 2 600 | 3 | 0 | 0 | — | 0 | 0 | dps (3) |
| Quillan | 0 | 0 | 1 | 0 | — | 0.167 | 0.25 | **none (0)**: did nothing |

## Fight 3 — trash, 6 000 ms

Orrin's two Fireballs and three swings at Garrond, who taunted once: 6 s of combat, under
the 10 s a fight needs to be judged. Nobody is named (TANK 0.2 for Garrond).

## Fight 4 — trash, 20 000 ms: no tank, no healer

Group 3, Σ damage 6 200; 7 swings (Ashvel 3, Orrin 2, Teskar 2); Teskar healed Orrin
once (240), the only healing on others.

| player | damage | casts (heals) | swings | heal on others | TANK | HEALER | role |
|---|---:|---|---:|---:|---:|---:|---|
| Ashvel | 1 850 | 1 | 3 | 0 | 0.4 + 0.010 = 0.410 | 0.026 | dps (3) |
| Orrin | 1 600 | 0 | 2 | 0 | 0.267 + 0.023 = 0.289 | 0.056 | dps (3) |
| Teskar | 2 750 | 5 (1) | 2 | 240 | 0.267 | 0.4 + 0.07 = 0.470 | dps (3) |

Taking the most swings, or giving the only healing, is not the role on its own.

## Fight 5 — trash, 15 000 ms: a solo shaman

Mirela alone against one enemy: 870 damage, 5 swings at her, three casts inside the
fight (her first Lightning Bolt is cast before the fight's first hit and lands nowhere),
two of them heals, both on herself. TANK = 0.4 (all the swings, no kit) and HEALER =
0.35 × 2/3 = 0.233: a solo player heals nobody else, so is never a healer. **dps (3)**.

## The visit's Overall (`tests/roles.rs`)

The five fights merged, on their summed clocks (combat 111 s; span 111 s): Garrond
16 swings of 36, 8 kit casts, 29 s of stance — a tank; Fennoa a tank; Mirela (1 200 on
others, 6 heals of 8 casts) and Ilsabet (1 300, 3 of 4) healers; the rest dps; Quillan
none.
