//! The journal's text for an encounter's abilities. Blizzard's words stay
//! out of the repository, like the extracted art: `gen-rubric` renders them
//! into a per-machine sidecar beside the rubric, never into a draft, and a
//! reader lays the sidecar over an encounter at runtime
//! (`Rubric::with_texts`). A machine without the sidecars reads every
//! ability with no `text`.
//!
//! ```text
//! $XDG_DATA_HOME/wowdps/rubric-text/<season>/<instance>/<id>-<slug>.toml
//! ```
//!
//! A sidecar is shaped like a draft's abilities, `text` alone:
//! `[ability.<slug>]` for the text where the ability first appears, and
//! `[difficulty.<name>.ability.<slug>]` where a difficulty reads otherwise,
//! resolved through the same fallback chain as the rubric.

use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::Deserialize;
use serde::de::IgnoredAny;
use toml::{Table, Value};

use crate::{Encounter, difficulty};

/// An encounter's journal text, by ability key: `ability` where each
/// ability first appears, and the whole reading on each difficulty (by
/// name); a difficulty not here reads `ability`. What `write::text_file`
/// writes and `read` reads.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Texts {
    pub ability: BTreeMap<String, String>,
    pub difficulty: BTreeMap<String, BTreeMap<String, String>>,
}

impl Texts {
    /// The text as `difficulty` (the log's difficulty id; `None`, the
    /// base) reads it, by ability key.
    pub fn reading(&self, difficulty: Option<u32>) -> &BTreeMap<String, String> {
        difficulty
            .and_then(|id| difficulty::chain(id).first())
            .and_then(|name| self.difficulty.get(*name))
            .unwrap_or(&self.ability)
    }

    /// Lay the text over an encounter as `difficulty` reads it. An ability
    /// the encounter has takes its text unless it already has one; one it
    /// lacks (dropped by its `only`, or renamed since) is passed over.
    pub fn lay(&self, e: &mut Encounter, difficulty: Option<u32>) {
        let texts = self.reading(difficulty);
        for (key, a) in &mut e.ability {
            if a.text.is_none()
                && let Some(text) = texts.get(key)
            {
                a.text = Some(text.clone());
            }
        }
    }
}

/// Where the sidecars live on this machine: `rubric-text` under the data
/// home every per-machine cache resolves through (`wowdps_proto::dirs`:
/// `$XDG_DATA_HOME/wowdps`, else `~/.local/share/wowdps`, an empty or
/// relative `$XDG_DATA_HOME` read as unset). None without either.
pub fn dir() -> Option<PathBuf> {
    wowdps_proto::dirs::data_path("rubric-text")
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct File {
    #[serde(rename = "schema")]
    _schema: IgnoredAny,
    #[serde(default)]
    ability: BTreeMap<String, Entry>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Entry {
    text: String,
}

/// A sidecar's text, whole: the reading where each ability first appears,
/// and each difficulty's that reads otherwise, through its fallback chain.
/// A sidecar any difficulty cannot read is an error.
pub fn read(text: &str) -> Result<Texts, String> {
    let t = crate::migrate(text.parse::<Table>().map_err(|e| e.to_string())?)?;
    let reading = |chain: &[&str]| -> Result<BTreeMap<String, String>, String> {
        let mut t = t.clone();
        crate::lay_difficulties(&mut t, chain)?;
        let f = File::deserialize(Value::Table(t)).map_err(|e| e.to_string())?;
        Ok(f.ability.into_iter().map(|(k, a)| (k, a.text)).collect())
    };
    let ability = reading(&[])?;
    let mut by = BTreeMap::new();
    for name in difficulty::NAMES {
        let r = reading(difficulty::chain_named(name))?;
        if r != ability {
            by.insert(name.to_string(), r);
        }
    }
    Ok(Texts {
        ability,
        difficulty: by,
    })
}

/// Lay a sidecar's text over an encounter as `difficulty` (the log's
/// difficulty id; `None`, the base) reads it: `read`, then `Texts::lay`.
pub fn lay(text: &str, e: &mut Encounter, difficulty: Option<u32>) -> Result<(), String> {
    read(text)?.lay(e, difficulty);
    Ok(())
}
