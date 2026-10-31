---
type: Crate
title: wowdps-encounter-rubric
description: 'Deprecated 2026-10-30: the seasonal encounter rubric (per-encounter TOML, its layer stack and resolver, the base season files) left wowdps with the replay for the private wowdps-replay repository; wowdps keeps only the generated placed table, now proto''s.'
tags: [crate, replay]
status: deprecated
generated: { by: claude-code/opus-5.5, at: 2026-10-30T23:00:00-07:00 }
sources:
  - id: replay
    resource: https://github.com/kriswill/wowdps-replay
    title: wowdps-replay — the private repository the replay and the encounter rubric live in
---

The encounter rubric described each boss of a season beyond what the log
and the game's tables give: its room, its NPCs' roles, its abilities and
their shapes, its phases and what it leaves on the floor, as TOML files
under the crate's `seasons/`, embedded at build time and resolved per
difficulty through an ordered layer stack. The fight replay was its one
reader, so on 2026-10-30 it left wowdps with the replay, its base season
files, its curated set and its generator (gen-rubric) for the private
wowdps-replay repository[^replay], which takes wowdps's crates at a pinned
revision. Its record goes on there; the log entries here that name it are
its history.

The one thing a wowdps crate read from it, the generated table of what
players place in the room, stayed: it is
[wowdps-proto](proto.md)'s `placed` since the same day, read by the
daemon's replay cut ([R29](../rulings/r29.md)).

[^replay]: wowdps-replay — the private repository the replay and the encounter rubric live in
