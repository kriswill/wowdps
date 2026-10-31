---
type: Tool
title: seed-creatures
description: 'Seed the history store''s creatures.tsv from Wowhead''s NPC data (the season''s zone listings and a tooltip for each other NPC the stored replays hold), under the addon''s sightings, so the replay rings a rare silver before the addon has seen it.'
resource: tools/seed-creatures.sh
tags: [tool, replay, game-data]
status: stable
generated: { by: claude-code/opus-5.5, at: 2026-10-10T15:40:00-07:00 }
sources:
  - id: script
    resource: ../../../tools/seed-creatures.sh
    title: tools/seed-creatures.sh — the manual, in its header
  - id: code
    resource: ../../../tools/extract/src/wowhead.rs
    title: wowhead.rs — ZONES, the two readers, the fetcher, seed and lay_seed
  - id: codec
    resource: ../../../crates/proto/src/creatures.rs
    title: proto creatures.rs — the file's one codec and its merge
  - id: wowhead
    resource: https://www.wowhead.com/zone=14032
    title: Wowhead — a zone page (Algeth'ar Academy) and its NPC listing
---

`tools/seed-creatures.sh [--store DIR] [-o FILE] [--cache DIR] [--dry-run]`
builds [wowdps-extract](../crates/extract.md) and runs
`seed-creatures`[^script]. It fills the history store's `creatures.tsv`,
the file the replay spike's NPC rings read
(Ring Every NPC The Replay Sees As Its Nameplate Reads),
from Wowhead's NPC data before the wowdps addon has met a creature
([Creature Classifications Come From The Addon, Kept As One TSV](../decisions/creature-classes-come-from-the-addon.md)).
It is a dev step on one machine, rerun when the store gains a new
instance: nothing it fetches or writes enters the repository.

## What it asks about

- **Every hostile NPC the store's replays hold.** It reads the replay tier
  offline, as level-posts does, and takes each boss's
  and add's creature id. Friendly units are left out: a cut keeps no GUID
  for an NPC, so a pet (whose id is its beast's) cannot be told from a
  guardian or a friendly NPC.
- **Every NPC of the season's zone listings.** A zone's page
  (`https://www.wowhead.com/zone=<id>`)[^wowhead] carries an `npcs`
  Listview whose data is JSON, one object per NPC with `id`, `name` and a
  numeric `classification`. The search path `npcs/zone:<id>` ignores the
  zone and serves the site's 1,000 most popular NPCs, so the zone page is
  the one read. `ZONES` names the zones[^code].
- **A tooltip for the rest.** An NPC no listing classifies gets
  `https://nether.wowhead.com/tooltip/npc/<id>?dataEnv=1&locale=0`, JSON
  whose `tooltip` holds a type line ending in the rank in parentheses.

## Wowhead's words

Checked on 2026-10-10 by fetching the tooltip of NPCs the listing gives
each number:

| listing | tooltip | `creatures.tsv` |
|---|---|---|
| 0 | `(Normal)` | `normal` |
| 1 | `(Elite)` | `elite` |
| 2 | `(Rare Elite)` | `rareelite` |
| 3 | `(Boss)` | `worldboss` |
| 4 | `(Rare)` | `rare` |

Wowhead writes `(Normal)` for a normal NPC, so a tooltip with no rank (a
listing's `null`) is one Wowhead holds no rank for: it gives no row, and
the ring reads from the log alone. Nothing maps to `minus`, and Wowhead
has no lieutenant flag: those come from the addon only. A raid boss is
mostly `1` with a separate `boss` flag the seed does not read; the
spike's gold comes from the encounter.

## The zones

Wowhead's zone id is the client's AreaTable id of the instance map's top
area (`AreaTable.ContinentID` = the map, `ParentAreaID` = 0, through
[wowdps-db2](wowdps-db2.md)). The run checks each page's title names the
zone it expects.

| Map | Zone | Instance |
|---|---|---|
| 2526 | 14032 | Algeth'ar Academy |
| 2993 | 16865 | Altar of Fangs |
| 2825 | 16368 | Den of Nalorakk |
| 1762 | 9526 | Kings' Rest |
| 2811 | 15829 | Magisters' Terrace |
| 2874 | 16395 | Maisara Caverns |
| 2813 | 16091 | Murder Row |
| 2915 | 16573 | Nexus-Point Xenas |
| 658 | 4813 | Pit of Saron |
| 2521 | 14063 | Ruby Life Pools |
| 1753 | 8910 | The Seat of the Triumvirate |
| 1209 | 6988 | Skyreach |
| 1877 | 9527 | Temple of Sethraliss |
| 2859 | 16359 | The Blinding Vale |
| 2923 | 16425 | Voidscar Arena |
| 2805 | 15808 | Windrunner Spire |
| 2912 | 16340 | The Voidspire (raid) |
| 2913 | 16342 | Isle of Quel'Danas (raid: March on Quel'Danas) |
| 2987 | 16671 | The Tidebound Grotto (raid) |
| 3004 | 16915 | The Venomous Abyss (raid) |
| 1592 | 16279 | Sporefall (raid) |

The first sixteen are the dungeons whose levels are rendered under
`~/.local/share/wowdps/floors/`; the rest are the raids the store held
this season. A map the store holds outside the table (timewalking
dungeons and Ulduar, delves) is listed in the run's report, and its NPCs
come by tooltip.

## The file

Each seed row is `seen_unix` 0 and lieutenant 0. `lay_seed` takes out the
seed rows already there that the new seed covers, then
`creatures::merge`s the seed in[^codec]: an id the file lacks takes its
row, and a row the addon saw (`seen_unix` above 0) stays, since the merge
replaces a row only with one seen later. A rerun therefore refreshes its
own rows and never touches a sighting. The file is read first and refused
whole if its header names another format, then rendered and written
through a temporary file beside it, renamed into place.

The daemon folds the file on disk in before each merge of the addon's
records (the merge fix on main). A daemon built before that fix merges
into the copy it read when it opened the store, so after seeding under a
live one, restart the dev unit (asked first), or its next merge writes the
file without the seed.

## Politeness

At most two requests a second, with a User-Agent that names the tool and
its purpose, `--compressed`, and up to six tries on a 429, a 5xx or no
answer, pausing 2 s and doubling (Retry-After where given). Any other
status ends the run. Every answer is cached as it arrives,
`~/.cache/wowdps/wowhead/zone/<id>.html` and `npc/<id>.json` (a 404 as an
empty `.404` file), so a rerun asks only for what it lacks and an
interrupted run resumes.

## A run

On 2026-10-10 the store's 586 replays held 825 hostile NPC ids on 35 maps
(165 friendly ids left out), and the 21 listings named 1,188 NPCs. The
listings classified 1,147 NPCs and 302 tooltips the rest. 49 tooltips
carried no rank, 20 of them for store NPCs, so 805 of the 825 have a row.
None failed. The run took 157 s for 21 pages and 310 tooltips; a rerun
took 1 s and sent nothing. It wrote 1,449 rows: normal 445, elite 992,
rare elite 10, rare 2, world boss 0.

Of the rares, only the keystone affix's Xal'atath (236933 and 230937,
rare elite by Wowhead) is in the stored replays, in 24 keys. Its ring and
frame turn from a caster's purple to silver, and its words read "rare
elite". The rest come from the listings and wait for a pull to show them:
the Seat of the Triumvirate's five void lieutenants, Kings' Rest's
Weaponmaster Halu, and two rares Wowhead also lists in Maisara Caverns and
Kings' Rest (Lost Soul, Skarr).

[^script]: `tools/seed-creatures.sh`, its header.
[^code]: `tools/extract/src/wowhead.rs`: `ZONES`, `npcs`, `tooltip`, `Fetcher`, `seed`, `lay_seed`, and `wowhead/tests.rs`.
[^codec]: `crates/proto/src/creatures.rs`: `parse`, `merge`, `render`.
[^wowhead]: Wowhead's zone page for Algeth'ar Academy, its `npcs` Listview.
