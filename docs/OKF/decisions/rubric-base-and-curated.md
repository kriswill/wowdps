---
type: Decision
title: Keep The Rubric's Base Apart From Its Curated Layer
description: 'Superseded by Resolve An Encounter Through An Ordered Layer Stack (base, curated, user, with provenance). Was: every encounter resolves at a tier: the base is the generated draft over the season and instance files, with no hand amendment; curated lays the tuned file over it. The base must stand alone, so the curated layer can ship apart from it, and the replay can show either.'
tags: [replay, rubric]
status: deprecated
generated: { by: claude-code/opus-5.5, at: 2026-10-06T19:30:00-07:00 }
sources:
  - id: readme
    resource: ../../../crates/encounter-rubric/README.md
    title: The rubric's schema reference — How an encounter resolves, Two tiers
  - id: toml
    resource: encounter-rubric-toml.md
    title: Keep The Encounter Rubric As Reviewed TOML — the draft and the tuned file
---

**Where:** [wowdps-encounter-rubric](../crates/encounter-rubric.md) (`Tier`,
`Rubric::encounter_at`, `with_tier`, `has_curated`), read by the replay
(its own repository).

**Superseded** on 2026-10-09 by
[Resolve An Encounter Through An Ordered Layer Stack](encounter-rubric-layers.md):
the two tiers became three (base, curated, user), the tuned file one
curated source among bundles laid in at runtime, and the crate
`wowdps-encounter-rubric` with its seasons inside it. The base standing
alone, the rule this record made, holds unchanged.

## Context

An encounter's rubric has two kinds of content[^toml]:
- **The draft:** what gen-rubric extracts from
  the client's tables.
- **The tuned file:** what a person found by investigating the fight and
  checking it against logs and the game, such as Nek'zali's well as a room
  off the arena, the Latent Cultists' orbit, the Restless Amani's reach,
  and fourteen mechanic lanes where the draft gives one.

They were laid together into one answer, so nothing could show, test or
ship the extracted rubric without the hand-made one.

## Decision

An encounter resolves at a `Tier`[^readme]:
- **`Base`:** the season's and the instance's files and the draft,
  including the draft's own difficulty overrides. Nothing written by hand
  for that encounter but its map, and what an NPC is where the draft
  lacks one or has it wrong, both of which the instance file gives
  ([Keep An Encounter's Map In The Base, Cut To A Stencil](encounter-map-in-the-base.md)).
- **`Curated`** (the default): the tuned file and its overrides laid over
  the base.

The rules that keep the tiers apart:
- **A reader picks the tier per encounter:** `encounter_at(id,
  difficulty, tier)`. `encounter` reads at the rubric's own tier.
- **The rubric can be capped:** `with_tier` sets the most it will answer,
  and a higher tier asked for resolves at the cap. `has_curated` says
  whether an encounter has a tuned file at all.
- **The base never leans on a tuned file.** The embedded rubric's test
  resolves and `check`s every encounter on every difficulty at both tiers.
  A tuned file with no draft under it has no base.

The files stayed side by side under the repository's `rubric/` (now the
crate's `seasons/`). The spike shows either tier: `a`, the panel's Rubric switch, `--rubric base`, or the
`rubric` control command. A switch reloads the pull and keeps the
playhead, the play, the camera, the followed player and the mechanics
taken off the map.

## Consequences

- The curated layer can later be built, embedded or distributed on its
  own terms without touching the base, since the base already stands
  alone.
- The difference is visible. On Nek'zali the base has one mechanics lane
  (the kick lane its journal marks give), no room inset, and the Drowned
  Echo drawn at the well on the arena. Curated has fourteen lanes, the
  well's inset and its divers.
- `Rubric::version()` still hashes every file, tuned ones included. A
  reader that records it should record the tier it read at too.
- gen-floors reads at the curated tier, because
  floors are rendered once per machine, not per tier. Since 2026-10-06 the
  `[map]` and the `[room]`s it reads are the base's, so no render depends
  on the curated layer.

[^readme]: `crates/encounter-rubric/README.md`, How an encounter resolves.
[^toml]: Keep The Encounter Rubric As Reviewed TOML: the draft and the tuned file.
