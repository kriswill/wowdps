# Fight replay — game data and assets

Research for `docs/plan-fight-replay.md` (Q4, Q5, Q10, part of Q6, and the
season question the plan left implicit): where the replay's rooms, art and
encounter data live in the game files, how to tell which of it belongs to
the current season, and what a season-scoped cache costs. Every finding
below was checked against the live client, build **12.1.0.69933**
(Midnight Season 2), on 2026-10-04, with `tools/extract` reading the
install's own CASC storage and DuckDB joining the exported tables. Nothing
here is built yet.

## 1. Where the data lives

| Source | What it holds for the replay | Notes |
| --- | --- | --- |
| **CASC storage** (`Data/`) | Every DB2 table and every texture (BLP), by FileDataID | `tools/extract` reads both already. This build's root manifest carries no name hashes, so tables are fetched by FileDataID; WoWDBDefs' `manifest.json` maps table name → `db2FileDataID`. |
| **Hotfix cache** (`_retail_/Cache/ADB/<locale>/DBCache.bin`) | Rows the server pushed after the client build: XFTH v9, one entry per (table hash, record id) with a status (1 valid, 2 removed, 3 invalidated, 4 not public) | Not read by the extractor today. On this machine (written today, 2.8 MB) it touches 283 tables, among them `JournalEncounterSection` (440 rows, 6 removals), `EncounterEvent` (16), `JournalTierXInstance` (12), `MythicPlusSeasonTrackedMap` (2), `TimeEventData` (53), `SpellEffect` (~2,000). For Season 2's membership the CASC tables already held the same rows (checked: `JournalTierXInstance` 1552–1612, `MythicPlusSeasonTrackedMap` 285/287); journal text and spell tuning are what drift between builds. |
| **The combat log** | `ENCOUNTER_START` (encounter id, difficulty, group size, instance map id), `CHALLENGE_MODE_START` (map id), and each advanced line's `ui_map_id` | The `ui_map_id` names the floor directly (2610 on every line of the Coiled Altar slice). |
| **Not in the client** | Season start/end times; most area-trigger shapes; unreleased patches | The season's time events (1552, 2771) are in neither CASC (`TimeEventData` ships 5 rows) nor the hotfix cache: the server holds them. `AreaTrigger` ships 3,663 rows and `AreaTriggerCreateProperties` 156. Season 2's third raid (patch 12.1.5, due 2026-10-13) is absent from 12.1.0 entirely. It has no rows, and the tables checked each hold one section that decodes in full, so no missing key is hiding it. It arrives with the 12.1.5 client. |

## 2. Resolving the current season

The client models a season in three places. All three agree for Season 2.

```
DisplaySeason ─┬─ MythicPlusSeasonTrackedMap ── MapChallengeMode ── Map        (the keystone pool)
               └─ DelvesSeason (FactionID only)
JournalTier (Expansion = 9000, "Current Season")
  └─ JournalTierXInstance.AvailabilityCondition ── PlayerCondition.ModifierTreeID
        └─ ModifierTree type 289 (time event): [start passed] AND [end not passed]
              start event = MythicPlusSeason.StartTimeEvent
```

**`DisplaySeason`** row 37 is "Midnight Season 2" (Season 18, ExpansionID 11,
the expansion's season 2, DelvesSeasonID 5). Row 34 is Season 1.

**The keystone pool**: `MythicPlusSeasonTrackedMap` where
`DisplaySeasonID = 37`, eight rows:

| MapChallengeMode | Dungeon | Map | From |
| --- | --- | --- | --- |
| 588 | Altar of Fangs | 2993 | new in 12.1 |
| 584 | The Blinding Vale | 2859 | Midnight |
| 585 | Voidscar Arena | 2923 | Midnight |
| 586 | Den of Nalorakk | 2825 | Midnight |
| 587 | Murder Row | 2813 | Midnight |
| 249 | Kings' Rest | 1762 | Battle for Azeroth |
| 250 | Temple of Sethraliss | 1877 | Battle for Azeroth |
| 399 | Ruby Life Pools | 2521 | Dragonflight |

**Raids and dungeons together**: the Encounter Journal's "Current Season"
tier (`JournalTier` 505, found by `Expansion = 9000` rather than by name or
id) lists instances in groups, one `AvailabilityCondition` per season. Each
condition's modifier tree is two time-event checks: the season's start has
passed and its end has not. The start events are `MythicPlusSeason`'s
`StartTimeEvent` (1525 for Season 1, 1552 for Season 2). Group 156363 is
Season 2:

| JournalInstance | Name | Map | Kind (`Map.InstanceType`) | Bosses |
| --- | --- | --- | --- | --- |
| 1320 | The Venomous Abyss | 3004 | raid | 8 |
| 1317 | The Tidebound Grotto | 2987 | raid (a one-boss Lair) | 1 |
| 1322 | Altar of Fangs | 2993 | dungeon | 3 |
| 1311, 1304, 1309, 1313 | Den of Nalorakk, Murder Row, The Blinding Vale, Voidscar Arena | | dungeon | 3–4 |
| 1041, 1030, 1202 | Kings' Rest, Temple of Sethraliss, Ruby Life Pools | | dungeon | 3–4 |
| 1319 | "Keystone Dungeons" (OrderIndex 9999) | 3029 | not a dungeon | journal pages "Mythic Keystones" and "Affixes" |

Group 149388 (Season 1) holds The Dreamrift, The Voidspire, March on
Quel'Danas and Sporefall (Season 1's Lair). Entry 1312, "Midnight", has no
condition and holds the expansion's world bosses.

**The rule, offline.** The current season is the newest `DisplaySeason` with
tracked maps, and the "Current Season" group whose start event belongs to
the newest `MythicPlusSeason` (by `MilestoneSeason`). The real switch is
a server time event, so a client can know about a season before it opens (a
patch day). Two guards cover that: the user's own logs (the encounter and
keystone ids seen in recent weeks must fall inside the resolved set), and a
config override. The user's store agrees with the resolution: since August
the raid pulls are The Venomous Abyss's eight bosses and Nymrissa
Wavecaller, and the keys are the eight dungeons above.

**Encounters**: `JournalEncounter` rows of each instance give
`DungeonEncounterID` (the id `ENCOUNTER_START` carries — all nine Season 2
raid encounters in the user's store join), `UiMapID` (the boss's floor) and
`Map_0`/`Map_1` (the journal's boss pin, normalised on that floor).

**Delves** are scenario maps (`InstanceType` 5). `DelvesSeason` carries only
a faction, and no table found here lists a season's delves. Delves are out of
scope until someone wants them.

**The third raid** will need a regenerate on 12.1.5's patch day. We expect it
to join group 156363 as a new `JournalInstance`, but that has to be checked
on 12.1.5.

## 3. Difficulties

Each instance has its own set of difficulties, and the user raids several of them
(105 Normal, 281 Heroic, 77 Mythic and 1 World pull in the store).

| Instance | Difficulties (`MapDifficulty`) |
| --- | --- |
| The Venomous Abyss | 14 Normal, 15 Heroic, 16 Mythic, 17 LFR, 220 Story (5–10), 241 Lorewalking |
| The Tidebound Grotto | 14 Normal, 15 Heroic, 233 Mythic (flex 15–25), 250 World (up to 40) |
| Altar of Fangs | 1 Normal, 2 Heroic, 23 Mythic, 8 Mythic Keystone, 205 Follower |
| Kings' Rest | 2 Heroic, 23 Mythic, 8 Mythic Keystone, 24 Timewalking |

**What varies by difficulty:**

- **Journal sections.** `JournalSectionXDifficulty`. 40 of The Coiled
  Altar's 195 sections apply to named difficulties only (22 to Mythic).
- **Spell effects.** `SpellEffect.DifficultyID`. Of the 34 hostile damage
  spells in a Heroic Coiled Altar pull, 20 have per-difficulty rows. Some
  shapes change: Widow's Kiss and Death's Embrace are 40 yd on Normal and
  50 yd on Heroic and Mythic.
- **Durations and periods.** `SpellMisc.DifficultyID` and the effect period.
- **Group size and lockout.** `MapDifficulty`, and `DungeonEncounter.DifficultyID`.

**What doesn't:** the rooms. `UiMapXMapArt` and `UiMapAssignment` carry no
difficulty, so one set of floors serves every mode.

The client carries its own resolution order, `Difficulty.FallbackDifficultyID`:

- Mythic Keystone 8 → Mythic 23 → Heroic 2 → Normal 1 → 0
- raid Mythic 16 → flex Mythic 233 → Heroic 15 → Normal 14 → 0
- LFR 17 → Normal 14, and World 250 → Normal 14
- Story 220 and Lorewalking 241 → 0

A (spell, difficulty) lookup walks this chain to the first row that exists,
as TrinityCore's `SpellMgr` does.
Encounter data is therefore keyed by (encounter, difficulty) and resolved
through the chain. A keystone run reads Mythic's mechanics, plus its
affixes.

## 4. Rooms

### The in-game dungeon map (default floor)

```
log ui_map_id ─ UiMap (Type 4, a dungeon floor; UiMapGroupMember: FloorIndex, floor name)
                 └─ UiMapXMapArt ─ UiMapArt (style 1) ─ UiMapArtStyleLayer: 1002 × 668, 256-px tiles
                                     └─ UiMapArtTile: (row, col, layer, FileDataID) × 12
               UiMapAssignment: world region (min/max x, y, z) ↔ UI 0..1, per WMO group
```

- **A floor is 12 BLP tiles**, 3 rows × 4 columns, stitched and cropped to
  1002 × 668. One floor is about 540 KB of BLP and 2.7 MB as RGBA.
- **The Venomous Abyss has five floors.** UiMaps 2606–2610 are The Soulcoil
  Well, Pit of Fangs, The Vile Crypt, Crypt of the Soulcoilers and The Coiled
  Altar. Two bosses share a floor in three places:
  - The Vile Crypt: Entombed Sentinels and Vashnik
  - Crypt of the Soulcoilers: The Lost Explorers and Sszorak
  - The Coiled Altar: The Coiled Altar and Ula'tek
- **The transform** puts the log's `position_x` on world X, which points up
  the map, and `position_y` on world Y, which points left:

  ```
  u = (y_max − y) / (y_max − y_min)        v = (x_max − x) / (x_max − x_min)
  ```

  A floor has one assignment row per WMO group, and those rows share one
  region. The log already names the floor, so the row to use is the first
  match on (`MapID`, `UiMapID`).
- **Verified on a real pull.** For The Coiled Altar the region is x 980–1755,
  y ±581.25, which is exactly the art's 3:2. A plot of the Heroic kill's
  samples (every eighth per unit) lands on the altar's platform. The
  pull's centre maps to (0.49, 0.76) and the journal's boss pin is (0.50,
  0.77).
- **Resolution is the constraint.** About 0.86 px per yard: 1002 px over
  1162.5 yd. The pull covers about 112 × 106 yd, which is roughly 95 px of
  art. The art itself is a parchment illustration with a frame and a title.
- **The reworked dungeons kept their maps.** Kings' Rest, Temple of
  Sethraliss and Ruby Life Pools use their original art: tile FileDataIDs
  1.99M, 2.04M and 4.64M, against 7.7M–8.2M for Season 2's new content.
  Season 2's rework changed packs and pacing, not floors.

### The minimap render (optional true floor)

- **Where it comes from.** Each map's WDT (`Map.WdtFileDataID`) has a `MAID`
  chunk of 64 × 64 tiles. Entry 7 of a tile is its minimap texture: a
  top-down render of the actual geometry, 512 px per 533⅓-yd ADT tile (0.96
  px/yd), about 130 KB of BLP.
- **Finding a tile.** row = ⌊32 − x/533⅓⌋, col = ⌊32 − y/533⅓⌋.
- **Coverage.** All ten Season 2 maps have them, 6 to 81 tiles each. The
  Coiled Altar pull lands on its platform here too.
- **Limit: one layer.** Floors stacked in Z collide. The Venomous Abyss's
  floor regions overlap in X/Y, which is why the game tells floors apart by
  WMO group.

### The room's own geometry (a render of our own)

The floor's assignment rows lead to the 3D geometry, one room at a time:

```
UiMapAssignment.WMODoodadPlacementID ─ the ADT's MODF entry (same unique id) ─ WMO root FileDataID
UiMapAssignment.WMOGroupID ─ the MOGP unique id of one group file (root GFID list)
```

- **The joins hold.** Placement 65937803 is the MODF entry, in the ADT under
  the altar, for WMO 7287227. That WMO has 113 groups and 1,222 textures.
  Every `WMOGroupID` of the five raid floors resolves to one group file.
- **Rooms map to groups.** The Coiled Altar's floor (UiMap 2610) is two
  groups. 79439 spans world x 1026–1313, where the pull is: the altar room.
  79429 spans x 1294–1728, where Ula'tek's journal pin sits: Ula'tek's room.
- **Size.** A room group is 30–60k vertices, 31–62k triangles and 57–115
  render batches. Each batch's material names its texture by FileDataID
  (`MOMT`), and `MOCV` carries baked vertex colours.
- **What it buys.** A top-down orthographic render of a floor's groups, at
  any resolution, with the right floor by construction. That fixes both
  limits above: resolution, and stacked floors.
- **What it costs.**
  - A software rasterizer with texture sampling.
  - A way to drop ceilings: the log carries no Z, so the cut has to come
    from the geometry, e.g. upward-facing triangles below the group's
    ceiling.
  - Texture blending kept simple: diffuse times vertex colour.

  It is a real piece of work, and generic once done: every boss, every
  season, from data.

### Orientation

All three floor sources are drawn with the game's north up: world +X up,
+Y left. Players don't think in that frame. They think "boss at the top" or
"we came in at the bottom". So the map rotates (the plan's decision 8), and
the data can supply a sensible default.

**The room's own axes, from its geometry.** Fold the near-vertical
triangles' horizontal normals mod 90°, weight them by area, and the peak is
the room's wall angle. (The building's placement is a half turn about Z, so
angles mod 90° are the same in its frame and the world's.)

| Room (WMO group) | Wall angle off north, mod 90° | Wall area within ±2° |
| --- | --- | --- |
| Entombed Sentinels, The Vile Crypt (79326) | 60° | 88% |
| The Coiled Altar (79439) | 0° | 66% |
| Twin Fangs, Pit of Fangs (79428) | 0° | 84% |
| Ula'tek's room (79429) | 0–1° | 32%: rounder walls, a weak signal |

The Sentinels room, which looks diagonal on every game map, sits 60° off
north. That is about 30° short of a quarter turn either way, not the 45° an
eye guesses.

**Which side is up.** The wall angle fixes the rotation up to a quarter
turn. Take the direction from the raid's centroid at the pull toward the
boss (the midpoint of a council), and snap it to the nearest of the room's
four wall-aligned orientations.

- **Positions only.** The rule needs no facing convention.
- **Checked on the Coiled Altar kill.** The 25 players' first samples sit
  26 yd from Zul'jan at a bearing of +2°. That snaps to 0°, which is the
  game's own north-up for that room.
- **A facing data point for Q2.** Zul'jan's facing at the pull was 3.16
  rad, back at the raid. That fits facing 0 = +X, increasing toward +Y.
- **Where there is no clear wall angle,** as in round rooms, it falls back
  to the raid-to-boss bearing snapped to 15°.

**What the default is, and where it lives.** The default is computed once
per encounter from its first captured pull, and the user's own angle
replaces it. Rotation is a view setting: config keyed by encounter (and by
floor, for a fight that changes floors). It never goes into the capture or
the interpretation.

**What rotates and what doesn't.**

- **Turns with the world:** the floor, the positions, facing ticks, cones,
  lines and ground shapes. They all go through one transform about the
  room's centre, so the shapes stay true.
- **Stays upright:** discs, icons and text.
- **A north mark** shows the game's frame, so a call made in game terms
  still reads.
- **No mirroring.** The game never shows a room mirrored, and left/right
  calls depend on that.
- **Floors with text.** The dungeon-map art carries a title and a frame,
  which would turn with it. When the map is rotated it crops to the room.
  The minimap render and our own render carry no text, so any angle is
  clean.
- **No hard edges at any angle.** See "Rotating without edges", next.
- **Words never use compass terms.** The event list, and anything else
  that names a direction, speaks relative to the room or the boss ("behind
  the boss", "12 yd from the boss"). Those stay true at any angle. The
  plan's mcp positions tool should answer the same way.

### Sharper floors: uprezzing at extraction (2026-10-04)

The Phase 0 feel test passed, with one criticism: the floor goes blurry as
the view zooms in. The replay's room view is about 5 px per yard by default
and more when zoomed. The dungeon map is 0.86 px per yard, and the game
itself never magnifies it past 2.14× (`UiMapArtStyleLayer.MaxScale`, with
no higher-detail layer for dungeon floors). So the art has to gain
resolution before it ships to the GUI. There are two routes, both run once
at extraction.

**Upscale the dungeon map with a super-resolution model: works now.**

- **The tool.** Real-ESRGAN's `realesrgan-x4plus`, through
  `realesrgan-ncnn-vulkan` 0.2.0 (nixpkgs, MIT, the model BSD-3), turns a
  1002 × 668 floor into 4008 × 2672 (3.4 px per yard).
- **Speed.** About 4 s on the RTX 5080. It needs `-t 128`: the default
  tile size lost the Vulkan device.
- **Measured in the spike at 11 px per yard.** The original is mush at 13×
  magnification. The 4× floor, magnified 3×, shows crisp carved rings and
  stone joints.
- **Why it works here.** The parchment art is line art, which these models
  sharpen without inventing much.
- **It does not work on the minimap render.** At 1 px per yard there is no
  detail to recover. Edges sharpen, but the venom pool and debris become
  invented blobs.
- **How the generator would do it:**
  1. stitch each current-season floor
  2. hand it to the upscaler when it is on `PATH` (the dev shell can carry
     it, as it carries `okf`)
  3. read the result back, compress it, and store it in the per-machine
     floor cache

  Without the tool, the cache holds the plain floor.
- **Storage.** A 4× floor is 43 MB as RGBA, so a season's 24 floors would
  be about 1 GB. They need block compression: BC1 at half a byte per pixel
  is about 5.4 MB a floor, or about 130 MB a season. Decoding means
  sharing `blp.rs`'s DXT decoder with gui-logic, and only the floor on
  show is decoded.
- **Stays per-machine.** The upscaled art is still Blizzard's, so it never
  lands in the repository.

**Re-render the room's geometry: the real fix, and the spike after Phase
1.**

- **The detail exists.** The Coiled Altar room's floor materials sample
  textures of 512² to 2048² px that repeat every 4 to 13 yd: roughly 40 to
  150 texture pixels per yard. The minimap render sits at about 1.
- **So a re-render is genuine detail, not invented.** A top-down
  orthographic render of the floor's WMO groups (§4, "The room's own
  geometry") at 6–10 px per yard needs no model at all.
- **What it takes:**
  - the group meshes and their UV sets
  - the materials. This build's main floor shader, type 23, blends
    `MOMT`'s second and third textures, and a new `MOMX` chunk sits beside
    them, so its format still has to be worked out.
  - the baked vertex colours
  - backface culling, which drops ceilings without any Z
  - the props (M2 doodads), optional at first

**At runtime, either way.** The CPU resample can switch from bilinear to a
Catmull-Rom kernel for its settled frame. And the zoom stops where the
floor stops resolving: about 3× the floor's own pixels per yard.

### Rotating without edges

At any angle but a multiple of 90°, a rectangular floor image swings its
corners into the viewport. To stay edge-free, the floor source has to cover
the viewport's circumscribed disc: half the viewport's diagonal around the
pivot. A room-sized view (about 140 × 140 to 140 × 225 yd) needs 100–130 yd
of coverage around the room.

**No larger image ships.**

- The floors have no background texture (`UiMap.BkgAtlasID` is 0 on every
  Season 2 floor).
- Their parent map is the zone (UiMap 2512, The Coiled Isle): outdoor art at
  another scale, not the dungeon around the room.
- Upscaling changes resolution, not coverage.

So the coverage has to come from the floor sources themselves.

**The minimap grid slots into place.**

- **Same scale and projection.** Neighbouring ADT tiles share both, so
  stitching every tile that touches the disc brings in the real corridors
  and rooms around the boss.
- **Empty space is one exact colour.** Outside the geometry, every pixel is
  RGB (112, 113, 112), opaque: 72% of the 3 × 3 tiles around the Sentinels
  room. Compression blends at the edges leave 0.6% of pixels within 8 of it,
  so a 2% tolerance keys them all. Keyed to transparent, the room sits on
  the theme's background, and no tile edge shows at any angle.
- **A missing tile** is simply transparent.
- **Liquid planes stay flat rectangles.** The venom under the Sentinels room
  draws as one green square. It is real content and turns correctly, but it
  keeps a hard edge.
- **The minimap's other limits still apply:** about 1 px per yard, and one
  layer where floors stack.

**The parchment art has margins, then a frame.**

- **It is fully opaque.** The torn border, the corner ornaments and the
  title banner are all painted in.
- **Room margins to the frame range from 40 to 259 yd** across this
  season's raid and Altar of Fangs bosses:
  - Zul'jan 40, Rav'i 96, Sentinels 101, Ula'tek 117.
  - Ten of twelve exceed 100 yd; only Zul'jan and Rav'i fall short.
- **Masking the frame.** Mask it out (about 30 px on three sides, 75 px at
  the top) and feather the art into the background. A room-sized rotated
  view then never shows an edge for most bosses. Near the frame, or zoomed
  out, it fades rather than cutting off.
- **Labels turn with it.** The art's baked labels ("THE VILE CRYPT") rotate
  with it and cannot be removed.

**Our own render** of the WMO groups (above) can cover any disc. It is
transparent beyond the geometry, and the neighbouring rooms can be drawn
dimmed for context. It has no edge problem by construction.

**GPUI cannot rotate an image, which settles how to draw it.**

- **Why.** An image is a `PolychromeSprite` with axis-aligned bounds and
  no transform. Only monochrome sprites (SVG paths, glyphs) take a
  `TransformationMatrix` (gpui-pre 0.3.7, `scene.rs`). The floor therefore
  can't be uploaded once and turned on the GPU.
- **The approach: resample on the CPU, into a viewport-sized image.** For
  each angle and zoom, each viewport pixel is mapped back into the floor
  source and sampled. One pass does four jobs:
  - It rotates the floor.
  - It upscales it to screen resolution.
  - It treats void, out-of-source and the masked frame as transparent.
  - It feathers the source's edges.
- **Result.** The GPU only ever draws an axis-aligned image the size of
  the viewport, so the rectangle's corners never exist.
- **Cost.** A 900 × 700 viewport is 0.63 MP, a few milliseconds bilinear.
  It runs once per settled angle and zoom, and per frame only while a
  rotate or zoom gesture is in flight (finite, as every animation here
  must be). Results are cached by angle and zoom.
- **Everything else stays as paths.** Positions, shapes and lines go
  through the same matrix in our code and are painted as paths, so they
  stay crisp.

The plan's Q12 spike should include a rotate drag in its CPU budget.

A worked check (2026-10-04): the Sentinels room, turned −30°, squares to
the screen as the wall measurement predicts.

- **From the keyed minimap tiles:** its neighbours fill the corners, and
  there are no tile edges.
- **From the parchment:** the same crop pulls the title banner and the
  room's label into view, tilted.

**Recommendation.**

- **v1:** the dungeon-map art as the floor everywhere. It is right per floor,
  and it is what players know. Offer the minimap render where the floor's
  region overlaps no other floor.
- **Spike:** a render of the floor's WMO groups as the sharp floor (§8).
- **Either way:** draw positions and shapes as vectors over the floor, so
  they stay crisp at any zoom.

## 5. Bosses and other art

| Asset | Source | Size |
| --- | --- | --- |
| Boss portrait | `JournalEncounterCreature.FileDataID` (the creature row that has one) | 128 × 64 |
| Instance button, background, lore panel | `JournalInstance` Button / ButtonSmall / Background / Lore FileDataIDs | 256 × 128, 512², 512² |
| Loading screen and expansion logo | `Map.LoadingScreenID` → `LoadingScreens.MainImageFileDataID` / `LogoFileDataID` | 2992 × 1684, 1140 × 592 |
| Spell icons | already in `spell-icons.bin` | 64² |
| Journal section icons | `JournalEncounterSection.IconFileDataID` (a texture), or `IconCreatureDisplayInfoID` (a 3D model the game renders at runtime: none for us) | |
| Creature models | `CreatureDisplayInfo` (scale) → `CreatureModelData.FileDataID` (an `.m2`) + texture variations | 3D: drawing one needs a model renderer, out of scope |
| A creature's footprint | `CreatureModelData.GeoBox` × `CreatureDisplayInfo.CreatureModelScale` | e.g. the Twin Fangs ≈ 12 × 14 yd; some rows are degenerate (a −2 or a 24.4 cube), so a sanity check is needed |

All of these decode with the existing `blp.rs`.

## 6. Mechanics data

### The journal tree

`JournalEncounterSection` has four row types:

- stage headers (Type 0)
- creatures (Type 1)
- spells (Type 2, with `SpellID`)
- the overview (Type 3)

`IconFlags` carries the role and warning bits, and `DifficultyMask` plus
`JournalSectionXDifficulty` scope rows to difficulties. The Coiled Altar has
195 sections, 53 spells and four named stages: *Stage One: Serpent's
Bargain*, *Stage Two: Usurper's Reprisal*, *Intermission: The Claimed Vessel*
and *Stage Three: Coiled Union*. Nek'zali and Ula'tek name theirs the same
way. Bosses with no stage header list their spells straight under the
overview.

### The built-in timeline's vocabulary (Q10)

`EncounterEvent` is new in 12.0. Per `DungeonEncounterID` it lists the
spells the game's own boss timeline alerts on, 12–18 per boss, each with a
`Severity` (0–2), an optional `BroadcastText` warning, `Flags` and an icon.
It carries no timings. **Q10's answer:** the vocabulary is client-side and
the timeline is server-side, pushed during the encounter. For layers 1 and
3 it is a list of the abilities Blizzard thinks matter, chosen by Blizzard
and not by us.

### Linking alerts to hits

The Heroic Coiled Altar pull's census:

- 34 hostile damage spell ids hit players.
- The journal names 11 of them directly.
- `EncounterEvent` names none of them. Its ids are the boss's casts
  (`SPELL_CAST_START` / `_SUCCESS`) and the auras it applies. 10 of its 18
  appear in the pull that way, so it is also a ready-made filter for
  layer 0's boss cast lane.

Getting from an alert to its damage needs the trigger chase that
`item_spells` already does (`SpellEffect.EffectTriggerSpell`, two levels),
with name matching behind it.

### Shapes (Q6, in part)

The shapes sit on the damage spells, not on the alert spells. The alert
spells are dummy effects with 200–600 yd target selectors, meaning "everyone
in the room". Real shapes from this pull:

| Spell | Shape |
| --- | --- |
| Gloombomb | 15-yd circle |
| Guillotine, Grim Guillotine | 9 yd |
| Sever | 35-yd cone, 60° |
| Soul Sever, Blighted Sever | 45-yd cone, 60° |
| Volatile Venom | 5 yd |
| Widow's Kiss | 40/50 yd, by difficulty |

A 200- or 300-yd radius draws nothing. Ground effects created as area
triggers are mostly server-side (§1), so layer 2's inference from events
remains the way to place them.

## 7. What a season-scoped cache holds and costs

The house rule holds: per-machine caches under `~/.local/share/wowdps/`,
generated from the install, never committed. A machine without them shows
the yard grid.

- **`encounters.json`** is the season manifest:
  - the resolved season (§2)
  - per instance: kind, difficulties, floors, and transforms
  - per (encounter, difficulty): the floor, the journal pin, the portrait
    FileDataID, stage names, the journal's spells, the `EncounterEvent`
    rows, and the shapes resolved through the fallback chain

  It is small, like `talents.json`.
- **`maps.bin`** holds the floors. Season 2 has 24 of them: 6 raid
  (5 + 1) and 18 dungeon. That's about 13 MB as BLP, or 64 MB as decoded
  RGBA (as large as `talent-art.bin`). Optional minimap crops cover only the
  encounter rooms, a few tiles per boss.
- **`encounter-art.bin`** holds portraits (about 30 × 128 × 64), the
  instance art, and the loading screens downscaled to about 1280 px wide (a
  full-size one is 20 MB as RGBA).
- **What a season change costs.** Regenerate on patch day. The install keeps
  every older map, so a stored capture from an earlier season can have its
  floor generated on demand by `ui_map_id`. "Current season only" bounds the
  default cache without stranding old replays.
- **For the spec.** gui-logic has no BLP decoder (`blp.rs` lives in
  `tools/extract`). It can store decoded RGBA, as the existing bins do, or
  move the decoder somewhere shared and store DXT blocks, which are about 5×
  smaller.

## 8. Prior art, compared

An existing web replay, built on Warcraft Logs' API, covers four of the
raid's bosses. We compared it against these extractions (2026-10-04),
looking at its assets only. Our rule stands: nothing of its code, art or
words is used.

- **Floors.** One hand-captured, top-down render per boss, recoloured,
  1100–2448 px across a single room: about 5 px per yard against our 0.86.
  It is neither the dungeon-map art nor the minimap tiles, but the rooms are
  the same geometry. At the Sentinels journal pin our minimap render shows
  the same chamfered square, with a red gem and a green gem on opposite
  walls. The altar's carved platform is the one the Coiled Altar pull lands
  on. Their Sentinels room is turned so its walls run square to the
  screen. The geometry says that takes a 60° turn (§4, "Orientation"),
  which a fixed north-up map cannot give.
- **Boss discs.** Crops of the Encounter Journal portraits, the same 128 × 64
  files as §5.
- **Player discs.** Spec icons from a public icon CDN, the same art as
  `class-icons.bin`.
- **Shapes.** Plain circles for soaks and ranges. No cones, no lines, and no
  boss facing.
- **No game tables.** Nothing suggests it reads them. Its per-boss art and
  mechanics are made by hand, which is exactly the per-boss cost the plan's
  generic layers avoid.

The lesson for us is fidelity. A replay's floor reads best at several pixels
per yard, and the game's own map art sits at under one. §4's own render of
the WMO groups is the route to that sharpness without hand work.

## 9. Open questions

- **Spike the WMO floor render.** One floor, The Coiled Altar's two groups,
  at 4–5 px per yard. It answers three things: ceiling removal without Z,
  texture blending, and the time and size per floor.
- **Test the orientation default on every Season 2 boss.**
  - Wall angle per room, and raid-to-boss bearing per pull.
  - Is the default stable across pulls and difficulties?
  - Councils, rooms with weak walls, and fights where the raid starts
    behind the boss.
  - The source for a dungeon boss's room, whose floor may not name its WMO
    groups.
- **Read the hotfix cache in the generator.** That means an XFTH reader,
  each table's hotfix record layout (the DBD's build layout less the
  non-inline id, strings inline), and replace/remove by status. A scratch
  reader proved the framing and decoded `TimeEventData`,
  `JournalTierXInstance` and `MythicPlusSeasonTrackedMap` rows.
- **`EncounterEvent.Flags` and difficulty.** No difficulty column was found.
  Do the flags scope a row to a difficulty?
- **Floor changes mid-pull.** Which encounters switch `ui_map_id` mid-pull
  (the plan's risk)? None of this season's five bosses sharing a floor does.
- **Area triggers.** Measure how many of a pull's ground effects have any
  client-side area-trigger row at all.
- **Patch 12.1.5.** Verify the third raid's place in the season tier, and
  re-run §2's checks.
