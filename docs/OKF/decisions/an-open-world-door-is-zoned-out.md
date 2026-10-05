---
type: Decision
title: An Open-World Door Is Zoned Out
description: 'A ZONE_CHANGE onto a map Map.db2 calls open world (InstanceType 0, the generated `open_world_maps.rs`) reads as difficulty 0 whatever it carries, because the game stamps a door out of an instance with the difficulty it just left and those doors opened "Silvermoon City" visits that split a raid night in two.'
tags: [meter, ruling, game-data]
status: stable
generated: { by: claude-code/opus-5.5, at: 2026-10-04T23:10:00-07:00 }
sources:
  - id: contract
    resource: ../../../CONTRACT.md
    title: CONTRACT.md — R10 (Visits & Overall), the open-world door amendment
  - id: pull
    resource: a-pull-zones-in.md
    title: A Pull Zones In The Door That Logged Zero — the resume this protects
---

**Where:** [`wowdps-core`](../crates/core.md) (`meter::door_difficulty`, read
by `Meter`'s ZONE_CHANGE arm, `Meter::encounter_visit` and the scanner alike;
the generated `open_world_maps.rs`), [`tools/extract`](../crates/extract.md)
(`mapgen.rs`), [gen-open-world-maps](../tools/gen-open-world-maps.md),
[R10](../rulings/r10.md); gated by `crates/core/tests/instance.rs` beside
[instance.txt](../fixtures/instance.md).

## Context

[R10](../rulings/r10.md) opens a visit at a ZONE_CHANGE with a nonzero
difficulty.[^contract] The game does not always clear the difficulty on the
way out: a hearth out of a Heroic raid logs
`ZONE_CHANGE,0,"Silvermoon City",15`, the end of a key
`ZONE_CHANGE,2444,"The Waking Shores",8`, and a sweep of 47 local logs found
ten such doors (map 0's Silvermoon City and The Coiled Isle at 8, 14, 15, 16
and 208; the Dragon Isles at 8). Each opened a visit in a city, with members
and an emitted Σ: the history store held five "Silvermoon City" Σ cards, one
at Mythic with 36.8 minutes of combat. It also closed the raid's visit, which
defeats [the pull's resume](a-pull-zones-in.md):[^pull] the next pull behind a
door logged at 0 found a city visit current and opened a second raid visit,
splitting the night's Σ.

## Decision

Map.db2 says which maps are instanced: its InstanceType is 0 for the open world
(Eastern Kingdoms, Kalimdor, the Dragon Isles, Voidstorm, Vaults of Atal'Utek
— 589 maps on build 12.1.0.69933) and 1, 2 or 5 for every dungeon, raid and
delve the sweep met. `tools/gen-open-world-maps.sh` decodes it from the local
install, as the keystone timers are, into the sorted `open_world_maps.rs`.
`meter::door_difficulty(map, difficulty)` reads a door onto a listed map as 0,
and both the meter and the scanner call it before any R10 decision, so they
cannot disagree; a pull on a listed map never zones in either. A map the table
does not know, from a patch newer than the table, keeps its door's
difficulty: a stale table fails toward the old reading, never toward hiding an
instance.

The cheaper guard, "a door onto map 0 is zoned out", was rejected: the Dragon
Isles door at 8 is map 2444, and the next expansion's continents would each
need another special case.

## Consequences

Tonight's log loses its "Silvermoon City" visit, and five other logs with
such doors keep scanner and replay in step with no city visit. An abandoned
key the old rule closed at such a door now resumes on re-entry, as R10 says
an abandoned key does, so the short pre-key visit that followed it is gone
too. The index cache's magic moves to `\x12`.

A rescan of the ten affected logs against the store found nine Σ cards the
new rules no longer produce: eight open-world ones (five "Silvermoon City",
two "The Coiled Isle", one "The Waking Shores") and that one pre-key "Ruby
Life Pools". A regrade skips them, because it matches a Σ card to an Overall
starting at the same moment, so they have to be removed by their fight ids;
every other Σ card of those logs regrades in place. The table is one more
per-patch generator to run.

[^contract]: CONTRACT.md — R10 (Visits & Overall), the open-world door amendment
[^pull]: A Pull Zones In The Door That Logged Zero — the resume this protects
