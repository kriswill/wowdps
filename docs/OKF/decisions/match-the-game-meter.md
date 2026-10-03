---
type: Decision
title: Count And Clock The Way The Game's Own Meter Does
description: 'v40 — overkill leaves every amount, friendly fire leaves the Damage row, and every rate divides by an engagement-based combat clock, because a reconciliation against the game''s built-in meter on a real +15 traced each of our differences to one of those three.'
tags: [meter, ruling, wire]
status: stable
generated: { by: claude-code/opus-5.5, at: 2026-10-02T21:30:00-07:00 }
sources:
  - id: r1
    resource: ../rulings/r1.md
    title: R1 Damage — the amount, overkill out
  - id: r7
    resource: ../rulings/r7.md
    title: R7 Duration — the combat clock amendment
  - id: r22
    resource: ../rulings/r22.md
    title: R22 Self-harm — friendly fire beside it
  - id: blizzard-meter
    resource: https://warcraft.wiki.gg/wiki/Damage_Meter
    title: Warcraft Wiki — the built-in Damage Meter (tracked server-side)
---

**Where:** [`wowdps-core`](../crates/core.md) (`Meter::file_hit`,
`Segment::combat_ms`), [`wowdps-proto`](../crates/proto.md) (v40), every
client. Rulings [R1](../rulings/r1.md), [R7](../rulings/r7.md),
[R10](../rulings/r10.md), [R17](../rulings/r17.md), [R22](../rulings/r22.md).

## Context

On a timed +15 Murder Row the overlay's Σ read Tranqlock 464.5M at 270.6k
while the game's built-in Damage Done meter — computed server-side, not from
the combat log[^blizzard-meter] — read 460M at 288K. The other four players
were off by up to 1.5% in both directions. An independent awk pass over the
key's lines reproduced the overlay to the unit, so this was definition, not
parsing. Each difference came down to one cause, all measured:

- **Rate (−6% for everyone).** The keyed Σ divided by the key timer (28:36,
  R10's "run DPS"); the game divides by combat time (~1600 s).
- **Overkill.** Four of the Warlock's abilities matched the game to the
  thousand as `amount + absorbed + whole absorbs − overkill` (Mind Sear
  28 261 505 against "28261 K" — the game truncates). A Windwalker's Touch of
  Death was 7.85M of overkill on 14.5M.
- **Friendly fire.** A Restoration Shaman's Damage row carried 2.12M of Spirit
  Link Totem's redistribution onto her teammates; [R22](../rulings/r22.md)
  only held damage to oneself off the row.
- **Uncontrolled demons.** The game drops hits from a demon whose flags say
  it is no longer player-controlled (`0xa28`, 237 lines, 2.29M); we credit
  them through the summon map. Left as is — those demons did the damage.
- **A blind spot in the log.** After the logger released and came back ~190
  yards away, no damage on the pack the group was fighting was written for
  ~12 s (about 20M of HP loss, found by walking the advanced block's
  `current_hp`). The game credited it to the other four. Unrecoverable from
  the log.

## Decision

Three rulings move, one wire version:

- **R1:** a hit's amount is `amount + absorbed − overkill` on every ledger it
  reaches — the attacker's row, R17's Taken, R22's tallies, R24's enemy row,
  R21's cells, the series. The Damage row's `extra` still carries the
  overkill, so an overkill report has it; the R9 recap keeps the raw hit and
  names its overkill apart.[^r1]
- **R7:** every rate divides by `Segment::combat_ms`. An encounter's is its
  START..END. A trash segment's is an ENGAGEMENT clock: the stretches between
  consecutive DAMAGE lines between the group and an enemy (by flags), a quiet
  one over `ENGAGE_QUIET_MS` (30 s) left out. A first cut ran the clock on the
  meter's whole combat-line set and gave 1632 s even at a 5 s cut, because a
  healer's HoT ticks kept it running on the walk between packs. Engagement
  lines alone gave 1610.7 s against the game's 1600.4 s, flat from 30 s to
  45 s. Misses and friendly fire never run the clock, so a trash segment's
  combat time can never exceed its duration. An Overall's clock is Σ its
  members' (R10), a key's included. The key's `duration_ms` stays the timer
  it is judged on, and its RUN rate is a reader's sum.[^r7]
- **R22:** friendly fire — a hit on another `Player-`/`Pet-` from its own side
  by the flags, or from a unit summoned by a player on its side (judged by
  the summoner's own flags — an arena enemy's totem hitting us stays the
  enemy's damage, a gap code review caught; a totem's last lines
  read `0xa28`) — is held off the Damage row as `friendly_fire`. The victim's
  Taken keeps it, and the R17 identity carries it.[^r22]
- **v40:** `SegmentInfo.combat_ms` and `FightCard.combat_ms` (`None` on older
  cards; `FightCard::rate_ms` falls back to `duration_ms`, so an old card
  reads as it did). DuckDB's `effective_dps_sql` divides by
  `coalesce(combat_ms, duration_ms)` once a probe proves the column is typed.

The user chose combat DPS as the primary number on a key's rows, with run
DPS secondary: "Run dps" and "In combat" on the window's stat line, "run …
dps" on the overlay's chip, the TUI header, and `run_per_sec` in the mcp
rows.

## Consequences

After the change the Warlock reads 463.2M at 287.6k against the game's 460M
at 288K. The remaining gap is the uncontrolled demons. The others are left
with the log's blind spot. Every fixture total with a killing blow moved
(`sample.txt` −5 200, `support.txt` −2 500, `taken.txt` −25 000).
[`taken.txt`](../fixtures/taken.md) gained a third segment built for the
clock and friendly fire, and `check.awk` computes both independently
(`combat_ms`, `friendly_fire`). Every stored card was written under the old
rules, so `wowdps history regrade` rewrites them. The clock is calibrated on
one run: retune `ENGAGE_QUIET_MS` when a second run disagrees. Detecting the
blind spot from `current_hp` gaps is a possible follow-up; recovering the
damage is not.

[^r1]: R1 Damage — the amount, overkill out.
[^r7]: R7 Duration — the combat clock amendment.
[^r22]: R22 Self-harm — friendly fire beside it.
[^blizzard-meter]: Warcraft Wiki — the built-in Damage Meter (tracked server-side).
