---
type: Decision
title: Give The Map A Vocabulary Per Level And Per Area
description: 'The encounter rubric gains what a map editor will write: a dungeon level as an instance file''s [level.<ui_map>], read as the same Map an arena''s is, and inside any map the places the camera knows (areas, with their own altitudes), the ways between them (paths, with how a party goes them), what the replay does while the log is quiet (transition) and the render''s framing tweaks, all given by the base and a level by the base alone.'
tags: [replay, rubric, design]
status: stable
generated: { by: claude-code/opus-5.5, at: 2026-10-12T12:30:00-07:00 }
sources:
  - id: readme
    resource: ../../../crates/encounter-rubric/README.md
    title: The rubric's schema reference — A dungeon's levels, Areas, Paths, Transitions
  - id: snapshot
    resource: ../../../crates/encounter-rubric/fixtures/resolved-maps.txt
    title: Every encounter's map and view as origin/main resolved them before the vocabulary
---

**Where:** [wowdps-encounter-rubric](../crates/encounter-rubric.md)
(`Level`, `Rubric::level`, `levels`, `check_level`; `Map`'s `crop`,
`include_groups`, `include_models`, `fade`, `ground`, `exposure`, `area`,
`path`, `transition`; `Travel`), for
[A Fight Replay Captured Once And Interpreted Per Tier](fight-replay-captured-once.md),
laid as [Resolve An Encounter Through An Ordered Layer Stack](encounter-rubric-layers.md)
lays a map: in the base.

## Context

The replay draws every room by general rules: it guesses the floor's
level, its ceiling, how much of the world to frame, which groups and
models to leave out, and how far to zoom. Where a guess is wrong, an
encounter's `[map]` corrects it by hand (a level, a ceiling, a stencil).
A keystone run spends most of its time on a dungeon's levels, between
bosses, and a level had no entry in the rubric at all: nothing could
correct it. The replay tier already posts every unit on whichever floor
(UiMap) the log names ([R29](../rulings/r29.md)), so the run follows the
party from level to level; it is the drawing that knew nothing of them.

Two more gaps showed once a run is drawn whole. The camera's zoom was one
number per encounter, relative to the whole arena, where a level holds
courts of very different sizes. And the log goes quiet while a party is
carried: on Algeth'ar Academy's drake flights it moves 16–19 yd a second,
on Ruby Life Pools it is posted on no level of the dungeon for about 16 s
between two. Drawing a unit across those gaps needs to know the ways a
party goes and how fast.

A map editor (private, later) will write all of this per level and per
area. It needs a vocabulary to write, in the public crate, before it
exists.

## Decision

- **A level is a table of its own, keyed by UiMap.** An instance file's
  `[level.<ui_map>]` gives a level's `map`, the `view` while no area holds
  the party, and its `travel`[^readme]. The key is the UiMap because that
  is what every post names. It resolves as an encounter does, from fewer
  sources: the season's `[defaults]`, the instance's, then its own table,
  each read for `map`, `view` and `travel`. No difficulty applies to a
  level: the floor is the same on all of them. Its `map` is the same
  `Map` type an encounter's is, so a reader draws a raid's arena and a
  dungeon's level with one type, and an arena takes the same new keys
  inside `[encounter.<id>.map]`.
- **Areas carry altitudes.** An area is a place the camera knows (a
  boss's court, a hall, a landing): a stencil's shapes, its fit the view
  that shows them whole. Its `far`, `home` and `near` are multiples of
  that fit, `far ≤ home ≤ near`, so a 20-yd court and a 50-yd one each
  frame themselves. Today's numbers map onto them: the season's `zoom`
  1.15 is `home`, `zoom_max` 8 is `near`, the whole arena is `far` 1.
  `Area::altitudes(&view)` fills the unsaid ones so, and `check` holds the
  filled three above 0 and in order, so a reader's `clamp` never panics on
  a `near` written under the unsaid `far`. An area may turn the camera,
  draw its own ceiling, and name the boss fought in it (`encounter`); a
  map holds one court a boss.
- **Paths carry modes.** A path joins two areas of one floor and goes
  both ways. Its `mode` (`run`, `mount`, `vehicle`, `leap`) says how a
  party goes it, so a solver routing a quiet unit can time it. Run and
  mount have the season's speeds (`[defaults.travel]`, 7 and 14 yd a
  second, beside `[defaults.map]` and `[defaults.view]`, and an arena's
  or a level's own `travel` over them); a vehicle unsaid and a leap are
  timed to the next post, because their speed is not a player's. A path's
  own `speed` overrides; only a leap names a spell, its device's.
- **Transitions name what the replay does while the log is quiet.** A
  quiet unit (`after` seconds unposted) holds (today's behavior), fades to
  a ghost, or travels the path graph timed to its next post, drawn as a
  ghost. A jump past `port_yd` is a port, cut or flashed. The camera
  holds, glides or cuts as the party moves to another area.
- **The render's tweaks join the map.** `crop` frames a floor where the
  heuristic frames the wrong thing and a stencil is more than wanted (a
  box with `x0 < x1` and `y0 < y1`); `include_groups` and
  `include_models` keep what a rule would drop; `fade` draws a group or a
  model see-through, one fade a piece and never a piece left out;
  `ground` says what a terrain level's ceiling is measured from
  (`"terrain"`, `"walked"` or a height; the type is `Datum`, as `Ground`
  is what a fight leaves on the floor); `exposure` scales the graded look.
- **A level is the base's alone.** The base gives every map whole (an
  arena's in its draft and drawing, a level's in its own table), so a
  reader of the base sees every floor as it is meant. A level lives in
  the base's instance file alone: a curated or user set's instance file
  giving one is refused, and a set's `[defaults]` skip levels, though they
  reach every arena. Review asked whether sets should be refused an
  arena's `map`, `view` and `travel` as well. They are not: the layer
  stack lets a set amend anything ([Resolve An Encounter Through An
  Ordered Layer Stack](encounter-rubric-layers.md)), its tests lay a
  bundle's `[defaults.map]` and `[defaults.view]`, a user's
  `[defaults.view]` and tuned files' `[view]` and `[map.layer]` over
  arenas, and the README promises a user's `[defaults.view]` sets every
  encounter's view. So the rule was narrowed to what holds: sets may
  amend an arena's map and view, never a level's.

## Consequences

- Nothing that resolves today changes. Every encounter's map and view, on
  every difficulty at every tier, was written from origin/main before the
  change[^snapshot] (75 encounters, each alike on all 33 readings), and a
  test holds the new build to it, its new keys held apart: default, but
  for the areas an instance file has since given. Every level as it
  resolves is written out beside it (`fixtures/resolved-levels.txt`,
  rewritten with `WOWDPS_BLESS=1`), so a later `[defaults.travel]` or an
  edit to a level shows in review.
- The first levels are hand-written from this machine's stored keystone
  runs: two of Algeth'ar Academy's, three of Altar of Fangs', three of
  Ruby Life Pools'. A boss's court is a circle round the middle of the
  players' posts while it was engaged, its radius about their 98th
  percentile distance; a path is the party's middle every 10 s between
  two bosses, thinned by hand, its mode read from its pace. The other
  places, their radii and their names are guesses for the editor to
  refine. Vashnik's hall is the worked raid example: one area, its shape
  the stencil's and its altitudes the view's.
- A court's encounter is posted on its level: the embedded test holds its
  map's `ui_map` to the level's key, with the exceptions it names. Echo of
  Doragosa is one: its draft says UiMap 2099 (Algeth'ar's level 3, whose
  box holds the court too), but every player post of the fight on both
  stored runs is on 2097.
- Algeth'ar Academy is in Midnight Season 1's pool on main, so its levels
  live in that season's instance file; a returning dungeon's newest
  season gives its levels, as it gives its encounters.
- Nothing on main reads a level yet: the renderer and the editor are the
  replay branch's. The vocabulary is the contract they meet at.
- Not done here: a user or curated set cannot amend a level yet; levels
  have no provenance trace; `check_level` cannot see whether an area's
  `encounter` is its instance's or posted on its level (the embedded test
  holds both).
- The every-room pass used it on every room of both seasons and found
  where it falls short ("Walk Every Room Once And Say Only What The
  Rubric Can", the replay's record): the resolved-maps test now
  holds an encounter's paths and crop apart with its areas, and an
  instance file's later edit to a key the snapshot had (Melidrussa
  Chillworn's stencil) is written into its line.
- The vocabulary landed in `a47e2d9` and the first levels in `f6083a3`;
  the review's fixes (altitudes filled before they are checked, an
  ordered crop, the fade, court and spell checks, an arena's `travel`,
  the levels fixture and this narrower base rule) in the commit after.

[^readme]: `crates/encounter-rubric/README.md`: A dungeon's levels, Areas, Paths, Transitions.
[^snapshot]: `crates/encounter-rubric/fixtures/resolved-maps.txt`, held by `every_map_and_view_resolves_as_before_the_map_vocabulary`.

**Update (2026-10-30): an area cuts and bounds the render.** An area's
`ceiling`, written since the vocabulary landed and read by nothing, is
read over its shapes (the lowest where areas overlap); an area gains a
`stencil`, which with its level's paths' corridors (a path's `width`,
else 8 yd) bounds a level's render; and a map and an area gain a `hang`,
leaving out what hangs over the walk. An area's `shape` still frames the
camera alone. No schema bump, no pinned hash moved ("Cut And Bound A Level's Render
Area By Area", the replay's record).
