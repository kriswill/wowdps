---
type: Fixture
title: arena.txt
description: 'Arena matches as named win/loss Encounter segments — arenas zone in at difficulty 0, so R10 never sees them.'
resource: crates/core/fixtures/arena.txt
tags: [fixture]
status: stable
generated: { by: human:kris, at: 2026-09-05T17:38:40-07:00 }
---

The match's segment is titled from the last ZONE_CHANGE at any difficulty, and the verdict is the home side's, read from match-local COMBATANT_INFO factions.

The Ashamane's Fall win also holds the other team's death (Yel, 0:25 in, to a Fireball): the raid timeline keeps it and flags it `enemy` (R25), so no reader counts it among the group's deaths.

## Rulings exercised

- [R13 Arena](../rulings/r13.md).
- [R25 Raid timeline](../rulings/r25.md) — an enemy death.

## Gate

`crates/core/tests/arena.rs`; the enemy death in `crates/core/tests/raid.rs` (`an_arena_enemys_death_is_flagged_enemy`) and, stored, `crates/daemon/tests/history.rs`.

## Source

- Log: [`crates/core/fixtures/arena.txt`](../../../crates/core/fixtures/arena.txt)
- Format: [`FORMAT-NOTES.md`](../../../crates/core/fixtures/FORMAT-NOTES.md); parser-independent check: [`check.awk`](../../../crates/core/fixtures/check.awk) via [`verify.sh`](../../../crates/core/fixtures/verify.sh).
