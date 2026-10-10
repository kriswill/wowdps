---
type: Decision
title: The Replay Tier Lives In The History Store, Cut By The Parser
description: 'v45 (R29): every boss pull and keystone run is cut from its parsed lines into a binary replay tier beside the series tier, kept on the details tier''s caps and all season for what retention keeps whole — and the replay spike''s offline text cutter is rewritten onto the parser''s events, so the log is read one way and the spike''s files come from either path through one writer; tier format 2 keeps every floor''s posts and the boss rows, so a keystone run replays through its whole dungeon.'
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

[^contract]: CONTRACT.md — ruling R29 and the v45 row of the version table
[^plan]: Fight replay — research plan (the capture once, interpreted per tier)
[^format]: FORMAT-NOTES — the advanced block's fields 14–18, the replay lines
