---
type: Decision
title: The Replay Tier Lives In The History Store, Cut By The Parser
description: 'v45 (R29): every boss pull and keystone run is cut from its parsed lines into a binary replay tier beside the series tier, kept on the details tier''s caps and all season for what retention keeps whole — and the replay spike''s offline text cutter is rewritten onto the parser''s events, so the log is read one way and the spike''s files come from either path through one writer.'
tags: [replay, history, wire, design]
status: stable
generated: { by: claude-code/opus-5.5, at: 2026-10-09T15:30:00-07:00 }
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
identity — renumbering is a format bump. The tier records only the most
posted floor: a keystone run that spans floors keeps its main one (a
later format can keep the rest). Units carry a GUID for players alone, what
names the owner as `you` at write time; NPCs are known by creature id.

[^contract]: CONTRACT.md — ruling R29 and the v45 row of the version table
[^plan]: Fight replay — research plan (the capture once, interpreted per tier)
[^format]: FORMAT-NOTES — the advanced block's fields 14–18, the replay lines
