---
type: Decision
title: Keep An Encounter's Map In The Base, Cut To A Stencil
description: 'An encounter''s map, meaning how its room is drawn and first shown and what changes it in the fight (rooms off it, its places, the events its layers break on), is the rubric''s base however much of it was made by hand, and so is what an NPC in the room is where the draft lacks one or has it wrong. Both live in the instance file''s [encounter.<id>], and a [map] stencil cuts the render to what of the room matters.'
tags: [replay, rubric, game-data]
status: stable
generated: { by: claude-code/opus-5.5, at: 2026-10-06T22:30:00-07:00 }
sources:
  - id: readme
    resource: ../../../crates/encounter-rubric/README.md
    title: The rubric's schema reference — Files, How an encounter resolves, [map]'s stencil
  - id: tiers
    resource: rubric-base-and-curated.md
    title: Keep The Rubric's Base Apart From Its Curated Layer
---

**Where:** [wowdps-encounter-rubric](../crates/encounter-rubric.md) (the instance file's
`[encounter.<id>]`, `StencilShape`, `Map::stencil_bounds` and
`stencil_depth`), read by gen-floors and the replay (its own
repository).

## Context

The tiers split an encounter into the base, what extraction gives, and
curated, what a person found by investigating the fight[^tiers]. Every
hand-made `[map]` tweak sat in the tuned files: Entombed Sentinels' hidden
strips, Nek'zali's light and Ula'tek's platform layers. So a base-only
reader would have drawn the rooms without them.

The rooms themselves needed more than tweaks. Entombed Sentinels' render
was 10082×10008 px at 8 px per yard, about 1,260 yd on a side, for a fight
held in about 130 yd. Most of it was a 210 yd venom plane under the
platform and the corridors around it. The replay showed that plane bright
green round the arena, and the render took a long time to make.

## Decision

An encounter's map is the base's, however much of it was made by
hand[^readme]:
- **Where it lives.** The instance file gives it per encounter, by
  DungeonEncounterID: `[encounter.<id>.map]` and `[encounter.<id>.view]`.
  An encounter resolves from the season's defaults, the instance's, the
  draft, then this, then the tuned file. `gen-rubric` writes an instance
  file only when it is missing, so the hand-made map is safe from it.
- **What it holds.** The map, how it is first shown, and what changes it
  in the fight: `map` and `view`, then `room`, `place` and `event`. Since
  2026-10-08 also `npc`, what an NPC is where the draft lacks one or has
  it wrong (below). Any other table there is an error, as is an id the
  instance has no files for.
- **Its major events are the map's too.** At first a layer's `breaks` and
  every `[room]` stayed curated, as what the fight does. The same day the
  user ruled that a map's customizations for its major events belong in
  the base. So Ula'tek's layers break in the instance file on The
  Shattering and Circling Prey, events defined beside them, and Nek'zali's
  well and the Soulcoil Well place it is entered through moved there too.
  Ula'tek's heart window, its phases and every mark, ground and lane stay
  curated.
- **So are the NPCs in the room.** Ula'tek's Venomous Heart shares
  Ula'tek's health (Ula'tek's Bond) and is out only in 20 s windows. The
  journal lists no creature for it, so the draft holds none, and at first
  only the tuned file made it a boss that `shares` Ula'tek's health. On
  2026-10-08 the user ruled it the base's. So an instance file's
  `[encounter.<id>]` takes `npc` entries too (`INSTANCE_TABLES`), laid key
  by key over the draft's, adding an NPC or amending one. Who is in the
  room, and what each is, then reads the same at every tier. Mythic's
  spectral Ula'tek, which wears her name in the log, is one too, an
  `object` by its creature id; and The Shattering now counts from her
  second Rage of the Shackled, since Mythic's clock runs 10 s later.

A `[map]` may name a `stencil`, polygons and circles in world yards,
joined. `gen-floors` then renders the stencil's bounds alone and clears
everything outside the shapes, with a yard's fade, in the floor, its base
and every layer. It writes no minimap backdrop and removes an earlier one,
and its sidecar says `stencil = <shapes>`. The replay draws no render-edge
line where there is no backdrop, so its own dark ground shows past the
room.

## Consequences

- Entombed Sentinels' stencil keeps the platform, its venom gutter, the
  side alcoves with their pillars, and the steps to both corridors. The
  render went from 10082×10008 px to 1312×1232 (62 times fewer pixels) in
  8.9 s. Every one of the Mythic kill's 72,965 player positions lies inside
  it.
- Neither move changed anything curated: Sentinels', Nek'zali's and
  Ula'tek's resolved encounters are identical before and after. The base
  gained the rest. The replay at the base now opens Nek'zali's well on
  Mythic (27 stays on the kill) and breaks Ula'tek's platform at 5:53 and
  its quarters at 7:10, 8:01 and 9:02, where before it read both from the
  players' positions or not at all.
- Tables merge key by key, so a tuned file that made a layer break on a
  phase, where the instance says an event, would merge into a table with
  both and be refused with the key named. A test holds that.
- A stencil is written by hand, in world yards, traced with
  stencil: the render turned by its sidecar's
  `wall_angle` under a yard grid and the pulls' positions, the outline
  traced in those yards, converted back and checked against the positions.
- Every raid boss the replay spike has a pull for took one the same day:
  the rest of the Venomous Abyss, Nymrissa Wavecaller, five Voidspire
  bosses and Midnight Falls. Most keep a platform out to its rim, with its
  steps or stairs; a few needed circles too (corner pillars, pylons, an
  island, an open field ringed by bones). Those fourteen renders went from
  8,000–15,000 px a side (826 MB) to 1,000–2,400 (99 MB), all of them in
  71 s. Every one keeps its pull's positions, but for strays a few yards
  off an edge.
- A stencil is only as good as the outline traced. Ula'tek's ring of
  terraces stands at the same radius as the venom showing between them, so
  its octagon trims only the corridor and what lies past the ring. Finding
  the walls round the players from the geometry, so a stencil is suggested
  rather than traced, is left for later.
- The players' positions are not the room. Vashnik's first stencil kept
  the middle chamber the raid fights in, and two of the nine Malignant
  Tumors the log placed on 10/5 stood a few yards past it: they rise
  anywhere in the hall. On the user's word its stencil now takes in the
  whole hall, nave, corridors and galleries, and its `[view]` keeps to the
  stencil's bounds, opening turned 240° on the middle chamber.
- The map also carries the render's density (`ppy`, 16 for the four season
  instances) and the liquid the game colors itself (`glow`): both how the
  room is drawn, so both the base's.
- An instance's `npc` is hand-made base content that is not the map. Use
  it only for what extraction cannot tell, such as a creature the journal
  leaves out or a health pool two NPCs share. Where the census or the
  journal could tell it, gen-rubric should
  derive it into the draft instead. With the heart in the base, the replay
  frames it under Ula'tek at both tiers.

[^readme]: `crates/encounter-rubric/README.md`: Files (the instance file), How an encounter resolves (layer 4, Two tiers), and `[map]`'s `stencil`.
[^tiers]: Keep The Rubric's Base Apart From Its Curated Layer: the base stands alone; curated lays the tuned file over it.
