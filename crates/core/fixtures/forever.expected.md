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
reads 0 inside the dungeon (so the replay cut posts nobody). There is no
`COMBATANT_INFO`: R8's inference is the only class source. One boss pull,
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

## The tables Forever has none of

Forever has no role, absorb, proc or placed table, so `check.awk` reads its
Retail copies (`ROLE`, `SHIELD`, `PLACE`, `TELL`) only while the version line
names Retail (`edition_tables`):

- Shield Wall (871, a defensive in Retail's role table) opens no R18 span:
  `spans` and `am_uptime_ms` are 0 for Tharokk.
- Power Word: Shield (17, in Retail's absorb table) opens no R20 shield, so
  its applied 250 and its removal's 50 are never seen; the 200 it absorbed
  still credits Elowyn's healing (`absorbheal` 200, R2) and opens an
  unknown-size shield on her ledger, open at the pull's end:
  `shields_unknown` 1, `absorb_applied` 0, `absorb_wasted` blank.
- The cut keeps no buff row for either (neither is a role buff here, and
  both casters are the group's).

## Totals (segment 1, the pull, 21 000 ms of combat)

| player | damage | how |
|---|---|---|
| Brindlewick | 670 | 310 + (410 − 50 overkill) |
| Hollin | 205 | 160 + 45: the id no table knows counted like any other |
| Velkris | 180 | 120 + a 60 swing (its `_LANDED` twin is no second hit) |
| Tharokk | 140 | |
| Elowyn | 95 | |
| **Σ** | **1 290** | the boss's health, exactly |

Healing: Elowyn 280 = 80 effective (110 − 30 overheal) + the 200 absorbed.
Taken: Tharokk 290 = 90 + 200 absorbed (`reduced` 10: 300 unmitigated − 290).
Velkris's Expose Armor stands on the boss 19 000 ms (`dot_uptime_ms`).
