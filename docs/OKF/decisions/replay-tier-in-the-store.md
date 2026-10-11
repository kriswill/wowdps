---
type: Decision
title: The Replay Tier Lives In The History Store, Cut By The Parser
description: 'v45 (R29): every boss pull and keystone run is cut from its parsed lines into a binary replay tier beside the series tier, kept on the details tier''s caps and all season for what retention keeps whole — and the replay spike''s offline text cutter is rewritten onto the parser''s events, so the log is read one way and the spike''s files come from either path through one writer; tier format 2 keeps every floor''s posts and the boss rows, so a keystone run replays through its whole dungeon; format 3 adds the shields, every pool kind, the buffs that matter to the fight and the players'' debuffs on NPCs, the rows a followed unit''s HUD reads; format 4 counts an aura up before the pull R18''s way, from its start; format 5 reads each pool as the game''s bar, a cast''s after its cost and an energize''s gain on the pool it fills.'
tags: [replay, history, wire, design]
status: stable
generated: { by: claude-code/opus-5.5, at: 2026-10-09T22:00:00-07:00 }
sources:
  - id: contract
    resource: ../../../CONTRACT.md
    title: CONTRACT.md — ruling R29 and the v45 row of the version table
  - id: plan
    resource: ../../plan-fight-replay.md
    title: Fight replay — research plan (the capture once, interpreted per tier)
  - id: format
    resource: ../../../crates/core/fixtures/FORMAT-NOTES.md
    title: FORMAT-NOTES — the advanced block's fields 14–18, the replay lines
---

**Where:** [`wowdps-model`](../crates/model.md) (`replay`: the rows),
[`wowdps-core`](../crates/core.md) (`replay::cut`, `cut_and_meter`, the
parser's `HpHint.pos` and the passive replay events, WORLD_MARKER seeds in
the index), [`wowdps-proto`](../crates/proto.md) (`replay`: the `WDRP`
codec and `replay::csv`; `GetReplay` / `Replay`, PROTO_VERSION 45),
[`wowdps-daemon`](../crates/daemon.md) (`daemon::replay` over
[`wowdps-encounter-rubric`](../crates/encounter-rubric.md)'s placed table,
the history store's replay slots, `CutJob`), and
[`wowdps-history`](../crates/history.md) (`replay-export`, `stats`). The
ruling is [R29](../rulings/r29.md); the fixture
[replay.txt](../fixtures/replay.md).

## Context

The replay spike drew pulls cut OFFLINE by the extractor's `cut-pull`, a
port of three Perl scripts that read the log as text with Perl's field
split and number reading — a second reading of the log beside the
parser's, and one only a developer with the log on disk could run. The plan
[A Fight Replay Captured Once](fight-replay-captured-once.md) wanted the
capture taken in the background once per pull and kept, so a replay could
open any stored pull.[^plan]

## Decision

- **One reading.** The cut is rewritten onto the parser's `LogLine` /
  `Event`, not moved as text: the advanced block's place joins `HpHint`
  (fields 14–17, exact fixed point; 18 the item level), and what only the
  replay reads — a failed cast and its reason, a create, a unit destroyed,
  a creature down unconscious, the world markers — parses as passive events
  no meter ledger reads, so segmentation and lazy/full parity hold; every
  WORLD_MARKER line becomes an index seed so a lazy pull knows the markers
  standing.[^format] `cut(seeds, slice, placed, owner)` reads each line
  once (`cut_and_meter` shares the parse with the meter on an import).
  Where the reading parts from the text cutter it is on purpose, listed in
  `core::replay`'s module doc; validated over 48 real pulls, every
  remaining difference is classed (a deliberate parser reading, an old
  cutter bug, or a new-cut bug fixed — the `DAMAGE_SPLIT` block the parser
  never found).
- **A binary tier in the store.** `replay/<id>.bin` (`WDRP`, a section
  index, interned strings, each post coded against its unit's last): a
  10-minute 26-player raid pull is ~1.6 MB against 17 MB of CSV. The raid's
  damage per second is NOT kept again — R25 answers it from the details
  tier, and the CSV writer takes it from there.[^contract]
- **Kept like the details.** Written for every boss pull and keystone run,
  wipes included (never trash, arenas, an aborted pull or a raid night's Σ); a fight keeps it
  while protected — every kill, timed key, progression wipe
  ([A Progression Wipe Is Kept Whole](progression-kept-whole.md)) and pin all
  season — else among the newest `history_keep_details_per_encounter` of its
  group, under a generous `history_replay_mb` (4 GiB) that never takes a
  protected one. Produced from the log through the existing rewrite path:
  beside the meter on import and regrade, as a cut alone after a live close
  once the history thread is idle, and by the rewrite queue for a slot
  missing a current file — so stored fights gain it without a regrade.
- **One writer.** `proto::replay::csv` writes a `Cut` as the seven files
  the spike reads, column for column as the old writers did, so the
  extractor's `cut-pull` and `wowdps history replay-export` share it.

## Consequences

The spike can read the store (after the branch merges) instead of a
developer's cut; the parser now knows where every unit stood, which later
features can use. A unit's number is its first sight, so it is the format's
identity — renumbering is a format bump. Format 1 recorded only the most
posted floor; format 2 (below) keeps the rest. Units carry a GUID for
players alone, what names the owner as `you` at write time; NPCs are known
by creature id.

## Format 2: every floor and the boss rows

**Context.** Format 1 kept one floor per fight, the UiMap id the players
were posted on most, and dropped every post on another. A boss room is one
UiMap (48 of 48 real pulls), but a keystone run walks a dungeon's levels:
cut whole, three real +14 runs lost 36 %, 58 % and 59 % of their posts
(Ruby Life Pools 44 626 of 123 727 on floors 2095 and 1978 beside 2094;
Altar of Fangs 86 978 and 107 212 on 2589 and 2590 beside 2588), with the
hits that found a player there and the units seen only there. Nor did the
cut say where the bosses inside a run began and ended: the ENCOUNTER lines
sat in the slice, and only the card's `bosses` knew.[^contract]

**Decision.**

- **Every post keeps its floor** (`Post::map_id`); the cut's `floor` keeps
  its meaning — the players' most posted, what a boss pull is drawn on —
  and `Cut::maps` counts each floor's posts, so it stays derivable. A hit's
  place and a placed thing's are on whatever floor the block put the unit,
  the victim's or caster's post at that moment saying which.
- **Boss rows**: each ENCOUNTER_START and ENCOUNTER_END the slice holds is
  an event (`boss_engaged`, `boss_killed`, `boss_wiped`), the encounter as
  the row's spell and no unit — the first unit-less row, so
  `Event::unit` became an `Option`. A boss pull holds its own two; a run's
  match the card's `bosses` to the millisecond, wipes included.
- **The tier carries both without paying for a single floor.** The post
  head byte had no free bit, so the floors are a `maps` section: a table,
  then only the tracks that leave the head's floor, each change of floor at
  its post index. A one-floor pull pays four bytes (the 48 pulls grew 22 to
  106 bytes each with their boss rows: 17 428 496 → 17 430 035 B); a
  unit-less row is `has` bit 5. Format 1 still reads (every post on its
  floor), and as an older format the rewrite queue recuts it from its log
  while idle, as the series tier's were at v42 — no regrade, no wire change
  (`GetReplay` carries the bytes opaquely).
- **CSV**: `tracks.csv` gains a trailing `map_id`, and `events.csv` writes
  the boss rows in its eight-column shape (`unit` empty) — additive for a
  reader by column.

**Consequences.** A run's replay can follow the party from floor to floor
and mark each boss fight, and its tier is larger for it (Altar of Fangs
+14: 413 129 → 836 288 B). A reader that draws one floor must now filter a
track by `map_id`, or it would draw another level's posts on its own.

## Format 3: shields, every pool, the auras a followed unit's HUD reads

**Context.** The replay spike's followed-unit HUD (its plan on the replay
branch, phase 2) draws a player's defensives and externals under its frame,
the player's own auras over its target's, a ghosted absorb on its health and
its class pips. Format 2 had none of it: no buff rows at all, debuff rows on
players alone, no shield, and one pool per post — the first the block
names, which is not always the same kind for a unit (a Retribution
paladin's posts in a real +15 run named mana 10 727 times and holy power
395), so a class whose resource seldom leads the line showed no pips.[^format]

**Decision.**

- **Buff rows, cut to what matters to the fight** (`buff_applied`,
  `buff_removed`, `buff_dose`, kinds 16–18, the debuff rows' columns): a
  buff on a player whose spell [R18](../rulings/r18.md)'s curated
  `role_spells` table names as anything but a support buff — a defensive, a
  tank's mitigation, an external (its giver in `src`), a major or a healing
  cooldown — or whose source is not ours (an NPC's empowerment or mark, the
  encounter's from no one). As format 3 shipped, a removal or dose whose
  application the cut never saw was dropped as up before the pull; format 4
  (below) counts such a buff R18's way instead, from the pull's start. A long
  buff (stamina, a flask) is in neither set, so it makes no row at all. A
  buff from a hostile unit, a debuff row in format 2, is a buff row now.
- **A player's debuffs on NPCs, every one** (the debuff kinds, `unit` the
  NPC, `src` the player): the evidence of what a player pressed, a triggered
  effect whose cast the log never writes included (a guardian druid's Feral
  Frenzy logs no cast, only its triggered spell's hits and a debuff on the
  target — two applications on the boss in a real pull, and no cast row
  of either id).
- **Posts keep the block's absorb and every pool kind.** `Post::absorb` is
  field 9; `Post::pools` is the last reading of every kind the unit's posts
  have named, every entry of a cast's `a|b` lists in (`HpHint.more_power`;
  the first stays `HpHint.power`, the only one [R28](../rulings/r28.md)
  reads). Pools grow from posts alone, by one rule
  (`model::replay::set_pool`).
- **The tier pays for the rows, not the pools.** The posts section is format
  2's byte for byte. An `absorb` section (tag 10) codes each track's
  changes of shield at their post indices; a `pools` section (tag 11) lists
  only the posts whose pools are not their last's with their own pool set
  in, which only a cast's second pool makes — every format reads the rest by
  that rule, so a format-2 file reads its posts' own pools and no shield.
  Format 2 still reads, and as an older format the rewrite queue recuts it
  from its log while idle — no regrade, no wire change.
- **CSV**: `tracks.csv` gains trailing `absorb` and `pools`
  (`kind:current:max`, `|` between), `events.csv` the buff rows in the debuff
  rows' shapes — additive for a reader by column.

**Consequences.** Recut from the night's log (2026-10-10), two +15
keystone runs grew 678 967 → 758 560 B (+11.7 %) and 883 453 → 990 552 B
(+12.1 %), and one boss pull inside the second 88 310 → 95 721 B (+8.4 %):
the events section +56 397 and +80 139 B (1 458 and 2 311 buff rows, 6 431
and 8 821 player debuffs on NPCs — the Ironfur and Void Metamorphosis
stacks most of the buff doses), the absorb section 22 879 and 26 526 B, the
pools section 1 B. 21 885 and 37 972 posts carried two pool kinds or more,
every one a player's. Against the run's stored R18 spans, 91 of one player's 93
defensive spans matched the buff rows to the millisecond; the other two are
one buff the meter split at a segment seam (974 429 / 974 432 ms), which
the cut reads whole. A unit's number is still its first sight, so the new
rows can number a unit earlier than format 2 did — the format bump covers
it. The HUD's reader takes the new kinds and the trailing columns by
position; the pull directories the spike cuts itself gain them through the
one writer.

## Format 4: an aura up before the pull counts, R18's way

**Context.** An independent review of format 3 found its cut dropped a
defensive that was up when the pull began, though [R18](../rulings/r18.md)
counts that one from the pull's start, so the HUD's buff row and the
meter's cooldown windows would disagree on every pre-pull Bone Shield or
Ironfur. The user's call (2026-10-10): count it R18's way. The review also
found that decoding and cutting could allocate quadratically from a small
file, since every post copies its unit's growing pool set and a kind was any
u32; that `encode` wrote a hand-built post's untidy pools into a file decode
refuses; and that a cut-rule change alone recuts nothing.[^contract]

**Decision.**

- **R18's segment-start rule in the cut.** A buff R18's table names, or a
  player's debuff on an NPC, keyed (unit, spell, caster), whose first line
  in the slice is a refresh, a dose or a removal was up when the slice
  began: an applied row at t 0 opens it (once per key, that line's caster
  as `src`), and the dose or removal is kept; a later dose or removal with
  nothing up is dropped, and a refresh is never a row of its own. An NPC's
  buff R18 does not name keeps format 3's rule (only after an application),
  and a debuff on a player keeps every row. The opened row is found at its
  line, numbering its units there, and the events are sorted by time, so it
  stands at t 0 after the first lines' rows. Two deliberate differences
  from R18: a dose is a first sight too (R18 reads no dose), and a key's
  first sight is the only one that can open it (R18 would open again after
  a stray second removal).
- **Pools are bounded.** A power type at or above 64 (`replay::POOL_KINDS`;
  the game's run below 32) is no pool, in the parser's `power` and
  `more_power` alike, so R28 reads none either; `set_pool` keeps at most
  eight kinds (`MAX_POOLS`, a druid's every form names six). The decoder
  refuses such a type and a pools list longer than eight before allocating
  it, so a post's pools are a few readings whatever a file says, and
  decoding allocates in proportion to the bytes in hand again.
- **`encode` writes what decodes.** Each post's pools go through
  `model::replay::next_pools` (sorted, one per kind, bounded; a post that
  lists none grown from its own pool), and a power type no reader takes is
  written as none; `model::replay::fill_pools` tidies a hand-built track the
  same way, for an importer that fills `power` alone.
- **The recut is a format number.** The rewrite queue knows a file by its
  head's number alone and no other marker records the rules a file was cut
  by, so FORMAT is 4 on format 3's codec, byte for byte but the head: every
  format-1, 2 and 3 file is recut from its log while idle. Format 3 had
  shipped on main (#104) but was never deployed; the number still moved,
  since the dev unit builds from a branch that already carries it.

**Consequences.** Recut from the night's log, the two +15 runs opened 1 and
2 buffs at t 0 (a guardian druid's Incarnation up when each key began, and
a Void Metamorphosis), the boss pull inside the second 1 (that Void
Metamorphosis); no player's debuff on an NPC needed it and no orphan row was
dropped. The files grew 11, 25 and 12 B; the pool bound and the tidying
changed no byte (no post carried more than three kinds). Against the stored
R18 spans, the boss pull's 45 spans, every player's, matched to the
millisecond, its t 0 Void Metamorphosis included; over the second run, 132
of one player's 145 marks matched to the millisecond and the other 13 are
the meter's own seams — five spans crossing a segment boundary (two marks
each, one span here), the run's start (R18 opens at the first segment's
first combat line, 1 673 ms after the run's) and its end (two buffs still up
when the last segment closed; the cut keeps them to the run's end).

A known gap: a pet's, a guardian's or a totem's debuff on an NPC is never a
row (never credited to its owner), so the HUD's own-aura row misses a
hunter's pet's bleeds or a warlock's demon's debuffs.

## Format 5: a pool reads as the game's bar

**Context.** The HUD's resource row showed a Demonology warlock at 5 soul
shards through half a minute in which he cast Hand of Gul'dan five times.
Two readings were wrong, both from the log's shape.[^format] A
`SPELL_CAST_SUCCESS` reports its caster BEFORE the cast pays: Hand of
Gul'dan's block reads `7,50,50,30` (50 tenths of a shard, the cost of 30
in field 13, which the parser dropped), so the cut stored 50 where the bar
showed 20. And the warlock's shards rise only through `SPELL_ENERGIZE`
lines whose block, the target's, names his mana: the gain is in the suffix
(`2.0000,0.0000,7,50`), which the cut never read, so the pool stood at the
last spender's pre-cost value until the next spender. A paladin's holy
power, a rogue's combo points and a monk's chi read the same way.

**Decision.**

- **The parser reads the cost.** `parser::Power` gains `cost` (block field
  13, each `a|b` entry its own; nonzero only on a cast's success) and its
  `current` is the block's less that cost, never below 0, so every reader
  sees the pool after the line. That reader includes
  [R28](../rulings/r28.md)'s power series: no fixture line carried a cost
  and no ruling pinned the pre-cost value, and the one consumer that draws
  it (a healer's mana under the inspector, its "low" point) reads truer
  after the cost, as the game's own bar shows it. A regrade moves the
  stored series.
- **The cut applies energizes.** On an energize line whose block posts its
  destination and names no reading of the power type the suffix fills, the
  cut sets that kind to its last reading (0 with none) plus the gain, at
  most the max (the suffix's, else the last reading's); a gain that lost
  any of itself to the cap fills the pool; with no max, nothing. A block
  that names the type already reads after the gain (a demon hunter's fury,
  a druid's rage) and stands as it is. The gain is in the game's displayed
  units and the block in its own: tenths for rage, runic power, soul shards
  and astral power, hundredths for insanity (`display_mod`, measured on
  real lines where a block names the pool just energized: Bear Form's 25
  rage reads 250, Tentacle Slam's 6 insanity 600, a shard's 1.0 against a
  max of 50). Nothing between two lines is inferred: a regenerating mana or
  focus bar moves only when a line says so.
- **An energize starts only a counted pool.** With no reading of its kind
  yet, an energize opens the pool (from 0) only when its max is below 100
  in the block's units (`COUNTED_POOL_MAX`): soul shards, holy power, combo
  points, chi, arcane charges, essence, runes — the resources a block names
  only on a spender's line, which empty out of combat. A continuous pool
  (energy at 100 or more, focus, fury, maelstrom, rage, astral power, runic
  power, insanity, mana) it advances only once a block has named it: a
  Restoration shaman's Mana Spring on an elemental shaman, whose blocks name
  maelstrom, would otherwise open a mana pool that is spent on lines that
  never name it and so only climbs — and the replay frame's power line,
  which shows the pool that moved most, could pick it (the coordinator's
  call in review, 2026-10-11).
- **The format number moves** (FORMAT 5 on format 4's codec), since every
  stored file's `power` and `pools` change and the rewrite queue knows a
  file by its number alone.

**Consequences.** Recut from the night's log, the warlock's shards read
50→20→40→10→20→30→50→20→30→0→10→30 across the half minute, each spender's
block equal to the last spender's leftover plus the gains between; over two
+15 runs 526 of his 533 shard spenders closed exactly (the rest a gain the
log does not write as an energize), and 394 of a paladin's 395 holy power
spenders. The store's 595 files took about three minutes to recut as
format 4, and format 5's recut costs as much once deployed. A known gap: a
counted pool's first gain in a cut starts from 0, though a warlock walks in
with shards, until his first spender re-anchors it.

[^contract]: CONTRACT.md — ruling R29 and the v45 row of the version table
[^plan]: Fight replay — research plan (the capture once, interpreted per tier)
[^format]: FORMAT-NOTES — the advanced block's fields 14–18, the replay lines
