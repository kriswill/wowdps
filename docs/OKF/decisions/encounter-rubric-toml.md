---
type: Decision
title: Keep The Encounter Rubric As Reviewed TOML
description: 'The replay''s per-season, per-encounter knowledge (room tweaks, view, NPC roles, abilities, phases, floor features, events) lives as TOML under crates/encounter-rubric/seasons/, a generated draft beside a small hand-tuned overlay, per-difficulty overrides resolved through the client''s fallback chain, embedded at build time and read through one crate allowed serde/toml.'
tags: [replay, design, dependencies]
status: stable
generated: { by: claude-code/opus-5.5, at: 2026-10-06T14:30:00-07:00 }
sources:
  - id: plan
    resource: ../../plan-fight-replay.md
    title: Fight replay plan — §3 layers 3–4, decision 5, Q13
  - id: assets
    resource: ../../replay-assets.md
    title: Fight replay assets — §3, difficulties
  - id: readme
    resource: ../../../crates/encounter-rubric/README.md
    title: The rubric's schema reference
---

**Where:** [wowdps-encounter-rubric](../crates/encounter-rubric.md), read by
[wowdps-extract](../crates/extract.md)'s `gen-floors` and the replay spike.

## Context

The spike's second round (43 pulls, every room rendered) kept running into
per-room facts that no generic rule settled. Zul'jan's floor level is one.
Another is a turn the user prefers, or an add that is really an enemy's
pet. And the plan already wanted per-boss phase rules and verdicts as
"data, not renderer code, one definitions file per tier" (Q13)[^plan].

The user asked for one seasonal rubric to hold all of it:

- map details, the default turn and zoom, geometry tweaks and liquid heights
- the enemies' abilities, NPC pets and icons
- puddle positions
- phases, and the events that begin them

It had to be tunable per encounter and per difficulty, and its schema had
to be able to change when a season does.

## Decision

The user decided four things (2026-10-05).

**TOML, read by the runtime.** The engine crates are stdlib-only, so the
policy gains one exception: a new crate, `wowdps-rubric` (now `wowdps-encounter-rubric`), may name serde
(derive) and toml. The daemon, the extractor and the GUI read the rubric
through it and never name serde/toml themselves (CONTRACT.md
§Dependencies).

**Committed and embedded.** The files live under `crates/encounter-rubric/seasons/` and are reviewed
in PRs. They are embedded at build time, so a release carries its season and
a change takes a rebuild. There is no per-machine override directory.

**A generated draft beside a hand-tuned overlay.**
- `tools/gen-rubric.sh` rewrites `<id>-<slug>.draft.toml` from the client's
  tables (and a census of the user's pulls).
- A person writes only `<id>-<slug>.toml` over it.
- A patch's regeneration never clobbers tuning, and a review shows only
  what a person decided.

**Overrides per difficulty, by name.** `[difficulty.mythic]` and the like
are laid in the order of the client's `Difficulty.FallbackDifficultyID`
chain, the most general first[^assets]. So a keystone reads Mythic's, then
Heroic's, then Normal's overrides, and its own last. An entry's `only`
list is exact, since the journal scopes abilities exactly.

**The rest follows the plan:**
- Seasons are directories, named by the client's own season (DisplaySeason
  and its season number, `order`).
- Files are versioned with `schema = N` and migrated forward on load.
- `Rubric::version()` is what an interpretation records, so stored fights
  regrade when the rubric changes (the plan's §4)[^plan].
- The schema is documented key by key in `crates/encounter-rubric/README.md`[^readme].

## Consequences

- A per-room fix becomes a few reviewed lines in a tuned file, not code.
- A new season is a new directory, a generator run, and tuning.
- A schema change is a migration in code plus a version bump.
- Blizzard's descriptions stay out of the repository: drafts carry names
  and ids only, like the generated spell tables.
  - Amended 2026-10-06: the journal's text for each ability is rendered
    into a per-machine sidecar (`$XDG_DATA_HOME/wowdps/rubric-text/`) that
    the replay reads; the drafts carry the spell facts (numbers) alone
    (gen-rubric).
- serde and toml are now linked into the extractor, and into the GUI's
  examples. The daemon takes them the day it interprets.
- The spike's first consumers: the view's turn, zoom and center; NPC roles
  over the extractor's guess; layers held standing by `breaks = "never"`.
  Phase and event triggers wait for the interpretation layer (the plan's
  Phase 5).

[^plan]: `docs/plan-fight-replay.md` §3 layers 3–4, decision 5 and Q13; §4 on interpretation and regrading.
[^assets]: `docs/replay-assets.md` §3, "Difficulties": the client's fallback chain.
[^readme]: `crates/encounter-rubric/README.md`, the schema's reference.
