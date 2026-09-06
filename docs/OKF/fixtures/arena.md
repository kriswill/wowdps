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

## Rulings exercised

- [R13 Arena](../rulings/r13.md).

## Gate

`crates/core/tests/arena.rs`.

## Source

- Log: [`crates/core/fixtures/arena.txt`](../../../crates/core/fixtures/arena.txt)
- Format: [`FORMAT-NOTES.md`](../../../crates/core/fixtures/FORMAT-NOTES.md); parser-independent check: [`check.awk`](../../../crates/core/fixtures/check.awk) via [`verify.sh`](../../../crates/core/fixtures/verify.sh).
