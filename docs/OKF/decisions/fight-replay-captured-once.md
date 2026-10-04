---
type: Decision
title: A Fight Replay Captured Once And Interpreted Per Tier
description: 'Roadmap item 2a: the pull view gains a replay mode built on the positions every advanced log line already carries — captured in the background once per encounter pull and kept for good, with mechanics, phases and verdicts re-interpreted from that capture whenever a tier''s definitions change, local only and behind a sticky off switch.'
tags: [replay, history, design]
status: stable
generated: { by: claude-code/opus-5.5, at: 2026-10-04T13:30:00-07:00 }
sources:
  - id: plan
    resource: ../../plan-fight-replay.md
    title: Fight replay — research plan (decisions 1–8, the five layers, Q1–Q16)
  - id: roadmap
    resource: ../../roadmap.md
    title: Roadmap item 2a and the ground rules' scoped exception
  - id: format
    resource: ../../../crates/core/fixtures/FORMAT-NOTES.md
    title: FORMAT-NOTES — the advanced block's 19 fields
---

**Where:** nothing is built yet. This is a research plan[^plan], so the
code homes below are planned, not existing:

- **The parser** ([`wowdps-core`](../crates/core.md)) gains a position hint,
  the sibling of `hp_hint`.
- **The capture** is a history-store tier in
  [`wowdps-daemon`](../crates/daemon.md), encoded like
  [the series tier](a-series-tier-for-stored-windows.md).
- **The drawing** goes in [`wowdps-gui`](../crates/gui.md) over
  [`wowdps-gui-logic`](../crates/gui-logic.md)'s geometry.

The game data it draws on is
[Scope The Replay's Game Data To The Season The Client Names](replay-data-from-the-season.md)
and [Draw The Replay's Room From The Game's Own Floors](replay-floors-from-game-files.md).
The room's orientation is
[The Replay's Room Turns To The User, Without Edges](replay-room-turns-without-edges.md).

## Context

Every advanced log line already carries a unit's position, facing and map,
in fields 14–17 of the advanced block[^format]. The parser drops them.

On one real Heroic kill the samples came about sixteen a second per player,
with a p99 gap under half a second, so straight-line interpolation is
enough.

The usual way to add mechanics to a replay is an analyzer per boss, with a
hand-drawn floor. That costs the same work again for every boss of every
tier.

## Decision

The user's decisions[^plan]:

- **A mode of the pull view.** The rail, the fight header and the ribbon
  stay; the ribbon becomes the scrubber.
- **Captured in the background, live included.** A capture pass runs over
  the pull's byte range on the loader pool after the encounter closes, so
  the live meter never parses a position.
- **A sticky off switch.** With it off, the daemon does no replay work.
- **Captured once, kept for good.** The history store's
  [retention](stored-pulls-kept-whole.md) never evicts a capture. This is
  the roadmap's one scoped exception to storing summaries only[^roadmap].
- **Curated for the current tier only.**
- **Local only.** No Warcraft Logs key is asked of anyone. Importing a
  Warcraft Logs fight waits, but the capture format is designed so that
  import can land in it.
- **The room turns to the user's picture of it** (decision 8).

**The split that makes "never re-process" and "regrade" compatible.**

- **The capture** is everything from the log: tracks, plus the hostile
  event stream with each victim's exact position. It is the expensive half,
  written once.
- **The interpretation** is mechanics, phases and verdicts. It is computed
  from the capture, the tier's definitions and the game-data caches, cached
  by (fight, definitions version), and never stored as truth.
- **What a new tier costs.** New definitions re-interpret every stored
  fight without its log. Only a capture that is missing something needs the
  log again.

## Consequences

**Five layers carry the value, from generic to curated:**

1. the log alone
2. the game's tables
3. inferred mechanics, a rule per mechanic kind
4. phases, rules per boss
5. verdicts, per boss for the current tier

Each layer ships on its own. Every curated item costs work again each tier,
so the plan's central research question is how much the generic layers
carry.

**Risks.**

- **Size.** Permanence makes the capture the store's largest tier. Q11
  sizes a season before any format is fixed.
- **A capture too lean** forces a re-capture from logs the user may have
  deleted.

Planned in `b19496c`; the game-data research and decision 8 landed in
`88e2f58`.

[^plan]: `docs/plan-fight-replay.md`, "Decisions (2026-10-04)", §3 (the layers) and §4 (capture and interpretation).
[^roadmap]: `docs/roadmap.md`, the ground rules and item 2a.
[^format]: `crates/core/fixtures/FORMAT-NOTES.md`, "Advanced block".
