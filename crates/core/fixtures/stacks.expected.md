# `stacks.txt` — expected values (R21: stacked-debuff conditioning)

Authoritative expected output for the R21 fixture. **Computed independently of
the Rust implementation** by `check.awk`'s `debuff_aura` / `stack_hit` machine
(the validator's own reading of the log grammar under CONTRACT.md R21). The
machine-readable form is `stacks.expected.tsv`; this file is the same numbers
with the per-cell table the TSV rolls up, cell by cell (the Rust side of that
table is `crates/core/tests/stacks.rs`).

Regenerate / check:

```sh
./verify.sh                                   # every gated fixture: PASS
./verify.sh stacks.txt stacks.expected.tsv    # PASS
```

Every (segment, player) row carries the pre-existing metrics, then five R21
ones, always emitted (zeros included):

| metric | R21 definition (per player, per segment; pets fold onto owners) |
|---|---|
| `stack_hits` | Σ `hits` over the player's cells — a hit under two open debuffs counts in two cells |
| `stack_sum` | Σ `sum` over the cells (`amount + absorbed`, R17's Taken amount) |
| `stack_max` | the largest single hit in any cell |
| `stack_cells` | distinct cells: (damage label, damage id, aura id, level ≥ 1) |
| `stack_auras` | distinct hostile debuffs seen open on the player at any level |

## Roster

- `T` = `Player-1168-0A1B2C51` "Brannoc-Nebula-US", `0x511` — Protection Paladin (spec 66), the tank.
- `H` = `Player-1168-0A1B2C52` "Liora-Nebula-US", `0x514` — Holy Priest (257).
- `M` = `Player-1168-0A1B2C53` "Vexara-Nebula-US", `0x514` — Frost Mage (64), owner of
  `Pet-…-0301A1B2E1` "Water Elemental" (summoned at 20:05:00.5).
- Boss `Creature-…-219000-0000AF01` "Stacks Test Boss", add `…-219010-0000AF02` "Stacks Test Add".

Spells: **Tectonic Strike** 1305225 (the stacking debuff, and a hit of its own),
**Crushing Smash** 1305230 (the hit that scales), **Cold Claws** 1305240 (the add's
stacking debuff), **Slow** 31589 (the mage's debuff on the tank — a controlled
source), **Hellbent Commander** 1281559 (a stacking BUFF on the mage).

## Segment 1 — Encounter "Stacks Test Boss" (kill, 60 000 ms)

The tank's ledger, in log order (level = Tectonic's level when the hit lands;
a hit at the same millisecond as an aura line lands AFTER it):

| time | line | Tectonic level after | cell |
|---|---|---|---|
| :01 | Smash 200 000 | — | none (no debuff) |
| :02 | Smash 210 000 | — | none |
| :03 | APPLIED | 1 | |
| :03 | Tectonic 90 000 | 1 | (Tectonic, Tectonic, 1) |
| :04 | Smash 230 000 | 1 | (Tectonic, Smash, 1) |
| :06 | APPLIED_DOSE 2 | 2 | |
| :06 | Tectonic 110 000 | 2 | (Tectonic, Tectonic, 2) |
| :07 | Smash 360 000 + 20 000 absorbed = 380 000 | 2 | (Tectonic, Smash, 2) |
| :09 | APPLIED_DOSE 3 | 3 | |
| :09 | Tectonic 130 000 | 3 | (Tectonic, Tectonic, 3) |
| :10 | Smash 450 000 | 3 | (Tectonic, Smash, 3) |
| :11 | Smash DODGE | 3 | none — a miss is not a hit (R17 counts it) |
| :12 | Smash 620 000 | 3 | (Tectonic, Smash, 3) — the max |
| :13 | REFRESH | 3 | unchanged |
| :14 | Smash 440 000 | 3 | (Tectonic, Smash, 3) |
| :15 | Cold Claws APPLIED (add), :15.5 APPLIED_DOSE 2 | 3 / Claws 2 | |
| :16 | Smash 500 000 | 3 / Claws 2 | (Tectonic, Smash, 3) AND (Claws, Smash, 2) |
| :17 | Cold Claws REMOVED | 3 | |
| :18 | REMOVED_DOSE 2 | 2 | counts DOWN |
| :19 | Smash 370 000 | 2 | (Tectonic, Smash, 2) |
| :20 | REMOVED | — | |
| :21 | Smash 205 000 | — | none |
| :30–:32 | the mage's Slow APPLIED, DOSE 2, REMOVED on T; Smash 215 000 at :31 | — | none — a controlled source never conditions |

Cells for `T`:

| aura | damage | level | hits | sum | max |
|---|---|---|---|---|---|
| Tectonic | Crushing Smash | 1 | 1 | 230 000 | 230 000 |
| Tectonic | Crushing Smash | 2 | 2 | 750 000 | 380 000 |
| Tectonic | Crushing Smash | 3 | 4 | 2 010 000 | 620 000 |
| Tectonic | Tectonic Strike | 1 | 1 | 90 000 | 90 000 |
| Tectonic | Tectonic Strike | 2 | 1 | 110 000 | 110 000 |
| Tectonic | Tectonic Strike | 3 | 1 | 130 000 | 130 000 |
| Cold Claws | Crushing Smash | 2 | 1 | 500 000 | 500 000 |

`stack_hits` 11, `stack_sum` 3 820 000, `stack_max` 620 000, `stack_cells` 7,
`stack_auras` 2. Debuffs seen: Tectonic (max 3, 10 hits), Cold Claws (max 2, 1).
The unconditioned Crushing Smash row is 11 hits / 3 820 000 (12 events with the
dodge); the DERIVED level-0 row for Tectonic is 3 820 000 − 2 990 000 = 830 000
(= 200 + 210 + 205 + 215 k) over 12 − 7 = 5 events (4 hits + the dodge — the
row's count is R17's events, so the derived level-0 count includes every miss).

`H`: an `APPLIED_DOSE 4` at :25 with no prior APPLIED opens the entry at 4; the
Smash 150 000 at :26 is (Tectonic, Smash, 4). One cell, 1 hit, 150 000; Tectonic
max 4. `stack_auras` 1.

`M`: Hellbent Commander (BUFF, doses 3 → 2 → 1) never enters; the Smash 120 000
on her at :36 is under no debuff — no cell. Her pet takes Tectonic APPLIED at :40
and a Smash 60 000 at :41 — (Tectonic, Smash, 1) folded onto `M`. One cell, 1
hit, 60 000, Tectonic max 1.

## Segment 2 — Trash (20:06:06 – :08)

The Tectonic APPLIED at 20:06:05 falls after ENCOUNTER_END and before any trash
segment opened — it lands nowhere. The Smash 100 000 at :06.5 is under no
debuff. The `APPLIED_DOSE 2` at :07 is an orphan and opens the entry at 2; the
Smash 100 000 at :08 is (Tectonic, Smash, 2). `T`: 1 cell, 1 hit, 100 000, max
100 000, Tectonic max 2.

## Segment 3 — Trash (20:07:31 – :32)

The `APPLIED_DOSE 3` at 20:07:30 is 82 s past segment 2's last combat — past
the trash gap, it lands nowhere. The Smash 100 000 at :31 opens a NEW segment
with an empty ledger: no cell, no debuff seen. Every R21 metric 0.
