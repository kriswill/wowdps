---
type: Crate
title: wowdps-encounter-rubric
description: 'The seasonal encounter rubric: per-encounter TOML under the crate''s own seasons/ (the room''s map and view, NPC roles, abilities and shapes, phases and their triggers, floor features, what the fight leaves on the floor, the events that change the room and the game''s spell facts), embedded at build time and resolved here for one difficulty at one tier; main carries the generated base alone.'
resource: crates/encounter-rubric
tags: [crate, replay]
status: stable
generated: { by: claude-code/opus-5.5, at: 2026-10-09T11:25:00-07:00 }
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
repository by design. Nothing on main reads the crate yet.

It is the one engine-side crate that names serde/toml (CONTRACT.md
§Dependencies), and it takes [`wowdps-proto`](proto.md) for `proto::dirs`,
the XDG directories its text sidecars and the user's rubric directory
resolve through. Beside the rubric it
holds the formats the replay branch's extractor writes and its GUI reads for
an encounter's art, one reader and writer each: `floor` (a rendered floor's
placement sidecar, `Floors::scan` over the floors cache) and `portraits`
(the crops file and the portraits cache's index); and `placed`, the
GENERATED table of what players place in the room (totems, gateways, rings,
zones) and how the log tells where each stands, the one part of the crate
that is not per encounter.

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
  (`with_user`, `with_user_dir` over `$XDG_CONFIG_HOME/wowdps/rubric/`). A
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
  lacks or has wrong (`INSTANCE_TABLES`). Any other table there, or an id
  with no files, is an error. A room should look and change the same, with
  the same NPCs in it, at every tier.
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
  in its `ORDER`, a test holding the order to exactly the keys of an
  encounter with every field set, and another writes every committed draft
  back byte for byte from what it reads as, with no game install.
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
