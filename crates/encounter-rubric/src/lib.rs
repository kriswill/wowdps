//! The seasonal encounter rubric: what the replay knows about each boss of
//! a season beyond what the log and the game's tables give for free — the
//! room's tweaks, the default view, the NPCs and their roles, the abilities
//! and their shapes, the phases and what begins them, the floor's fixed
//! features, what the fight leaves on it and takes back, and the events
//! that change the room.
//!
//! The rubric is TOML under the crate's `seasons/` (the crate's `README.md`
//! is the schema's reference), embedded at build time:
//!
//! ```text
//! seasons/<season>/season.toml                    the season, its defaults
//! seasons/<season>/<instance>/instance.toml       the raid or dungeon
//! seasons/<season>/<instance>/<id>-<slug>.draft.toml   generated (gen-rubric)
//! seasons/<season>/<instance>/<id>-<slug>.toml         tuned by hand
//! ```
//!
//! An encounter resolves through an ordered stack of SOURCES (`Source`),
//! each laid over the last: the season's defaults, the instance's, the
//! generated draft, the encounter's map and NPCs as the instance's file
//! gives them (`[encounter.<id>]`: its `map` and `view`, what changes the
//! map: its `room`s, `place`s and the `event`s they key on, and an `npc`
//! the draft lacks or has wrong); then every curated file set in the order
//! it was laid (a tuned file embedded beside its draft, then each bundle
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
//!   unchanged, and a curated set ships apart from the base as a bundle.
//! - USER: the files on this machine, laid last, so a user can override
//!   anything for themselves.
//!
//! A reader asks for a tier per encounter (`encounter_at`); the rubric's
//! own (`with_tier`, the user tier unless lowered) is the most it will
//! answer.
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
//! replay alike.

mod check;
pub mod difficulty;
pub mod floor;
mod layer;
pub mod placed;
pub mod portraits;
pub mod schema;
mod stack;
pub mod text;
pub mod write;

use std::collections::BTreeMap;
use std::path::PathBuf;

pub use check::check;
pub use layer::{Origin, Provenance, SetBy, Source};
use layer::{Set, lay};
use schema::Gated;
pub use schema::*;
use serde::Deserialize;
use serde::de::DeserializeOwned;
pub use stack::user_dir;
use toml::{Table, Value};

/// The rubric format this build reads and writes. A file in an older one is
/// migrated forward on load (`migrate`); a newer one is refused.
pub const SCHEMA: u32 = 1;

/// What an instance file's `[encounter.<id>]` may hold: the encounter's map,
/// how it is first shown, what changes it in the fight (rooms off it, its
/// fixed places, and the events its layers, rooms and places key on), and
/// its NPCs where the draft lacks one or has it wrong.
const INSTANCE_TABLES: [&str; 6] = ["map", "view", "room", "place", "event", "npc"];

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
    /// FNV-1a over every file the rubric holds, the base's and every set
    /// laid over it alike (`version`).
    hash: u64,
    version: String,
    /// Each encounter's journal text on this machine, by id (`with_texts`).
    texts: BTreeMap<u32, text::Texts>,
    /// The sidecars `with_texts` found and could not read.
    text_errors: Vec<String>,
    /// The file sets laid over the base, in the stack's order: the
    /// curated in the order laid (the embedded tuned files first), then
    /// the user's.
    sets: Vec<Set>,
    /// The user's files `with_user` and `with_user_dir` could not lay.
    user_errors: Vec<String>,
    /// The most any encounter resolves with (`with_tier`).
    tier: Tier,
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
    #[serde(skip)]
    pub instances: BTreeMap<String, Instance>,
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
    #[serde(default)]
    defaults: Table,
    #[serde(skip)]
    encounters: BTreeMap<u32, Layers>,
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

impl Rubric {
    /// The rubric this binary was built with.
    pub fn embedded() -> Result<Rubric, Vec<String>> {
        Rubric::from_files(
            embedded::FILES
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
        let mut files: Vec<(String, String)> = files.into_iter().collect();
        files.sort();
        let hash = hash_files(
            0xcbf2_9ce4_8422_2325,
            files.iter().map(|(p, t)| (p.as_str(), t.as_str())),
        );
        let mut errs = Vec::new();
        let mut roles = Vec::new();
        for (path, text) in &files {
            match FileRole::of(path) {
                Ok(Some(role)) => roles.push((path.as_str(), text.as_str(), role)),
                Ok(None) => {}
                Err(e) => errs.push(format!("{path}: {e}")),
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
            match typed::<Instance>(t) {
                Ok(i) => {
                    drawings.extend(Drawing::read(path, season, instance, encounter, &mut errs));
                    let slug = instance.to_string();
                    s.instances.insert(slug.clone(), Instance { slug, ..i });
                }
                Err(e) => errs.push(format!("{path}: {e}")),
            }
        }
        // The tuned files beside their drafts: the curated tier's first set.
        let mut tuned = Set::new(Source::Curated(Origin::Embedded));
        for &(path, text, ref role) in &roles {
            let FileRole::Encounter {
                season,
                instance,
                id,
                stem,
                draft,
            } = *role
            else {
                continue;
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
        let index = index(&seasons)?;
        let mut r = Rubric {
            seasons,
            index,
            hash,
            version: String::new(),
            texts: BTreeMap::new(),
            text_errors: Vec::new(),
            sets: vec![tuned],
            user_errors: Vec::new(),
            tier: Tier::default(),
        };
        r.version = r.versioned();
        Ok(r)
    }

    /// Every season, newest first.
    pub fn seasons(&self) -> &[Season] {
        &self.seasons
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
        let chain = difficulty.map(difficulty::chain).unwrap_or(&[]);
        let Stacked {
            mut t,
            overrides,
            mut prov,
            ..
        } = stack;
        // The difficulty overrides last, the most general difficulty first,
        // and each difficulty's in the stack's order.
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
        if !t.contains_key("instance") {
            t.insert("instance".into(), Value::String(inst.slug.clone()));
            if let Some(p) = prov {
                p.set(
                    "instance",
                    SetBy {
                        source: Source::Instance,
                        difficulty: None,
                    },
                );
            }
        }
        let mut e =
            Encounter::deserialize(Value::Table(t)).map_err(|e| format!("{where_}: {e}"))?;
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
        Ok(e)
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

fn seasons_ids(seasons: &[Season]) -> String {
    seasons
        .iter()
        .map(|s| s.id.as_str())
        .collect::<Vec<_>>()
        .join(",")
}

/// A file's table, brought to `SCHEMA`: every file names the format it was
/// written in, and a migration per older version runs in order. None yet.
fn migrate(t: Table) -> Result<Table, String> {
    let v = t
        .get("schema")
        .and_then(Value::as_integer)
        .ok_or("no `schema = N`")?;
    match u32::try_from(v) {
        Ok(SCHEMA) => Ok(t),
        Ok(n) if n > SCHEMA => Err(format!(
            "schema {n} is newer than this build reads ({SCHEMA})"
        )),
        _ => Err(format!("schema {v} has no migration to {SCHEMA}")),
    }
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
    } = e;
    let Map {
        layer,
        glow,
        ui_map: _,
        floor: _,
        level: _,
        ceiling: _,
        light: _,
        exclude_groups: _,
        exclude_models: _,
        exclude_textures: _,
        stencil: _,
        ppy: _,
    } = map;
    retain(npc, &keep);
    retain(ability, &keep);
    retain(phase, &keep);
    retain(place, &keep);
    retain(ground, &keep);
    retain(mark, &keep);
    retain(line, &keep);
    retain(volley, &keep);
    retain(flash, &keep);
    retain(range, &keep);
    retain(apart, &keep);
    retain(streak, &keep);
    retain(room, &keep);
    retain(reach, &keep);
    retain(event, &keep);
    retain(layer, &keep);
    retain(glow, &keep);
}

#[cfg(test)]
mod tests;
