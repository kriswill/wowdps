# `bosses.txt` — expected values (R29: which hostiles the cut calls bosses)

Authoritative expected output for the boss-rule fixture. The machine-readable
form is `bosses.expected.tsv`, computed **independently of the Rust
implementation** by `check.awk` (its R29 block, `r_bosses`: the
`replay_bosses` row of each pull, the units the cut calls `boss`, and its
`replay_boss_npcs` row, their creature ids summed); every other
row is the same metrics every gated fixture carries. The kinds below are the
ruling's, worked by hand.

Regenerate / check:

```sh
./verify.sh bosses.txt bosses.expected.tsv        # PASS
cargo test -p wowdps-core --test replay bosses     # the kinds by name, pull by pull
cargo test -p wowdps-core --test fixture_totals bosses
```

## The log

A Forever client's log (`COMBAT_LOG_VERSION,22,…,BUILD_VERSION,1.60.1,PROJECT_ID,18`),
the advanced block on every line with map 0 (no UiMap inside a dungeon, so
every post stands on the instance's map 48), no `COMBATANT_INFO`. A door into
Blackfathom Deeps (map 48, difficulty 1), then four boss pulls, each a
START..END pair whose END writes a wipe, by two invented players:

| guid | player | flags |
|---|---|---|
| `…5F01` | Orlan Hearthward | `0x511`: the logger (R31) |
| `…5F02` | Sefa Brightmoor | `0x512` |

Every hostile is flagged `0xa48` on the lines that name it and posted with
its most health by the block a hit on it carries. Every name is invented.

## The rule (CONTRACT.md R29)

A hostile is a `boss` when a title its cut opens names it (the whole title
or a piece its commas and lone "and"s divide it into, case aside). When none
is so named, in a cut of one encounter: when exactly one hostile NAME is a
single edit from such a piece (two neighbors swapped, a letter inserted,
deleted or substituted; case and punctuation folded; both of 5 letters or
more) and its strongest unit is the strongest posted hostile, every unit
of that name. Else by health: every hostile with at least 30% of the
strongest posted one's.

## Pull 1 — "Thalgrim Ashvane" (encounter 2950, 20 000 ms): a near miss

The client spells the title and its creature two letters apart:

| unit | most health | share of 5 000 | by health alone | the rule |
|---|---:|---:|---|---|
| Thalgirm Ashvane | 5 000 | 100% | boss | **boss**: one swap from the title |
| Ashvane Acolyte (`…C002`) | 2 000 | 40% | boss | add |
| Ashvane Acolyte (`…C003`) | 2 000 | 40% | boss | add |
| Cinder Imp | 500 | 10% | add | add |

No hostile is named "thalgrim ashvane"; "Thalgirm Ashvane" folds to
`thalgirm ashvane`, one swap (`ri`/`ir`) from the title, and is the only name
within one edit. It is the strongest posted hostile, so it alone is the boss:
`replay_bosses` 1 and `replay_boss_npcs` 990101, where the health rule
would say 3.

Damage: Orlan 950 (200 + 500 on the imp, which dies, + 250), Sefa 850
(300 + 250 on the acolytes + 300); Orlan takes 400 from Thalgirm's Ashen
Cleave. Rates over the pull's 20 s: 47.50 and 42.50.

## Pull 2 — "Kezran the Hollow" (encounter 2951, 15 000 ms): the title exact

"Kezran the Hollow" (4 000) is named by the title itself, so it is the
boss; Hollow Shade (1 600, 40%) is an add though the health rule would
crown it too. `replay_bosses` 1 and `replay_boss_npcs` 990111. Damage: Orlan 200, Sefa 650 (13.33 and
43.33 a second).

## Pull 3 — "Morvessa" (encounter 2952, 15 000 ms): a near miss not the strongest

"Morvesa" is one deletion from the title, but its 600 is under the Gate
Golem's 3 000: the near miss names nobody, and the health rule decides —
the golem is the boss (Morvesa's 20% is under the 30% share), Morvesa an
add. `replay_bosses` 1 and `replay_boss_npcs` 990122, the golem's (the
wrong pick would read 990121). Damage: Orlan 200, Sefa 650.

## Pull 4 — "Évara" (encounter 2953, 15 000 ms): an accented near miss

"Éavra" (4 000) is one swap from the title, both five letters with the É,
and the strongest: the boss, the Pale Wisp (2 000, 50%) an add.
`replay_bosses` 1 and `replay_boss_npcs` 990131. Folded under a locale
that reads bytes, the É would drop, the title fall to four letters and
health crown both: `verify.sh` runs check.awk under `C.UTF-8`. Damage:
Orlan 200, Sefa 650.

## Beside the rule

- R31: Orlan, flagged `0x511`, is each pull's logger (`logger` 1); Sefa 0.
- R30: neither player takes a swing, casts the kit or heals, so both deal
  damage (`role` 3) in every pull.
- R29's other rows: pull 1 numbers five units (Orlan, posted by the hit
  taken, and the four hostiles; Sefa only casts), pulls 2 to 4 two each.
