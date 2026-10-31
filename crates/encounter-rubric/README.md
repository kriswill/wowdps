# The seasonal encounter rubric

What the fight replay knows about each boss of a season beyond what the
combat log and the game's tables give for free:

- how its room is drawn
- how the replay first shows it
- who is in the fight
- what they cast and the shapes it leaves
- the fight's stages and what begins them
- the floor's fixed features
- the moments that change the room

It is TOML, reviewed like code, kept in this crate
(`crates/encounter-rubric`, `wowdps-encounter-rubric`) under `seasons/`,
and embedded in the binaries at build time. A change takes a rebuild, but
for a tuned file, which a reader may read from a checkout at runtime
(below).

## Files

```text
seasons/<season>/season.toml                        the season, its defaults and rules
seasons/<season>/<instance>/instance.toml           a raid or dungeon
seasons/<season>/<instance>/<id>-<slug>.draft.toml  generated, never edited
seasons/<season>/<instance>/<id>-<slug>.toml        tuned by hand (curated)
seasons/<season>/<instance>/trash.toml              tuned by hand (curated)
```

The public repository carries the BASE: the drafts and the instance and
season files. The tuned files and the trash files are the curated layer and
ship apart from it, by design (How an encounter resolves, below). A tree
may hold them beside their drafts, as the layout above shows, and reads
them so; this crate's own `seasons/` never does. On the replay branch the
curated set lives at the repository's root, in the same layout
(`curated/seasons/<season>/<instance>/<id>-<slug>.toml` and `trash.toml`),
and the replay embeds it and lays it over the base this crate embeds as
one bundle (`Rubric::with_curated`, named `curated`).

`<id>` is the encounter's DungeonEncounterID, the number the combat log's
`ENCOUNTER_START` writes.

- **The draft** is written by `tools/gen-rubric.sh` (on the replay branch,
  with the generator's other tools) from the client's tables:
  the journal (names, creatures, ability sections, stage headers,
  difficulty scoping), `EncounterEvent` (the abilities the game's own
  timeline alerts on) and the spell tables (how far each spell reaches, how
  long it lasts, how often it ticks). With `--census` it also reads the
  replay spike's pulls for creature ids, and for the NPCs the journal leaves
  out. Re-run it after a patch; it rewrites every draft and never touches a
  tuned file.
- **The tuned file** is small. It holds only what a person decided about
  the fight: a phase's trigger, a pet's owner, a puddle a mechanic leaves.
- **The instance file** holds each encounter's map, however much of it was
  worked out by hand, and its NPCs where the draft lacks one or has it
  wrong, under `[encounter.<id>]`:
  - how its room is drawn and how the replay first shows it (`map`,
    `view`);
  - what changes the map in the fight: rooms off it (`room`), its fixed
    places (`place`), and the events its layers, rooms and places key on
    (`event`). Ula'tek's platform breaking on The Shattering and Circling
    Prey, and Nek'zali's well, are here;
  - what an NPC is (`npc`), laid key by key over the draft's: Ula'tek's
    Venomous Heart, which the journal lists no creature for, sharing
    Ula'tek's health;
  - the speeds its paths go at, where the season's do not fit the arena
    (`travel`).

  A map and its NPCs are part of the base (below): a reader of the base
  alone sees every room whole, with the same NPCs in it. A curated or user
  set may still amend an arena's map, as it may amend anything, but never
  a level's. A dungeon's file also gives each of its levels, by UiMap id,
  under `[level.<ui_map>]` (A dungeon's levels, below), and its own
  creatures with their enemy forces, `[npc.<key>]` and `forces` (A
  dungeon's creatures, below). `gen-rubric` writes an instance file only
  when it is missing.
- **The journal's text** is not in the repository. Blizzard's words stay
  out, like the extracted art. `gen-rubric` writes each encounter's text to
  a sidecar on this machine,
  `$XDG_DATA_HOME/wowdps/rubric-text/<season>/<instance>/<id>-<slug>.toml`
  (`--text-dir` puts it elsewhere, never in the checkout).

Every file starts with `schema = N`, the format it is written in. A build
reads its own format and migrates older ones forward, and refuses a newer
one. A new season is a new directory. When a season changes the format,
the schema number moves and the migration is code. A new kind of file is
refused by its name rather than its number: a build from before schema 2
takes a `trash.toml` for a misnamed encounter file, and so refuses a tree
or a set holding one whole.

## How an encounter resolves

An encounter resolves through an ordered stack of sources, each laid over
the last:

1. the season's `[defaults]` (the `season` source)
2. the instance's `[defaults]` (`instance`)
3. the draft (`draft`)
4. the encounter's map and NPCs from the instance file, `[encounter.<id>]`
   (`drawing`)
5. each curated set, in the order it was laid (`curated`): a tuned file
   embedded beside its draft first (or a tree on disk's in its place),
   then each bundle laid in at runtime
6. the user's own files (`user`)
7. the difficulty overrides, `[difficulty.<name>]`, in the order the
   client's fallback chain gives them, the most general first; each
   difficulty's from every source above, in the stack's order

A curated set, or the user's, is a file tree shaped like `seasons/`, its
paths `<season>/<instance>/<file>` relative to it, laid as one block: its
`season.toml`, its `instance.toml`, then the encounter's `<id>-<slug>.toml`,
each over the last, and the whole block over everything under it. So a
user's `season.toml` with `[defaults.view]` sets the view of every
encounter of the season, over the draft and the instance's drawing (never a
dungeon's level: a level is the base's alone).
Such a set's `season.toml` gives only `[defaults]`, and its `instance.toml`
only `[defaults]` and `[encounter.<id>]` (the tables an
instance file gives): what a season or an instance is (its name, kind,
order) stays the base's. Every season and instance a set names must be the
base's, an encounter's file and drawing lie where the rubric answers the
encounter (a returning dungeon's newest season, never an older copy no
reader looks at), the file keeps the slug the encounter's other files
have, and a draft is never one of its files.

Every encounter under a `[defaults]`, the base's or a set's, a season's
or an instance's, lays it in whole, so a default gives only an
encounter's tables (`map`, `view`, `travel`, `npc`, `ability`, … `spell`)
and their `[difficulty.<name>]` overrides. Any other key, a level's
`pull` among them, would leave every encounter under it unreadable, and
is refused where its file is read, naming the file and
`[defaults.<key>]`.

The difficulty overrides come last, after every source's own values: a
draft's `[difficulty.heroic]` still wins over a curated or user file's
plain value on Heroic. To change a value on a difficulty, a layer
overrides it on that difficulty.

**Three tiers.** Each source belongs to a tier, and the tiers are ordered:

- **Base** (sources 1 to 4, with the draft's own difficulty overrides):
  what extraction gives, and the encounter's map and NPCs, with no other
  hand amendment.
- **Curated** (5): what the fight does in the room, found by investigating
  it and checking it against logs and the game.
- **User** (6): the files on this machine, laid last, so a user can
  override anything for themselves.

The base must stand alone. Every encounter's base resolves and checks clean
on its own (a test holds it, at every tier), and a curated or user layer
never adds a draft. A reader asks for a tier per encounter
(`Rubric::encounter_at`), and the rubric answers at the user tier unless
`Rubric::with_tier` caps it lower. `Rubric::layers_of` lists the sources an
encounter reads; `has_curated` and `has_user` say whether a curated set or
the user's touches it. `Rubric::encounter_traced` answers who set each key
(`Provenance`): per key path, such as `ability.serpents-bite.shape.length`
or `map.stencil`, the source that last set it and the difficulty override
it came through. An encounter whose only file is curated or the user's
answers from that tier up, and not below.

**The curated layer comes three ways.** A tuned `<id>-<slug>.toml` beside
its draft in `seasons/` reads as curated (`embedded`). This crate's
`seasons/` carries the base alone, so there are none there, but a tree
that holds them works unchanged. `Rubric::with_curated_dir(origin, dir)`
reads the tuned files of a `seasons/` tree on disk (on the replay branch,
a checkout's `curated/seasons/`) in the embedded ones' place
(`Origin::Dir`, named `origin`): a tuned file saved into the checkout (as
the editors save one) is read the next time a reader
makes its rubric, with no rebuild. Only the tree's tuned files are read.
Its drafts and its instance and season files are the base's, and the base
stays the one the binary was built with. A reader that wants the tree's
base too reads the whole tree in place of the build's with
`Rubric::from_dir(dir)`: its seasons, instances (levels and creatures
among them), drafts and drawings as the base (`base_origin` names the
tree), its tuned and trash files as the first curated set
(`Origin::Dir`), each file checked as the build's are and every
encounter, trash and level read once, any failure refusing the tree, each
error naming its file by its place on disk (a stray entry, a dot-file such
as an editor's lock file or a link to nothing, is passed over). That is
stricter than the build's reader, which leaves its files' readings to its
test: a file saved mid-work that breaks one difficulty refuses the whole
tree until it is fixed, so a writer saves each file whole (written beside,
then moved over it). `TreeFiles::read` walks a tree without parsing it, its
`fingerprint` telling a tree unchanged since the last walk, and
`from_tree`, `with_curated_tree` and `with_user_tree` lay what it read. A tree that holds the same
files as the build resolves every encounter, level and trash as the build
does. And
`Rubric::with_curated(origin, files)` lays a named bundle in at runtime: a
file set in the same `seasons/`-relative tree, laid after the embedded
tuned files (or the tree's) and every bundle laid before it. The replay
lays its embedded curated set so, named `curated`. A tree and a
bundle are laid whole or refused whole: a file it cannot lay, or an
encounter it would make unreadable, is an error naming it (a tree's file
by its place on disk).

**The user's files** live in `$XDG_CONFIG_HOME/wowdps/rubric/` (else
`~/.config/wowdps/rubric/`), a tree that mirrors `seasons/`:
`<season>/season.toml`, `<season>/<instance>/instance.toml` and
`<season>/<instance>/<id>-<slug>.toml`. `Rubric::with_user_dir(None)` reads
every `.toml` there, and `with_user(files)` lays a file set given in hand.
A file that cannot be read, parsed or laid, or that would make an
encounter unreadable, is named in `Rubric::user_errors()` and left out, and
the rest are laid: a user's mistake never takes the rubric down.

`Rubric::version()` hashes every file laid, the base's, the curated sets'
and the user's alike, so an interpretation that records it regrades when
any of them changes. A reader that records it should record the tier it
read at too.

### An instance's trash: `trash.toml`

A keystone run is its trash as much as its bosses. An instance's
`trash.toml` says what its creatures do outside the bosses' fights, in the
words an encounter's file uses, and resolves to an `Encounter`:
`Rubric::trash(map)`, by the map id a run's log writes (`trash_at` caps
the tier, `trash_traced` adds the `Provenance`; `trash_layers_of` lists
the sources with a file it reads, as `layers_of` does an encounter's, and
`has_curated_trash` and `has_user_trash` read that). Its stack is shorter
than an encounter's:

1. the instance file's own creatures, its `[npc]` (the `instance` source;
   A dungeon's creatures, below): who is in the dungeon and the forces
   each gives, the base
2. each curated set's `trash.toml`, in the order it was laid (embedded
   beside the instance's file, or a tree on disk's in its place, then each
   bundle)
3. the user's own `trash.toml`
4. the difficulty overrides a keystone reads: `normal`, `heroic`,
   `mythic`, then `keystone`'s own, each difficulty's from every source
   above in the stack's order

No season or instance `[defaults]` are laid: they hold an arena's map,
view and travel, which the trash has none of. The resolved encounter's
`encounter` is 0, its `name` and `instance` the instance's, its `only`
lists as exact as a boss's. `crate::check_trash` checks it as `check` does
an encounter, and for its clock (below); `check` itself never applies the
trash's rules, even to an encounter numbered 0.

```toml
schema = 2

[npc.stone-brute]           # one of the instance's creatures, amended by key
still = true

[ability.ember-bolt]
name = "Ember Bolt"
spell = [900001, 900002]
by = "flame-caller"         # a key of the instance's [npc]
marks = ["interrupt"]

[ground.cinder-patch]
name = "Cinder Patch"
kind = "pool"
tint = "orange"
shape = { kind = "circle", radius = 4 }
at = [{ death = { npc = "ember-imp" } }]
ends = { after = 20 }
```

A trash file gives what a run's creatures are, do and leave: `npc`,
`ability`, `place`, `ground`, `mark`, `line`, `volley`, `flash`, `knock`,
`range`, `apart`, `streak`, `reach`, `event`, `spell`, and its
`[difficulty.<name>]` overrides of them (`TRASH_TABLES`). A run's `map`,
`view`, `travel`, `phase` and `room` are its levels' and its bosses', its
`encounter`, `name` and `instance` the instance's, and it has no `order`:
any of them is an error naming the file and the key. A trash file may
amend one of the instance's creatures by key (its `name`, `role`,
`control`, `icon`, `owner`, `shares`, `still`, `aloft`, `only`, or
`enabled`, so it may
switch back on one the instance's file has off), never its `creature` ids
or its `forces`, there or under a difficulty. It may add a creature the
instance lacks, as a curated set may add an NPC to a boss, with its own
`creature` ids but no `forces`: an added creature gives none. Who the
creatures are and the forces they give stay the base's, in the instance
file, and what they do is the curated layer's.

The trash runs the whole key, so it has no clock of its own: a `"start"`
trigger or an `after` with no `since` would count from the key's start,
and `check_trash` refuses both. Time a moment from what a pack does instead:
`{ after = { secs = 3, since = { death = { npc = "ashseer-flamelasher" } } } }`.

Every instance has a trash. With no trash file it is the base alone, the
instance's creatures and nothing else, so the base still stands alone; the
embedded test resolves and checks every instance's trash at every tier.
A trash file is laid, refused and left out as a tuned file is, and
`with_curated_dir` reads a tree's trash files with its tuned ones. A trash
file under an older season's copy of a returning dungeon, where no reader
looks, is refused.

Tables merge key by key; anything else (a number, a list) is replaced.
Named entries are tables keyed by a slug (`[npc.ulatek]`,
`[ability.serpents-bite]`), so a later layer changes one by naming it.
`enabled = false` drops an entry. `only = ["mythic", "keystone"]` keeps an
entry to exactly the difficulties named. Presence does not inherit the way
values do: the journal lists an ability's difficulties exactly, and what
Heroic alone has, Mythic lacks.

**Difficulties** are named, never numbered: `normal`, `heroic`, `mythic`,
`keystone`, `lfr`, `world`, `story`, `lorewalking`, `timewalking`,
`follower`. A dungeon's Mythic (23) and a raid's (16) are both `mythic`.

| A pull on | reads, in order |
| --- | --- |
| Mythic Keystone (8) | normal, heroic, mythic, keystone |
| Mythic (16, 23) | normal, heroic, mythic |
| Heroic (2, 15) | normal, heroic |
| Normal (1, 14) | normal |
| LFR (17) | normal, lfr |

So a Heroic override reaches Mythic and keystones too, unless they say
otherwise. That is how the game's own data inherits.

Every struct refuses keys it does not know: a typo is an error that names
its file. `crate::check` reports a name an encounter uses but does not have,
such as an ability's NPC, a trigger's phase or a phase an NPC is aloft in,
a creature id two NPCs list, a color the rubric does not name (Tints,
below), and an `only` that names no difficulty (which would keep its entry
off every one).

## An encounter, key by key

As resolved, every key in one place. In the files, `[map]`, `[view]`,
`[room]`, `[place]` and the events they key on come from the draft and the
instance file's `[encounter.<id>]`, and a tuned file leaves them alone. An
`[npc]` comes from the draft, amended or added by the instance file.
Tables merge key by key, so a tuned file that made a layer break on a phase
where the instance says an event would be refused, never half-merged.

```toml
schema = 2
encounter = 3492            # DungeonEncounterID
name = "Ula'tek"
order = 8                   # the journal's boss order

[map]                       # how gen-floors draws the room
ui_map = 2610               # the UiMap the fight is posted on
floor = "ulatek"            # the render's slug in the floors cache
level = 390.2               # the arena's height, when the derived one is wrong
ceiling = 25                # yards over the level that are drawn
hang = 6                    # yards over the ground at which a placed object
                            # hangs over the walk and is left out; unsaid,
                            # nothing hangs (Areas, below)
light = "graded"            # the building's warm grade; unsaid, as the client
                            # lights it, over the map's ambient (Light tables)
exclude_groups = [79440]    # WMO group uids left out
exclude_models = [1234567]  # model FileDataIDs left out
exclude_textures = [4516750]  # surfaces drawn with these textures left out
stencil = [                 # what of the room is worth drawing, world X and Y
  { points = [[1640, 70], [1640, -70], [1510, -70], [1510, 70]] },
  { center = [1575, 0], radius = 12 },   # shapes join; a polygon's corners in
]                           # order round it, or a circle; unsaid, all of it
ppy = 16                    # pixels a yard the arena is rendered at; unsaid,
                            # the tool's 8 (a stenciled room has room for more)
crop = [1510, -70, 1640, 70]   # the world box the render covers, x0 y0 x1 y1,
                            # x0 < x1 and y0 < y1: the framing is wrong and a
                            # stencil more than wanted
include_groups = [79441]    # WMO group uids drawn though a rule leaves them out
include_models = [7654321]  # model FileDataIDs drawn though a rule leaves them out
ground = "walked"           # what a terrain level's ceiling is measured from:
                            # "terrain", "walked" or a height in yards
exposure = 1.2              # times the graded look's brightness; unsaid, 1
transition = { after = 3, unit = "travel", port = "flash", port_yd = 40, camera = "glide" }
                            # while the log is quiet (Transitions, below)

[map.layer.starting-platform]   # a piece of the room a fight takes away
wmo = 7549724
placement = 68066522        # when the WMO is placed more than once
breaks = { phase = "intermission" }   # or "occupancy" (default), "never",
                                      # { event = "shattering" }

[map.glow.flame]            # liquid the game colors and lights itself
name = "Fountain of Flame"
color = [200, 55, 22]       # red, green, blue: the flow at rest
hot = [255, 140, 50]        # what it brightens to; unsaid, `color` at full
area = [{ points = [[431.2, -271.3], [434.6, -286.9], [392.0, -295.9]] }]
                            # where it lies: a stencil's shapes, world X and Y;
                            # the liquid in it takes `color`
except = [{ center = [378.3, -285.8], radius = 6.5 }]   # shapes cut out of the
                            # area: liquid the game keeps its own color
below = 1                   # yards under the arena level a surface in the
                            # area takes `color` (a channel's floor, under its
                            # lattice); unsaid, only the liquid
on = { event = "fountains-erupt" }   # it brightens each time; unsaid, never
rise = 0.6                  # seconds it takes to brighten, holds and fades:
hold = 2                    # these are the defaults
fade = 2.5

[map.water]                 # how the render draws the floor's water (Water,
close = [18, 22, 29]        # below): red, green, blue where it is shallow,
far = [14, 20, 38]          # and at `depth` and deeper, as the floor shows it
shallow = 0.7               # how much of the bed it hides at the shore, 0–1,
deep = 0.85                 # and at `depth` and deeper
depth = 18                  # yards at which it is `far` and `deep`, above 0
bed = [51, 101, 145]        # the color the bed fades toward with depth (its
                            # murk), all of it at `depth`; unsaid, no fade
types = [924]               # the liquid types it draws, exactly; unsaid, every
                            # water whose colors are the default light's
shore = 0                   # yards over its bed at which all of it shows;
                            # unsaid, the render's 0.75; 0, none fades

[map.fade.walkway]          # a piece drawn see-through rather than left out
group = 79442               # one WMO group uid, or `model = <FileDataID>`
alpha = 0.3                 # how much of it shows: above 0, under 1; one fade
                            # a piece, and never a piece left out

[map.area.platform]         # a place the camera knows (Areas, below)
name = "The platform"
shape = [{ center = [1575, 0], radius = 40 }]   # a stencil's shapes
far = 1                     # its altitudes, times its fit: the furthest the
home = 1.15                 # camera goes, where it rests, and the closest;
near = 8                    # far ≤ home ≤ near, unsaid the view's
turn = 0                    # degrees clockwise over it; unsaid, the view's
ceiling = 25                # yards over the ground drawn over it, the lowest
                            # where areas overlap; unsaid, the map's
hang = 4                    # the map's `hang` over it; unsaid, the map's
stencil = [{ center = [1575, 0], radius = 30 }]   # where a level's render
                            # ends over it, a stencil's shapes; an arena's is
                            # checked, not drawn
encounter = 3492            # the boss fought here (DungeonEncounterID)

[map.path.from-the-corridor]   # a way between two of the map's areas (Paths)
from = "corridor"           # area keys; it goes both ways
to = "platform"
mode = "run"                # run | mount | vehicle | leap
width = 6                   # yards across; unsaid, single file
speed = 9                   # yards a second; unsaid, the season's for its mode
# spell = 1234567           # a leap's device's spell; only a leap names one
points = [[1690, 0], [1640, 0]]   # its way, world X and Y, `from`'s end first;
                            # unsaid, straight

[travel]                    # the speeds a path goes at by its mode, from the
run = 7                     # season's [defaults.travel]: yards a second on
mount = 14                  # foot, mounted, and carried (unsaid, a vehicle is
vehicle = 20                # timed to its next post)

[view]                      # the replay's first look
turn = 0                    # degrees clockwise; unsaid, north up (the
                            # game's +X at the top); `wowdps-replay-extract
                            # suggest-turns` proposes one from stored pulls
zoom = 1.15                 # times closer than the whole arena
zoom_max = 8                # the closest view, times closer than the arena
center = [1575.0, 0.0]      # world X and Y; unsaid, the arena's middle
arena = [1640.0, 70.0, 1510.0, -70.0]   # world X and Y, corner to corner: the
                            # view keeps to it; unsaid, where players went

[npc.ulatek]
name = "Ula'tek"
creature = [248711]         # creature ids (a GUID's sixth field); a unit is
                            # the NPC its creature id is listed under, else
                            # the one its name is (the spectral Ula'tek, 268029,
                            # wears her name and is an NPC of its own); two
                            # NPCs never list one id
role = "boss"               # boss | add | pet | object | ignore
# control = "stun"          # how it can be crowd-controlled: all (unsaid) |
                            # stun (stuns and incapacitates land, no full
                            # control) | immune (nothing lands)
icon = 5926310              # FileDataID (the journal portrait)
# forces = 25               # what a keystone's enemy forces gain as one dies,
                            # counted by its `creature` ids; unsaid, none (a
                            # dungeon's trash says it in its instance's [npc])
# aloft = ["stage-two"]     # the phases (keys) it is in the air through; unsaid,
                            # never

[npc.venom-coagulation]
name = "Venom Coagulation"
role = "pet"
owner = "ulatek"            # drawn with its owner

[npc.venomous-heart]
name = "Venomous Heart"
role = "boss"
shares = "ulatek"           # one health pool with that NPC: the replay shows
                            # it once, on that NPC's frame, this one's frame
                            # under it with what the pool lost since it came
                            # and the seconds until it goes

[npc.malignant-totem]
creature = [269430]
still = true                # it never moves: wherever the log places it once,
                            # it stood from its first act until it died

[ability.serpents-bite]
name = "Serpent's Bite"
spell = [1234567]           # a cast and its damage may be several ids
by = "ulatek"
shape = { kind = "cone", angle = 60, length = 35 }
                            # circle { radius } | ring { inner, outer }
                            # | cone { angle, length } | line { width, length }
lifetime = 30               # seconds on the ground
alert = 2                   # the game's timeline alerts on it, at this severity
marks = ["tank", "deadly"]  # the journal's role and warning marks
phase = "stage-three"       # the stage the journal lists it under
under = "serpents-coil"     # the ability the journal lists it under (gen-rubric)
only = ["mythic"]

[phase.stage-two]
name = "Stage Two: The Adds"
order = 2
enter = [{ plateau = { npc = "ulatek", secs = 30 } }]   # any one begins it

[phase.intermission]         # a phase the fight comes back to
name = "Intermission"       # numbered as it comes: "Intermission 2"
order = 3
enter = [{ cast = { spell = 1284588 } }]
again = true                # it begins again each time `enter` fires
leave = [{ aura_gone = { spell = 1284590 } }]   # and ends at the first of these

[place.venom-pool]          # a fixed feature of the floor
kind = "pool"               # puddle | pool | spawn | soak | …
tint = "teal"               # the color it is drawn in (Tints, below)
at = [1575.0, 0.0]          # world X and Y, as the log writes positions
radius = 30
liquid_height = 388
swirl = { texture = 5207558, colors = ["#03201a", "#7af0b8"], twist = 330, spin = -12 }
                            # a spell's swirling texture filling the circle,
                            # feathered over its outer 5%, over the floor and
                            # under its ring and label (`swirl::disc` draws
                            # it: the replay turning; the map editor will
                            # draw it still, opening the install itself):
                            # texture: a gray, tileable picture's FileDataID
                            # colors: [dark, light], each #rrggbb; the gray,
                            # leveled 5% to 95%, runs between them
                            # twist: degrees the middle turns, none at the
                            # rim, as ImageMagick's -swirl
                            # spin: degrees a second, clockwise on screen
from = { event = "shattering" }   # it opens each time this fires; unsaid,
                            # there from the start
until = { phase = "stage-three" }   # and closes at the next of these;
                            # unsaid, it stays to the end

[ground.blood-venom]        # what the fight leaves on the floor, each time
kind = "pool"               # pool (stay off) | soak (step on) | puddle | …
tint = "red"                # the color the game draws it in
shape = { kind = "circle", radius = 6 }
at = [{ aura_removed = { spell = 1288297 } }]   # each time: where its unit stood
delay = 1                   # forms this long after
ahead = 21.5                # yards in front of its unit, the way it faced as
                            # `at` fired; unsaid, where it stood
grow = "stacks"             # radius times the dropping aura's stacks
spread = 3.6                # yd a second it widens to that size once it
                            # forms; unsaid, it forms whole
preview = true              # a dashed ring under the unit whose aura drops
                            # it, with the seconds left, its last 10 s
ends = "pull"               # (the default) stays; "at": gone as `at` fires;
                            # { after = 30 }: seconds after it forms;
                            # { hit = { spell = 384024 } }: gone at that
                            # spell's first hit on someone standing in it
                            # (`lands = true`: its landing, not its dot's
                            # tick, Triggers below);
                            # { first = [{ hit = … }, { after = 40 }] }:
                            # at the first of those to come
# renew = true              # a firing inside one standing renews it (an
                            # `after` counts again from it) instead of
                            # placing another, found where its `push` has
                            # taken it; never with a `from`, a `seen` spell
                            # it is placed by, nor ending "at"
# falls = true              # gone the first moment its spot leaves the
                            # map's stencil, where its `push` or `orbit` took
                            # it; formed or last found again outside, it
                            # never falls; found again inside, it falls when
                            # it next leaves; needs a push or an orbit, and
                            # a stencil
from = { cast = { spell = 1234567 } }   # there since the last of these before
                                        # `at` (a droplet seen only as it is soaked)
# follows = { aura = 1258901 }   # a line or a cone turned each moment while
                            # it stands, from its spot: toward "target" (the
                            # target a cast's row names) or a player
                            # carrying an aura, or along "facing", its
                            # unit's as posted; unsaid, the way its unit
                            # faced as `at` fired; never a circle, nor with
                            # an `orbit` or an `ahead`
orbit = { around = "soulcoil-well", turn = -3.0 }   # circles a place or an NPC
                            # from where it formed: degrees a second,
                            # counter-clockwise from above
# push = { on = [{ cast = { spell = 1234567 } }], toward = [0, 90, 180, 270], speed = 4, secs = 3 }
                            # each time one of `on` fires (a list; one
                            # alone reads too), every one of it on the floor
                            # moves along the next bearing in turn, or with
                            # toward = "drift" the way the players drifted
                            # then, `speed` yards a second for `secs`; never
                            # with an `orbit`
seen = [1288554]            # its hits place it again at their victims
# clears = ["coalesced-venom"]   # other grounds (keys) it sweeps away as it
                            # forms: each standing then whose spot lies in
                            # it is gone from that moment; never itself, nor
                            # one that clears it back
# sweeps = ["lingering-frost"]   # all the while it stands: each whose shape
                            # it ever meets (any part of one, not only its
                            # spot) is gone then

[mark.helical-toxins]       # an aura drawn on the players carrying it
aura = 1284590
count = { tick = { spell = 1284813, each = 15867 } }   # unsaid: its stacks; or a
                            # tick's unmitigated amount over one application's
goal = 4                    # the count that clears it; past it, a failure

[mark.blighted-blood]       # a mark drawn as a ring, not a count
aura = 1284471
ring = "orange"             # a ring round each carrier in this color
label = "carried"           # "carried": seconds on; "left": seconds to go
link = true                 # a line to the nearest other carrier
soak = [1288282]            # spells whose hits as it comes off are its soakers

[mark.mark-of-blood]        # the side a player is on
aura = 1284506
dot = "red"                 # a dot on each carrier, larger with its stacks

[line.living-venom]         # a strike from where a unit stood to an NPC
tint = "green"
from = { hit = { spell = 1284451 } }   # each time: its unit's spot
to = "breath-of-ulatek"     # the NPC it runs to, or "@source": the unit that
                            # put its `from` aura on
delay = 4                   # strikes this long after; drawn faint until then
hits = [1284209]            # the spells that hit along it as it strikes

[line.acid-spit]            # to the one of several adds alike that marked
from = { aura_removed = { spell = 900011 } }   # an aura's trigger, on or off
to = "@source"              # the unit that put that aura on
hits = [900012]

[line.dread-gaze]           # held while an aura stays on, never struck
hold = true                 # from the aura's carrier, wherever it goes, until
                            # the aura comes off; never with a delay or hits
from = { aura_applied = { spell = 900013 } }   # an aura going on alone
to = "@source"              # the add that fixated them

[volley.plague-wave]        # projectiles sent out from a spot on fixed bearings
tint = "green"
from = { aura_removed = { spell = 1281913 } }   # each time: its unit's spot
# around = "alluring-bubble"   # the middle it sets out round instead: a
                            # place's spot, or an NPC's unit as `from`
                            # fires; `from` may then be any trigger
toward = [0, 90, 180, 270]  # degrees: 0 along the X the log writes, 90 its Y
# aim = "facing"            # turned with its unit's facing as `from` fires:
                            # 0 straight ahead, 90 to its left; unsaid, "world"
# spread = 26               # degrees either side each may go, turned to the
                            # hits it made; unsaid, each keeps its bearing
# spots = [[7, -3], [10, 6]]  # where each sets out: yards ahead of its unit
                            # and to its left, turned as the bearings are, one
                            # projectile for each spot and bearing; unsaid, the
                            # unit's own spot
# side = "raid"             # the spots' left mirrored to the side of the unit
                            # this NPC stood on as `from` fired, or, "raid",
                            # the side the living players' middle moved to
                            # in the seconds after
speed = 12                  # yards a second
width = 5                   # yards across: what it hits (a front's depth)
# front = 80                # yards across a wave's front, square to its way:
                            # each a wall, not a disc
# form = "crescent"         # each a crescent bowed the way it goes, bright on
                            # its leading edge, darker aft; unsaid, a disc
start = 2.5                 # yards out each one's middle sets out; unsaid, 0
length = 60                 # yards each goes; unsaid, until it leaves the room
# inward = true             # each sets out `start` yards out on its bearing
                            # and comes in toward the middle, `length` yards;
                            # unsaid, all the way in
# delay = 0.5               # seconds after `from` the first sets out; unsaid, 0
# every = 4.5               # seconds until it is sent again from the same spot
# times = 2                 # and facing, this many in all; unsaid, once
# alternate = true          # each sending from the other side than the last:
                            # its spots mirrored in turn
# holes = true              # its projectiles are slots each sending fills only
                            # some of: a hole where a living player stood in
                            # its way unhit, whole otherwise
# holes_by = "cluster"      # with holes: one hole a sending, the slots the
                            # largest cluster of the living players crossed
                            # unhit spans round its middle (a radial volley's:
                            # bearings round one middle); unsaid, "slot":
                            # every slot one was crossed in unhit
# faint = true              # with holes: one no one came near is drawn faint,
                            # one a hit lay along whole
hits = [1295798]            # the spells that hit along them
destroys = ["malignant-totem"]   # NPCs they destroy as they cross them
place_kills = true          # each such kill the log never placed drawn where
                            # a front stood as it died, as a guess; unsaid,
                            # none; never without `destroys`; a trash
                            # volley's never (no room to place them in)
preview = true              # its bearings dashed from the carrier of `from`'s
                            # aura while it waits; the projectiles as it erupts;
                            # beside another `from`, with a `delay`: each
                            # drawn still at its start from `from` until it
                            # sets out

[flash.noxious-blast]       # hits flashed on their victims
spell = [1284452]
tint = "green"

[knock.burst]               # players thrown straight out from a middle
name = "Burst"
tint = "teal"
on = { hit = { spell = 900051 } }    # each time: throws the unit it is about
from = "center-pool"        # the place (its `at`) or NPC they fly out from
within = "platform"         # a place with a radius: landing outside it is a
                            # fault; unsaid, they may land anywhere

[range.marks]               # a circle round NPCs at an aura's reach
npc = ["blood-of-ulatek", "breath-of-ulatek"]
tint = ["red", "green"]     # each NPC's color, in order
spell = 1284506             # the radius the game's tables give it; or radius = 40
sides = true                # two NPCs: the line halfway between, and the gap

[apart.dominance]           # two NPCs to keep apart
npc = ["blood-of-ulatek", "breath-of-ulatek"]
within = 25                 # yards: nearer is a fault
allow = ["intermission"]    # phases they may stand together in
grace = 4                   # seconds either side of those to split them

[streak.empowering-slam]    # a spell hitting one player until another takes it
spell = [1284458]
by = "breath-of-ulatek"
most = 4                    # more in a row than this is a fault

[room.the-well]             # a room off the arena players go to and come back from
about = "grasping-depths"   # the ability whose journal section tells of it
through = "soulcoil-well"   # the place it is entered through; unsaid, a room
                            # elsewhere, holding whoever stands in its radius
enter = [{ aura_removed = { spell = 1293214 } }]   # one of these about a player
                            # standing in that place takes them in
leave = [{ aura_applied = { spell = 1300235 } }]   # brings them back; unsaid or
                            # never, as the way closes
npc = ["drowned-echo"]      # who lives in it; the way is open while they do
radius = 21                 # yards round its middle the inset shows
# center = [206.9, 0.0]     # its middle; unsaid, the `through` place's
# ui_map = 2700             # a map floor of its own; unsaid, the arena's
# level = 496               # its floor's height for gen-floors; unsaid, the arena's
# ceiling = 20              # yards over its level that are drawn
light = "baked"             # as the client lights it; unsaid, as its map is
scale = 0.57                # its picture this many times its render's size round
                            # its middle: a phased copy smaller than the floor
                            # it copies; unsaid, 1

[reach.amani-at-well]       # an NPC that must not get to a place
npc = "restless-amani"
place = "soulcoil-well"     # one with a radius: within it is a fault
# absorbs = true            # the place takes in one that gets there and is
                            # never posted again: drawn sliding to its
                            # middle and gone, counted absorbed, not dead

[event.shattering]          # a moment a phase, layer or place keys on
name = "The Shattering"
on = { cast = { spell = 1234567 } }

[spell.1284505]             # what the game's spell tables say of a spell
name = "Mark of Blood"
period = 5.0                # seconds between its ticks
triggers = [1284506]        # the spells its effects set off

[spell.1284506]
name = "Mark of Blood"
radius = 40.0               # yards: its largest effect radius
duration = 40.0             # seconds an aura of it lasts
period = 2.0
stacks = 99                 # the most stacks it holds

[difficulty.mythic.ability.serpents-bite]
shape = { length = 45 }     # Mythic's bite reaches further; the rest stands

[difficulty.lfr.spell.1284506]
radius = 30.0               # LFR's own rows reach less far
stacks = 10
```

### Triggers

What the log shows that a phase, an event or a place keys on:

| Trigger | Means |
| --- | --- |
| `"start"` | the pull begins |
| `{ after = 90 }` | seconds into the pull |
| `{ after = { secs = 68.5, since = { cast = … }, nth = 2 } }` | seconds after the `nth` time another trigger fired (unsaid, the first) |
| `{ after = { secs = 9, since = { cast = … }, each = true } }` | seconds after every time another trigger fired, each the pull reached (never beside an `nth`) |
| `{ cast = { spell = N, by = "npc" } }` | a hostile cast lands (`SPELL_CAST_SUCCESS`) |
| `{ cast_start = { spell = N, by = "npc" } }` | a hostile cast begins (`SPELL_CAST_START`), landed or cut off; an instant cast has none |
| `{ aura_applied = { spell = N, on = "npc" } }` | a hostile aura goes on |
| `{ aura_removed = { spell = N, on = "npc" } }` | it comes off |
| `{ health_below = { npc = "x", pct = 60 } }` | an NPC's health falls under a share |
| `{ plateau = { npc = "x", secs = 30 } }` | an NPC takes no damage that long |
| `{ appears = { npc = "x" } }` | an NPC is first seen |
| `{ death = { npc = "x" } }` | an NPC dies |
| `{ hit = { spell = N } }` | a hostile spell hits (at its victim) |
| `{ hit = { spell = N, lands = true } }` | it hits as it lands, never at a periodic tick it writes after (below) |
| `{ aura_gone = { spell = N } }` | the last player carrying a hostile aura loses it |
| `{ event = "name" }` | a named event happens |
| `{ phase = "name" }` | a phase begins |

`by` and `on` are optional. A place's `until` is a trigger like `from`.

**A hit can be its landing alone.** A spell that leaves a damage-over-time
aura on its victim writes its landing and every tick after as the same
spell's hits, and the log flags neither. With `lands = true` a `hit`
trigger (or a ground's `ends = { hit = … }`) reads its landings alone
(`crate::lands`, the one reading for the replay and the editors): a hit
within 0.1 s of its aura going on or gaining a stack on its victim lands;
else, where the encounter's `[spell.<id>]` gives the spell a `period`
(the draft's, from the game's tables), a hit one period (±50 ms) after
an earlier hit of it on the same victim, while the aura from the
victim's last landing is still on (within its `duration`, where the
tables give one), is a tick and any other lands (any earlier hit, not
only the last: a second landing mid-dot keeps the ticks' cadence); else,
a spell whose aura the log writes on no one lands every time and one
whose aura it writes is a tick but at its aura's rows. Frost Orb's touch
and its 2 s dot are one id (1313448): a Mythic kill writes 158 hits and
only 33 rows of its aura going on or stacking, and the cadence tells 75
landings from 83 ticks, so a tick on someone who walks over another orb
neither ends it nor places Lingering Frost. The ticks keep their cadence
to within 25 ms (1059 of 1067 over eight pulls). A touch that refreshes
an aura already on writes no row the cut keeps (it keeps an aura going
on, gaining a stack and coming off), so one that falls within 50 ms of
its victim's cadence by chance is read as a tick: about one touch in
twenty while a dot ticks on its victim. A volley's `hits` are read as
they land by the aura's rows alone, with no period.

**A place can come and go.** It opens each time its `from` fires and
closes at the next `until` after it opened: a `from` that fires while it
is open is passed over, and a burst of `until`s (a blow's hits on the
raid) closes it at its first. Unsaid, `from` opens it at the start and
`until` keeps it to the end; a `from` that never fires leaves it off the
whole fight. While it is closed the replay draws no ring, swirl or label
for it, and a `[reach]` into it is no fault. Nymrissa's Alluring Bubble
stands from each Alluring Bubble cast until the whirlpools reach it and
it pops, its Pop! the first hit on the raid: on a Mythic kill, open from
18 s to 120 s, 128 s to 230 s, 238 s to 340 s and 348 s to the end.
`Place::windows` reads them, the replay's and the editors' one reading;
a `from` that fired before the reader's clock starts (a keystone fight,
read over its own stretch of the run) has it there from the start where
it is open still. A ground's `orbit` or a volley's `around` a place that
is not there does nothing: a ground forming then stands still, and a
volley is not sent. A room entered `through` a place, and the camera and
the inset that frame the room, still read its spot whether it is there
or not.

**A ground lands where a unit stood.** Each time one of its `at` triggers
fires, a ground appears at the unit the trigger is about: an aura's
target as it goes on or comes off, a hit's victim, a cast's caster as
the cast lands (`cast`) or as it begins (`cast_start`), an NPC as it
appears or dies. A trigger that names no one (the start, a clock,
health, a phase, an event) cannot place one. Entombed Sentinels
holds both shapes the table was made for:
- **Blood Venom:** a pool each blood venom drops as it comes off. It
  spreads to a size set by the venom's stacks, lasts 20 s on Heroic and
  stays to the end of the pull on Mythic.
- **Toxic Droplets:** soaked by stepping on them. The log shows one only
  as it is soaked, so each stands `from` the spray until it ends `at` its
  soak.

A ground's `ends` is a word (`"pull"`, `"at"`) or a table (`{ after = 30
}`, `{ hit = { spell = N } }`, `{ first = [...] }`), and tables merge key
by key. A layer may retime an `after` or name another `hit` spell, and a
word replaces either table whole or is replaced by one, but no layer, a
difficulty override among them, can switch an `after` to a `hit` or
back: the two merge into a table naming both, which is refused, as a
trigger switched from one kind to another is. To change the form, change
the ground where it is given, or switch it off and give another.

A ground can end at the first of several endings: `ends = { first = [{
hit = { spell = 1313448, lands = true } }, { after = 40 }] }`. Each is a
`hit` or an `after`: it is gone at the first hit of any of its `hit`s on
someone standing in it, else its earliest `after` past its forming. A
Frost Orb lies until someone touches it, and one no one touches shatters
on the raid 40 s after it dropped. A renewal counts its `after` again,
as a lone `after` does. The list is a list, so a layer gives it whole
again (an `after` in it is never retimed alone), and `first` merges with
another form into a table naming both, refused as above. `check` refuses
an empty list and one holding a `first`, `"at"` or `"pull"` (a ground
gone as it lands, or never, is no ending to race). `Ends::after` and
`Ends::hits` read it, the replay's and the editors' one reading.

A ground can `renew`: where one of its `at` triggers fires inside one of
it still standing, each one the firing lands in is renewed and nothing
new is placed. Standing is formed and not yet gone, so a firing inside
one still in its `delay` places another. Inside is where a new one would
land (`ahead` of its unit where it says so), within the footprint as it
is drawn then: a circle at its radius after `grow` and `spread`, a line
or a cone along its bearing, each where its `orbit` and its `seen` hits
have taken it. An `{ after = s }` ending counts again from that firing,
as one the firing placed would; a `hit` ending and the pull's are kept.
A renewal is not a new ground forming: the ground keeps its footprint as
it first formed (`grow = "stacks"` at its first firing's stacks, its
`spread` not begun again). The replay reads the firings in time order
across every `at` trigger, the ground's `seen` and ending hits between
them, each against the grounds as it found them (an ending hit at a
firing's moment before it, a `seen` hit after), so a firing on the spot
of one a hit has burst places another. Sszorak's Caustic Residue is
placed only where it hurts someone, a tick a second on each player
standing in a puddle: a tick inside a standing pool is that pool, and one
past every pool's edge starts another, each standing from its first hurt
to the pull's end, so a spot is a pool or a few, not one per tick. A
renewing ground that is also pushed is read where its pushes have taken
it by each firing: the pushes do not depend on the grounds, so each
ground is given the ones under way or to come as it forms (one under way
for the rest of it) as it is placed,
and a tick inside a puddle the wind has blown across the platform renews
that puddle rather than starting another at every step of its way. One
that has fallen off the stencil (`falls`) is gone, and a firing where it
would have been places another.
`check` refuses `renew` beside a `from`
(one would stand before the firing that places it), `ends = "at"` (each
is gone as it lands, so none stands to renew) and a `seen` spell one of
its `hit` triggers places it by (each such hit would drag the ground to
its victim, so it would follow the players).

A ground can land `ahead` of its unit, along the way the unit faced as
its trigger fired: each of Ula'tek's Spectral Coils crushes the floor
21.5 yd in front of itself, where its soakers stand (their middle 20.3 to
22.5 yd ahead and none aside, over 3700 soaks).

A ground at a cast's start stands where its caster stood as the cast
began, and a line or a cone lies along the way it faced then. A start
writes no position, so the replay reads it off the caster's track as it
draws the caster then: part-way to its next post where the posts either
side are under 1.5 s apart, its last post otherwise; a caster the log
posts only as the cast lands is held from the start where and as it
landed. A caster posted during its cast that turns to its target after
the start leaves such a cone the way it faced before: of a Mythic
kill's ten Mutilates, Sszorak's 3 s frontal, the soak lands in three
cones placed at the starts and in all ten placed at the landings. Such
a frontal is drawn while it is cast, aimed where it lands, as a ground
standing `from` the cast's start `at` its landing, gone as it lands:

```toml
[ground.frontal]
kind = "soak"
shape = { kind = "cone", angle = 70, length = 25 }
at = [{ cast = { spell = 1234567, by = "boss" } }]
from = { cast_start = { spell = 1234567, by = "boss" } }
ends = "at"                 # unsaid, it stays the pull; { after = 3.5 }
                            # counts from `from`, the cast's start
```

A line or a cone can follow a unit while it stands (`follows`), turned
each moment from its spot rather than lying the way its unit faced as its
trigger fired. Its spot stays where it formed (or where a `push` or a
`seen` hit takes it); only the way it points moves. `follows = "target"`
points it toward its cast's target: the unit a cast was aimed at, as it
lands or as it starts, where the log's row names one. (An aura's carrier
and a hit's victim are the unit the ground lands on, so they are no
target to follow.) `follows = { aura = N }` points it toward a player
carrying aura N (the log's hostile auras on players alone): the first it
goes on from the firing, within a second, else the latest to take it
still carrying it then. `follows = "facing"` lays it along its unit's
facing as the log posts it, a frontal its caster turns while it stands.

The unit is picked as the ground's trigger fires and kept for as long as
the ground stands: a taunt swap, or the aura going on another player
after, is not followed, and a ground renewed (`renew`) keeps its first
firing's unit. The unit is read as the replay draws it: between two posts
under 1.5 s apart, part-way; past that, at its last post, so where the
log stops posting it the ground holds the way it last pointed, and a unit
that dies is followed to where it fell. Where the unit stands on its spot
(within a tenth of a yard) it points toward the unit's latest post off
the spot since it formed; before the unit is posted, or where no one is
found to follow (a cast row naming no target, an aura no one took, a unit
the log never posts), it keeps the way its unit faced as its trigger
fired. What it covers is where it points, for whoever stands in it, a
`renew`, a hit it ends on and a `sweeps`: the map and the sweep read the
one footprint.

Say a boss stands still and blasts the tank for 5 s with a beam, and the
beam follows the tank as the tank is moved round her. The log's cast row
names no target, but the beam's aura on the tank goes on with the cast
(the ids here are made up):

```toml
[ground.tide-beam]
kind = "pool"
shape = { kind = "line", width = 5, length = 50 }
at = [{ cast = { spell = 900021, by = "tide-caller" } }]
ends = { after = 5 }
follows = { aura = 900022 }    # the beam's 5 s aura on the tank
sweeps = ["frost-patch"]
```

`check` refuses `follows` on a circle or a ring (no way to point), beside
an `orbit` (it turns one way) or an `ahead` (it would turn about the spot
ahead of its unit, not the unit), `"target"` where one of its `at`
triggers is no cast or cast start (an aura's carrier, a hit's victim and
an NPC that appears or dies are the unit it lands on) and an aura of
spell 0. `follow` holds the reading the replay and the editors share:
`bearing` (the way from a spot to a point, none within a tenth of a yard),
`aim` (the way it points at a moment, from the unit then, its earlier
posts, and the way it formed) and `carrier` (whom an aura names).

**A mark is drawn on whoever carries its aura**, with its count: the
aura's stacks, or, where the log writes the aura but never its stacks,
what its ticks say (`count = { tick = … }`: a tick's amount before
mitigation over one application's; `each` unsaid, the pull's smallest
tick is one). With a `goal`, the replay draws the goal's slots, the count
filled, and shows a clear at the goal and a failure past it: Entombed
Sentinels' Helical Toxins, 1–3 applications per player that two players
clear by colliding into exactly 4.
Two who meet are joined by a line for a moment: green at the goal, amber
under it, red past it.

A mark that matters for being on, not for how much, is a `ring` instead:
Blighted Blood, with the seconds it has been on (`label = "carried"`) until
a healer takes it off; Unstable Miasma, with the seconds to its burst
(`label = "left"`) and, as it bursts, how many its `soak` spells hit;
Shifting Protovenom, whose carriers clear it by touching, each joined to
the nearest other (`link`). A `dot` is a side: each player's Mark of Blood
or Mark of Acid, larger with its stacks. Where the game's tables give the
aura a duration, a ringed mark carried that long was never taken off.

**A range is a circle round NPCs** at the reach of an aura they pulse, its
radius the game's for its `spell` unless a `radius` says otherwise, round
every unit of each NPC standing, as each pulses its own (The Coiled
Altar's axes, ten a kill, ten rings). With `sides`, two NPCs also get the
line halfway between them, between the unit of each the replay places the
NPC by: the Sentinels' Marks, each marking everyone within 40 yd, split
the raid along it. An
`[apart]` pair is two NPCs nearer than `within` outside the phases it
`allow`s (and `grace` seconds either side of each): Ula'tek's Dominance.
A `[streak]` counts a spell hitting the same player again and again until
another takes it, a tank swap, and a run past `most` is a fault.

**A ground can move where the log never follows it.** With an `orbit` it
circles a place (or an NPC) from where it formed, `turn` degrees a second.
With `seen`, each hit of those spells places it again at its victim: the
standing one drawn nearest, within 20 yd. Nek'zali's Latent Cultists rise
where a dispelled Essence Rend came off, circle the Soulcoil Well at about
3 degrees a second, and are found again whenever they hurt someone.

A ground can be pushed instead of circling: each time one of its
`push`'s `on` triggers fires, every one of it on the floor moves along the
next of its `toward` bearings in turn (the first firing along the first,
round again after the last), `speed` yards a second for `secs` seconds.
`on` is a list of triggers, and one alone reads as a list of one; it is
written as a list. Every firing of every one is a push, taken in time
order across them, and one within a second of the last counted is that
push (a hit on five players is one), so the bearings go round by the
pushes, whichever trigger fired each. One that forms while a push is
under way is moved for the rest of it (a puddle a tick lands mid-gale is
blown with the gale from there), and one formed after a push is over
takes the next alone. The bearings are the world's, read as a volley's
are: 0 along the X the log writes, 90 along its Y. The log never places a
pushed ground either, so it is drawn as inferred, and `seen` still finds
it anew. `check` refuses a push on no trigger, with no bearing, a speed
or a time not above 0, and one beside an `orbit`: a ground does one or
the other.

A wind the log never writes has one witness: the players it moves. With
`toward = "drift"` each push goes the way the living players drifted
over it, from its firing for its `secs` (`drift`, the one reader for the
replay and the editors): their places every half second, each read
between their posts either side where those are under 1.5 s apart; the
mean of their steps, a step whose raid speed is over 8 yd/s left out as
a knock (a cyst bursting under them, the rush onto one), the bearing
that mean's, never snapped to a set of bearings. A push whose drift reads
nothing moves none. Say a boss's storm brings three gales of about 6 s
each, at 2, 7 and 12.5 s after its cast, each a different way each time;
the slick it leaves is pushed on three `after` clocks that count from
each cast (`each`), toward the drift (the ids here are made up):

```toml
push = { on = [
    { after = { secs = 2, since = { cast = { spell = 900100, by = "windlord" } }, each = true } },
    { after = { secs = 7, since = { cast = { spell = 900100, by = "windlord" } }, each = true } },
    { after = { secs = 12.5, since = { cast = { spell = 900100, by = "windlord" } }, each = true } },
], toward = "drift", speed = 3, secs = 6 }
```

A ground that `falls` is gone the first moment its spot lies outside the
encounter's map stencil, where its push or its orbit has taken it: a
puddle blown off the platform. Its end is the first of that moment and
the one it would have had, and no renewal lasts past it. Each `seen` hit
that finds it again is a fresh witness of where it is: formed or last
found again outside the stencil (in a room off the arena), it never
falls; found again inside, it falls when it next leaves. `check`
refuses `falls` on a ground with neither a push nor an orbit, and where
the encounter's map has no stencil: a trash file gives no map, so its
grounds never fall.

A ground can clear others as it forms: each ground of the keys its
`clears` names standing then whose spot lies inside its footprint is gone
from that moment. The Coiled Altar's Sever, Soul Sever and Blighted Sever
are tank frontals that destroy the Coalesced Venom orbs they land on, and
the log writes no row for an orb so destroyed: without `clears` an orb
stands until its pickup or the pull's end. Forming is its `from` moment,
after its `delay`; a renewal is not one. Standing is formed and not yet
gone, and the footprint and the spots are each as it is drawn then
(`grow` and `spread`, the bearing, where `orbit`, `push` and `seen` have
taken it). A volley's `destroys` is the same between a volley and NPCs.

A ground can sweep others all the while it stands, not only as it forms:
each ground of the keys its `sweeps` names is gone the first moment its
own shape meets the sweeper's footprint while both stand, each as drawn
then. That is any overlap, not its spot alone as `clears` reads it: a
puddle 8 yd in radius goes as the jet's edge reaches its rim, its middle
never under the jet. A still one under it as it forms is gone then, one
formed under it is gone as it forms (and a firing there places another,
gone at once), and a moving or turning one, either way round, is gone
where they first touch (looked for every 0.1 s, then to the
millisecond). Nymrissa's Water Jet, a 6 s line that follows the tank,
washes away the Lingering Frost it turns across. (`sweeps` first
read a swept ground's spot alone, as `clears` does; with only that tune
using it, the key changed its meaning rather than another key joining
it.)

The replay reads a ground after every ground that clears or sweeps it,
and each clear is a moment in its own reading, in time order with its
firings and hits (a clear at a millisecond after its own firings and
hits there): a ground that `renew`s finds one cleared gone, so a firing
on its spot after the clear places another, and a `seen` hit after it
finds a neighbor still standing. `check` refuses a name that is no
ground of the encounter (an NPC's among them), the ground itself, and a
ground that clears or sweeps this one in turn, by any way round: each is
read after those that clear it. A name may be any ground of the
encounter's file on any difficulty, and one the difficulty does not have
(kept to others by `only`, or switched off) clears nothing: Mythic
Keystone lays the `[difficulty.mythic]` overlays but has no Mythic-only
ground. A name that is no ground on any difficulty is refused on each.

**A room is somewhere off the arena that players go to and come back
from.** The log writes no heights and, for Nek'zali's well, no change of
map: the players in the well keep the arena's map and yards, phased onto
its own floor round the well. So only the way in says who is there. A player goes in when one of the room's
`enter` triggers fires about them while they stand in its `through`
place: Grasping Depths coming off a player in the well, when the raid's
own removal finds everyone far from it. They come back at the first of
its `leave` triggers about them (Soul Exhaustion), or as the way closes.
The way is open while one of its `npc` lives: a Drowned Echo, which the
log never kills. It is beaten at 0%. A room with no way in (one an
encounter ports players to, on yards of its own) holds whoever stands
within its `radius` of its `center`. The replay draws a room's players
and NPCs in a round inset of its own, never on the arena, and
`gen-floors` renders it apart, three times denser, from its own `ui_map` or
the arena's, at its own `level` and `ceiling` (unsaid, the arena's), lit
as the client lights it but over no map ambient: the well's phase is dark
navy where the arena is teal. A `scale` draws its
picture smaller round its middle: the well's phase floors over the arena's
mouth with a smaller copy of its medallion, whose dark middle the divers
stand at the rim of, 4.3 yd out where the arena's mouth ends at 7.6. Its `about` names
the ability whose journal section tells of it, and the replay's help for
the room is that section rolled up: the ability, then every ability
`under` it, nested as the game's journal nests them (Grasping Depths;
Immortal Coil; the Drowned Echo's Soulcoiler's Curse; Swirling Spirit).

**Every floor is lit as the client lights it** unless its `[map]` says
`light = "graded"` (the building's warm grade, matched by eye to The Coiled
Altar before there was more to go on): a building's indoor groups by the
light baked into their vertices and its props by their placements', the
ground, outdoor groups and props on it by the sun on their facing, all
over the ambient and sun of the light the game finds there (`ZoneLight`
polygons, `Light` spheres, the map's own, its cosmetic parent's).

**The journal's marks are read too.** An ability the journal marks
`interrupt` gets a lane of its casts in the replay, each kicked (and by
whom), landed (a fault), or cut short: the Drowned Echo's Soulcoiler's
Curse, kicked 10 times of 13 on the kill and never landed.

A `[reach]` is an NPC that must not get to a place: Restless Amani walking
on the Soulcoil Well. One within the place's radius is a fault. A place
that `absorbs` takes it in: one that got there and that the log never
posts again, its last post alive and within 3 yd of the circle with no
death after it, was drawn in, and the replay draws it sliding to the
place's middle and gone, counted absorbed rather than dead. One last
posted in the pull's last 5 s is not: the log stops posting every add as
a wipe or a kill ends the encounter, so it may only have outlived the
log. Nor is one last posted while the place is gone, where it comes and
goes: it reached the place while it was there, and is taken in only if
the place is there still at its last post. Nymrissa
Wavecaller's Alluring Bubble draws in the murlocs that reach it, each
last posted alive 8 to 11 yd from its middle with no death row; without
`absorbs` one reads alive, then dead when the log forgets it. Only one
that came within the radius reached the place, so the radius says who
is taken in.

**A line strikes between a spot and an NPC.** It starts where its `from`
trigger's unit stood and runs to its `to` NPC, wherever the NPC is. It is
drawn faint until it strikes `delay` later, and then its `hits` flash on
the players along it: Living Venom, a soaked droplet's slime going back to
Breath of Ula'tek. Where several of its NPC stand as it strikes, it runs
to the one the log posted last: on Mythic a spectral Ula'tek pours Toxic
Incubation at the Blightscale Wretch she tore down most lately, while the
one before still stands. Where several alike stand and the strike comes
from the one that put an aura on, `to = "@source"` runs it to that unit:
where three adds alike stand at once, each one's mark comes off its
target as the add that put it on lands its cast. The source is the one the aura's row names as `from`
fires; a row naming none (applied, dose or removed) takes the one the
latest applied or dose row naming one named while the aura stayed on that
unit, so two applications of it on one unit share it and the later
applier wins. A firing with neither draws no strike. Only an aura's
trigger (`aura_applied`, `aura_removed`) has a source, so `check` refuses
`"@source"` beside any other `from`; it refuses any other `to` starting
with `@` too, rather than look for an NPC by it. A line can `hold`
rather than strike: drawn from the unit its `from` aura went on, wherever
that unit goes, to `to` for as long as the aura stays on it, gone as it
comes off (at the pull's end where it never does), steady and faint. The
Coiled Altar's Unnerving Fixation goes on a player from the Manifestation
of Dread that fixates them, which they must face: with `to = "@source"`
it runs to that ghost, which the log posts once as it forms, so to where
it formed. `check` refuses `hold` beside a `from` that is no
`aura_applied` (only an aura going on stays on), a `delay` or `hits` (it
never strikes). A `[flash]` is a spell
whose every hit flashes on its victim. One that hits most of the raid at
once reads as one blow to the raid: Noxious Blast, a droplet left
unsoaked, whose spot the log never writes.

**A knock throws players straight out from a middle.** Each time its
`on` trigger fires, the unit it is about (a hit's victim, an aura's
carrier) is thrown out from its `from`: a place's `at`, or where the log
has an NPC's unit then. The log writes the blow and keeps posting each
victim through their flight, but never says a throw was one, so the
replay reads each throw off its victim's track: from where they stood as
it fired until they stop going out from the middle (their speed away
from it under 4 yd a second, after it was over 8), at most 2.5 s later;
a victim the log posts next only past that is read by how far out that
post finds them. Their speed alone
would not do: a player thrown out runs straight back in, as fast as they
flew. One that never goes out that fast, or less than 3 yd, was not
thrown (a knockback immunity, a root, a blink back in first). A position-only reading of the
log would take a blink, a leap or a roll for one, so a knock is said, not
found. With `within`, a throw landing outside that place's circle is a
fault: Nymrissa Wavecaller's Pop! bursts at the island's middle and
throws everyone it hits about 30 yd out on Mythic, 20 to 27 on Heroic and
Normal, and a player standing too far out lands in the lagoon, where the
Shark bites. `check` refuses a trigger about no one, a `from` that is
neither a place nor an NPC, and a `within` that is no place with a
radius, each on a difficulty that has the knock: a place kept to other
difficulties by its `only` is none there, and a knock landing within
one would land no throw out. Firings more than a second after a
knock's first are another knock, so a straggler hit (a player the blow
reached late) is a knock of one. A place that comes and goes (`from`,
`until`) is a knock's while it is there: thrown `from` one, a knock is
sent only while it stands, the moment it closes counted in (the burst
that pops it, its `until`, is thrown from it), and a throw landing while
its `within` place is gone is no fault. Thrown from an NPC, the middle is
where the log last posted its unit as the knock fired, however long
before (an NPC the log seldom posts throws from where it last was).

**A volley crosses the floor.** Each time its `from` trigger fires, a
projectile sets out on each of its bearings from where the trigger's unit
stood, `start` yards out, at `speed`, and goes its `length` or, unsaid,
until it leaves the room (the map's stencil, else its render). The
bearings are fixed to the world, never the unit's facing: Vashnik's Plague
Froth sends a Plague Wave to each cardinal point from every carrier as it
runs out, and 114 hits on three pulls all lay within 2.6 yd of those lines.
A hit of its `hits` is the volley's whose projectile was passing its
victim, and with `preview` its bearings are dashed out from the carrier
while the aura is on; the projectiles appear only as it erupts. An NPC of its `destroys` that
dies while a volley is out is its kill: on Mythic the waves destroy the
Malignant Tumors they cross, which the log never places, only kills.

**A volley can place its kills (`place_kills`).** A kill the log never
placed died where one of the volley's fronts stood as it died: each
projectile of each sending out then (every one, not only the earliest;
never a hole, a slot its sending left out) was its `start` plus its
`speed` times the seconds since that sending set out along its bearing,
so each gives a candidate, unless the projectile is past its reach (its
`length`, or where it leaves the room, and its width more while it
fades). The candidates outside the room (the map's stencil, else its
render) are dropped, and of the rest the pick is the one farthest from
every spot of the same NPC still standing then, posted or placed (first
acted by the death and not dead before it: a unit the log posted at its
spot, a kill already placed), ties going to the earliest sending, then
its lowest projectile; the kills are placed latest death first, so those
still standing are placed already, and a kill with no candidate inside
is placed nowhere. No grid or table: the game's scripted spots are not
the same from pull to pull. The replay draws each from its first act
(its cast's start) to its death as a dashed gray ring, never solid and
never in the colors of an NPC fighting the raid, and says on its card:
the count, the moment and the distance are the log's; the spot is
picked. A unit the log did place keeps its own spot and its solid disc.
A trash rubric's volley places none: a keystone run's trash has no room
of its own to place them in, which the replay says. `guess` is the pick,
pure geometry, so the editors draw the same guesses; `check` refuses
`place_kills` with no `destroys`. On a Mythic kill of Vashnik, 96 tumors
die while the waves are out, each with 10 to 20 candidates inside the
room, and every one is placed.

A volley can be aimed by its unit instead (`aim = "facing"`): its
bearings turn with the way the unit faced as `from` fired, and its
projectiles can set out from `spots` about it rather than its own spot.
Ula'tek's Caustic Waves cross her platform at 9.8 yd a second as nodules,
drawn as crescents bright on their leading edge (`form`). In Stage Three
she sends a row the way she faces, straight across her front: 139 hits on
16 pulls fit it at r² 0.99. Its nodules leave holes that change every
wave, which the raid stands in, and a nodule goes on through whoever it
hits; the log writes neither nodule nor hole, so the row is eight slots
across her 80-yd front (`holes`, `faint`): a slot is seen where a hit lies
along it, a hole where a living player stood in its way unhit, and faint
where no one came near (on 66 pulls' rows, 93 slots seen, 159 holes, 156
unknown). Each lesser tail rising from the venom faces her and sends
nodules up to 26° off the line to her, which ways the log never writes.
A `spread` finds them: the volleys whose ways the log fixes claim the
hits along them first, and a volley with a `spread` takes what they
leave, a projectile on each bearing those hits lie on (more than 6°
apart, two in 50 tail casts, three in 6), turned to the median of its
hits, or kept on its bearing when it hit no one. Her waves' hits are
hers, the rest the tails'. A spell that hits and then ticks under one
id, as Caustic Waves does, is the volley's only as it lands: with its
aura going on or gaining a stack.

A volley can close on a middle of its own. With `around`, it sets out
round a place's spot (or an NPC's unit as `from` fires) rather than
`from`'s unit, so `from` may be any trigger, one that names no one among
them; with `inward`, each projectile sets out `start` yards out on its
bearing and comes in toward the middle, `length` yards or, unsaid, all
the way in. Its way is the outward one turned round (`Volley::ways`, the
one geometry for the replay and the editors), so a hit along it, a hole
and a kill are read as along any other. A `preview` beside a `from` that
is no aura coming off, with a `delay`, draws each projectile still at
its start from `from` until it sets out, and a hit there in that time is
its. Nymrissa's Swirling Whirlpools form on the lagoon's rim as her 4 s
cast lands, 52-84 yd from the Alluring Bubble (eight hits on eight pulls
in the 1.5 s after it), then 4.3 s later close on it at about 15 yd a
second (38 hits 6.5-8.5 s after the landing, r² of their line 0.85),
reaching its rim, 11 yd out, 8.4 s after the landing, as Pop! bursts at
9.0 s. The raid stands in the one gap of the ring, 25-35° wide and at
another bearing each time; its whirlpools' hits fall just outside it. A
ring of slots with `holes` draws that gap where the living players stood
in a slot's way unhit. Every such slot is a hole, so a player crossed
unhit away from the raid (a paladin under Divine Shield) made a second
gap; `holes_by = "cluster"` keeps one a sending. The living players its
slots crossed unhit are clustered by their bearings round its middle, a
gap of more than one slot's spacing between two splitting them (the
raid stacks in its gap, so neighbors there lie a few degrees apart); the
cluster of the most players is the hole (on a tie, the one farther from
the slots a hit lay along, then the first round from the volley's 0):
the slots from the one its first player stands nearest, by bearing,
round to the one its last stands nearest, so the hole is as wide as the
raid stood; one a hit lay along among them where its bearing lies inside
the players' span (the slots close up near the middle, so a hit there
lies along several), not at the run's end past it; and a player crossed
unhit outside it makes none. A slot that only crossed it is not the
hole's: near the middle a player is crossed by several neighbors either
way, which would widen the hole far past the gap. `check` refuses
`"cluster"` with no `holes`, and `"slot"`, unsaid, is every slot as
before.

`"cluster"` is meant for a radial volley: one set out round one middle
(`around`, or a unit's spot) on bearings spaced about evenly, as a ring
is. An uneven `toward` with two bearings close together shrinks the
spacing that splits the clusters to that pair's gap, and a volley sent
from `spots` or along parallel ways has bearings round its middle that
say little of where its slots lie:

```toml
[volley.closing-rings]     # the ids here are made up
from = { cast = { spell = 900041, by = "tide-caller" } }
around = "center-pool"
inward = true
toward = [0, 20, 40, 60, 80, 100, 120, 140, 160, 180, 200, 220, 240, 260, 280, 300, 320, 340]
speed = 12
width = 8
start = 60
length = 50
delay = 3
preview = true
holes = true
holes_by = "cluster"
hits = [900042]
```

A ring's slots are discs (or walls a fixed `front` across) on their
bearings, so they crowd together as they close and overlap near the
middle; slots that are arcs of the ring, each its share of the turn
whatever its distance, are not drawn yet.

Round a middle, the firings of `from` within a second of the last
counted are one, so an aura going on twenty players at once sends one
ring, not twenty; a `preview` that holds holds the first of its sendings
alone (its `delay` is the wait), and `every` sends the rest on from
there. `check` refuses `inward` beside a `spread` (a spread turns a way
about where it sets out, the rim, so it would no longer close on its
middle), and `aim = "facing"` or a `side` round a middle whose `from`
names no unit to face or keep a side of.

A volley can be sent again: `every` seconds, `times` in all, from the spot
and facing its trigger gave. In Stage One Ula'tek sweeps a raised wing
forward across her front, and six nodules flow from it, outside in, the
outer tip leading the row by 15-20°, the innermost just across the
platform's middle. On Heroic a channel sends two rows, half a second in
and 4.5 s later (`delay`, `every`, `times`), each from the other wing
than the one before, always (`alternate`). The log never writes which
wing goes first, and the rows hit too few to tell it (most of Stage One's
hits are the tails' nodules converging on the raid); the raid tells it,
stepping toward the raised wing in the seconds after the cast, so the
first row's spots keep to the side the players' middle moves to
(`side = "raid"`). The rows cross the band the raid stands in and mostly
miss it, so they have `holes`: a nodule dropped where a living player
stood in its way unhit, the rest drawn whole. Stage Three's row, whose
gaps are many, adds `faint`: a slot no one came near is drawn faint, one
a hit lay along whole. Where several volleys name a
hit's spell, it is the one whose projectile was passing its victim,
nearest of them all (those without a `spread` first), so no hit is
claimed twice.

**A still NPC stands where the log placed it, all its life.** A totem
that never moves and that the log places once, as it finishes a cast,
stood there from its first act (its cast's start) until it died. A
Malignant Tumor is placed only as it finishes Malignance, 85 s after it
rose: so the tumors the log places are exactly the ones the waves
missed, and those are drawn solid. The ones the waves cleared are drawn
dashed where the waves' `place_kills` puts them.

**A glow is liquid the game colors and lights itself.** `gen-floors`
colors the liquid in its `area` and the surfaces there that lie further
than `below` under the arena level, at their own light so a texture stays
one: Vashnik's channels, whose flow is no geometry at all, only the
groove's floor two yards under the arena between the bars of the lattice
over it, which keeps its own color, and venom-green liquid in their last
yards to the Cavity. Its `except` shapes are cut out of the area: the
green pools beside each snake's head, which the game keeps green. It
writes the glow's shape as a picture of what it brightens to (`hot`, at
each surface's light) and the replay eases it in over `rise`, holds it and
fades it each time `on` fires: the fountains erupting.

**Phases are entered in `order`.** A phase's triggers are looked for only
once the phase before it has begun, so the same trigger can begin two
stages in turn. Ula'tek's Stage Two and its Intermission are each "a
30-s stretch with no damage to Ula'tek": the first such stretch, then the
next.
A phase that comes `again` begins each time `enter` fires and ends at the
first of its `leave` triggers, numbered as it comes: Entombed Sentinels'
four intermissions each begin with Vitriolic Stasis and end as the last
pair clears its Helical Toxins (`aura_gone`).

**A layer breaks at one time its event happened.** An event can happen
again and again, and each time breaks one of the layers keyed to it, each
layer once, so Ula'tek's four quarters can all name Circling Prey and each
go with its own cast. Where a unit casts the event (`cast` or
`cast_start`) and the log posts it as it does, that time breaks the
standing layer whose middle is nearest the caster: Ula'tek casts each
Circling Prey from just off the outer corner of the quarter it breaks. A
time with no caster, or one the log did not post then, is read from where
the players stood: a layer they stopped standing on takes the free time
nearest to when they left it, one they never left the first free time,
and it stands when none is left (the quarter the raid kills Ula'tek on).
A layer made of other layers (the whole platform over its quarters)
breaks the first time its event happens, and its pieces come only then:
before, the whole is the floor alone. An event that never happens leaves
the reading from where the players stood.

**What the log never writes can still be timed.** A scripted moment with
no line of its own is an `{ after = secs }` event, measured over every pull
of a fight that runs on a fixed clock. Where the clock differs, by
difficulty or by a few seconds a pull, it is timed from a moment the log
does write: `since` names the trigger and `nth` which time it fired.
Ula'tek's Shattering, which flings the raid as the platform cracks, comes
68.5 s after her second Rage of the Shackled lands on every pull, Heroic
or Mythic, though Mythic's clock runs 10 s later and one Mythic kill's
2.3 s earlier still. With `each = true` in place of an `nth` it strikes
after every time its `since` fired, each one the pull reached: a
moment that comes again with a stage that does. A later layer that gives
the clock `{ secs = … }` retimes it and keeps what it counts from; a bare
number replaces it. Layers merge the table key by key, so `check`
refuses a clock that ends with both an `nth` and `each`, whichever layer
gave each.

**The draft carries the game's numbers; this machine keeps its words.**
`[spell.<id>]` is written for every spell an ability is, every spell its
journal text names, and what those set off, two levels deep. The base rows
give it.
A difficulty with rows of its own, read through the client's fallback
chain, says only what differs, in `[difficulty.<name>.spell.<id>]`:
Entombed Sentinels' Marks reach 40 yd, and 30 on LFR. A tuned entry takes a
radius or a duration from here rather than a measurement. An aura that
lasts until it is taken away has no `duration`. A radius of 300 yd is the
whole room: a spell that hits everyone. A size the game keeps elsewhere
(an area trigger's) is not here.

An ability's `text` is what the journal says of it: its spell's
description, then what its listing adds, the `$` numbers filled in from
the spell tables. An ability the journal does not list, only the timeline,
has its spell's description. It is never written into a committed file. The
sidecar holds it in a draft's shape, `text` alone: `[ability.<slug>]` for
the reading where the ability first appears, and
`[difficulty.<name>.ability.<slug>]` where a difficulty reads otherwise (a
Mythic branch, an "On Mythic difficulty" note shown off Mythic, Mythic's
own numbers). A reader lays it over an encounter with
`Rubric::with_texts(text::dir())`; without the sidecar, `text` is unsaid.
Damage numbers are the tables' base points, which the game scales: compare
them, do not quote them.

### Tints

An entry's colors are the rubric's own names, which the replay paints in
its theme's hues: green, red, blue, orange, teal, cyan, purple, pink,
gold, yellow, amber, white and ice (`tint::NAMES`, `tint::known`). They
are what a place's, a ground's, a line's, a volley's, a flash's and a
knock's `tint` say, each of a range's, and a mark's `ring` and `dot`.
`check` (and `check_trash`) refuses any other name, exactly as written: a
misspelled or capitalized color is an error, where a reader would draw it
as the foe's. A place's `swirl` is the one exception: its two `colors` are
`#rrggbb`, the game's own texture leveled between them in every theme,
and `check` refuses any other form.

## A dungeon's levels

A raid's boss is fought in one room, drawn by its encounter's `[map]`. A
keystone run goes through a dungeon's levels, each a map floor of its own
(a UiMap), most of it far from any boss. The instance file gives each
level by its UiMap id:

```toml
[level.2097.map]            # the same keys an encounter's [map] has
floor = "algethar-academy-L1"   # the level's render in the floors cache

[level.2097.map.area.vexamus-court]
shape = [{ center = [1818, -2779], radius = 32 }]
encounter = 2562

[level.2097.map.area.arrival]
shape = [{ center = [1995, -2900], radius = 15 }]

[level.2097.map.path.arrival-to-vexamus]
from = "arrival"
to = "vexamus-court"
points = [[1995, -2900], [1890, -2860], [1818, -2779]]

[level.2097.view]           # what the replay shows while no area holds
zoom = 1.15                 # the party

[level.2097.travel]         # this level's speeds, over the season's
run = 8
```

A level resolves as an encounter does, from fewer sources: the season's
`[defaults]`, the instance's, then its own `[level.<ui_map>]`, the
defaults read for its `map`, `view` and `travel` alone; its `pull`s
(Pulls, below) are its own. No difficulty applies to a
level. Its `map.ui_map` is its key, and a map naming another is an error.
`Rubric::level(ui_map)` answers one and `Rubric::levels()` lists them; a
returning dungeon's newest season gives its levels, and two instances of
one season giving a UiMap is an error. `crate::check_level` checks one as
`check` does an encounter: a glow or a layer naming an event or a phase
names one the level does not have; and its pulls.

A level holds the same `Map` an encounter's does, so a reader draws a
raid's arena and a dungeon's level with one type. A raid's arena keeps its
`[encounter.<id>.map]` and `.view` and takes the same keys inside `.map`:
Vashnik's hall is one area, its shape the stencil's and its altitudes the
view's. An area's key may be an encounter's and a level's both; they are
different tables.

**A level is the base's alone.** The base gives every map whole: an
arena's in its draft and its instance file's drawing, a level's in its own
table, with their views, travel and areas. A level lives in the base's
instance file alone: a curated or user set's instance file giving one is
refused (`a level is the base's alone`), and a set's `[defaults]` (its
`map`, `view` and `travel` with the rest) never reach a level, though they
reach every arena. A set may still amend an arena's map and view, from its
encounter file, its drawing or its defaults, as the layer stack lets a set
amend anything. So the base stands alone by construction, and the embedded
test holds it: every level resolves and checks with nothing over it, each
area's `encounter` is one of its instance's, and a court's encounter is
posted on its level (its map's `ui_map` is the level's key), with the
exceptions it names: Echo of Doragosa, whose draft says 2099 while the log
posts the fight on 2097.

### Areas

An area is a place of the floor the camera knows: a boss's court, a hall, a
landing. Its `shape` is a stencil's shapes, joined, and never empty; its
fit is the view that shows their bounds whole. Its altitudes are multiples
of that fit: `far`, the furthest the camera goes over it; `home`, where it
rests; and `near`, the closest. Unsaid, the floor's view gives them:
`home` its `zoom` (else 1.15), `near` its `zoom_max` (else 8), `far` 1,
the whole area. `Area::altitudes(&view)` fills them so, and a reader
frames with that; `check` holds the filled three above 0 and in
`far ≤ home ≤ near`, so `near = 0.8` alone (under the unsaid `far` 1) or
`home = 10` alone (over the unsaid `near` 8) is an error. An area may turn
the camera (`turn`) and name the boss fought in it (`encounter`), which
makes it that boss's court; a map holds one court a boss. `Map::area_at`
finds the area a point lies deepest inside.

Three keys of an area cut the render over it:

- **`ceiling`**: yards over the ground drawn over its shapes (above 0),
  in the map's `ceiling`'s place, below it (a hall under its own roof) or,
  on a terrain level, above it (a street whose balconies would read as
  walls). Where areas overlap, the lowest ceiling wins, with no order
  among them and no depth rule; outside every area saying one, the map's
  holds (`Map::ceiling_at`). The ground is what the map's ceiling stands
  on: a terrain level's `ground`, else the floor's level, so an area's
  ceiling cuts a level of buildings and an arena too, though there it can
  only cut lower than the map's cut, never higher. On a terrain level what
  stands on the terrain is still capped by the region's top.
- **`hang`**: the map's `hang` over the area (above 0, as the map's is).
  A map's `hang` leaves out what hangs over the walk: any placed object (a
  building, a WMO group, a doodad, a terrain prop) whose lowest point
  stands `hang` yards or more over the ground under its footprint's
  middle is left out, as an exclusion leaves it out. The exclusions come
  first; `include_groups` and `include_models` win over the hang; the
  hang comes before the ceiling's cut. What stands on the floor (a lamp
  post, a statue) is the ceiling's to cut. The ground is the ground cut's
  where there is one, else the floor's level. Where areas overlap, the
  lowest hang wins, as the lowest ceiling does; outside every area saying
  one, the map's holds (`Map::hang_at`).
- **`stencil`**: where a level's render ends over the area, a map
  stencil's shapes, apart from its `shape` (the camera's circles are wider
  than a room). A level with any enabled area giving one renders within
  the union of its areas' stencils and its paths' corridors (Paths, below)
  alone, clear past them over the arena stencil's one-yard fade
  (`Map::level_stencil_depth`, `level_stencil_bounds` and
  `level_stencil_keep`, beside `Map::stencil_depth`); a level with none
  renders whole. `check` holds its shapes as it holds a map's stencil. An
  arena's area stencil is written and checked, not drawn: an arena ends at
  its map's `stencil`.

### Paths

A path is a way between two areas of one floor (`from` and `to`, area
keys), and goes both ways. Its `points` are the way, `from`'s end first;
unsaid, it is straight. Its `mode` is how a party goes it:

| Mode | Means | Speed, unsaid |
| --- | --- | --- |
| `run` (the default) | on foot | `[defaults.travel]` `run`, else 7 yd a second |
| `mount` | on the players' own mounts | `mount`, else 14 |
| `vehicle` | carried: a flight the instance gives, a boat, a cart | `vehicle`, else timed to its next post |
| `leap` | thrown: a launcher, a jump pad, a drop | timed to its next post |

A path's own `speed` overrides the season's; only a leap names a `spell`,
its device's. `Path::speed_in` answers a path's speed given the travel its
map reads. The season's speeds are a `[defaults.travel]` table beside
`[defaults.map]` and `[defaults.view]`, so an encounter reads them as
`travel` and a level does too, with an arena's own
`[encounter.<id>.travel]` or a level's `[level.<ui_map>.travel]` over them.

On a level whose areas give a stencil, every enabled path is a corridor
the render reaches along too: its way (its `points`, else straight from
the middle of `from`'s bounds to the middle of `to`'s; `Map::path_way`)
widened to its `width`, else 8 yd (`Path::CORRIDOR_YD`,
`Path::corridor_width`). A path to an area without a stencil reaches that
area's middle, rounded off half a width past it, and no further; one with
no `points` whose end area is
missing, switched off or without a shape bounds nothing. On a level with
no area stencil, and on an arena, a path bounds nothing. Its `width` still
says how wide the party goes, too.

### Transitions

What the replay does while the log is quiet about a unit, and as the party
moves from one area to another, is the map's `transition`:

- `after`: seconds without a post before a unit is quiet. Unsaid, 3: out
  of combat the log posts a player every 2 s or so, and a shorter wait
  would ghost a unit at every such gap.
- `unit`: what a quiet unit does. `hold`: it stays where it was last
  posted, today's behavior. `fade`: a ghost of it stays at its last post.
  `travel`: a solver routes it along the floor's paths (its areas and the
  ways between them, a graph) from its last post to its next, timed to
  arrive as the next post does, and draws it as a ghost on the way.
- `port`, `port_yd`: a jump faster than anyone goes (a portal, a
  teleport) is a port. `port_yd` is that speed in yards a second: a jump
  between two posts further than `port_yd` yards for each second between
  them (a second at least) is one, and so is a move to another floor.
  Unsaid, 60 yd a second. `port` is how it lands. `cut`: the unit is
  drawn at its new post at once. `flash`: a flash where it left and where
  it landed.
- `camera`: what the camera does as the party moves to another area.
  `hold`: it stays. `glide`: it eases over to the new area's `home`.
  `cut`: it jumps there.

Unsaid, `unit`, `port` and `camera` are today's: every unit held at its
last post until its next, the camera held. `after` unsaid is 3 s and
`port_yd` 60 yd a second, the replay's own readings of a quiet leg; the
crate says them as `Transition::AFTER_S` and `Transition::PORT_YD`, which
every reader of a transition takes them from.

### Water

The render pours water over what lies under it by depth (the replay's
floor renderer, gen-floors), in the client's own look unless the map
says otherwise. An arena's or a level's `[map.water]` is that look for the
floor, written as the floor should show it, its light in it:

- `close` and `far`: red, green and blue, 0–255, where the water is
  shallow and from `depth` down.
- `shallow` and `deep`: how much of the surface under it the water hides,
  0 to 1, at the shore and from `depth` down.
- `depth`: yards under the water's surface, above 0, at which it is `far`
  and `deep`; between, linearly in yards (the client's own look reads its
  type's depth curve instead).
- `bed`: the color the surface under the water fades toward with depth,
  all of the way at `depth`, as murky water hides its floor; unsaid, none.

Every key but `bed`, `types` and `shore` is said in the resolved map, a
layer amending any of them over another's table. Unsaid, `types`, it
draws every water whose colors are the game default light's: rivers,
lakes, the ocean and procedural water; a magic water with colors of its
own (Tyrhold's, the void's, the bioluminescent) keeps them, as its colors
are its art. `types` names the liquid types it draws instead, exactly, a
magic water's among them (`types = [924]`, the void's water at Midnight
Falls); a type the render pours no water of (a venom, drawn opaque) is
ignored, and gen-floors warns of it. The water fades in from its shore
over its first 0.75 yd of depth, the render's own; `shore` sets that
width (0, none: a film a fraction of a yard deep, as the void's at
Midnight Falls is, shows whole). With no fade, a pixel where the water
stands a hair over the surface under it is all water or all surface, and
the CPU and the GPU path can read that hair apart, so such a pixel may
differ between them. The ripple is the render's either way. `check` holds
`shallow` and `deep` to 0 to 1, `depth` above 0, `shore` to 0 or more and
`types` to one id or more; a color is three bytes as it is read.
The replay's suggest-water tool proposes one from the game's light at the
floor, as TOML to paste and tune against a screenshot.

### Pulls

A level gives the usual route's packs on it, each a pull of the
instance's creatures (A dungeon's creatures, below) in one of its areas:

```toml
[level.2097.pull.bridge-pack]
name = "The bridge pack"
order = 3                   # its place in the level's usual route, from 1;
                            # one pull a place
optional = true             # the usual route may skip it at some key levels;
                            # unsaid, false
area = "bridge"             # the area of this level it stands in
npc = ["ember-hound", "ember-hound", "ash-caller"]   # instance [npc] keys,
                            # one a member: a key as often as the pack holds it
spots = [[1830, -2790], [1834, -2786], [1828, -2783]]   # where each member
                            # stands until it is pulled, world X and Y, by
                            # index with `npc`; unsaid, nowhere drawn
```

A pull's members are the instance file's creatures alone (its `[npc]`):
a creature only a `trash.toml` adds cannot be one, since who is in the
dungeon is the base's.

`order` is the route most runs take; a run may go another way, and the
replay numbers its packs by the run's own order. Across a dungeon's
levels the replay counts a pull's place in the order a run first visits
them, each level's pulls by `order`, to say where a pack usually comes. The log never shows a
pack before it is pulled, so where its members stand until then
(`spots`, one a member or none) comes from a census of many runs' first
posts. `Level::pulls_in_order` lists a level's pulls by `order`, then key,
and `enabled = false` drops one, as it drops an area.

`optional = true` marks a pack the usual route may skip at some key
levels: one a party takes where its count would fall short and leaves
where it would not. It keeps its `order`, and a run that leaves it has
not left the route, so nothing counts it missing; the replay draws its
members waiting in a lighter gray than the rest's. Which key levels skip
it is not said: the usual route depends on the key level, and a pull's
order and optional-ness by key band is a later refinement measured from
the stored runs.

A route's forces are read both ways. `Level::route_forces` gives
`(without, with)`: `Pull::forces` summed over `pulls_in_order` (a pull
switched off left out), first without the optional pulls, then with
them. Summed over an instance's levels and set against its whole
`forces`, the first says whether the usual route reaches 100% with its
optional pulls left out, and by how much ("576 of 551 without the
optional pack: 25 over"); the second, what the optional pulls add. It
reads `(0, 0)` until the instance's creatures are laid in
(`Level::npc`, which `Rubric::level` fills and a level read from text
alone lacks). An optional pull is checked as any other: nothing about
it alone can be wrong.

**A pull's share of the enemy forces is never written.** It is its
members' `forces` (`Pull::forces`), counted from the instance's
creatures, so a pull's name and an area's carry no percentage: "The
bridge pack", not "The bridge pack (12%)". A share written by hand goes
stale as the season's values move; a derived one cannot.

A pull is one level's own, and the base's alone as the rest of a level
is: no `[defaults]` gives one (a `[defaults.pull]` is refused where its
file is read, as no encounter has a `pull`), and a level reads the
defaults for its `map`, `view` and `travel` alone.
`Rubric::level` lays the instance's creatures into the level it answers
(`Level::npc`, never written), which the check and the forces read. A
level read from its text alone, as an editor reads one, has none laid
in: check it with `check_level_with(&level, instance.npcs())`, which
holds its pulls to the creatures given (`check_level` is that with the
level's own), and lay `npc` in before asking a pull's `forces`. Two
levels are equal whatever creatures either holds, so the one read and
the one answered compare alike.
`check_level` refuses a pull in an area the map lacks or in a boss's
court (an area with an `encounter`: a boss is fought, not pulled), a pull
of no one or naming a key the instance's creatures lack, `spots` that are
not one a member or not numbers, an `order` of 0 (the route counts from
1), and two pulls at one `order`.

## A dungeon's creatures

A keystone run is more than its bosses: its trash, and the enemy forces
each kill gives, count toward the timer. The instance file names them at
the top level, beside its levels:

```toml
forces = 551                # the dungeon's whole enemy forces: what a
                            # keystone's count runs to

[npc.primal-juggernaut]     # the same keys an encounter's [npc] has
name = "Primal Juggernaut"
creature = [188244]
role = "add"
control = "immune"          # no stun or incapacitate lands on it
forces = 25                 # what the count gains as one dies

[npc.primal-thundercloud]
name = "Primal Thundercloud"
creature = [197509]
role = "pet"
owner = "tempest-channeler" # a key of this table
```

The table is the instance's, not a level's: a creature may stand on more
than one level, and a reader finds it by the creature id alone, without
knowing the floor. `Rubric::instance_by_map(map)`
finds the instance by the map id a run's log writes (the newest season
holding it, as an encounter is answered), `Instance::npc_of(creature)` the
creature's entry and key, `Instance::npcs()` the table and
`Instance::forces()` the whole. A switched-off entry is left out.

A boss stays its encounter's: an encounter's `[npc]` is its own, and the
instance's names who is in the dungeon outside them (a creature both
name, such as a whelp a boss wakes, may be in both). The table is the
base's alone, like a level: a curated or user set's instance file giving
`npc` or `forces` is refused. `crate::check_instance` checks it as `check`
does an encounter's NPCs (an `owner` or `shares` naming a key the table
lacks, two keys listing one creature id, an `only` naming no difficulty),
and holds that a creature giving forces lists a creature id to count them
by and that an instance whose creatures give forces gives its total. An
instance has no phases, so one of its creatures naming any phase it is
`aloft` in is refused: a boss that takes wing says so in its encounter's
files, where its phases are.

A creature's `control` is how it can be crowd-controlled: `all`, the
default and never written, every control lands; `stun`, stuns and
incapacitates land but no full control; `immune`, nothing lands. It is
base data, as the creature's identity is, read from the stuns and
incapacitates the log shows landing on the creature or failing, and says
what a pull of it asks of a party that would stop its casts or hold it
still. An encounter's NPC may give one as well. A word other than the
three is the file's error; `check` holds nothing more of it.

## A season's keystone rules

Some of the game's rules for a season's keys are in none of the client's
tables. The season file gives them under `[rules]`:

```toml
[rules]
# From a key level up, until the next pair's, the seconds each player's
# death adds to the key's clock: [from_level, seconds], in any order.
death_penalty = [[2, 0], [7, 5], [12, 15]]
```

A key's level takes the pair with the highest from-level at or under it
(`SeasonRules::penalty_at`): here +2 to +5 cost nothing, as measured;
+6, which no measured run was, is assumed to cost nothing too until a
run shows otherwise; +7 to +11 cost five seconds a death and +12 up
fifteen. A level under the lowest pair, or a
season that says nothing, knows no penalty. A from-level given twice, or
seconds below 0 or not a number, is an error naming the file and the key
(`SeasonRules::problems`). A reader finds the season
through a run's map: `rubric.instance_by_map(map).map(|(s, _)|
s.rules.penalty_at(level))`. The rules are the base's by construction: a
curated or user set's `season.toml` gives `[defaults]` alone, so a set
giving `rules` is refused. The daemon, the MCP server and the TUI never
read the rubric; should one of them need the penalty, it moves to core
beside the keystone timers.

## Who reads what

- **`tools/gen-floors.sh`**: `[map]`, meaning the level, the ceiling, the
  groups, models and textures to leave out, the stencil, the density
  (`ppy`) and the glows, each colored in and written as
  `<floor>-glow-<key>.png` with a `glow = …` line in the sidecar; and each
  `[room]`'s map floor, middle, radius, level and ceiling, rendered beside
  the arena as `<floor>-<room>.png` with `room = "<key>"` in its sidecar. A
  stencil makes the render cover its shapes alone, clear outside them past
  a yard's fade, with no minimap backdrop and `stencil = <shapes>` in the
  sidecar. Entombed Sentinels' went from 10082×10008 px to 1312×1232.
  The map's `crop` is the world box the render covers (met with the
  stencil where there is one too; a room off the arena reads none),
  `include_groups` and `include_models` are drawn where a rule of the
  render's own would leave them out (another floor's group of the
  floor's building, faces over the ceiling or a terrain level's ground,
  with a group's split children and its props; an exclusion still wins),
  a `fade` draws its piece see-through over what lies under it (a
  group's split children and its props with it), `exposure` times
  the tool's graded exposure (`--exposure` given outright wins), and
  `water` is the look its water is poured in. A
  level's `[map]` is read for what it leaves out, includes and fades, its
  crop, exposure and water, and its `ground`: `"walked"`, every stored keystone
  run's posts on the level (what every season dungeon's terrain level
  says); a height; else its terrain. A level of buildings is cut by its
  own groups and reads no `ground`; nor does an arena (it is cut at one
  level, its `level` where it says one). On a walked terrain level the
  terrain is cut over the walked ground as a model is, a triangle by the
  ceiling over its middle where the walk reaches that middle, so a raised
  walk's terrain over a walked street goes where the walk reaches it;
  where no walk reaches, nothing of it is cut, and raised terrain the walk
  never climbs still draws (an area's `stencil` bounds it). A map's areas cut it too (Areas,
  above): an area's `ceiling` over its shapes, the lowest where areas
  overlap (a level of buildings or an arena is cut over its level where an
  area says a ceiling or the map a `hang`, as a terrain level is over its
  ground); the map's and an area's `hang`, which leave out what hangs over
  the walk; and, on a level whose areas give a `stencil`,
  the union of their stencils and their paths' corridors, which the
  render covers alone, clear outside past a yard's fade, with
  `stencil = <shapes and corridors>` in the sidecar
  (`Map::level_stencil_count`) beside the minimap line it keeps (the
  replay lays no minimap past a stenciled level). An arena reads its areas' ceilings and
  hangs, not their stencils. Each of these joins a render's `params` hash
  where it is said (`Map::params_hash`, `Map::level_params`: an area's
  ceiling and hang with the shape they cover, its stencil, and each
  path's corridor where a stencil is said; a room's hash leaves the crop
  out), and `gen-floors --stale` renders what the rubric has moved since.
  The terrain's cut is the renderer's own rule, in no hash: a walked level
  is rendered again by hand when it changes.
  And each `[place]`'s `swirl.texture` (an encounter's, and its instance's
  `trash.toml`'s): the game's texture, written once beside the floors as
  `swirl-<fdid>.png` for the replay to draw.
- **The map editor** (private, later) writes a level's and an arena's
  `[map]`: its areas, paths and transition, and the render's tweaks; and
  a level's pulls. Today they are written by hand.
- **The replay** (today the spike): `[view]`, `[npc]` roles, icons and
  `still`, `[ability]` marks and `under`, `[place]` (its `swirl` drawn over
  the texture gen-floors wrote), `[ground]`, `[mark]`,
  `[line]`, `[volley]` (its `place_kills` picked by `guess`, which the
  editors may draw from too), `[flash]`, `[knock]`, `[range]`, `[apart]`,
  `[streak]`, `[room]`, `[reach]` (its `absorbs`), the layers' `breaks`,
  the glows' `on` and easing, and the `[phase]` and `[event]` triggers
  they key on; and, from an
  arena's map or a level's, its areas, paths, `transition` and `travel`,
  and a level's `[view]` (a level render cut to its areas' stencil
  outlines the stencil, not the render's box, and lays no minimap past
  it, as a stenciled arena does; one made before the stencil is clipped
  on load by `Map::level_stencil_keep`); and in a keystone run, its
  instance's creatures and their `forces` (its packs and its enemy
  forces), its `trash.toml`, and its season's `[rules]` (each death's cost
  on the key's clock).
  Its mechanics timeline, its "Right now" panel and its list of what
  happened are built from those entries (the spike reads them itself until
  the interpretation does). `[spell]` gives the radius or duration an entry
  leaves to the game.
- **Whoever tunes an encounter**: `[spell]`, how far an ability reaches and
  how long it lasts, and, from this machine's sidecar, its `text`. A
  `stencil` is traced on the room's render with `tools/stencil.sh`, which
  turns it square under a yard grid and the pulls' positions, and turns the
  outline into the TOML.
- **The interpretation** (the plan's layers 2–3): `[ability]`, `[phase]`
  and `[event]`. It records `Rubric::version()`, so a stored fight regrades
  when the rubric changes.
