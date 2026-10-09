---
type: Decision
title: Resolve An Encounter Through An Ordered Layer Stack
description: 'The encounter rubric resolves each encounter through an ordered stack of sources in three tiers (base, curated, user): the base''s four files, every curated set in the order laid (a tuned file embedded beside its draft, then each runtime bundle), then the user''s own files, with a provenance per key path; main carries the base alone, so the curated layer can ship apart and a user can override anything on their own machine.'
tags: [replay, rubric, design]
status: stable
generated: { by: claude-code/opus-5.5, at: 2026-10-09T12:30:00-07:00 }
sources:
  - id: readme
    resource: ../../../rubric/README.md
    title: The rubric's schema reference — How an encounter resolves, Three tiers
  - id: plan
    resource: ../../plan-fight-replay.md
    title: Fight replay plan — §3 layers 3–4, the curated definitions per tier
---

**Where:** [wowdps-encounter-rubric](../crates/encounter-rubric.md)
(`Tier`, `Source`, `Origin`, `Provenance`, `Rubric::with_curated`,
`with_user`, `with_user_dir`, `encounter_traced`, `layers_of`), for
[A Fight Replay Captured Once And Interpreted Per Tier](fight-replay-captured-once.md).

## Context

The rubric is the plan's curated definitions per tier[^plan]. It grew on
the replay branch with two tiers: the BASE (the season and instance files
and the generated draft, the encounter's map and NPCs among them) and
CURATED (a hand-tuned file laid over it). Both lived side by
side in one embedded tree, and the resolver knew of exactly one tuned file
per encounter. Bringing the crate to the public repository raised three
needs that shape did not meet:

- **The base ships alone.** The tuned files are the work that makes the
  replay say what the fight does; they stay out of the public repository
  and must be able to arrive later as a separate set, without the base
  depending on them.
- **More than one curated set.** A set laid in at runtime (a bundle) has to
  stack over an embedded tuned file, and two bundles over each other, in a
  defined order.
- **The user's own say.** Someone tuning their own replay needs to override
  any key on their machine without touching the repository, and a mistake
  in their files must not take the rubric down.

A reader also had no way to ask why an encounter says what it says: which
file set a given key.

## Decision

An encounter resolves through an ordered stack of `Source`s[^readme]:
`Season`, `Instance`, `Draft`, `Drawing` (the base's four), then
`Curated(Origin)` for each curated set in the order laid (`Embedded`, a
tuned file beside its draft, first; then each `Bundle(name)`), then `User`.
Each names its `Tier` (`Base < Curated < User`); a reader asks for a tier
per encounter and `with_tier` caps the rubric, whose default is now `User`,
everything the machine has.

- **A set is one block.** A curated or user set is a file tree shaped like
  `rubric/`: its `season.toml` (`[defaults]` alone), its `instance.toml`
  (`[defaults]` and `[encounter.<id>]` drawings) and `<id>-<slug>.toml`,
  laid in that order and as a whole over everything under it. Every season
  and instance it names must be the base's; a draft is never in one.
- **Difficulty overrides stay last.** Every source's `[difficulty.<name>]`
  overrides are kept apart and laid after all of the stack's own values,
  the most general difficulty first and each difficulty's in stack order,
  which is the order the two-tier resolver already used. A tuned file
  embedded on the replay branch therefore answers exactly as before.
- **Two ways in for curated.** An embedded tuned file classifies as
  `Curated(Embedded)` as before; `with_curated(origin, files)` lays a
  bundle in at runtime, whole or not at all (a file it cannot lay, or an
  encounter it makes unreadable where the tiers under it read, refuses it).
- **The user layer never fails the rubric.** `with_user` lays files one by
  one; `with_user_dir` reads `$XDG_CONFIG_HOME/wowdps/rubric/` through
  `proto::dirs`. A file that cannot be read, parsed or laid, or that would
  make an encounter unreadable on any difficulty, goes to `user_errors` and
  is left out, as `with_texts` treats a bad sidecar.
- **Provenance per key path.** The one merge rule (tables key by key,
  anything else whole) runs with an optional `Provenance` beside it: per
  key path (`ability.serpents-bite.shape.length`, `map.stencil`), the
  source that last set it and the difficulty override it came through. A
  list or scalar laid over a subtree forgets the subtree's paths.
  `encounter_traced` answers it with the same `Encounter` `encounter_at`
  gives.
- **`layers_of` replaces the boolean.** It lists the sources with a file an
  encounter reads; `has_curated` and `has_user` read it.

## Consequences

- The public repository holds the generated base and every hand-made map
  and NPC amendment, and nothing of the curated layer; the embedded test
  resolves and checks every encounter at every tier, which on main is the
  base standing alone.
- A curated set can be built and distributed on its own terms (the product
  plan's later step fills `Origin::Bundle`) without touching the base.
- `Rubric::version()` folds every laid file into its hash, the curated sets'
  and the user's too, so a stored interpretation regrades when any of them
  changes; laying nothing leaves it as it was. A reader that records it
  should record the tier it read at as well.
- Validating a user file or a bundle resolves each encounter it touches on
  every difficulty, twice at most; a `season.toml` touches a whole season.
  That is milliseconds at load, never per pull.
- Not done here: no vocabulary gate for what a curated or user layer may
  name (an entry the base lacks is simply added), deferred by choice.
  Nothing on main reads the crate yet; the replay branch does, through
  the old name until it merges this.

[^readme]: `rubric/README.md`, How an encounter resolves.
[^plan]: `docs/plan-fight-replay.md` §3, the definitions per tier.
