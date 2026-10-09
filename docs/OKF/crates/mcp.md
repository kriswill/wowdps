---
type: Crate
title: wowdps-mcp
description: 'wowdps-mcp binary: MCP server exposing fight data to LLM harnesses.'
resource: crates/mcp
tags: [crate]
status: stable
generated: { by: okflight/0.4.0, at: 2026-09-06T18:29:58-07:00 }
---

`wowdps-mcp`: an MCP (Model Context Protocol) server over stdio, exposing the daemon's fight data as tools an LLM harness can call — a third frontend beside the TUI and GUI, and exactly as thin: model + proto only, snapshots in, JSON out. `wowdps mcp` reaches it through the dispatcher's external-command lookup.

## Tools

`tools.rs` declares them and `grade.rs` grades stored fights. JSON is
hand-rolled (`proto::json`); the crate is model + proto only, so it cannot
parse a log. The repository's `.mcp.json` registers it for Claude Code
through `cargo run`.

- `status`, `list_fights`, `compare`.
- `fight` — with `raid` ([R25](../rulings/r25.md)'s raid rate per 10 s, the
  deaths in order and the lust); rows carry `mine`. The fight object carries
  `combat` / `combat_ms`, the clock `per_sec` divides by, and a key's rows
  add `run_per_sec` over the key timer (`stored_fight` too). Healing rows
  carry `heal_absorbed`.
- `breakdown` — a recap event's `offset_secs`; damage and healing rows carry
  [R26](../rulings/r26.md)'s `group`, `casts` / `avg_cast` and `parts`, with
  `ability_groups` beside them (`stored_fight` too); an empowered spell's
  row adds `empower` (`stages` [1..4], `cancelled`, `stage_avg`) and a
  healing ability its `heal_absorbed`.
- `loadout` — a player's logged COMBATANT_INFO talents and gear, talents
  named through the dataset, the line's 22 stats by name and its pre-pull
  auras (flask, food, rune, raid buffs); gems are ids, one per gem.
- The history store's fixed questions, answered from the daemon's card
  index: `history`, `progression`, `trend`, `stored_fight`, `pin_fight`,
  `regrade_fights` (rewrites cards from their logs, pins kept) and
  `role_night`.
  - `history` carries the owner's grade as `me`, role-relative: a healer
    ranks among healers by HPS, a DPS among DPS by `effective_dps`
    ([R19](../rulings/r19.md): damage − received + given, one label) while
    the legacy `rank_dps` / `dps_*` block keeps raw dps (the block an
    Augmentation's buffs inflate); tanks are unranked but carry `taken` /
    `mitigated` / `prevented` / `mitigated_pct` / `dtps` and a `tank_pair`.
    `history { role }` filters by the SUBJECT's role.
  - Rows carry the healing split and the support scalars, `am_uptime_pct`
    ([R18](../rulings/r18.md), the stored `am_uptime_ms` over the card's
    duration) and `externals_given` / `externals_received` as
    `{count, secs}`; `tank_pair` carries `am_uptime_pct`; a healer subject
    gets a `healers` block; healer rows carry `absorb_wasted` (null =
    unknown), `shields_unknown` and `absorb_efficiency_pct`
    ([R20](../rulings/r20.md)).
  - `trend { measure: dps | hps | dtps | mitigated_pct | am_uptime |
    absorb_efficiency }` defaults by role (every DPS subject to
    `effective_dps`) and names its value field by measure (`per_sec` kept
    as an alias for the coach); `absorb_efficiency` skips unknown points.
  - `view: "taken"` on `fight` / `breakdown` / `stored_fight` adds a
    `mitigation` object to the drill. A stored by-ability list is capped at
    16 with the rest rolled up; a stored Taken drill carries the 10 s coarse
    timeline with marks (the Healing drill keeps the details tier's 1 s
    series on kills, `heal10` otherwise).
  - `stored_fight` reuses `fight` / `breakdown`'s row shapes, takes `boss`
    for a key's member (parsed from the log on demand) and `from_secs` /
    `to_secs` for a player's damage or healing drill over a zoom window
    (answered from the series tier). With `player` it returns a supporter's
    `support` block with targets, `uptime[]` — BOTH halves, the cells where
    the player is the target and those on other targets where the player is
    the caster, so "externals given, to whom" is one call — `shields[]` per
    shield spell, `energize` ([R27](../rulings/r27.md)) and `power`
    ([R28](../rulings/r28.md)).
  - `role_night { encounter, difficulty, night | date }` — one night's
    roster by role: tanks side by side, healers and DPS ranked.
- Talent tools — `talent_tree`, `decode_talents`, `encode_talents` —
  answered from the per-machine talent dataset ([R14](../rulings/r14.md)),
  never the daemon.
- `history_sql` shells out to `wowdps-history` and is registered only where
  that binary exists.

## Source

- Manifest: [`crates/mcp/Cargo.toml`](../../../crates/mcp/Cargo.toml)
- Root: [`crates/mcp/src/lib.rs`](../../../crates/mcp/src/lib.rs)

## Contract

Public signatures and dependency policy: [`CONTRACT.md`](../../../CONTRACT.md).
