---
type: Decision
title: Draw The Replay's Room From The Game's Own Floors
description: 'The replay''s floor comes from the client, joined by the log''s own ui_map_id: the in-game dungeon map per floor for v1 (verified against a real kill), the minimap render as an optional true floor where floors don''t stack, and a render of the floor''s WMO groups as the route to sharp rooms — never a hand-captured image per boss.'
tags: [replay, game-data, design]
status: draft
generated: { by: claude-code/opus-5.5, at: 2026-10-04T13:30:00-07:00 }
sources:
  - id: assets
    resource: ../../replay-assets.md
    title: Fight replay — game data and assets (§4 rooms, §5 art, §8 prior art)
  - id: plan
    resource: ../../plan-fight-replay.md
    title: Fight replay — research plan (Q2, Q4, Q14)
---

**Where:** planned floor generators in [`wowdps-extract`](../crates/extract.md),
which already decodes BLP and reads CASC. The transform and the geometry
belong to [`wowdps-gui-logic`](../crates/gui-logic.md). The game-data scope
is [Scope The Replay's Game Data To The Season The Client Names](replay-data-from-the-season.md),
and the room's orientation is
[The Replay's Room Turns To The User, Without Edges](replay-room-turns-without-edges.md).
A draft: v1's choice is made, and the sharp route is a spike[^assets].

## Context

A replay needs the room under the dots, on every boss of every tier,
without per-boss work.

An existing web replay, compared on 2026-10-04 and described here
generically[^assets], draws each of its four bosses on a hand-captured
top-down render. That is about 5 px per yard, its boss icons are crops of
the journal portraits, and its shapes are circles. It looks sharp, and it
is the per-boss cost the plan's generic layers exist to avoid. Nothing of
its code, art or words is used[^plan].

## Decision

**Three floor sources, all from the install.**

- **The dungeon map, as v1's floor.** The chain is
  `UiMap → UiMapXMapArt → UiMapArt → UiMapArtTile`: twelve 256-px tiles per
  floor, stitched to 1002 × 668. The log's advanced block names the floor
  directly (`ui_map_id`).
  - **The transform** is `UiMapAssignment`'s world region, with the log's
    `position_x` as world X (up the map) and `position_y` as world Y (left):
    `u = (y_max − y)/(y_max − y_min)`, `v = (x_max − x)/(x_max − x_min)`.
  - **Verified** on a real Coiled Altar kill. A sampled plot of every unit
    lands on the altar's platform. The pull's centre is (0.49, 0.76), and
    the journal's boss pin is (0.50, 0.77).
  - **Right for every floor and every difficulty**, but about 1 px per
    yard, and parchment.
- **The minimap render, optional.** The ADT tiles in the map's WDT
  (`MAID`, entry 7) are a true top-down render, 512 px per 533⅓ yd. Every
  Season 2 map has them. They hold one layer, so they are offered only
  where floors don't stack.
- **A render of the floor's WMO groups, as the route to sharp.**
  - **The joins hold.** `UiMapAssignment.WMODoodadPlacementID` is the ADT's
    `MODF` unique id. Its `WMOGroupID`s are the `MOGP` unique ids of exactly
    that floor's group files.
  - **The rooms are tractable.** 30–60k triangles each, with textures named
    by FileDataID.
  - **What it fixes.** A top-down render at any resolution, on the right
    floor by construction, fixes both limits above.

**Boss portraits and instance art come from the journal tables.** No
models: an `.m2` needs a renderer. A creature's bounding box only sizes a
footprint.

## Consequences

**v1 ships on data that already decodes.** That's `blp.rs`, the CASC
reader, and a transform proven on one pull. Q2's last piece, the facing
convention, has one data point so far.

**The sharp floor is a spike, not v1.** It decides three things: ceiling
removal (the log carries no Z), texture blending, and the time and size per
floor.

**Size.** Season 2's 24 floors are about 13 MB as BLP, or 64 MB decoded.

Landed as research in `88e2f58`.

[^assets]: `docs/replay-assets.md` §4 (the three sources, the transform and its check), §5 (art) and §8 (the comparison).
[^plan]: `docs/plan-fight-replay.md` Q14: our data comes from the log and the client, and our words are our own.
