//! The rubric's typed form: one `Encounter` per boss, resolved for one
//! difficulty. Every struct refuses unknown keys, so a typo in a reviewed
//! file is an error that names its path, not a silent default.
//!
//! Named entries (`npc`, `ability`, `phase`, `place`, `ground`, `mark`,
//! `line`, `volley`, `flash`, `range`, `apart`, `streak`, `room`, `reach`,
//! `event`, `map.layer`, `map.glow`) are TOML tables keyed by a slug, so a
//! later layer (the tuned file, a difficulty) changes one by naming it, and
//! `enabled = false` drops it; all but the map's two can be kept to some
//! difficulties by their `only`. The game's `spell` facts are keyed the
//! same way, by spell id.

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
            range: BTreeMap::new(),
            apart: BTreeMap::new(),
            streak: BTreeMap::new(),
            room: BTreeMap::new(),
            reach: BTreeMap::new(),
            event: BTreeMap::new(),
            spell: BTreeMap::new(),
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ceiling: Option<f32>,
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
            (ps @ [_, _, _, ..], None, None) => Some(ps.iter().fold(
                [f32::MAX, f32::MAX, f32::MIN, f32::MIN],
                |b, p| {
                    [
                        b[0].min(p[0]),
                        b[1].min(p[1]),
                        b[2].max(p[0]),
                        b[3].max(p[1]),
                    ]
                },
            )),
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
            let (ex, ey) = (b[0] - a[0], b[1] - a[1]);
            let len2 = ex * ex + ey * ey;
            let t = if len2 > 0.0 {
                (((p[0] - a[0]) * ex + (p[1] - a[1]) * ey) / len2).clamp(0.0, 1.0)
            } else {
                0.0
            };
            nearest = nearest.min((p[0] - a[0] - t * ex).hypot(p[1] - a[1] - t * ey));
        }
        if inside { nearest } else { -nearest }
    }
}

impl Map {
    /// The stencil's world bounds, `[x0, y0, x1, y1]`: `None` without one.
    pub fn stencil_bounds(&self) -> Option<[f32; 4]> {
        self.stencil
            .iter()
            .filter_map(StencilShape::bounds)
            .reduce(|a, b| {
                [
                    a[0].min(b[0]),
                    a[1].min(b[1]),
                    a[2].max(b[2]),
                    a[3].max(b[3]),
                ]
            })
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
    /// When the event happens: one that happens again and again, at the time
    /// nearest to when the players stopped standing on it (README).
    Event(String),
}

/// The replay's default view of the room.
#[derive(Debug, Clone, Default, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct View {
    /// Degrees clockwise on screen; unsaid, the room's default from the data.
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

/// A creature in the fight.
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
    /// Difficulty names it appears on; empty, every one.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub only: Vec<String>,
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

/// Something the log shows that a phase, an event or a layer keys on.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
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
    /// A hostile spell hits.
    Hit { spell: u32 },
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
/// clocks differ). A pull that ended first, or where `since` fired fewer
/// times, never strikes it. Written as a bare number when it counts from
/// the pull, so a later layer giving a number replaces the whole clock,
/// and one giving `{ secs = … }` retimes a `since` clock and keeps the rest.
#[derive(Debug, Clone, PartialEq)]
pub struct Clock {
    pub secs: f32,
    /// What it counts from; none, the pull's start.
    pub since: Option<Box<Trigger>>,
    /// Which time `since` fired it counts from, the first being 1. It
    /// means nothing without a `since`.
    pub nth: u32,
}

impl Clock {
    /// `secs` into the pull.
    pub fn pull(secs: f32) -> Clock {
        Clock {
            secs,
            since: None,
            nth: 1,
        }
    }
}

/// A clock's table form, as read.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Since {
    secs: f32,
    since: Box<Trigger>,
    #[serde(default = "first")]
    nth: u32,
}

/// A clock's table form, as written.
#[derive(Serialize)]
struct SinceRef<'a> {
    secs: f32,
    since: &'a Trigger,
    #[serde(skip_serializing_if = "is_first")]
    nth: u32,
}

fn first() -> u32 {
    1
}

fn is_first(n: &u32) -> bool {
    *n == 1
}

impl Serialize for Clock {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        match &self.since {
            None => s.serialize_f32(self.secs),
            Some(since) => SinceRef {
                secs: self.secs,
                since,
                nth: self.nth,
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
                f.write_str("seconds into the pull, or a table of `secs`, `since` and `nth`")
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
    /// When it is there; unsaid, all fight.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub from: Option<Trigger>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub until: Option<Trigger>,
    /// Difficulty names it appears on; empty, every one.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub only: Vec<String>,
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
    /// The color the game draws it in, so the replay's matches: "green",
    /// "red", "purple", "blue", "yellow", "orange".
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
    #[serde(default, skip_serializing_if = "Ends::is_pull")]
    pub ends: Ends,
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
    /// It circles a place (or an NPC) once it forms, at the distance it
    /// formed at: Nek'zali's Latent Cultists round the Soulcoil Well. The
    /// log never places it again, so it is drawn as inferred.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub orbit: Option<Orbit>,
    /// Spells it deals to the players it touches: each hit places it at
    /// its victim again, so a ground that moves where the log cannot follow
    /// it is found anew whenever it hurts someone.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub seen: Vec<u32>,
    /// Difficulty names it appears on; empty, every one.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub only: Vec<String>,
}

impl Trigger {
    /// Whether it is about a unit, so a ground can land where it stood: an
    /// aura's target, a hit's victim, a cast's caster, an NPC that appears
    /// or dies. The pull's start, a clock, health, a phase or an event name
    /// no one.
    pub fn marks_a_spot(&self) -> bool {
        matches!(
            self,
            Trigger::Cast { .. }
                | Trigger::AuraApplied { .. }
                | Trigger::AuraRemoved { .. }
                | Trigger::Appears { .. }
                | Trigger::Death { .. }
                | Trigger::Hit { .. }
        )
    }
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
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Range {
    #[serde(default = "yes", skip_serializing_if = "is_true")]
    pub enabled: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// The NPCs it is drawn round.
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
    /// gives way to the other's, and how far apart they stand.
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
    /// The NPC it runs to.
    pub to: String,
    /// Seconds after `from` it strikes; until then it is drawn faint.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub delay: Option<f32>,
    /// The spells that hit along it as it strikes.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub hits: Vec<u32>,
    /// Difficulty names it appears on; empty, every one.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub only: Vec<String>,
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
    /// unit, as a ground's `at`).
    pub from: Trigger,
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
    /// the room (the map's stencil, else its render).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub length: Option<f32>,
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
    /// While the aura `from` waits to come off is on a player, the bearings
    /// their volley will go out on are drawn faint from them; its
    /// projectiles appear as it sets out.
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

    /// How far one's middle has gone from its spot `secs` after it set out:
    /// `None` before it sets out and once it has gone its `length`.
    pub fn out(&self, secs: f32) -> Option<f32> {
        let d = self.start.unwrap_or(0.0) + self.speed * secs;
        let past = self
            .length
            .is_some_and(|l| d > self.start.unwrap_or(0.0) + l);
        (secs >= 0.0 && !past).then_some(d)
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

/// When a ground goes.
#[derive(Debug, Clone, Default, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Ends {
    /// With the pull: ground the raid has lost for good.
    #[default]
    Pull,
    /// As its trigger fires: a soak, gone as it is stepped on.
    At,
    /// This many seconds after it forms.
    After(f32),
}

impl Ends {
    fn is_pull(&self) -> bool {
        *self == Ends::Pull
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
    Npc, Ability, Phase, Place, Ground, Mark, Line, Volley, Flash, Range, Apart, Streak, Room,
    Reach, Event,
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
