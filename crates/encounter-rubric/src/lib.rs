//! The seasonal encounter rubric: what the replay knows about each boss of
//! a season beyond what the log and the game's tables give for free — the
//! room's tweaks, the default view, the NPCs and their roles, the abilities
//! and their shapes, the phases and what begins them, the floor's fixed
//! features, what the fight leaves on it and takes back, and the events
//! that change the room.
//!
//! The rubric is TOML under the repository's `rubric/` (its `README.md`
//! is the schema's reference), embedded at build time:
//!
//! ```text
//! rubric/<season>/season.toml                    the season, its defaults
//! rubric/<season>/<instance>/instance.toml       the raid or dungeon
//! rubric/<season>/<instance>/<id>-<slug>.draft.toml   generated (gen-rubric)
//! rubric/<season>/<instance>/<id>-<slug>.toml         tuned by hand
//! ```
//!
//! An encounter resolves by laying, each over the last: the season's
//! defaults, the instance's, the generated draft, the encounter's map and
//! NPCs as the instance's file gives them (`[encounter.<id>]`: its `map`
//! and `view`, what changes the map: its `room`s, `place`s and the `event`s
//! they key on, and an `npc` the draft lacks or has wrong), the tuned
//! file, then the difficulty overrides its fallback
//! chain names, the most general first (`difficulty`). Tables merge key by
//! key; anything else is replaced. An entry with `enabled = false`, or
//! whose `only` leaves the difficulty out, is dropped.
//!
//! An encounter resolves at a `Tier`: the BASE is what extraction gives,
//! and its map and NPCs, with no other hand amendment (the season's and
//! the instance's files and the generated draft, with the draft's own
//! difficulty overrides); CURATED lays the tuned file over the base. The
//! map is the base's, however much it was worked on by hand (a stencil, a
//! level, the view's turn, a platform that breaks on an event, a room off
//! the arena), because a room should look and change the same at every
//! tier; so is what an NPC is where the journal is silent or wrong (Ula'tek's
//! Venomous Heart, sharing Ula'tek's health), because who is in the room
//! should too. The curated layer is what the fight does in it. The two live side
//! by side under `rubric/` for now, but nothing in the base depends on a
//! tuned file, so the curated layer can be shipped apart from it. A reader
//! asks for a tier per encounter (`encounter_at`); the rubric's own
//! (`with_tier`) is the most it will answer.
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
pub mod placed;
pub mod portraits;
pub mod schema;
pub mod text;
pub mod write;

use std::collections::BTreeMap;
use std::path::PathBuf;

pub use check::check;
use schema::Gated;
pub use schema::*;
use serde::Deserialize;
use serde::de::DeserializeOwned;
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

/// Which of an encounter's layers it resolves with, the lesser first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default)]
pub enum Tier {
    /// What extraction gives with no hand amendment: the season's and the
    /// instance's files and the generated draft (`gen-rubric`).
    Base,
    /// The tuned file laid over the base: what was found by investigating
    /// the fight and checking it against logs and the game.
    #[default]
    Curated,
}

impl Tier {
    /// Its name, as a control or a command line spells it.
    pub fn name(self) -> &'static str {
        match self {
            Tier::Base => "base",
            Tier::Curated => "curated",
        }
    }

    /// The tier a name spells (`name`).
    pub fn from_name(s: &str) -> Option<Tier> {
        match s {
            "base" => Some(Tier::Base),
            "curated" => Some(Tier::Curated),
            _ => None,
        }
    }
}

/// Every season the rubric holds, newest first.
#[derive(Debug, Clone)]
pub struct Rubric {
    seasons: Vec<Season>,
    /// Every encounter it answers, by id: its season's place in `seasons`
    /// and its instance's slug (`index`).
    index: BTreeMap<u32, (usize, String)>,
    version: String,
    /// Each encounter's journal text on this machine, by id (`with_texts`).
    texts: BTreeMap<u32, text::Texts>,
    /// The sidecars `with_texts` found and could not read.
    text_errors: Vec<String>,
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

/// One encounter's two files, and its map and NPCs as its instance's file
/// gives them.
#[derive(Debug, Clone, Default)]
struct Layers {
    stem: String,
    draft: Option<Table>,
    /// `[encounter.<id>]` of the instance's file: `{ map = …, view = …,
    /// room = …, place = …, event = …, npc = … }`.
    drawing: Option<Table>,
    tuned: Option<Table>,
}

/// What a file under `rubric/` is, by where it lies.
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
    /// A path's role, relative to `rubric/`: `None` for a file beside an
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

impl Rubric {
    /// The rubric this binary was built with.
    pub fn embedded() -> Result<Rubric, Vec<String>> {
        Rubric::from_files(
            embedded::FILES
                .iter()
                .map(|&(p, t)| (p.to_string(), t.to_string())),
        )
    }

    /// The rubric from `(path, text)` pairs, paths relative to `rubric/`.
    pub fn from_files(
        files: impl IntoIterator<Item = (String, String)>,
    ) -> Result<Rubric, Vec<String>> {
        let mut files: Vec<(String, String)> = files.into_iter().collect();
        files.sort();
        let mut hash = 0xcbf2_9ce4_8422_2325u64;
        for (path, text) in &files {
            for b in path.bytes().chain(text.bytes()) {
                hash ^= u64::from(b);
                hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
            }
        }
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
            let file = if draft {
                &mut layers.draft
            } else {
                &mut layers.tuned
            };
            *file = Some(t);
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
        let version = format!("{}+{hash:016x}", seasons_ids(&seasons));
        Ok(Rubric {
            seasons,
            index,
            version,
            texts: BTreeMap::new(),
            text_errors: Vec::new(),
            tier: Tier::Curated,
        })
    }

    /// Every season, newest first.
    pub fn seasons(&self) -> &[Season] {
        &self.seasons
    }

    /// What an interpretation records it ran under: the seasons and a hash
    /// of every file.
    pub fn version(&self) -> &str {
        &self.version
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

    /// The most any encounter resolves with (`with_tier`; curated unless
    /// lowered).
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

    /// Whether an encounter has a curated layer over its base: a tuned file.
    pub fn has_curated(&self, id: u32) -> bool {
        self.find(id).is_some_and(|(_, _, l)| l.tuned.is_some())
    }

    /// An encounter as `encounter` gives it, at `tier` or the rubric's own,
    /// whichever is less. `None` too when the base is asked for and the
    /// encounter has no draft.
    pub fn encounter_at(
        &self,
        id: u32,
        difficulty: Option<u32>,
        tier: Tier,
    ) -> Option<Result<Encounter, String>> {
        let tier = tier.min(self.tier);
        let (season, inst, layers) = self.find(id)?;
        if tier == Tier::Base && layers.draft.is_none() {
            return None;
        }
        let mut e = resolve(season, inst, layers, difficulty, tier);
        if let (Ok(e), Some(texts)) = (&mut e, self.texts.get(&id)) {
            texts.lay(e, difficulty);
        }
        Some(e)
    }

    /// Every encounter id the rubric answers, with the season and instance
    /// it answers it from: the newest season's, seasons newest first, each
    /// id once.
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
    for (k, v) in over {
        match (base.get_mut(&k), v) {
            (Some(Value::Table(b)), Value::Table(o)) => merge(b, o),
            (_, v) => {
                base.insert(k, v);
            }
        }
    }
}

/// Take `t`'s `[difficulty]` overrides out of it and lay the ones `chain`
/// names over it, the most general first (`chain` is the most specific
/// first, as `difficulty::chain` gives it). A `difficulty` that is no
/// table, or an override named for no difficulty, is an error.
fn lay_difficulties(t: &mut Table, chain: &[&str]) -> Result<(), String> {
    let mut overlays = match t.remove("difficulty") {
        Some(Value::Table(o)) => o,
        Some(_) => return Err("`difficulty` is a table of difficulty names".into()),
        None => return Ok(()),
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
    for name in chain.iter().rev() {
        if let Some(Value::Table(o)) = overlays.remove(*name) {
            merge(t, o);
        }
    }
    Ok(())
}

fn resolve(
    season: &Season,
    inst: &Instance,
    layers: &Layers,
    difficulty: Option<u32>,
    tier: Tier,
) -> Result<Encounter, String> {
    let mut t = Table::new();
    merge(&mut t, season.defaults.clone());
    merge(&mut t, inst.defaults.clone());
    if let Some(d) = &layers.draft {
        merge(&mut t, d.clone());
    }
    // The map as the instance's file gives it: by hand, but part of the
    // base, so a room looks and changes the same at every tier.
    if let Some(d) = &layers.drawing {
        merge(&mut t, d.clone());
    }
    // The tuned file and its difficulty overrides are the curated layer.
    if tier == Tier::Curated
        && let Some(u) = &layers.tuned
    {
        merge(&mut t, u.clone());
    }
    let where_ = format!("{}/{}/{}", season.id, inst.slug, layers.stem);
    let chain = difficulty.map(difficulty::chain).unwrap_or(&[]);
    lay_difficulties(&mut t, chain).map_err(|e| format!("{where_}: {e}"))?;
    t.entry("instance")
        .or_insert_with(|| Value::String(inst.slug.clone()));
    let mut e = Encounter::deserialize(Value::Table(t)).map_err(|e| format!("{where_}: {e}"))?;
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
