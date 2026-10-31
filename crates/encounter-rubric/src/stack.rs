//! Laying file sets over the base at runtime: a curated bundle
//! (`Rubric::with_curated`), the tuned files of a `seasons/` tree on disk in
//! the embedded ones' place (`Rubric::with_curated_dir`, or a walk of it,
//! `Rubric::with_curated_tree`) and the user's own files
//! (`Rubric::with_user`, `Rubric::with_user_dir`).
//!
//! A set is a tree shaped like `seasons/`, paths relative to it. Its files
//! may be a `<season>/season.toml` (`[defaults]` alone), an
//! `<season>/<instance>/instance.toml` (`[defaults]` and the
//! `[encounter.<id>]` drawing tables an instance file may give) and
//! `<season>/<instance>/<id>-<slug>.toml`, and an instance's
//! `<season>/<instance>/trash.toml`; every season and instance it names
//! must be the base's, an encounter's file and drawing lie where the rubric
//! answers the encounter (a returning dungeon's newest season), as a trash
//! file lies where the rubric answers its map, the file keeps the slug the
//! encounter's other files have, and a draft is the base's alone. Each file
//! still carries `schema = N`.
//!
//! A tree on disk gives its tuned `<id>-<slug>.toml` and `trash.toml` files
//! alone: its drafts, `season.toml` and `instance.toml` are the base's,
//! which stays the one the rubric was made from. A reader that wants the
//! tree's base too reads the whole tree with `Rubric::from_dir` instead.
//!
//! A curated set is laid whole or not at all: any file it cannot lay, or
//! any encounter it makes unreadable where the tiers under it read it, is
//! an error, and nothing is laid. The user's files are laid one by one,
//! and one that cannot be laid, or that would make an encounter
//! unreadable, is named in `user_errors` and left out: a user's mistake
//! never takes the rubric down.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::path::{Path, PathBuf};

use toml::{Table, Value};

use crate::layer::{Placed, Set};
use crate::{
    Drawing, FileRole, Layers, Origin, Rubric, Source, Tier, difficulty, hash_files, index, parse,
    trash_keys,
};

/// How deep under the user's directory a file is looked for: a rubric file
/// lies at most two directories down; deeper is still read, so a misplaced
/// file is named rather than ignored, but a link that loops ends here.
const USER_DEPTH: usize = 8;

/// Where the user's own rubric files live on this machine:
/// `$XDG_CONFIG_HOME/wowdps/rubric`, else `~/.config/wowdps/rubric`, as
/// `wowdps_proto::dirs` resolves the config home. None without either.
pub fn user_dir() -> Option<PathBuf> {
    wowdps_proto::dirs::config_home().map(|c| c.join("wowdps").join("rubric"))
}

/// The `.toml` files of a tree shaped like `seasons/` on disk, read and
/// not yet parsed (`read_tree`): what `Rubric::from_tree` and
/// `Rubric::with_user_tree` lay, and a fingerprint of them, so a reader
/// can tell a tree unchanged since it last read it by a walk alone.
#[derive(Debug, Clone)]
pub struct TreeFiles {
    pub(crate) root: PathBuf,
    /// Whether `root` is a directory.
    pub(crate) present: bool,
    /// Each file by its path relative to `root` (`/`-joined) and its text,
    /// in path order.
    pub(crate) files: Vec<(String, String)>,
    /// What could not be read, each by its place on disk.
    pub(crate) errors: Vec<String>,
}

impl TreeFiles {
    /// Every `.toml` under `root`, read now, and what could not be read. A
    /// root that is not there holds nothing, and is no error here. A
    /// dot-file (an editor's lock or swap file), a link to nothing and a
    /// file gone since its directory was listed are passed over, as is any
    /// name that is not UTF-8 but a `.toml`'s.
    pub fn read(root: &Path) -> TreeFiles {
        let mut files = Vec::new();
        let mut errors = Vec::new();
        read_tree(root, "", 0, &mut files, &mut errors);
        files.sort();
        TreeFiles {
            root: root.to_path_buf(),
            present: root.is_dir(),
            files,
            errors,
        }
    }

    /// Where it was read from.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// FNV-1a over the root, every file's path and text in path order, and
    /// every entry that could not be read: the same while nothing read
    /// under the root has changed. No file is parsed.
    pub fn fingerprint(&self) -> u64 {
        let root = self.root.display().to_string();
        let present = if self.present { "dir" } else { "none" };
        hash_files(
            0xcbf2_9ce4_8422_2325,
            [("\0root\0", root.as_str()), ("\0present\0", present)]
                .into_iter()
                .chain(self.files.iter().map(|(p, t)| (p.as_str(), t.as_str())))
                .chain(self.errors.iter().map(|e| ("\0error\0", e.as_str()))),
        )
    }
}

/// What a laid file is read by, and so what laying it is checked against:
/// an encounter, or an instance's trash.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Subject {
    /// An encounter, by its DungeonEncounterID.
    Encounter(u32),
    /// An instance's trash, by its season and slug.
    Trash { season: String, instance: String },
}

impl fmt::Display for Subject {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Subject::Encounter(id) => write!(f, "encounter {id}"),
            Subject::Trash { season, instance } => write!(f, "{season}/{instance}'s trash"),
        }
    }
}

/// A subject on one difficulty (`None`: its own reading).
type Reading = (Subject, Option<u32>);

impl Rubric {
    /// The rubric with a curated file set laid over it, named `origin`
    /// (`Source::Curated(Origin::Bundle(origin))`): after the embedded
    /// tuned files and every bundle laid before it, under the user's. An
    /// error refuses the whole set, each naming its file.
    pub fn with_curated(
        mut self,
        origin: impl Into<String>,
        files: impl IntoIterator<Item = (String, String)>,
    ) -> Result<Rubric, Vec<String>> {
        let origin = origin.into();
        let source = Source::Curated(Origin::Bundle(origin.clone()));
        let at = self
            .sets
            .iter()
            .position(|s| s.source.tier() > Tier::Curated)
            .unwrap_or(self.sets.len());
        self.sets.insert(at, Set::new(source.clone()));
        let mut errs = Vec::new();
        let files = ordered(files, |p| p.to_string(), &mut errs);
        let mut touched = BTreeSet::new();
        for (path, text) in &files {
            match self.place(at, path, path, text) {
                Ok(placed) => touched.extend(self.touched(&placed)),
                Err(e) => errs.extend(e),
            }
        }
        if !errs.is_empty() {
            return Err(errs);
        }
        let broken = self.failing(&touched, Tier::Curated);
        if !broken.is_empty() {
            // Only what reads under the set and not over it is the set's.
            let set = self.sets.remove(at);
            let before = self.failing(&touched, Tier::Curated);
            errs = blame(&source.to_string(), broken, &before);
            if !errs.is_empty() {
                return Err(errs);
            }
            self.sets.insert(at, set);
        }
        self.relay_user_over(&touched, &source.to_string());
        // A set that lays nothing leaves the version as it was.
        if !files.is_empty() {
            self.hash = hash_files(
                self.hash,
                std::iter::once(("\0curated\0", origin.as_str()))
                    .chain(files.iter().map(|(p, t)| (p.as_str(), t.as_str()))),
            );
            self.version = self.versioned();
        }
        Ok(self)
    }

    /// The rubric with the tuned and trash files of a `seasons/` tree on
    /// disk (a checkout's `crates/encounter-rubric/seasons/`) as its first
    /// curated set, named `origin` (`Source::Curated(Origin::Dir(origin))`),
    /// in place of the ones it was built with (`Origin::Embedded`, or a
    /// tree laid there before): a file saved into the tree is read by the
    /// next rubric made, with no rebuild. Bundles laid with `with_curated`
    /// stay over it, and the user's files over everything.
    ///
    /// Only the tree's tuned `<season>/<instance>/<id>-<slug>.toml` files
    /// and its instances' `trash.toml` files are read: its drafts,
    /// `season.toml` and `instance.toml` files are the base's, and the base
    /// stays the one this rubric was made from (`Rubric::from_dir` reads a
    /// tree's base and tuned files alike). The files are laid as
    /// `with_curated` lays a bundle, through the same checks, and the set
    /// is refused whole the same way: a file that cannot be read or laid,
    /// or an encounter or an instance's trash the set would make unreadable
    /// on any difficulty (the replaced files' as well as its own), is an
    /// error naming its file by its place on disk, and nothing is laid. A
    /// `dir` that is no directory is an error too; one holding no tuned or
    /// trash file leaves the curated tier reading the base. An encounter
    /// only the replaced files brought (no draft) goes with them unless the
    /// tree holds its file too, and a trash the replaced files gave reads
    /// the tree's, or its base.
    pub fn with_curated_dir(
        self,
        origin: impl Into<String>,
        dir: &Path,
    ) -> Result<Rubric, Vec<String>> {
        if !dir.is_dir() {
            return Err(vec![format!("{}: not a directory", dir.display())]);
        }
        self.with_curated_tree(origin, TreeFiles::read(dir))
    }

    /// The rubric with the tuned and trash files of a tree a walk read
    /// (`TreeFiles::read` of a `seasons/` tree on disk) laid as
    /// `with_curated_dir` lays its directory's, in the first curated
    /// place: so a reader that compared the walk's fingerprint with the
    /// one it last laid parses only a tree that changed, and lays exactly
    /// the files it walked. A tree whose root was not there is refused as
    /// no directory; what the walk could not read is an error, as it is to
    /// `with_curated_dir`, beside any the files give.
    pub fn with_curated_tree(
        mut self,
        origin: impl Into<String>,
        tree: TreeFiles,
    ) -> Result<Rubric, Vec<String>> {
        let origin = origin.into();
        let TreeFiles {
            root,
            present,
            files: read,
            errors,
        } = tree;
        let dir = root.as_path();
        if !present {
            return Err(vec![format!("{}: not a directory", dir.display())]);
        }
        let shown = |p: &str| dir.join(p).display().to_string();
        let mut errs = errors;
        // The tuned files, and any file where no rubric file goes (named by
        // `ordered`): every other file of the tree is the base's.
        let tuned = read.into_iter().filter(|(p, _)| {
            matches!(
                FileRole::of(p),
                Ok(Some(
                    FileRole::Encounter { draft: false, .. } | FileRole::Trash { .. }
                )) | Err(_)
            )
        });
        let files = ordered(tuned, shown, &mut errs);
        let source = Source::Curated(Origin::Dir(origin.clone()));
        // The first curated place: the embedded tuned files', or a tree's
        // laid there before.
        let first =
            |s: &Set| matches!(s.source, Source::Curated(Origin::Embedded | Origin::Dir(_)));
        let (at, mut replaced) = match self.sets.iter().position(first) {
            Some(at) => (at, Some(self.sets.remove(at))),
            None => (0, None),
        };
        self.sets.insert(at, Set::new(source.clone()));
        let mut touched = BTreeSet::new();
        for (path, text) in &files {
            match self.place(at, path, &shown(path), text) {
                Ok(placed) => touched.extend(self.touched(&placed)),
                Err(e) => errs.extend(e),
            }
        }
        if !errs.is_empty() {
            return Err(errs);
        }
        // What the replaced files tuned reads otherwise without them (a
        // bundle over them may lean on what they gave), so it is checked
        // too: every encounter of theirs that stays (`kept_of`).
        if let Some(old) = &replaced {
            touched.extend(self.kept_of(old));
        }
        let broken = self.failing(&touched, Tier::Curated);
        if !broken.is_empty() {
            // Only what reads under the set and not over it is the set's:
            // the readings as they were, the replaced files back in place.
            let set = self.sets.remove(at);
            let before = match replaced.take() {
                Some(old) => {
                    self.sets.insert(at, old);
                    let before = self.failing(&touched, Tier::Curated);
                    replaced = Some(self.sets.remove(at));
                    before
                }
                None => self.failing(&touched, Tier::Curated),
            };
            errs = blame(&source.to_string(), broken, &before);
            if !errs.is_empty() {
                return Err(errs);
            }
            self.sets.insert(at, set);
        }
        if let Some(old) = &replaced {
            self.forget_brought(old);
        }
        self.relay_user_over(&touched, &source.to_string());
        // A tree as empty as the files it replaced leaves the version.
        if !files.is_empty() || replaced.as_ref().is_some_and(|s| !s.seasons.is_empty()) {
            self.hash = hash_files(
                self.hash,
                std::iter::once(("\0curated-dir\0", origin.as_str()))
                    .chain(files.iter().map(|(p, t)| (p.as_str(), t.as_str()))),
            );
            self.version = self.versioned();
        }
        Ok(self)
    }

    /// What does not read in the rubric as it stands, the user's files
    /// aside: each encounter and each instance's trash (where its map
    /// answers it) read on every difficulty at the base and at the curated
    /// tier, and every level. Each subject is named once with its first
    /// error and the difficulties it fails on, led by `who`. What
    /// `from_dir` refuses a tree for: the build's tree is held readable by
    /// its test, a tree on disk is checked as it is read, as a curated set
    /// is checked as it is laid.
    pub(crate) fn unreadable(&self, who: &str) -> Vec<String> {
        let mut subjects: BTreeSet<Subject> = self
            .index
            .keys()
            .map(|&id| Subject::Encounter(id))
            .collect();
        for s in &self.seasons {
            for i in s.instances.values() {
                if i.map.is_some_and(|m| {
                    self.instance_by_map(m)
                        .is_some_and(|(a, b)| a.id == s.id && b.slug == i.slug)
                }) {
                    subjects.insert(Subject::Trash {
                        season: s.id.clone(),
                        instance: i.slug.clone(),
                    });
                }
            }
        }
        let mut broken = self.failing(&subjects, Tier::Base);
        for (reading, e) in self.failing(&subjects, Tier::Curated) {
            broken.entry(reading).or_insert(e);
        }
        let mut out = blame(who, broken, &BTreeMap::new());
        for (ui_map, _, _) in self.levels() {
            // Its error names its place (`<season>/<instance>/level.<id>`).
            if let Some(Err(e)) = self.level(ui_map) {
                out.push(format!("{who}: {e}"));
            }
        }
        out
    }

    /// The user's files, where they were laid before a curated set `who`
    /// was laid or replaced, read again over it: a user file was checked
    /// against the curated tier as it stood then, and cannot know what a
    /// set laid after it gives or takes away. An encounter of `touched`
    /// that reads at the curated tier but not at the user's has its user
    /// file at fault: it is taken out and named in `user_errors`, as
    /// `lay_user` leaves one out; so is a user trash file of an instance
    /// whose trash reads so. (A user season or instance file is never taken
    /// out here: it holds defaults alone.)
    fn relay_user_over(&mut self, touched: &BTreeSet<Subject>, who: &str) {
        let Some(user_at) = self
            .sets
            .iter()
            .position(|s| matches!(s.source, Source::User))
        else {
            return;
        };
        let mut out: Vec<(Subject, String)> = Vec::new();
        for ((subject, d), e) in self.failing(touched, Tier::User) {
            if out.iter().any(|(s, _)| *s == subject) {
                continue;
            }
            // Broken under the user's file too: not the file's.
            if self
                .answer_subject(&subject, d, Tier::Curated)
                .is_some_and(|r| r.is_err())
            {
                continue;
            }
            let file = match &subject {
                Subject::Encounter(id) => {
                    let Some((season, instance, stem)) = self
                        .find(*id)
                        .map(|(s, i, l)| (s.id.clone(), i.slug.clone(), l.stem.clone()))
                    else {
                        continue;
                    };
                    let Some(u) = self
                        .sets
                        .get_mut(user_at)
                        .filter(|u| u.has_encounter(&season, &instance, *id))
                    else {
                        continue;
                    };
                    u.instance_mut(&season, &instance).encounters.remove(id);
                    format!("{season}/{instance}/{stem}.toml")
                }
                Subject::Trash { season, instance } => {
                    let Some(u) = self
                        .sets
                        .get_mut(user_at)
                        .filter(|u| u.has_trash(season, instance))
                    else {
                        continue;
                    };
                    u.instance_mut(season, instance).trash = None;
                    format!("{season}/{instance}/trash.toml")
                }
            };
            out.push((subject, format!("{file}: left out over {who}: {e}")));
        }
        if !out.is_empty() {
            self.hash = hash_files(
                self.hash,
                out.iter().map(|(_, e)| ("\0user-left-out\0", e.as_str())),
            );
            self.version = self.versioned();
            self.user_errors.extend(out.into_iter().map(|(_, e)| e));
        }
    }

    /// Whether an encounter a set taken off the stack held stays readable
    /// without it: a draft of the base's, or a set left holding its file.
    fn survives(&self, season: &str, instance: &str, id: u32) -> bool {
        let drafted = self
            .seasons
            .iter()
            .find(|x| x.id == season)
            .and_then(|x| x.instances.get(instance))
            .and_then(|x| x.encounters.get(&id))
            .is_some_and(|l| l.draft.is_some());
        drafted
            || self
                .sets
                .iter()
                .any(|set| set.has_encounter(season, instance, id))
    }

    /// The encounters a set taken off the stack held that stay
    /// (`survives`), and the instances whose trash it held (an instance's
    /// trash always stays, on its creatures): what reads otherwise without
    /// it.
    fn kept_of(&self, gone: &Set) -> BTreeSet<Subject> {
        let mut kept = BTreeSet::new();
        for (season, s) in &gone.seasons {
            for (instance, i) in &s.instances {
                for &id in i.encounters.keys() {
                    if self.survives(season, instance, id) {
                        kept.insert(Subject::Encounter(id));
                    }
                }
                if i.trash.is_some() {
                    kept.insert(Subject::Trash {
                        season: season.clone(),
                        instance: instance.clone(),
                    });
                }
            }
        }
        kept
    }

    /// Take out each encounter a set taken off the stack brought alone: no
    /// draft of the base's, and no set left holding its file.
    fn forget_brought(&mut self, gone: &Set) {
        for (season, s) in &gone.seasons {
            for (instance, i) in &s.instances {
                for &id in i.encounters.keys() {
                    if !self.survives(season, instance, id) {
                        self.forget_encounter(season, instance, id);
                    }
                }
            }
        }
    }

    /// The rubric with the user's own files laid over everything
    /// (`Source::User`), paths relative to the user's rubric directory as
    /// to `seasons/`. A file that cannot be laid, or that would make an
    /// encounter unreadable, is named in `user_errors` and left out; the
    /// rest are laid.
    pub fn with_user(self, files: impl IntoIterator<Item = (String, String)>) -> Rubric {
        self.lay_user(files, |p| p.to_string())
    }

    /// The rubric with the user's files under `root` laid over everything,
    /// as `with_user` lays them: every `.toml` under it, read once, here.
    /// `None` reads the user's rubric directory (`user_dir`). A directory
    /// that is not there lays nothing; a file that cannot be read is named
    /// in `user_errors`, by its place on disk, and left out.
    pub fn with_user_dir(self, root: Option<PathBuf>) -> Rubric {
        let Some(root) = root.or_else(user_dir) else {
            return self;
        };
        self.with_user_tree(TreeFiles::read(&root))
    }

    /// The rubric with the user's files a walk read (`TreeFiles::read` of
    /// their directory) laid over everything, as `with_user_dir` lays
    /// them: what could not be read is named in `user_errors`.
    pub fn with_user_tree(mut self, tree: TreeFiles) -> Rubric {
        self.user_errors.extend(tree.errors);
        let root = tree.root;
        self.lay_user(tree.files, |p| root.join(p).display().to_string())
    }

    /// The user's files `with_user` and `with_user_dir` found and could not
    /// lay, each naming its file; the rest of the rubric is laid as if they
    /// were not there.
    pub fn user_errors(&self) -> &[String] {
        &self.user_errors
    }

    /// Lay the user's files, one by one, as a new user set; `shown` names
    /// a file in what it says.
    fn lay_user(
        mut self,
        files: impl IntoIterator<Item = (String, String)>,
        shown: impl Fn(&str) -> String,
    ) -> Rubric {
        self.sets.push(Set::new(Source::User));
        let at = self.sets.len() - 1;
        let mut errs = Vec::new();
        let files = ordered(files, &shown, &mut errs);
        let mut laid = Vec::new();
        for (path, text) in files {
            let name = shown(&path);
            let placed = match self.place(at, &path, &name, &text) {
                Ok(placed) => placed,
                Err(e) => {
                    errs.extend(e);
                    continue;
                }
            };
            let touched = self.touched(&placed);
            let broken = self.failing(&touched, Tier::User);
            if !broken.is_empty() {
                self.unplace(at, &placed);
                let before = self.failing(&touched, Tier::User);
                let new = blame(&format!("{name}: left out"), broken, &before);
                if !new.is_empty() {
                    errs.extend(new);
                    continue;
                }
                // What it broke was broken without it: it is laid.
                if let Err(e) = self.place(at, &path, &name, &text) {
                    errs.extend(e);
                    continue;
                }
            }
            laid.push((path, text));
        }
        self.user_errors.extend(errs);
        // No files (no user directory) leaves the version as it was.
        if !laid.is_empty() {
            self.hash = hash_files(
                self.hash,
                std::iter::once(("\0user\0", ""))
                    .chain(laid.iter().map(|(p, t)| (p.as_str(), t.as_str()))),
            );
            self.version = self.versioned();
        }
        self
    }

    /// Lay one file of a set over the base: checked against the base, then
    /// placed in the set at `at`. `name` is what an error calls the file.
    fn place(
        &mut self,
        at: usize,
        path: &str,
        name: &str,
        text: &str,
    ) -> Result<Placed, Vec<String>> {
        let one = |e: String| vec![format!("{name}: {e}")];
        let tier = self
            .sets
            .get(at)
            .map_or(Tier::User, |s| s.source.tier())
            .name();
        let Some(role) = FileRole::of(path).map_err(one)? else {
            return Err(one("not where a rubric file goes".into()));
        };
        let mut errs = Vec::new();
        let Some(mut t) = parse(name, text, &mut errs) else {
            return Err(errs);
        };
        let base_season = |r: &Rubric, season: &str| r.seasons.iter().position(|s| s.id == season);
        match role {
            FileRole::Season { season } => {
                if base_season(self, season).is_none() {
                    return Err(one(format!("the base has no season {season}")));
                }
                let defaults = defaults_alone(&mut t, &[], tier).map_err(one)?;
                let set = self.sets.get_mut(at).ok_or_else(|| one("no set".into()))?;
                set.seasons.entry(season.to_string()).or_default().defaults = defaults;
                Ok(Placed::Season {
                    season: season.to_string(),
                })
            }
            FileRole::Instance { season, instance } => {
                let Some(inst) = base_season(self, season)
                    .and_then(|n| self.seasons.get(n))
                    .and_then(|s| s.instances.get(instance))
                else {
                    return Err(one(format!("the base has no {season}/{instance}")));
                };
                if t.contains_key("level") {
                    return Err(one(format!(
                        "`level`: a level is the base's alone; a {tier} layer gives none"
                    )));
                }
                let encounter = match t.remove("encounter") {
                    Some(Value::Table(e)) => e,
                    None => Table::new(),
                    Some(_) => {
                        return Err(one(
                            "`encounter` is a table of encounters' maps and NPCs, by DungeonEncounterID"
                                .into(),
                        ));
                    }
                };
                let defaults = defaults_alone(&mut t, &["encounter"], tier).map_err(one)?;
                let drawings = Drawing::read(name, season, instance, encounter, &mut errs);
                for d in &drawings {
                    if !inst.encounters.contains_key(&d.id) {
                        errs.push(format!(
                            "{name}: [encounter.{id}]: the instance has no files for encounter {id}",
                            id = d.id
                        ));
                    } else if let Some(e) = self.answered_elsewhere(d.id, season, instance) {
                        errs.push(format!("{name}: [encounter.{}]: {e}", d.id));
                    }
                }
                if !errs.is_empty() {
                    return Err(errs);
                }
                let drawn: Vec<u32> = drawings.iter().map(|d| d.id).collect();
                let set = self.sets.get_mut(at).ok_or_else(|| one("no set".into()))?;
                let files = set.instance_mut(season, instance);
                files.defaults = defaults;
                for d in drawings {
                    files.drawings.insert(d.id, d.map);
                }
                Ok(Placed::Instance {
                    season: season.to_string(),
                    instance: instance.to_string(),
                    drawn,
                })
            }
            FileRole::Encounter {
                season,
                instance,
                id,
                stem,
                draft,
            } => {
                if draft {
                    return Err(one(format!(
                        "a draft is the base's; a {tier} layer gives `{stem}.toml`"
                    )));
                }
                let Some(n) = base_season(self, season) else {
                    return Err(one(format!("the base has no {season}/{instance}")));
                };
                let Some(inst) = self
                    .seasons
                    .get_mut(n)
                    .and_then(|s| s.instances.get_mut(instance))
                else {
                    return Err(one(format!("the base has no {season}/{instance}")));
                };
                let new = match inst.encounters.get(&id) {
                    Some(l) if l.stem != stem => {
                        return Err(one(format!(
                            "encounter {id} already has files named {}",
                            l.stem
                        )));
                    }
                    Some(_) => false,
                    None => {
                        inst.encounters.insert(
                            id,
                            Layers {
                                stem: stem.to_string(),
                                ..Layers::default()
                            },
                        );
                        true
                    }
                };
                if new {
                    match index(&self.seasons) {
                        Ok(i) => self.index = i,
                        Err(e) => {
                            self.forget_encounter(season, instance, id);
                            return Err(e.into_iter().map(|e| format!("{name}: {e}")).collect());
                        }
                    }
                }
                // A file where no reader looks (an older season's copy of a
                // returning dungeon) would be laid and never read.
                if let Some(e) = self.answered_elsewhere(id, season, instance) {
                    if new {
                        self.forget_encounter(season, instance, id);
                    }
                    return Err(one(e));
                }
                let set = self.sets.get_mut(at).ok_or_else(|| one("no set".into()))?;
                set.instance_mut(season, instance).encounters.insert(id, t);
                Ok(Placed::Encounter {
                    season: season.to_string(),
                    instance: instance.to_string(),
                    id,
                    new,
                })
            }
            FileRole::Trash { season, instance } => {
                let Some(inst) = base_season(self, season)
                    .and_then(|n| self.seasons.get(n))
                    .and_then(|s| s.instances.get(instance))
                else {
                    return Err(one(format!("the base has no {season}/{instance}")));
                };
                trash_keys(&t, &inst.written).map_err(one)?;
                // A file where no reader looks (an older season's copy of a
                // returning dungeon) would be laid and never read.
                if let Some(e) = self.trash_elsewhere(season, instance) {
                    return Err(one(e));
                }
                let set = self.sets.get_mut(at).ok_or_else(|| one("no set".into()))?;
                set.instance_mut(season, instance).trash = Some(t);
                Ok(Placed::Trash {
                    season: season.to_string(),
                    instance: instance.to_string(),
                })
            }
        }
    }

    /// Take a placed file back out of the set at `at`.
    fn unplace(&mut self, at: usize, placed: &Placed) {
        let Some(set) = self.sets.get_mut(at) else {
            return;
        };
        match placed {
            Placed::Season { season } => {
                if let Some(s) = set.seasons.get_mut(season) {
                    s.defaults = None;
                }
            }
            Placed::Instance {
                season,
                instance,
                drawn,
            } => {
                let files = set.instance_mut(season, instance);
                files.defaults = None;
                for id in drawn {
                    files.drawings.remove(id);
                }
            }
            Placed::Encounter {
                season,
                instance,
                id,
                new,
            } => {
                set.instance_mut(season, instance).encounters.remove(id);
                if *new {
                    self.forget_encounter(season, instance, *id);
                }
            }
            Placed::Trash { season, instance } => {
                set.instance_mut(season, instance).trash = None;
            }
        }
    }

    /// Why a layer's file for encounter `id` under `season`/`instance` would
    /// never be read: the index answers the encounter from elsewhere (a
    /// newer season holding a returning dungeon), or not at all. `None`
    /// when it is answered from there.
    fn answered_elsewhere(&self, id: u32, season: &str, instance: &str) -> Option<String> {
        let at = self
            .index
            .get(&id)
            .and_then(|(n, slug)| Some((self.seasons.get(*n)?.id.as_str(), slug.as_str())));
        match at {
            Some((s, i)) if s == season && i == instance => None,
            Some((s, i)) => Some(format!(
                "encounter {id} is answered from {s}/{i}; a layer's file for it goes there"
            )),
            None => Some(format!("no season answers encounter {id}")),
        }
    }

    /// Take an encounter a set brought back out of its instance and the
    /// index.
    fn forget_encounter(&mut self, season: &str, instance: &str, id: u32) {
        if let Some(inst) = self
            .seasons
            .iter_mut()
            .find(|s| s.id == season)
            .and_then(|s| s.instances.get_mut(instance))
        {
            inst.encounters.remove(&id);
        }
        if let Ok(i) = index(&self.seasons) {
            self.index = i;
        }
    }

    /// What a placed file is read by: the encounters, as the index answers
    /// them, or its instance's trash.
    fn touched(&self, placed: &Placed) -> BTreeSet<Subject> {
        let at = |season: &str, instance: Option<&str>| -> BTreeSet<Subject> {
            self.index
                .iter()
                .filter(|(_, (n, slug))| {
                    self.seasons.get(*n).is_some_and(|s| s.id == season)
                        && instance.is_none_or(|i| i == slug)
                })
                .map(|(&id, _)| Subject::Encounter(id))
                .collect()
        };
        match placed {
            Placed::Season { season } => at(season, None),
            Placed::Instance {
                season, instance, ..
            } => at(season, Some(instance)),
            Placed::Encounter {
                season,
                instance,
                id,
                ..
            } => at(season, Some(instance))
                .into_iter()
                .filter(|s| *s == Subject::Encounter(*id))
                .collect(),
            Placed::Trash { season, instance } => BTreeSet::from([Subject::Trash {
                season: season.clone(),
                instance: instance.clone(),
            }]),
        }
    }

    /// Every reading of `subjects` at `tier`, uncapped, on every
    /// difficulty, that fails, with its error.
    fn failing(&self, subjects: &BTreeSet<Subject>, tier: Tier) -> BTreeMap<Reading, String> {
        let mut out = BTreeMap::new();
        for subject in subjects {
            for d in std::iter::once(None).chain(difficulty::probes().map(Some)) {
                if let Some(Err(e)) = self.answer_subject(subject, d, tier) {
                    out.insert((subject.clone(), d), e);
                }
            }
        }
        out
    }

    /// A subject on one difficulty at `tier`, uncapped: `None` when it
    /// does not stand there.
    fn answer_subject(
        &self,
        subject: &Subject,
        difficulty: Option<u32>,
        tier: Tier,
    ) -> Option<Result<crate::Encounter, String>> {
        match subject {
            Subject::Encounter(id) => self.answer(*id, difficulty, tier, None),
            Subject::Trash { season, instance } => {
                self.answer_trash(season, instance, difficulty, tier)
            }
        }
    }
}

/// What a set broke: each subject `broken` reads unreadable that `before`
/// (the same readings without the set) did not, once, with the first
/// error and the difficulties it fails on.
fn blame(
    who: &str,
    broken: BTreeMap<Reading, String>,
    before: &BTreeMap<Reading, String>,
) -> Vec<String> {
    let mut by: BTreeMap<Subject, (String, Vec<&'static str>)> = BTreeMap::new();
    for (reading, e) in broken {
        if before.contains_key(&reading) {
            continue;
        }
        let (subject, d) = reading;
        let on = d
            .and_then(|d| difficulty::chain(d).first().copied())
            .unwrap_or("its own reading");
        by.entry(subject)
            .or_insert_with(|| (e, Vec::new()))
            .1
            .push(on);
    }
    let every = 1 + difficulty::probes().count();
    by.into_iter()
        .map(|(subject, (e, on))| {
            let on = if on.len() == every {
                "every difficulty".to_string()
            } else {
                on.join(", ")
            };
            format!("{who}: {subject} (on {on}): {}", e.trim_end())
        })
        .collect()
}

/// A layer's season or instance file's `[defaults]`, its format number
/// (checked by `migrate`) taken out, refusing any key but those and `also`,
/// and any default no encounter has (`defaults_keys`).
fn defaults_alone(t: &mut Table, also: &[&str], tier: &str) -> Result<Option<Table>, String> {
    t.remove("schema");
    if let Some(k) = t
        .keys()
        .find(|k| k.as_str() != "defaults" && !also.contains(&k.as_str()))
    {
        let gives = if also.is_empty() {
            "`defaults`".to_string()
        } else {
            format!("`defaults` and `{}`", also.join("`, `"))
        };
        return Err(format!(
            "`{k}`: a {tier} layer's file gives only {gives}; what a season or an instance is stays the base's"
        ));
    }
    match t.remove("defaults") {
        Some(Value::Table(d)) => {
            crate::defaults_keys(&d)?;
            Ok(Some(d))
        }
        None => Ok(None),
        Some(_) => Err("`defaults` is a table".into()),
    }
}

/// A set's files in the order they are laid, each once: seasons, then
/// encounters and trash, then instances (whose drawings must find their
/// encounters' files), each by path. A file beside an encounter's that is no TOML is
/// passed over; one given twice, or anywhere a rubric file does not go,
/// is an error naming it as `shown` does.
fn ordered(
    files: impl IntoIterator<Item = (String, String)>,
    shown: impl Fn(&str) -> String,
    errs: &mut Vec<String>,
) -> Vec<(String, String)> {
    let mut files: Vec<(String, String)> = files.into_iter().collect();
    files.sort();
    let mut out: Vec<(u8, String, String)> = Vec::new();
    let mut last: Option<String> = None;
    for (path, text) in files {
        if last.as_deref() == Some(path.as_str()) {
            errs.push(format!("{}: given twice", shown(&path)));
            continue;
        }
        last = Some(path.clone());
        let rank = match FileRole::of(&path) {
            Ok(Some(FileRole::Season { .. })) => 0,
            Ok(Some(FileRole::Encounter { .. } | FileRole::Trash { .. })) => 1,
            Ok(Some(FileRole::Instance { .. })) => 2,
            Ok(None) => continue,
            Err(e) => {
                errs.push(format!("{}: {e}", shown(&path)));
                continue;
            }
        };
        out.push((rank, path, text));
    }
    out.sort();
    out.into_iter().map(|(_, p, t)| (p, t)).collect()
}

/// Every `.toml` under `dir`, its path relative to the tree's root
/// (`rel`, `/`-joined) and its text. A root that is not there is no
/// error; anything else that cannot be read is, by its place on disk. A
/// stray entry is passed over, never an error: a dot-file or directory
/// (an editor's `.#instance.toml` lock, a swap file), a link to nothing, a
/// file gone since its directory was listed, and a name that is not UTF-8
/// unless it names a `.toml` file.
pub(crate) fn read_tree(
    dir: &Path,
    rel: &str,
    depth: usize,
    out: &mut Vec<(String, String)>,
    errs: &mut Vec<String>,
) {
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(e) if depth == 0 && e.kind() == std::io::ErrorKind::NotFound => return,
        Err(e) => {
            errs.push(format!("{}: {e}", dir.display()));
            return;
        }
    };
    for entry in entries {
        let entry = match entry {
            Ok(e) => e,
            Err(e) => {
                errs.push(format!("{}: {e}", dir.display()));
                continue;
            }
        };
        let path = entry.path();
        let raw = entry.file_name();
        if raw.as_encoded_bytes().first() == Some(&b'.') {
            continue;
        }
        let toml = path.extension().is_some_and(|x| x == "toml");
        let Some(name) = raw.to_str().map(str::to_string) else {
            if toml && !path.is_dir() {
                errs.push(format!("{}: not a UTF-8 name", path.display()));
            }
            continue;
        };
        let rel = if rel.is_empty() {
            name
        } else {
            format!("{rel}/{name}")
        };
        if path.is_dir() {
            if depth < USER_DEPTH {
                read_tree(&path, &rel, depth + 1, out, errs);
            } else {
                errs.push(format!("{}: too deep", path.display()));
            }
        } else if toml {
            match std::fs::read_to_string(&path) {
                Ok(text) => out.push((rel, text)),
                // A link to nothing, or a file renamed away mid-walk (a
                // save written beside and moved over): not there.
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => errs.push(format!("{}: {e}", path.display())),
            }
        }
    }
}
