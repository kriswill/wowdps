//! The layer stack an encounter resolves through, and who set what in it.
//!
//! An encounter is laid together from SOURCES, each over the last: the
//! base's four (the season's defaults, the instance's, the generated
//! draft, the instance's drawing of the encounter), then every curated
//! file set in the order it was laid (a tuned file embedded beside its
//! draft first, then each bundle `Rubric::with_curated` laid in), then the
//! user's own files (`Rubric::with_user`, `with_user_dir`). Each source
//! names the `Tier` it belongs to, so a reader capped at a tier reads the
//! sources up to it alone.
//!
//! A curated or user set is a file tree shaped like `seasons/`, laid in as
//! one block: its `season.toml` (`[defaults]` alone), its `instance.toml`
//! (`[defaults]` and the `[encounter.<id>]` drawing tables), then its
//! `<id>-<slug>.toml`, each over the last, and the whole block over
//! everything below it. A draft is the base's: no set may hold one.
//!
//! The merge is the rubric's one rule (tables key by key, anything else
//! replaced whole), here with a `Provenance` kept beside it when asked:
//! per key path, the source that last set it, and the `[difficulty.<name>]`
//! override it came through, if any.

use std::collections::BTreeMap;
use std::fmt;

use toml::{Table, Value};

use crate::Tier;

/// Where a curated file set came from.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Origin {
    /// A tuned `<id>-<slug>.toml` beside its draft in the tree the binary
    /// was built with (`Rubric::embedded`, `Rubric::from_files`).
    Embedded,
    /// A named file set laid in at runtime (`Rubric::with_curated`).
    Bundle(String),
}

/// What laid a key of a resolved encounter: a file of the base, a curated
/// set, or the user's own. Ordered as the stack lays them, the base's
/// first; two bundles order by name, though the stack lays them in the
/// order they were laid in.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Source {
    /// The season's `season.toml`, its `[defaults]`.
    Season,
    /// The instance's `instance.toml`, its `[defaults]`.
    Instance,
    /// The generated `<id>-<slug>.draft.toml`.
    Draft,
    /// The instance file's `[encounter.<id>]`: the encounter's map, its
    /// view, what changes the map and an NPC the draft lacks or has wrong.
    Drawing,
    /// A curated file set: the embedded tuned file, or a bundle.
    Curated(Origin),
    /// The user's own files on this machine.
    User,
}

impl Source {
    /// The tier a reader must reach to read this source.
    pub fn tier(&self) -> Tier {
        match self {
            Source::Season | Source::Instance | Source::Draft | Source::Drawing => Tier::Base,
            Source::Curated(_) => Tier::Curated,
            Source::User => Tier::User,
        }
    }
}

impl fmt::Display for Source {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Source::Season => f.write_str("season"),
            Source::Instance => f.write_str("instance"),
            Source::Draft => f.write_str("draft"),
            Source::Drawing => f.write_str("drawing"),
            Source::Curated(Origin::Embedded) => f.write_str("curated"),
            Source::Curated(Origin::Bundle(name)) => write!(f, "curated {name:?}"),
            Source::User => f.write_str("user"),
        }
    }
}

/// Who set one key of a resolved encounter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SetBy {
    pub source: Source,
    /// The difficulty whose `[difficulty.<name>]` override of that source
    /// set it; `None` for the source's own value.
    pub difficulty: Option<&'static str>,
}

/// Who set each key of a resolved encounter, by its path: the table keys
/// from the encounter's top joined by `.` (`ability.serpents-bite.shape`,
/// `npc.venomous-heart.shares`, `map.stencil`). A path names a value that
/// was laid whole: a number, a string, a list (lists replace whole, so
/// `map.stencil` is one path, never its shapes), or an empty table. A
/// table merged key by key is no path of its own; its keys are.
///
/// It is the provenance of the table as laid, before entries are dropped
/// by `enabled` or `only`: a switched-off entry keeps its keys, its
/// `enabled` naming who switched it off. The journal's text
/// (`Rubric::with_texts`) is laid after and is not in it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Provenance {
    keys: BTreeMap<String, SetBy>,
}

impl Provenance {
    /// Who set the value at `path`, or the value that holds it: a key
    /// inside a list or a whole-laid table answers for its holder
    /// (`map.stencil.0` is `map.stencil`'s). `None` for a path no source
    /// set, or a table merged key by key (ask for its keys).
    pub fn source_of(&self, path: &str) -> Option<&SetBy> {
        let mut at = path;
        loop {
            if let Some(by) = self.keys.get(at) {
                return Some(by);
            }
            at = at.rsplit_once('.')?.0;
        }
    }

    /// Every path and who set it, in path order.
    pub fn iter(&self) -> impl Iterator<Item = (&str, &SetBy)> {
        self.keys.iter().map(|(k, v)| (k.as_str(), v))
    }

    /// Every path at or under `prefix` (`ability.bite`: the ability's
    /// keys) and who set it, in path order.
    pub fn under<'a>(&'a self, prefix: &'a str) -> impl Iterator<Item = (&'a str, &'a SetBy)> {
        self.iter().filter(move |(k, _)| {
            k.strip_prefix(prefix)
                .is_some_and(|rest| rest.is_empty() || rest.starts_with('.'))
        })
    }

    /// How many paths it holds.
    pub fn len(&self) -> usize {
        self.keys.len()
    }

    /// Whether it holds none.
    pub fn is_empty(&self) -> bool {
        self.keys.is_empty()
    }

    /// Forget `path` and every path under it: a value laid whole over it.
    fn forget(&mut self, path: &str) {
        self.keys.remove(path);
        let under: Vec<String> = self
            .keys
            .range(format!("{path}.")..format!("{path}/"))
            .map(|(k, _)| k.clone())
            .collect();
        for k in under {
            self.keys.remove(&k);
        }
    }

    /// Record `v`, laid whole at `path`: each of a table's keys, a leaf
    /// for anything else (an empty table among them).
    fn record(&mut self, path: &str, v: &Value, by: &SetBy) {
        match v {
            Value::Table(t) if !t.is_empty() => {
                for (k, v) in t {
                    self.record(&join(path, k), v, by);
                }
            }
            _ => {
                self.keys.insert(path.to_string(), by.clone());
            }
        }
    }

    /// Record who set `path` where the resolution fills it in itself (the
    /// encounter's `instance`, from the instance it lies in).
    pub(crate) fn set(&mut self, path: &str, by: SetBy) {
        self.keys.insert(path.to_string(), by);
    }
}

fn join(at: &str, k: &str) -> String {
    if at.is_empty() {
        k.to_string()
    } else {
        format!("{at}.{k}")
    }
}

/// Lay `over` onto `base` by the rubric's rule (tables merge key by key,
/// anything else replaces), recording in the provenance, when one is
/// kept, who set each path. `at` is the path `base` lies at.
pub(crate) fn lay(
    base: &mut Table,
    over: Table,
    at: &str,
    mut trace: Option<(&mut Provenance, &SetBy)>,
) {
    for (k, v) in over {
        let path = join(at, &k);
        match (base.get_mut(&k), v) {
            (Some(Value::Table(b)), Value::Table(o)) => {
                if o.is_empty() {
                    continue;
                }
                // A table that was laid empty is a table merged into now:
                // its keys answer for it.
                if let Some((p, _)) = trace.as_mut() {
                    p.keys.remove(&path);
                }
                lay(b, o, &path, trace.as_mut().map(|(p, by)| (&mut **p, *by)));
            }
            (_, v) => {
                if let Some((p, by)) = trace.as_mut() {
                    p.forget(&path);
                    p.record(&path, &v, by);
                }
                base.insert(k, v);
            }
        }
    }
}

/// What one instance's files of a set give: its `[defaults]`, its
/// `[encounter.<id>]` drawings, and its encounters' files.
#[derive(Debug, Clone, Default)]
pub(crate) struct InstanceFiles {
    pub defaults: Option<Table>,
    pub drawings: BTreeMap<u32, Table>,
    pub encounters: BTreeMap<u32, Table>,
}

/// What one season's files of a set give: its `[defaults]` and its
/// instances'.
#[derive(Debug, Clone, Default)]
pub(crate) struct SeasonFiles {
    pub defaults: Option<Table>,
    pub instances: BTreeMap<String, InstanceFiles>,
}

/// A file set laid over the base: one source's files, by season and
/// instance.
#[derive(Debug, Clone)]
pub(crate) struct Set {
    pub source: Source,
    pub seasons: BTreeMap<String, SeasonFiles>,
}

impl Set {
    pub fn new(source: Source) -> Set {
        Set {
            source,
            seasons: BTreeMap::new(),
        }
    }

    pub fn season(&self, season: &str) -> Option<&SeasonFiles> {
        self.seasons.get(season)
    }

    pub fn instance(&self, season: &str, instance: &str) -> Option<&InstanceFiles> {
        self.season(season)?.instances.get(instance)
    }

    pub fn instance_mut(&mut self, season: &str, instance: &str) -> &mut InstanceFiles {
        self.seasons
            .entry(season.to_string())
            .or_default()
            .instances
            .entry(instance.to_string())
            .or_default()
    }

    /// Whether the set holds the encounter's own file.
    pub fn has_encounter(&self, season: &str, instance: &str, id: u32) -> bool {
        self.instance(season, instance)
            .is_some_and(|i| i.encounters.contains_key(&id))
    }

    /// Whether any file of the set is read by the encounter: its own, its
    /// drawing, or its instance's or season's.
    pub fn touches(&self, season: &str, instance: &str, id: u32) -> bool {
        let Some(s) = self.season(season) else {
            return false;
        };
        s.defaults.is_some()
            || s.instances.get(instance).is_some_and(|i| {
                i.defaults.is_some()
                    || i.drawings.contains_key(&id)
                    || i.encounters.contains_key(&id)
            })
    }
}

/// What one file of a laid set put where, so a user file that turns out
/// to break an encounter can be taken back out.
#[derive(Debug, Clone)]
pub(crate) enum Placed {
    Season {
        season: String,
    },
    Instance {
        season: String,
        instance: String,
        /// The encounters its `[encounter.<id>]` drew.
        drawn: Vec<u32>,
    },
    Encounter {
        season: String,
        instance: String,
        id: u32,
        /// Whether the file brought an encounter the rubric had no files
        /// for, so taking it out takes the encounter out too.
        new: bool,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    fn by(source: Source) -> SetBy {
        SetBy {
            source,
            difficulty: None,
        }
    }

    #[test]
    fn a_value_laid_whole_forgets_what_was_under_it() {
        let mut t = Table::new();
        let mut p = Provenance::default();
        let first: Table = "[a]\nx = 1\ny = [1, 2]\n[a.inner]\nz = 2\n"
            .parse()
            .unwrap();
        lay(&mut t, first, "", Some((&mut p, &by(Source::Draft))));
        let paths: Vec<&str> = p.iter().map(|(k, _)| k).collect();
        assert_eq!(paths, ["a.inner.z", "a.x", "a.y"]);
        // A list is one path, and a key inside it answers for the list.
        assert_eq!(p.source_of("a.y.0"), p.source_of("a.y"));
        // A table merged key by key keeps what it does not name.
        let second: Table = "[a]\nx = 3\n[a.inner]\nw = 4\n".parse().unwrap();
        lay(&mut t, second, "", Some((&mut p, &by(Source::User))));
        assert_eq!(p.source_of("a.x").unwrap().source, Source::User);
        assert_eq!(p.source_of("a.inner.z").unwrap().source, Source::Draft);
        assert_eq!(p.source_of("a.inner.w").unwrap().source, Source::User);
        assert!(p.source_of("a").is_none());
        // A number over a table forgets the table's keys, and a table over
        // a number records its own.
        let third: Table = "a = { inner = 5, x = { q = 1 } }\n".parse().unwrap();
        lay(&mut t, third, "", Some((&mut p, &by(Source::Drawing))));
        let paths: Vec<&str> = p.iter().map(|(k, _)| k).collect();
        assert_eq!(paths, ["a.inner", "a.x.q", "a.y"]);
        assert_eq!(p.under("a.x").count(), 1);
        assert_eq!(p.under("a").count(), 3);
        // An empty table is a path until something merges into it.
        let fourth: Table = "[b]\n".parse().unwrap();
        lay(&mut t, fourth, "", Some((&mut p, &by(Source::Season))));
        assert_eq!(p.source_of("b").unwrap().source, Source::Season);
        let fifth: Table = "[b]\nk = 1\n".parse().unwrap();
        lay(&mut t, fifth, "", Some((&mut p, &by(Source::Instance))));
        assert!(!p.keys.contains_key("b"));
        assert_eq!(p.source_of("b.k").unwrap().source, Source::Instance);
    }

    #[test]
    fn sources_order_as_the_stack_lays_them_and_name_their_tier() {
        let stack = [
            Source::Season,
            Source::Instance,
            Source::Draft,
            Source::Drawing,
            Source::Curated(Origin::Embedded),
            Source::Curated(Origin::Bundle("a".into())),
            Source::User,
        ];
        assert!(stack.windows(2).all(|w| w[0] < w[1]));
        let tiers: Vec<Tier> = stack.iter().map(Source::tier).collect();
        assert_eq!(
            tiers,
            [
                Tier::Base,
                Tier::Base,
                Tier::Base,
                Tier::Base,
                Tier::Curated,
                Tier::Curated,
                Tier::User
            ]
        );
        assert_eq!(
            Source::Curated(Origin::Bundle("pack".into())).to_string(),
            "curated \"pack\""
        );
    }
}
