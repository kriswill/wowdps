---
type: Decision
title: The Replay's Room Turns To The User, Without Edges
description: 'The replay''s map rotates to the user''s picture of the room — a default from the room''s wall geometry and the raid-to-boss bearing at the pull, the user''s angle kept per encounter — as a view setting only; and because GPUI draws images axis-aligned, the floor is resampled on the CPU into a viewport-sized image whose empty space is transparent, so no rotation shows a hard edge.'
tags: [replay, gui, design]
status: stable
generated: { by: claude-code/opus-5.5, at: 2026-10-04T13:30:00-07:00 }
sources:
  - id: plan
    resource: ../../plan-fight-replay.md
    title: Fight replay — research plan (decision 8, the GUI's rotation bullet, Q12)
  - id: assets
    resource: ../../replay-assets.md
    title: Fight replay — game data and assets (§4 "Orientation" and "Rotating without edges")
  - id: scene
    resource: ../../gpui/README.md
    title: The GPUI reference (gpui-pre 0.3.7 sources mirrored under docs/gpui/src, scene.rs)
---

**Where:** the planned replay mode in [`wowdps-gui`](../crates/gui.md). The
view transform and its words go in [`wowdps-gui-logic`](../crates/gui-logic.md),
and config holds the user's angle. The floor sources are
[Draw The Replay's Room From The Game's Own Floors](replay-floors-from-game-files.md),
for [A Fight Replay Captured Once And Interpreted Per Tier](fight-replay-captured-once.md).

## Context

Every floor source the game has is drawn with its north up. Players don't
think in that frame: they think "boss at the top", "we came in at the
bottom". The Sentinels room, for one, sits diagonal on every game map.

The user's call (plan decision 8[^plan]): the room turns, and a slightly
different mental model is the user's to keep.

## Decision

**A view setting, not data.**

- **One transform, about the room's centre**, turns the floor, the
  positions, facing ticks, cones, lines and shapes together. Discs, icons
  and text stay upright.
- **A north mark** keeps the game's frame readable.
- **No mirroring.** In-game left and right calls depend on that.
- **Where it lives.** The user's angle is saved in config per encounter,
  and per floor for a fight that changes floors. Difficulty never matters.
- **What never rotates.** The capture, the interpretation and the mcp
  answers stay in world coordinates. Words name directions relative to the
  room or the boss, never by compass.

**A default from data**[^assets].

- **The wall angle.** Fold a room's near-vertical triangles' normals mod
  90°, weighted by area, and the peak is its wall angle. The Sentinels room
  is 60° off north (88% of its wall area within ±2°); the Coiled Altar's
  room is 0°.
- **Which side is up.** Take the bearing from the raid's centroid at the
  pull to the boss, and snap it to the nearest of the room's four
  orientations. On the Coiled Altar kill the bearing was +2°, which gives
  the game's own north-up.

**No hard edges at any angle.**

- **GPUI cannot rotate an image.** `PolychromeSprite` has axis-aligned
  bounds and no transform; only monochrome sprites take a
  `TransformationMatrix`[^scene].
- **This is designed around, not patched**, per
  [No Forked Or Patched GPUI](no-gpui-forks.md).
- **CPU resampling instead.** The floor is resampled on the CPU into an
  image exactly the viewport's size, for each settled angle and zoom. One
  pass rotates, upscales, keys empty space to transparent and feathers the
  source's edges.
- **What the GPU sees.** Only an upright, viewport-sized image, so the
  rectangle's corners never exist.

## Consequences

**Edges, per floor source.** The game ships no larger image of a room's
surroundings, so coverage comes from the sources themselves:

- **The minimap's empty space** is one exact colour, RGB (112, 113, 112).
  Keyed out, neighbouring tiles fill the corners with real corridors.
- **The parchment art** is opaque and framed. 10 of the 12 Season 2 boss
  rooms measured have over 100 yd of margin before the frame, enough for a
  room-sized view. The rest fade into the background rather than cut off.
- **A WMO render** has no edge problem at all.

**Cost.** A resample is a few milliseconds for a 900 × 700 view. It runs
per frame only during a rotate or zoom drag, finite as every animation here
must be, so the drawing spike (Q12) budgets a rotate drag.

**Liquid planes keep a hard edge.** They render in the minimap as flat
rectangles. That is real content and turns correctly.

Recorded in `88e2f58`.

[^plan]: `docs/plan-fight-replay.md`, decision 8 and §4's rotation bullet.
[^assets]: `docs/replay-assets.md` §4, "Orientation" (the measurements) and "Rotating without edges" (the demo on the Sentinels room).
[^scene]: `docs/gpui/src/gpui-pre-0.3.7/src/scene.rs`: `PolychromeSprite` against `MonochromeSprite` / `SubpixelSprite`.
