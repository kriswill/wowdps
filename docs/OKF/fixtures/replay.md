---
type: Fixture
title: replay.txt
description: 'The replay cut''s fixture (R29, v45) — two boss pulls with world markers placed and replaced before the first, a player''s posts on a second floor and a hit that found him there, every event kind a replay draws (a ranged hit, a miss, a hit from no source, a friendly totem''s tick that is none, a failed cast, a Feign Death, a creature down unconscious, a rez, the boss rows), a warlock known by R8, what players place (a gateway, a totem''s summon, touches, a create, a totem destroyed), a keystone run whose trash and boss stand on two floors, and (format 3) the buffs that matter beside one up before the pull, a healer''s debuff on the boss, two shields and a warlock''s second pool, and (format 4) a buff and a debuff up before the pull opened at its start, and (format 5) casts read after their cost and a warlock''s shards advanced by his energizes, and (format 6) the run''s START listing three affixes, which its cut and its boss pull carry in their heads.'
resource: crates/core/fixtures/replay.txt
tags: [fixture]
status: stable
generated: { by: claude-code/opus-5.5, at: 2026-10-11T12:00:00-07:00 }
---

Every count is laid out by hand in `replay.expected.md`; the TSV holds what
`check.awk`'s R29 block computes from the log grammar alone — and that block
writes each closed boss pull's `replay_*` rows into EVERY gated fixture's
TSV, so [sample.txt](sample.md) and the rest gate the cut too (their pulls
gained the boss rows and a `replay_map_posts` row per floor with tier format
2; [relog.txt](relog.md)'s pulls, whose players are never posted, now count
their creatures' posts on their own floor; with format 3 every pull gained
`replay_absorb`, `replay_pools`, `replay_pool_sum` and the buff rows, and
[sample.txt](sample.md)'s and [tree.txt](tree.md)'s players' debuffs on
their bosses became debuff rows). A finished keystone run's rows
carry `K<n>` in the segment column, kind `Key`. The placed spells are real
ids from the encounter rubric's generated table
([`wowdps-encounter-rubric`](../crates/encounter-rubric.md)), hard-coded in
check.awk as its other tables are.

## Rulings exercised

- [R29 The replay cut](../rulings/r29.md): the floor (thirteen votes on 2434,
  two on 2435, where the hunter's posts and the hit that found him are kept
  with their floor), units in order of first sight, the nineteen event kinds
  (the boss rows at t 0 and the END; format 3's buff rows, format 4's
  opened at t 0), the four placed kinds, the markers
  standing at the start (a marker removed and re-placed before the pull) and
  each change — one placed on another map while it stood on the pull's is its
  removal; a swing whose amount equals the floor's id posted once, as the
  parser reads it; a name with a comma. The keystone run (`K1`, a
  CHALLENGE_MODE_START to its END with a real challenge id, so its par
  timers are known and the store would cut it) holds its trash on floor 2094
  and its boss, an ENCOUNTER_START/END pair mid-run, on 2095: three posts
  above, four below, the main floor 2095, a hit and a Healing Rain touch off
  it, its boss rows at 59 s and 90 s. Format 3, in the first pull: Tank's Bone
  Shield, up before it, loses a stack and comes off — format 4 opens it at
  0 s (`buff_applied`, then the dose and the removal), as R18's span does —
  then goes up again, is refreshed (no row) and stacks (`buff_applied`,
  `buff_dose`); Healer's Ember Mark on the boss, up before the pull, opens
  at 0 s by its refresh and closes by its removal, a second removal no row;
  a Power Infusion seen only by its refresh opens at 0 s; Healer's Pain Suppression on him (`src` the giver), Ula'tek's
  buff on Hunt and the encounter's on Healer are buff rows, Healer's
  Fortitude none (a long buff); Healer's Thorn Rake on the boss is three
  debuff rows (unit the boss), the pet's beside it none; two posts carry a
  shield (`replay_absorb` 1 500), and Lock's soul shards join his mana while
  Ula'tek's `3|4` Spit gives it two pools. Format 5: the Spit pays a cost
  from each (`5|1`: energy 20, combo points 0), Lock's Chaos Bolt spends 20
  of his 30 shard tenths, an Incinerate energize (+ 0.2 shards, 2 tenths), a
  periodic Immolate one (+ 0.1) and a Soul Fire one that went over the cap
  (full, 50) advance the shards his mana-naming blocks never show, and the
  healer's second Rejuvenation in the run pays 1 200 mana (`replay_pools`
  24, `replay_pool_sum` 149 313; the run's 96 100).
- [R22 Self-harm](../rulings/r22.md): Spirit Link Totem's
  redistribution (its damage line a neutral `0xa28`) is friendly fire under
  the Shaman who summoned it and stays in the Tank's Taken — and, one of ours
  by that summon, it is no replay hit and numbers no unit.
- [R9 Deaths](../rulings/r9.md): a Feign Death (`unconsciousOnDeath` 1 on a
  player) is no death; a creature's is down for the replay alone.
- [R8 Class inference](../rulings/r8.md): the warlock with no
  COMBATANT_INFO is Destruction by Incinerate.
- [R28 Power](../rulings/r28.md): the `DAMAGE_SPLIT`'s block (off12) reports
  the healer's mana; Lock's second post reports his soul shards, a second
  power series beside his mana; his Chaos Bolt's second reports 10, after
  its cost (`power_sum` 428); the energizes add 4.0 gained and 0.3 wasted
  ([R27](../rulings/r27.md)).
- [R18 Aura spans](../rulings/r18.md): the pre-pull Bone Shield's removal
  opens a span at the pull's start (the segment-start rule), and since
  format 4 the replay opens it there too; Power Infusion, seen only by its
  refresh, an external of the whole pull in both; its second application runs to the kill, and Pain Suppression is
  an external given and received; [R26](../rulings/r26.md)'s DoT uptime
  takes Thorn Rake.
- [R10 Visits](../rulings/r10.md): the run's CHALLENGE_MODE_START opens its
  keyed visit, whose Σ is what the cut takes; its trash and its boss are
  ordinary segments, so the meter's rows stay check.awk's. Format 6: the
  START lists `[10,9,147]`, which the run's cut and the boss pull inside it
  carry in their heads (`replay_affixes` 3, `replay_affix_sum` 166); every
  other cut of every gated fixture reads 0 and 0
  ([Keep A Keystone Run's Affixes](../decisions/keep-keystone-affixes.md)).

## Gating

`crates/core/tests/replay.rs` (every gated fixture's cut against its
golden, the keystone runs too; the lazy cut — the index's seeds, WORLD_MARKER
and CHALLENGE_MODE lines among them — equal to the cut over every line
before the slice, for pulls and runs; this fixture's kill and run read row
by row), `tests/fixture_totals.rs` (its meter rows, the run's trash and
boss among them) and `index.rs`'s lazy/full parity and marker seeds.
