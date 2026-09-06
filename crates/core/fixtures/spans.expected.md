# `spans.txt` — expected values (R18: aura spans with caster and target)

Authoritative expected output for the R18 fixture. **Computed independently of
the Rust implementation** by `check.awk`'s aura arms (the validator's own
reading of the log grammar under CONTRACT.md R18 as written in
`docs/spec-role-pivots.md` §4.2 and `docs/plan-role-pivots-step4.md` §0). The
machine-readable form is `spans.expected.tsv`; this file is the same numbers with
every derivation shown, line by line.

Regenerate / check:

```sh
./verify.sh                                   # sample, taken, support AND spans: PASS
./verify.sh spans.txt spans.expected.tsv      # PASS
./verify.sh corrupt.txt sample.expected.tsv   # must FAIL (negative control)
```

TSV columns: `segment kind name result dur_ms enc_id difficulty player metric
value`. Every (segment, player) row carries **34 metrics in a fixed order, always
emitted (zeros included)**: the twelve original ones, the seven R17 ones, the
seven R19 ones (see `support.expected.md`), then the eight new ones:

| metric | definition (per player, per segment) |
|---|---|
| `am_uptime_ms` | the per-millisecond **union** of `ActiveMitigation` spans with the player as target — overlapping spans count once, so it can never exceed `dur_ms`; the meter's `am_uptime_pct` = this / `dur_ms` × 100 |
| `externals_given` | count of `External` spans whose **caster** (`src`) is the player |
| `externals_given_ms` | Σ duration of those spans |
| `externals_received` | count of `External` spans whose **target** is the player |
| `externals_received_ms` | Σ duration of those spans |
| `support_uptime_ms` | Σ duration of `SupportBuff` spans whose caster is the player, over every (target, spell) — the supporter's total; the per-(target, spell) rollup rows are written out below |
| `spans` | count of role spans (any kind) with the player as **target** |
| `taken10_0` | the `[0, 10 s)` bucket of the player's taken series (R17's `taken` amount on a 10 s grid from the segment's start) — a spot check; every bucket is listed below |

**What a span is.** A `BUFF` `SPELL_AURA_APPLIED` / `SPELL_AURA_REFRESH` whose
destination is a player (unit flag `0x400`) and whose spell id is in the role
table opens a span keyed by the target with the line's source as `src`; a
re-apply or refresh while that span is open is a no-op; `SPELL_AURA_REMOVED`
closes it. **A refresh or removal with no open span opens one at the segment's
start** (the buff predated the segment). A span still open when the segment
ends is closed **at read time** at the segment's end: `min(end, now) − at`.
Aura lines are passive — they never open, extend or split a segment — so one
after `ENCOUNTER_END` (or past the R4 trash gap) lands nowhere.

**The role table this fixture assumes** (`check.awk`'s `ROLE`, a hand-coded
subset of the generated `crates/core/src/role_spells.rs`; every id is the
**aura** the log applies, never a cast id):

| id | name | kind |
|---|---|---|
| 132404 | Shield Block | ActiveMitigation |
| 871 | Shield Wall | Defensive |
| 342246 | Alter Time | Defensive |
| 33206 | Pain Suppression | External |
| 47788 | Guardian Spirit | External |
| 10060 | Power Infusion | External |
| 80353 | Time Warp | External |
| 395152 | Ebon Might | SupportBuff |
| 410089 | Prescience | SupportBuff |
| 190319 | Combustion | Cooldown |

Not in the table, deliberately present: **1459 Arcane Intellect** (a class
spell — the item path vetoes it, R12 — and no role: **no mark, no span**) and
**1258223 "Nalorakk's Rage"** (in `item_spells` as a Trinket, not a class spell,
not a role: an **R12 `TrinketProc` mark**, untouched by R18).

## Roster

- `W` = `Player-1168-0A1B2C31` "Bastión-Nebula-US", flags `0x511` — **Protection
  Warrior** (COMBATANT_INFO spec 73). The tank: Shield Block spans, Shield Wall,
  receives Pain Suppression, Guardian Spirit, Ebon Might, Time Warp; the trinket
  proc.
- `H` = `Player-1168-0A1B2C32` "Lumenia-Nebula-US", `0x514` — **Holy Priest** (257).
  Gives Pain Suppression and Guardian Spirit to W, Power Infusion to M.
- `E` = `Player-1168-0A1B2C33` "Sandwyrm-Nebula-US", `0x514` — **Augmentation
  Evoker** (1473). Gives Ebon Might to W and M, Prescience to M. Receives nothing.
- `M` = `Player-1168-0A1B2C34` "Emberlyn-Nebula-US", `0x514` — **Fire Mage** (63).
  Casts Time Warp on M, W and H; Combustion (Cooldown), Alter Time (Defensive),
  Arcane Intellect (nothing).
- boss `Creature-…-217000-0000AD01` "Spans Test Boss", `0xa48`, raid flag `0x80`,
  max HP 296 000 (health reports in the players' target blocks; hits on the boss
  drive it to exactly 0 at l.77)
- add `Creature-…-217010-0000AD02` "Spans Test Add", `0xa48`, max HP 46 000
  (killed at l.68, `UNIT_DIED` l.69)
- `Creature-0-4232-2552-0-1985-0000CC03` "Wandering Boar", max HP 42 000 — the
  trash tail in Dornogal

No pets, no `_SUPPORT` share lines (R19 is gated by `support.txt`; this fixture
is about span *time*, not shares), no misses, no absorbs — every taken amount
is a plain hit with `absorbed` 0.

**Expected segment count: 2**, in order: Encounter "Spans Test Boss" (id 3147,
difficulty 16, **kill**, 60.000 s = 20:05:00.000 → 20:06:00.000, R4), then Trash
(8.000 s, R7: 20:06:10 → 20:06:18). Nothing fires between the `ZONE_CHANGE` in and
`ENCOUNTER_START`, so no pre-pull Trash exists. Times below are `m:ss` offsets
from `ENCOUNTER_START` (0:00 = 20:05:00.000); line numbers are `spans.txt`'s
(1-based).

---

## Segment 1 — Encounter "Spans Test Boss" — KILL, 60.000 s, enc 3147 / diff 16

### Damage dealt (R1)

| player | lines | hits | total |
|---|---|---|---|
| W | 18, 35, 47, 68, 75 | Shield Slam 20 000 + Shield Slam 22 000 + Revenge 12 000 + Shield Slam (add) 21 000 + swing 6 000 | **81 000** |
| M | 20, 34, 44, 58, 74 | Fireball 30 000 + Pyroblast 60 000 (crit) + Fireball 32 000 + Fireball (add) 25 000 + Pyroblast 45 000 | **192 000** |
| E | 23, 46, 67 | Eruption 15 000 + 16 000 + 14 000 | **45 000** |
| H | 29, 57, 77 | Holy Fire 9 000 + Smite 7 000 + Smite 8 000 | **24 000** |

Σ = 342 000. dps over 60 s: W 1 350.00, M 3 200.00, E 750.00, H 400.00; pct: W
23.68, M 56.14, E 13.16, H 7.02. Overkill 0 everywhere (the killing blows at l.68
and l.77 log overkill 0 — the target reaches exactly 0). Row order: M, W, E, H.

### Healing (R2) — H only

l.24 Flash Heal on W 40 000 / overheal 18 000 → 22 000; l.54 Heal on W 30 000 /
overheal 5 000 → 25 000. **H heal 47 000, overheal 23 000**; **W healed_received
47 000**, self_healed 0.

### Damage taken (R17) and the taken series

Boss swings on W (`SWING_DAMAGE` + its `_LANDED` twin, counted once):

| line | at | amount | 10 s bucket |
|---|---|---|---|
| 16 | 0:03 | 10 000 | 0 |
| 21 | 0:07 | 12 000 | 0 |
| 32 | 0:15 | 8 000 | 1 |
| 39 | 0:23 | 9 000 | 2 |
| 52 | 0:33 | 11 000 | 3 |
| 63 | 0:44 | 7 000 | 4 |
| 72 | 0:52 | 13 000 | 5 |

**W taken 70 000**; **W taken10 = [22 000, 8 000, 9 000, 11 000, 7 000, 13 000]**
(`taken10_0` = 22 000). l.56 Cinder Lash on M at 0:36, 5 000: **M taken 5 000**,
taken10 = [0, 0, 0, 5 000, 0, 0] (`taken10_0` = 0). E, H take nothing. Identity
(R17): Σ boss by_target over friendlies 75 000 = Σ Taken 75 000.

### Spans (R18) — every aura line, in order

The four role-less / non-span lines first: l.9 Arcane Intellect (M→M, 1459) —
not in the table, a class spell: **nothing**; l.50 / l.66 Nalorakk's Rage (W→W,
1258223) APPLIED 0:30 / REMOVED 0:45 — an **R12 `TrinketProc` mark on W, dur
15 000**, exactly as before R18 (it is not a span and appears in no R18 metric).

Role spans, `(target, spell, kind, src, start → end, dur)`:

| # | lines | target | spell | kind | src | start → end | dur ms | note |
|---|---|---|---|---|---|---|---|---|
| 1 | 10 / 60 | M | Time Warp | External | M | 0:01 → 0:41 | 40 000 | self-cast external: given by M AND received by M |
| 2 | 11 / 61 | W | Time Warp | External | M | 0:01 → 0:41 | 40 000 | |
| 3 | 12 / 62 | H | Time Warp | External | M | 0:01 → 0:41 | 40 000 | |
| 4 | 13 / 27 | W | Ebon Might | SupportBuff | E | 0:02 → 0:12 | 10 000 | |
| 5 | 14 / 28 | M | Ebon Might | SupportBuff | E | 0:02 → 0:12 | 10 000 | |
| 6 | 15 / 37 | M | Prescience | SupportBuff | E | 0:03 → 0:21 | 18 000 | |
| 7 | 19 / 26 | W | Shield Block | ActiveMitigation | W | **0:00** → 0:11 | 11 000 | **segment-start rule**: a `SPELL_AURA_REFRESH` at 0:05 with no open span opens at the segment's start (0:00), not at 0:05 |
| 8 | 25 / 49 | M | Power Infusion | External | H | 0:10 → 0:30 | 20 000 | external received by a non-tank |
| 9 | 30 / 41 | M | Ebon Might | SupportBuff | E | 0:14 → 0:24 | 10 000 | second EM on M: count 2, 20 000 for (E, M, EM) |
| 10 | 31 / 45 | M | Combustion | Cooldown | M | 0:15 → 0:27 | 12 000 | |
| 11 | 36 / 43 | W | Shield Block | ActiveMitigation | W | 0:20 → 0:26 | 6 000 | |
| 12 | 38 / 48 | W | Shield Wall | Defensive | W | 0:22 → 0:30 | 8 000 | overlaps span 11 — **Defensive, not AM** |
| 13 | 42 / 51 | W | Pain Suppression | External | H | 0:24 → 0:32 | 8 000 | overlaps spans 11 and 12 — **External, not AM** |
| 14 | 55 / 65 | M | Alter Time | Defensive | M | 0:35 → 0:45 | 10 000 | |
| 15 | 59 / 71 | W | Guardian Spirit | External | H | 0:40 → 0:50 | 10 000 | |
| 16 | 70 / — | W | Shield Block | ActiveMitigation | W | 0:50 → **(end 1:00)** | 10 000 | **open at the kill**: closed at read time, `min(end, now) − at` = 60 000 − 50 000 |

`spans` (as target): **W 8** (#2, 4, 7, 11, 12, 13, 15, 16), **M 7** (#1, 5, 6, 8,
9, 10, 14), **H 1** (#3), **E 0**.

#### `am_uptime_ms` — the union, as a per-second bitmap

Only ActiveMitigation spans enter the union — Shield Wall (Defensive) and Pain
Suppression (External) overlap the second Shield Block but are NOT counted, so
the union is the Shield Block spans alone:

- span 7: seconds 0–10 (11 s)
- span 11: seconds 20–25 (6 s)
- span 16: seconds 50–59 (10 s)

No two AM spans overlap here, so union = sum: **W am_uptime_ms 27 000** (→
`am_uptime_pct` 45.00 % of 60 000). Everyone else 0. Had the union been
computed over *all* of W's spans, seconds 22–31 (Shield Wall + PS) would have
added 10 s → 37 000: the wrong number, which the gate now catches.

#### `externals_given` / `externals_received`

| player | given (count, ms) | from spans | received (count, ms) | from spans |
|---|---|---|---|---|
| M | **3, 120 000** | #1, 2, 3 (Time Warp ×3 × 40 000) | **2, 60 000** | #1 (TW 40 000) + #8 (PI 20 000) |
| H | **3, 38 000** | #8 PI 20 000 + #13 PS 8 000 + #15 GS 10 000 | **1, 40 000** | #3 (TW) |
| W | 0, 0 | | **3, 58 000** | #2 TW 40 000 + #13 PS 8 000 + #15 GS 10 000 |
| E | 0, 0 | | 0, 0 | |

**Identity: Σ given ms = 120 000 + 38 000 = 158 000 = 40 000 + 60 000 + 58 000 +
0 = Σ received ms** (counts: 6 = 6). Holds by construction — every External span
has exactly one caster and one target — and is what the meter must reproduce.

#### `support_uptime` — per (supporter, target, spell), from the rollup

| supporter | target | spell | count | total ms | spans |
|---|---|---|---|---|---|
| E | W | Ebon Might | 1 | 10 000 | #4 |
| E | M | Ebon Might | 2 | 20 000 | #5, #9 |
| E | M | Prescience | 1 | 18 000 | #6 |

**E support_uptime_ms = 10 000 + 20 000 + 18 000 = 48 000** (Σ over targets =
the supporter's total — the second identity). Per target: W 10 000, M 38 000.
Nobody else has SupportBuff spans as caster.

### The identities

1. Σ `externals_given_ms` over players = Σ `externals_received_ms` = **158 000**
   (and Σ counts 6 = 6).
2. Σ `support_uptime` over (target, spell) for E = **48 000** = E's
   `support_uptime_ms`.
3. `am_uptime_ms` ≤ `dur_ms` for everyone (27 000 ≤ 60 000); the union never
   double-counts.
4. R17 unchanged: Σ dealt on friendlies 75 000 = Σ taken 75 000; Σ taken10
   buckets = taken (W: 22 + 8 + 9 + 11 + 7 + 13 = 70 000; M: 5 000).
5. R12 unchanged: W's marks are exactly one `TrinketProc` (Nalorakk's Rage,
   at 30 000, dur 15 000); M's Arcane Intellect leaves no mark.

---

## Segment 2 — Trash, 8.000 s (20:06:10 → 20:06:18)

The dead zone first: **l.81 Shield Block APPLIED at 1:05** (20:06:05) comes after
`ENCOUNTER_END` (l.79, 1:00) and before any combat line. No segment is open
(the encounter closed exactly at `ENCOUNTER_END`, R4) and an aura is passive —
it never opens one — so this line **lands nowhere**: not in the encounter (which
would read a span starting past its end), not in the trash that has not begun.
The meter's every mark call site goes through `open_segment_for_passive`; a
lazy load of the encounter's byte range could never see this line, and full
replay must agree.

Then: l.82 the boar swings on W at 1:10 for 1 500 (opens the Trash segment; **W
taken 1 500, taken10_0 1 500**); l.84 W swings the boar for 6 000 at 1:13; **l.86
Shield Block REMOVED at 1:15** — there is no open Shield Block span *in this
segment* (the 1:05 apply landed nowhere; the encounter's open span #16 was that
segment's, closed at ITS end), so the **segment-start rule** opens one at the
trash segment's start, 1:10, and the removal closes it: span `(W, Shield Block,
ActiveMitigation, W, 1:10 → 1:15, 5 000)`. l.87 M's Fireball kills the boar at
1:18 (36 000, overkill 0), `UNIT_DIED` l.88.

| player | damage | dps | pct | taken | am_uptime_ms | spans | taken10_0 |
|---|---|---|---|---|---|---|---|
| M | 36 000 | 4 500.00 | 85.71 | 0 | 0 | 0 | 0 |
| W | 6 000 | 750.00 | 14.29 | 1 500 | **5 000** | **1** | 1 500 |

Externals and support are 0 for both. The trash's clock is R7's (first → last
combat line, 1:10 → 1:18 = 8 000 ms), so the 5 s span is 62.5 % of it — under
100 %, as the union must always be (a removal-only span can never start before
the segment).

---

## Edge shapes deliberately present

| shape | line(s) | expected behaviour |
|---|---|---|
| `SPELL_AURA_REFRESH` with no prior APPLIED in the segment | 19 | span from the SEGMENT'S START (0:00), not the refresh time; 11 000 with l.26 |
| a span still open at `ENCOUNTER_END` | 70 | read-time close at the segment end: 10 000; never 0 (an item mark still open reads 0 — that is R12's rule, not R18's) |
| Defensive + External overlapping an AM span | 36–51 | the AM union is Shield Block only: 27 000, not 37 000 |
| a self-cast External (M's own Time Warp) | 10 / 60 | given by M and received by M, both |
| one External cast on three targets | 10–12 / 60–62 | given 3 / 120 000; received 1 / 40 000 each |
| an External received by a non-tank (PI on M) | 25 / 49 | M externals_received includes it |
| two spans of one SupportBuff on one target (EM on M) | 14/28, 30/41 | rollup count 2, 20 000 |
| a SupportBuff on the tank (EM on W) | 13 / 27 | a span on W too — W `spans` 8, not 7 |
| a Cooldown and a Defensive on a DPS | 31/45, 55/65 | spans on M with their kinds; in no rollup metric here (`spans` counts them) |
| a class buff not in the role table (Arcane Intellect) | 9 | nothing — no mark, no span |
| a trinket proc on the tank | 50 / 66 | R12 `TrinketProc` mark dur 15 000; untouched by R18 |
| an aura APPLIED after `ENCOUNTER_END`, before any combat | 81 | lands nowhere (passive gate) |
| a REMOVED inside trash with no open span | 86 | segment-start rule inside trash: [1:10, 1:15] = 5 000 |
| boss swings at known seconds | 16 … 72 | taken10 buckets [22 000, 8 000, 9 000, 11 000, 7 000, 13 000] |

## Ambiguities resolved here (assumptions, stated)

1. **A self-cast External counts on both sides.** The ruling defines
   `externals_given` by `src` and `externals_received` by target with no
   `src ≠ target` clause, so M's own Time Warp (l.10) is one of M's 3 given AND
   M's 2 received. This is what keeps identity 1 exact; a "self excluded" rule
   would need to drop it from both sides.
2. **A SupportBuff on the supporter's own tank is a span on the tank** (EM on W,
   l.13): a span is keyed by whatever player the buff lands on, regardless of
   the target's role — W's `spans` is 8. Nothing in the ruling gates a span by
   the target's role or class.
3. **A REFRESH-opened span takes the refresh line's `src`** as its caster (l.19:
   W). The line that opened the real aura is not in the segment; the refresh is
   the only evidence of who cast it.
4. **The read-time close of a Trash segment's open span uses the trash's last
   combat line** (R7's end), the same clock as its `dur_ms`. No such span
   exists in this fixture (the trash Shield Block is explicitly removed), so the
   goldens do not depend on it; it is stated for the awk's sake.
5. **The taken-series grid starts at the segment's `start_ms`**:
   `ENCOUNTER_START` for an encounter, the first combat line for trash (the
   same origin as R12's `series`). A stagger tick — excluded from `taken` by
   R17 — is excluded from the buckets too (none here).
6. **The union bitmap is per second**, exact because every fixture timestamp
   is a whole second; the meter computes it per millisecond, and the two agree
   on whole-second spans. The awk marks seconds `[floor((at − start)/1000),
   ceil((end − start)/1000))`.
7. **The l.81 Shield Block is not remembered** when trash opens at 1:10: an
   aura that landed nowhere leaves no state. The trash span therefore starts at
   the segment's start (1:10), not at 1:05 — the ruling's "opens one at the
   segment's start" is literal.
8. **`spans` is a plain count**, unaffected by `SPAN_CAP` (16 spans in the
   busiest segment here; the cap is 256). The rollup measures
   (`am_uptime_ms`, externals, support) are what the ruling says is gated
   after a wrap; this fixture never wraps.
