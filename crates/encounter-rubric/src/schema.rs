//! The rubric's typed form: one `Encounter` per boss, resolved for one
//! difficulty. Every struct refuses unknown keys, so a typo in a reviewed
//! file is an error that names its path, not a silent default.
//!
//! Named entries (`npc`, `ability`, `phase`, `place`, `ground`, `mark`,
//! `line`, `volley`, `flash`, `knock`, `range`, `apart`, `streak`, `room`,
//! `reach`, `event`, `map.layer`, `map.glow`, `map.fade`, `map.area`, `map.path`)
//! are TOML tables keyed by a slug, so a later layer (the tuned file, a
//! difficulty) changes one by naming it, and `enabled = false` drops it;
//! all but the map's can be kept to some difficulties by their `only`. The
//! game's `spell` facts are keyed the same way, by spell id.
//!
//! A dungeon's level (`Level`) holds the same `Map` and a `View`, so a
//! reader draws a raid's arena and a dungeon's level with one type, and
//! the usual route's pulls on it (`pull`, keyed by a slug like the rest).

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// One encounter, every layer applied for one difficulty.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Encounter {
    /// The rubric format these files are written in (`crate::SCHEMA`).
    pub schema: u32,
    /// The DungeonEncounterID: what the combat log's ENCOUNTER_START names.
    pub encounter: u32,
    pub name: String,
    /// The instance's slug (its directory).
    #[serde(default)]
    pub instance: String,
    /// The boss's place in the journal's order.
    #[serde(default)]
    pub order: u32,
    #[serde(default)]
    pub map: Map,
    #[serde(default)]
    pub view: View,
    #[serde(default)]
    pub npc: BTreeMap<String, Npc>,
    #[serde(default)]
    pub ability: BTreeMap<String, Ability>,
    #[serde(default)]
    pub phase: BTreeMap<String, Phase>,
    #[serde(default)]
    pub place: BTreeMap<String, Place>,
    /// What the fight leaves on the floor and takes back, one each time
    /// the log shows it.
    #[serde(default)]
    pub ground: BTreeMap<String, Ground>,
    /// Auras the replay draws on the players carrying them, with their
    /// count.
    #[serde(default)]
    pub mark: BTreeMap<String, Mark>,
    /// Strikes between a spot and an NPC: a dart, a charge, a beam.
    #[serde(default)]
    pub line: BTreeMap<String, Line>,
    /// Projectiles sent across the floor from a spot, each along a fixed
    /// bearing.
    #[serde(default)]
    pub volley: BTreeMap<String, Volley>,
    /// Spells whose hits the replay flashes on their victims.
    #[serde(default)]
    pub flash: BTreeMap<String, Flash>,
    /// Players thrown straight out from a middle as a trigger fires: a
    /// knockback.
    #[serde(default)]
    pub knock: BTreeMap<String, Knock>,
    /// Circles the replay draws round NPCs: how far an aura they pulse
    /// reaches.
    #[serde(default)]
    pub range: BTreeMap<String, Range>,
    /// Pairs of NPCs that must be kept apart.
    #[serde(default)]
    pub apart: BTreeMap<String, Apart>,
    /// Hostile spells that keep hitting one player until another takes
    /// them: a tank swap.
    #[serde(default)]
    pub streak: BTreeMap<String, Streak>,
    /// Rooms off the main one, entered through a place.
    #[serde(default)]
    pub room: BTreeMap<String, Room>,
    /// NPCs that must not reach a place.
    #[serde(default)]
    pub reach: BTreeMap<String, Reach>,
    #[serde(default)]
    pub event: BTreeMap<String, Event>,
    /// What the game's own spell tables say about the spells the encounter
    /// uses, keyed by spell id: written into the draft by the generator, so
    /// a tuned entry can take a radius or a duration from the game rather
    /// than a measurement.
    #[serde(default)]
    pub spell: BTreeMap<String, Spell>,
    /// The speeds the map's paths go at by their mode, from the season's
    /// `[defaults.travel]`.
    #[serde(default)]
    pub travel: Travel,
}

/// A blank encounter in this build's format (`crate::SCHEMA`): no id, no
/// name, nothing in it. What a generator fills in.
impl Default for Encounter {
    fn default() -> Self {
        Encounter {
            schema: crate::SCHEMA,
            encounter: 0,
            name: String::new(),
            instance: String::new(),
            order: 0,
            map: Map::default(),
            view: View::default(),
            npc: BTreeMap::new(),
            ability: BTreeMap::new(),
            phase: BTreeMap::new(),
            place: BTreeMap::new(),
            ground: BTreeMap::new(),
            mark: BTreeMap::new(),
            line: BTreeMap::new(),
            volley: BTreeMap::new(),
            flash: BTreeMap::new(),
            knock: BTreeMap::new(),
            range: BTreeMap::new(),
            apart: BTreeMap::new(),
            streak: BTreeMap::new(),
            room: BTreeMap::new(),
            reach: BTreeMap::new(),
            event: BTreeMap::new(),
            spell: BTreeMap::new(),
            travel: Travel::default(),
        }
    }
}

/// One spell, as the client's tables give it (Spell, SpellEffect,
/// SpellRadius, SpellMisc, SpellDuration, SpellAuraOptions). Effects that
/// differ by difficulty land in `[difficulty.<name>.spell.<id>]`.
#[derive(Debug, Clone, Default, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Spell {
    pub name: String,
    /// Yards: its largest effect radius.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub radius: Option<f32>,
    /// Seconds an aura of it lasts; none for an instant one, or one that
    /// lasts until it is taken away.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration: Option<f32>,
    /// Seconds between its periodic effect's ticks.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub period: Option<f32>,
    /// The most stacks an aura of it holds.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stacks: Option<u32>,
    /// The spells its effects set off.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub triggers: Vec<u32>,
}

/// The room: which floor, and how `gen-floors` draws it.
#[derive(Debug, Clone, Default, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Map {
    /// The UiMap the fight is posted on.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ui_map: Option<u32>,
    /// The render's slug in the floors cache (`<slug>.png`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub floor: Option<String>,
    /// The arena's height, where the one derived from the players'
    /// positions is wrong.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub level: Option<f32>,
    /// Yards over the arena level that are drawn (`gen-floors --ceiling`).
    /// An area's own `ceiling` says it over the area (`Map::ceiling_at`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ceiling: Option<f32>,
    /// Yards over the ground at which a placed object hangs over the walk:
    /// a building, a WMO group, a doodad or a terrain prop whose lowest
    /// point stands this high or higher over the ground under its
    /// footprint's middle is left out, as an exclusion leaves it out
    /// (`include_groups` and `include_models` still draw it). What stands
    /// on the floor is the ceiling's to cut. Unsaid, nothing hangs; an
    /// area's own `hang` says it over the area (`Map::hang_at`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hang: Option<f32>,
    /// How it is lit: unsaid or `baked`, as the client lights it (the light
    /// baked into a building, the sun on the ground, over the map's
    /// ambient); `graded`, by its building's ambient.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub light: Option<Light>,
    /// WMO group uids left out of the render.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub exclude_groups: Vec<u32>,
    /// Model FileDataIDs left out of the render.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub exclude_models: Vec<u32>,
    /// Texture FileDataIDs whose surfaces are left out: geometry the game
    /// draws where no player sees it (Entombed Sentinels' flat yellow strips
    /// under the venom, poking out past the pool's edge).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub exclude_textures: Vec<u32>,
    /// Pieces of the room a fight can take away.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub layer: BTreeMap<String, Layer>,
    /// What of the room is worth drawing, its shapes joined: `gen-floors`
    /// renders their bounds alone, leaves everything outside them clear and
    /// lays no minimap past them. Unsaid, the room and what lies round it.
    /// An arena's alone: a level is bounded by its areas' (`Area::stencil`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub stencil: Vec<StencilShape>,
    /// Pixels a yard the arena is rendered at (`gen-floors --ppy`); unsaid,
    /// the tool's. A stenciled room is small enough for more.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ppy: Option<f32>,
    /// Liquid the game colors and lights by itself, which the render draws
    /// otherwise or not at all: a fountain's pool, the channel it runs down.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub glow: BTreeMap<String, Glow>,
    /// The world box the render covers, `[x0, y0, x1, y1]` with `x0 < x1`
    /// and `y0 < y1`: where the heuristic frames the wrong thing and a
    /// stencil is more than wanted. With a stencil too, the two meet.
    /// Unsaid, the renderer's own framing.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub crop: Option<[f32; 4]>,
    /// WMO group uids drawn although a rule would leave them out: another
    /// floor's group of the floor's own building, faces over the ceiling
    /// or over a terrain level's ground. An exclusion still wins.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub include_groups: Vec<u32>,
    /// Model FileDataIDs drawn although a rule would leave them out (as
    /// `include_groups`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub include_models: Vec<u32>,
    /// Pieces drawn see-through rather than left out: a roof over the
    /// path, a bridge over a court.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub fade: BTreeMap<String, Fade>,
    /// What the ceiling is measured from on a terrain level: the terrain
    /// under each point, the ground the players walked (every stored
    /// keystone run's posts on the level), or a height. Unsaid, the
    /// renderer's own reading. An arena does not read it: it is cut at one
    /// level, its `level` where it says one. Its type is `Datum`, as
    /// `Ground` is what a fight leaves on the floor.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ground: Option<Datum>,
    /// Times the graded look's brightness (`light = "graded"`, and every
    /// surface that gives no light of its own): gen-floors' `--exposure`
    /// default times this; an `--exposure` given outright wins. Unsaid, 1.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exposure: Option<f32>,
    /// The places of the floor the camera knows: a boss's court, a hall,
    /// a landing. Each frames itself at its own altitudes.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub area: BTreeMap<String, Area>,
    /// The ways between two areas of this floor, and how a party goes
    /// them: what a unit the log stops posting is routed along.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub path: BTreeMap<String, Path>,
    /// What the replay does while the log is quiet about a unit and when a
    /// unit or the party moves on. Unsaid, today's: every unit held where
    /// it was last posted, the camera held.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub transition: Option<Transition>,
    /// How the render draws the floor's water: its look in place of the
    /// client's, on every water whose colors are the game's default light's
    /// (rivers, lakes, the ocean, procedural water), or on the liquid types
    /// its `types` names; a magic water with colors of its own (Tyrhold's,
    /// the void's) keeps them unless named. Unsaid, the client's look.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub water: Option<Water>,
}

/// A floor's water as its render draws it (`[map.water]`): every color
/// red, green and blue as the floor should show it (lit, final), every
/// key but `bed`, `types` and `shore` given in the resolved map (a layer
/// may amend one key over another's table, as tables merge).
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Water {
    /// Its color where it is shallow.
    pub close: [u8; 3],
    /// Its color at `depth` and deeper.
    pub far: [u8; 3],
    /// How much of the surface under it it hides at the shore, 0 to 1.
    pub shallow: f32,
    /// How much it hides at `depth` and deeper, 0 to 1.
    pub deep: f32,
    /// Yards under its surface at which it is `far` and `deep`, from its
    /// shore linearly in yards; above 0.
    pub depth: f32,
    /// The color the surface under it fades toward with depth, all of the
    /// way at `depth` (the water's own murk); unsaid, no fade.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bed: Option<[u8; 3]>,
    /// The liquid types (`LiquidType` ids) it draws, exactly: a magic water
    /// with colors of its own among them (the void's 924). Unsaid, every
    /// water whose colors are the default light's. A type the render pours
    /// no water of (a venom, which is drawn opaque) is ignored.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub types: Option<Vec<u32>>,
    /// Yards over the surface under it at which all of it shows, faded in
    /// from its shore; 0 or more. Unsaid, the render's own (0.75); 0, none
    /// fades: a film of water a fraction of a yard deep (a void's) shows
    /// whole.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shore: Option<f32>,
}

impl Water {
    /// Whether it draws liquid type `liquid_type`, a water whose colors are
    /// its type's own (`own_colors`) or the default light's: the types it
    /// names where it names some, else those of the default light's.
    pub fn draws(&self, liquid_type: u32, own_colors: bool) -> bool {
        match &self.types {
            Some(types) => types.contains(&liquid_type),
            None => !own_colors,
        }
    }
}

/// A piece of the floor drawn see-through: one WMO group or one model,
/// by exactly one of `group` and `model`.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Fade {
    #[serde(default = "yes", skip_serializing_if = "is_true")]
    pub enabled: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// A WMO group uid.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub group: Option<u32>,
    /// A model FileDataID.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<u32>,
    /// How much of it shows, above 0 and under 1.
    pub alpha: f32,
}

/// What a terrain level's ceiling is measured from (`map.ground`): written
/// `"terrain"`, `"walked"` or a height in yards.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Datum {
    /// The terrain's height under each point.
    Terrain,
    /// The height of the ground the players walked, near each point.
    Walked,
    /// One height, yards, for the whole level.
    Height(f32),
}

impl Serialize for Datum {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        match self {
            Datum::Terrain => s.serialize_str("terrain"),
            Datum::Walked => s.serialize_str("walked"),
            Datum::Height(h) => s.serialize_f32(*h),
        }
    }
}

impl<'de> Deserialize<'de> for Datum {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Datum, D::Error> {
        use serde::de::{self, Visitor};
        struct WordOrHeight;
        impl Visitor<'_> for WordOrHeight {
            type Value = Datum;
            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                f.write_str("\"terrain\", \"walked\" or a height in yards")
            }
            fn visit_str<E: de::Error>(self, v: &str) -> Result<Datum, E> {
                match v {
                    "terrain" => Ok(Datum::Terrain),
                    "walked" => Ok(Datum::Walked),
                    _ => Err(E::unknown_variant(v, &["terrain", "walked"])),
                }
            }
            fn visit_f64<E: de::Error>(self, v: f64) -> Result<Datum, E> {
                Ok(Datum::Height(v as f32))
            }
            fn visit_i64<E: de::Error>(self, v: i64) -> Result<Datum, E> {
                Ok(Datum::Height(v as f32))
            }
            fn visit_u64<E: de::Error>(self, v: u64) -> Result<Datum, E> {
                Ok(Datum::Height(v as f32))
            }
        }
        d.deserialize_any(WordOrHeight)
    }
}

/// A place of a floor the camera knows: a boss's court, a hall, a landing.
/// Its altitudes are multiples of its fit, the view that shows its shapes'
/// bounds whole: `far` the furthest the camera goes over it, `home` where
/// it rests, `near` the closest (`far ≤ home ≤ near`). Unsaid, the floor's
/// view's: `home` its `zoom`, `near` its `zoom_max`, `far` 1 (`altitudes`).
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Area {
    #[serde(default = "yes", skip_serializing_if = "is_true")]
    pub enabled: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Where it lies, its shapes joined in world yards (a stencil's).
    pub shape: Vec<StencilShape>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub far: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub home: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub near: Option<f32>,
    /// Degrees clockwise on screen while the camera is over it; unsaid,
    /// the floor's view's.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub turn: Option<f32>,
    /// Yards over the ground drawn over its shapes, in the map's
    /// `ceiling`'s place: below it, or on a terrain level above it; on an
    /// arena or a level of buildings an area can only lower the cut. Where
    /// areas overlap the lowest wins (`Map::ceiling_at`). Unsaid, the map's.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ceiling: Option<f32>,
    /// Yards over the ground at which a placed object hangs over its shapes
    /// (the map's `hang`); where areas overlap the lowest wins
    /// (`Map::hang_at`). Unsaid, the map's.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hang: Option<f32>,
    /// Where the render ends over it, in a map stencil's form: a level
    /// renders within the union of its areas' stencils and its paths'
    /// corridors (`Map::level_stencil_depth`), all of it where no area
    /// gives one. An arena's is written and checked, not drawn: an arena
    /// ends at its map's `stencil`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub stencil: Vec<StencilShape>,
    /// The boss fought here, by DungeonEncounterID: a boss's court.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub encounter: Option<u32>,
}

impl Area {
    /// `far` unsaid: the whole area.
    pub const FAR: f32 = 1.0;
    /// `home` unsaid with the view's `zoom` unsaid: the seasons' default.
    pub const HOME: f32 = 1.15;
    /// `near` unsaid with the view's `zoom_max` unsaid: the replay's closest.
    pub const NEAR: f32 = 8.0;

    /// Its altitudes, `[far, home, near]`, those unsaid filled from `view`:
    /// `far` 1, `home` the view's `zoom` (else 1.15), `near` its `zoom_max`
    /// (else 8). What a reader frames it with; `crate::check` holds them
    /// above 0 and in order, so `home.clamp(far, near)` never panics.
    pub fn altitudes(&self, view: &View) -> [f32; 3] {
        [
            self.far.unwrap_or(Area::FAR),
            self.home.or(view.zoom).unwrap_or(Area::HOME),
            self.near.or(view.zoom_max).unwrap_or(Area::NEAR),
        ]
    }

    /// Its shapes' world bounds, `[x0, y0, x1, y1]`: its fit. `None` with
    /// no shape that is one (`crate::check` says which).
    pub fn bounds(&self) -> Option<[f32; 4]> {
        union_bounds(&self.shape)
    }

    /// How far inside it a world point lies, in yards (the deepest of its
    /// shapes): negative outside, and with no shapes.
    pub fn depth(&self, p: [f32; 2]) -> f32 {
        self.shape
            .iter()
            .map(|s| s.depth(p))
            .reduce(f32::max)
            .unwrap_or(f32::MIN)
    }
}

/// A way between two areas of one floor, and how a party goes it.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Path {
    #[serde(default = "yes", skip_serializing_if = "is_true")]
    pub enabled: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// The areas it runs between, by key: two of the same floor's. It goes
    /// both ways.
    pub from: String,
    pub to: String,
    #[serde(default, skip_serializing_if = "Mode::is_run")]
    pub mode: Mode,
    /// Yards across; unsaid, a single file. A level's render reads it as
    /// its corridor's width (`Path::corridor_width`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub width: Option<f32>,
    /// Yards a second, over the season's for its `mode` (`Travel`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub speed: Option<f32>,
    /// The spell a leap's device casts (a launcher, a jump pad); only a
    /// leap has one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub spell: Option<u32>,
    /// The way between its ends, world X and Y, `from`'s end first; unsaid,
    /// straight.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub points: Vec<[f32; 2]>,
}

impl Path {
    /// Its corridor's width where its `width` is unsaid, yards: what a
    /// stenciled level renders along it (`Map::level_stencil_depth`).
    pub const CORRIDOR_YD: f32 = 8.0;

    /// Its corridor's width, yards: its `width`, else `CORRIDOR_YD`.
    pub fn corridor_width(&self) -> f32 {
        self.width.unwrap_or(Path::CORRIDOR_YD)
    }

    /// Yards a second a party goes it: its own `speed`, else the season's
    /// for its mode. `None`: timed to the next post.
    pub fn speed_in(&self, travel: &Travel) -> Option<f32> {
        self.speed.or_else(|| travel.speed(self.mode))
    }
}

/// How a party goes a path.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Mode {
    /// On foot.
    #[default]
    Run,
    /// On their own mounts.
    Mount,
    /// Carried: a flight the instance gives, a boat, a cart.
    Vehicle,
    /// Thrown: a launcher, a jump pad, a drop.
    Leap,
}

impl Mode {
    fn is_run(&self) -> bool {
        *self == Mode::Run
    }
}

/// The season's speeds, yards a second, a path goes at by its mode
/// (`[defaults.travel]`): a path's `speed` overrides them.
#[derive(Debug, Clone, Default, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Travel {
    /// On foot; unsaid, 7 (`Travel::RUN`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run: Option<f32>,
    /// Mounted; unsaid, 14 (`Travel::MOUNT`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mount: Option<f32>,
    /// Carried; unsaid, timed to the next post.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vehicle: Option<f32>,
}

impl Travel {
    /// A run unsaid, yards a second: a player's base speed.
    pub const RUN: f32 = 7.0;
    /// A mount unsaid, yards a second: a ground mount's base speed.
    pub const MOUNT: f32 = 14.0;

    /// Yards a second by `mode`. `None` for a leap, and a vehicle unsaid:
    /// those are timed to the next post.
    pub fn speed(&self, mode: Mode) -> Option<f32> {
        match mode {
            Mode::Run => Some(self.run.unwrap_or(Travel::RUN)),
            Mode::Mount => Some(self.mount.unwrap_or(Travel::MOUNT)),
            Mode::Vehicle => self.vehicle,
            Mode::Leap => None,
        }
    }
}

/// What the replay does while the log is quiet about a unit, and as a unit
/// or the party moves on (README: Transitions).
#[derive(Debug, Clone, Default, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Transition {
    /// Seconds without a post before a unit is quiet; unsaid,
    /// `Transition::AFTER_S`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub after: Option<f32>,
    /// What a quiet unit does.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unit: Option<UnitQuiet>,
    /// How a port lands: a jump faster than `port_yd`, or to another floor.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub port: Option<Port>,
    /// Yards a second past which a jump is a port: further than this for
    /// each second between its two posts, a second at least. Unsaid,
    /// `Transition::PORT_YD`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub port_yd: Option<f32>,
    /// What the camera does as the party moves from one area to another.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub camera: Option<CameraQuiet>,
}

impl Transition {
    /// What `after` means where it is unsaid: 3 s without a post before a
    /// unit is quiet. Longer than the replay holds a unit before it calls
    /// it stale (1.5 s): out of combat the log posts a player every 2 s or
    /// so, and a ghost at every such gap would flicker. Every reader of a
    /// transition (the replay, the map editor beside a key it leaves
    /// unsaid) reads it here.
    pub const AFTER_S: f32 = 3.0;
    /// What `port_yd` means where it is unsaid: 60 yd a second, past which
    /// a jump between two posts is a port.
    pub const PORT_YD: f32 = 60.0;
}

/// What a quiet unit does.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum UnitQuiet {
    /// It stays where it was last posted (today's).
    Hold,
    /// A ghost of it stays where it was last posted.
    Fade,
    /// It is routed along the floor's paths to where it is posted next,
    /// timed to get there as the post does, drawn as a ghost.
    Travel,
}

/// How a port is drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Port {
    /// The unit is drawn at its new post at once.
    Cut,
    /// A flash where it left and where it lands.
    Flash,
}

/// What the camera does as the party moves to another area.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CameraQuiet {
    /// It stays where it is.
    Hold,
    /// It eases over to the new area's home.
    Glide,
    /// It jumps to the new area's home.
    Cut,
}

/// One level of a dungeon, `[level.<ui_map>]` in its instance file, read
/// over the season's and the instance's `[defaults]` (their `map`, `view`
/// and `travel`, never a `pull`). A difficulty never applies to it. Its
/// `map` is the same `Map` an encounter's is, so a reader draws an arena
/// and a level alike; its `view` is what the replay shows while no area
/// holds the party; its `pull`s are the usual route's packs on it.
#[derive(Clone, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Level {
    /// The UiMap id the level is: its key.
    #[serde(skip)]
    pub ui_map: u32,
    /// The instance's slug (its directory).
    #[serde(skip)]
    pub instance: String,
    #[serde(default)]
    pub map: Map,
    #[serde(default)]
    pub view: View,
    #[serde(default)]
    pub travel: Travel,
    /// The usual route's pulls on the level, by key: packs of the
    /// instance's creatures, each in one of the map's areas.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub pull: BTreeMap<String, Pull>,
    /// The instance's creatures (its enabled `[npc]`), laid in by
    /// `Rubric::level`: what a pull's members name, so `crate::check_level`
    /// holds them to it and `Pull::forces` counts them. Never in a file:
    /// the table is the instance's, not the level's, so a level read from
    /// text alone (an editor's) has none; check one so with
    /// `crate::check_level_with` and the instance's `Instance::npcs`. Left
    /// out of a level's equality and its dump.
    #[serde(skip)]
    pub npc: BTreeMap<String, Npc>,
}

/// Two levels are equal by what their file gives and where it lies,
/// whatever creatures were laid into either (`npc`): a level an editor
/// reads from text equals the one `Rubric::level` answers for it.
impl PartialEq for Level {
    fn eq(&self, other: &Level) -> bool {
        // Every field named, so a field added does not build here until it
        // is compared or said not to be.
        let Level {
            ui_map,
            instance,
            map,
            view,
            travel,
            pull,
            npc: _,
        } = self;
        *ui_map == other.ui_map
            && *instance == other.instance
            && *map == other.map
            && *view == other.view
            && *travel == other.travel
            && *pull == other.pull
    }
}

impl Level {
    /// Its pulls in the route's order: by `order`, then by key.
    pub fn pulls_in_order(&self) -> Vec<(&str, &Pull)> {
        let mut pulls: Vec<(&str, &Pull)> =
            self.pull.iter().map(|(k, p)| (k.as_str(), p)).collect();
        pulls.sort_by_key(|(_, p)| p.order);
        pulls
    }

    /// The enemy forces its route gives, without its optional pulls and
    /// with them: `(without, with)`, each the sum of `Pull::forces` over
    /// `pulls_in_order`, a pull switched off left out (a level read from
    /// text alone keeps one). Summed over an instance's levels and set
    /// against its whole (`Instance::forces`), the first says whether the
    /// usual route reaches 100% with its optional pulls left out, and by
    /// how much; the second what they add. Both 0 until the instance's
    /// creatures are laid in (`Level::npc`).
    pub fn route_forces(&self) -> (u32, u32) {
        self.pulls_in_order()
            .into_iter()
            .filter(|(_, p)| p.enabled)
            .fold((0, 0), |(without, with), (_, p)| {
                let f = p.forces(self);
                let without = if p.optional {
                    without
                } else {
                    without.saturating_add(f)
                };
                (without, with.saturating_add(f))
            })
    }
}

/// Written as the derived form was before the level had pulls, so a dump
/// of a level with none (`fixtures/resolved-levels.txt`) reads as it did:
/// `pull` only where it has some, and never `npc`, a copy of the
/// instance's own table.
impl std::fmt::Debug for Level {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Every field named, so a field added does not build here until it
        // is dumped or said not to be.
        let Level {
            ui_map,
            instance,
            map,
            view,
            travel,
            pull,
            npc: _,
        } = self;
        let mut d = f.debug_struct("Level");
        d.field("ui_map", ui_map)
            .field("instance", instance)
            .field("map", map)
            .field("view", view)
            .field("travel", travel);
        if !pull.is_empty() {
            d.field("pull", pull);
        }
        d.finish()
    }
}

/// One pull of a dungeon level's usual route, `[level.<ui_map>.pull.<key>]`:
/// a pack of the instance's creatures standing in one of the level's areas
/// until the party pulls it. The log never shows a pack before it is
/// pulled, so where each member stands (`spots`) comes from a census of
/// the runs' first posts. Its share of the enemy forces is never said: it
/// is its members' (`Pull::forces`), so an area's name need not carry it.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Pull {
    #[serde(default = "yes", skip_serializing_if = "is_true")]
    pub enabled: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Its place in the usual route on its level, from 1; two pulls of a
    /// level never share one. A run may go another way: the replay numbers
    /// its packs by the run's own order.
    pub order: u32,
    /// A pack the usual route may skip at some key levels: a run that
    /// leaves it has not left the route, so it is never counted missing;
    /// the replay draws its members waiting in a lighter gray; and a
    /// route's forces are read with it and without it
    /// (`Level::route_forces`). Which key levels skip it is not said.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub optional: bool,
    /// The area it stands in, a key of the level's `map.area`: never a
    /// boss's court (an area with an `encounter`).
    pub area: String,
    /// Who is in it: keys of the instance's `[npc]`, one a member, so a key
    /// comes as many times as the pack holds that creature.
    pub npc: Vec<String>,
    /// Where each member stands until the pull, world X and Y, by index
    /// with `npc`: one a member, or none.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub spots: Vec<[f32; 2]>,
}

impl Pull {
    /// Its share of the enemy forces: what its members give as they die,
    /// each by the `forces` of its creature in `level`'s (`Level::npc`). A
    /// member giving none, or that the instance lacks, adds nothing; so
    /// does every member of a level read from text alone until its `npc`
    /// is laid in from `Instance::npcs`.
    pub fn forces(&self, level: &Level) -> u32 {
        self.npc
            .iter()
            .filter_map(|k| level.npc.get(k)?.forces)
            .fold(0, u32::saturating_add)
    }
}

/// One glowing liquid: where it lies, its color in the game, and when it
/// brightens. `gen-floors` colors it into the render, and writes its shape
/// as a picture the replay brightens each time `on` fires.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Glow {
    #[serde(default = "yes", skip_serializing_if = "is_true")]
    pub enabled: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Red, green and blue, as the game shows the channel at rest.
    pub color: [u8; 3],
    /// The color it brightens to; unsaid, `color` at full strength.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hot: Option<[u8; 3]>,
    /// Where it lies, its shapes joined in world yards (a stencil's): the
    /// liquid within them takes `color`.
    pub area: Vec<StencilShape>,
    /// Shapes cut out of `area`: liquid the game keeps its own color (the
    /// venom-green pools round a fountain's snake head).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub except: Vec<StencilShape>,
    /// Yards: within `area`, every surface lying further than this under
    /// the arena level takes `color` too (a channel's floor, seen between
    /// the bars of the lattice over it, which keep their own); unsaid,
    /// the liquid alone.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub below: Option<f32>,
    /// When it brightens: each time this fires. Unsaid, never.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub on: Option<Trigger>,
    /// Seconds it takes to brighten, holds and fades; unsaid, 0.6, 2 and 2.5.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rise: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hold: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fade: Option<f32>,
}

impl Glow {
    /// How bright it is `secs` after it fires: 0 before, easing up to 1 over
    /// `rise`, 1 through `hold`, easing down to 0 over `fade` (smoothstep at
    /// both ends).
    pub fn brightness(&self, secs: f32) -> f32 {
        let (rise, hold, fade) = (
            self.rise.unwrap_or(0.6),
            self.hold.unwrap_or(2.0),
            self.fade.unwrap_or(2.5),
        );
        let ease = |t: f32| {
            let t = t.clamp(0.0, 1.0);
            t * t * (3.0 - 2.0 * t)
        };
        if secs < 0.0 {
            0.0
        } else if secs < rise {
            ease(secs / rise.max(f32::EPSILON))
        } else if secs < rise + hold {
            1.0
        } else {
            1.0 - ease((secs - rise - hold) / fade.max(f32::EPSILON))
        }
    }

    /// The color it brightens to: `hot`, else `color` scaled until its
    /// strongest channel is full.
    pub fn hot(&self) -> [u8; 3] {
        self.hot.unwrap_or_else(|| {
            let top = self.color.into_iter().max().unwrap_or(0).max(1);
            self.color
                .map(|c| (u32::from(c) * 255 / u32::from(top)).min(255) as u8)
        })
    }

    /// Whether a world point lies in its area and none of its `except`.
    pub fn covers(&self, p: [f32; 2]) -> bool {
        self.area.iter().any(|s| s.depth(p) >= 0.0)
            && !self.except.iter().any(|s| s.depth(p) >= 0.0)
    }
}

/// One shape of a room's stencil, in world yards (X and Y as the combat log
/// writes them): a polygon by its corners in order round it, or a circle by
/// its middle and radius.
#[derive(Debug, Clone, Default, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct StencilShape {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub points: Vec<[f32; 2]>,
    #[serde(default, alias = "centre", skip_serializing_if = "Option::is_none")]
    pub center: Option<[f32; 2]>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub radius: Option<f32>,
}

impl StencilShape {
    /// Its world bounds, `[x0, y0, x1, y1]`; `None` for a shape that is
    /// neither a polygon nor a circle (`crate::check` says which).
    pub fn bounds(&self) -> Option<[f32; 4]> {
        match (self.points.as_slice(), self.center, self.radius) {
            (ps @ [_, _, _, ..], None, None) => points_bounds(ps),
            ([], Some(c), Some(r)) if r > 0.0 => Some([c[0] - r, c[1] - r, c[0] + r, c[1] + r]),
            _ => None,
        }
    }

    /// How far inside it a world point lies, in yards: negative outside.
    pub fn depth(&self, p: [f32; 2]) -> f32 {
        if let (Some(c), Some(r)) = (self.center, self.radius) {
            return r - (p[0] - c[0]).hypot(p[1] - c[1]);
        }
        let ps = &self.points;
        let (mut inside, mut nearest) = (false, f32::MAX);
        for (a, b) in ps.iter().zip(ps.iter().cycle().skip(1)) {
            if (a[1] > p[1]) != (b[1] > p[1])
                && p[0] < a[0] + (p[1] - a[1]) / (b[1] - a[1]) * (b[0] - a[0])
            {
                inside = !inside;
            }
            nearest = nearest.min(segment_distance(p, *a, *b));
        }
        if inside { nearest } else { -nearest }
    }
}

/// How far a world point lies from the segment `a` to `b`, in yards: from
/// `a` where the two are one point. A polygon's edge (`StencilShape::depth`)
/// and a path's corridor (`Map::level_stencil_depth`) both measure by it.
pub(crate) fn segment_distance(p: [f32; 2], a: [f32; 2], b: [f32; 2]) -> f32 {
    let (ex, ey) = (b[0] - a[0], b[1] - a[1]);
    let len2 = ex * ex + ey * ey;
    let t = if len2 > 0.0 {
        (((p[0] - a[0]) * ex + (p[1] - a[1]) * ey) / len2).clamp(0.0, 1.0)
    } else {
        0.0
    };
    (p[0] - a[0] - t * ex).hypot(p[1] - a[1] - t * ey)
}

/// Points' world bounds, `[x0, y0, x1, y1]`: `None` with no point.
fn points_bounds(ps: &[[f32; 2]]) -> Option<[f32; 4]> {
    ps.iter().map(|p| [p[0], p[1], p[0], p[1]]).reduce(join)
}

/// Two world bounds joined, `[x0, y0, x1, y1]`.
fn join(a: [f32; 4], b: [f32; 4]) -> [f32; 4] {
    [
        a[0].min(b[0]),
        a[1].min(b[1]),
        a[2].max(b[2]),
        a[3].max(b[3]),
    ]
}

impl Map {
    /// The stencil's world bounds, `[x0, y0, x1, y1]`: `None` without one.
    pub fn stencil_bounds(&self) -> Option<[f32; 4]> {
        union_bounds(&self.stencil)
    }

    /// The area a world point lies deepest inside, by key: `None` outside
    /// every one.
    pub fn area_at(&self, p: [f32; 2]) -> Option<(&str, &Area)> {
        self.area
            .iter()
            .map(|(k, a)| (k.as_str(), a, a.depth(p)))
            .filter(|(_, _, d)| *d >= 0.0)
            .max_by(|a, b| a.2.total_cmp(&b.2))
            .map(|(k, a, _)| (k, a))
    }

    /// How far inside the stencil a world point lies, in yards (the
    /// deepest of its shapes): negative outside, and `f32::MAX` with no
    /// stencil, where all of the room is drawn.
    pub fn stencil_depth(&self, p: [f32; 2]) -> f32 {
        self.stencil
            .iter()
            .map(|s| s.depth(p))
            .reduce(f32::max)
            .unwrap_or(f32::MAX)
    }

    /// How much of a world point the stencil keeps, 0 to 1: all of it
    /// half `STENCIL_FEATHER` inside the edge and deeper, none half of it
    /// outside and further, fading between; all of it with no stencil.
    /// What gen-floors cuts a render with, and the replay an older render
    /// cut to fewer shapes.
    pub fn stencil_keep(&self, p: [f32; 2]) -> f32 {
        (self.stencil_depth(p) / STENCIL_FEATHER + 0.5).clamp(0.0, 1.0)
    }

    /// Whether a level's render is cut to its areas: some enabled area
    /// gives a `stencil`.
    pub(crate) fn level_stenciled(&self) -> bool {
        self.area
            .values()
            .any(|a| a.enabled && !a.stencil.is_empty())
    }

    /// The world middles of a path's two areas (their bounds' middles),
    /// `from`'s first: `None` where either is missing, switched off or
    /// without a shape that is one.
    fn path_ends(&self, path: &Path) -> Option<[[f32; 2]; 2]> {
        let middle = |key: &str| {
            let [x0, y0, x1, y1] = self.area.get(key).filter(|a| a.enabled)?.bounds()?;
            Some([(x0 + x1) / 2.0, (y0 + y1) / 2.0])
        };
        Some([middle(&path.from)?, middle(&path.to)?])
    }

    /// The way a path goes, world X and Y, `from`'s end first: its
    /// `points` where it gives two or more, else the straight line from
    /// the middle of `from`'s bounds to the middle of `to`'s. `None` with
    /// neither: an end area missing, switched off or without a shape.
    pub fn path_way(&self, path: &Path) -> Option<Vec<[f32; 2]>> {
        if path.points.len() >= 2 {
            return Some(path.points.clone());
        }
        self.path_ends(path).map(Vec::from)
    }

    /// How far inside a path's corridor a world point lies, in yards: half
    /// its `corridor_width` less its distance from the path's way
    /// (`path_way`); negative outside. `None` for a path with no way.
    fn corridor_depth(&self, path: &Path, p: [f32; 2]) -> Option<f32> {
        let distance = if path.points.len() >= 2 {
            let ps = &path.points;
            ps.iter()
                .zip(ps.iter().skip(1))
                .map(|(a, b)| segment_distance(p, *a, *b))
                .fold(f32::MAX, f32::min)
        } else {
            let [a, b] = self.path_ends(path)?;
            segment_distance(p, a, b)
        };
        Some(path.corridor_width() / 2.0 - distance)
    }

    /// How far inside a level's stencil a world point lies, in yards: the
    /// deepest of every enabled area's `stencil` shapes and every enabled
    /// path's corridor (its way, `path_way`, widened to its
    /// `corridor_width`), negative outside them all. `None` where no
    /// enabled area gives a stencil: the level renders whole, its corridors
    /// cutting nothing. A path with no way gives no corridor. What a
    /// level's render is cut by, as `stencil_depth` is an arena's.
    pub fn level_stencil_depth(&self, p: [f32; 2]) -> Option<f32> {
        if !self.level_stenciled() {
            return None;
        }
        let shapes = self
            .area
            .values()
            .filter(|a| a.enabled)
            .flat_map(|a| &a.stencil)
            .map(|s| s.depth(p));
        let corridors = self
            .path
            .values()
            .filter(|path| path.enabled)
            .filter_map(|path| self.corridor_depth(path, p));
        shapes.chain(corridors).reduce(f32::max)
    }

    /// A level's stencil's world bounds, `[x0, y0, x1, y1]`: every enabled
    /// area's `stencil` shapes and every corridor of `level_stencil_depth`
    /// (its way's bounds padded by half its width) joined. `None` where
    /// no enabled area gives a stencil. Unfeathered, as `stencil_bounds`:
    /// a render pads either by `STENCIL_FEATHER`.
    pub fn level_stencil_bounds(&self) -> Option<[f32; 4]> {
        if !self.level_stenciled() {
            return None;
        }
        let shapes = self
            .area
            .values()
            .filter(|a| a.enabled)
            .flat_map(|a| &a.stencil)
            .filter_map(StencilShape::bounds);
        let corridors = self
            .path
            .values()
            .filter(|path| path.enabled)
            .filter_map(|path| {
                let half = path.corridor_width() / 2.0;
                let [x0, y0, x1, y1] = points_bounds(&self.path_way(path)?)?;
                Some([x0 - half, y0 - half, x1 + half, y1 + half])
            });
        shapes.chain(corridors).reduce(join)
    }

    /// How much of a world point a level's stencil keeps, 0 to 1, by
    /// `stencil_keep`'s rule over `level_stencil_depth`: all of it with no
    /// stencil.
    pub fn level_stencil_keep(&self, p: [f32; 2]) -> f32 {
        self.level_stencil_depth(p)
            .map_or(1.0, |d| (d / STENCIL_FEATHER + 0.5).clamp(0.0, 1.0))
    }

    /// How many shapes a level's render is cut to (its sidecar's
    /// `stencil`): every enabled area's `stencil` shapes and every corridor
    /// of `level_stencil_depth`; 0 where no enabled area gives a stencil.
    pub fn level_stencil_count(&self) -> usize {
        if !self.level_stenciled() {
            return 0;
        }
        let shapes: usize = self
            .area
            .values()
            .filter(|a| a.enabled)
            .map(|a| a.stencil.len())
            .sum();
        let corridors = self
            .path
            .values()
            .filter(|path| {
                path.enabled && (path.points.len() >= 2 || self.path_ends(path).is_some())
            })
            .count();
        shapes + corridors
    }

    /// The ceiling over a world point, yards over the ground there: the
    /// lowest `ceiling` among the enabled areas whose shapes it lies in
    /// (on their edge counts), else the map's own. `None` where neither
    /// says one: the renderer's own.
    pub fn ceiling_at(&self, p: [f32; 2]) -> Option<f32> {
        self.area
            .values()
            .filter(|a| a.enabled)
            .filter_map(|a| a.ceiling.filter(|_| a.depth(p) >= 0.0))
            .reduce(f32::min)
            .or(self.ceiling)
    }

    /// The hang over a world point, yards over the ground there: the
    /// lowest `hang` among the enabled areas whose shapes it lies in (on
    /// their edge counts), as `ceiling_at` takes the lowest ceiling, else
    /// the map's own. `None` where neither says one: nothing hangs there.
    pub fn hang_at(&self, p: [f32; 2]) -> Option<f32> {
        self.area
            .values()
            .filter(|a| a.enabled)
            .filter_map(|a| a.hang.filter(|_| a.depth(p) >= 0.0))
            .reduce(f32::min)
            .or(self.hang)
    }
}

/// How soft a stencil's edge is, in yards: a render reaches this far past
/// the stencil's bounds, and what lies within it of the edge fades
/// (`Map::stencil_keep`).
pub const STENCIL_FEATHER: f32 = 1.0;

/// Shapes' world bounds joined, `[x0, y0, x1, y1]`: `None` with no shape
/// that is one.
fn union_bounds(shapes: &[StencilShape]) -> Option<[f32; 4]> {
    shapes.iter().filter_map(StencilShape::bounds).reduce(join)
}

/// One removable piece of the room: a building placed on the floor.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Layer {
    #[serde(default = "yes", skip_serializing_if = "is_true")]
    pub enabled: bool,
    /// The building's WMO FileDataID.
    pub wmo: u32,
    /// Its placement's unique id, where the WMO is placed more than once.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub placement: Option<u32>,
    #[serde(default)]
    pub breaks: Breaks,
}

/// When a layer gives way.
#[derive(Debug, Clone, Default, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Breaks {
    /// When the players stop standing on it (the replay's reading).
    #[default]
    Occupancy,
    /// It stands all fight.
    Never,
    /// When the phase begins.
    Phase(String),
    /// When the event happens: one that happens again and again breaks one
    /// layer each time, the one nearest where its caster stood where a unit
    /// casts it, else the one the players stopped standing on nearest that
    /// time (README).
    Event(String),
}

/// The replay's default view of the room.
#[derive(Debug, Clone, Default, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct View {
    /// Degrees clockwise on screen; unsaid, north up (the game's +X at the
    /// top). `turn::room_turn` proposes one from a pull (the extractor's
    /// `suggest-turns`, the map editor's suggestion); no render reads it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub turn: Option<f32>,
    /// How much closer than the whole arena the view opens.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub zoom: Option<f32>,
    /// The closest view, times closer than the whole arena; unsaid, 8.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub zoom_max: Option<f32>,
    /// Where the view opens, world X and Y; unsaid, the arena's middle.
    #[serde(default, alias = "centre", skip_serializing_if = "Option::is_none")]
    pub center: Option<[f32; 2]>,
    /// The arena: the box the view keeps to, world X and Y from one corner
    /// to the other (`[x0, y0, x1, y1]`). The furthest view shows all of it
    /// and the view never pans past it. Unsaid, where the players went.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub arena: Option<[f32; 4]>,
}

/// A creature in the fight, or, in an instance file's `[npc]`, in the
/// dungeon.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Npc {
    #[serde(default = "yes", skip_serializing_if = "is_true")]
    pub enabled: bool,
    pub name: String,
    /// Creature ids (a GUID's sixth field) that are this NPC.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub creature: Vec<u32>,
    #[serde(default)]
    pub role: Role,
    /// How it can be crowd-controlled; unsaid, every control lands.
    #[serde(default, skip_serializing_if = "is_all")]
    pub control: Control,
    /// The icon's FileDataID (the journal's portrait, else none).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon: Option<u32>,
    /// The NPC that summons or owns it (an enemy's pet).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner: Option<String>,
    /// The NPC whose health it shares (Ula'tek's Venomous Heart, bound to
    /// Ula'tek): one pool, which the replay shows once, on that NPC's
    /// frame, with this one's frame under it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shares: Option<String>,
    /// It never moves (a totem): wherever the log places it once, it
    /// stood from its first act until it died, though the log places it
    /// only as it finishes a cast.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub still: bool,
    /// What a keystone's enemy forces gain as one of it dies, by the
    /// season's forces table; unsaid, none (a boss, a pet, a raid's NPC).
    /// Counted by its `creature` ids alone.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub forces: Option<u32>,
    /// The phases (their keys) it is in the air through: a boss that takes
    /// wing. Empty, it never is.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub aloft: Vec<String>,
    /// Difficulty names it appears on; empty, every one.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub only: Vec<String>,
}

/// How a creature can be crowd-controlled, as the stuns and incapacitates
/// the log shows landing on it or failing say: what a pull of it asks of a
/// party that would stop its casts or hold it still.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Control {
    /// Every control lands.
    #[default]
    All,
    /// Stuns and incapacitates land, but no full control.
    Stun,
    /// Nothing lands.
    Immune,
}

fn is_all(c: &Control) -> bool {
    *c == Control::All
}

/// What an NPC is to the replay.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    Boss,
    #[default]
    Add,
    /// An enemy's pet or summon: drawn with its owner.
    Pet,
    /// A mechanic that is a unit (an orb, a totem): no health arc.
    Object,
    /// Not drawn.
    Ignore,
}

impl Role {
    pub const ALL: [Role; 5] = [Role::Boss, Role::Add, Role::Pet, Role::Object, Role::Ignore];

    /// Its name, as the rubric's TOML and the portraits index write it.
    pub const fn name(self) -> &'static str {
        match self {
            Role::Boss => "boss",
            Role::Add => "add",
            Role::Pet => "pet",
            Role::Object => "object",
            Role::Ignore => "ignore",
        }
    }

    /// A role by its `name`.
    pub fn from_name(s: &str) -> Option<Role> {
        Role::ALL.into_iter().find(|r| r.name() == s)
    }
}

/// A hostile ability.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Ability {
    #[serde(default = "yes", skip_serializing_if = "is_true")]
    pub enabled: bool,
    pub name: String,
    /// The spell ids that are this ability (a cast and its damage, say).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub spell: Vec<u32>,
    /// The NPC that uses it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub by: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shape: Option<Shape>,
    /// Seconds it lasts on the ground.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lifetime: Option<f32>,
    /// The game's own timeline alerts on it (`EncounterEvent`), at this
    /// severity (0–2).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub alert: Option<u8>,
    /// The journal's role and warning marks (tank, healer, deadly,
    /// interrupt, magic …).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub marks: Vec<String>,
    /// The journal stage it is listed under.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub phase: Option<String>,
    /// The ability the journal lists it under, through any creature header
    /// between: Soulcoiled under Soulcoiler's Curse, the Drowned Echo's, under
    /// Grasping Depths. What a section rolls up.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub under: Option<String>,
    /// The journal's own description of it, its `$` tokens filled in from
    /// the spell tables, as the difficulty reads it; for an ability only the
    /// timeline lists, its spell's description. Filled at runtime from the
    /// per-machine sidecar (`text`, `Rubric::with_texts`), never written
    /// into a committed file: Blizzard's words stay out of the repository.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    /// Difficulty names it appears on; empty, every one.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub only: Vec<String>,
}

/// An ability's footprint on the ground, in yards and degrees.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Shape {
    Circle { radius: f32 },
    Ring { inner: f32, outer: f32 },
    Cone { angle: f32, length: f32 },
    Line { width: f32, length: f32 },
}

/// A stage of the fight.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Phase {
    #[serde(default = "yes", skip_serializing_if = "is_true")]
    pub enabled: bool,
    pub name: String,
    /// Its place among the stages.
    #[serde(default)]
    pub order: u32,
    /// What begins it: any one of these.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub enter: Vec<Trigger>,
    /// It begins again each time `enter` fires, and ends at `leave`: an
    /// intermission the fight comes back to (Entombed Sentinels has four).
    /// Between times the fight is in the phase before it.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub again: bool,
    /// What ends a phase that comes `again`: the first of these after it
    /// began. Unsaid, it lasts until it begins again.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub leave: Vec<Trigger>,
    /// Difficulty names it appears on; empty, every one.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub only: Vec<String>,
}

/// Something the log shows that a phase, an event or a layer keys on. A
/// key a variant's table does not know is refused, as a struct's is.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum Trigger {
    /// The pull begins.
    Start,
    /// Seconds into the pull, or after another trigger fired (`Clock`).
    After(Clock),
    /// A hostile cast landing (`SPELL_CAST_SUCCESS`): a cast cut off never
    /// fires.
    Cast {
        spell: u32,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        by: Option<String>,
    },
    /// A hostile cast beginning (`SPELL_CAST_START`): one with a cast time,
    /// as its caster starts it, whether it lands or is cut off. An instant
    /// cast has no start. As a ground's `from` beside its landing's `at`,
    /// a frontal is drawn while it is cast (a boss's cone it casts for a
    /// few seconds).
    CastStart {
        spell: u32,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        by: Option<String>,
    },
    /// A hostile aura applied (on the NPC named, else anyone).
    AuraApplied {
        spell: u32,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        on: Option<String>,
    },
    AuraRemoved {
        spell: u32,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        on: Option<String>,
    },
    /// An NPC's health falls under a share (percent).
    HealthBelow { npc: String, pct: f32 },
    /// An NPC takes no damage for this long: a stage it cannot be hurt in.
    Plateau { npc: String, secs: f32 },
    /// An NPC is first seen.
    Appears { npc: String },
    /// An NPC dies.
    Death { npc: String },
    /// A hostile spell hits. With `lands`, only as it lands, never at a
    /// periodic tick the same spell writes after (`crate::lands`): Frost
    /// Orb's touch, not the 12 s of its dot.
    Hit {
        spell: u32,
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        lands: bool,
    },
    /// The last player carrying a hostile aura loses it: the raid is clear
    /// of it (the end of an intermission's Helical Toxins).
    AuraGone { spell: u32 },
    /// A named event happens.
    Event(String),
    /// A phase begins.
    Phase(String),
}

/// When an `after` trigger strikes: `secs` into the pull (`{ after = 353 }`),
/// or `secs` after the `nth` time its `since` trigger fired (`{ after = {
/// secs = 68.5, since = { cast = … }, nth = 2 } }`: Ula'tek's Shattering,
/// 68.5 s after her second Rage of the Shackled on every difficulty, whose
/// clocks differ), or with `each`, `secs` after every time it fired (`{
/// after = { secs = 9, since = { cast = … }, each = true } }`: the second
/// gale of each storm a boss casts, 9 s into it). A pull that
/// ended first, or where `since` fired fewer times, never strikes it.
/// Written as a bare number when it counts from the pull, so a later layer
/// giving a number replaces the whole clock, and one giving `{ secs = … }`
/// retimes a `since` clock and keeps the rest.
#[derive(Debug, Clone, PartialEq)]
pub struct Clock {
    pub secs: f32,
    /// What it counts from; none, the pull's start.
    pub since: Option<Box<Trigger>>,
    /// Which time `since` fired it counts from, the first being 1; unsaid,
    /// the first (`Clock::nth_or_first`). Held as said, so `check` can
    /// refuse one beside `each`, as a layer may lay one over the other.
    /// It means nothing without a `since`.
    pub nth: Option<u32>,
    /// It strikes `secs` after every time `since` fired, each one the pull
    /// reached, not after one of them: `check` refuses it beside an `nth`.
    /// Written only where it is true. It means nothing without a `since`.
    pub each: bool,
}

impl Clock {
    /// `secs` into the pull.
    pub fn pull(secs: f32) -> Clock {
        Clock {
            secs,
            since: None,
            nth: None,
            each: false,
        }
    }

    /// Which time `since` fired it counts from: its `nth`, else the first.
    pub fn nth_or_first(&self) -> u32 {
        self.nth.unwrap_or(1)
    }
}

/// A clock's table form, as read.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Since {
    secs: f32,
    since: Box<Trigger>,
    #[serde(default)]
    nth: Option<u32>,
    #[serde(default)]
    each: bool,
}

/// A clock's table form, as written.
#[derive(Serialize)]
struct SinceRef<'a> {
    secs: f32,
    since: &'a Trigger,
    #[serde(skip_serializing_if = "Option::is_none")]
    nth: Option<u32>,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    each: bool,
}

impl Serialize for Clock {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        match &self.since {
            None => s.serialize_f32(self.secs),
            Some(since) => SinceRef {
                secs: self.secs,
                since,
                nth: self.nth,
                each: self.each,
            }
            .serialize(s),
        }
    }
}

impl<'de> Deserialize<'de> for Clock {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Clock, D::Error> {
        use serde::de::{self, MapAccess, Visitor};
        struct Seconds;
        impl<'de> Visitor<'de> for Seconds {
            type Value = Clock;
            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                f.write_str(
                    "seconds into the pull, or a table of `secs`, `since` and `nth` or `each`",
                )
            }
            fn visit_f64<E: de::Error>(self, v: f64) -> Result<Clock, E> {
                Ok(Clock::pull(v as f32))
            }
            fn visit_i64<E: de::Error>(self, v: i64) -> Result<Clock, E> {
                Ok(Clock::pull(v as f32))
            }
            fn visit_u64<E: de::Error>(self, v: u64) -> Result<Clock, E> {
                Ok(Clock::pull(v as f32))
            }
            // The table's own keys and errors: an unknown one is refused.
            fn visit_map<A: MapAccess<'de>>(self, map: A) -> Result<Clock, A::Error> {
                let s = Since::deserialize(de::value::MapAccessDeserializer::new(map))?;
                Ok(Clock {
                    secs: s.secs,
                    since: Some(s.since),
                    nth: s.nth,
                    each: s.each,
                })
            }
        }
        d.deserialize_any(Seconds)
    }
}

/// A fixed feature of the floor: a puddle, a pool, a spawn point.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Place {
    #[serde(default = "yes", skip_serializing_if = "is_true")]
    pub enabled: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// What it is: "puddle", "pool", "spawn", "soak" …
    pub kind: String,
    /// The color the game draws it in ("purple", "green" …).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tint: Option<String>,
    /// World X and Y, as the combat log writes positions.
    pub at: [f32; 2],
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub radius: Option<f32>,
    /// Its surface, where it is liquid.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub liquid_height: Option<f32>,
    /// A swirling texture filling its circle (`at`, `radius`): a spell's
    /// visual the game draws over it, not the map's geometry.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub swirl: Option<Swirl>,
    /// When it is there: it opens each time this fires and closes at the
    /// next `until` (`Place::windows`); unsaid, it is there from the start.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub from: Option<Trigger>,
    /// What closes it, the first firing after it opened; unsaid, it stays
    /// to the end.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub until: Option<Trigger>,
    /// Difficulty names it appears on; empty, every one.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub only: Vec<String>,
}

impl Place {
    /// Whether it is there only some of the fight: it gives a `from` or an
    /// `until`.
    pub fn comes_and_goes(&self) -> bool {
        self.from.is_some() || self.until.is_some()
    }

    /// When it is there, from `start` to `end`, given every time its `from`
    /// and its `until` fired (each read only where the place gives it; in
    /// any order): it opens at each firing of `from` (unsaid, at `start`)
    /// and closes at the first firing of `until` after it opened (unsaid,
    /// or none after, at `end`). A `from` firing while it is open is passed
    /// over, so a place cast again before it closes stays one window;
    /// `until` closes it at its first firing, the first hit of a burst. One
    /// that opened before `start` and is open still at it (a keystone
    /// fight's clock, its stretch of the run) is there from `start`. Each
    /// window is `(opened, closed)`, in time order, none empty. The one
    /// reading of a place's windows: the replay's and the editors'.
    pub fn windows<T: Copy + PartialOrd>(
        &self,
        from: &[T],
        until: &[T],
        start: T,
        end: T,
    ) -> Vec<(T, T)> {
        let sorted = |v: &[T]| {
            let mut v = v.to_vec();
            v.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
            v
        };
        let opens = match self.from {
            Some(_) => sorted(from),
            None => vec![start],
        };
        let closes = match self.until {
            Some(_) => sorted(until),
            None => Vec::new(),
        };
        let mut out: Vec<(T, T)> = Vec::new();
        // When the last window closed, by the firings' own times.
        let mut shut: Option<T> = None;
        for o in opens {
            // From the end on, or while it is open.
            if o.partial_cmp(&end) != Some(std::cmp::Ordering::Less) || shut.is_some_and(|c| o < c)
            {
                continue;
            }
            let c = closes.iter().copied().find(|&c| c > o).unwrap_or(end);
            shut = Some(c);
            let c = if c < end { c } else { end };
            // Open before the start: there from it, if still open then.
            let o = if o < start { start } else { o };
            if o < c {
                out.push((o, c));
            }
        }
        out
    }
}

/// A place's swirling texture: the game's gray picture, colored between
/// two colors, twisted toward the middle and turning. It fills the place's
/// circle, feathered over the outer 5% of its radius, over the floor and
/// under the place's ring and label. It is a spell's visual (the Soulcoil
/// Well's), not the map's geometry, so no render of the floor bakes it:
/// `crate::swirl::disc` draws it, the one look for the replay (turning it
/// with the playhead) and the map editor (still).
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Swirl {
    /// The texture's FileDataID: a gray, tileable picture of the game's.
    pub texture: u32,
    /// Dark then light, each `#rrggbb`: the texture's gray, leveled from
    /// 5% to 95%, runs from one to the other (`Swirl::rgb`).
    pub colors: [String; 2],
    /// Degrees the middle is turned, falling off to none at the rim:
    /// `twist · (1 − r / R)²` at `r` of the radius `R`, as ImageMagick's
    /// `-swirl` turns a picture.
    pub twist: f32,
    /// Degrees a second the whole disc turns, clockwise on screen
    /// (`Swirl::turned`).
    pub spin: f32,
}

impl Swirl {
    /// The colors as red, green and blue, dark then light; `None` where
    /// either is not `#rrggbb` (`check` names which).
    pub fn rgb(&self) -> Option<[[u8; 3]; 2]> {
        let [dark, light] = &self.colors;
        Some([hex_rgb(dark)?, hex_rgb(light)?])
    }

    /// How far the disc has turned clockwise on screen `secs` on (the
    /// replay's playhead): `spin` degrees a second, in radians in [0, 2π),
    /// the `angle` `crate::swirl::disc` takes. Reduced in degrees first, so
    /// a long fight keeps its precision.
    pub fn turned(&self, secs: f32) -> f32 {
        let a = (self.spin * secs).rem_euclid(360.0).to_radians();
        if a < std::f32::consts::TAU { a } else { 0.0 }
    }
}

/// `#rrggbb` as red, green and blue; `None` of anything else (no `#`, three
/// digits, a letter past `f`, a sign).
pub(crate) fn hex_rgb(s: &str) -> Option<[u8; 3]> {
    let digits = s.strip_prefix('#')?.as_bytes();
    if digits.len() != 6 {
        return None;
    }
    let nibble = |b: u8| char::from(b).to_digit(16);
    let mut out = [0u8; 3];
    for (o, [hi, lo]) in out.iter_mut().zip(digits.as_chunks::<2>().0) {
        *o = u8::try_from(nibble(*hi)? * 16 + nibble(*lo)?).ok()?;
    }
    Some(out)
}

/// Something the fight leaves on the floor, once each time the log shows
/// it: a pool a debuff drops as it comes off, a droplet a player soaks, a
/// puddle an add leaves where it dies. Unlike a `place` it has no fixed
/// spot: each one lands where the unit its trigger is about stood (an
/// aura's target, a hit's victim, a cast's caster, an NPC that appears or
/// dies).
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Ground {
    #[serde(default = "yes", skip_serializing_if = "is_true")]
    pub enabled: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// What it is to the players: "pool" (ground to stay off), "soak" (one
    /// to step on), "puddle" …
    pub kind: String,
    /// The color the game draws it in, so the replay's matches: one of the
    /// rubric's names (`crate::tint::NAMES`), as every tint is.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tint: Option<String>,
    pub shape: Shape,
    /// What lands one: any of these, each time it fires, where its unit
    /// stood.
    pub at: Vec<Trigger>,
    /// Seconds after its trigger it forms.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub delay: Option<f32>,
    /// Yards in front of its unit it lands, along the way the unit faced as
    /// `at` fired: a blow that falls ahead of whoever casts it (a Spectral
    /// Coil crushes the floor 21.5 yd in front of itself). Unsaid, where the
    /// unit stood.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ahead: Option<f32>,
    /// It was there from the last time this fired before its trigger: a
    /// ground the log shows only as it ends (a droplet seen as it is
    /// soaked).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub from: Option<Trigger>,
    /// When it goes: with the pull (unsaid), as `at` fires, seconds after
    /// it forms, or at the first hit of a spell on someone standing in it.
    #[serde(default, skip_serializing_if = "Ends::is_pull")]
    pub ends: Ends,
    /// A firing of `at` that lands inside one of it still on the floor
    /// renews that one instead of placing another: a ground the log shows
    /// only as it hurts someone standing in it, whose every tick would
    /// otherwise place a new one (a residue that hurts by ticks). On the floor
    /// is formed and not yet gone: a firing inside one still in its
    /// `delay`, not yet formed, places another. Inside is where a new one
    /// would land (`ahead` of its unit where it says so), within its
    /// footprint as it is drawn then: a circle at its radius after `grow`
    /// and `spread`, a line or a cone along its bearing, each where its
    /// `orbit` and its `seen` hits have taken it. Each one it lands in is
    /// renewed: an `{ after = s }` counts again from this firing, as one
    /// this firing placed would (its `delay` after it, then `s`), a
    /// `{ hit }` end and the pull's are kept, and nothing new is placed.
    /// A renewal is not a new one forming: it keeps its footprint as it
    /// first formed (`grow = "stacks"` at its first firing's stacks, its
    /// `spread` not begun again). The firings are read in time order
    /// across every `at` trigger, the hits of its `seen` and its `ends`
    /// among them, each against the grounds as it found them (a `seen` hit
    /// at a firing's moment after it, an ending one before), and where its
    /// `push` has taken them by then: each one is given the pushes under
    /// way or to come as it forms (one under way for the rest of it) as it
    /// is placed, so a renewal finds it
    /// where it is drawn (a residue blown across a platform: a tick inside
    /// a puddle where the wind has taken it renews it). One
    /// that `falls` and is gone is not standing. `check` refuses it beside
    /// a `from` (one would stand before the firing that places it), `ends
    /// = "at"` (each gone as it lands, none stands to renew) and a `seen`
    /// spell one of its `hit` triggers places it by (each hit would drag
    /// it to its victim, so it would follow them).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub renew: bool,
    /// One that formed inside the encounter's map stencil is gone the first
    /// moment its spot lies outside it, where its `push` or its `orbit` has
    /// taken it: a puddle blown off the platform by a storm's gales. Its end is the first of
    /// that moment and the one it would have had, and no later renewal
    /// lasts past it. Each `seen` hit that finds it again is a fresh
    /// witness of where it is: formed or last found again outside the
    /// stencil (in a room off the arena), it never falls; found again
    /// inside, it falls when it next leaves. `check` refuses it on a ground
    /// that neither
    /// moves (no `push`, no `orbit`) nor has a stencil to fall off (the
    /// encounter's `map.stencil`; a trash file gives no map, so its grounds
    /// never fall).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub falls: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub grow: Option<Grow>,
    /// Yards a second it widens once it forms, from the shape's radius to
    /// its full size: a four-stack Blood Venom pool forms at 6 yd and
    /// reaches 24 about five seconds later. Unsaid, it forms whole.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub spread: Option<f32>,
    /// While the aura its `at` waits on is still on a player, a dashed ring
    /// under them says one is coming, and in how long.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub preview: bool,
    /// A line or a cone that turns while it stands: each moment it points
    /// from its spot toward a unit as the replay draws it then (its cast's
    /// target, or the player carrying an aura), or along its trigger's
    /// unit's facing as the log posts it (`Follows`). Its spot stays where
    /// it formed, or where a `push` or a `seen` hit takes it, and it follows
    /// the unit it picked as it fired for as long as it stands.
    /// A beam a boss aims at the tank turns with the tank as it moves round
    /// the boss. Unsaid, it keeps the way its unit faced
    /// as its trigger fired. `check` refuses it on a circle or a ring,
    /// beside an `orbit` (it turns one way) or an `ahead` (it would turn
    /// about a spot ahead of its unit, not the unit), `"target"` by a
    /// trigger that is no cast (its target would be the unit it lands on),
    /// and an aura of spell 0.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub follows: Option<Follows>,
    /// It circles a place (or an NPC) once it forms, at the distance it
    /// formed at: Nek'zali's Latent Cultists round the Soulcoil Well. The
    /// log never places it again, so it is drawn as inferred.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub orbit: Option<Orbit>,
    /// Each time one of its triggers fires, every one of it on the floor
    /// moves along the next of some bearings in turn, or the way the
    /// players drifted then: a ground the fight shoves about. The log never
    /// places it again, so it is drawn as inferred, as an orbit is, and it
    /// never orbits too.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub push: Option<Push>,
    /// Spells it deals to the players it touches: each hit places it at
    /// its victim again, so a ground that moves where the log cannot follow
    /// it is found anew whenever it hurts someone.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub seen: Vec<u32>,
    /// Other grounds (their keys) it sweeps away as it forms: each one of
    /// them standing then whose spot lies inside its footprint is gone from
    /// that moment. A tank's frontal cone that destroys the orbs it lands on
    /// is the case: the log writes no row for an orb so destroyed, so without it an orb lasts until its pickup or
    /// the pull's end. Forms is its `from` moment, after its `delay` (a
    /// renewal is not a forming); standing is formed and not yet gone; its
    /// footprint and their spots as each is drawn then (after `grow` and
    /// `spread`, along its bearing, where `orbit`, `push` and `seen` have
    /// taken each). A volley's `destroys` is the same between a volley and
    /// NPCs. A key may be any ground of the encounter's file on any
    /// difficulty: one the difficulty resolved does not have (kept to
    /// others by `only`, or switched off) is dropped as it resolves, and
    /// clears nothing. A ground it clears is read after it, each clear a
    /// moment in that ground's own reading, so one that `renew`s places
    /// another at a firing after the clear and a `seen` hit after it finds
    /// a neighbor still standing. `check` refuses a key that is no ground
    /// of the encounter, the ground itself, and a ground that clears or
    /// sweeps this one in turn (by any way round): each is read after
    /// those that clear it.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub clears: Vec<String>,
    /// Other grounds (their keys) it sweeps away all the while it stands,
    /// not only as it forms: each one of them is gone the first moment its
    /// own shape meets this one's footprint while both stand, each as
    /// drawn then: any overlap, not its spot alone as `clears` reads it.
    /// A still one meets it at the later of the two forming (one formed
    /// under it is gone as it forms); a moving or turning one where they
    /// first touch. A beam that follows the tank washes away the frost
    /// patches it touches, a puddle going as the beam reaches its rim. Read and refused as
    /// `clears` is, its keys dropped as it resolves where the difficulty
    /// lacks them.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sweeps: Vec<String>,
    /// Difficulty names it appears on; empty, every one.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub only: Vec<String>,
}

impl Trigger {
    /// Whether it is about a unit, so a ground can land where it stood: an
    /// aura's target, a hit's victim, a cast's caster (as it lands or as it
    /// starts), an NPC that appears or dies. The pull's start, a clock,
    /// health, a phase or an event name no one.
    pub fn marks_a_spot(&self) -> bool {
        matches!(
            self,
            Trigger::Cast { .. }
                | Trigger::CastStart { .. }
                | Trigger::AuraApplied { .. }
                | Trigger::AuraRemoved { .. }
                | Trigger::Appears { .. }
                | Trigger::Death { .. }
                | Trigger::Hit { .. }
        )
    }

    /// Whether it can name a unit apart from the one it lands on, a
    /// ground's `follows = "target"`: a cast's target as it lands or as it
    /// starts, where the log's row names one. An aura's carrier and a hit's
    /// victim are the unit a ground lands on, so a ground there would
    /// follow itself; an NPC that appears or dies is about itself; the rest
    /// name no one.
    pub fn names_a_target(&self) -> bool {
        matches!(self, Trigger::Cast { .. } | Trigger::CastStart { .. })
    }
}

/// What a line or a cone turns to while it stands (`Ground::follows`):
/// written `"target"`, `"facing"` or `{ aura = N }`. Each moment it points
/// from its spot toward the unit it follows, as that unit is drawn then
/// (between its posts where they are close, else at its last), or along
/// its unit's facing; before that unit is posted, or while it stands on the
/// spot, the way it last pointed, from the way its unit faced as its
/// trigger fired (`crate::follow`, the one reading for the replay and the
/// editors).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum Follows {
    /// Toward its cast's target: the unit a cast was aimed at, as it lands
    /// or as it starts, where the log's row names one. A firing whose row
    /// names none keeps the way its caster faced.
    Target,
    /// Along its trigger's unit's facing, as the log posts it each moment:
    /// a frontal its caster turns while it stands.
    Facing,
    /// Toward the player carrying this aura (a spell id; the log's
    /// hostile auras on players alone), as `follow::carrier` picks them:
    /// the first it goes on from the firing, within
    /// `follow::AURA_WITHIN_MS`, else the latest to take it still carrying
    /// it then. Where the log's cast row names no target but the spell's
    /// aura on its victim goes on with it (a beam's aura on the tank).
    Aura(u32),
}

/// An aura the replay draws on each player while they carry it, with its
/// count: Helical Toxins' 1–3 applications that two players clear by
/// colliding into exactly 4.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Mark {
    #[serde(default = "yes", skip_serializing_if = "is_true")]
    pub enabled: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// The aura's spell id: while it is on a player, they wear the mark.
    pub aura: u32,
    /// How its count is read; unsaid, the aura's stacks.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub count: Option<Count>,
    /// The count that clears it: the aura comes off harmlessly as it
    /// reaches this, and a count past it is a failure. Two players who
    /// meet are joined by a line: green when they make the goal, amber
    /// under it, red past it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub goal: Option<u32>,
    /// A ring round each carrier, in this color, in place of the count's
    /// slots: an aura that matters for being on, not for how much.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ring: Option<String>,
    /// Words by the ring: how long it has been on, or how long until it
    /// comes off.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<MarkLabel>,
    /// Each carrier is joined to the nearest other: the partner to touch,
    /// when two carriers clear it by meeting (Shifting Protovenom).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub link: bool,
    /// Spells whose hits, as it comes off, are its soakers: the replay
    /// counts them ("soaked by 8": Unstable Miasma's burst).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub soak: Vec<u32>,
    /// A dot in this color on each carrier, larger with its stacks: the
    /// side a player is on (Mark of Blood red, Mark of Acid green).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dot: Option<String>,
    /// Spells it sets off on players who do not carry it: a carrier who
    /// touches one (Protovenom Eruption). The replay names who they hit and
    /// the carrier nearest them, and counts the mark's wave as gone wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub harm: Vec<u32>,
    /// Difficulty names it appears on; empty, every one.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub only: Vec<String>,
}

/// A ground circling a place or an NPC.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Orbit {
    /// The place (a `[place]` key) or NPC it circles.
    pub around: String,
    /// Degrees a second, counter-clockwise seen from above; clockwise is
    /// negative.
    pub turn: f32,
}

/// What pushes a ground along the floor: each time one of `on` fires, every
/// one of its entry on the floor moves along the next of `toward`'s
/// bearings in turn (the first firing along the first, round again after
/// the last), or the way the players drifted then, at `speed` for `secs`.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Push {
    /// What pushes them: each time any of these fires, every firing of
    /// every one a push, in time order across them; one within a second of
    /// the last counted is that push (a hit on five players is one). Read
    /// as one trigger or a list, always written as a list: the three gales
    /// of a boss's storm, each an `after` counting from its cast
    /// (`Clock::each`). `check` refuses an empty list.
    #[serde(deserialize_with = "one_or_many")]
    pub on: Vec<Trigger>,
    /// Which way each push goes (`Toward`).
    pub toward: Toward,
    /// Yards a second it moves while pushed.
    pub speed: f32,
    /// Seconds each push lasts.
    pub secs: f32,
}

/// Which way a push goes: a list of bearings, degrees in the world as a
/// volley's `toward` reads them (0 along the X the combat log writes, 90
/// along its Y), the firings taking them in turn; or the word `"drift"`:
/// the way the living players drifted over each push, `secs` from its
/// firing (`crate::drift`), the log's only witness of a wind it never
/// writes. A push whose drift reads nothing moves none.
#[derive(Debug, Clone, PartialEq)]
pub enum Toward {
    Bearings(Vec<f32>),
    Drift,
}

impl Toward {
    /// The word a drift is written as.
    pub const DRIFT: &str = "drift";
}

impl Serialize for Toward {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        match self {
            Toward::Bearings(b) => b.serialize(s),
            Toward::Drift => s.serialize_str(Toward::DRIFT),
        }
    }
}

impl<'de> Deserialize<'de> for Toward {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Toward, D::Error> {
        use serde::de::{self, SeqAccess, Visitor};
        struct Way;
        impl<'de> Visitor<'de> for Way {
            type Value = Toward;
            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                write!(f, "a list of bearings in degrees, or {:?}", Toward::DRIFT)
            }
            fn visit_str<E: de::Error>(self, v: &str) -> Result<Toward, E> {
                if v == Toward::DRIFT {
                    Ok(Toward::Drift)
                } else {
                    Err(E::invalid_value(de::Unexpected::Str(v), &self))
                }
            }
            fn visit_seq<A: SeqAccess<'de>>(self, seq: A) -> Result<Toward, A::Error> {
                Vec::<f32>::deserialize(de::value::SeqAccessDeserializer::new(seq))
                    .map(Toward::Bearings)
            }
        }
        d.deserialize_any(Way)
    }
}

/// A list of triggers, or one alone (`Push::on`): read either way, held as
/// a list. Read through its TOML value, so a lone trigger is read exactly
/// as one in a list is: a table naming two kinds (laid key by key from two
/// layers) is refused as any trigger's is.
fn one_or_many<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Vec<Trigger>, D::Error> {
    use serde::de::Error;
    let v = toml::Value::deserialize(d)?;
    match v {
        toml::Value::Array(_) => Vec::<Trigger>::deserialize(v),
        v => Trigger::deserialize(v).map(|t| vec![t]),
    }
    .map_err(D::Error::custom)
}

/// A room off the main one that players are taken to and come back from,
/// entered through a place (Nek'zali's well, which the players assigned to
/// it drop through to kill a Drowned Echo) or ported to. It can be a
/// building of its own whose yards overlap the arena's (Nek'zali's
/// Venomfall Deeps, its own UiMap on its own map), and the log writes no
/// map change, so only the way in says who is there: the replay draws them
/// apart, in an inset of their own, while they are, and `gen-floors`
/// renders it from its own `ui_map`.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Room {
    #[serde(default = "yes", skip_serializing_if = "is_true")]
    pub enabled: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// The ability whose journal section tells of it (Grasping Depths): its
    /// help is that section rolled up, every ability `under` it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub about: Option<String>,
    /// The place it is entered through. Unsaid, a player is in the room
    /// while they stand within its `radius` of its `center`: a room
    /// elsewhere, with yards of its own.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub through: Option<String>,
    /// What takes a player in: one of these about them while they stand in
    /// the `through` place (Grasping Depths coming off a player in the
    /// well: the raid's own removal finds them far from it).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub enter: Vec<Trigger>,
    /// What brings a player back: the first of these about them after they
    /// went in; else the last of its `npc` dying, else the pull's end.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub leave: Vec<Trigger>,
    /// The NPCs that live in it (the Drowned Echo): drawn in its inset,
    /// never on the arena. The way in is open while one of them lives.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub npc: Vec<String>,
    /// Its middle, world X and Y; unsaid, the `through` place's.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub center: Option<[f32; 2]>,
    /// How far round its middle it reaches, yards: what is rendered and
    /// what the inset shows.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub radius: Option<f32>,
    /// The map floor (UiMap) it is on, where it is one of its own: a room an
    /// encounter ports players to, on a map of its own with its own yards.
    /// Unsaid, the arena's.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ui_map: Option<u32>,
    /// Its floor's height, yards: what `gen-floors` renders it at; unsaid,
    /// derived as an arena's is.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub level: Option<f32>,
    /// Yards over its level that are drawn, cutting away the floor above.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ceiling: Option<f32>,
    /// How `gen-floors` lights it: unsaid, as its map is.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub light: Option<Light>,
    /// How many times its render's size its picture is drawn, round its
    /// middle: a phased copy smaller than the floor it copies. Unsaid, 1.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scale: Option<f32>,
    /// Difficulty names it appears on; empty, every one.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub only: Vec<String>,
}

/// How a render is lit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Light {
    /// Every pixel graded by the building's ambient: the old look, matched
    /// by eye to The Coiled Altar.
    Graded,
    /// As the client lights it (the default): a building's indoor groups by
    /// the light baked into their vertices (`MOCV`) and its props by their
    /// placements' (`MODD`), the ground, outdoor groups and props on it by
    /// the sun on their facing, all over the map's ambient from its `Light`
    /// tables: Nek'zali's teal, which the grade turned olive.
    Baked,
}

/// NPCs that must not reach a place: Restless Amani walking to the
/// Soulcoil Well. One that gets within the place's radius alive is a
/// fault.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Reach {
    #[serde(default = "yes", skip_serializing_if = "is_true")]
    pub enabled: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    pub npc: String,
    /// The `[place]` it must not reach.
    pub place: String,
    /// The place takes in a unit that gets there: one the log never posts
    /// again after it got there is drawn sliding to the place's middle and
    /// gone, counted absorbed rather than dead (an add drawn into a bubble,
    /// last posted alive at its edge, with no death).
    /// Unsaid, it is not.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub absorbs: bool,
    /// Difficulty names it appears on; empty, every one.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub only: Vec<String>,
}

/// What a mark's ring says.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MarkLabel {
    /// Seconds it has been on: "Blighted Blood 6 s", left until a healer
    /// dispels it.
    Carried,
    /// Seconds until it comes off, as the log has it: "bursts in 3 s".
    Left,
}

/// A circle round each of some NPCs at the reach of an aura they pulse:
/// Entombed Sentinels' Marks, each Sentinel marking everyone within 40 yd.
/// Every unit of an NPC standing is ringed, as each pulses its own: ten
/// totems standing, ten rings.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Range {
    #[serde(default = "yes", skip_serializing_if = "is_true")]
    pub enabled: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// The NPCs it is drawn round, each unit of them standing.
    pub npc: Vec<String>,
    /// Each NPC's color, in the same order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tint: Vec<String>,
    /// Yards. Unsaid, the radius the game's tables give `spell`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub radius: Option<f32>,
    /// The spell that reaches this far (`[spell.<id>]`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub spell: Option<u32>,
    /// Two NPCs: also the line halfway between them, where one's reach
    /// gives way to the other's, and how far apart they stand (one unit of
    /// each, the one the replay places the NPC by).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub sides: bool,
    /// Difficulty names it appears on; empty, every one.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub only: Vec<String>,
}

/// A hostile spell that hits the same player again and again until another
/// takes it: Empowering Slam, Breath of Ula'tek's tank hit harder each time
/// until the tanks swap. The replay counts each run.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Streak {
    #[serde(default = "yes", skip_serializing_if = "is_true")]
    pub enabled: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    pub spell: Vec<u32>,
    /// The NPC that casts it; unsaid, any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub by: Option<String>,
    /// The most in a row a run should reach; past it, a fault.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub most: Option<u32>,
    /// Difficulty names it appears on; empty, every one.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub only: Vec<String>,
}

/// Two NPCs that must be kept apart: Ula'tek's Dominance, the Sentinels
/// taking almost no damage while within 25 yd of each other.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Apart {
    #[serde(default = "yes", skip_serializing_if = "is_true")]
    pub enabled: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// The two NPCs.
    pub npc: Vec<String>,
    /// Yards: closer than this is too close.
    pub within: f32,
    /// Phases they may stand close in, and the seconds after each one it
    /// takes to split them.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub allow: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub grace: Option<f32>,
    /// Difficulty names it appears on; empty, every one.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub only: Vec<String>,
}

/// A strike between where a unit stood and an NPC, each time a trigger
/// fires: Entombed Sentinels' Living Venom, the slime a soaked droplet
/// sends back to Breath of Ula'tek 4 s later, through whoever stands
/// between.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Line {
    #[serde(default = "yes", skip_serializing_if = "is_true")]
    pub enabled: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// The color the game draws it in ("green", "red" …).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tint: Option<String>,
    /// What starts one, where its unit stood (a trigger that names a unit,
    /// as a ground's `at`).
    pub from: Trigger,
    /// The NPC it runs to (its key), or `"@source"` (`Line::SOURCE`): the
    /// unit that put `from`'s aura on, the source the aura's row names as
    /// it fires. A row naming none (applied, dose or removed) takes the one
    /// the latest applied or dose row naming one named while the aura
    /// stayed on that unit, so two applications of it on one unit share
    /// it and the later applier wins. Only an aura's trigger has a source:
    /// an add's mark that comes off as the add that put it on lands its
    /// cast, three such adds standing at once.
    pub to: String,
    /// Seconds after `from` it strikes; until then it is drawn faint.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub delay: Option<f32>,
    /// The spells that hit along it as it strikes.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub hits: Vec<u32>,
    /// It is held rather than struck: drawn from the unit `from`'s aura
    /// went on, wherever that unit goes, to `to` for as long as the aura
    /// stays on it, gone as it comes off (or at the pull's end, where it
    /// never does). A fixation an add puts on a player (`to = "@source"`),
    /// who must face it. `check` refuses it beside a `from` that is no
    /// `aura_applied` (only an aura going on stays on), a `delay` or
    /// `hits` (it never strikes).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub hold: bool,
    /// Difficulty names it appears on; empty, every one.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub only: Vec<String>,
}

impl Line {
    /// What `to` says for the unit that put its trigger's aura on, in place
    /// of an NPC's key. A name starting with `@` is a keyword, and this is
    /// the one.
    pub const SOURCE: &'static str = "@source";

    /// Whether it runs to the unit that put its trigger's aura on (`to =
    /// "@source"`) rather than to an NPC.
    pub fn runs_to_source(&self) -> bool {
        self.to == Self::SOURCE
    }
}

/// Projectiles a trigger sends out from where its unit stood, each along a
/// fixed bearing and across the floor at a speed, hitting whoever stands in
/// their way: Plague Froth's Plague Waves, one to each of the four cardinal
/// points from every carrier as it runs out, the same way round however the
/// carrier faces.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Volley {
    #[serde(default = "yes", skip_serializing_if = "is_true")]
    pub enabled: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// The color the game draws them in ("green", "red" …).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tint: Option<String>,
    /// What sends one out, where its unit stood (a trigger that names a
    /// unit, as a ground's `at`); with `around`, any trigger, sent round
    /// that middle.
    pub from: Trigger,
    /// The middle it sets out round, in place of `from`'s unit: a place's
    /// spot (its key), or an NPC's unit as `from` fires. A ring of
    /// whirlpools closes on a bubble from far out; the cast that calls them
    /// is the boss's, wherever it stands. A place
    /// that is not there as `from` fires sends none. `check` refuses a
    /// unit's `aim` or a `side` round one where `from` names no unit.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub around: Option<String>,
    /// Their bearings, degrees: with the world's `aim`, 0 along the X the
    /// combat log writes, 90 along its Y; with the unit's, 0 the way it
    /// faced as `from` fired, 90 to its left.
    pub toward: Vec<f32>,
    /// What the bearings are turned with: the world (unsaid), or the
    /// unit's facing as `from` fires.
    #[serde(default, skip_serializing_if = "Aim::is_world")]
    pub aim: Aim,
    /// Degrees either side of its bearing each one may go: the log writes
    /// no projectile's path, only whom it hit, so each is turned to the
    /// hits it made within this of its bearing (those no volley without a
    /// spread explains, one projectile for each bearing they lie on), and
    /// keeps its bearing with none: Ula'tek's tails' nodules, found by the
    /// hits her waves leave. Unsaid, each keeps its bearing.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub spread: Option<f32>,
    /// Where they set out from: each spot yards ahead of the unit and to
    /// its left, turned with `aim` as the bearings are, one projectile for
    /// each spot and bearing. Unsaid, the unit's own spot. Ula'tek's wave
    /// is a row of six nodules out from her raised wing, a seventh ahead.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub spots: Vec<[f32; 2]>,
    /// Whose side of the unit the spots are on, their left mirrored to it:
    /// an NPC's key, the side it stood on as `from` fired; or `"raid"`, the
    /// side the living players' middle moved to over the seconds after
    /// (the raid steps toward Ula'tek's raised wing, which the log never
    /// writes: rows on that side explain the most hits for the fewest unhit
    /// players crossed).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub side: Option<String>,
    /// Yards a second each crosses.
    pub speed: f32,
    /// Yards across each one is: what it hits. A `front`'s depth, along
    /// its way.
    pub width: f32,
    /// Yards across a wave's front, square to its bearing: each one is a
    /// wall this long and `width` deep, not a disc (Ula'tek's Caustic Waves
    /// across her platform). Unsaid, a disc `width` across.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub front: Option<f32>,
    /// How each one looks: a disc (unsaid), or a crescent bowed the way it
    /// goes, bright on its leading edge and darker aft, as Caustic Waves'
    /// nodules are drawn in the game.
    #[serde(default, skip_serializing_if = "Form::is_disc")]
    pub form: Form,
    /// Yards from the spot each one's middle sets out at; unsaid, 0.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub start: Option<f32>,
    /// Yards each one travels before it is gone; unsaid, until it leaves
    /// the room (the map's stencil, else its render), or coming `inward`
    /// to its middle.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub length: Option<f32>,
    /// Each one comes in toward its middle rather than out from it: it
    /// sets out `start` yards out on its bearing and comes in `length`
    /// yards (unsaid, all the way in). Its way is the outward one turned
    /// round (`Volley::ways`), so everything read along a way reads it
    /// alike. A ring's slots close on its middle, so at a fixed `front`
    /// (or a disc's width) they overlap near it; slots that are arcs of
    /// the ring are not drawn yet. `check` refuses it beside a `spread`,
    /// which turns a way about its start, the rim.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub inward: bool,
    /// Seconds after `from` fires the first sets out; unsaid, at once.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub delay: Option<f32>,
    /// Seconds between one sending and the next, `times` in all, each from
    /// the spot and facing `from` gave: a channel's waves (Ula'tek's rows
    /// on Heroic, two 4.5 s apart).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub every: Option<f32>,
    /// How many times it is sent, `every` seconds apart; unsaid, once.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub times: Option<u32>,
    /// Each sending after the first sets out from the other side of its
    /// unit than the one before: its spots mirrored in turn, a wave from
    /// each of Ula'tek's wings.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub alternate: bool,
    /// Its projectiles (each spot and bearing) are slots each sending fills
    /// only some of, different every time, and the log writes none of
    /// them: a slot is a hole where a living player stood in its way unhit
    /// (Ula'tek's rows, their gaps changing every wave); with `faint`, one
    /// no one came near is unknown too, drawn faint, and one a hit lay along
    /// is seen. A hit never takes a projectile away.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub holes: bool,
    /// How `holes` are found: every slot a living player stood in the way
    /// of unhit is a hole (`"slot"`, unsaid), or each sending has one, the
    /// slots the largest cluster of those players spans round its middle
    /// (`"cluster"`: a ring of whirlpools that leaves the raid one gap, where a player crossed unhit elsewhere, under an immunity, made
    /// a second). `check` refuses `"cluster"` with no `holes`.
    #[serde(default, skip_serializing_if = "HolesBy::is_slot")]
    pub holes_by: HolesBy,
    /// With `holes`, a slot no one came near is drawn faint rather than
    /// whole: where the gaps are many and the raid tells few (Stage Three).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub faint: bool,
    /// The spells that hit along them.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub hits: Vec<u32>,
    /// NPCs they destroy as they cross them (Mythic's Malignant Tumors):
    /// one that dies while a volley is out is its kill.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub destroys: Vec<String>,
    /// Its kills are drawn where it cleared them: an NPC of its `destroys`
    /// that the log never places, dying while it is out, stood on one of
    /// its projectiles' fronts, `start` plus `speed` times the seconds
    /// since that sending set out (`guess::place`: of every front of every
    /// sending in flight, not past its reach, inside the room, the farthest
    /// from the NPC's spots still standing, posted or placed). The count,
    /// the moment and the distance are the log's; the spot is picked, so
    /// it is drawn as a guess. Unsaid, none is drawn; `check` refuses it
    /// with no `destroys`; a trash rubric's volley places none (a run's
    /// trash has no room to place them in).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub place_kills: bool,
    /// While the aura `from` waits to come off is on a player, the bearings
    /// their volley will go out on are drawn faint from them; its
    /// projectiles appear as it sets out. Beside any other `from`, with a
    /// `delay`, each projectile of its first sending is drawn still at its
    /// start from `from` until it sets out (`Volley::holds`), and a hit
    /// there then is its:
    /// whirlpools forming far out as the cast lands, a while before they
    /// close on the bubble.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub preview: bool,
    /// Difficulty names it appears on; empty, every one.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub only: Vec<String>,
}

impl Volley {
    /// Seconds after `from` fires each sending sets out: its `delay`, then
    /// `every` apart, `times` in all (once, unsaid).
    pub fn sendings(&self) -> Vec<f32> {
        let (delay, every) = (self.delay.unwrap_or(0.0), self.every.unwrap_or(0.0));
        (0..self.times.unwrap_or(1).max(1))
            .map(|n| delay + n as f32 * every)
            .collect()
    }

    /// Each sending's seconds after `from` fires, and whether its spots are
    /// on the unit's left: the first on `left`, each after it on the other
    /// side than the one before where it `alternate`s.
    pub fn sendings_sided(&self, left: bool) -> Vec<(f32, bool)> {
        self.sendings()
            .into_iter()
            .enumerate()
            .map(|(n, secs)| (secs, left ^ (self.alternate && n % 2 == 1)))
            .collect()
    }

    /// How far one's middle has gone along its way (`ways`) `secs` after
    /// it set out: `None` before it sets out and once it has gone its
    /// `length` (`reach`).
    pub fn out(&self, secs: f32) -> Option<f32> {
        let d = self.sets_out() + self.speed * secs;
        let past = self.reach().is_some_and(|r| d > r);
        (secs >= 0.0 && !past).then_some(d)
    }

    /// Yards along its way each one's middle sets out at: its `start`, or
    /// 0 for one coming `inward`, whose way begins `start` out.
    pub fn sets_out(&self) -> f32 {
        if self.inward {
            0.0
        } else {
            self.start.unwrap_or(0.0)
        }
    }

    /// Yards along its way each one's middle goes before it is gone: its
    /// `start` and its `length`, or coming `inward` its `length` (unsaid,
    /// its `start`: all the way in). None where it goes until it leaves
    /// the room.
    pub fn reach(&self) -> Option<f32> {
        if self.inward {
            self.length.or(self.start)
        } else {
            self.length.map(|l| self.start.unwrap_or(0.0) + l)
        }
    }

    /// Whether each one is drawn still at its start from `from` until it
    /// sets out: a `preview` with a `delay`, beside a `from` that is no
    /// aura coming off (which previews its bearings from the carrier).
    pub fn holds(&self) -> bool {
        self.preview
            && self.delay.is_some_and(|d| d > 0.0)
            && !matches!(self.from, Trigger::AuraRemoved { .. })
    }

    /// Each projectile's way, when `from`'s unit faced `facing` (radians,
    /// turned only with the unit's `aim`) and `side`'s NPC stood to its
    /// left (`left`) or right: for each spot (`origins`; its place in
    /// `spots`, none for the unit's own spot) and bearing (`steps_facing`,
    /// by its place in `toward`), where it sets out from (yards along the
    /// log's X and Y from the volley's middle: the unit's spot, or the
    /// `around`'s) and its unit step. Outward, the spot and the bearing;
    /// coming `inward`, `start` yards out on the bearing and the bearing
    /// turned round, so its middle comes in toward the spot as it goes
    /// (`sets_out` 0 along it). The one geometry for the replay and the
    /// editors.
    pub fn ways(&self, facing: f32, left: bool) -> Vec<Way> {
        let spot = |j: usize| (!self.spots.is_empty()).then_some(j);
        let start = self.start.unwrap_or(0.0);
        let steps = self.steps_facing(facing);
        let mut out = Vec::with_capacity(steps.len());
        for (j, [ox, oy]) in self.origins(facing, left).into_iter().enumerate() {
            for (i, &[sx, sy]) in steps.iter().enumerate() {
                let (from, step) = if self.inward {
                    ([ox + sx * start, oy + sy * start], [-sx, -sy])
                } else {
                    ([ox, oy], [sx, sy])
                };
                out.push(Way {
                    spot: spot(j),
                    bearing: i,
                    from,
                    step,
                });
            }
        }
        out
    }

    /// Each bearing as a unit step in the log's X and Y, its `aim` turned
    /// with the world.
    pub fn steps(&self) -> Vec<[f32; 2]> {
        self.steps_facing(0.0)
    }

    /// Each bearing as a unit step in the log's X and Y when `from`'s unit
    /// faced `facing` (radians, as the log writes it: 0 along its X, a
    /// quarter turn along its Y). The world's `aim` never turns.
    pub fn steps_facing(&self, facing: f32) -> Vec<[f32; 2]> {
        let turn = match self.aim {
            Aim::World => 0.0,
            Aim::Facing => facing,
        };
        self.toward
            .iter()
            .map(|deg| {
                let (s, c) = (deg.to_radians() + turn).sin_cos();
                [c, s]
            })
            .collect()
    }

    /// Where each sets out from, as yards along the log's X and Y from the
    /// unit's spot, when it faced `facing` (radians, turned only with the
    /// unit's `aim`) and `side`'s NPC stood to its left (`left`) or right:
    /// the `spots`, else the unit's spot alone.
    pub fn origins(&self, facing: f32, left: bool) -> Vec<[f32; 2]> {
        if self.spots.is_empty() {
            return vec![[0.0, 0.0]];
        }
        let turn = match self.aim {
            Aim::World => 0.0,
            Aim::Facing => facing,
        };
        let (s, c) = turn.sin_cos();
        let mirror = if left { 1.0 } else { -1.0 };
        self.spots
            .iter()
            .map(|&[ahead, beside]| {
                let beside = beside * mirror;
                [ahead * c - beside * s, ahead * s + beside * c]
            })
            .collect()
    }
}

/// One projectile's way of a volley (`Volley::ways`): which of its
/// `spots` it sets out from (none, the unit's own spot) and which of its
/// bearings it goes on (by their places in `spots` and `toward`), where
/// its way begins (yards along the log's X and Y from the volley's
/// middle) and its unit step along the log's X and Y.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Way {
    pub spot: Option<usize>,
    pub bearing: usize,
    pub from: [f32; 2],
    pub step: [f32; 2],
}

/// A spell whose hits are flashed on their victims: one that hits most of
/// the raid at once reads as one blow to the raid (Noxious Blast, a
/// droplet left unsoaked; the log never says where it lay).
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Flash {
    #[serde(default = "yes", skip_serializing_if = "is_true")]
    pub enabled: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    pub spell: Vec<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tint: Option<String>,
    /// Difficulty names it appears on; empty, every one.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub only: Vec<String>,
}

/// Units thrown straight out from a middle each time a trigger fires about
/// them, a knockback: a bubble bursting at an island's middle and
/// throwing everyone it hits out. The log writes the hit and posts the victims
/// through their flight, but never says a throw was one: a reader takes
/// each victim's throw off their track, from the firing until they stop
/// going out from the middle.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Knock {
    #[serde(default = "yes", skip_serializing_if = "is_true")]
    pub enabled: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// The color it is drawn in ("blue" …).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tint: Option<String>,
    /// What throws one: each firing throws the unit it is about (a hit's
    /// victim, an aura's carrier), so it names a unit
    /// (`Trigger::marks_a_spot`).
    pub on: Trigger,
    /// The middle they are thrown out from: a `[place]` (its `at`) or an
    /// NPC (where the log has its unit as `on` fires), by key.
    pub from: String,
    /// A `[place]` whose circle a throw must land in: one landing outside
    /// it is a fault (into the water round an island). Unsaid, none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub within: Option<String>,
    /// Difficulty names it appears on; empty, every one.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub only: Vec<String>,
}

/// Where a mark's count comes from, when the aura's stacks do not say it.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum Count {
    /// Its ticks: a hit of this spell deals `each` per application before
    /// mitigation, so the hit's unmitigated amount over `each` is the count.
    /// `each` unsaid, the smallest such hit the pull holds is one.
    Tick {
        spell: u32,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        each: Option<u32>,
    },
}

/// What a volley's bearings are turned with.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Aim {
    /// Fixed to the world, however its unit faces: Plague Froth's Plague
    /// Waves to the four cardinal points.
    #[default]
    World,
    /// Turned with its unit's facing as `from` fires: a wave sent the way
    /// its caster faces.
    Facing,
}

impl Aim {
    fn is_world(&self) -> bool {
        *self == Aim::World
    }
}

/// How a volley's projectiles look.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Form {
    /// A disc: Plague Froth's Plague Waves.
    #[default]
    Disc,
    /// A crescent bowed the way it goes: Caustic Waves' nodules.
    Crescent,
}

impl Form {
    fn is_disc(&self) -> bool {
        *self == Form::Disc
    }
}

/// How a volley's `holes` are found.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum HolesBy {
    /// Every slot a living player stood in the way of unhit is a hole:
    /// Ula'tek's rows, their gaps many and changing every wave.
    #[default]
    Slot,
    /// One hole a sending: the living players its slots crossed unhit,
    /// clustered by their bearings round its middle (a gap of more than a
    /// slot's spacing between two splits them), the slots the largest
    /// cluster's players stand nearest, first to last; one crossed unhit
    /// outside it makes none. For
    /// a radial volley, its bearings round one middle spaced about evenly:
    /// spots or parallel ways give bearings that say little.
    Cluster,
}

impl HolesBy {
    fn is_slot(&self) -> bool {
        *self == HolesBy::Slot
    }
}

/// When a ground goes.
#[derive(Debug, Clone, Default, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum Ends {
    /// With the pull: ground the raid has lost for good.
    #[default]
    Pull,
    /// As its trigger fires: a soak, gone as it is stepped on.
    At,
    /// This many seconds after it forms.
    After(f32),
    /// At the first hit of this spell on a player standing in it: a ground
    /// that bursts on contact. Written as a `hit` trigger is, `{ hit = {
    /// spell = N } }`, and with `lands` as one is, its landing hits alone
    /// (`crate::lands`): a dot's tick on someone who walks over it after
    /// is not a touch.
    Hit {
        spell: u32,
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        lands: bool,
    },
    /// At the first of these to come, each a `hit` or an `after`: a Frost
    /// Orb ends at its first touch, else shatters 40 s after it dropped,
    /// `{ first = [{ hit = { spell = 1313448, lands = true } }, { after =
    /// 40 }] }`. Its `after` is the earliest of its `after`s
    /// (`Ends::after`), its hits every `hit` (`Ends::hits`). A list is
    /// laid whole: a layer gives the whole list again. `check` refuses an
    /// empty one and one holding a `first`, `"at"` or `"pull"`.
    First(Vec<Ends>),
}

impl Ends {
    fn is_pull(&self) -> bool {
        *self == Ends::Pull
    }

    /// The seconds after it forms it is gone, where nothing comes first:
    /// an `after`'s, or the earliest of a `first`'s.
    pub fn after(&self) -> Option<f32> {
        match self {
            Ends::After(s) => Some(*s),
            Ends::First(of) => of
                .iter()
                .filter_map(Ends::after)
                .min_by(|a, b| a.total_cmp(b)),
            Ends::Pull | Ends::At | Ends::Hit { .. } => None,
        }
    }

    /// The hits that end it, each its spell and whether its landings alone
    /// do (`lands`): a `hit`'s, or every `hit` of a `first`'s, in order.
    pub fn hits(&self) -> Vec<(u32, bool)> {
        match self {
            Ends::Hit { spell, lands } => vec![(*spell, *lands)],
            Ends::First(of) => of.iter().flat_map(Ends::hits).collect(),
            Ends::Pull | Ends::At | Ends::After(_) => Vec::new(),
        }
    }
}

/// What makes a ground larger.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Grow {
    /// Its radius is the shape's times the stacks of the aura that dropped it
    /// (Blood Venom: 6 yd a stack, so a four-stack pool spreads to 24 yd).
    Stacks,
}

/// A significant moment: what a phase, a layer or a place can key on.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Event {
    #[serde(default = "yes", skip_serializing_if = "is_true")]
    pub enabled: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    pub on: Trigger,
    /// Difficulty names it appears on; empty, every one.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub only: Vec<String>,
}

fn yes() -> bool {
    true
}

fn is_true(b: &bool) -> bool {
    *b
}

/// A named entry a layer can switch off (`enabled = false`) and keep to
/// some difficulties (`only`; the map's layers and glows have none).
pub(crate) trait Gated {
    fn enabled(&self) -> bool;
    /// Difficulty names it appears on; empty, every one.
    fn only(&self) -> &[String] {
        &[]
    }
}

macro_rules! gated {
    ($($t:ty),* $(,)?) => {$(
        impl Gated for $t {
            fn enabled(&self) -> bool {
                self.enabled
            }
            fn only(&self) -> &[String] {
                &self.only
            }
        }
    )*};
}

gated!(
    Npc, Ability, Phase, Place, Ground, Mark, Line, Volley, Flash, Knock, Range, Apart, Streak,
    Room, Reach, Event,
);

impl Gated for Layer {
    fn enabled(&self) -> bool {
        self.enabled
    }
}

impl Gated for Glow {
    fn enabled(&self) -> bool {
        self.enabled
    }
}

impl Gated for Fade {
    fn enabled(&self) -> bool {
        self.enabled
    }
}

impl Gated for Area {
    fn enabled(&self) -> bool {
        self.enabled
    }
}

impl Gated for Path {
    fn enabled(&self) -> bool {
        self.enabled
    }
}

impl Gated for Pull {
    fn enabled(&self) -> bool {
        self.enabled
    }
}
