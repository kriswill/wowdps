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

It is TOML, reviewed like code, and embedded in the binaries at build time
(`crates/encounter-rubric`, `wowdps-encounter-rubric`). A change takes a rebuild.

## Files

```text
rubric/<season>/season.toml                        the season and its defaults
rubric/<season>/<instance>/instance.toml           a raid or dungeon
rubric/<season>/<instance>/<id>-<slug>.draft.toml  generated, never edited
rubric/<season>/<instance>/<id>-<slug>.toml        tuned by hand
```

`<id>` is the encounter's DungeonEncounterID, the number the combat log's
`ENCOUNTER_START` writes.

- **The draft** is written by `tools/gen-rubric.sh` from the client's tables:
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
    Ula'tek's health.

  A map and its NPCs are part of the base (below), so a room looks and
  changes the same, with the same NPCs in it, at every tier. `gen-rubric`
  writes an instance file only when it is missing.
- **The journal's text** is not in the repository. Blizzard's words stay
  out, like the extracted art. `gen-rubric` writes each encounter's text to
  a sidecar on this machine,
  `$XDG_DATA_HOME/wowdps/rubric-text/<season>/<instance>/<id>-<slug>.toml`
  (`--text-dir` puts it elsewhere, never in the checkout).

Every file starts with `schema = N`, the format it is written in. A build
reads its own format and migrates older ones forward, and refuses a newer
one. A new season is a new directory. When a season changes the format,
the schema number moves and the migration is code.

## How an encounter resolves

Each layer is laid over the last:

1. the season's `[defaults]`
2. the instance's `[defaults]`
3. the draft
4. the encounter's map and NPCs from the instance file, `[encounter.<id>]`
5. the tuned file
6. the difficulty overrides, `[difficulty.<name>]`, in the order the
   client's fallback chain gives them, the most general first

**Two tiers.** An encounter resolves at a tier. The **base** is what
extraction gives, and the encounter's map and NPCs, with no other hand
amendment:
layers 1 to 4, with the draft's own difficulty overrides. **Curated** lays
the tuned file and its overrides over the base: what the fight does in the
room. The two sit side by side in these directories,
but the base never leans on a tuned file: every encounter's base resolves
and checks clean on its own (a test holds it), so the curated layer can
ship apart from it. A reader asks for a tier per encounter
(`Rubric::encounter_at`); `Rubric::with_tier` caps what it will answer, and
`Rubric::has_curated` says whether an encounter has a tuned file at all.
The replay spike switches between them with `a`, the panel's Rubric
switch, or `--rubric base`.

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
such as an ability's NPC or a trigger's phase, and an `only` that names no
difficulty (which would keep its entry off every one).

## An encounter, key by key

As resolved, every key in one place. In the files, `[map]`, `[view]`,
`[room]`, `[place]` and the events they key on come from the draft and the
instance file's `[encounter.<id>]`, and a tuned file leaves them alone. An
`[npc]` comes from the draft, amended or added by the instance file.
Tables merge key by key, so a tuned file that made a layer break on a phase
where the instance says an event would be refused, never half-merged.

```toml
schema = 1
encounter = 3492            # DungeonEncounterID
name = "Ula'tek"
order = 8                   # the journal's boss order

[map]                       # how gen-floors draws the room
ui_map = 2610               # the UiMap the fight is posted on
floor = "ulatek"            # the render's slug in the floors cache
level = 390.2               # the arena's height, when the derived one is wrong
ceiling = 25                # yards over the level that are drawn
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

[view]                      # the replay's first look
turn = 0                    # degrees clockwise; unsaid, derived
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
                            # wears her name and is an NPC of its own)
role = "boss"               # boss | add | pet | object | ignore
icon = 5926310              # FileDataID (the journal portrait)

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
tint = "teal"               # the color it is drawn in
at = [1575.0, 0.0]          # world X and Y, as the log writes positions
radius = 30
liquid_height = 388
from = { event = "shattering" }   # when it is there; unsaid, all fight
until = { phase = "stage-three" }

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
                            # { after = 30 }: seconds after it forms
from = { cast = { spell = 1234567 } }   # there since the last of these before
                                        # `at` (a droplet seen only as it is soaked)
orbit = { around = "soulcoil-well", turn = -3.0 }   # circles a place or an NPC
                            # from where it formed: degrees a second,
                            # counter-clockwise from above
seen = [1288554]            # its hits place it again at their victims

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
to = "breath-of-ulatek"     # the NPC it runs to
delay = 4                   # strikes this long after; drawn faint until then
hits = [1284209]            # the spells that hit along it as it strikes

[volley.plague-wave]        # projectiles sent out from a spot on fixed bearings
tint = "green"
from = { aura_removed = { spell = 1281913 } }   # each time: its unit's spot
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
# delay = 0.5               # seconds after `from` the first sets out; unsaid, 0
# every = 4.5               # seconds until it is sent again from the same spot
# times = 2                 # and facing, this many in all; unsaid, once
# alternate = true          # each sending from the other side than the last:
                            # its spots mirrored in turn
# holes = true              # its projectiles are slots each sending fills only
                            # some of: a hole where a living player stood in
                            # its way unhit, whole otherwise
# faint = true              # with holes: one no one came near is drawn faint,
                            # one a hit lay along whole
hits = [1295798]            # the spells that hit along them
destroys = ["malignant-totem"]   # NPCs they destroy as they cross them
preview = true              # its bearings dashed from the carrier of `from`'s
                            # aura while it waits; the projectiles as it erupts

[flash.noxious-blast]       # hits flashed on their victims
spell = [1284452]
tint = "green"

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
| `{ cast = { spell = N, by = "npc" } }` | a hostile cast lands (`SPELL_CAST_SUCCESS`) |
| `{ aura_applied = { spell = N, on = "npc" } }` | a hostile aura goes on |
| `{ aura_removed = { spell = N, on = "npc" } }` | it comes off |
| `{ health_below = { npc = "x", pct = 60 } }` | an NPC's health falls under a share |
| `{ plateau = { npc = "x", secs = 30 } }` | an NPC takes no damage that long |
| `{ appears = { npc = "x" } }` | an NPC is first seen |
| `{ death = { npc = "x" } }` | an NPC dies |
| `{ hit = { spell = N } }` | a hostile spell hits (at its victim) |
| `{ aura_gone = { spell = N } }` | the last player carrying a hostile aura loses it |
| `{ event = "name" }` | a named event happens |
| `{ phase = "name" }` | a phase begins |

`by` and `on` are optional. A place's `until` is a trigger like `from`.

**A ground lands where a unit stood.** Each time one of its `at` triggers
fires, a ground appears at the unit the trigger is about: an aura's
target as it goes on or comes off, a hit's victim, a cast's caster, an
NPC as it appears or dies. A trigger that names no one (the start, a
clock, health, a phase, an event) cannot place one. Entombed Sentinels
holds both shapes the table was made for:
- **Blood Venom:** a pool each blood venom drops as it comes off. It
  spreads to a size set by the venom's stacks, lasts 20 s on Heroic and
  stays to the end of the pull on Mythic.
- **Toxic Droplets:** soaked by stepping on them. The log shows one only
  as it is soaked, so each stands `from` the spray until it ends `at` its
  soak.

A ground can land `ahead` of its unit, along the way the unit faced as
its trigger fired: each of Ula'tek's Spectral Coils crushes the floor
21.5 yd in front of itself, where its soakers stand (their middle 20.3 to
22.5 yd ahead and none aside, over 3700 soaks).

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
radius the game's for its `spell` unless a `radius` says otherwise. With
`sides`, two NPCs also get the line halfway between them: the Sentinels'
Marks, each marking everyone within 40 yd, split the raid along it. An
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
on the Soulcoil Well. One within the place's radius is a fault.

**A line strikes between a spot and an NPC.** It starts where its `from`
trigger's unit stood and runs to its `to` NPC, wherever the NPC is. It is
drawn faint until it strikes `delay` later, and then its `hits` flash on
the players along it: Living Venom, a soaked droplet's slime going back to
Breath of Ula'tek. Where several of its NPC stand as it strikes, it runs
to the one the log posted last: on Mythic a spectral Ula'tek pours Toxic
Incubation at the Blightscale Wretch she tore down most lately, while the
one before still stands. A `[flash]` is a spell whose every hit flashes
on its victim. One that hits most of the raid at once reads as one blow
to the raid: Noxious Blast, a droplet left unsoaked, whose spot the log
never writes.

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
rose: so the tumors the replay can draw are exactly the ones the waves
missed.

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
again and again. A layer keyed to it breaks at the time nearest to when
the players stopped standing on it, so Ula'tek's four quarters can all
name Circling Prey and each go with its own cast. A layer the players never
left takes a time no other layer took, and stands when none is left (the
quarter the raid kills Ula'tek on). A layer made of other layers (the whole
platform over its quarters) breaks the first time its event happens, and
its pieces come only then: before, the whole is the floor alone. An
event that never happens leaves the reading from where the players stood.

**What the log never writes can still be timed.** A scripted moment with
no line of its own is an `{ after = secs }` event, measured over every pull
of a fight that runs on a fixed clock. Where the clock differs, by
difficulty or by a few seconds a pull, it is timed from a moment the log
does write: `since` names the trigger and `nth` which time it fired.
Ula'tek's Shattering, which flings the raid as the platform cracks, comes
68.5 s after her second Rage of the Shackled lands on every pull, Heroic
or Mythic, though Mythic's clock runs 10 s later and one Mythic kill's
2.3 s earlier still. A later layer that gives the clock `{ secs = … }`
retimes it and keeps what it counts from; a bare number replaces it.

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
- **The replay** (today the spike): `[view]`, `[npc]` roles, icons and
  `still`, `[ability]` marks and `under`, `[place]`, `[ground]`, `[mark]`,
  `[line]`, `[volley]`, `[flash]`, `[range]`, `[apart]`, `[streak]`,
  `[room]`, `[reach]`, the layers' `breaks`, the glows' `on` and easing,
  and the `[phase]` and `[event]` triggers they key on.
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
