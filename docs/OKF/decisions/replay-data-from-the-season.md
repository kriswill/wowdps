---
type: Decision
title: Scope The Replay's Game Data To The Season The Client Names
description: 'The replay''s game-data caches cover the current season''s raids and dungeons only, resolved from the client''s own tables (the keystone pool and the journal''s "Current Season" tier) and checked against the user''s logs; encounter data is keyed by difficulty through the client''s fallback chain, rooms never are, and everything stays a per-machine cache regenerated on patch day.'
tags: [replay, game-data, design]
status: draft
generated: { by: claude-code/opus-5.5, at: 2026-10-04T13:30:00-07:00 }
sources:
  - id: assets
    resource: ../../replay-assets.md
    title: Fight replay — game data and assets (§1 sources, §2 season, §3 difficulties, §7 cache)
  - id: dbd
    resource: https://github.com/wowdev/WoWDBDefs
    title: WoWDBDefs — table definitions and manifest.json (table name → FileDataID)
  - id: adb
    resource: https://wowdev.wiki/ADB
    title: wowdev wiki — the client's hotfix cache (DBCache.bin)
---

**Where:** planned generators in [`wowdps-extract`](../crates/extract.md)
(an encounters manifest and floor maps), for
[A Fight Replay Captured Once And Interpreted Per Tier](fight-replay-captured-once.md).
The cache rule is the one [gen-talent-art](../tools/gen-talent-art.md) and
the icon bins already follow. This is a draft: the research settled the
rule against build 12.1.0.69933[^assets], and the spec confirms it.

## Context

A season is a content release. It has its own raids, its own keystone
pool, which mixes new dungeons with returning ones, and a raid that can
land mid-season in a patch. Extracting every instance the game has ever had
would bury the cache in art nobody replays.

The question was whether the client itself knows what is current. The
alternative was a list curated from wikis.

## Decision

**Resolve the season from the client's tables**[^assets]:

- **Keystone pool.** `MythicPlusSeasonTrackedMap` rows for the newest
  `DisplaySeason`. For Season 2 that is exactly its eight dungeons.
- **Raids and dungeons together.** The journal's "Current Season" tier
  (`JournalTier` with `Expansion = 9000`, found by that rather than by name
  or id) lists instances in groups, one `AvailabilityCondition` per season.
  Each condition is two time-event checks: the start has passed, the end
  has not. The start is the season's `MythicPlusSeason.StartTimeEvent`.
- **Filter the pseudo-entries.** The world-boss page and the "Keystone
  Dungeons" journal page are not instances.
- **Guard patch day.** The time events themselves are server state, in
  neither the tables nor the hotfix cache. So the resolved set is checked
  against the encounter and keystone ids in the user's recent logs, and a
  config override exists.

**Key encounter data by difficulty, never the room.** Journal sections
(`JournalSectionXDifficulty`), spell effects, durations and target
restrictions all carry a difficulty. The map tables carry none.

Lookups walk `Difficulty.FallbackDifficultyID`, as the server emulators do:

- Mythic Keystone → Mythic → Heroic → Normal
- raid Mythic → flex Mythic → Heroic → Normal

**Per-machine caches, current season only.** An encounters manifest, the
floors and the encounter art go under `~/.local/share/wowdps/`, never into
the repository, and a machine without them falls back to a yard grid. A
stored capture from an earlier season gets its floor generated on demand,
by the `ui_map_id` it carries. The install still holds every older map.

## Consequences

**Checked against the user's own store.** Every raid pull since August is
one of the nine bosses the tier names, and every key is one of the eight
dungeons.

**A mid-season raid arrives with its client patch.** It is a regenerate,
not a code change. Season 2's third raid is absent from 12.1.0 entirely: no
rows, and no encrypted section waiting for a key.

**The extractor still ignores the server's hotfix cache.**
`Cache/ADB/<locale>/DBCache.bin`[^adb] held about 440 journal-section and
2,000 spell-effect rows on the day. Season membership didn't need them, but
journal text and spell tuning drift there between builds. A generator
should overlay it.

**Fetch tables by FileDataID.** The root manifest of this build carries no
name hashes, so tables are fetched by FileDataID through WoWDBDefs'
manifest[^dbd].

Landed as research in `88e2f58`.

[^assets]: `docs/replay-assets.md` §1–§3 and §7, with the queries' results.
[^dbd]: WoWDBDefs' `manifest.json` maps each table name to its `db2FileDataID`.
[^adb]: The XFTH format: per entry a table hash, a record id and a status (valid, removed, invalidated, not public).
