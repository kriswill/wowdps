---
type: Decision
title: A Pull Zones In The Door That Logged Zero
description: 'An ENCOUNTER_START while zoned out, at an instanced difficulty on the map the last door named, resumes or opens its visit, because the game logs some doors at difficulty 0 (a raid re-entered after a difficulty switch, delves, Timewalking) and those pulls fell outside any visit while the visit before them stayed live; such a START becomes a seed line, replayed through a new `Meter::seed` that opens no segment.'
tags: [meter, ruling, index]
status: stable
generated: { by: claude-code/opus-5.5, at: 2026-10-04T21:30:00-07:00 }
sources:
  - id: contract
    resource: ../../../CONTRACT.md
    title: CONTRACT.md — R10 (Visits & Overall), Meter::seed, index seeds, tail Seeds
  - id: r10
    resource: ../rulings/r10.md
    title: R10 Visits & Overall — the ruling this amends
  - id: key-start
    resource: ../rulings/r10.md
    title: R10's 2026-09-12 amendment — a CHALLENGE_MODE_START opens a key the door left at 0
---

**Where:** [`wowdps-core`](../crates/core.md) (`Meter::encounter_visit` and
`Meter::seed`, the scanner's mirror in `index.rs`, `TailEvent::Seeds`),
[`wowdps-daemon`](../crates/daemon.md) (the engine replays `Seeds` through
`Meter::seed`, the loader replays a slice through `SegmentText::meter`, the
index cache's magic moves to `\x11`), [R10](../rulings/r10.md); gated by
`crates/core/tests/instance.rs` beside [instance.txt](../fixtures/instance.md).

## Context

[R10](../rulings/r10.md) opens a visit at a ZONE_CHANGE with a nonzero
difficulty and reads 0 as zoned out.[^r10] On 2026-10-04 a raid cleared
The Venomous Abyss on Heroic (its door logged 15), stepped out, switched to
Mythic, and walked back in: every door back in logged
`ZONE_CHANGE,3004,"The Venomous Abyss",0`, while each pull still said
`ENCOUNTER_START,…,16,20,3004`. No Mythic visit opened. The Mythic pulls
landed outside any visit, so the overlay showed them as loose blocks with no
Σ strip, and the Heroic visit stayed current and LIVE for the rest of the
night, its Σ never closed and never stored. A sweep of 47 local logs found
the same 0-logged door in front of delves (208), Timewalking (24) and a
Mythic dungeon (23); Mythic raid doors on other nights logged 16. The keystone
version of this gap was closed on 2026-09-12, when CHALLENGE_MODE_START was
made authoritative;[^key-start] ENCOUNTER_START had no such authority.

## Decision

An ENCOUNTER_START now **zones in** when the meter is zoned out, the pull's
difficulty is instanced, and its instance id is the map the last ZONE_CHANGE
named (`last_zone_map`, a new field on the meter and on `ScanState`). It
resumes the current unended visit when that visit stands on the map at the
pull's difficulty: a keyed one on the map alone, and a keystone pull on an
unkeyed one counts as a key the log joined mid-run. Otherwise it closes the
current visit and opens a new one under the last zone's name at the pull's
difficulty. It settles the visit before its own segment opens, so the segment
carries it.[^contract]

Three narrowings keep the change from reaching anything else:

- **Zoned in, the door stands.** The rule acts only behind a 0-logged door.
  The synthetic `sample.txt` zones in at 16 and pulls at 15, and no real log
  pairs a nonzero door with a different non-keystone pull difficulty, so a
  stricter "the pull always wins" rule would only have split visits that are
  fine today.
- **Instanced difficulty.** Difficulty.db2 gives three ids an InstanceType of
  0 (open world): 172 World Boss, 192 and 230. A world boss's pull keeps its
  place outside any visit (`meter::instanced_difficulty`).
- **The door's own map.** The visit takes the last zone's name, so the rule
  requires the START's instance id to equal the map that zone was logged on.

**The seed.** Ordinals index the file's visit table, so every lazy slice and
the live tail must rebuild the visits this START opened or resumed. The
scanner therefore records every START that zoned in as a SEED line. Feeding
a seeded START would open a segment ahead of the slice, which would land
trash lines in a phantom encounter or announce a phantom closed fight to the
history store. Seeds now replay through a new `Meter::seed`. It feeds every
other seed kind unchanged and applies only the visit rule for an
ENCOUNTER_START. Every caller that had fed seeds now separates them:

- the tailer emits them as `TailEvent::Seeds`;
- a loaded segment is a `SegmentText` that hands out `seeds()` and
  `slice()` apart, and `meter()` replays them the right way. The old
  `load_segment`, one `Vec<String>` of seeds and slice together, is gone,
  so nothing can feed a seed by accident;
- whole-log replays keep `meter_from_lines`.

A START's own slice begins with it, so its own seeds stop short of it. The
meter and the scanner decide every resume through one predicate,
`meter::Standing::resumed_by`, a door's and a pull's alike, so the two cannot
drift.

## Consequences

The real log replays as intended: the Heroic Σ closes where the first Mythic
pull began, and a Mythic visit holds every Mythic pull and the trash between
them. Every lazy slice rebuilds as one segment in the same visit. The trash
between a 0-logged door and its first pull stays outside the visit, the same
cost the door-0 key already accepts for the stretch before its START. Each
trip back through the same door re-pays it until the next pull resumes the
visit. A door logged at 0 never resumes a raid visit by itself, because it
cannot tell whether the difficulty changed while outside.

The index cache's magic moves to `\x11`: a checkpoint scanned by the old rule
holds those pulls outside any visit. Cards the history store already wrote
for such pulls carry no visit until regraded. Landed on branch
`fix/encounter-opens-visit`.

[^contract]: CONTRACT.md — R10 (Visits & Overall), Meter::seed, index seeds, tail Seeds
[^r10]: R10 Visits & Overall — the ruling this amends
[^key-start]: R10's 2026-09-12 amendment — a CHALLENGE_MODE_START opens a key the door left at 0
