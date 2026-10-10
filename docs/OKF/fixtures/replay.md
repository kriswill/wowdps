---
type: Fixture
title: replay.txt
description: 'The replay cut''s fixture (R29, v45) — two boss pulls with world markers placed and replaced before the first, a player''s posts on a second floor and a hit that found him there, every event kind a replay draws (a ranged hit, a miss, a hit from no source, a friendly totem''s tick that is none, a failed cast, a Feign Death, a creature down unconscious, a rez, the boss rows), a warlock known by R8, what players place (a gateway, a totem''s summon, touches, a create, a totem destroyed), and a keystone run whose trash and boss stand on two floors.'
resource: crates/core/fixtures/replay.txt
tags: [fixture]
status: stable
generated: { by: claude-code/opus-5.5, at: 2026-10-09T22:00:00-07:00 }
---

Every count is laid out by hand in `replay.expected.md`; the TSV holds what
`check.awk`'s R29 block computes from the log grammar alone — and that block
writes each closed boss pull's `replay_*` rows into EVERY gated fixture's
TSV, so [sample.txt](sample.md) and the rest gate the cut too (their pulls
gained the boss rows and a `replay_map_posts` row per floor with tier format
2; [relog.txt](relog.md)'s pulls, whose players are never posted, now count
their creatures' posts on their own floor). A finished keystone run's rows
carry `K<n>` in the segment column, kind `Key`. The placed spells are real
ids from the encounter rubric's generated table
([`wowdps-encounter-rubric`](../crates/encounter-rubric.md)), hard-coded in
check.awk as its other tables are.

## Rulings exercised

- [R29 The replay cut](../rulings/r29.md): the floor (nine votes on 2434,
  two on 2435, where the hunter's posts and the hit that found him are kept
  with their floor), units in order of first sight, the sixteen event kinds
  (the boss rows at t 0 and the END), the four placed kinds, the markers
  standing at the start (a marker removed and re-placed before the pull) and
  each change — one placed on another map while it stood on the pull's is its
  removal; a swing whose amount equals the floor's id posted once, as the
  parser reads it; a name with a comma. The keystone run (`K1`, a
  CHALLENGE_MODE_START to its END with a real challenge id, so its par
  timers are known and the store would cut it) holds its trash on floor 2094
  and its boss, an ENCOUNTER_START/END pair mid-run, on 2095: three posts
  above, four below, the main floor 2095, a hit and a Healing Rain touch off
  it, its boss rows at 59 s and 90 s.
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
- [R10 Visits](../rulings/r10.md): the run's CHALLENGE_MODE_START opens its
  keyed visit, whose Σ is what the cut takes; its trash and its boss are
  ordinary segments, so the meter's rows stay check.awk's.

## Gating

`crates/core/tests/replay.rs` (every gated fixture's cut against its
golden, the keystone runs too; the lazy cut — the index's seeds, WORLD_MARKER
and CHALLENGE_MODE lines among them — equal to the cut over every line
before the slice, for pulls and runs; this fixture's kill and run read row
by row), `tests/fixture_totals.rs` (its meter rows, the run's trash and
boss among them) and `index.rs`'s lazy/full parity and marker seeds.
