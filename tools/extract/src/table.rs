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
    Compression, Db2, FLAG_SPARSE, Row, StorageInfo, bitpacked_value, cstr, elem_bits,
};
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

pub fn write_csv(db2: &Db2, dbd: &Dbd, out: &mut dyn std::io::Write) -> Result<(), String> {
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

    let mut line = String::new();
    for (i, col) in cols.iter().enumerate() {
        if i > 0 {
            line.push(',');
        }
        // Array columns expand the way wago.tools exports them: Name_0, Name_1…
        match col.def.array {
            Some(n) => {
                for j in 0..n {
                    if j > 0 {
                        line.push(',');
                    }
                    // Writing to a String is infallible.
                    let _ = write!(line, "{}_{j}", col.def.name);
                }
            }
            None => line.push_str(&col.def.name),
        }
    }
    line.push('\n');
    out.write_all(line.as_bytes()).map_err(|e| e.to_string())?;

    let mut order: Vec<&Row> = db2.rows.iter().collect();
    order.sort_by_key(|r| r.id);

    let sparse = db2.header.flags & FLAG_SPARSE != 0;
    for row in order {
        line.clear();
        if sparse {
            sparse_row(db2, &cols, row, &mut line)?;
        } else {
            packed_row(db2, &cols, row, &mut line)?;
        }
        line.push('\n');
        out.write_all(line.as_bytes()).map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// Fixed-size records: every storage field is read at its storage-info bit
/// offset, so column decode order is independent.
fn packed_row(db2: &Db2, cols: &[Col], row: &Row, line: &mut String) -> Result<(), String> {
    let rec = db2.record(row);
    for (ci, col) in cols.iter().enumerate() {
        if ci > 0 {
            line.push(',');
        }
        let k = match col.source {
            Source::RowId => {
                push_int(line, col.def, row.id as u64);
                continue;
            }
            Source::Foreign => {
                push_int(line, col.def, row.foreign.unwrap_or(0) as u64);
                continue;
            }
            Source::Inline(k) => k,
        };
        if col.def.is_id {
            // Inline id: display the resolved row id (copy-table rows must
            // show their own id, not the copied record's).
            push_int(line, col.def, row.id as u64);
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
                        line.push(',');
                    }
                    let bit = info.offset_bits as usize + (j * elem) as usize;
                    let raw = read_bits(rec, bit, elem);
                    match col.ty {
                        ColType::Str | ColType::LocStr => {
                            let rel = sign_extend(raw, elem);
                            let s = db2.string_at(row.global, (bit / 8) as u32, rel)?;
                            push_str(line, s);
                        }
                        ColType::Float => push_float(line, raw),
                        ColType::Int => push_int(line, col.def, raw),
                    }
                }
            }
            Compression::Bitpacked | Compression::BitpackedSigned => {
                require_int(col)?;
                push_int(line, col.def, bitpacked_value(rec, info));
            }
            Compression::CommonData => {
                let v = db2
                    .commons
                    .get(k)
                    .and_then(|m| m.get(&row.id))
                    .copied()
                    .unwrap_or(info.args[0]);
                match col.ty {
                    ColType::Float => push_float(line, v as u64),
                    _ => push_int(line, col.def, v as u64),
                }
            }
            Compression::PalletIndexed => {
                let idx = read_bits(rec, info.offset_bits as usize, info.size_bits as u32);
                let v = pallet_get(db2, k, idx as usize, col)?;
                match col.ty {
                    ColType::Float => push_float(line, v as u64),
                    _ => push_int(line, col.def, v as u64),
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
                        line.push(',');
                    }
                    let v = pallet_get(db2, k, (idx as u32 * card + j) as usize, col)?;
                    match col.ty {
                        ColType::Float => push_float(line, v as u64),
                        _ => push_int(line, col.def, v as u64),
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
fn sparse_row(db2: &Db2, cols: &[Col], row: &Row, line: &mut String) -> Result<(), String> {
    let rec = db2.record(row);
    let mut bit = 0usize;
    for (ci, col) in cols.iter().enumerate() {
        if ci > 0 {
            line.push(',');
        }
        let k = match col.source {
            Source::RowId => {
                push_int(line, col.def, row.id as u64);
                continue;
            }
            Source::Foreign => {
                push_int(line, col.def, row.foreign.unwrap_or(0) as u64);
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
                let s = cstr(rec.get(start.min(rec.len())..).unwrap_or(&[][..]))?;
                let owned = s.to_string();
                bit = (start + owned.len() + 1) * 8;
                if col.def.is_id {
                    return Err("sparse string id field".into());
                }
                push_str(line, &owned);
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
                        line.push(',');
                    }
                    let raw = read_bits(rec, bit, elem);
                    bit += elem as usize;
                    if col.def.is_id {
                        push_int(line, col.def, row.id as u64);
                    } else if col.ty == ColType::Float {
                        push_float(line, raw);
                    } else {
                        push_int(line, col.def, raw);
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
