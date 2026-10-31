//! Decode parsed WDC5 rows through a DBD layout into CSV.
//!
//! The DBD layout drives column order and naming (arrays expand to
//! `Name[0]`, `Name[1]`, …). Non-inline `$id$`/`$relation$` columns come
//! from the row itself; every other layout field maps, in order, onto one
//! storage field of the record. Signedness and display width come from the
//! DBD (`<u32>` vs `<32>`); sizeless int columns display as signed 32-bit,
//! matching DBCD (and therefore wago.tools exports).

use crate::bits::{mask, read_bits, sign_extend};
use crate::dbd::{ColType, Dbd, FieldDef};
use crate::wdc5::{
    Compression, Db2, FLAG_SPARSE, Row, StorageInfo, bitpacked_value, cstr_lossy, elem_bits,
};
use std::collections::HashMap;
use std::fmt::Write as _;

enum Source {
    RowId,
    Foreign,
    Inline(usize),
}

struct Col<'a> {
    def: &'a FieldDef,
    ty: ColType,
    source: Source,
}

/// The layout's columns, each mapped onto its source: what both `write_csv`
/// and `decode` walk the rows by.
fn layout<'a>(db2: &Db2, dbd: &'a Dbd) -> Result<Vec<Col<'a>>, String> {
    let hash = db2.header.layout_hash;
    let ver = dbd.version_for_layout(hash).ok_or_else(|| {
        let known: Vec<String> = dbd
            .known_layouts()
            .iter()
            .map(|h| format!("{h:08X}"))
            .collect();
        format!(
            "layout {hash:08X} not in dbd (has: {}); update WoWDBDefs",
            known.join(", ")
        )
    })?;

    // Map layout columns onto record storage fields.
    let mut cols = Vec::with_capacity(ver.fields.len());
    let mut inline = 0usize;
    for def in &ver.fields {
        let source = if def.noninline && def.is_id {
            Source::RowId
        } else if def.noninline && def.is_relation {
            Source::Foreign
        } else {
            inline += 1;
            Source::Inline(inline - 1)
        };
        cols.push(Col {
            def,
            ty: dbd.col_type(&def.name)?,
            source,
        });
    }
    if inline != db2.header.field_count as usize {
        return Err(format!(
            "layout {hash:08X} has {inline} inline fields, file has {}",
            db2.header.field_count
        ));
    }
    Ok(cols)
}

/// The header's names into `out`, `sep` between them; array columns expand
/// the way wago.tools exports them: Name_0, Name_1…
fn header(cols: &[Col], out: &mut impl Sink) {
    for (i, col) in cols.iter().enumerate() {
        if i > 0 {
            out.sep();
        }
        match col.def.array {
            Some(n) => {
                for j in 0..n {
                    if j > 0 {
                        out.sep();
                    }
                    out.name(&format!("{}_{j}", col.def.name));
                }
            }
            None => out.name(&col.def.name),
        }
    }
}

/// Each row, by id, its cells into `out` (`row` starts it).
fn rows(db2: &Db2, cols: &[Col], out: &mut impl Sink) -> Result<(), String> {
    let mut order: Vec<&Row> = db2.rows.iter().collect();
    order.sort_by_key(|r| r.id);
    let sparse = db2.header.flags & FLAG_SPARSE != 0;
    for row in order {
        out.row()?;
        if sparse {
            sparse_row(db2, cols, row, out)?;
        } else {
            packed_row(db2, cols, row, out)?;
        }
    }
    Ok(())
}

/// The table as CSV text, a line per row by id under its header (what
/// `csv` writes and wago.tools' exports diff against).
pub fn write_csv(db2: &Db2, dbd: &Dbd, out: &mut dyn std::io::Write) -> Result<(), String> {
    let cols = layout(db2, dbd)?;
    let mut text = Text {
        line: String::new(),
        out,
        open: false,
    };
    text.row()?;
    header(&cols, &mut text);
    rows(db2, &cols, &mut text)?;
    text.finish()
}

/// The table decoded straight into cells: exactly `parse_csv` of what
/// `write_csv` writes, without the text between. The two walk the same
/// cells in the same order (`Sink`), and the text's only transformation
/// is undone exactly by the reader: a number is written bare (digits, a
/// sign, a point, an exponent, `inf` or `NaN`: never a comma, a quote or a
/// line break), a column name is a DBD identifier, and a string is quoted
/// by `push_str` whenever it holds anything the reader would split on or
/// drop (a comma, a quote, a CR or LF), quotes doubled, which the reader
/// unquotes. A row with other than the header's count of cells is the same
/// error `parse_csv` gives.
pub fn decode(db2: &Db2, dbd: &Dbd) -> Result<Csv, String> {
    let cols = layout(db2, dbd)?;
    let mut cells = Cells::default();
    cells.row()?;
    header(&cols, &mut cells);
    rows(db2, &cols, &mut cells)?;
    let mut rows = cells.finish();
    let header = rows.remove(0);
    for (i, r) in rows.iter().enumerate() {
        if r.len() != header.len() {
            return Err(format!(
                "csv: row {} has {} cells, header {}",
                i + 2,
                r.len(),
                header.len()
            ));
        }
    }
    Ok(Csv { header, rows })
}

/// Where a table's cells go: CSV text (`write_csv`) or cells (`decode`).
/// A line (the header, then each row) is opened by `row`, which ends the
/// one before, and ended by the next `row` or the sink's `finish`; within
/// it `sep` comes between cells, each written by `name` (the header's),
/// `int`, `float` or `text`.
trait Sink {
    fn row(&mut self) -> Result<(), String>;
    fn sep(&mut self);
    fn name(&mut self, name: &str);
    fn int(&mut self, def: &FieldDef, raw: u64);
    fn float(&mut self, raw: u64);
    fn text(&mut self, s: &str);
}

/// CSV text, written a line at a time.
struct Text<'a> {
    line: String,
    out: &'a mut dyn std::io::Write,
    open: bool,
}

impl Text<'_> {
    /// The open line, ended, out.
    fn end(&mut self) -> Result<(), String> {
        self.line.push('\n');
        self.out
            .write_all(self.line.as_bytes())
            .map_err(|e| e.to_string())?;
        self.line.clear();
        Ok(())
    }

    /// The last line out.
    fn finish(&mut self) -> Result<(), String> {
        if self.open {
            self.open = false;
            self.end()?;
        }
        Ok(())
    }
}

impl Sink for Text<'_> {
    fn row(&mut self) -> Result<(), String> {
        if self.open {
            self.end()?;
        }
        self.open = true;
        Ok(())
    }
    fn sep(&mut self) {
        self.line.push(',');
    }
    fn name(&mut self, name: &str) {
        self.line.push_str(name);
    }
    fn int(&mut self, def: &FieldDef, raw: u64) {
        push_int(&mut self.line, def, raw);
    }
    fn float(&mut self, raw: u64) {
        push_float(&mut self.line, raw);
    }
    fn text(&mut self, s: &str) {
        push_str(&mut self.line, s);
    }
}

/// Cells, a list a line (the header's names the first): a line ends with
/// its last cell, so a line always has one, as a CSV line does.
#[derive(Default)]
struct Cells {
    rows: Vec<Vec<String>>,
    row: Vec<String>,
    cell: String,
    open: bool,
}

impl Cells {
    /// Every line, the last one ended.
    fn finish(mut self) -> Vec<Vec<String>> {
        if self.open {
            self.end();
        }
        self.rows
    }

    fn end(&mut self) {
        self.row.push(std::mem::take(&mut self.cell));
        self.rows.push(std::mem::take(&mut self.row));
    }
}

impl Sink for Cells {
    fn row(&mut self) -> Result<(), String> {
        if self.open {
            self.end();
        }
        self.open = true;
        Ok(())
    }
    fn sep(&mut self) {
        self.row.push(std::mem::take(&mut self.cell));
    }
    fn name(&mut self, name: &str) {
        self.cell.push_str(name);
    }
    fn int(&mut self, def: &FieldDef, raw: u64) {
        push_int(&mut self.cell, def, raw);
    }
    fn float(&mut self, raw: u64) {
        push_float(&mut self.cell, raw);
    }
    fn text(&mut self, s: &str) {
        self.cell.push_str(s);
    }
}

/// Fixed-size records: every storage field is read at its storage-info bit
/// offset, so column decode order is independent.
fn packed_row(db2: &Db2, cols: &[Col], row: &Row, out: &mut impl Sink) -> Result<(), String> {
    let rec = db2.record(row);
    for (ci, col) in cols.iter().enumerate() {
        if ci > 0 {
            out.sep();
        }
        let k = match col.source {
            Source::RowId => {
                out.int(col.def, row.id as u64);
                continue;
            }
            Source::Foreign => {
                out.int(col.def, row.foreign.unwrap_or(0) as u64);
                continue;
            }
            Source::Inline(k) => k,
        };
        if col.def.is_id {
            // Inline id: display the resolved row id (copy-table rows must
            // show their own id, not the copied record's).
            out.int(col.def, row.id as u64);
            continue;
        }

        let info = storage(db2, k, col)?;
        let fs = db2
            .fields
            .get(k)
            .ok_or_else(|| format!("field {}: no field structure {k}", col.def.name))?;
        match info.compression {
            Compression::None => {
                let elem = elem_bits(fs, info);
                if elem == 0 || elem > 64 {
                    return Err(format!("field {}: bad element width {elem}", col.def.name));
                }
                let count = col.def.array.unwrap_or(1);
                if count as u64 * elem as u64 != info.size_bits as u64 && info.size_bits != 0 {
                    return Err(format!(
                        "field {}: dbd says {count}x{elem} bits, file says {}",
                        col.def.name, info.size_bits
                    ));
                }
                for j in 0..count {
                    if j > 0 {
                        out.sep();
                    }
                    let bit = info.offset_bits as usize + (j * elem) as usize;
                    let raw = read_bits(rec, bit, elem);
                    match col.ty {
                        ColType::Str | ColType::LocStr => {
                            let rel = sign_extend(raw, elem);
                            let s = db2.string_at(row.global, (bit / 8) as u32, rel)?;
                            out.text(s);
                        }
                        ColType::Float => out.float(raw),
                        ColType::Int => out.int(col.def, raw),
                    }
                }
            }
            Compression::Bitpacked | Compression::BitpackedSigned => {
                let raw = bitpacked_value(rec, info);
                match col.ty {
                    ColType::Int => out.int(col.def, raw),
                    // A float can only be bitpacked as its whole IEEE
                    // pattern; any narrower packing has no meaning
                    // (SpellRadius.Radius is stored this way).
                    ColType::Float if info.size_bits == 32 => out.float(raw),
                    _ => {
                        require_int(col)?;
                    }
                }
            }
            Compression::CommonData => {
                let v = db2
                    .commons
                    .get(k)
                    .and_then(|m| m.get(&row.id))
                    .copied()
                    .unwrap_or(info.args[0]);
                match col.ty {
                    ColType::Float => out.float(v as u64),
                    _ => out.int(col.def, v as u64),
                }
            }
            Compression::PalletIndexed => {
                let idx = read_bits(rec, info.offset_bits as usize, info.size_bits as u32);
                let v = pallet_get(db2, k, idx as usize, col)?;
                match col.ty {
                    ColType::Float => out.float(v as u64),
                    _ => out.int(col.def, v as u64),
                }
            }
            Compression::PalletArray => {
                let idx = read_bits(rec, info.offset_bits as usize, info.size_bits as u32);
                let card = info.args[2];
                let count = col.def.array.unwrap_or(1);
                if count != card {
                    return Err(format!(
                        "field {}: dbd array [{count}] vs pallet cardinality {card}",
                        col.def.name
                    ));
                }
                for j in 0..card {
                    if j > 0 {
                        out.sep();
                    }
                    let v = pallet_get(db2, k, (idx as u32 * card + j) as usize, col)?;
                    match col.ty {
                        ColType::Float => out.float(v as u64),
                        _ => out.int(col.def, v as u64),
                    }
                }
            }
        }
    }
    Ok(())
}

/// Sparse (offset-map) records inline their strings, so later fields have no
/// fixed offset: decode sequentially in storage order. All fields are
/// uncompressed in sparse tables.
fn sparse_row(db2: &Db2, cols: &[Col], row: &Row, out: &mut impl Sink) -> Result<(), String> {
    let rec = db2.record(row);
    let mut bit = 0usize;
    for (ci, col) in cols.iter().enumerate() {
        if ci > 0 {
            out.sep();
        }
        let k = match col.source {
            Source::RowId => {
                out.int(col.def, row.id as u64);
                continue;
            }
            Source::Foreign => {
                out.int(col.def, row.foreign.unwrap_or(0) as u64);
                continue;
            }
            Source::Inline(k) => k,
        };
        let info = storage(db2, k, col)?;
        if info.compression != Compression::None {
            return Err(format!(
                "sparse field {} uses {:?} compression",
                col.def.name, info.compression
            ));
        }
        match col.ty {
            ColType::Str | ColType::LocStr => {
                let start = bit.div_ceil(8);
                let (owned, len) = cstr_lossy(rec.get(start.min(rec.len())..).unwrap_or(&[][..]))?;
                bit = (start + len + 1) * 8;
                if col.def.is_id {
                    return Err("sparse string id field".into());
                }
                out.text(&owned);
            }
            ColType::Float | ColType::Int => {
                let fs = db2
                    .fields
                    .get(k)
                    .ok_or_else(|| format!("field {}: no field structure {k}", col.def.name))?;
                let mut elem = elem_bits(fs, info);
                if elem == 0 {
                    elem = 32;
                }
                let count = col.def.array.unwrap_or(1);
                for j in 0..count {
                    if j > 0 {
                        out.sep();
                    }
                    let raw = read_bits(rec, bit, elem);
                    bit += elem as usize;
                    if col.def.is_id {
                        out.int(col.def, row.id as u64);
                    } else if col.ty == ColType::Float {
                        out.float(raw);
                    } else {
                        out.int(col.def, raw);
                    }
                }
            }
        }
    }
    Ok(())
}

fn pallet_get(db2: &Db2, k: usize, idx: usize, col: &Col) -> Result<u32, String> {
    db2.pallets
        .get(k)
        .and_then(|p| p.get(idx))
        .copied()
        .ok_or_else(|| format!("field {}: pallet index {idx} out of range", col.def.name))
}

/// The storage info for inline field `k`, which the layout/field-count check
/// in [`write_csv`] already guarantees exists.
fn storage<'a>(db2: &'a Db2, k: usize, col: &Col) -> Result<&'a StorageInfo, String> {
    db2.infos
        .get(k)
        .ok_or_else(|| format!("field {}: no storage info {k}", col.def.name))
}

fn require_int(col: &Col) -> Result<(), String> {
    match col.ty {
        ColType::Int => Ok(()),
        _ => Err(format!("field {}: bitpacked non-int column", col.def.name)),
    }
}

/// A parsed CSV table (as produced by [`write_csv`] or wago exports).
#[derive(Debug, PartialEq)]
pub struct Csv {
    pub header: Vec<String>,
    pub rows: Vec<Vec<String>>,
}

impl Csv {
    /// Index of a named column.
    pub fn col(&self, name: &str) -> Result<usize, String> {
        self.header
            .iter()
            .position(|c| c == name)
            .ok_or_else(|| format!("csv: no column {name:?} (have {})", self.header.join(",")))
    }
}

/// Every client table a generator reads, by name, with its FileDataID: the
/// live build's root manifest carries no names, so a table is fetched by
/// its id (WoWDBDefs' `manifest.json` and wowdev's listfile name them; a
/// file keeps its id forever). One line a table, in name order; a
/// generator lists the names it reads (`TABLES`). The replay's extractor
/// (`wowdps-replay-extract`) loads its tables through `load` too, so its
/// generators' tables are listed here beside this crate's.
pub const FILE_IDS: [(&str, u32); 74] = [
    ("ChrCustomizationElement", 3512765),
    ("ChrCustomizationGeoset", 3456171),
    ("ChrCustomizationMaterial", 3459652),
    ("ChrModelTextureLayer", 3548976),
    ("ChrSpecialization", 1343390),
    ("CreatureDisplayInfo", 1108759),
    ("CreatureDisplayInfoExtra", 1264997),
    ("CreatureDisplayInfoGeosetData", 1720141),
    ("CreatureDisplayInfoOption", 3692043),
    ("CreatureModelData", 1365368),
    ("Difficulty", 1352127),
    ("EncounterEvent", 7571075),
    ("Item", 841626),
    ("ItemEffect", 969941),
    ("ItemSparse", 1572924),
    ("ItemXItemEffect", 3177687),
    ("JournalEncounter", 1240336),
    ("JournalEncounterCreature", 1301155),
    ("JournalEncounterSection", 1134413),
    ("JournalInstance", 1237438),
    ("JournalSectionXDifficulty", 1121900),
    ("Light", 1375579),
    ("LightData", 1375580),
    ("LightParams", 1334669),
    ("LiquidType", 1371380),
    ("LiquidTypeXTexture", 2261065),
    ("Map", 1349477),
    ("MapChallengeMode", 801709),
    ("SkillLine", 1240935),
    ("SkillLineAbility", 1266278),
    ("SkillLineXTraitTree", 4505477),
    ("SpecSetMember", 2057624),
    ("SpecializationSpells", 1240335),
    ("Spell", 1140089),
    ("SpellAuraOptions", 1139952),
    ("SpellCastTimes", 1134089),
    ("SpellCategories", 1139939),
    ("SpellCategory", 1280619),
    ("SpellCooldowns", 1139924),
    ("SpellDescriptionVariables", 1140004),
    ("SpellDuration", 1137828),
    ("SpellEffect", 1140088),
    ("SpellEmpower", 4507381),
    ("SpellMisc", 1003144),
    ("SpellName", 1990283),
    ("SpellPower", 982806),
    ("SpellRadius", 1134584),
    ("SpellRange", 1146820),
    ("SpellXDescriptionVariables", 1724949),
    ("TextureFileData", 982459),
    ("TraitCond", 4543085),
    ("TraitCost", 4420295),
    ("TraitCurrency", 4524216),
    ("TraitCurrencySource", 4539393),
    ("TraitDefinition", 4420327),
    ("TraitEdge", 4420308),
    ("TraitNode", 4420297),
    ("TraitNodeEntry", 4420298),
    ("TraitNodeEntryXTraitCond", 4543088),
    ("TraitNodeGroupXTraitCond", 4543090),
    ("TraitNodeGroupXTraitCost", 4420301),
    ("TraitNodeGroupXTraitNode", 4420302),
    ("TraitNodeXTraitCond", 4543092),
    ("TraitNodeXTraitCost", 4420303),
    ("TraitNodeXTraitNodeEntry", 4420304),
    ("TraitSubTree", 5534447),
    ("TraitTreeLoadout", 4669507),
    ("TraitTreeXTraitCurrency", 4524218),
    ("UiMapAssignment", 1957219),
    ("UiTextureAtlas", 897470),
    ("UiTextureAtlasElement", 1989276),
    ("UiTextureAtlasMember", 897532),
    ("ZoneLight", 1310253),
    ("ZoneLightPoint", 1310256),
];

/// A table's FileDataID (`FILE_IDS`).
pub fn file_id(name: &str) -> Result<u32, String> {
    FILE_IDS
        .binary_search_by(|(n, _)| (*n).cmp(name))
        .ok()
        .and_then(|i| FILE_IDS.get(i))
        .map(|&(_, fdid)| fdid)
        .ok_or_else(|| format!("{name}: no FileDataID in table::FILE_IDS"))
}

/// One client table from the install, by its name (its FileDataID from
/// `FILE_IDS`), decoded by its WoWDBDefs schema `<dbd_dir>/<name>.dbd`:
/// what every command reading tables loads (`Install::open_tables`).
pub fn load(
    game: &crate::game::Game,
    dbd_dir: &std::path::Path,
    name: &str,
) -> Result<Csv, String> {
    let db2 =
        Db2::parse(game.fetch_fdid(file_id(name)?, 0x2)?).map_err(|e| format!("{name}: {e}"))?;
    decode_by(&db2, &dbd_dir.join(format!("{name}.dbd")), name)
}

/// `load`, and where the schema kept will not decode this build's table
/// (a copy older than the build, or none at all), `refetch` fetches it
/// into its path again and it is read once more: what `wowdps-db2` does
/// for its own cache, here for a caller that owns its schemas' directory
/// (the map editor). The table's file is read from the install once.
pub fn load_refetching(
    game: &crate::game::Game,
    dbd_dir: &std::path::Path,
    name: &str,
    refetch: &mut dyn FnMut(&std::path::Path) -> Result<(), String>,
) -> Result<Csv, String> {
    let db2 =
        Db2::parse(game.fetch_fdid(file_id(name)?, 0x2)?).map_err(|e| format!("{name}: {e}"))?;
    let path = dbd_dir.join(format!("{name}.dbd"));
    match decode_by(&db2, &path, name) {
        Ok(csv) => Ok(csv),
        Err(first) => {
            refetch(&path).map_err(|e| format!("{first}; fetching its schema again: {e}"))?;
            decode_by(&db2, &path, name)
        }
    }
}

/// `db2` decoded by the schema at `path`.
fn decode_by(db2: &Db2, path: &std::path::Path, name: &str) -> Result<Csv, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let dbd = Dbd::parse(&text).map_err(|e| format!("{name}: {e}"))?;
    decode(db2, &dbd).map_err(|e| format!("{name}: {e}"))
}

/// The decoded tables a generator reads, by name.
#[derive(Debug, Default)]
pub struct Tables {
    by_name: HashMap<String, Csv>,
}

impl Tables {
    /// Keep `csv` as table `name` (a table loaded twice keeps the last).
    pub fn insert(&mut self, name: &str, csv: Csv) {
        self.by_name.insert(name.to_string(), csv);
    }

    /// Table `name`; one the generator never loaded is an error.
    pub fn get(&self, name: &str) -> Result<&Csv, String> {
        self.find(name)
            .ok_or_else(|| format!("missing table {name}"))
    }

    /// Table `name`, where it was loaded (a table a generator can do
    /// without).
    pub fn find(&self, name: &str) -> Option<&Csv> {
        self.by_name.get(name)
    }

    /// Table `name` to change (a test's variation on a table).
    pub fn get_mut(&mut self, name: &str) -> Option<&mut Csv> {
        self.by_name.get_mut(name)
    }

    /// Table `name` taken out.
    pub fn remove(&mut self, name: &str) -> Option<Csv> {
        self.by_name.remove(name)
    }
}

impl<'a> FromIterator<(&'a str, Csv)> for Tables {
    fn from_iter<I: IntoIterator<Item = (&'a str, Csv)>>(it: I) -> Tables {
        let mut t = Tables::default();
        for (name, csv) in it {
            t.insert(name, csv);
        }
        t
    }
}

/// A row's cell in column `c` (from `Csv::col`, so a miss means the row
/// itself is short: a malformed table, named by `what`).
pub fn cell<'a>(row: &'a [String], c: usize, what: &str) -> Result<&'a str, String> {
    row.get(c)
        .map(String::as_str)
        .ok_or_else(|| format!("{what}: row has no column {c}"))
}

/// A row's cell in column `c`, empty where the row is short.
pub fn text(row: &[String], c: usize) -> &str {
    row.get(c).map_or("", String::as_str)
}

/// A row's cell in column `c` as an integer, 0 where it is missing or is
/// not one.
pub fn int(row: &[String], c: usize) -> i64 {
    text(row, c).parse().unwrap_or(0)
}

/// A row's cell in column `c` (`cell`) read as a `T`; a cell that is not
/// one is an error naming `what`.
pub fn num<T: std::str::FromStr>(row: &[String], c: usize, what: &str) -> Result<T, String> {
    let s = cell(row, c, what)?;
    s.parse().map_err(|_| format!("{what}: bad number {s:?}"))
}

/// Minimal RFC-4180-ish reader (quotes, escaped quotes, CRLF); enough for
/// our own output and wago exports.
pub fn parse_csv(text: &str) -> Result<Csv, String> {
    let mut rows: Vec<Vec<String>> = Vec::new();
    let mut row: Vec<String> = Vec::new();
    let mut cell = String::new();
    let mut chars = text.chars().peekable();
    let mut quoted = false;
    loop {
        let Some(c) = chars.next() else {
            if quoted {
                return Err("csv: unterminated quote".into());
            }
            if !cell.is_empty() || !row.is_empty() {
                row.push(std::mem::take(&mut cell));
                rows.push(std::mem::take(&mut row));
            }
            break;
        };
        match c {
            '"' if quoted => {
                if chars.peek() == Some(&'"') {
                    chars.next();
                    cell.push('"');
                } else {
                    quoted = false;
                }
            }
            '"' if cell.is_empty() => quoted = true,
            ',' if !quoted => row.push(std::mem::take(&mut cell)),
            '\r' if !quoted => {}
            '\n' if !quoted => {
                row.push(std::mem::take(&mut cell));
                rows.push(std::mem::take(&mut row));
            }
            c => cell.push(c),
        }
    }
    if rows.is_empty() {
        return Err("csv: empty".into());
    }
    let header = rows.remove(0);
    for (i, r) in rows.iter().enumerate() {
        if r.len() != header.len() {
            return Err(format!(
                "csv: row {} has {} cells, header {}",
                i + 2,
                r.len(),
                header.len()
            ));
        }
    }
    Ok(Csv { header, rows })
}

/// Display width and signedness come from the DBD; sizeless int columns
/// (non-inline ids/relations) display as signed 32-bit, like DBCD.
fn push_int(line: &mut String, def: &FieldDef, raw: u64) {
    let bits = def.bits.unwrap_or(32);
    // Writing to a String is infallible.
    if def.unsigned {
        let _ = write!(line, "{}", raw & mask(bits));
    } else {
        let _ = write!(line, "{}", sign_extend(raw, bits));
    }
}

fn push_float(line: &mut String, raw: u64) {
    let f = f32::from_bits((raw & 0xFFFF_FFFF) as u32);
    let _ = write!(line, "{f}");
}

// Quote like wago.tools' exporter (PHP fputcsv): spaces and tabs force
// quoting too, so exports diff byte-for-byte.
fn push_str(line: &mut String, s: &str) {
    if s.contains([',', '"', '\n', '\r', ' ', '\t']) {
        line.push('"');
        for ch in s.chars() {
            if ch == '"' {
                line.push('"');
            }
            line.push(ch);
        }
        line.push('"');
    } else {
        line.push_str(s);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn csv_reader_handles_quotes_crlf_and_a_missing_final_newline() {
        let csv = parse_csv("a,b\r\n\"x, y\",\"say \"\"hi\"\"\"\r\n1,2").unwrap();
        assert_eq!(csv.header, ["a", "b"]);
        assert_eq!(
            csv.rows,
            [
                vec!["x, y".to_string(), "say \"hi\"".to_string()],
                vec!["1".to_string(), "2".to_string()]
            ]
        );
        assert_eq!(csv.col("b").unwrap(), 1);
        assert!(
            csv.col("z")
                .unwrap_err()
                .contains("no column \"z\" (have a,b)")
        );
        // A trailing newline adds no empty row; a quote in the middle of a
        // cell is literal.
        let csv = parse_csv("h\nv\"q\n").unwrap();
        assert_eq!(csv.rows, [vec!["v\"q".to_string()]]);
    }

    #[test]
    fn csv_reader_rejections() {
        assert!(
            parse_csv("a,b\n\"open")
                .unwrap_err()
                .contains("unterminated quote")
        );
        assert!(parse_csv("").unwrap_err().contains("empty"));
        assert!(
            parse_csv("a,b\n1\n")
                .unwrap_err()
                .contains("row 2 has 1 cells, header 2")
        );
    }

    /// Every table a generator here reads has its FileDataID, once, in
    /// name order. (The replay's extractor, gen-rubric's drafter among its
    /// generators, reads the rest of `FILE_IDS`: its own test holds the two
    /// crates' lists to it whole.)
    #[test]
    fn every_generators_tables_have_their_file_ids() {
        assert!(FILE_IDS.windows(2).all(|w| w[0].0 < w[1].0));
        let lists: [&[&str]; 12] = [
            &crate::absorbgen::TABLES,
            &crate::artgen::TABLES,
            &crate::artgen::ATLAS_TABLES,
            &crate::classgen::TABLES,
            &crate::itemgen::TABLES,
            &crate::journal::JOURNAL_TABLES,
            &crate::placegen::TABLES,
            &crate::procgen::TABLES,
            &crate::rolegen::TABLES,
            &crate::spelltipgen::TABLES,
            &crate::talentgen::TABLES,
            &[
                crate::icongen::TABLE,
                crate::keystonegen::TABLE,
                crate::mapgen::TABLE,
                crate::spellicongen::TABLE,
            ],
        ];
        for name in lists.iter().flat_map(|l| l.iter()) {
            assert!(file_id(name).is_ok(), "{name}");
        }
        assert_eq!(file_id("SpellEffect"), Ok(1140088));
        assert!(file_id("NoSuchTable").unwrap_err().contains("NoSuchTable"));
    }

    /// The text sink and the cell sink see the same cells: what `parse_csv`
    /// reads back from the text is what the cells hold, for every string
    /// `push_str` quotes or leaves bare, and for a row of one empty cell.
    #[test]
    fn the_cell_sink_reads_as_the_text_parses() {
        let strings = [
            "plain",
            "",
            "two words",
            "say \"hi\"",
            "a,b",
            "cr\rlf\n",
            "tab\there",
            "\"lead",
            "trail\"",
            "Akan\u{f4}s \u{2620}",
        ];
        let rows: Vec<Vec<&str>> = vec![strings.to_vec(), vec![""; strings.len()]];
        let mut out = Vec::new();
        let mut text = Text {
            line: String::new(),
            out: &mut out,
            open: false,
        };
        let mut cells = Cells::default();
        for sink in [&mut text as &mut dyn Sink, &mut cells] {
            sink.row().unwrap();
            for (i, _) in strings.iter().enumerate() {
                if i > 0 {
                    sink.sep();
                }
                sink.name(&format!("C{i}"));
            }
            for row in &rows {
                sink.row().unwrap();
                for (i, s) in row.iter().enumerate() {
                    if i > 0 {
                        sink.sep();
                    }
                    sink.text(s);
                }
            }
        }
        text.finish().unwrap();
        let parsed = parse_csv(std::str::from_utf8(&out).unwrap()).unwrap();
        let mut all = cells.finish();
        let header = all.remove(0);
        assert_eq!(parsed, Csv { header, rows: all });
        assert_eq!(parsed.rows[0], strings);
    }

    #[test]
    fn string_cells_quote_like_fputcsv() {
        let mut line = String::new();
        push_str(&mut line, "plain");
        line.push('|');
        push_str(&mut line, "two words");
        line.push('|');
        push_str(&mut line, "say \"hi\"");
        line.push('|');
        push_str(&mut line, "tab\there");
        assert_eq!(line, "plain|\"two words\"|\"say \"\"hi\"\"\"|\"tab\there\"");
    }
}
