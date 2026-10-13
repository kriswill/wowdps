---
type: Crate
title: wowdps-encounter-rubric
description: 'The seasonal encounter rubric: per-encounter TOML under the crate''s own seasons/ (the room''s map and view, NPC roles, abilities and shapes, phases and their triggers, floor features, what the fight leaves on the floor, the events that change the room and the game''s spell facts), embedded at build time and resolved here for one difficulty at one tier; main carries the generated base alone.'
resource: crates/encounter-rubric
tags: [crate, replay]
status: stable
generated: { by: claude-code/opus-5.5, at: 2026-10-12T19:45:00-07:00 }
sources:
  - id: readme
    resource: ../../../crates/encounter-rubric/README.md
    title: The rubric's schema reference, key by key
  - id: plan
    resource: ../../plan-fight-replay.md
    title: Fight replay plan — §3 layers 3–4, the curated definitions this crate holds
  - id: assets
    resource: ../../replay-assets.md
    title: Fight replay assets — §3, difficulties and the client's fallback chain
---

The seasonal knowledge the fight replay needs per boss, beyond what the log
and the game's tables give for free. It is the plan's "one definitions file
per tier" (layers 3–4)[^plan], written as TOML so it is reviewed like code,
for [A Fight Replay Captured Once And Interpreted Per Tier](../decisions/fight-replay-captured-once.md)
over [the season the client names](../decisions/replay-data-from-the-season.md).
It is named for the ENCOUNTER so it is never taken for a spec rubric (a
coaching rubric grades a player's spec; this one describes a boss).

Each encounter's draft is generated from the client's tables on the replay
branch, where the generator, the replay spike and the floor renderer live,
and committed here with its instance's and season's files. Main carries the
BASE alone: the hand-tuned files are the curated layer and stay out of this
repository by design. On main the daemon reads only the generated `placed`
table (its replay cut, [R29](../rulings/r29.md)); nothing reads the rubric
itself yet.

It is the one engine-side crate that names serde/toml (CONTRACT.md
§Dependencies), and it takes [`wowdps-proto`](proto.md) for `proto::dirs`,
the XDG directories its text sidecars and the user's rubric directory
resolve through. Beside the rubric it
holds the formats the replay branch's extractor writes and its GUI reads for
an encounter's art, one reader and writer each: `floor` (a rendered floor's
`.txt` placement sidecar, `Sidecar`, `Floors::scan` over the floors cache,
`floor::slug`; a dungeon level's picture says its instance's `map` and is
found by `Floors::levels`, and a level rendered from the world's geometry
says the same with a `minimap` line naming that picture, found by
`Floors::level_renders`) and `portraits` (the crops file and the portraits
cache's index); and `placed`, the GENERATED table of what players place in
the room (totems, gateways, rings, zones) and how the log tells where each
stands, the one part of the crate that is not per encounter.

## Source

- Manifest: [`crates/encounter-rubric/Cargo.toml`](../../../crates/encounter-rubric/Cargo.toml)
- Root: [`crates/encounter-rubric/src/lib.rs`](../../../crates/encounter-rubric/src/lib.rs)
- Schema: [`crates/encounter-rubric/src/schema.rs`](../../../crates/encounter-rubric/src/schema.rs),
  documented key by key in the crate's [`README.md`](../../../crates/encounter-rubric/README.md)[^readme]
- Seasons: [`crates/encounter-rubric/seasons/`](../../../crates/encounter-rubric/seasons/),
  a directory per season (`midnight-s1`, `midnight-s2`), each with its
  instances' directories

## Seams

- **Embedded, so a change is a rebuild.** `build.rs` walks the crate's
  own `seasons/` and writes an `include_str!` per `.toml` into `OUT_DIR`,
  paths relative to it (`<season>/<instance>/<file>`, the same paths a
  bundle and the user's directory use). The seasons live inside the crate
  so the flake's `./crates` source set carries them, and the crate is
  whole in itself.
- **Raw tables merge before anything is typed.** Sources merge as
  `toml::Table`s, key by key, in the stack's order: season defaults,
  instance defaults, draft, the instance file's `[encounter.<id>]` (the
  encounter's map and NPCs), each curated set, the user's files, then every
  source's difficulty overrides. Only the result is deserialized. So an
  override can name one key of an inline table (`shape = { length = 45 }`),
  and the typed structs never see a partial entry.
- **Three tiers, an ordered stack, the base standing alone.** Each
  `Source` names its `Tier` (`Base < Curated < User`, the default `User`):
  the base's four (`Season`, `Instance`, `Draft`, `Drawing`), then
  `Curated(Origin)` for each curated set in the order laid (a tuned file
  embedded beside its draft is `Curated(Embedded)`, a runtime
  `with_curated` bundle `Curated(Bundle(name))`), then `User`
  (`with_user`, `with_user_dir` over `$XDG_CONFIG_HOME/wowdps/rubric/`, a
  tree that mirrors `seasons/`). A
  set is one block (its `season.toml`, its `instance.toml`, the encounter's
  file) laid over everything under it; a draft is never in one. A bundle is
  laid whole or refused; a bad user file goes to `user_errors` and is left
  out. `encounter_at` takes the tier per encounter, `with_tier` caps it,
  `layers_of` lists an encounter's sources, and `encounter_traced` answers
  a `Provenance` (per key path, who last set it, through which difficulty
  override) with the same `Encounter`. The embedded test resolves and
  `check`s every encounter at every tier, so no base leans on a curated
  file ([Resolve An Encounter Through An Ordered Layer Stack](../decisions/encounter-rubric-layers.md)).
- **An encounter's map is the base's.** The instance file gives each
  encounter's `map` and `view` by DungeonEncounterID, however hand-made,
  and what changes the map in the fight: its `room`s, `place`s and the
  `event`s they and its layers key on. It also gives an `npc` the draft
  lacks or has wrong and the arena's own `travel` (`INSTANCE_TABLES`). Any
  other table there, or an id with no files, is an error. The base alone
  draws every room whole, with the same NPCs in it; a curated or user set
  may still amend an arena's map and view. `Map::stencil_bounds` and
  `stencil_depth` answer where a stencil lies, and `stencil_keep` how much
  of a point its cut keeps, faded over `STENCIL_FEATHER` (a yard), so the
  renderer and the replay cut a stencil's edge alike.
- **A dungeon's level is a map too.** An instance file's
  `[level.<ui_map>]` (`LEVEL_TABLES`: `map`, `view`, `travel`) resolves
  over the season's and the instance's `[defaults]` alone, never a
  difficulty or a set, into a `Level` holding the same `Map` an
  encounter's does (`Rubric::level`, `levels`, `check_level`). Any map,
  an arena's or a level's, may name its areas (with altitudes a reader
  fills with `Area::altitudes`), the paths between them (with a mode) and
  a `transition`, and the render's `crop`, `include_*`, `fade`, `ground`
  and `exposure`: the vocabulary a map editor writes ([Give The Map A Vocabulary Per Level And Per Area](../decisions/map-vocabulary-per-level-and-area.md)).
  Every season dungeon's terrain level says `ground = "walked"`, so its
  render is cut over the ground its parties walked (every stored keystone
  run's posts on it); a level of buildings says none, as its render reads
  no ground. A set may amend an arena's map, never a level's. `gate_map`
  gates every map's named entries; `fixtures/resolved-maps.txt` holds every
  encounter's map and view to what they resolved to before, and
  `fixtures/resolved-levels.txt` every level as it resolves now
  (`WOWDPS_BLESS=1` rewrites it).
- **A render says what it was made from.** Every sidecar a renderer writes
  ends with `params` and `made` (`Params`, `RenderOptions`; both parsed
  tolerant of their absence, as an older render has neither). `params` is
  an FNV-1a hash, walked over the values themselves (numbers
  little-endian, floats by their bits, a list by its length, an absent
  value apart), so no serializer's text and no bump of one moves it, of
  what of the resolved `[map]` changes the render's pixels, with the
  options that do written after it as words. `Map::params_hash`
  destructures `Map` and `Glow` whole, so a key added to either is a
  compile-time choice; the render keys `crop`, `include_*`, `fade` and
  `exposure` are walked only where a map says them, so a hash made before
  the renderer drew by them stands. `Encounter::floor_params` adds a
  room's cut (`Encounter::room_render`, the one way a room is read) and
  leaves the arena's `crop` out of a room's, as a room never reads it;
  `Map::level_params` hashes a level's render by the posts it was cut
  over and what of its level's map it draws by. `Floors::stale` compares
  a render's `params` with the hash the rubric gives now, so a reader
  re-renders only what an edit moved, and the base must give every map
  whole for that hash to mean the same everywhere.
- **Typos are errors.** Every struct is `deny_unknown_fields`, and errors
  carry the encounter's file. `check` (its own module, a method per
  section) reports names an encounter uses but does not have: an
  ability's NPC, a pet's owner, a trigger's phase or event, a layer's
  break; and an `only` naming no difficulty, which would keep its entry
  off every one.
- **Values inherit; presence does not.** Difficulty overrides apply along
  the client's fallback chain, the most general first. A Heroic override
  reaches Mythic and keystones, as the game's own data inherits[^assets].
  `difficulty::CHAINS` is the one table of the chains, and one step,
  `lay_difficulties`, lays a file's overrides. An entry's `only` list is
  exact, though: what Heroic alone has, Mythic lacks. Every named entry is
  `Gated`, and `resolve` names every field of `Encounter` and `Map`, so a
  kind of entry added to the schema does not build until it is gated.
- **The writer is held to the files.** `write` lists every key it writes
  in its `ORDER` (public, so an editor adding a key to an instance file
  puts it where the writer would), a test holding the order to exactly the
  keys of an encounter with every field set, and another writes every
  committed draft back byte for byte from what it reads as, with no game
  install.
- **Versioned twice.** Every file carries `schema = N`. `migrate` brings
  older files forward and refuses newer ones. `Rubric::version()` is the
  seasons plus an FNV-1a hash over every file laid (the base's, the
  curated sets', the user's), which an interpretation records so a stored
  fight regrades when the rubric changes; laying nothing leaves it.
- **Blizzard's words stay on the machine.** An ability's `text` is never in
  a committed file. The generator writes each encounter's journal text to
  a per-machine sidecar,
  `$XDG_DATA_HOME/wowdps/rubric-text/<season>/<instance>/<id>-<slug>.toml`,
  and `Rubric::with_texts(text::dir())` reads each once and lays it over
  the encounter whenever it is answered. Without the sidecar the text is
  unsaid; one that cannot be read is named by `text_errors`.
- **Newest season first.** A returning dungeon can sit in two seasons.
  `encounter(id, difficulty)` takes the season with the highest `order`,
  the client's own season number; two instances giving one id in one
  season, or in two seasons of one `order`, is an error naming both.

[^readme]: The rubric's schema reference.
[^plan]: `docs/plan-fight-replay.md` §3, "Layer 3 — phases" and "Layer 4 — curated verdicts".
[^assets]: `docs/replay-assets.md` §3, "Difficulties".
