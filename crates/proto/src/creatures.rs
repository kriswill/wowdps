//! `creatures.tsv` — the history store's table of NPC classifications, the
//! nameplate facts the combat log never carries: whether a creature is
//! elite, rare, a world boss, a minor `minus` mob or a lieutenant. The
//! wowdps addon records them from the game's API by creature id (its
//! `WOWDPS_DATA.creatures`, flushed on logout like the rest of its table);
//! the daemon's history thread merges every account's records into
//! `<history dir>/creatures.tsv`, and the replay reads that file to color
//! its rings the way nameplates do.
//!
//! The format is a contract (CONTRACT.md, the addon paragraph), UTF-8,
//! one header line then one row per creature id, ascending:
//!
//! ```text
//! # wowdps creatures 1
//! creature_id<TAB>classification<TAB>lieutenant<TAB>seen_unix<TAB>name
//! ```
//!
//! `classification` is one of [`CLASSIFICATIONS`], `lieutenant` is `0` or
//! `1`, `seen_unix` the whole seconds UTC the addon last saw the creature,
//! and `name` the NPC's name (never a player's: the addon records units
//! whose GUID is a `Creature-` or `Vehicle-` one, never player-controlled).
//! Readers skip a `#` line after the header and a row they cannot read,
//! and ignore any column past the fifth.

use std::collections::BTreeMap;

use crate::history::addon_table;
use crate::lua::{Key, Lua};

/// The file's name at the root of the history store.
pub const FILE: &str = "creatures.tsv";

/// The format this build writes, named by the header line.
pub const FORMAT: u32 = 1;

/// The header's text before the format number.
const HEADER_PREFIX: &str = "# wowdps creatures ";

/// What `UnitClassification` answers, as the game spells it. Nothing else
/// is ever written.
pub const CLASSIFICATIONS: [&str; 6] =
    ["normal", "elite", "rareelite", "rare", "worldboss", "minus"];

/// One creature id's row: what the addon saw last.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Creature {
    /// The NPC id: the sixth dash-separated field of a `Creature-` GUID,
    /// as the combat log writes it.
    pub id: u32,
    /// One of [`CLASSIFICATIONS`].
    pub classification: String,
    /// `UnitIsLieutenant` (12.x): the silver-dragon tier between a pack's
    /// trash and its boss.
    pub lieutenant: bool,
    /// Whole seconds UTC (the addon's `GetServerTime()`).
    pub seen_unix: i64,
    pub name: String,
}

/// `header` names a format this build reads: `Some(format)` for
/// `# wowdps creatures N`, `None` for anything else.
fn header_format(header: &str) -> Option<u32> {
    header
        .trim_end_matches('\r')
        .strip_prefix(HEADER_PREFIX)?
        .trim()
        .parse()
        .ok()
}

/// The header line this build writes.
pub fn header() -> String {
    format!("{HEADER_PREFIX}{FORMAT}")
}

/// A known classification, as written; `None` for anything else.
fn classification(text: &str) -> Option<&'static str> {
    CLASSIFICATIONS.iter().copied().find(|c| *c == text)
}

/// A name with every tab, line break and other control character turned
/// into a space, so it can never split a row.
fn one_line(name: &str) -> String {
    name.chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect::<String>()
        .trim()
        .to_string()
}

impl Creature {
    /// One row of the file; `None` for a row this reader cannot place.
    fn from_row(line: &str) -> Option<Self> {
        let mut cols = line.trim_end_matches('\r').split('\t');
        let id = cols.next()?.parse::<u32>().ok().filter(|id| *id > 0)?;
        let classification = classification(cols.next()?)?.to_string();
        let lieutenant = match cols.next()? {
            "0" => false,
            "1" => true,
            _ => return None,
        };
        let seen_unix = cols.next()?.parse::<i64>().ok()?;
        let name = cols.next().unwrap_or_default().to_string();
        Some(Self {
            id,
            classification,
            lieutenant,
            seen_unix,
            name,
        })
    }

    /// The row this creature writes, without its line break.
    fn to_row(&self) -> String {
        format!(
            "{}\t{}\t{}\t{}\t{}",
            self.id,
            self.classification,
            u8::from(self.lieutenant),
            self.seen_unix,
            one_line(&self.name)
        )
    }

    /// One `creatures[id]` record of the addon's table. `None` for a key
    /// that is not a creature id, an unknown classification or a record
    /// without a `seen` time.
    fn from_lua(key: &Key, rec: &Lua) -> Option<Self> {
        let id = match key {
            Key::Int(n) => u32::try_from(*n).ok(),
            Key::Str(s) => s.parse::<u32>().ok(),
        }
        .filter(|id| *id > 0)?;
        let classification = classification(rec.get("classification")?.as_str()?)?.to_string();
        // The addon writes 0 / 1; a boolean reads the same.
        let lieutenant = match rec.get("lieutenant") {
            Some(Lua::Bool(b)) => *b,
            Some(Lua::Num(n)) => *n != 0.0,
            _ => false,
        };
        let seen = rec
            .get("seen")?
            .as_f64()
            .filter(|s| s.is_finite() && *s > 0.0)?;
        Some(Self {
            id,
            classification,
            lieutenant,
            seen_unix: seen as i64,
            name: rec
                .get("name")
                .and_then(Lua::as_str)
                .map(one_line)
                .unwrap_or_default(),
        })
    }

    /// Every record in the addon's `WOWDPS_DATA` table (`data`): its
    /// `creatures`, keyed by creature id. A table without one is an empty
    /// answer — an addon older than the section.
    pub fn from_addon_table(data: &Lua) -> Vec<Self> {
        data.get("creatures")
            .and_then(Lua::as_table)
            .map(|t| t.iter().filter_map(|(k, v)| Self::from_lua(k, v)).collect())
            .unwrap_or_default()
    }

    /// Every creature record in one account's `wowdps.lua`. A file without
    /// the global is an empty answer, not an error.
    pub fn read_saved_variables(text: &str) -> Result<Vec<Self>, String> {
        Ok(addon_table(text)?
            .map(|data| Self::from_addon_table(&data))
            .unwrap_or_default())
    }
}

/// The file's rows by creature id. An error for a file whose first line is
/// not a header this build reads — another format, a newer one, or not
/// ours at all — so the daemon never rewrites what it cannot read whole. A
/// row it cannot read is skipped; the last row for an id wins.
pub fn parse(text: &str) -> Result<BTreeMap<u32, Creature>, String> {
    let mut lines = text.strip_prefix('\u{feff}').unwrap_or(text).lines();
    let head = lines.next().unwrap_or_default();
    match header_format(head) {
        Some(f) if (1..=FORMAT).contains(&f) => {}
        Some(f) => {
            return Err(format!(
                "{FILE}: format {f}, newer than this build's {FORMAT}"
            ));
        }
        None => return Err(format!("{FILE}: no `{}` header", header())),
    }
    Ok(lines
        .filter(|l| !l.starts_with('#'))
        .filter_map(Creature::from_row)
        .map(|c| (c.id, c))
        .collect())
}

/// The file for `rows`: the header, then one row per creature in the
/// iterator's order (a `BTreeMap`'s values are id-ascending).
pub fn render<'a>(rows: impl IntoIterator<Item = &'a Creature>) -> String {
    let mut out = header();
    out.push('\n');
    for c in rows {
        out.push_str(&c.to_row());
        out.push('\n');
    }
    out
}

/// Take `incoming` into `table`: a creature id's row is replaced when the
/// incoming record was SEEN later; a record seen no later than the stored
/// one changes nothing, and a row the incoming records lack stays (the
/// addon prunes in-game what it has not seen in a while; that refutes
/// nothing). The number of rows changed.
pub fn merge(table: &mut BTreeMap<u32, Creature>, incoming: Vec<Creature>) -> usize {
    let mut changed = 0;
    for c in incoming {
        let newer = table
            .get(&c.id)
            .is_none_or(|have| c.seen_unix > have.seen_unix);
        if newer {
            table.insert(c.id, c);
            changed += 1;
        }
    }
    changed
}
