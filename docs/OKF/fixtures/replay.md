---
type: Fixture
title: replay.txt
description: 'The replay cut''s fixture (R29, v45) — two boss pulls with world markers placed and replaced before the first, posts on two floors, every event kind a replay draws (a ranged hit, a miss, a hit from no source, a friendly totem''s tick that is none, a failed cast, a Feign Death, a creature down unconscious, a rez), a warlock known by R8, and what players place: a gateway, a totem''s summon, touches, a create, a totem destroyed.'
resource: crates/core/fixtures/replay.txt
tags: [fixture]
status: stable
generated: { by: claude-code/opus-5.5, at: 2026-10-09T15:30:00-07:00 }
---

Every count is laid out by hand in `replay.expected.md`; the TSV holds what
`check.awk`'s R29 block computes from the log grammar alone — and that block
writes each closed boss pull's `replay_*` rows into EVERY gated fixture's
TSV, so [sample.txt](sample.md) and the rest gate the cut too. The placed
spells are real ids from the encounter rubric's generated table
([`wowdps-encounter-rubric`](../crates/encounter-rubric.md)), hard-coded in
check.awk as its other tables are.

## Rulings exercised

- [R29 The replay cut](../rulings/r29.md): the floor (seven votes on 2434,
  one on 2435, whose post is dropped), units in order of first sight, the
  thirteen event kinds, the four placed kinds, the markers standing at the
  start (a marker removed and re-placed before the pull) and each change;
  a swing whose amount equals the floor's id posted once, as the parser
  reads it; a name with a comma.
- [R22 Self-harm](../rulings/r22.md): Spirit Link Totem's
  redistribution (its damage line a neutral `0xa28`) is friendly fire under
  the Shaman who summoned it and stays in the Tank's Taken — and, one of ours
  by that summon, it is no replay hit and numbers no unit.
- [R9 Deaths](../rulings/r9.md): a Feign Death (`unconsciousOnDeath` 1 on a
  player) is no death; a creature's is down for the replay alone.
- [R8 Class inference](../rulings/r8.md): the warlock with no
  COMBATANT_INFO is Destruction by Incinerate.
- [R28 Power](../rulings/r28.md): the `DAMAGE_SPLIT`'s block (off12) reports
  the healer's mana.

## Gating

`crates/core/tests/replay.rs` (every gated fixture's cut against its
golden; the lazy cut — the index's seeds, WORLD_MARKER lines among them —
equal to the cut over every line before the slice; this fixture's kill read
row by row), `tests/fixture_totals.rs` (its meter rows: none of the new
lines moves a total) and `index.rs`'s lazy/full parity and marker seeds.
