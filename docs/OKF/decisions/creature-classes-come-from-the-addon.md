---
type: Decision
title: Creature Classifications Come From The Addon, Kept As One TSV
description: 'The combat log never says whether an NPC is elite, rare or a lieutenant and the client''s Creature.db2 ships only a thin slice, so the wowdps addon records UnitClassification and UnitIsLieutenant by creature id from nameplates, and the daemon merges them into creatures.tsv at the history store''s root, a small contracted file the replay reads whole to color its rings; it lags a logout like the guilds.'
tags: [daemon, history, proto, addon, replay]
status: stable
generated: { by: claude-code/opus-5.5, at: 2026-10-10T12:00:00-07:00 }
sources:
  - id: contract
    resource: ../../../CONTRACT.md
    title: CONTRACT.md — the addon paragraph, "Creature classifications"
  - id: spec
    resource: ../../spec-history-store.md
    title: History store spec §9b
---

**Where:** `addon/wowdps.lua` (the `creatures` section),
[`wowdps-proto`](../crates/proto.md) (`creatures`: the TSV codec, the
addon-table reader, the merge), [`wowdps-daemon`](../crates/daemon.md)
(`history.rs`: `Store::merge_creatures` from the SavedVariables poll),
[`wowdps-history`](../crates/history.md) (`stats.creatures`), the contract's
addon paragraph[^contract] and spec §9b[^spec].

## Context

The replay draws every NPC as a ring and wanted to mark them the way the
game's nameplates do: silver for a lieutenant, the elite and rare marks.
The combat log carries none of it: an NPC is a GUID, a name, flags and hit
points. The client's `Creature.db2` has a classification column but ships
a thin slice of the creatures, and none of a season's dungeon trash. The
only source that answers for any NPC the player meets is the game API:
`UnitClassification` (`normal`, `elite`, `rareelite`, `rare`, `worldboss`,
`minus`) and, since 12.x, `UnitIsLieutenant`. The wowdps addon already runs
in the game for the guilds ([Guilds Come From An Addon](guilds-come-from-an-addon.md)),
and SavedVariables are its only way out.

## Decision

- **The addon records by creature id.** On `NAME_PLATE_UNIT_ADDED`,
  `UNIT_CLASSIFICATION_CHANGED` and a target change it takes a unit whose
  GUID is a `Creature-` or `Vehicle-` one and that no player controls (no
  player, pet or guardian), skips one wearing a group member's name (a
  mirror clone, so the file never holds a player's name), reads the
  creature id from the GUID's sixth
  field (the log's join key), and keeps the newest sighting:
  classification, lieutenant (0/1), `seen`, name, client build, difficulty
  id, creature type and power type. It records inside a dungeon, raid or
  scenario, and a world boss anywhere, since questing would only fill the
  table with mobs no replay shows; ids unseen for 60 days are pruned on
  load. `UnitIsLieutenant` is guarded for older clients, a 12.x secret
  value is never stored, and the recorder runs under `pcall` so nothing it
  meets mid-pull puts an error on the player's screen.
- **The daemon keeps one TSV at the store's root.** The history thread reads
  the section with the players, on start and on the 30 s idle poll, and
  merges it into `creatures.tsv`: `# wowdps creatures 1`, then
  `creature_id, classification, lieutenant, seen_unix, name`,
  tab-separated, ids ascending, rewritten whole through `write_atomic`. The
  newest `seen` per id wins and a row the SavedVariables no longer hold
  stays, because an in-game prune refutes nothing. A TSV and not one JSON
  document per creature like `affiliations/`: its reader is the replay,
  which loads one small file whole and never scans a directory, and a
  diff of it reads at a glance.
- **The format is a contract.** CONTRACT.md fixes the header, the columns,
  the six values and the reader rules (refuse another or a newer format,
  skip `#` lines and unreadable rows, ignore columns past the fifth);
  `proto::creatures` is the one codec and `tests/creatures.rs` pins its
  bytes. The daemon never rewrites a file the codec refuses.
- **No wire change.** `HistoryStatus` stays as it is; `wowdps history
  stats` reports the file (rows, lieutenants, newest sighting, rows by
  classification) and `wowdps addon` counts each account's creatures.

## Consequences

The file lags a logout: the game flushes SavedVariables on logout,
`/reload` or exit, so a new NPC's color appears after the next logout and
the daemon's next poll, never during the night it was met. The replay
treats the file as optional, so an install without the addon replays as
before. A creature id's classification can differ by difficulty; the file
keeps the latest sighting alone, while the addon's record also notes the
difficulty it was seen at. Changing the addon makes every installed copy
stale, so the daemon rewrites it on its next start, and the in-game table's
`SCHEMA` moved to 2.

[^contract]: CONTRACT.md — the addon paragraph, "Creature classifications"
[^spec]: History store spec §9b
