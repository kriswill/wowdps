---
type: Crate
title: wowdps-encounter-rubric
description: 'The seasonal encounter rubric: per-encounter TOML under the crate''s own seasons/ (the room''s map and view, NPC roles, abilities and shapes, phases and their triggers, floor features, what the fight leaves on the floor, the events that change the room and the game''s spell facts), a dungeon''s creatures with their enemy forces and its curated trash, embedded at build time and resolved here for one difficulty at one tier; main carries the generated base alone, and the replay lays its private curated set over it.'
resource: crates/encounter-rubric
tags: [crate, replay]
status: stable
generated: { by: claude-code/opus-5.5, at: 2026-10-29T23:00:00-07:00 }
sources:
  - id: readme
    resource: ../../../crates/encounter-rubric/README.md
    title: The rubric's schema reference, key by key
  - id: plan
    resource: ../../plan-fight-replay.md
    title: Fight replay plan — §3 layers 3–4, the curated definitions this crate holds
  - id: assets
    resource: ../../replay-assets.md
    title: Fight replay assets — §3, difficulties and the client's fallback chain
  - id: turn
    resource: ../../../crates/encounter-rubric/src/turn.rs
    title: turn.rs — the turn proposed for a room and the map plane it is read on
  - id: swirl
    resource: ../../../crates/encounter-rubric/src/swirl.rs
    title: swirl.rs — a place's swirling texture drawn as a disc of pixels
  - id: guess
    resource: ../../../crates/encounter-rubric/src/guess.rs
    title: guess.rs — where a volley's kill stood, picked from its fronts
  - id: drift
    resource: ../../../crates/encounter-rubric/src/drift.rs
    title: drift.rs — the way the players drifted over a push, its bearing
  - id: follow
    resource: ../../../crates/encounter-rubric/src/follow.rs
    title: follow.rs — the way a line that follows points, and whom an aura names
---

The seasonal knowledge the fight replay needs per boss, beyond what the log
and the game's tables give for free. It is the plan's "one definitions file
per tier" (layers 3–4)[^plan], written as TOML so it is reviewed like code
([Keep The Encounter Rubric As Reviewed TOML](../decisions/encounter-rubric-toml.md)),
for [A Fight Replay Captured Once And Interpreted Per Tier](../decisions/fight-replay-captured-once.md)
over [the season the client names](../decisions/replay-data-from-the-season.md).
It is named for the ENCOUNTER so it is never taken for a spec rubric (a
coaching rubric grades a player's spec; this one describes a boss). It was
`wowdps-rubric` (`crates/rubric`, the data in the repository's `rubric/`)
on the replay branch until 2026-10-09, when it came to main under this
name with its seasons inside it.

gen-rubric drafts it from the client's tables into `seasons/` (the
generator comes to main with the extractor's library half; until then it
runs on the replay branch and its drafts are committed here), a person
tunes it, and the replay's floor renderer and the replay read it through
this crate. Main, the public repository, carries the BASE alone. The
curated layer, the tuned and trash files, is private: the replay keeps it
in its own tree, `curated/seasons/`, embeds it and lays it over this
crate's base as one bundle
([Keep The Curated Set Out Of The Rubric Crate's Tree](../decisions/curated-set-leaves-the-rubric-tree.md)),
so this crate's `seasons/` holds drafts, instance and season files alone,
which `build.rs` and a test hold it to.
On main the daemon reads only its generated `placed` table, for its
replay cut ([R29](../rulings/r29.md)); nothing on main reads the rubric
itself yet.

It is the one engine-side crate that names serde/toml (CONTRACT.md
§Dependencies), and it takes [`wowdps-proto`](proto.md) for `proto::dirs`,
the XDG directories its text sidecars and the user's rubric directory
resolve through. Beside the rubric it holds the formats the extractor
writes and the GUI reads for an encounter's art, one reader and writer
each: `floor` (a rendered floor's `.txt` placement sidecar — `Sidecar`,
`Floors::scan` over the floors cache, `floor::slug`; a dungeon level's
sidecar, from `gen-floors --map`, says its `map` and is found by
`Floors::levels`; a level's render from the geometry says the same and a
`minimap` line naming the minimap picture, and is found by
`Floors::level_render`; since 2026-10-12 every sidecar ends with what its
render was made from, `params`, a hash of the resolved `[map]`'s keys
that change pixels, walked in a fixed order with floats by their bits
(`Map::params_hash`, whose doc comment names every key either way;
`Encounter::floor_params` adds a room's cut from
`Encounter::room_render`, which gen-floors draws rooms by, and leaves the
arena's `crop` out of a room's hash, as a room never reads it;
`Map::level_params` hashes a level's posts and what of its level's map
its render draws by (a `ceiling` only where it is not the options' own,
so a level saying its season's 25 hashes as before levels read one; the
extractor's `floorfiles::level_hash` leaves it out on a level of
buildings, which reads none),
`floor::level_params` a level whose map says none;
the render keys `crop`, `include_*`, `fade`, `exposure` and `water` are
walked only where a map says them, and so, through one walk both hashes
share (`Walk::cut`, since 2026-10-30), are the map's `hang`, each enabled
area's `ceiling` and `hang` with the `shape` that places them, and on a
level each area's `stencil` and, once one is said, each enabled path's
corridor by its resolved way and width, so a hash moved only where one is said) with
the options that change pixels, and `made`; `Floors::stale` compares
("Load A Floor's Scene Once, As Typed Objects" and "Lay A Faded Piece
From A Veil, In Its Own Grade", the replay's records))
and `portraits` (the
crops file, `tools/replay-extract/portrait-crops.tsv`, and the portraits cache's
`index.tsv`)
([Keep The Extractor's And The GUI's Shared Formats In Rubric And Proto](../decisions/shared-formats-in-rubric-and-proto.md)).
It also holds `placed`, the GENERATED table of what players place in the
room (totems, gateways, rings, zones) and how the log tells where each
stands, which the daemon's replay cut keeps the lines of for the history
store and the replay draws: the one part of the crate that is not per
encounter. On main the daemon reads it, through core's
`replay::PlacedTable`, and gen-placed-spells writes it (the generator
comes to main with the extractor's library half).

## Source

- Manifest: [`crates/encounter-rubric/Cargo.toml`](../../../crates/encounter-rubric/Cargo.toml)
- Root: [`crates/encounter-rubric/src/lib.rs`](../../../crates/encounter-rubric/src/lib.rs)
- Schema: [`crates/encounter-rubric/src/schema.rs`](../../../crates/encounter-rubric/src/schema.rs),
  documented key by key in the crate's [`README.md`](../../../crates/encounter-rubric/README.md)[^readme]
- Seasons: [`crates/encounter-rubric/seasons/`](../../../crates/encounter-rubric/seasons/),
  a directory per season (`midnight-s1`, `midnight-s2`), each with its
  instances' directories

## Seams

- **Embedded, so a change is a rebuild,** but for a tree a reader reads
  from a checkout at runtime: its tuned and trash files alone over the
  build's base (`with_curated_dir`), or the whole tree in the build's
  place (`from_dir`, the replay spike's `--seasons`; `base_origin` is
  `Origin::Dir`, the tree refused whole on any file, encounter, trash or
  level that does not read, stricter than the build's reader, the
  version folding in the tree's path; strays such as an editor's lock
  file passed over). `TreeFiles` walks a tree without parsing it, its
  `fingerprint` telling a reader the tree did not change; `from_tree`,
  `with_curated_tree` and `with_user_tree` lay a walk's files, so what a
  reader fingerprinted is what it laid.
  `build.rs` walks the crate's
  own `seasons/` and writes an `include_str!` per `.toml` into `OUT_DIR`,
  paths relative to it (`<season>/<instance>/<file>`, the same paths a
  bundle and the user's directory use). The seasons live inside the crate
  so the flake's `./crates` source set carries them, and the crate is
  whole in itself. gen-rubric reads only the seasons' files of the
  checkout it writes into, so a draft or tuned file in any state never
  stops a regenerate.
- **Raw tables merge before anything is typed.** Sources merge as
  `toml::Table`s, key by key, in the stack's order: season defaults,
  instance defaults, draft, the instance file's `[encounter.<id>]` (the
  encounter's map and NPCs), each curated set, the user's files, then every
  source's difficulty overrides. Only the result is deserialized. So an
  override can name one key of an inline table (`shape = { length = 45 }`),
  and the typed structs never see a partial entry.
- **Three tiers, an ordered stack, the base standing alone.** Each
  `Source` names its `Tier` (`Base < Curated < User`, the default `User`):
  the base's four (`Season`, `Instance`, `Draft`, `Drawing`), then
  `Curated(Origin)` for each curated set in the order laid (a tuned file
  embedded beside its draft is `Curated(Embedded)`, a checkout's tree on
  disk read in its place by `with_curated_dir` `Curated(Dir(name))` (its
  tuned files alone, the base staying the build's; read whole by
  `from_dir`, the same source under the tree's path), a runtime
  `with_curated` bundle `Curated(Bundle(name))`), then `User`
  (`with_user`, `with_user_dir` over `$XDG_CONFIG_HOME/wowdps/rubric/`, a
  tree that mirrors `seasons/`). A
  set is one block (its `season.toml`, its `instance.toml`, the encounter's
  file) laid over everything under it; a draft is never in one. A bundle is
  laid whole or refused; a bad user file goes to `user_errors` and is left
  out. `encounter_at` takes the tier per encounter, `with_tier` caps it,
  `layers_of` lists an encounter's sources, and `encounter_traced` answers
  a `Provenance` (per key path, who last set it, through which difficulty
  override) with the same `Encounter`. The embedded test resolves and
  `check`s every encounter at every tier, so no base leans on a curated
  file ([Resolve An Encounter Through An Ordered Layer Stack](../decisions/encounter-rubric-layers.md)).
- **An encounter's map is the base's.** The instance file gives each
  encounter's `map` and `view` by DungeonEncounterID, however hand-made,
  and what changes the map in the fight: its `room`s, `place`s and the
  `event`s they and its layers key on. It also gives an `npc` the draft
  lacks or has wrong and the arena's own `travel` (`INSTANCE_TABLES`):
  Ula'tek's Venomous Heart. Any other table there, or an id with no files,
  is an error. The base alone draws every room whole, with the same NPCs
  in it; a curated or user set may still amend an arena's map and view.
  A `[map]`'s
  `stencil` (`StencilShape`: a polygon or a circle in world yards) tells
  gen-floors what of the room to keep, and
  `Map::stencil_bounds` and `stencil_depth` answer where it lies, and
  `stencil_keep` how much of a point the cut keeps (`STENCIL_FEATHER`,
  the yard gen-floors and the replay both fade it over)
  ([Keep An Encounter's Map In The Base, Cut To A Stencil](../decisions/encounter-map-in-the-base.md)).
  Its `ppy` is the render's density, and its `glow`s (`Glow`: a color at
  rest, the `hot` one it brightens to, an `area` of stencil shapes less its `except`
  ones, the depth `below` the arena that takes the color, the trigger it brightens
  `on`) are liquid the game colors itself. `Glow::brightness` is the one
  easing (smoothstep up over `rise`, held, down over `fade`), so the
  replay and any other reader brighten alike.
- **A dungeon's level is a map too.** An instance file's
  `[level.<ui_map>]` (`LEVEL_TABLES`: `map`, `view`, `travel`, `pull`)
  resolves over the season's and the instance's `[defaults]` alone
  (`LEVEL_DEFAULTS`: never a pull; a `[defaults]` giving any key that is
  no encounter's table, `pull` among them, is refused where its file is
  read, `DEFAULTS_TABLES`), never a difficulty or a set, into a
  `Level` holding the same `Map` an encounter's does (`Rubric::level`,
  `levels`, `check_level`), the usual route's pulls on it (`Pull`: an
  `order`, an `area`, its members as instance creature keys, optional
  `spots`, and `optional`, a pack the route may skip at some key levels,
  never counted missing; `Level::pulls_in_order`), and the instance's
  creatures laid in (`Level::npc`, skipped in a file and left out of a
  level's equality and dump), so `check_level` holds a pull's members to
  them, `Pull::forces` derives its share of the forces and
  `Level::route_forces` the route's, without its optional pulls and with
  them; a level read from text
  alone, as the editors read one, is checked with `check_level_with` and
  the instance's `Instance::npcs`
  ([Resolve An Encounter Through An Ordered Layer Stack](../decisions/encounter-rubric-layers.md),
  its pull tables amendment). Any map,
  an arena's or a level's, may name its areas (with altitudes a reader
  fills with `Area::altitudes`), the paths between them (with a mode) and
  a `transition`, and the render's `crop`, `include_*`, `fade`, `ground`,
  `exposure` and `water` (the look its water is poured in, on every water
  whose colors are the default light's; since 2026-10-29): the vocabulary
  a map editor writes ([Give The Map A Vocabulary Per Level And Per Area](../decisions/map-vocabulary-per-level-and-area.md)).
  Every season dungeon's terrain level says `ground = "walked"`, the
  ground its render is cut over (2026-10-12). A set may amend an arena's
  map, never a level's. `gate_map` gates every
  map's named entries; `fixtures/resolved-maps.txt` holds every
  encounter's map and view to what they resolved to before, and
  `fixtures/resolved-levels.txt` every level as it resolves now
  (`WOWDPS_BLESS=1` rewrites it). Since 2026-10-30 an area cuts and
  bounds the render over it: its `ceiling` is read (`Map::ceiling_at`,
  the lowest of the enabled areas holding a point, else the map's), a
  `hang` on the map and on an area leaves out what hangs over the walk
  (`Map::hang_at`, the lowest wins as well), and an area's `stencil`
  with every enabled path's corridor (`Map::path_way` widened to
  `Path::corridor_width`, `Path::CORRIDOR_YD` 8 yd unsaid) bounds a
  level's render (`Map::level_stencil_depth`, `level_stencil_bounds`,
  `level_stencil_keep`, `level_stencil_count` for the sidecar's
  `stencil`), the one union gen-floors, the replay and the editors read;
  an arena's area stencil is checked and written, not drawn. `check`
  holds both hangs above 0 and an area's stencil as a map's;
  `write::ORDER` lists `hang` after `ceiling`. No schema bump: optional
  keys with defaults, noted on `SCHEMA`'s doc comment
  ("Cut And Bound A Level's Render Area By Area", the replay's record).
- **A proposed turn has one rule** (`turn`, re-exported at the crate's
  root)[^turn]: the value proposed for a map's `view.turn`, by the
  extractor's `suggest-turns` (from stored pulls, to seed the rubric) and
  the map editor's suggestion, here so both read one rule and a change
  moves them both. No render reads it: a map's turn is the rubric's
  alone, and an unsaid one is north up
  ([The Replay's Room Turns To The User, Without Edges](../decisions/replay-room-turns-without-edges.md);
  `docs/replay-assets.md` §4, "Orientation"). `room_turn` takes the
  raid's and the bosses' midpoints on the map plane (`plane`, the world's
  x up and y to the left as u right and v down, the replay spike's `geom::plane` a re-export of it;
  `midpoint`, the mean, none of none) and the floor sidecar's
  `wall_angle` and `wall_share` (`floor::Sidecar::wall`), and gives the
  clockwise turn in [0, 2π), 0 north up: the boss above the raid, north
  up within 3 yd, snapped to the four wall-square turns at a share of
  `ROOM_WALL_SHARE` (0.4) or more, else to 15°. The caller gathers the
  midpoints from each unit's first sample within `ROOM_TURN_WINDOW_MS`
  (15 s) of the pull's start, and proposes none where either side has
  none. Plain arithmetic, no serde.
- **A place's swirl has one renderer** (`swirl`)[^swirl]: a `[place]`'s
  `swirl` (`Swirl`, written inline: a gray, tileable texture's
  FileDataID, its two `colors`, dark and light, as `#rrggbb`, a `twist`
  in degrees that falls off to none at the rim as ImageMagick's
  `-swirl`, and a `spin` in degrees a second, clockwise on screen) is a
  spell's visual filling the place's circle, not the map's geometry, so
  no floor render bakes it. `swirl::disc` draws it as a straight-alpha
  RGBA disc (`swirl::rows`, a band of its rows, for a caller's own
  threads): feathered over the outer 5% of the radius, the texture's gray
  leveled from 5% to 95% between the two colors, turned by an angle
  `Swirl::turned` reckons from `spin`. One look for the replay and the
  map editor: the replay turns it with the playhead, and the editor will
  show it still through `swirl::disc` (it opens the install itself),
  over the floor and under the place's ring and label.
  `check` refuses a color not `#rrggbb` (the one color outside
  `tint::NAMES`), a swirl on a place with no radius, a twist or spin
  that is not a number, and texture 0. Plain arithmetic, no allocation
  but the disc. The Soulcoil Well's is the first (2026-10-18).
- **A volley's arithmetic is the schema's.** `Volley::out` is how far a
  projectile's middle has gone at a time (none before it sets out or past
  its `length`) and `Volley::steps_facing` its bearings as unit steps along
  the log's X and Y: the world's own axes, or turned with the facing of
  `from`'s unit where its `aim` is `facing` (`steps` is the world's). Its
  `spread` lets a reader turn each projectile to the hits it made, a
  `front` makes each a wall across its way, `width` deep, and `every` and
  `times` send it again from the same spot, the first `delay` after its
  trigger (`Volley::sendings`); `spots` set each projectile out from its
  own place about the unit, mirrored to an NPC's `side`
  (`Volley::origins`) and, with `alternate`, to the other side each
  sending (`Volley::sendings_sided`); `holes` makes the projectiles slots
  a sending fills only some of, dropped where a living player stood in
  the way unhit (`faint` draws the untold ones faint), each such slot or,
  with `holes_by = "cluster"` (`HolesBy`, unsaid `"slot"`), one hole a
  sending where those players cluster round a radial volley's middle,
  which the replay reads (2026-10-29); a `side` of
  `"raid"` is the side the players' middle moves to; `form` draws each a disc or a crescent. An
  NPC's `still` says the log's one placing of it holds for its whole life,
  and its `shares` names the NPC whose health it shares, one pool (`check`
  holds that the NPC is the encounter's).
- **A volley's placed kills are picked by `guess`**[^guess]: where a
  volley says `place_kills` (it needs `destroys`, `check` holds), a kill
  the log never placed stood on one of its fronts as it died. `guess` is
  the pick, pure geometry over plain types (`Run`: a sending's departure
  in ms, its `start` and `speed` and its projectiles' `Way`s, each a
  spot, a unit step and a reach, the yards out its middle goes before it
  is gone; the death; the room's `StencilShape`s, none for all of it;
  the spots still standing): `place` gives every front not past its
  reach and inside the room as a `Candidate` and the `Pick`, the one
  farthest from the nearest standing spot, ties to the earliest sending,
  then its lowest projectile, none where no front is inside;
  `place_latest_first` places one NPC's kills from the last to die, the
  spots standing at each death (`Standing::at_death`: first acted by it,
  not dead before it) those of its units the log posted, which the caller
  gives, and of its kills placed already, so a guess keeps clear of a
  posted unit as of another guess; of kills dying at one moment, the
  later listed is placed first and the earlier sees it. No grid, no
  table. The replay spike gives each projectile its path's end and a
  width more (as far as it draws it), never a hole, and draws each placed
  kill dashed and gray from its first act to its death; a trash rubric's
  volley places none, with no room to place in. The editors, which take
  this crate by pinned rev, can draw the same; the count, the moment and
  the distance are the log's, the spot is picked. Vashnik's Plague Wave is
  the first (2026-10-19).
- **Typos are errors.** Every struct is `deny_unknown_fields`, and errors
  carry the encounter's file. `check` (its own module, a method per
  section) reports names an encounter uses but does not have: an
  ability's NPC, a pet's owner, a trigger's phase or event, a layer's
  break; an `only` naming no difficulty, which would keep its entry off
  every one; and a color outside the rubric's vocabulary (`tint::NAMES`:
  green, red, blue, orange, teal, cyan, purple, pink, gold, yellow, amber,
  white, ice; `tint::known`) in any tint field, a place's, ground's,
  line's, volley's, flash's or range's `tint` or a mark's `ring` or `dot`
  (since 2026-10-17; a reader would draw an unknown name as the foe's).
  The fields stay strings, so the editors' types do not move. The
  seasons' and instances' files are read straight into `Season` and
  `Instance`.
- **Values inherit; presence does not.** Difficulty overrides apply along
  the client's fallback chain, the most general first. A Heroic override
  reaches Mythic and keystones, as the game's own data inherits[^assets].
  `difficulty::CHAINS` is the one table of the chains, a row per name
  (`chain` by a log's id, `chain_named` by a name), and one step,
  `lay_difficulties`, lays a file's overrides for the encounters, the text
  sidecars and the writer alike. An entry's `only` list is exact, though:
  the journal lists an ability's difficulties exactly, and what Heroic
  alone has, Mythic lacks. Every named entry is `Gated` (switched off by
  `enabled`, kept to difficulties by `only`), and `resolve` names every
  field of `Encounter` and `Map`, so a kind of entry added to the schema
  does not build until it is gated.
- **A dungeon's creatures are the base's; its trash is curated.** An
  instance file gives its own creatures (`[npc.<key>]`, the same `Npc`,
  with the enemy `forces` each gives, an `aloft` for one that takes
  wing and a `control`, `all`, `stun` or `immune`, for how it can be
  crowd-controlled) and the dungeon's whole `forces`: `Instance::npcs`,
  `npc_of` (by creature id), `forces`, found by the map id a run's log writes
  (`Rubric::instance_by_map`). A set's instance file giving either is
  refused, as a level is; `check_instance` holds the table to what
  `check` holds an encounter's NPCs to, and to a total where its creatures
  give forces. An instance's `trash.toml` (`FileRole::Trash`, curated) says
  what they do outside the bosses, in an encounter's vocabulary, and
  resolves over the instance's creatures into an `Encounter` numbered 0 at
  a keystone (`Rubric::trash`, `trash_at`, `trash_traced`; its sources
  `trash_layers_of`, `has_curated_trash`, `has_user_trash`), giving none of
  `TRASH_TABLES`' refused keys (a run's map, view, travel, phases and
  rooms are its levels' and bosses'); `check_trash` checks it as `check`
  does an encounter and refuses a trigger in it that counts from its
  start, which would be the key's. The stack's
  checks take it as a `Subject` of its own beside the encounters
  ([Resolve An Encounter Through An Ordered Layer Stack](../decisions/encounter-rubric-layers.md),
  its 2026-10-17 amendment).
- **A season's keystone rules are the base's.** `season.toml`'s
  `[rules]` (`Season::rules`, a `SeasonRules`) gives what the client's
  tables do not: `death_penalty`, from-level and seconds pairs, which
  `SeasonRules::penalty_at(level)` reads as the pair with the highest
  from-level at or under a key's level (none under the lowest, none
  unsaid); a from-level given twice, or a cost below 0 or not a number,
  is an error as the file is read (`SeasonRules::problems`). A set's
  `season.toml` gives `[defaults]` alone, so no set can
  give it; a reader reaches it by a run's map through
  `Rubric::instance_by_map`
  ("Count A Key Run's Enemy Forces And Keep Its Timer As The Log Reckons
  Them", the replay's record).
- **The writer is held to the files.** `write` lists every key it writes
  in its `ORDER` (public: the map editor puts a key it adds to an instance
  file where `ORDER` does), a test holding the order to exactly the keys of an
  encounter with every field set, and another writes every committed draft
  back byte for byte from what it reads as, with no game install.
- **Versioned twice.** Every file carries `schema = N`. `migrate` brings
  older files forward, a migration a version, and refuses newer ones;
  `SCHEMA` 2 (2026-10-17) added an instance's creatures and an NPC's
  `forces` and `aloft`, keys alone, so its 1 to 2 migration only moves the
  number, and a build that reads 1 refuses a file in 2 by it (though a
  `trash.toml` by its name: such a build takes it for a misnamed
  encounter file and refuses a tree holding one whole). `Rubric::version()` is the
  seasons plus an FNV-1a hash over every file laid (the base's, the
  curated sets', the user's), which an interpretation records so a stored
  fight regrades when the rubric changes; laying nothing leaves it.
- **Grounds land where a unit stood.** A `[ground]` is what each occurrence
  of a trigger leaves on the floor: a pool a debuff drops as it comes off,
  a droplet a player soaks. Only a trigger about a unit can place one (an
  aura's target, a hit's victim, a cast's caster as the cast lands or, by
  `cast_start` (`SPELL_CAST_START`), as it begins, an NPC that appears or
  dies), and `check` refuses the rest, `ahead` yards in front of the unit
  where it says so (a Spectral Coil's crush). The `hit` trigger came with
  it. Entombed Sentinels holds the first two. `cast_start` came with
  Sszorak's frontals, 3 s casts drawn while he casts them. A start
  writes no position, so the replay reads it off the caster's track as
  it draws the caster then: part-way to its next post under 1.5 s
  apart, its last post otherwise; a caster posted only as it lands is
  held where and as it landed. Sszorak, posted during his casts, turns
  to his target after the start, so his frontal stands `from` the start
  `at` the landing (`ends = "at"`), aimed where it lands. A ground that
  `renew`s is renewed by a firing inside one standing rather than placed
  again, an `after` ending counted from that firing: Sszorak's Caustic
  Residue, which the log places only as its ticks hurt someone standing
  in it, a pool or a few a spot instead of one a tick. A renewal keeps
  the footprint as it first formed. The replay reads such a ground's
  firings and hits in one pass in time order (a `seen` hit at a firing's
  moment after it), so `check` refuses it beside a `from`, an `"at"`
  ending and a `seen` spell one of its `hit` triggers places it by. Beside
  a `push` it is allowed since 2026-10-27: the pushes do not depend on the
  grounds, so each ground is given those under way or to come as it forms
  (one under way for the rest of it: a puddle a tick lands mid-gale is
  blown with the gale) as it
  is placed, and a firing finds it where it is drawn (the residue blown
  across Sszorak's platform renews one pool through a gale, not one a
  tick). A ground that `falls` is gone the first moment its spot leaves
  the encounter's map stencil where its push or orbit has taken it (a
  puddle blown off the platform), no renewal lasting past that; formed
  or last found again outside (each `seen` find a fresh witness), it
  never falls, and found again inside, it falls when it next leaves.
  `check` refuses it with neither a push nor
  an orbit, and with no stencil, which is every trash file's case: a
  trash file gives no map.
  A ground that `clears` other grounds (their keys) sweeps away, as it
  forms, those standing then whose spot lies in its footprint: The Coiled
  Altar's Severs, tank frontals that destroy the Coalesced Venom orbs the
  log never writes gone. A volley's `destroys` is the same between a
  volley and NPCs. A ground the difficulty lacks (`only`, switched off)
  leaves `clears` as it resolves (Mythic Keystone lays Mythic's overlays
  without its Mythic-only grounds), and `check` refuses a name that is no
  ground, the ground itself, and a cleared ground that `renew`s or has
  `seen` hits (its firings and hits are read before any clear).
- **A clock counts from the pull or from a trigger.** `Trigger::After`
  holds a `Clock`: `secs` into the pull, or `secs` after the `nth` time
  its `since` trigger fired (unsaid, the first), or with `each` after
  every time it fired (2026-10-27: the gales of each of Sszorak's Howling
  Maelstroms). Its serde is by hand so it writes as it reads: a bare
  number from the pull, else a table, `nth` where it was said (held as an
  `Option`, so a written `nth = 1` is told from an unsaid one) and `each`
  only where true, after `nth` in `write::ORDER`. Layers merge tables key
  by key, so the two forms keep their meaning there: a later layer's
  number replaces the whole clock, and its `{ secs = … }` retimes a
  `since` clock and keeps the rest. `check` refuses a time under 0 or not
  a number, an `nth` of 0, an `nth` beside `each` (whichever layers gave
  them), and a `since` naming what the encounter lacks. Ula'tek's
  Shattering, 68.5 s after her second Rage of the Shackled, is the first.
- **A push fires on a list and can follow the players' drift**[^drift]
  (2026-10-27). A ground's `push.on` is a list of triggers (one alone
  still reads, as a list of one; it is written as a list), every firing
  of every one a push in time order, one within a second of the last
  counted that one, the bearings going round by the merged firings.
  `toward` is a list of bearings or the word `"drift"` (`Toward`): each
  push then goes the way the living players drifted over its `secs`,
  read by `drift::read`, pure arithmetic over plain types (each player's
  `Post`s and the window) that the editors can call too: steps of 500 ms,
  a player read at each end between posts under 1.5 s apart, a step's
  raid velocity the mean of its players', a step faster than 8 yd/s a
  knock left out, the drift the mean of the kept steps, its bearing in
  world degrees as a volley's `toward`, its speed, its kept and total
  steps and its coherence (the mean resultant of the moving players' unit
  steps); none where nothing is kept or it is about still, and a push
  that reads none moves none. Nothing is snapped to set bearings: the
  gales blow a different way each time. `check` refuses an empty `on`.
- **Lines and flashes draw what has no spot of its own.** A `[line]`
  strikes from where a trigger's unit stood to an NPC, `delay` later,
  through the players its `hits` find along it (Living Venom), or with
  `to = "@source"` (`Line::SOURCE`, an aura's trigger alone, which
  `check` holds it to) to the unit that put the trigger's aura on, where
  several alike stand (Corrosive Spit's adds). A line that `hold`s never
  strikes: it runs from the aura's carrier, wherever it goes, for as long
  as an `aura_applied` aura stays on (Unnerving Fixation, to the ghost
  that fixated them); `check` refuses it beside another trigger, a
  `delay` or `hits`. A `[flash]`
  flashes a spell's hits on their victims, and a blow on most of the raid
  reads as one (Noxious Blast, whose droplet the log never places).
- **Marks read a count the log may not write.** A `[mark]` is an aura the
  replay draws on whoever carries it, with its count and a `goal` that
  clears it. Where the log writes the aura but never its stacks (Helical
  Toxins), `count = { tick }` reads them from a tick's unmitigated amount
  over one application's, unsaid the pull's smallest tick.
- **A mechanic is a kind of entry, not boss code.** `[range]` (a circle
  round each standing unit of its NPCs at a spell's reach, `sides` the
  line between two), `[apart]`
  (two NPCs nearer than `within` outside the phases they may), `[streak]`
  (a spell hitting one player until another takes it), a `[knock]` (the
  unit its trigger is about thrown out from a place or an NPC, landing
  `within` a place: Nymrissa's Pop!), a `[mark]` drawn as
  a `ring` with seconds, a `link` to the nearest carrier, its `soak`ers, a
  side's `dot` or the `harm` it sets off, a `[ground]` seen coming
  (`preview`), and a `[phase]` that comes `again` until its `leave`
  (`aura_gone`). The replay draws, times, narrates and judges them, and
  builds its mechanics timeline and "Right now" panel from them
  ("Draw A Fight's Mechanics From The Rubric's Entries", the replay's
  record).
  `check` refuses a range with no size, an apart that is not a pair, a
  `leave` on a phase that does not come again, a knock about no one or
  from neither a place nor an NPC.
- **A room off the arena is found by its way in.** A `[room]` holds the
  players its `enter` triggers take in while they stand in its `through`
  place, until its `leave`, while its `npc` live: Nek'zali's well, whose
  divers the log keeps on the arena's map and yards. A room with no way in
  holds whoever stands in its radius. The replay draws it in an inset, and
  `gen-floors` renders it apart
  ("Find A Room Off The Arena By Its Way In", the replay's record).
  A `[reach]` is an NPC that must not get to a place (one whose place
  `absorbs` takes in a unit that gets there and is never posted again,
  drawn sliding into the middle, counted absorbed, not dead), and a ground's
  `orbit` and `seen` move it where the log never posts it. `check` refuses
  an `enter` with no place, a reach to a place with no radius and an orbit
  round nothing.
- **A line or a cone can follow a unit**[^follow] (2026-10-29). A
  ground's `follows` (`Follows`: `"target"`, `"facing"` or `{ aura = N
  }`) turns it each moment from its spot, which stays where it formed:
  toward its cast's target (as it lands or starts, where the log's row
  names one), toward a player carrying aura N (`follow::carrier`: the
  first it goes on from the firing within a second, else the latest
  still carrying it), or along its unit's posted facing, the unit picked
  as it fires and kept while it stands. `follow::aim` is the moment's reading,
  pure arithmetic the editors can call: toward the unit as drawn, else
  (on the spot, within `NEAR_YD`) toward its latest post off the spot
  since the ground formed, else the way it formed. Nymrissa's Water Jet
  follows the tank by its 6 s aura, the cast row naming no target.
  `check` refuses it on a circle or a ring, beside an `orbit` or an
  `ahead`, `"target"` by a trigger that is no cast (an aura's carrier and
  a hit's victim are the unit it lands on), and an aura of spell 0.
- **The journal's tree is kept.** gen-rubric writes each ability's `under`,
  the nearest ability above it in the journal (through any creature
  header), so a room's `about` rolls its section up for the replay's help.
  A difficulty that lacks the parent leaves the child standing alone.
- **Sizes from the game where it says them.** The draft's `[spell.<id>]`
  facts (radius, duration, period, stacks, triggers, a difficulty's own in
  its overlay) are what a `[range]` takes its radius from and what tells a
  ringed mark carried its whole duration.
- **Blizzard's words stay on the machine.** An ability's `text` is never in
  a committed file. `gen-rubric` writes each encounter's journal text to a
  per-machine sidecar,
  `$XDG_DATA_HOME/wowdps/rubric-text/<season>/<instance>/<id>-<slug>.toml`,
  shaped like a draft's abilities, and `Rubric::with_texts(text::dir())`
  reads each once (`text::read`) and lays it over the encounter whenever
  it is answered, as each difficulty reads it. Without the sidecar the
  text is unsaid; one that cannot be read is named by `text_errors`.
- **Newest season first.** A returning dungeon (Kings' Rest, Ruby Life
  Pools) can sit in two seasons. `encounter(id, difficulty)` takes the
  season with the highest `order`, which is the client's own season number.
  `from_files` and `from_dir` build the index every lookup reads
  (`encounter_at`, `has_curated`, `encounters`, each id once); two
  instances giving one id in one season, or in two seasons of one
  `order`, is an error naming both.

[^readme]: The rubric's schema reference.
[^plan]: `docs/plan-fight-replay.md` §3, "Layer 3 — phases" and "Layer 4 — curated verdicts".
[^assets]: `docs/replay-assets.md` §3, "Difficulties".
[^turn]: `crates/encounter-rubric/src/turn.rs` and its tests: north up, the 3 yd guard, the 15° steps on either side of 7.5°, the wall snap at a share of 0.4 and its fallback under it, every turn in [0, 2π), `midpoint` and `plane`.
[^guess]: `crates/encounter-rubric/src/guess.rs` and its tests: the fronts as far out as the wave went, none past its reach, the stencil's filter (an edge inside), the farthest pick, ties to the earliest sending and its lowest projectile, nothing without a candidate inside, kills placed from the last to die (one dying at the same moment standing for the one placed after it), and a posted spot standing from its first act to its death.
[^swirl]: `crates/encounter-rubric/src/swirl.rs` and its tests: the rim's feather, the 5%–95% levels, a quarter turn bringing the right to the bottom, a twist carrying the middle counter-clockwise as ImageMagick's `-swirl` does, a band of rows the whole disc's, and the colors read as `#rrggbb` alone.
[^drift]: `crates/encounter-rubric/src/drift.rs` and its tests: a steady drift's bearing and speed (90 along the log's Y, 225 down and back along both), a step faster than 8 yd/s left out as a knock, a player whose posts are 1.5 s apart or more read only on a post (none under 1.5 s read between), nothing read for players standing still, a stretch no one was posted in, or one shorter than a step, and the mean and coherence of players moving three ways.
[^follow]: `crates/encounter-rubric/src/follow.rs` and its tests: the bearing from a spot to a point (0 along the log's X, a quarter turn along its Y, none within a tenth of a yard or to no number), a unit on the spot or not posted pointed at where it last stood off it else the way it formed, and an aura's carrier (the first it goes on after the firing within a second, else the latest still carrying it, none for neither); the check in the crate's tests.
