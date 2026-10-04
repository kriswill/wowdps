# Fight replay — research plan

Scope: what we need to learn before we can spec a fight replay. The replay
is a mode of the pull view. It shows the encounter's room, drawn from the
game's own map art, with every player, boss and add moving on it over a clock
you can scrub, and the mechanics drawn where they happened: puddles, soaks,
droplets and the lines from them to the boss, tethers, frontals, and phases on
the timeline. This document is the research. Its output is
`docs/spec-fight-replay.md` and an implementation plan. Nothing here is built
yet.

Naming: the feature is the **fight replay**. `tools/overlay-replay.sh` is
something else, a *log* replay (it feeds a real log through an overlay at
speed). Keep the two words apart in code and docs.

Roadmap: item 2 lists "Boss phase markers" and "Replay scrubbing". This
plan covers both and goes further.

## Decisions (2026-10-04)

These come from the user and shape every section below.

1. **A mode of the pull view**, not a place of its own. A pull switches
   between its meter and its replay. The rail, the fight header and the
   ribbon stay.
2. **Captured in the background, live included.** The daemon builds a
   pull's replay data off the hub thread, using the spare CPUs, so live
   metering is never slowed.
3. **A sticky off switch.** With replays off, the daemon does no replay work
   at all. This is for players who only want live metering.
4. **Captured once, kept for good.** A fight's replay data is written to the
   history store when it is captured and never re-processed after that.
   Regrading is something the user asks for, per fight: from the raw log if it
   still exists, or from the stored capture where that holds enough
   fidelity.
5. **Curate the current tier only.** Mechanics, phases and verdicts are
   defined for the tier being raided. A new tier gets new definitions, and
   the stored fights are regraded against them.
6. **Local only, no Warcraft Logs keys.** Asking every user to obtain an API
   key is a cost we won't impose, so no part of the replay, including the
   research's own cross-checks, depends on the Warcraft Logs API.
7. **Later: import from Warcraft Logs.** Paste a fight URL, download its
   events, replay it, and compare against another guild's pull. That import
   needs a key, so it waits. The capture format is designed now so the import
   can land in it (§6).
8. **The room turns to the user's picture of it.** The game's north is a
   convention of its map files. Players think of a room as "boss at the
   top" or "we came in at the bottom". The map rotates, starting from a
   default derived from data, and the user's own angle sticks per
   encounter. Rotation is a view setting only: the capture, the
   interpretation and the mcp answers stay in world coordinates.
   (`docs/replay-assets.md` §4, "Orientation".)

## 0. The approach, and what makes it general

Positional replays of WoW fights are not new. Warcraft Logs draws one from
its API's event data (`report.events(..., includeResources: true)` carries
each event's x, y, facing and map). The usual way to add mechanics is
bespoke: an analyzer written by hand for each boss, with a hand-drawn floor
and its events mapped one by one. That costs the same work again for every
boss of every tier.

This plan aims at the general version:

- **The raw log, locally and live.** No upload, no API budget, no cap.
- **The game files.** `tools/extract` reads the client's own tables and art,
  so every encounter's room, journal and spell shapes come from data, not
  from drawing.
- **One axis already drawn.** The ribbon (R25) and the inspector's graph
  share `ribbon::fight_span`. A replay playhead is a third reader of that
  axis, not a new clock.

Mechanics knowledge still does not come free. The central research question
is **how much of a replay is generic** (falls out of the log plus the game's
tables for every encounter) **and how much must be curated per boss.** Every
curated item costs work again each tier, so the generic layers have to carry
most of the value.

## 1. What already exists (don't rebuild)

- **The advanced block.** The parser already finds it on every line that
  has one. `crates/core/fixtures/FORMAT-NOTES.md` §"Advanced block" gives the
  19 fields: `position_x` 14, `position_y` 15, `ui_map_id` 16, `facing` 17.
  §"Which unit the advanced block describes" covers attribution: the source
  on `SWING_DAMAGE`, the target on spell damage, heals and `_LANDED`.
- **`LogLine::hp_hint`** (`crates/core/src/parser.rs`) is the precedent for
  an additive advanced-block hint. It is carried on *any* advanced line,
  including `Event::Other`, and ignored by callers that don't want it. A
  position hint is its sibling.
- **Hostile state the meter already keeps:**
  - R9 recaps
  - R17 damage taken by ability and attacker
  - R18 spans: lust, cooldowns, externals, defensives
  - R21 hostile debuff ledger on friendlies, with stack levels
  - R23 death spans with the rezzer
  - R24 enemy units by name, with per-unit series
  - R25 raid timeline with deaths and lust
- **Background work in the daemon:**
  - the loader pool parses segments off the hub thread
  - the history thread takes one `Segment` clone per closed fight
  - `Store::rewrites` already queues per-fight rewrites from their logs
    while idle (v42)
  - `wowdps history regrade` and mcp's `regrade_fights` already rewrite
    stored cards from their logs, per fight, per encounter or per kind

  Capture and regrade ride these paths rather than new ones.
- **The extraction stack** (`tools/extract`):
  - the local CASC reader
  - WDC5 + WoWDBDefs → CSV for *any* table
  - BLP decoding (`blp.rs`)
  - atlas cropping (`artgen.rs`, the talent art)
  - `$`-token substitution in spell text (`spelltip.rs`)
- **The per-machine cache pattern.** `class-icons.bin`, `spell-icons.bin`,
  `talent-art.bin` and `talents.json` live under `~/.local/share/wowdps/`.
  Extracted Blizzard art never lands in the repository, and the GUI renders
  fine without it.
- **The history store's binary series tier** (`proto::series`, v39/v42). It
  is the model for the capture's encoding: varints, runs, a per-unit index so
  one unit's block reads alone, and a format number that triggers a rewrite
  when it is older than the reader's.
- **The window's surfaces the replay hangs off:**
  - the fight header and the ribbon's crosshair and death skulls
    (`open_death`)
  - the inspector's drag-zoom window
  - the Deaths table's j/k walk
  - the stage's selected player
  - the rail's `[` `]`, which keep the view across pulls (and would keep the
    mode)
- **A corpus.** `~/.local/share/wowdps/design-shots/coiled-altar.txt` is one
  real Heroic Coiled Altar kill. The user's raid nights are in their logs
  dir.

## 2. First measurement (2026-10-04)

Over the Coiled Altar slice (one pull, 25 players, 7:02, 129 MB, 424,030
lines), I counted the advanced lines inside the encounter by the GUID the
block names (`info_guid`):

| | |
| --- | --- |
| Advanced lines in the encounter | 292,480 |
| … describing a player | 183,561 (63 %) |
| … describing a creature | 52,163 (18 %) |
| Samples per player per second | min 10.1, median 16.1, max 59.2 |
| Gap between a player's samples | p50 0.04 s, p95 0.26 s, p99 0.46 s, max 92 s |
| Coordinate span | x 1109.0–1221.2, y −43.9–62.0 (≈ 112 × 106, yards) |
| `ui_map_id` | one, 2610, on every line |

Reading: **sampling is dense enough for straight-line interpolation**, with a
sub-half-second gap at p99. The long tail is presumably a dead player (Q3
confirms this). The hard problems are storage and transport, not sampling.
Coordinates are yards on one map, so the distance between two units is
plain arithmetic.

## 3. The features, and where each one's data comes from

Five layers, from free to expensive. Each layer ships on its own and is
useful without the next. "Generic" means every encounter gets it with no
per-boss work. Layers 0 and 1 are drawn straight from the capture; layers 2–4
are *interpretation* of it (§4 explains why the split matters).

### Layer 0 — the log alone (generic)

| Feature | Source | Open question |
| --- | --- | --- |
| Players moving, facing, class disc | advanced block (position, facing) + the existing class/spec icons | Q1, Q2 |
| Health per unit | `hp_hint`, already parsed | |
| Dead/alive, rez | R23 death spans | |
| Bosses and adds moving | creature-described advanced lines (bosses are sampled whenever they are hit) | Q3: what happens to a boss nobody hits (intermissions)? |
| Boss health bars | `hp_hint` on the boss | |
| Boss cast lanes | hostile `SPELL_CAST_START` (parsed as `Other` today) + `SPELL_CAST_SUCCESS` from the encounter's NPCs | |
| Debuff badges/rings on players | the R21 ledger's hostile debuffs on friendlies | |
| **Hit map**: a dot at the victim's position for every hostile hit | spell damage's advanced block describes the *target*, so the victim's position at the hit is exact | Q7: do puddles show up as clusters without any curation? |
| Tethers/beams | a hostile aura from unit A on unit B → a line between them while it lasts | Q1: which aura families carry positions? (Aura lines have no advanced block, so we use both units' last samples) |
| Fallback stage | a yard grid over the pull's bounding box | |

The hit map is the cheapest "mechanics on the map" there is. If Q7 shows
that ground effects appear as clusters, layer 0 already explains most deaths.

### Layer 1 — the game's tables (generic)

| Feature | Candidate source (to confirm) | Open question |
| --- | --- | --- |
| The room's floor art | `UiMap` → `UiMapXMapArt` → `UiMapArt` → `UiMapArtTile`: 256 px BLP tiles by FileDataID, stitched | Q4 |
| World → image transform | `UiMapAssignment`: world-space region min/max ↔ normalized UI min/max | Q2, Q4 |
| Which map an encounter uses | `JournalEncounter` (UiMapID, map pin), joined to `ENCOUNTER_START`'s id through `DungeonEncounter` | Q5 |
| Boss portraits | `JournalEncounterCreature` (the encounter journal's portrait FileDataID) | Q5 |
| Ability list, icons, role flags (tank, healer, deadly, interrupt, magic …) | `JournalEncounterSection` + `JournalSectionXDifficulty`; descriptions through `spelltip.rs` | Q5 |
| Stage *names* | the journal's stage header sections ("Stage One: …", "Intermission") | Q5, Q9 |
| Circle sizes | `SpellEffect.EffectRadiusIndex` → `SpellRadius` | Q6 |
| Cones and lines | `SpellTargetRestrictions` (cone degrees, width) + the boss's facing | Q6 |
| Ground-effect shapes and lifetimes | `SpellEffect` CREATE_AREATRIGGER → `AreaTrigger` / `AreaTriggerCreateProperties` (shape type, shape data, polygon vertices); `SpellDuration` | Q6 |

Generators follow the house pattern:

- **`tools/gen-encounter-maps.sh`** writes `maps.bin`: stitched, cropped,
  per-machine. The transforms are numbers, not art, so they could be a
  generated table in the repo.
- **`tools/gen-encounters.sh`** writes `encounters.json`: bosses, portraits,
  sections, spell shapes, per-machine, like `talents.json`.

Both are re-run when a new tier ships, the way the class-spell and talent
generators are re-run each patch.

### Layer 2 — inferred mechanics (a rule per mechanic *kind*, not per boss)

Area triggers (puddles, pools, droplets) are **not in the combat log**. We
infer their placement from events that bracket them, then check it against
the damage they deal:

- **Dropped puddle.** A hostile debuff is removed from its carrier → a
  puddle at the carrier's position at that moment. Shape, size and lifetime
  come from layer 1. Check: the spell's hits on players land inside the
  shape.
- **Soak circle.** The players inside the radius at expiry, counted from
  their positions. Check: the debuff's or damage spell's target count.
- **Line from a puddle to a boss** (a projectile or beam between them). A segment
  from the inferred puddle to the boss's position, with its width from layer
  1. Check: the line's hits fall within width/2 of the segment.
- **NPC mechanics** (orbs, coagulations, adds). Some mechanics are creatures
  with their own GUIDs, so their positions are sampled like any unit's (Q8).

Every inferred shape must *look* inferred (a dashed edge, say) and carry a
confidence measured over real pulls. A drawn puddle that is wrong is worse
than none.

### Layer 3 — phases (rules per boss, names from the journal)

The log has no phase event. Candidate signals, most trusted first:

1. a boss aura applied or removed (intermission shields, immunities, "stasis")
2. a boss cast that opens a stage
3. boss health crossing a threshold (the `hp_hint` curve is free)
4. a gap in damage to the boss (untargetable), adds appearing (first
   sample or `SPELL_SUMMON`), a boss dying in a council fight

These are per-boss rules, so they belong in the tier's curated definitions
(layer 4's format). A **generic fallback** marks unnamed boundaries from
signals 1 and 4 alone, so a boss with no definitions still gets landmarks.
Q9 measures whether the fallback is good enough for the ribbon on its own.

Checking the rules without Warcraft Logs (decision 6):

- **Hand-labelled reference pulls.** A person records a handful of the
  user's own pulls' transition times by watching the boss's health, casts
  and auras in the replay itself, or from a recording. They are committed as
  times only, with no names, and they are the phase rules' golden file, as
  `sample.expected.tsv` is the meter's.
- **The journal's own thresholds.** Where a stage header says "at 60%
  health", signal 3 is checked against it.
- **Boss mods** (BigWigs, DBM) encode stage triggers as spell IDs per boss.
  They are reference reading for whoever curates. Check their licences, and
  never vendor them.

Phases pay off outside the replay too: ribbon bands, inspector graph
landmarks, mcp answers, and stage splits on Home's boss rows.

### Layer 4 — curated verdicts (per boss, current tier)

Verdicts say what went wrong and who it involved: a player standing in
another's line, a pairing that failed, a debuff left to expire, a burst that
hit bystanders instead of its carrier. They are predicates over the tracks,
auras and damage.

The format is **data, not renderer code**: one definitions file per tier,
with an entry per encounter, validated by a generator against a real-log
census, as `role_spells.rs` and `proc_spells.rs` are. A curated entry the
logs never prove fails the generator. The file carries a version, and every
interpretation records the version it ran under (§4). Q13 designs the format,
and two bosses written fully in it are the exit test.

### Around the map (every layer)

- **Clock.** The playhead is the ribbon's crosshair on `fight_span`:
  - play, pause, and 0.25×–4×
  - a death skull jumps to five seconds before the death
  - the inspector's zoom window becomes a loop range
- **Follow a player.** j/k walk players as on the meter. The stage's
  selected player carries across the mode switch, so a player picked on the
  meter is the one followed on the map.
- **Status panel.** What each player carries at the playhead.
- **Event list.** Deaths and layer-2/3/4 events; a press jumps
  the clock there.
- **Live.** The game flushes the log in multi-minute bursts, so a live
  pull's replay runs minutes behind, "as far as the log has it". The main use
  is after the pull.

## 4. Where it lands in the architecture

### Capture and interpretation are separate

Decision 4 asks for two things at once: never re-process, and regrade when
definitions change. Both hold if the replay is split in two:

- **The capture** is everything a replay needs *from the log*, written once
  and kept for good. It is the expensive half: a pass over the pull's lines.
- **The interpretation** is layers 2–4: mechanics, phases and verdicts,
  computed from the capture, the tier's definitions and the game-data
  caches. It is the cheap half, and it never touches the log.

A curation fix or a new tier's definitions then re-interprets every stored
fight without its log and without re-processing. Re-capture from the log is
needed only when the capture itself grows, meaning it lacks something new
definitions want. The capture should therefore be **generous**: keep more
than today's layers read, so a new tier almost never needs the log. Q15
decides what "generous" holds.

Interpretation is computed at read time and cached by (fight id,
definitions version). It is never stored as truth, so it never goes stale.
Phases may also be denormalised onto the card for Home and mcp; the spec
decides.

### The capture

- **Contents (Q15 confirms).**
  - **Tracks.** Per unit (players, pets, bosses, adds): time, x, y, facing,
    health and map. They are simplified with an error bound (drop a sample
    the line between its neighbours already predicts within ~0.2 yd), so
    standing still costs nothing and movement keeps its shape.
  - **The hostile event stream:**
    - casts (start and success) by hostiles
    - every aura with a hostile on either side, with doses
    - hostile damage and misses on friendlies, each with the victim's
      *exact* position from its own advanced block, so a hit test never
      depends on simplified tracks
    - deaths, rezzes, summons
    - friendly damage to each hostile unit per second (the "untargetable"
      phase signal)
- **When.** When an encounter closes, the history thread queues a capture
  job on the loader pool. The job reads the pull's byte range from the log
  file (still in the page cache moments after the pull) and runs a dedicated
  capture pass. The live meter never parses a position, so live metering
  costs exactly what it does today, whether replays are on or off.

  An incremental capture during the pull, for a partial replay while it is
  live, is a later option on the same format. Q11 measures whether
  compile-on-close is fast enough to make it unnecessary.
- **Which fights.** Encounter pulls: raid bosses (wipes included, since
  progression is where a replay matters most) and dungeon bosses inside
  keys. Trash is never captured. The spec may let config narrow this to
  kills.
- **Stored for good.** A `replay/<id>.bin` tier in the history store,
  encoded in the series tier's style (varints, runs, a per-unit index,
  quantized as 0.1 yd i16 from the map's origin and facing as u8) with a
  format number. It sits outside every retention cap and demotion, and a
  captured fight's card is protected with it so the replay never orphans.
  Only an explicit "forget this replay" removes it. Q11 sizes a season,
  because permanence makes this the store's largest tier.
- **The off switch (decision 3).** Config `replay_capture = false` (the
  name is the spec's). Off means no capture jobs, and nothing else changes:
  the meter was never doing replay work. The daemon reads config once at
  start, so a toggle in the ⚙ card needs a way to reach a running daemon
  (Q16).

### Regrade

There are two levels, and only one needs the log:

- **Re-interpret.** Automatic. A new definitions version invalidates the
  cache, and the next read recomputes. No command, no log.
- **Re-capture.** For fights captured in an older format, captured while
  replays were off, or from before the feature existed. It needs the log,
  and it extends what exists:
  - `wowdps history regrade` gains a replay option (per fight id, per
    encounter, per kind)
  - mcp's `regrade_fights` gains the same
  - the pull view offers "Capture replay" on a pull whose log still exists,
    and says why not when the log is gone
  - `Store::rewrites` may also queue older-format captures while idle, as
    v42 does for series files, but only while capture is on

### The rest of the stack

- **Parser.** A `pos_hint` (guid, x, y, map, facing) beside `hp_hint`,
  carried on any advanced line. Parse `SPELL_CAST_START` (boss cast bars),
  passive. Only the capture pass reads either.
- **CONTRACT.md: a new ruling (R27, positions).** It fixes:
  - which unit a sample describes, per event family
  - the coordinate system and facing convention
  - what interpolation a reader may assume
  - a dead unit's position
  - that positions never open or extend a segment (the passive gate, so the
    scanner is untouched)

  It needs a `positions.txt` fixture, with `check.awk` metrics (sample
  counts per unit, one known distance) and a parity gate: a capture from the
  file's byte range equals one built by replaying the whole file.
- **Proto: `GetReplay` / `Replay`, chunked by time range.** A capture is
  megabytes, and neither the frame nor the 10 Hz push was built for that.
  The interpretation travels with it, so the GUI holds no definitions. This
  is a `PROTO_VERSION` bump.
- **gui-logic.** `replay`:
  - interpolation and staleness
  - the map transform
  - shape geometry and hit tests
  - the event list's words
  - phase bands on the shared axis

  All of it is tested and has no GPUI. Interpretation itself lives beside
  the definitions, where the daemon and mcp can run it too (crate placement
  is the spec's call).
- **GUI: a mode of the pull view (decision 1).**
  - **What stays.** The rail, the fight header and the ribbon. The ribbon
    becomes the scrubber, with phases drawn as bands on it.
  - **What swaps.** Below the ribbon, the view tabs, the meter and the
    inspector give way to the map and its side panel. At 820 px and under,
    the map sits over the panel.
  - **Switching.** A switch in the fight header (Meter | Replay) and a key,
    with a place in the Esc chain. A move between pulls keeps the mode, as
    it keeps the view.
  - **A pull with no capture** says why (capture is off, it was never
    captured, the log is gone) and offers the capture where the log
    exists.
  - **Drawing.** The map is one canvas, painting images and paths in order
    as the talent panes do.
  - **Rotation (decision 8).** One view transform turns the floor, the
    positions and the shapes together about the room's centre.
    - Discs, icons and text stay upright. Facing ticks, cones and lines
      turn with the world.
    - A small north mark keeps the game's own north readable, so a call
      made in game terms still makes sense.
    - The user's angle is saved in config per encounter (and per floor,
      for a fight that changes floors). Difficulty never matters, since
      the room is the same in every mode.
    - The keys and gestures are the spec's call.
- **Overlay and TUI.** Nothing in v1.
- **MCP.** A `positions` (or `replay`) tool for the coach: a player's
  distance to the boss at a time, where someone died, who stood in a given
  ground effect. It is cheap once captures exist, and it gives `wow-coach`
  questions it cannot answer today.

## 5. Research questions

Each has a method and an exit criterion. Q1–Q3 decide feasibility and are
cheap. Run them first. Nothing here uses the Warcraft Logs API (decision 6).

**Findings so far.** `docs/replay-assets.md` (2026-10-04) answers Q4, Q5
and Q10 and part of Q6 against build 12.1.0.69933. It also covers two
questions this list missed:

- how to resolve the current season from the client's own tables
- how difficulties change encounter data (they never change the room)

**Q1 — Who a sample describes.** Extend FORMAT-NOTES' attribution table to
every family that carries a block: casts, energizes, drains, heals,
`_LANDED`, `_SUPPORT`. Method: a census over the role-spell census's eight
real logs comparing `info_guid` with source and dest per family. Exit: every
family in the table, with no "unknown".

**Q2 — The coordinate system.** Find which world axis is the image's
horizontal, the sign of each axis, and facing's zero and direction. Method:
a pull with known geometry (a tank facing a boss, where facing ≈ the angle to
the boss) and positions plotted over Q4's art. Exit: one transform function,
tested against a real pull.

*Progress (2026-10-04, `docs/replay-assets.md` §4).*

- **Positions.** `position_x` is world X (up the game's map) and
  `position_y` is world Y (left). This is verified on the Coiled Altar
  pull.
- **Facing.** One data point so far: facing 0 = +X, increasing toward +Y.
  Zul'jan faced 3.16 rad, back at a raid that stood on −X, but one pull is
  not yet a tank-and-boss test.

**Q3 — Gaps and the units nobody touches.** Find what the long gaps are
(death, out of range, phased), how dense bosses and adds are when nobody hits
them, and pets. Exit: a staleness rule ("faded after N s, frozen at death").

**Q4 — The rooms.** For the tier's encounters: the UiMap, its art tiles
stitched, and the `UiMapAssignment` transform. Some maps are whole-instance
overviews where the room is a small crop, and some encounters span floors or
change `ui_map_id` mid-fight. Method: extract Coiled Altar and Sentinels,
draw the measured pull on them, and compare with the room in game by eye.
Exit: the measured pull lands on the floor. Also a count of encounters with
usable art versus the yard-grid fallback.

**Q5 — The Encounter Journal.** Extract `JournalInstance`,
`JournalEncounter`, `JournalEncounterCreature`, `JournalEncounterSection`
(+ difficulty), and `DungeonEncounter`. Check that `ENCOUNTER_START`'s id
joins. Exit:
- an `encounters.json` for the tier
- a census: the share of hostile spells cast in a real pull that the
  journal names (the rest are helper and trigger spells to classify)

**Q6 — Spell geometry.** For every journal ability of two bosses, find a
shape (circle r, cone θ and r, line w × l, polygon) or "none", from
`SpellRadius`, `SpellTargetRestrictions` and the area-trigger tables. Exit:
the shapes match what the game draws, by eye.

**Q7 — Inferring ground effects.** Three kinds: a dropped puddle, a soak,
and a line from a puddle to a boss. For each, find the bracketing events,
fit a rule, and score it. Exit:
- a rule per kind, with the share of that mechanic's hits falling inside
  the drawn shape on real pulls (aim ≥ 95 %)
- whether the bare hit map (layer 0) already shows the puddles

**Q8 — Mechanics that are creatures.** A census of hostile `Creature-`
GUIDs per encounter by NPC id, with sample counts. Exit: per boss, which
mechanics move on their own samples.

**Q9 — Phases.** Per boss, the transition signals from §3 layer 3, checked
against hand-labelled reference pulls and the journal's stated thresholds.
Exit:
- per-boss rules agreeing within 1 s on every labelled pull
- the generic fallback's agreement rate on the same pulls

**Q10 — Midnight's built-in encounter timeline.** 12.0 added a boss
timeline and warnings to the game. Does its data (stages, ability timings)
ship in the client's tables or arrive from the server? Method: search the
build's DBD list for encounter and timeline tables. Exit: tables named, or
"server-side". If client-side, it may hand us stage names and timings that
layer 3 would otherwise curate.

**Q11 — Cost, size and speed.** Measure:
- **capture time** for one pull from the file, on the measured 129 MB
  slice and inside a 300 MB night. Target: far under the gap between pulls,
  on one loader thread.
- **capture size** per pull (a raid kill, a long wipe, a dungeon boss), raw
  and after error-bounded simplification and quantization, and the
  simplification's worst-case error
- **a season's size** under permanence, from the user's store: count its
  encounter pulls (wipes included) and multiply. If it is gigabytes, the spec
  needs a cheaper default (kills, or recent nights at full rate) before
  anything is kept for good.
- **read time.** Interpretation of one pull from its capture. Target:
  instant enough to need no stored copy.

Exit: compile-on-close confirmed or replaced, sizes signed off.

**Q12 — Drawing it in GPUI.** One canvas with the stitched floor, 25–40
discs, shapes and lines, playing. GPUI re-renders the whole view that asks
for a frame (the live-dot pulse cost 5.7 % of a core). Spike the replay as
its own entity driven by a timer, so a playing frame does not re-render the
window.

GPUI draws images axis-aligned only (`PolychromeSprite` has no transform),
so a rotated floor is resampled on the CPU into a viewport-sized image.
That same pass upscales the floor and fades its edges
(`docs/replay-assets.md` §4, "Rotating without edges").

Exit: CPU at 1× and 4× under a budget the user signs off, and a rotate
drag within it.

**Q13 — The curation format.** Phases, mechanic kinds and verdicts as data,
versioned per tier, with a generator that proves each entry against a
census. Exit: two bosses written fully in it, interpreted from their
captures alone.

**Q14 — Assets and terms.** Map art and portraits stay per-machine (the
existing rule), and screenshots with map art stay out of the repo, as the
design shots do. Nothing is taken from another replay tool's code, art or
words: our data comes from the log and the client, our words are our own.

**Q15 — What the capture must hold.** Design the capture so layers 2–4
never need the log. Method: interpretation takes the capture and the
definitions and nothing else, so the type system enforces it. Write Q13's two
bosses against it. Then list what a plausible next-tier mechanic might need
that the capture lacks (a friendly aura? a cast's destination?) and decide
whether to keep it now. Exit: both bosses interpreted with no log access, and
a written list of what the capture deliberately leaves out.

**Q16 — The off switch on a running daemon.** The daemon reads config once
at start. The options are a client message the daemon applies (and the GUI
persists), or the daemon polling the config's mtime as the overlay already
does for its theme (roadmap item 3's config reload). Exit: one path chosen,
and turning it off mid-night stops capture jobs without a restart.

## 6. Later: importing from Warcraft Logs

Decision 7, deferred because every user would need their own Warcraft Logs
API client (v2 client credentials, free but a hurdle). The idea: paste a
fight URL, fetch its events with positions, convert them into **the same
capture format**, and replay it beside your own. One use is comparing
against another guild's kill, possibly drawn as an overlay on your own pull
in the same room.

What this asks of the design now:

- **Interpretation reads only the capture**, never the log (Q15 enforces
  this). An imported fight is then interpreted, drawn and compared like any
  other.
- **The capture records its source** (a local log or an imported report)
  and keeps foreign fights apart. An imported pull never counts as yours on
  Home, in grades or in the coach's trends.

Research for when it starts:

- Warcraft Logs' units and axes against the log's.
- Which actor a resource block describes (its equivalent of Q1).
- The point cost of one pull against a personal key's hourly budget.
- The API's terms for local, personal use.

## 7. Milestones (2026-10-04)

The work ships in phases. Each phase ends in something a person can run on
real pulls and judge **by feel**, and that review steers the next phase's
UX and scope. Each phase gets an *As built* note and a refined next step,
the way `docs/plan-gui-new.md` did. Research questions attach to the phase
that needs them rather than all running first.

**We are not starting from nothing.**

- The research above, and `docs/replay-assets.md`.
- An existing web replay that does this well. Its feel is the benchmark:
  - playback that never stutters
  - a zoom anchored at the pointer
  - a wheel the map captures
  - overlays that scale with the floor
  - a mechanic timeline under the map
  - a "now" panel tied to the clock
  - follow and trails

  Q14 still holds: none of its code, art or words.

**The platform targets.** 144 Hz on the user's main monitor (DP-3 runs at
143.98 Hz; the portrait DP-1 at 60). macOS and Windows later, so the replay
adds nothing Linux-only. (The daemon's unix socket and the overlay's layer
shell are their own porting work.)

### Phase 0 — the platform gate: GPUI, or a web view?

The reason for GPUI was smooth 120 Hz-plus motion without an Electron
shell, in Rust end to end. This phase proves that holds for a replay before
any production code is written.

**What the sources say (gpui-pre 0.3.7 and Kit 0.7.0, read 2026-10-04):**

- **Frame pacing.** On Wayland, GPUI paces frames from the compositor's
  frame callbacks, with Mailbox presentation. So it can draw at 144 Hz
  where the monitor does. GPUI has Metal on macOS and DirectX on Windows
  (`gpui-pre-macos`, `gpui-pre-windows`); Zed runs at 120 Hz on ProMotion
  Macs.
- **Cached views.** An entity embedded with `.cached(style)` reuses its
  last frame unless notified, and `request_animation_frame` notifies only
  the view that asks.
- **But the root view re-renders every frame.** Its element is laid out
  and prepainted each draw. The window is one root view today
  (`CLAUDE.md`: one endless pulse idled the window at 5.7% of a core). So
  the replay needs the window's heavy parts — rail, header, ribbon, side
  panel — as cached entities, and a cheap root.
- **Primitives that fit:**
  - quads with corner radii, for discs and rings (cheap, anti-aliased)
  - paths, for cones, lines and trails
  - images, for the floor and icons, clipped round by corner radii
  - monochrome SVG sprites, which take a transform: facing arrows and
    glyphs can rotate
  - shaped text, cached per line
- **Primitives that don't fit:**
  - Images cannot rotate. The floor is CPU-resampled per angle
    (`docs/replay-assets.md` §4); during a drag that is a new texture per
    frame, so `drop_image` hygiene matters.
  - There are no custom shaders (no forks).
  - The cost of many paths per frame (trails!) at 144 Hz is unknown.

**What a web view would mean:**

- **No embedding on Linux.** Kit has no web view. wry, the library Tauri
  uses, embeds as a child window only on X11. Under Wayland it needs a GTK
  container, so in a GPUI window it would be a separate window or the
  system browser. That breaks decision 1, because the rail, header and
  ribbon would no longer frame it. On macOS and Windows it embeds.
- **Three engines to tune feel against:** WebKitGTK, WKWebView and
  WebView2. WebKitGTK's high-refresh behaviour is unverified.
- **A bridge.** The daemon speaks a binary protocol on a unix socket that
  a page cannot open. That means either a proxy in the GUI, or an HTTP or
  WebSocket server in a stdlib-only daemon.
- **Two implementations to keep in parity.** The codec, interpolation,
  transform and geometry would be ported to TypeScript (or built to wasm).
- **A dependency-policy exception.** A web view plus a JS toolchain needs
  its own decision record.
- **The upside:** a mature 2D canvas, hot reload, and the benchmark itself
  is a web page.

**Leaning: GPUI.** It keeps the replay inside the pull view on every
platform, keeps one Rust implementation under the borrow checker, and adds
no runtime or toolchain. The spike decides with numbers, not leaning.

**The spike.** A GPUI prototype in the real window shell, the rest of the
window as cached entities. It runs on one real pull: the Coiled Altar
slice, whose tracks are written by a throwaway tool to a scratch file, so
the GUI still never parses a log. It draws:

- the rotated floor (CPU resample)
- 25 discs with spec icons
- the boss
- about 20 shapes and lines
- 10 s trails

The interactions are play at 1× and 4×, scrub, zoom anchored at the
pointer, and a rotate drag.

- **Measured:** frame time (p50/p99), dropped frames and CPU at 144 Hz on
  DP-3, plus the rotate drag's resample cost.
- **Felt:** the user plays, scrubs, zooms and rotates.
- **Exit:** numbers and feel signed off, and GPUI or a web view chosen in
  a decision record. If a web view, the phases below are re-planned around
  the bridge.
- **Answers:** Q12.

### Phase 1 — the dots move on the real floor

- **Capture v0, built on demand.** Built from the log when a pull is
  opened in replay mode, and held in memory: tracks for players, pets,
  bosses and adds, plus deaths. It is not stored yet, so the format can
  change freely until the feel settles.
  - Parser: `pos_hint`, passive.
  - R27 drafted, with the `positions.txt` fixture and its parity gate.
  - Proto: `GetReplay`, chunked, with the version bump.
- **Floors.** The current season's dungeon-map floors from a first
  generator, and the yard grid without them.
- **The mode switch** in the fight header, and the ribbon as scrubber:
  play, pause and speeds.
- **Follow a player.** j/k, and the stage's selection carries over.
- **Rotation.** The data default and the per-encounter override
  (decision 8).
- **Answers:** Q1–Q4, Q11's capture time.
- **The human test.** Does the room read? Do the movements read? Is the
  default orientation right? How do scrub, play, zoom and rotate feel?
- **Steers:** disc size, names, trails, the default zoom, the panel's
  contents.

### Phase 2 — what hit whom (layer 0)

- **Boss cast lanes.** `SPELL_CAST_START`, filtered by `EncounterEvent`
  (Blizzard's own alert list).
- **Health.** Boss and player health.
- **The hit map.** Each hostile hit at its victim's exact position.
- **Debuffs and tethers.** Badges and rings on the players carrying a
  debuff, and tethers between linked units.
- **Deaths.** A skull jumps the clock to five seconds before; the recap
  follows.
- **The status panel**, at the playhead.
- **Answers:** Q7's bare-hit-map half, Q8.
- **The human test.** Can you explain a death from the map alone?
- **Steers:** which layers are on by default, density, and colours per
  theme.

### Phase 3 — kept for good

- **The capture tier.** `replay/<id>.bin` in the history store, captured in
  the background when the encounter closes.
- **Controls.** The sticky off switch, re-capture through regrade, and
  stored pulls replaying.
- **Answers:** Q11's sizes for a season, Q15, Q16.
- **The human test.** Replay last week's pull, and turn capture off
  mid-night.
- **Steers:** what is captured by default (every pull, or kills), and
  the capture's contents.

### Phase 4 — the game's tables (layer 1)

- **The encounters manifest.** Stage names, portraits and journal icons.
- **Spell shapes drawn where the tables give them.** Circles and cones,
  difficulty-aware through the fallback chain.
- **Answers:** Q5, Q6.
- **The human test.** Do the shapes match what the game showed?
- **Steers:** which shapes to draw, and how a known shape looks against an
  inferred one.

### Phase 5 — inferred mechanics and phases (layers 2–3)

- **Inferred ground effects.** Puddles, soaks and lines, each drawn as
  inferred and carrying its measured confidence.
- **Phases.** Bands on the ribbon, from per-boss rules plus the generic
  fallback, checked against hand-labelled pulls.
- **Answers:** Q7, Q9, Q10.
- **The human test.** Do the phases match memory? Are the inferred shapes
  trustworthy?

### Phase 6 — verdicts for the current tier (layer 4)

- **The definitions format.** Two bosses written fully, then the rest of
  the tier.
- **The mcp positions tool** for the coach.
- **Answers:** Q13.
- **The human test.** Are the verdicts fair, and useful after a wipe?

### Spike, any time after Phase 1 — the sharp floor

A render of the floor's WMO groups (`docs/replay-assets.md` §4, §9).

- **The human test.** Is it worth it next to the dungeon map?

## Risks

- **The capture bends a roadmap ground rule.** The daemon "stores
  *summaries* it can derive, not raw events", and a capture is event-level.
  The roadmap now names the capture as the one scoped exception (item 2a).
  Keep it scoped: filtered to pull time and hostile parties, simplified, in
  our own format, never a copy of log lines.
- **Permanent storage grows without bound.** Every captured pull is kept
  for good. Q11's season number decides whether the default captures every
  pull or fewer.
- **Curation never ends.** Every tier brings eight bosses, so layers 0–2
  must carry most of the value. If Q7 shows the hit map does not, the plan
  shrinks to layers 0–1 plus phases.
- **A capture too lean forces re-capture,** and re-capture needs logs the
  user may have deleted. That argues for a generous capture (Q15), traded
  against size (Q11).
- **Inferred shapes can mislead.** They must look inferred and carry a
  measured confidence.
- **Floors and platforms.** A `ui_map_id` that changes mid-pull needs a map
  switch on the clock.
