//! The seasonal encounter rubric: what the replay knows about each boss of
//! a season beyond what the log and the game's tables give for free — the
//! room's tweaks, the default view, the NPCs and their roles, the abilities
//! and their shapes, the phases and what begins them, the floor's fixed
//! features, what the fight leaves on it and takes back, and the events
//! that change the room.
//!
//! The rubric is TOML under the crate's `seasons/` (the crate's `README.md`
//! is the schema's reference), embedded at build time (`Rubric::embedded`),
//! or read whole from a tree shaped so on disk at runtime
//! (`Rubric::from_dir`, a checkout's, where the editors save):
//!
//! ```text
//! seasons/<season>/season.toml                    the season, its defaults, its rules
//! seasons/<season>/<instance>/instance.toml       the raid or dungeon
//! seasons/<season>/<instance>/<id>-<slug>.draft.toml   generated (gen-rubric)
//! seasons/<season>/<instance>/<id>-<slug>.toml         tuned by hand
//! seasons/<season>/<instance>/trash.toml               tuned by hand
//! ```
//!
//! An encounter resolves through an ordered stack of SOURCES (`Source`),
//! each laid over the last: the season's defaults, the instance's, the
//! generated draft, the encounter's map and NPCs as the instance's file
//! gives them (`[encounter.<id>]`: its `map` and `view`, what changes the
//! map: its `room`s, `place`s and the `event`s they key on, and an `npc`
//! the draft lacks or has wrong); then every curated file set in the order
//! it was laid (a tuned file embedded beside its draft, or a tree on disk's
//! in its place, `Rubric::with_curated_dir`; then each bundle
//! `Rubric::with_curated` lays in); then the user's own files
//! (`Rubric::with_user`, `with_user_dir`). Last come the difficulty
//! overrides the difficulty's fallback chain names, the most general
//! first (`difficulty`), each difficulty's from every source in the
//! stack's order. Tables merge key by key; anything else is replaced
//! whole. An entry with `enabled = false`, or whose `only` leaves the
//! difficulty out, is dropped. `Rubric::encounter_traced` answers who set
//! each key (`Provenance`).
//!
//! The sources belong to three TIERS, ordered (`Tier`):
//!
//! - BASE: what extraction gives, and the encounter's map and NPCs, with
//!   no other hand amendment (the season's and the instance's files, the
//!   draft with its own difficulty overrides, the instance's drawing). The
//!   map is the base's, however much it was worked on by hand (a stencil,
//!   a level, the view's turn, a platform that breaks on an event, a room
//!   off the arena), because a room should look and change the same at
//!   every tier; so is what an NPC is where the journal is silent or wrong
//!   (Ula'tek's Venomous Heart, sharing Ula'tek's health), because who is
//!   in the room should too. The base must stand alone: nothing in it
//!   leans on a curated or user file, and neither ever adds a draft.
//! - CURATED: what the fight does in the room, found by investigating it.
//!   The public repository carries the base alone; a tuned file beside its
//!   draft still reads as curated, so a tree that holds them works
//!   unchanged, a checkout's tree on disk is read in their place at
//!   runtime, and a curated set ships apart from the base as a bundle.
//! - USER: the files on this machine, laid last, so a user can override
//!   anything for themselves.
//!
//! A reader asks for a tier per encounter (`encounter_at`); the rubric's
//! own (`with_tier`, the user tier unless lowered) is the most it will
//! answer.
//!
//! A dungeon's LEVELS are the instance file's `[level.<ui_map>]`
//! (`Rubric::level`, `levels`), each read over the season's and the
//! instance's `[defaults]` (their `map`, `view` and `travel`) and never
//! over a difficulty or a curated or user file: a level is the base's
//! alone, where a set may still amend an arena's map. Its `map` is the
//! same `Map` an encounter's is, so a reader draws a raid's arena and a
//! dungeon's level with one type, and either may name the places the
//! camera knows (`area`), the ways between them (`path`) and what the
//! replay does while the log is quiet (`transition`). A level also gives
//! the usual route's pulls on it (`pull`): packs of the instance's
//! creatures in its areas, whose forces are their members', some of which
//! the route may skip (`optional`), so its forces read with them and
//! without (`Level::route_forces`).
//!
//! A dungeon's CREATURES are the instance file's own `[npc.<key>]`, with
//! the enemy forces each gives, and its whole `forces`
//! (`Instance::npcs`, `npc_of`, `forces`; `Rubric::instance_by_map` finds
//! the instance by the map id a run's log writes): who is in the dungeon
//! outside its encounters' own tables, the base's alone, as a level is.
//!
//! A dungeon's TRASH is what those creatures do outside its bosses: a
//! curated `trash.toml` per instance, shaped like an encounter's file and
//! resolved into an `Encounter` (`Rubric::trash`, by the map id; its
//! `encounter` 0, its name and instance the instance's) over the
//! instance's `[npc]`, at a keystone's difficulty. It gives no map, view,
//! travel, phase or room of its own (`TRASH_TABLES`), and `check_trash`
//! refuses a trigger counting from its start, which would be the key's.
//!
//! An entry's colors are the rubric's own names (`tint::NAMES`), which a
//! reader paints in its theme's hues and `check` holds every tint to.
//!
//! The journal's words for each ability are not in the repository: they
//! live in a per-machine sidecar (`text`) a reader lays over an encounter
//! with `Rubric::with_texts`.
//!
//! `floor` reads and writes a rendered floor's placement sidecar, the
//! `<slug>.txt` gen-floors writes beside each render in the floors cache,
//! and finds an encounter's render among them. `portraits` does the same
//! for the NPC portraits: the repository's crops file and the portraits
//! cache's index. `placed` is the generated table of what players place in
//! the room (totems, gateways, rings, zones) and how the log tells where
//! each stands: no encounter's, but read by the extractor's cut and the
//! replay alike. `turn` is the rule that proposes a map's `view.turn`:
//! the boss at the top as seen from the raid at the pull, squared with the
//! render's walls; the extractor's `suggest-turns` and the map editor offer
//! it, and no render reads it (an unsaid turn is north up). `swirl` draws a
//! place's swirling texture (`Swirl`) as a disc of pixels, the one look
//! for the replay and the map editor. `guess` picks where a volley's kill
//! stood that the log never placed (its `place_kills`): on one of its
//! projectiles' fronts as it died, the one inside the room farthest from
//! the NPC's spots still standing, the one rule for the replay and the
//! editors. `drift` reads the way the players drifted over a stretch, the
//! bearing a ground's push takes where it says `toward = "drift"` (a wind
//! the log never writes), the one rule for both again.

mod check;
pub mod difficulty;
pub mod drift;
pub mod floor;
pub mod follow;
pub mod guess;
pub mod lands;
mod layer;
pub mod placed;
pub mod portraits;
pub mod schema;
mod stack;
pub mod swirl;
pub mod text;
pub mod tint;
pub mod turn;
pub mod write;

use std::collections::BTreeMap;
use std::path::PathBuf;

pub use check::{check, check_instance, check_level, check_level_with, check_trash};
pub use layer::{Origin, Provenance, SetBy, Source};
use layer::{Set, lay};
use schema::Gated;
pub use schema::*;
use serde::Deserialize;
use serde::de::DeserializeOwned;
pub use stack::{TreeFiles, user_dir};
use toml::{Table, Value};
pub use turn::{
    ROOM_TURN_GUARD_YD, ROOM_TURN_WINDOW_MS, ROOM_WALL_SHARE, midpoint, plane, room_turn,
};

/// The rubric format this build reads and writes. A file in an older one is
/// migrated forward on load (`migrate`); a newer one is refused.
///
/// 2 (2026-10-17): an instance's creatures and their enemy forces
/// (`[npc]`, `forces`), an NPC's `forces` and `aloft`, and the trash file.
/// Keys added, none changed, so a file in 1 reads as it is; a build that
/// reads 1 refuses a file in 2 by its number rather than on a key it does
/// not know. A `trash.toml` it cannot place at all: it takes the name for
/// a misnamed encounter file, so it refuses a tree holding one whole.
/// Added to 2 later the same day, before any committed file gave one: a
/// level's `pull` tables (a pull's `optional` among them), an NPC's
/// `control`, a ground that `ends` on a hit and a ground's `push`. A
/// build of 2 without them refuses a file giving one on the key. Later
/// still, a line's `to = "@source"` (`Line::SOURCE`): a value, not a key,
/// so a build of 2 without it reads the file and its `check` refuses the
/// line, naming an NPC `"@source"` it does not have. Later still, a
/// ground's `clears` and a line's `hold`: keys, so a build of 2 without
/// them refuses a file giving one on the key. Later still (2026-10-27), a
/// ground's `falls` and a clock's `each`, keys, and two value forms of a
/// push: `on` as a list of triggers (still read as one alone, always
/// written as a list) and `toward` as the word `"drift"`; a `renew` beside
/// a `push`, which `check` refused before, is allowed. A build of 2
/// without them refuses a file giving a key on the key, and a list `on` or
/// a `"drift"` on its value's form; one written before reads as it did.
/// Later still (2026-10-29), a hit's `lands` (a `hit` trigger's and a
/// ground's `hit` ending's), a ground's `ends = { first = [...] }` and its
/// `sweeps`, keys: a build of 2 without them refuses a file giving one on
/// the key. Its `clears` may then name a ground that renews or has `seen`
/// hits, which `check` refused before. And a volley's `around` and
/// `inward`, keys, and its `preview` beside a `from` that is no aura coming
/// off where it has a `delay` (a value `check` refused before).
/// Later still (2026-10-29), a `[knock]` table and a reach's `absorbs`:
/// keys, so a build of 2 without them refuses a file giving one on the
/// key. Later still (2026-10-29), a ground's `follows`: a key, so a build
/// of 2 without it refuses a file giving one on the key. Later still
/// (2026-10-30), an area's `stencil`, a map's and an area's `hang`: keys,
/// so a build of 2 without them refuses a file giving one on the key.
pub const SCHEMA: u32 = 2;

/// What an instance file's `[encounter.<id>]` may hold: the encounter's map,
/// how it is first shown, what changes it in the fight (rooms off it, its
/// fixed places, and the events its layers, rooms and places key on), its
/// NPCs where the draft lacks one or has it wrong, and the speeds its paths
/// go at where the season's do not fit the arena (`travel`).
const INSTANCE_TABLES: [&str; 7] = ["map", "view", "room", "place", "event", "npc", "travel"];

/// What an instance file's `[level.<ui_map>]` may hold: its map, the view
/// while no area holds the party, its paths' speeds, and the usual route's
/// pulls on it.
const LEVEL_TABLES: [&str; 4] = ["map", "view", "travel", "pull"];

/// What a level reads of the season's and the instance's `[defaults]`:
/// its map, view and travel, never a pull, which is one level's alone.
const LEVEL_DEFAULTS: [&str; 3] = ["map", "view", "travel"];

/// What a `[defaults]` may give, a season's or an instance's, the base's
/// or a set's: an encounter's tables, which every encounter under it lays
/// in whole (its levels read the map, view and travel among them,
/// `LEVEL_DEFAULTS`), and their `[difficulty.<name>]` overrides. Anything
/// else would make every encounter under it unreadable, so it is refused
/// where its file is read (`defaults_keys`): a level's `pull`, one level's
/// alone, and an encounter's own words (its id, name, order), its draft's.
const DEFAULTS_TABLES: [&str; 20] = [
    "map", "view", "travel", "npc", "ability", "phase", "place", "ground", "mark", "line",
    "volley", "flash", "knock", "range", "apart", "streak", "room", "reach", "event", "spell",
];

/// What an instance's `trash.toml` may give, besides its `schema` and its
/// `[difficulty.<name>]` overrides of these: what its creatures are, do and
/// leave, and the game's facts of their spells. Its `encounter`, `name` and
/// `instance` are its instance's, laid in as it resolves; it has no `order`,
/// and the `map`, `view`, `travel`, `phase` and `room` of a run are its
/// levels' and its bosses'.
const TRASH_TABLES: [&str; 15] = [
    "npc", "ability", "place", "ground", "mark", "line", "volley", "flash", "knock", "range",
    "apart", "streak", "reach", "event", "spell",
];

/// The difficulty an instance's trash resolves at: a keystone (Mythic
/// Keystone, 8), through Mythic's, Heroic's and Normal's.
const KEYSTONE: u32 = 8;

mod embedded {
    include!(concat!(env!("OUT_DIR"), "/embedded.rs"));
}

/// Which of an encounter's sources it resolves with, the lesser first:
/// each tier reads its own sources and every lesser tier's.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub enum Tier {
    /// What extraction gives with no hand amendment but the map and its
    /// NPCs: the season's and the instance's files, the generated draft
    /// (`gen-rubric`) and the instance's drawing of the encounter.
    Base,
    /// The curated sets laid over the base: what was found by
    /// investigating the fight and checking it against logs and the game.
    Curated,
    /// The user's own files laid over everything: all this machine has.
    #[default]
    User,
}

impl Tier {
    /// Every tier, the lesser first.
    pub const ALL: [Tier; 3] = [Tier::Base, Tier::Curated, Tier::User];

    /// Its name, as a control or a command line spells it.
    pub fn name(self) -> &'static str {
        match self {
            Tier::Base => "base",
            Tier::Curated => "curated",
            Tier::User => "user",
        }
    }

    /// The tier a name spells (`name`).
    pub fn from_name(s: &str) -> Option<Tier> {
        Tier::ALL.into_iter().find(|t| t.name() == s)
    }
}

/// Every season the rubric holds, newest first.
#[derive(Debug, Clone)]
pub struct Rubric {
    seasons: Vec<Season>,
    /// Every encounter it answers, by id: its season's place in `seasons`
    /// and its instance's slug (`index`).
    index: BTreeMap<u32, (usize, String)>,
    /// Every dungeon level it answers, by UiMap id, placed as `index`
    /// places an encounter.
    levels: BTreeMap<u32, (usize, String)>,
    /// FNV-1a over every file the rubric holds, the base's and every set
    /// laid over it alike (`version`).
    hash: u64,
    version: String,
    /// Each encounter's journal text on this machine, by id (`with_texts`).
    texts: BTreeMap<u32, text::Texts>,
    /// The sidecars `with_texts` found and could not read.
    text_errors: Vec<String>,
    /// The file sets laid over the base, in the stack's order: the
    /// curated in the order laid (the embedded tuned files first, or a
    /// tree on disk's in their place), then the user's.
    sets: Vec<Set>,
    /// The user's files `with_user` and `with_user_dir` could not lay.
    user_errors: Vec<String>,
    /// The most any encounter resolves with (`with_tier`).
    tier: Tier,
    /// Where the base was read from (`base_origin`): the files handed in
    /// (`Embedded`), or a tree on disk (`Dir`, `from_dir`).
    base: Origin,
}

/// One season: its raids and dungeons. Its `season.toml` is read into it
/// as it is, less the format number `migrate` checked.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Season {
    /// The directory's name (`midnight-s2`).
    #[serde(skip)]
    pub id: String,
    pub name: String,
    /// Newer seasons have higher numbers.
    pub order: i64,
    /// The JournalInstance ids `gen-rubric` drafts the season from.
    #[serde(default, rename = "instances")]
    pub instances_from: Vec<u32>,
    #[serde(default)]
    defaults: Table,
    /// The game's rules for the season's keystones that its tables do
    /// not hold (`[rules]`); none said, none known.
    #[serde(default)]
    pub rules: SeasonRules,
    #[serde(skip)]
    pub instances: BTreeMap<String, Instance>,
}

/// A season's rules for its keystones, which the client's tables do not
/// hold (`[rules]` of its `season.toml`): what a player's death costs the
/// key's clock, by the key's level. The base's by construction: a curated
/// or user set's `season.toml` gives `[defaults]` and nothing else, so no
/// set can give or amend them. A reader finds them through a run's map:
/// `rubric.instance_by_map(map).map(|(s, _)| s.rules.penalty_at(level))`.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SeasonRules {
    /// `[[from_level, seconds], …]`: from each key level up, until the
    /// next pair's, the seconds each death adds to the key's clock
    /// (`death_penalty = [[2, 0], [7, 5], [12, 15]]`), in any order.
    #[serde(default)]
    pub death_penalty: Vec<(u32, f32)>,
}

impl SeasonRules {
    /// What is wrong with the rules as written, each said once: a
    /// from-level given twice, or seconds that are not a finite number at
    /// or above 0. `Rubric::from_files` refuses a season file whose rules
    /// say any, naming the file and the key.
    pub fn problems(&self) -> Vec<String> {
        let mut out = Vec::new();
        let mut seen: Vec<u32> = Vec::new();
        for &(from, secs) in &self.death_penalty {
            if seen.contains(&from) {
                out.push(format!("`rules.death_penalty`: +{from} given twice"));
            }
            seen.push(from);
            if !secs.is_finite() || secs < 0.0 {
                out.push(format!(
                    "`rules.death_penalty`: +{from} costs {secs} s, not a number of seconds at or above 0"
                ));
            }
        }
        out
    }

    /// The seconds a death costs a key of `level`: the pair with the
    /// highest from-level at or under it. `None` under the lowest, or
    /// where the season gives no table.
    pub fn penalty_at(&self, level: u32) -> Option<f32> {
        self.death_penalty
            .iter()
            .filter(|(from, _)| *from <= level)
            .max_by_key(|(from, _)| *from)
            .map(|&(_, secs)| secs)
    }
}

/// One raid or dungeon of a season. Its `instance.toml` is read into it as
/// it is, less the format number and each encounter's map and NPCs
/// (`[encounter.<id>]`, laid on the encounter's layers).
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Instance {
    /// The directory's name (`the-venomous-abyss`).
    #[serde(skip)]
    pub slug: String,
    pub name: String,
    /// "raid" or "dungeon".
    pub kind: String,
    /// Its JournalInstance id and its map id.
    #[serde(default)]
    pub journal: Option<u32>,
    #[serde(default)]
    pub map: Option<u32>,
    /// The dungeon's whole enemy forces: what a keystone's count runs to
    /// (`forces`).
    #[serde(default)]
    forces: Option<u32>,
    /// Who is in the dungeon outside its encounters' own tables, by key,
    /// as its file writes them, switched-off entries and all: its trash,
    /// with their enemy forces, the base's alone. What is laid under the
    /// instance's trash, so a trash file may switch one back on.
    #[serde(default, rename = "npc")]
    written: BTreeMap<String, Npc>,
    /// `written`'s enabled entries (`npcs`).
    #[serde(skip)]
    npc: BTreeMap<String, Npc>,
    /// Each creature id `npc` lists, to the first key (in key order)
    /// listing it (`npc_of`).
    #[serde(skip)]
    creatures: BTreeMap<u32, String>,
    #[serde(default)]
    defaults: Table,
    #[serde(skip)]
    encounters: BTreeMap<u32, Layers>,
    /// Each of a dungeon's levels as its file gives it, by UiMap id
    /// (`[level.<ui_map>]`): `{ map = …, view = …, travel = … }`.
    #[serde(skip)]
    levels: BTreeMap<u32, Table>,
}

impl Instance {
    /// The dungeon's whole enemy forces (its file's top-level `forces`):
    /// what a keystone's count runs to. `None` where it says none (a raid).
    pub fn forces(&self) -> Option<u32> {
        self.forces
    }

    /// The instance's own creatures (`[npc.<key>]` of its file), by key:
    /// who is in the dungeon outside its encounters' tables, with their
    /// enemy forces. Read as written, its switched-off entries left out;
    /// no difficulty applies here (an entry's `only` is read where the
    /// table is laid under the instance's trash).
    pub fn npcs(&self) -> &BTreeMap<String, Npc> {
        &self.npc
    }

    /// The instance's creature a creature id is (a GUID's sixth field),
    /// with its key: the first key, in key order, that lists it. `None`
    /// for a creature it does not list (a boss its encounter names).
    pub fn npc_of(&self, creature: u32) -> Option<(&str, &Npc)> {
        let key = self.creatures.get(&creature)?;
        self.npc.get_key_value(key).map(|(k, n)| (k.as_str(), n))
    }

    /// Its creatures as read: the switched-off ones left out of `npc`, and
    /// every creature id it lists indexed to the first key listing it.
    fn read_npcs(mut self) -> Instance {
        self.npc = self
            .written
            .iter()
            .filter(|(_, n)| n.enabled)
            .map(|(k, n)| (k.clone(), n.clone()))
            .collect();
        self.creatures.clear();
        for (key, n) in &self.npc {
            for &c in &n.creature {
                self.creatures.entry(c).or_insert_with(|| key.clone());
            }
        }
        self
    }
}

/// One encounter's base: its files' stem, its draft, and its map and NPCs
/// as its instance's file gives them. Its curated and user files are the
/// rubric's sets'.
#[derive(Debug, Clone, Default)]
struct Layers {
    stem: String,
    draft: Option<Table>,
    /// `[encounter.<id>]` of the instance's file: `{ map = …, view = …,
    /// room = …, place = …, event = …, npc = … }`.
    drawing: Option<Table>,
}

/// What a file under `seasons/` is, by where it lies.
enum FileRole<'a> {
    /// `<season>/season.toml`.
    Season { season: &'a str },
    /// `<season>/<instance>/instance.toml`.
    Instance { season: &'a str, instance: &'a str },
    /// `<season>/<instance>/<id>-<slug>.draft.toml`, or `.toml` tuned.
    Encounter {
        season: &'a str,
        instance: &'a str,
        id: u32,
        stem: &'a str,
        draft: bool,
    },
    /// `<season>/<instance>/trash.toml`: the instance's trash, tuned by
    /// hand.
    Trash { season: &'a str, instance: &'a str },
}

impl<'a> FileRole<'a> {
    /// A path's role, relative to `seasons/`: `None` for a file beside an
    /// encounter's that is no TOML.
    fn of(path: &'a str) -> Result<Option<FileRole<'a>>, String> {
        let parts: Vec<&'a str> = path.split('/').collect();
        match *parts.as_slice() {
            [season, "season.toml"] => Ok(Some(FileRole::Season { season })),
            [season, instance, "instance.toml"] => {
                Ok(Some(FileRole::Instance { season, instance }))
            }
            [season, instance, "trash.toml"] => Ok(Some(FileRole::Trash { season, instance })),
            [season, instance, file] => {
                let Some(name) = file.strip_suffix(".toml") else {
                    return Ok(None);
                };
                let (stem, draft) = match name.strip_suffix(".draft") {
                    Some(s) => (s, true),
                    None => (name, false),
                };
                let id = stem
                    .split('-')
                    .next()
                    .and_then(|n| n.parse().ok())
                    .ok_or("an encounter's file is named <encounter id>-<slug>")?;
                Ok(Some(FileRole::Encounter {
                    season,
                    instance,
                    id,
                    stem,
                    draft,
                }))
            }
            _ => Err("not where a rubric file goes".into()),
        }
    }
}

/// An instance file's `[encounter.<id>]`: the encounter's map as the
/// instance draws it, and its NPCs as the instance amends them, laid on
/// its layers once they are read.
struct Drawing<'a> {
    path: &'a str,
    season: &'a str,
    instance: &'a str,
    id: u32,
    map: Table,
}

impl<'a> Drawing<'a> {
    /// Each encounter's map and NPCs in an instance file's `encounter`
    /// table.
    fn read(
        path: &'a str,
        season: &'a str,
        instance: &'a str,
        encounter: Table,
        errs: &mut Vec<String>,
    ) -> Vec<Drawing<'a>> {
        let mut out = Vec::new();
        for (k, v) in encounter {
            let Ok(id) = k.parse::<u32>() else {
                errs.push(format!(
                    "{path}: [encounter.{k}]: an encounter is named by its DungeonEncounterID"
                ));
                continue;
            };
            match v {
                Value::Table(map)
                    if map.keys().all(|k| INSTANCE_TABLES.contains(&k.as_str())) =>
                {
                    out.push(Drawing {
                        path,
                        season,
                        instance,
                        id,
                        map,
                    })
                }
                _ => errs.push(format!(
                    "{path}: [encounter.{id}]: an instance gives only an encounter's map and NPCs (`{}`)",
                    INSTANCE_TABLES.join("`, `")
                )),
            }
        }
        out
    }
}

/// Whether a trash file's keys are what one may give (`TRASH_TABLES`, at
/// its top and in each `[difficulty.<name>]`), its creatures amended but
/// never their identity or worth: no `forces` for any creature, and no
/// `creature` ids for one of `base`'s (the instance's, as written), though
/// a creature the file adds gives its own ids. An error names the key.
fn trash_keys(t: &Table, base: &BTreeMap<String, Npc>) -> Result<(), String> {
    let refuse = |k: &str| {
        Err(format!(
            "`{k}`: a trash file gives only `{}`, and its `[difficulty.<name>]` overrides of them",
            TRASH_TABLES.join("`, `")
        ))
    };
    // A body's creatures: what each is stays the instance's.
    let creatures = |at: &str, body: &Table| -> Result<(), String> {
        let Some(Value::Table(npc)) = body.get("npc") else {
            return Ok(());
        };
        for (key, n) in npc {
            let Value::Table(n) = n else { continue };
            if n.contains_key("forces") {
                return Err(format!(
                    "`{at}npc.{key}.forces`: what a creature gives toward a keystone's forces is the instance's (its [npc]); a trash file gives none, and a creature it adds gives none"
                ));
            }
            if n.contains_key("creature") && base.contains_key(key) {
                return Err(format!(
                    "`{at}npc.{key}.creature`: which creature ids are the instance's {key} is the instance's (its [npc]); a trash file gives ids only to a creature it adds"
                ));
            }
        }
        Ok(())
    };
    for (k, v) in t {
        match (k.as_str(), v) {
            ("schema", _) => {}
            ("difficulty", Value::Table(d)) => {
                for (name, o) in d {
                    let Value::Table(o) = o else { continue };
                    if let Some(k) = o.keys().find(|k| !TRASH_TABLES.contains(&k.as_str())) {
                        return refuse(&format!("difficulty.{name}.{k}"));
                    }
                    creatures(&format!("difficulty.{name}."), o)?;
                }
            }
            (k, _) if TRASH_TABLES.contains(&k) || k == "difficulty" => {}
            (k, _) => return refuse(k),
        }
    }
    creatures("", t)
}

/// A file's text as its table, brought to `SCHEMA` (`migrate`); an error
/// names the file.
fn parse(path: &str, text: &str, errs: &mut Vec<String>) -> Option<Table> {
    let t = text
        .parse::<Table>()
        .map_err(|e| e.to_string())
        .and_then(migrate);
    t.map_err(|e| errs.push(format!("{path}: {e}"))).ok()
}

/// A season's or an instance's table as its type, its format number
/// (checked by `migrate`) left out.
fn typed<T: DeserializeOwned>(mut t: Table) -> Result<T, String> {
    t.remove("schema");
    T::deserialize(Value::Table(t)).map_err(|e| e.to_string())
}

/// Fold `(path, text)` pairs into an FNV-1a hash, in the order given.
fn hash_files<'a>(mut hash: u64, files: impl IntoIterator<Item = (&'a str, &'a str)>) -> u64 {
    for (path, text) in files {
        for b in path.bytes().chain(text.bytes()) {
            hash ^= u64::from(b);
            hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
        }
    }
    hash
}

/// The files under the crate's `seasons/` this binary was built with, each
/// `(path, text)`, the path relative to `seasons/` as `Rubric::from_files`
/// takes it, in path order: what `Rubric::embedded` reads, and what a tool
/// that edits the files (the map editor) reads where no checkout of them
/// is at hand.
pub fn embedded_files() -> &'static [(&'static str, &'static str)] {
    embedded::FILES
}

impl Rubric {
    /// The rubric this binary was built with.
    pub fn embedded() -> Result<Rubric, Vec<String>> {
        Rubric::from_files(
            embedded_files()
                .iter()
                .map(|&(p, t)| (p.to_string(), t.to_string())),
        )
    }

    /// The rubric from `(path, text)` pairs, paths relative to `seasons/`.
    /// A tuned `<id>-<slug>.toml` among them is the curated tier's first
    /// set (`Source::Curated(Origin::Embedded)`); the rest is the base.
    pub fn from_files(
        files: impl IntoIterator<Item = (String, String)>,
    ) -> Result<Rubric, Vec<String>> {
        Rubric::read(files, &|p: &str| p.to_string(), Origin::Embedded)
    }

    /// The rubric from a `seasons/` tree on disk (a checkout's
    /// `crates/encounter-rubric/seasons/`, where the editors save): every
    /// `.toml` under `dir`, read now and laid as `from_files` lays the
    /// files a build embeds, its base (the seasons, the instances with
    /// their levels and creatures, the drafts and the drawings) and its
    /// tuned and trash files as the curated tier's first set, named for
    /// the tree (`Source::Curated(Origin::Dir(dir))`, which
    /// `with_curated_dir` replaces as it does the embedded set). So a map,
    /// a level or a creature saved into the tree is read with no rebuild.
    /// `base_origin` says the base came from `dir`.
    ///
    /// A stray entry is passed over (a dot-file, a link to nothing, a
    /// non-UTF-8 name but a `.toml`'s: `TreeFiles::read`). As `from_files`
    /// refuses a build's tree, any error refuses the
    /// whole: a file that cannot be read, parsed or placed, an encounter
    /// or a level two instances give, a trash file no reader looks at. Each
    /// names its file by its place on disk (a clash, by the files' places
    /// under `dir`). So is a `dir` that is no directory, or holds no
    /// `<season>/season.toml`. And as `with_curated_dir` refuses a set
    /// that leaves an encounter unreadable, the tree is refused where any
    /// encounter or instance's trash does not read on some difficulty at
    /// the base or the curated tier, or a level does not read: the build's
    /// tree is held so by its test, a tree on disk is checked here, each
    /// subject named under `dir` with where its files lie under it. So a
    /// tree is held to more than the build's reader holds a build's files
    /// to (`from_files` never reads them so): a draft saved mid-work that
    /// fails on one difficulty refuses the whole tree until it is fixed,
    /// and a writer should save each file whole (written beside, then
    /// moved over it), or a reader may read it half written.
    ///
    /// The version folds in the tree's path after every file: a rubric
    /// read from a tree never shares its version with one built from the
    /// same files (its curated set's provenance names the tree), as
    /// `with_curated_dir`'s does not.
    pub fn from_dir(dir: &std::path::Path) -> Result<Rubric, Vec<String>> {
        Rubric::from_tree(TreeFiles::read(dir))
    }

    /// The rubric from a tree's files a walk read (`TreeFiles::read`), as
    /// `from_dir` reads it: so a reader that compared the walk's
    /// fingerprint with the one it last read parses only a tree that
    /// changed.
    pub fn from_tree(tree: TreeFiles) -> Result<Rubric, Vec<String>> {
        let TreeFiles {
            root,
            present,
            files,
            errors,
        } = tree;
        let dir = root.as_path();
        if !present {
            return Err(vec![format!("{}: not a directory", dir.display())]);
        }
        if !errors.is_empty() {
            return Err(errors);
        }
        let seasons = files
            .iter()
            .any(|(p, _)| matches!(FileRole::of(p), Ok(Some(FileRole::Season { .. }))));
        if !seasons {
            return Err(vec![format!(
                "{}: no season: a seasons tree holds a <season>/season.toml",
                dir.display()
            )]);
        }
        let name = dir.display().to_string();
        let shown = |p: &str| dir.join(p).display().to_string();
        let mut r = Rubric::read(files, &shown, Origin::Dir(name.clone()))?;
        let errs = r.unreadable(&name);
        if !errs.is_empty() {
            return Err(errs);
        }
        r.hash = hash_files(r.hash, [("\0dir\0", name.as_str())]);
        r.version = r.versioned();
        Ok(r)
    }

    /// Where the base was read from: `Origin::Embedded` for the files a
    /// build embeds or `from_files` was handed, `Origin::Dir` for a tree on
    /// disk (`from_dir`), by its path. Never a bundle: a bundle is laid
    /// over a base, never read as one.
    pub fn base_origin(&self) -> &Origin {
        &self.base
    }

    /// The rubric from `(path, text)` pairs, paths relative to `seasons/`:
    /// `from_files`, and `from_dir` over a tree's files. `shown` names a
    /// file in an error; `origin` is where the files came from, the base's
    /// (`base_origin`) and the tuned and trash files' set's alike.
    fn read(
        files: impl IntoIterator<Item = (String, String)>,
        shown: &dyn Fn(&str) -> String,
        origin: Origin,
    ) -> Result<Rubric, Vec<String>> {
        let mut files: Vec<(String, String)> = files.into_iter().collect();
        files.sort();
        let hash = hash_files(
            0xcbf2_9ce4_8422_2325,
            files.iter().map(|(p, t)| (p.as_str(), t.as_str())),
        );
        let mut errs = Vec::new();
        // Each file by its role, named as `shown` names it: from here on a
        // file's path is what an error calls it.
        let names: Vec<String> = files.iter().map(|(p, _)| shown(p)).collect();
        let mut roles = Vec::new();
        for ((path, text), name) in files.iter().zip(&names) {
            match FileRole::of(path) {
                Ok(Some(role)) => roles.push((name.as_str(), text.as_str(), role)),
                Ok(None) => {}
                Err(e) => errs.push(format!("{name}: {e}")),
            }
        }
        // Seasons and instances first, so encounters find their parents.
        let mut seasons: BTreeMap<String, Season> = BTreeMap::new();
        for &(path, text, ref role) in &roles {
            let FileRole::Season { season } = *role else {
                continue;
            };
            let Some(t) = parse(path, text, &mut errs) else {
                continue;
            };
            match typed::<Season>(t) {
                Ok(s) => {
                    if let Err(e) = defaults_keys(&s.defaults) {
                        errs.push(format!("{path}: {e}"));
                    }
                    let problems = s.rules.problems();
                    if !problems.is_empty() {
                        errs.extend(problems.into_iter().map(|p| format!("{path}: {p}")));
                        continue;
                    }
                    let id = season.to_string();
                    seasons.insert(id.clone(), Season { id, ..s });
                }
                Err(e) => errs.push(format!("{path}: {e}")),
            }
        }
        let mut drawings = Vec::new();
        for &(path, text, ref role) in &roles {
            let FileRole::Instance { season, instance } = *role else {
                continue;
            };
            let Some(mut t) = parse(path, text, &mut errs) else {
                continue;
            };
            let Some(s) = seasons.get_mut(season) else {
                errs.push(format!("{path}: no {season}/season.toml"));
                continue;
            };
            // Each encounter's map, by its DungeonEncounterID: how its room
            // is drawn and first shown (`[encounter.<id>.map]` and `.view`)
            // and what changes it (`.room`, `.place`, `.event`), and its NPCs
            // where the draft lacks one or has it wrong (`.npc`), part of the
            // base.
            let encounter = match t.remove("encounter") {
                Some(Value::Table(e)) => e,
                None => Table::new(),
                Some(_) => {
                    errs.push(format!(
                        "{path}: `encounter` is a table of encounters' maps and NPCs, by DungeonEncounterID"
                    ));
                    continue;
                }
            };
            // A dungeon's levels, by UiMap id: each one's map, its view and
            // its paths' speeds, the base's alone.
            let level = match t.remove("level") {
                Some(Value::Table(l)) => l,
                None => Table::new(),
                Some(_) => {
                    errs.push(format!(
                        "{path}: `level` is a table of a dungeon's levels, by UiMap id"
                    ));
                    continue;
                }
            };
            match typed::<Instance>(t) {
                Ok(i) => {
                    if let Err(e) = defaults_keys(&i.defaults) {
                        errs.push(format!("{path}: {e}"));
                    }
                    drawings.extend(Drawing::read(path, season, instance, encounter, &mut errs));
                    let levels = read_levels(path, level, &mut errs);
                    let slug = instance.to_string();
                    s.instances
                        .insert(slug.clone(), Instance { slug, levels, ..i }.read_npcs());
                }
                Err(e) => errs.push(format!("{path}: {e}")),
            }
        }
        // The tuned files beside their drafts, and each instance's trash
        // file: the curated tier's first set.
        let mut tuned = Set::new(Source::Curated(origin.clone()));
        for &(path, text, ref role) in &roles {
            let (season, instance, id, stem, draft) = match *role {
                FileRole::Encounter {
                    season,
                    instance,
                    id,
                    stem,
                    draft,
                } => (season, instance, id, stem, draft),
                FileRole::Trash { season, instance } => {
                    let Some(t) = parse(path, text, &mut errs) else {
                        continue;
                    };
                    match seasons.get(season).and_then(|s| s.instances.get(instance)) {
                        None => errs.push(format!("{path}: no {season}/{instance}/instance.toml")),
                        Some(inst) => match trash_keys(&t, &inst.written) {
                            Err(e) => errs.push(format!("{path}: {e}")),
                            Ok(()) => tuned.instance_mut(season, instance).trash = Some(t),
                        },
                    }
                    continue;
                }
                FileRole::Season { .. } | FileRole::Instance { .. } => continue,
            };
            let Some(t) = parse(path, text, &mut errs) else {
                continue;
            };
            let Some(inst) = seasons
                .get_mut(season)
                .and_then(|s| s.instances.get_mut(instance))
            else {
                errs.push(format!("{path}: no {season}/{instance}/instance.toml"));
                continue;
            };
            let layers = inst.encounters.entry(id).or_default();
            if !layers.stem.is_empty() && layers.stem != stem {
                errs.push(format!(
                    "{path}: encounter {id} already has files named {}",
                    layers.stem
                ));
                continue;
            }
            layers.stem = stem.to_string();
            if draft {
                layers.draft = Some(t);
            } else {
                tuned
                    .instance_mut(season, instance)
                    .encounters
                    .insert(id, t);
            }
        }
        for d in drawings {
            match seasons
                .get_mut(d.season)
                .and_then(|s| s.instances.get_mut(d.instance))
                .and_then(|i| i.encounters.get_mut(&d.id))
            {
                Some(l) => l.drawing = Some(d.map),
                None => errs.push(format!(
                    "{}: [encounter.{id}]: the instance has no files for encounter {id}",
                    d.path,
                    id = d.id
                )),
            }
        }
        if !errs.is_empty() {
            return Err(errs);
        }
        let mut seasons: Vec<Season> = seasons.into_values().collect();
        seasons.sort_by_key(|s| std::cmp::Reverse(s.order));
        let (index, levels) = match (index(&seasons), level_index(&seasons)) {
            (Ok(i), Ok(l)) => (i, l),
            (i, l) => {
                return Err(i.err().into_iter().chain(l.err()).flatten().collect());
            }
        };
        let mut r = Rubric {
            seasons,
            index,
            levels,
            hash,
            version: String::new(),
            texts: BTreeMap::new(),
            text_errors: Vec::new(),
            sets: vec![tuned],
            user_errors: Vec::new(),
            tier: Tier::default(),
            base: origin,
        };
        r.version = r.versioned();
        // A trash file where no reader looks (an older season's copy of a
        // returning dungeon) would be laid and never read.
        let mut errs = Vec::new();
        for set in &r.sets {
            for (season, s) in &set.seasons {
                for (instance, i) in &s.instances {
                    if i.trash.is_some()
                        && let Some(e) = r.trash_elsewhere(season, instance)
                    {
                        errs.push(format!(
                            "{}: {e}",
                            shown(&format!("{season}/{instance}/trash.toml"))
                        ));
                    }
                }
            }
        }
        if !errs.is_empty() {
            return Err(errs);
        }
        Ok(r)
    }

    /// Every season, newest first.
    pub fn seasons(&self) -> &[Season] {
        &self.seasons
    }

    /// The raid or dungeon whose map id (the instance file's `map`, the id
    /// the log's ZONE_CHANGE and CHALLENGE_MODE_START write) is `map`, with
    /// its season: the newest season's, as `encounter` answers a returning
    /// dungeon's encounters from it; within a season, the first by slug.
    /// `None` when no season has it.
    pub fn instance_by_map(&self, map: u32) -> Option<(&Season, &Instance)> {
        self.seasons.iter().find_map(|s| {
            s.instances
                .values()
                .find(|i| i.map == Some(map))
                .map(|i| (s, i))
        })
    }

    /// What an interpretation records it ran under: the seasons and a hash
    /// of every file, the base's and every curated and user file laid over
    /// it. A reader that records it should record the tier it read at too.
    pub fn version(&self) -> &str {
        &self.version
    }

    fn versioned(&self) -> String {
        format!("{}+{:016x}", seasons_ids(&self.seasons), self.hash)
    }

    /// The rubric with the journal's text laid over every encounter it
    /// answers, from the per-machine sidecars under `root` (`text::dir()`):
    /// `<root>/<season>/<instance>/<id>-<slug>.toml`, each read once, here.
    /// An encounter with no sidecar, or one that cannot be read
    /// (`text_errors` names it), keeps its abilities' text unsaid.
    pub fn with_texts(mut self, root: impl Into<PathBuf>) -> Rubric {
        let root = root.into();
        let files: Vec<(u32, PathBuf)> = self
            .index
            .keys()
            .filter_map(|&id| {
                let (s, i, l) = self.find(id)?;
                Some((
                    id,
                    root.join(&s.id)
                        .join(&i.slug)
                        .join(format!("{}.toml", l.stem)),
                ))
            })
            .collect();
        self.texts.clear();
        self.text_errors.clear();
        for (id, file) in files {
            match std::fs::read_to_string(&file) {
                Ok(t) => match text::read(&t) {
                    Ok(texts) => {
                        self.texts.insert(id, texts);
                    }
                    Err(e) => self.text_errors.push(format!("{}: {e}", file.display())),
                },
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => self.text_errors.push(format!("{}: {e}", file.display())),
            }
        }
        self
    }

    /// The sidecars `with_texts` found but could not read, each naming its
    /// file; their encounters' abilities keep their text unsaid.
    pub fn text_errors(&self) -> &[String] {
        &self.text_errors
    }

    /// The rubric answering no more than `tier`: an encounter asked for at
    /// a higher one resolves at this one.
    pub fn with_tier(mut self, tier: Tier) -> Rubric {
        self.tier = tier;
        self
    }

    /// The most any encounter resolves with (`with_tier`; the user tier,
    /// everything this machine has, unless lowered).
    pub fn tier(&self) -> Tier {
        self.tier
    }

    /// An encounter (its DungeonEncounterID) as the newest season that has
    /// it describes it, at the rubric's own tier; for `difficulty` (the
    /// log's difficulty id) its overrides too, else the encounter's own
    /// alone. `None` when no season has it.
    pub fn encounter(&self, id: u32, difficulty: Option<u32>) -> Option<Result<Encounter, String>> {
        self.encounter_at(id, difficulty, self.tier)
    }

    /// An encounter as `encounter` gives it, at `tier` or the rubric's own,
    /// whichever is less. `None` too when no source at that tier holds the
    /// encounter's own file: the base answers an encounter with a draft, the
    /// curated tier one with a draft or a curated file, the user tier one
    /// with any of them or a user file.
    pub fn encounter_at(
        &self,
        id: u32,
        difficulty: Option<u32>,
        tier: Tier,
    ) -> Option<Result<Encounter, String>> {
        self.answer(id, difficulty, tier.min(self.tier), None)
    }

    /// An encounter as `encounter_at` gives it, and who set each of its
    /// keys: the same `Encounter`, with its `Provenance`.
    pub fn encounter_traced(
        &self,
        id: u32,
        difficulty: Option<u32>,
        tier: Tier,
    ) -> Option<Result<(Encounter, Provenance), String>> {
        let mut prov = Provenance::default();
        let e = self.answer(id, difficulty, tier.min(self.tier), Some(&mut prov))?;
        Some(e.map(|e| (e, prov)))
    }

    /// The sources with a file this encounter reads, in the stack's order:
    /// the base's (its season's and instance's files always, its draft and
    /// its drawing where it has them), then each curated set and the
    /// user's that holds its file, its drawing, or its instance's or
    /// season's. A set laid twice in a row is listed once. Empty when no
    /// season has the encounter.
    pub fn layers_of(&self, id: u32) -> Vec<Source> {
        let Some((season, inst, layers)) = self.find(id) else {
            return Vec::new();
        };
        let mut out = vec![Source::Season, Source::Instance];
        if layers.draft.is_some() {
            out.push(Source::Draft);
        }
        if layers.drawing.is_some() {
            out.push(Source::Drawing);
        }
        for set in &self.sets {
            if set.touches(&season.id, &inst.slug, id) && out.last() != Some(&set.source) {
                out.push(set.source.clone());
            }
        }
        out
    }

    /// Whether an encounter has a curated layer over its base: any curated
    /// set with a file it reads (`layers_of`).
    pub fn has_curated(&self, id: u32) -> bool {
        self.layers_of(id)
            .iter()
            .any(|s| matches!(s, Source::Curated(_)))
    }

    /// Whether the user's files touch an encounter (`layers_of`).
    pub fn has_user(&self, id: u32) -> bool {
        self.layers_of(id).contains(&Source::User)
    }

    /// Every encounter id the rubric answers, with the season and instance
    /// it answers it from: the newest season's, seasons newest first, each
    /// id once. An id whose only file is curated or the user's is listed
    /// too; `encounter_at` answers it from that tier up.
    pub fn encounters(&self) -> Vec<(u32, &Season, &Instance)> {
        let mut out = Vec::new();
        for (n, s) in self.seasons.iter().enumerate() {
            for (slug, i) in &s.instances {
                for &id in i.encounters.keys() {
                    if self
                        .index
                        .get(&id)
                        .is_some_and(|(m, at)| *m == n && at == slug)
                    {
                        out.push((id, s, i));
                    }
                }
            }
        }
        out
    }

    /// A dungeon level, by its UiMap id, as the newest season that has it
    /// gives it: the season's `[defaults]`, the instance's, then the
    /// level's own `[level.<ui_map>]`, each read for its `map`, `view` and
    /// `travel` (a `pull` is the level's own, never a default's), with the
    /// instance's creatures laid in (`Level::npc`). No difficulty applies
    /// to a level, and it is the base's alone: a curated or user set never
    /// reaches it. `None` when no season has it.
    pub fn level(&self, ui_map: u32) -> Option<Result<Level, String>> {
        let (n, slug) = self.levels.get(&ui_map)?;
        let season = self.seasons.get(*n)?;
        let inst = season.instances.get(slug)?;
        let own = inst.levels.get(&ui_map)?;
        Some(resolve_level(season, inst, ui_map, own))
    }

    /// Every level the rubric answers, by UiMap id, with the season and
    /// instance it answers it from: the newest season's, seasons newest
    /// first, each id once.
    pub fn levels(&self) -> Vec<(u32, &Season, &Instance)> {
        let mut out: Vec<(usize, u32, &Season, &Instance)> = self
            .levels
            .iter()
            .filter_map(|(&id, (n, slug))| {
                let s = self.seasons.get(*n)?;
                Some((*n, id, s, s.instances.get(slug)?))
            })
            .collect();
        out.sort_by_key(|&(n, id, _, i)| (n, i.slug.as_str(), id));
        out.into_iter().map(|(_, id, s, i)| (id, s, i)).collect()
    }

    /// An instance's trash, by the map id a run's log writes: what its
    /// creatures do outside its encounters, as an `Encounter` (its
    /// `encounter` 0, its `name` and `instance` the instance's) at the
    /// rubric's own tier. The instance's own creatures (`[npc]`) are its
    /// base, and each curated or user set's `trash.toml` is laid over them
    /// in the stack's order; it resolves at a keystone's difficulty (8),
    /// through Mythic's, Heroic's and Normal's, an `only` as exact as an
    /// encounter's. Every instance has one, its base alone where no set
    /// gives a file. `None` when no season has the map (`instance_by_map`).
    pub fn trash(&self, map: u32) -> Option<Result<Encounter, String>> {
        self.trash_at(map, self.tier)
    }

    /// An instance's trash as `trash` gives it, at `tier` or the rubric's
    /// own, whichever is less.
    pub fn trash_at(&self, map: u32, tier: Tier) -> Option<Result<Encounter, String>> {
        let (season, inst) = self.instance_by_map(map)?;
        Some(self.resolve_trash(season, inst, Some(KEYSTONE), tier.min(self.tier), None))
    }

    /// An instance's trash as `trash_at` gives it, and who set each of its
    /// keys: the same `Encounter`, with its `Provenance`.
    pub fn trash_traced(
        &self,
        map: u32,
        tier: Tier,
    ) -> Option<Result<(Encounter, Provenance), String>> {
        let (season, inst) = self.instance_by_map(map)?;
        let mut prov = Provenance::default();
        let e = self.resolve_trash(
            season,
            inst,
            Some(KEYSTONE),
            tier.min(self.tier),
            Some(&mut prov),
        );
        Some(e.map(|e| (e, prov)))
    }

    /// The sources with a file an instance's trash reads, by the map id,
    /// in the stack's order: the instance's own file (its creatures)
    /// always, then each curated set and the user's that holds its
    /// `trash.toml`. A set laid twice in a row is listed once. Empty when
    /// no season has the map. What `layers_of` is to an encounter.
    pub fn trash_layers_of(&self, map: u32) -> Vec<Source> {
        let Some((season, inst)) = self.instance_by_map(map) else {
            return Vec::new();
        };
        let mut out = vec![Source::Instance];
        for set in &self.sets {
            if set.has_trash(&season.id, &inst.slug) && out.last() != Some(&set.source) {
                out.push(set.source.clone());
            }
        }
        out
    }

    /// Whether a curated set gives the map's instance a trash file
    /// (`trash_layers_of`).
    pub fn has_curated_trash(&self, map: u32) -> bool {
        self.trash_layers_of(map)
            .iter()
            .any(|s| matches!(s, Source::Curated(_)))
    }

    /// Whether the user's files give the map's instance a trash file
    /// (`trash_layers_of`).
    pub fn has_user_trash(&self, map: u32) -> bool {
        self.trash_layers_of(map).contains(&Source::User)
    }

    /// An instance's trash, by its season and slug, on `difficulty` at
    /// `tier`, uncapped: what laying a set checks. `None` when the base has
    /// no such instance.
    fn answer_trash(
        &self,
        season: &str,
        instance: &str,
        difficulty: Option<u32>,
        tier: Tier,
    ) -> Option<Result<Encounter, String>> {
        let s = self.seasons.iter().find(|s| s.id == season)?;
        let inst = s.instances.get(instance)?;
        Some(self.resolve_trash(s, inst, difficulty, tier, None))
    }

    /// Why a layer's trash file for `season`/`instance` would never be
    /// read: the instance gives no map id to find it by, or a newer season
    /// answers its map (a returning dungeon). `None` when its trash is
    /// answered from there.
    fn trash_elsewhere(&self, season: &str, instance: &str) -> Option<String> {
        let map = self
            .seasons
            .iter()
            .find(|s| s.id == season)
            .and_then(|s| s.instances.get(instance))
            .and_then(|i| i.map);
        let Some(map) = map else {
            return Some(format!(
                "{season}/{instance} gives no `map` id, which its trash is found by"
            ));
        };
        match self.instance_by_map(map) {
            Some((s, i)) if s.id == season && i.slug == instance => None,
            Some((s, i)) => Some(format!(
                "map {map}'s trash is answered from {}/{}; a layer's trash file goes there",
                s.id, i.slug
            )),
            None => Some(format!("no season answers map {map}")),
        }
    }

    /// Lay an instance's trash up to `tier`: its own creatures, then each
    /// set's trash file, then the difficulty overrides `difficulty` reads;
    /// and type the result.
    fn resolve_trash(
        &self,
        season: &Season,
        inst: &Instance,
        difficulty: Option<u32>,
        tier: Tier,
        prov: Option<&mut Provenance>,
    ) -> Result<Encounter, String> {
        let where_ = format!("{}/{}/trash", season.id, inst.slug);
        let mut stack = Stacked {
            t: Table::new(),
            overrides: Vec::new(),
            prov,
            where_: &where_,
        };
        // Who is in the dungeon: the instance's own creatures, the base's,
        // as written (a trash file may switch one back on; `typed` drops
        // the ones still off).
        if !inst.written.is_empty() {
            let npc = Value::try_from(&inst.written).map_err(|e| format!("{where_}: {e}"))?;
            stack.lay(Source::Instance, &Table::from_iter([("npc".into(), npc)]))?;
        }
        // What they do: each set's trash file, in the stack's order.
        for set in self.sets.iter().filter(|s| s.source.tier() <= tier) {
            if let Some(t) = set
                .instance(&season.id, &inst.slug)
                .and_then(|i| i.trash.as_ref())
            {
                stack.lay(set.source.clone(), t)?;
            }
        }
        stack.typed(
            difficulty,
            [
                ("schema", Value::Integer(i64::from(SCHEMA))),
                ("encounter", Value::Integer(0)),
                ("name", Value::String(inst.name.clone())),
                ("instance", Value::String(inst.slug.clone())),
            ],
        )
    }

    /// An encounter's season, instance and files, as the index places it.
    fn find(&self, id: u32) -> Option<(&Season, &Instance, &Layers)> {
        let (n, slug) = self.index.get(&id)?;
        let season = self.seasons.get(*n)?;
        let inst = season.instances.get(slug)?;
        Some((season, inst, inst.encounters.get(&id)?))
    }

    /// An encounter at `tier`, uncapped, who set each key kept in `prov`:
    /// `None` when no source up to `tier` holds its own file.
    fn answer(
        &self,
        id: u32,
        difficulty: Option<u32>,
        tier: Tier,
        prov: Option<&mut Provenance>,
    ) -> Option<Result<Encounter, String>> {
        let found @ (season, inst, layers) = self.find(id)?;
        let stands = layers.draft.is_some()
            || self.sets.iter().any(|set| {
                set.source.tier() <= tier && set.has_encounter(&season.id, &inst.slug, id)
            });
        if !stands {
            return None;
        }
        let mut e = self.resolve(found, id, difficulty, tier, prov);
        if let (Ok(e), Some(texts)) = (&mut e, self.texts.get(&id)) {
            texts.lay(e, difficulty);
        }
        Some(e)
    }

    /// Lay an encounter's sources up to `tier`, then the difficulty
    /// overrides `difficulty` reads, and type the result.
    fn resolve(
        &self,
        (season, inst, layers): (&Season, &Instance, &Layers),
        id: u32,
        difficulty: Option<u32>,
        tier: Tier,
        prov: Option<&mut Provenance>,
    ) -> Result<Encounter, String> {
        let where_ = format!("{}/{}/{}", season.id, inst.slug, layers.stem);
        let mut stack = Stacked {
            t: Table::new(),
            overrides: Vec::new(),
            prov,
            where_: &where_,
        };
        stack.lay(Source::Season, &season.defaults)?;
        stack.lay(Source::Instance, &inst.defaults)?;
        if let Some(d) = &layers.draft {
            stack.lay(Source::Draft, d)?;
        }
        // The map as the instance's file gives it: by hand, but part of the
        // base, so a room looks and changes the same at every tier.
        if let Some(d) = &layers.drawing {
            stack.lay(Source::Drawing, d)?;
        }
        // Each set over the base as one block: its season's file, its
        // instance's, its drawing of the encounter, the encounter's own.
        for set in self.sets.iter().filter(|s| s.source.tier() <= tier) {
            let Some(s) = set.season(&season.id) else {
                continue;
            };
            if let Some(t) = &s.defaults {
                stack.lay(set.source.clone(), t)?;
            }
            let Some(i) = s.instances.get(&inst.slug) else {
                continue;
            };
            if let Some(t) = &i.defaults {
                stack.lay(set.source.clone(), t)?;
            }
            if let Some(t) = i.drawings.get(&id) {
                stack.lay(set.source.clone(), t)?;
            }
            if let Some(t) = i.encounters.get(&id) {
                stack.lay(set.source.clone(), t)?;
            }
        }
        stack.typed(difficulty, [("instance", Value::String(inst.slug.clone()))])
    }
}

/// An encounter's table as its sources are laid on it: their own values
/// at once, their difficulty overrides kept, in the stack's order, for
/// the end.
struct Stacked<'a, 'p> {
    t: Table,
    overrides: Vec<(Source, Table)>,
    prov: Option<&'p mut Provenance>,
    where_: &'a str,
}

impl Stacked<'_, '_> {
    /// Lay one file of `source`: its `[difficulty]` overrides kept apart,
    /// checked, the rest laid at once.
    fn lay(&mut self, source: Source, file: &Table) -> Result<(), String> {
        let mut file = file.clone();
        let o =
            take_difficulties(&mut file).map_err(|e| format!("{} ({source}): {e}", self.where_))?;
        let by = SetBy {
            source,
            difficulty: None,
        };
        lay(
            &mut self.t,
            file,
            "",
            self.prov.as_deref_mut().map(|p| (p, &by)),
        );
        if !o.is_empty() {
            self.overrides.push((by.source, o));
        }
        Ok(())
    }

    /// The table laid, typed: the difficulty overrides `difficulty` reads
    /// laid last, the most general difficulty first and each difficulty's
    /// in the stack's order; each of `fill` set where no file set it (by
    /// the instance the encounter lies in); every entry switched off, or
    /// kept to other difficulties, dropped.
    fn typed<const N: usize>(
        self,
        difficulty: Option<u32>,
        fill: [(&str, Value); N],
    ) -> Result<Encounter, String> {
        let chain = difficulty.map(difficulty::chain).unwrap_or(&[]);
        let Stacked {
            mut t,
            overrides,
            mut prov,
            where_,
        } = self;
        for name in chain.iter().rev() {
            for (source, o) in &overrides {
                if let Some(Value::Table(o)) = o.get(*name) {
                    let by = SetBy {
                        source: source.clone(),
                        difficulty: Some(name),
                    };
                    lay(&mut t, o.clone(), "", prov.as_deref_mut().map(|p| (p, &by)));
                }
            }
        }
        for (k, v) in fill {
            if !t.contains_key(k) {
                t.insert(k.into(), v);
                if let Some(p) = prov.as_deref_mut() {
                    p.set(
                        k,
                        SetBy {
                            source: Source::Instance,
                            difficulty: None,
                        },
                    );
                }
            }
        }
        let mut e =
            Encounter::deserialize(Value::Table(t)).map_err(|e| format!("{where_}: {e}"))?;
        let grounds: std::collections::BTreeSet<String> = e.ground.keys().cloned().collect();
        gate(&mut e, |g| {
            g.enabled() && (difficulty.is_none() || difficulty::applies(g.only(), chain))
        });
        // An ability under one this difficulty does not have stands alone.
        let kept: std::collections::BTreeSet<String> = e.ability.keys().cloned().collect();
        for a in e.ability.values_mut() {
            if a.under.as_ref().is_some_and(|u| !kept.contains(u)) {
                a.under = None;
            }
        }
        // A ground this difficulty does not have (kept to others, or
        // switched off) is cleared by none: Mythic Keystone lays the Mythic
        // overlays, whose `clears` may name a Mythic-only ground it lacks.
        // A name that was never a ground stays, for `check` to refuse.
        let kept: std::collections::BTreeSet<String> = e.ground.keys().cloned().collect();
        for g in e.ground.values_mut() {
            for list in [&mut g.clears, &mut g.sweeps] {
                list.retain(|c| kept.contains(c) || !grounds.contains(c));
            }
        }
        Ok(e)
    }
}

/// Every encounter by id, where its files are: its season's place in
/// `seasons` (newest first) and its instance. The newest season holding an
/// id gives it: a returning dungeon sits in two. Two instances giving one
/// id, in one season or in two seasons of one `order`, is an error naming
/// both.
fn index(seasons: &[Season]) -> Result<BTreeMap<u32, (usize, String)>, Vec<String>> {
    let mut index: BTreeMap<u32, (usize, String)> = BTreeMap::new();
    let mut errs = Vec::new();
    let at = |s: &Season, slug: &str, id: u32| {
        let stem = s
            .instances
            .get(slug)
            .and_then(|i| i.encounters.get(&id))
            .map_or("", |l| l.stem.as_str());
        format!("{}/{slug}/{stem}", s.id)
    };
    for (n, s) in seasons.iter().enumerate() {
        for (slug, i) in &s.instances {
            for &id in i.encounters.keys() {
                match index.get(&id) {
                    None => {
                        index.insert(id, (n, slug.clone()));
                    }
                    Some((m, other)) => {
                        if let Some(first) = seasons.get(*m).filter(|f| f.order == s.order) {
                            errs.push(format!(
                                "{}: encounter {id} is {} too; a season gives an encounter once",
                                at(s, slug, id),
                                at(first, other, id)
                            ));
                        }
                    }
                }
            }
        }
    }
    if errs.is_empty() {
        Ok(index)
    } else {
        Err(errs)
    }
}

/// Every dungeon level by UiMap id, where its file is, as `index` places
/// an encounter: the newest season's, and two instances giving one id in
/// one season, or in two seasons of one `order`, an error naming both.
fn level_index(seasons: &[Season]) -> Result<BTreeMap<u32, (usize, String)>, Vec<String>> {
    let mut index: BTreeMap<u32, (usize, String)> = BTreeMap::new();
    let mut errs = Vec::new();
    for (n, s) in seasons.iter().enumerate() {
        for (slug, i) in &s.instances {
            for &id in i.levels.keys() {
                match index.get(&id) {
                    None => {
                        index.insert(id, (n, slug.clone()));
                    }
                    Some((m, other)) => {
                        if let Some(first) = seasons.get(*m).filter(|f| f.order == s.order) {
                            errs.push(format!(
                                "{}/{slug}: [level.{id}] is {}/{other}'s too; a season gives a level once",
                                s.id, first.id
                            ));
                        }
                    }
                }
            }
        }
    }
    if errs.is_empty() {
        Ok(index)
    } else {
        Err(errs)
    }
}

/// Each level an instance file's `level` table gives, by UiMap id: a key
/// that is no UiMap id, or a level giving anything but `LEVEL_TABLES`, is
/// an error naming the file.
fn read_levels(path: &str, level: Table, errs: &mut Vec<String>) -> BTreeMap<u32, Table> {
    let mut out = BTreeMap::new();
    for (k, v) in level {
        let Ok(id) = k.parse::<u32>() else {
            errs.push(format!(
                "{path}: [level.{k}]: a level is named by its UiMap id"
            ));
            continue;
        };
        match v {
            Value::Table(t) if t.keys().all(|k| LEVEL_TABLES.contains(&k.as_str())) => {
                out.insert(id, t);
            }
            _ => errs.push(format!(
                "{path}: [level.{id}]: a level gives only its `{}`",
                LEVEL_TABLES.join("`, `")
            )),
        }
    }
    out
}

/// A level laid over its season's and instance's `[defaults]` (their
/// `LEVEL_DEFAULTS` alone) and typed: its map's UiMap its own unless it
/// says one, every entry switched off dropped, and the instance's
/// creatures laid in (`Level::npc`), which its pulls name.
fn resolve_level(
    season: &Season,
    inst: &Instance,
    ui_map: u32,
    own: &Table,
) -> Result<Level, String> {
    let mut t = Table::new();
    for defaults in [&season.defaults, &inst.defaults] {
        let read: Table = defaults
            .iter()
            .filter(|(k, _)| LEVEL_DEFAULTS.contains(&k.as_str()))
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        merge(&mut t, read);
    }
    merge(&mut t, own.clone());
    let mut l = Level::deserialize(Value::Table(t))
        .map_err(|e| format!("{}/{}/level.{ui_map}: {e}", season.id, inst.slug))?;
    l.ui_map = ui_map;
    l.instance = inst.slug.clone();
    if l.map.ui_map.is_none() {
        l.map.ui_map = Some(ui_map);
    }
    let keep = |g: &dyn Gated| g.enabled();
    gate_map(&mut l.map, &keep);
    l.pull.retain(|_, p| keep(p));
    l.npc = inst.npcs().clone();
    Ok(l)
}

/// A `[defaults]` table's keys, each one of `DEFAULTS_TABLES` or the
/// `difficulty` overrides of them: an error names the first that is not,
/// as `[defaults.<key>]`.
pub(crate) fn defaults_keys(d: &Table) -> Result<(), String> {
    let refused = |at: String, k: &str| {
        if k == "pull" {
            format!("[{at}.pull]: a pull is one level's alone, never a default")
        } else {
            format!(
                "[{at}.{k}]: a default gives only an encounter's tables (`{}`), which every encounter under it reads",
                DEFAULTS_TABLES.join("`, `")
            )
        }
    };
    for (k, v) in d {
        match (k.as_str(), v) {
            ("difficulty", Value::Table(overlays)) => {
                for (name, over) in overlays {
                    if let Value::Table(over) = over
                        && let Some(k) =
                            over.keys().find(|k| !DEFAULTS_TABLES.contains(&k.as_str()))
                    {
                        return Err(refused(format!("defaults.difficulty.{name}"), k));
                    }
                }
            }
            (k, _) if DEFAULTS_TABLES.contains(&k) => {}
            (k, _) => return Err(refused("defaults".into(), k)),
        }
    }
    Ok(())
}

fn seasons_ids(seasons: &[Season]) -> String {
    seasons
        .iter()
        .map(|s| s.id.as_str())
        .collect::<Vec<_>>()
        .join(",")
}

/// A file's table, brought to `SCHEMA`: every file names the format it was
/// written in, and a migration per older version runs in order, each one
/// version forward, the file then naming `SCHEMA`.
fn migrate(mut t: Table) -> Result<Table, String> {
    let v = t
        .get("schema")
        .and_then(Value::as_integer)
        .ok_or("no `schema = N`")?;
    let no_way = || format!("schema {v} has no migration to {SCHEMA}");
    let mut n = u32::try_from(v).map_err(|_| no_way())?;
    if n > SCHEMA {
        return Err(format!(
            "schema {n} is newer than this build reads ({SCHEMA})"
        ));
    }
    while n < SCHEMA {
        n = match n {
            // 1 → 2: the instance's creatures and forces, an NPC's `forces`
            // and `aloft`: keys added, none changed.
            1 => 2,
            _ => return Err(no_way()),
        };
    }
    t.insert("schema".into(), Value::Integer(i64::from(SCHEMA)));
    Ok(t)
}

/// Lay `over` onto `base`: tables merge key by key, anything else replaces.
fn merge(base: &mut Table, over: Table) {
    lay(base, over, "", None);
}

/// Take `t`'s `[difficulty]` overrides out of it, by difficulty name. A
/// `difficulty` that is no table, or an override named for no difficulty,
/// is an error.
fn take_difficulties(t: &mut Table) -> Result<Table, String> {
    let overlays = match t.remove("difficulty") {
        Some(Value::Table(o)) => o,
        Some(_) => return Err("`difficulty` is a table of difficulty names".into()),
        None => return Ok(Table::new()),
    };
    if let Some(name) = overlays
        .keys()
        .find(|n| !difficulty::NAMES.contains(&n.as_str()))
    {
        return Err(format!(
            "[difficulty.{name}]: not a difficulty ({})",
            difficulty::NAMES.join(", ")
        ));
    }
    Ok(overlays)
}

/// Take `t`'s `[difficulty]` overrides out of it and lay the ones `chain`
/// names over it, the most general first (`chain` is the most specific
/// first, as `difficulty::chain` gives it). A `difficulty` that is no
/// table, or an override named for no difficulty, is an error.
fn lay_difficulties(t: &mut Table, chain: &[&str]) -> Result<(), String> {
    let mut overlays = take_difficulties(t)?;
    for name in chain.iter().rev() {
        if let Some(Value::Table(o)) = overlays.remove(*name) {
            merge(t, o);
        }
    }
    Ok(())
}

/// Drop every named entry `keep` refuses: one switched off, or kept to
/// difficulties the one resolved is not.
fn gate(e: &mut Encounter, keep: impl Fn(&dyn Gated) -> bool) {
    fn retain<T: Gated>(entries: &mut BTreeMap<String, T>, keep: &dyn Fn(&dyn Gated) -> bool) {
        entries.retain(|_, entry| keep(entry));
    }
    // Every collection of named entries, so a new one does not build here
    // until it is gated or said not to be.
    let Encounter {
        map,
        npc,
        ability,
        phase,
        place,
        ground,
        mark,
        line,
        volley,
        flash,
        knock,
        range,
        apart,
        streak,
        room,
        reach,
        event,
        schema: _,
        encounter: _,
        name: _,
        instance: _,
        order: _,
        view: _,
        spell: _,
        travel: _,
    } = e;
    gate_map(map, &keep);
    retain(npc, &keep);
    retain(ability, &keep);
    retain(phase, &keep);
    retain(place, &keep);
    retain(ground, &keep);
    retain(mark, &keep);
    retain(line, &keep);
    retain(volley, &keep);
    retain(flash, &keep);
    retain(knock, &keep);
    retain(range, &keep);
    retain(apart, &keep);
    retain(streak, &keep);
    retain(room, &keep);
    retain(reach, &keep);
    retain(event, &keep);
}

/// Drop every named entry of a map `keep` refuses: an encounter's or a
/// level's.
fn gate_map(map: &mut Map, keep: &dyn Fn(&dyn Gated) -> bool) {
    fn retain<T: Gated>(entries: &mut BTreeMap<String, T>, keep: &dyn Fn(&dyn Gated) -> bool) {
        entries.retain(|_, entry| keep(entry));
    }
    // Every collection of the map's named entries, so a new one does not
    // build here until it is gated or said not to be.
    let Map {
        layer,
        glow,
        fade,
        area,
        path,
        ui_map: _,
        floor: _,
        level: _,
        ceiling: _,
        hang: _,
        light: _,
        exclude_groups: _,
        exclude_models: _,
        exclude_textures: _,
        stencil: _,
        ppy: _,
        crop: _,
        include_groups: _,
        include_models: _,
        ground: _,
        exposure: _,
        transition: _,
        water: _,
    } = map;
    retain(layer, keep);
    retain(glow, keep);
    retain(fade, keep);
    retain(area, keep);
    retain(path, keep);
}

#[cfg(test)]
mod tests;
