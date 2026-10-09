//! The replay's NPC portraits: the two files that say which portrait each
//! NPC wears and where it is cut, read and written in one place.
//!
//! - The CROPS file, `tools/extract/portrait-crops.tsv` in the repository:
//!   where a person cut an NPC's portrait. The portrait editor writes it
//!   ("Save to repo"), gen-portraits embeds and cuts from it. Its leading
//!   `#` lines are its notes; then a header naming [`CROP_COLUMNS`], then a
//!   line per NPC, by encounter and NPC key. [`parse_crops`] and
//!   [`crops_tsv`] are the rows; [`CropsFile`] is the file whole, notes
//!   kept, and reads and writes the committed file back byte for byte.
//! - The INDEX, `index.tsv` in the per-machine portraits cache: a line per
//!   NPC of every encounter, the portrait it wears, where it fights and
//!   what else it could wear. gen-portraits writes it, the portrait editor
//!   reads it and nothing else. [`IndexRow`] names every column;
//!   [`index_tsv`] writes what gen-portraits' `index` writes and
//!   [`parse_index`] reads it back.
//!
//! A portrait is a [`Portrait`]: the Encounter Journal's own (a
//! FileDataID) or a creature display rendered from its model (`r` and the
//! display's id), named in both files by its [`Portrait::key`].

use std::collections::HashMap;
use std::str::FromStr;

use crate::Role;

/// A portrait an NPC can wear: the journal's (by its FileDataID), or a
/// creature display rendered from its model (by the display's id).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Portrait {
    Journal(u32),
    Render(u32),
}

impl Portrait {
    /// Its name in the cache's files, the index and the crops file: the
    /// FileDataID, or `r` and the display's id.
    pub fn key(self) -> String {
        match self {
            Portrait::Journal(fdid) => fdid.to_string(),
            Portrait::Render(id) => format!("r{id}"),
        }
    }

    /// A portrait from its `key`.
    pub fn parse(key: &str) -> Option<Portrait> {
        match key.strip_prefix('r') {
            Some(id) => id.parse().ok().map(Portrait::Render),
            None => key.parse().ok().map(Portrait::Journal),
        }
    }

    /// Its words for a person: `journal 7448163`, `render 142158`.
    pub fn words(self) -> String {
        match self {
            Portrait::Journal(fdid) => format!("journal {fdid}"),
            Portrait::Render(id) => format!("render {id}"),
        }
    }
}

/// An NPC as a reader sees it: `"<encounter>: <npc>"`, or the NPC's name
/// alone where the encounter wears it. The crops file's `who`.
pub fn who(encounter_name: &str, npc_name: &str) -> String {
    if encounter_name == npc_name {
        npc_name.to_string()
    } else {
        format!("{encounter_name}: {npc_name}")
    }
}

// ---------------------------------------------------------------------------
// The crops file.
// ---------------------------------------------------------------------------

/// Where a person cut an NPC's portrait: a line of the crops file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Crop {
    /// The encounter (its DungeonEncounterID) and the NPC's key in its
    /// rubric.
    pub encounter: u32,
    pub npc: String,
    /// The NPC's own `icon`, another's of its encounter where one picture
    /// shows both, or a render of its display.
    pub portrait: Portrait,
    /// `[x, y, side]` in a 128×64 picture: a render is cut as its 128×64
    /// reduction would be.
    pub square: [usize; 3],
    /// The encounter and the NPC's name ([`who`]), for the file's reader.
    pub who: String,
}

/// The crops file's columns, in order.
pub const CROP_COLUMNS: [&str; 7] = ["encounter", "npc", "portrait", "x", "y", "side", "who"];

/// The crops in `text`: `#` lines and blank ones skipped, then a header
/// naming `CROP_COLUMNS` in order, then a line per NPC. Every malformed
/// line is an error that names it.
pub fn parse_crops(text: &str) -> Result<Vec<Crop>, String> {
    let mut lines = text
        .lines()
        .enumerate()
        .filter(|(_, l)| !l.trim().is_empty() && !l.starts_with('#'));
    match lines.next() {
        Some((_, head)) if head.split('\t').eq(CROP_COLUMNS) => {}
        Some((n, head)) => {
            return Err(format!(
                "line {}: the header must be {:?}, not {head:?}",
                n + 1,
                CROP_COLUMNS.join("\t")
            ));
        }
        None => return Err("no header".into()),
    }
    let mut out = Vec::new();
    for (n, line) in lines {
        let bad = |what: &str| format!("line {}: {what}: {line:?}", n + 1);
        let f: Vec<&str> = line.split('\t').collect();
        let [encounter, npc, portrait, x, y, side, who] = f.as_slice() else {
            return Err(bad("not 7 columns"));
        };
        let num = |v: &str| {
            v.parse::<usize>()
                .map_err(|_| bad("a square not in whole pixels"))
        };
        out.push(Crop {
            encounter: encounter.parse().map_err(|_| bad("no encounter id"))?,
            npc: (*npc).to_string(),
            portrait: Portrait::parse(portrait).ok_or_else(|| bad("no portrait key"))?,
            square: [num(x)?, num(y)?, num(side)?],
            who: (*who).to_string(),
        });
    }
    Ok(out)
}

/// The crops under their header, by encounter and NPC key (byte order).
pub fn crops_tsv(crops: &[Crop]) -> String {
    let mut rows: Vec<&Crop> = crops.iter().collect();
    rows.sort_by(|a, b| (a.encounter, &a.npc).cmp(&(b.encounter, &b.npc)));
    let mut out = CROP_COLUMNS.join("\t");
    out.push('\n');
    for c in rows {
        let [x, y, side] = c.square;
        out.push_str(&format!(
            "{}\t{}\t{}\t{x}\t{y}\t{side}\t{}\n",
            c.encounter,
            c.npc,
            c.portrait.key(),
            c.who
        ));
    }
    out
}

/// A crops file's notes: its leading `#` lines, each with its line break.
pub fn notes(text: &str) -> String {
    text.lines()
        .take_while(|l| l.starts_with('#'))
        .map(|l| format!("{l}\n"))
        .collect()
}

/// The crops file whole: its notes, kept as they were written, and its
/// crops, written back under the header in their order.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CropsFile {
    /// The leading `#` lines ([`notes`]).
    pub notes: String,
    pub crops: Vec<Crop>,
}

impl CropsFile {
    pub fn parse(text: &str) -> Result<CropsFile, String> {
        Ok(CropsFile {
            notes: notes(text),
            crops: parse_crops(text)?,
        })
    }

    /// The notes, then [`crops_tsv`]: what the portrait editor saves.
    pub fn to_text(&self) -> String {
        format!("{}{}", self.notes, crops_tsv(&self.crops))
    }
}

// ---------------------------------------------------------------------------
// The index.
// ---------------------------------------------------------------------------

/// The columns of `index.tsv`, in order.
pub const INDEX_COLUMNS: [&str; 23] = [
    "portrait",
    "x",
    "y",
    "side",
    "chosen",
    "season",
    "season_order",
    "season_name",
    "instance",
    "instance_order",
    "instance_kind",
    "instance_name",
    "encounter",
    "encounter_order",
    "encounter_name",
    "npc",
    "npc_name",
    "role",
    "creatures",
    "icon",
    "display",
    "portraits",
    "file",
];

/// One line of `index.tsv`: an NPC of one encounter, each column by its
/// name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndexRow {
    /// The portrait it wears (none: an empty cell) and the square cut from
    /// it, in a 128×64 picture's pixels (0s without one), and whether a
    /// person chose that square (`1`) or the shape did (`0`).
    pub portrait: Option<Portrait>,
    pub x: usize,
    pub y: usize,
    pub side: usize,
    pub chosen: bool,
    /// The season's directory, its order (newer is higher) and name.
    pub season: String,
    pub season_order: i64,
    pub season_name: String,
    /// The instance's directory, its place in the season's list
    /// (`usize::MAX`: not listed), its kind (`raid`, `dungeon`) and name.
    pub instance: String,
    pub instance_order: usize,
    pub instance_kind: String,
    pub instance_name: String,
    /// The encounter's DungeonEncounterID, journal order and name.
    pub encounter: u32,
    pub encounter_order: u32,
    pub encounter_name: String,
    /// The NPC's key in the rubric, its name and role.
    pub npc: String,
    pub npc_name: String,
    pub role: Role,
    /// Its NPC ids, comma-separated.
    pub creatures: Vec<u32>,
    /// The journal's own portrait for it (0: none) and its creature
    /// display where one rendered (0: none).
    pub icon: u32,
    pub display: u32,
    /// Every journal portrait any NPC of its encounter wears, the ones it
    /// may be cut from, comma-separated.
    pub portraits: Vec<u32>,
    /// Its cut portrait under the cache (`npc/<encounter>-<npc>.png`),
    /// empty without one.
    pub file: String,
}

impl IndexRow {
    /// The square, where it wears a portrait.
    pub fn crop(&self) -> Option<(Portrait, [usize; 3])> {
        self.portrait.map(|p| (p, [self.x, self.y, self.side]))
    }

    /// Its [`who`].
    pub fn who(&self) -> String {
        who(&self.encounter_name, &self.npc_name)
    }
}

/// `index.tsv` for `rows`: [`INDEX_COLUMNS`], then a line per row, as
/// gen-portraits writes it. A tab or a line break in a text cell reads as
/// a space, so every line keeps its columns.
pub fn index_tsv(rows: &[IndexRow]) -> String {
    let clean = |s: &str| s.replace(['\t', '\n', '\r'], " ");
    let list = |v: &[u32]| v.iter().map(u32::to_string).collect::<Vec<_>>().join(",");
    let mut out = INDEX_COLUMNS.join("\t");
    out.push('\n');
    for r in rows {
        let cells = [
            r.portrait.map(Portrait::key).unwrap_or_default(),
            r.x.to_string(),
            r.y.to_string(),
            r.side.to_string(),
            u8::from(r.chosen).to_string(),
            clean(&r.season),
            r.season_order.to_string(),
            clean(&r.season_name),
            clean(&r.instance),
            r.instance_order.to_string(),
            clean(&r.instance_kind),
            clean(&r.instance_name),
            r.encounter.to_string(),
            r.encounter_order.to_string(),
            clean(&r.encounter_name),
            clean(&r.npc),
            clean(&r.npc_name),
            r.role.name().to_string(),
            list(&r.creatures),
            r.icon.to_string(),
            r.display.to_string(),
            list(&r.portraits),
            clean(&r.file),
        ];
        out.push_str(&cells.join("\t"));
        out.push('\n');
    }
    out
}

/// Read `index.tsv`: its header names the columns (in any order, others
/// ignored), and each line must give every one of them a value of its
/// kind. An error names the column it lacks, or the line and column it
/// could not read.
pub fn parse_index(text: &str) -> Result<Vec<IndexRow>, String> {
    let mut lines = text.lines().enumerate().filter(|(_, l)| !l.is_empty());
    let head: HashMap<&str, usize> = lines
        .next()
        .map(|(_, h)| h.split('\t').enumerate().map(|(i, c)| (c, i)).collect())
        .unwrap_or_default();
    let mut at = [0usize; INDEX_COLUMNS.len()];
    for (slot, name) in at.iter_mut().zip(INDEX_COLUMNS) {
        *slot = *head.get(name).ok_or_else(|| format!("no column {name}"))?;
    }
    let mut out = Vec::new();
    for (n, line) in lines {
        // Read in the columns' order: each `IndexRow` field is the next.
        let mut l = Cells {
            n: n + 1,
            cells: line.split('\t').collect(),
            at: &at,
            next: 0,
        };
        out.push(IndexRow {
            portrait: l.get("portrait", |v| match v {
                "" => Some(None),
                k => Portrait::parse(k).map(Some),
            })?,
            x: l.get("x", parsed)?,
            y: l.get("y", parsed)?,
            side: l.get("side", parsed)?,
            chosen: l.get("chosen", |v| match v {
                "1" => Some(true),
                "0" => Some(false),
                _ => None,
            })?,
            season: l.string("season")?,
            season_order: l.get("season_order", parsed)?,
            season_name: l.string("season_name")?,
            instance: l.string("instance")?,
            instance_order: l.get("instance_order", parsed)?,
            instance_kind: l.string("instance_kind")?,
            instance_name: l.string("instance_name")?,
            encounter: l.get("encounter", parsed)?,
            encounter_order: l.get("encounter_order", parsed)?,
            encounter_name: l.string("encounter_name")?,
            npc: l.string("npc")?,
            npc_name: l.string("npc_name")?,
            role: l.get("role", Role::from_name)?,
            creatures: l.list("creatures")?,
            icon: l.get("icon", parsed)?,
            display: l.get("display", parsed)?,
            portraits: l.list("portraits")?,
            file: l.string("file")?,
        });
    }
    Ok(out)
}

fn parsed<T: FromStr>(v: &str) -> Option<T> {
    v.parse().ok()
}

/// One line of the index, its cells read in `INDEX_COLUMNS`' order: each
/// read takes the next column, found in the line through the header once.
struct Cells<'a> {
    /// Its line number, from 1.
    n: usize,
    cells: Vec<&'a str>,
    /// Where each of `INDEX_COLUMNS` stands in the line.
    at: &'a [usize; INDEX_COLUMNS.len()],
    /// The column the next read takes, in `INDEX_COLUMNS`.
    next: usize,
}

impl<'a> Cells<'a> {
    /// The next column's cell; `name` is that column (said in the errors,
    /// and checked in a debug build).
    fn cell(&mut self, name: &str) -> Result<&'a str, String> {
        let k = self.next;
        self.next += 1;
        debug_assert_eq!(INDEX_COLUMNS.get(k), Some(&name), "read out of order");
        self.at
            .get(k)
            .and_then(|&i| self.cells.get(i))
            .copied()
            .ok_or_else(|| format!("line {}: no {name}", self.n))
    }

    fn bad(&self, name: &str, v: &str) -> String {
        format!("line {}: {name} {v:?}", self.n)
    }

    /// A cell read as its kind by `read`.
    fn get<T>(&mut self, name: &str, read: impl Fn(&str) -> Option<T>) -> Result<T, String> {
        let v = self.cell(name)?;
        read(v).ok_or_else(|| self.bad(name, v))
    }

    fn string(&mut self, name: &str) -> Result<String, String> {
        self.cell(name).map(str::to_string)
    }

    /// A comma-separated list of ids (spaces round them allowed).
    fn list(&mut self, name: &str) -> Result<Vec<u32>, String> {
        let v = self.cell(name)?;
        v.split(',')
            .map(str::trim)
            .filter(|n| !n.is_empty())
            .map(|n| parsed(n).ok_or_else(|| self.bad(name, v)))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The repository's crops file, read at test time like the editor
    /// reads it.
    const CROPS_FILE: &str = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../tools/extract/portrait-crops.tsv"
    );

    #[test]
    fn a_portrait_is_named_by_its_key() {
        for p in [Portrait::Journal(7966626), Portrait::Render(142158)] {
            assert_eq!(Portrait::parse(&p.key()), Some(p));
        }
        assert_eq!(Portrait::Render(142158).key(), "r142158");
        assert_eq!(Portrait::Render(142158).words(), "render 142158");
        assert_eq!(Portrait::Journal(7448163).words(), "journal 7448163");
        assert_eq!(Portrait::parse(""), None);
        assert_eq!(Portrait::parse("r"), None);
        assert_eq!(Portrait::parse("rx"), None);
    }

    #[test]
    fn who_names_the_encounter_unless_the_npc_is_it() {
        assert_eq!(
            who("Vashnik the Malignant", "Vashnik the Malignant"),
            "Vashnik the Malignant"
        );
        assert_eq!(
            who("Vashnik the Malignant", "Burning Venom"),
            "Vashnik the Malignant: Burning Venom"
        );
    }

    #[test]
    fn the_committed_crops_file_round_trips_byte_for_byte() {
        // The crops file lives beside the extractor that embeds it, on the
        // branch that carries gen-portraits and the portrait editor; a
        // checkout without it (main) has no committed file to hold. Where
        // it is, the extractor's `include_str!` refuses to build without
        // it, so this skips nothing that could go missing unnoticed.
        let text = match std::fs::read_to_string(CROPS_FILE) {
            Ok(text) => text,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return,
            Err(e) => panic!("{CROPS_FILE}: {e}"),
        };
        let file = CropsFile::parse(&text).unwrap();
        assert!(file.crops.len() > 100, "{}", file.crops.len());
        assert!(file.notes.starts_with("# Where a person cut"));
        assert_eq!(file.to_text(), text);
    }

    #[test]
    fn crops_read_back_as_they_were_written() {
        let text = "# a comment\n\n\
                    encounter\tnpc\tportrait\tx\ty\tside\twho\n\
                    3497\ttrader-gebbo\tr143083\t41\t9\t55\tThe Lost Explorers: Trader Gebbo\n\
                    1698\tranjit\t1044362\t41\t1\t54\tRanjit\n";
        let read = parse_crops(text).unwrap();
        assert_eq!(
            read.first(),
            Some(&Crop {
                encounter: 3497,
                npc: "trader-gebbo".into(),
                portrait: Portrait::Render(143083),
                square: [41, 9, 55],
                who: "The Lost Explorers: Trader Gebbo".into(),
            })
        );
        // Written back by encounter, under the header alone; the notes are
        // the leading `#` lines, up to the first that is not one.
        let written = crops_tsv(&read);
        assert!(written.starts_with("encounter\tnpc\tportrait\tx\ty\tside\twho\n1698\t"));
        let [first, second] = read.as_slice() else {
            panic!("{read:?}");
        };
        assert_eq!(
            parse_crops(&written),
            Ok(vec![second.clone(), first.clone()])
        );
        assert_eq!(notes(text), "# a comment\n");
        assert_eq!(
            CropsFile::parse(text).unwrap().to_text(),
            format!("# a comment\n{written}")
        );

        for (bad, says) in [
            ("npc\tencounter\n", "the header"),
            ("", "no header"),
            (
                "encounter\tnpc\tportrait\tx\ty\tside\twho\n1\tx\t2\t3\t4\n",
                "7 columns",
            ),
            (
                "encounter\tnpc\tportrait\tx\ty\tside\twho\n1\tx\tq\t3\t4\t5\tX\n",
                "portrait",
            ),
            (
                "encounter\tnpc\tportrait\tx\ty\tside\twho\n1\tx\t2\t3\t-4\t5\tX\n",
                "pixels",
            ),
        ] {
            let e = parse_crops(bad).unwrap_err();
            assert!(e.contains(says), "{bad:?}: {e}");
        }
    }

    #[test]
    fn a_real_index_round_trips_byte_for_byte() {
        let text = include_str!("../fixtures/portraits/index.tsv");
        let rows = parse_index(text).unwrap();
        assert_eq!(rows.len(), 8);
        assert_eq!(index_tsv(&rows), text);
        let ravi = &rows[0];
        assert_eq!(ravi.crop(), Some((Portrait::Journal(7966629), [34, 0, 64])));
        assert!(ravi.chosen);
        assert_eq!(ravi.who(), "Rav'i");
        assert_eq!((ravi.season_order, ravi.instance_order), (18, 2));
        assert_eq!(ravi.role, Role::Boss);
        assert_eq!(ravi.file, "npc/3456-ravi.png");
        let bare = &rows[1];
        assert_eq!(bare.crop(), None);
        assert_eq!(bare.who(), "The Writhing Coil: Rattling Writhe");
        assert_eq!(bare.portraits, [7966635]);
        assert!(rows[2].creatures.is_empty());
        assert_eq!(rows[3].creatures, [234648, 255050]);
        assert_eq!(rows[6].role, Role::Object);
        assert_eq!(rows[4].portrait, Some(Portrait::Render(110633)));
    }

    #[test]
    fn the_index_keeps_its_columns() {
        let row = IndexRow {
            portrait: Some(Portrait::Render(55846)),
            x: 30,
            y: 0,
            side: 64,
            chosen: false,
            season: "midnight-s2".into(),
            season_order: 18,
            season_name: "Midnight Season 2".into(),
            instance: "the-venomous-abyss".into(),
            instance_order: usize::MAX,
            instance_kind: "raid".into(),
            instance_name: "The Venomous\tAbyss".into(),
            encounter: 3455,
            encounter_order: 4,
            encounter_name: "Vashnik the\nMalignant".into(),
            npc: "burning-venom".into(),
            npc_name: "Burning Venom".into(),
            role: Role::Pet,
            creatures: vec![259181, 266403],
            icon: 0,
            display: 55846,
            portraits: vec![7966626, 7966628],
            file: "npc/3455-burning-venom.png".into(),
        };
        let text = index_tsv(std::slice::from_ref(&row));
        let mut lines = text.lines();
        assert_eq!(lines.next(), Some(INDEX_COLUMNS.join("\t").as_str()));
        let cells: Vec<&str> = lines.next().unwrap().split('\t').collect();
        assert_eq!(cells.len(), INDEX_COLUMNS.len());
        assert_eq!(cells[0], "r55846");
        assert_eq!(cells[9], "18446744073709551615");
        assert_eq!(cells[11], "The Venomous Abyss");
        assert_eq!(cells[14], "Vashnik the Malignant");
        assert_eq!(cells[17], "pet");
        assert_eq!(cells[18], "259181,266403");
        assert_eq!(cells[21], "7966626,7966628");
        // Read back, the cleaned cells say what was written.
        let back = parse_index(&text).unwrap();
        let want = IndexRow {
            instance_name: "The Venomous Abyss".into(),
            encounter_name: "Vashnik the Malignant".into(),
            ..row
        };
        assert_eq!(back, [want]);
    }

    #[test]
    fn a_bad_index_says_what() {
        let text = include_str!("../fixtures/portraits/index.tsv");
        let no_file = text.replace("\tfile\n", "\n");
        assert_eq!(parse_index(&no_file).unwrap_err(), "no column file");
        let head = text.lines().next().unwrap();
        for (line, says) in [
            ("q\t0\t0\t0\t0", "line 2: portrait \"q\""),
            ("\t0\t0\t0\t2", "line 2: chosen \"2\""),
            ("\t0\t-1\t0\t0", "line 2: y \"-1\""),
            ("\t0\t0", "line 2: no side"),
        ] {
            let e = parse_index(&format!("{head}\n{line}\n")).unwrap_err();
            assert!(e.contains(says), "{e} (wanted {says})");
        }
    }
}
