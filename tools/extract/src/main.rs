//! CLI for the local game-data extractor.
//!
//!   wowdps-extract csv <table.db2> --dbd <table.dbd> [-o out.csv]
//!   wowdps-extract info <table.db2>
//!   wowdps-extract diffcsv <ours.csv> <theirs.csv>
//!
//! `csv` needs the matching WoWDBDefs definition (the layout is selected by
//! the file's layout hash); `info` dumps the header and per-field storage
//! layout without a schema, which is the first thing to look at when a
//! table refuses to decode. `diffcsv` compares an export against a
//! wago.tools CSV semantically: rows are keyed by the ID column, and cells
//! match on equal text or equal f32 bits — wago re-formats floats through
//! PHP (14 significant digits, ~11 decimals) while we print the shortest
//! round-trip form, so float text differs even when the value is identical.
//! It also tolerates wago's DBCD-inherited `_Index`-style column renames.

use std::io::Write;
use std::process::ExitCode;
use wowdps_extract::{dbd::Dbd, table, wdc5};

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("wowdps-extract: {e}");
            ExitCode::FAILURE
        }
    }
}

const USAGE: &str = "usage:
  wowdps-extract csv <table.db2> --dbd <table.dbd> [-o out.csv]
  wowdps-extract info <table.db2>
  wowdps-extract diffcsv <ours.csv> <theirs.csv>";

fn run() -> Result<(), String> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("csv") => csv(&args[1..]),
        Some("info") => info(&args[1..]),
        Some("diffcsv") => diffcsv(&args[1..]),
        _ => Err(USAGE.into()),
    }
}

fn csv(args: &[String]) -> Result<(), String> {
    let mut db2_path = None;
    let mut dbd_path = None;
    let mut out_path = None;
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--dbd" => dbd_path = Some(it.next().ok_or("--dbd needs a path")?.clone()),
            "-o" | "--out" => out_path = Some(it.next().ok_or("-o needs a path")?.clone()),
            _ if db2_path.is_none() => db2_path = Some(a.clone()),
            other => return Err(format!("unexpected argument {other:?}\n{USAGE}")),
        }
    }
    let db2_path = db2_path.ok_or(USAGE)?;
    let dbd_path = dbd_path.ok_or("csv requires --dbd <table.dbd>")?;

    let db2 = load_db2(&db2_path)?;
    let dbd_text = std::fs::read_to_string(&dbd_path).map_err(|e| format!("{dbd_path}: {e}"))?;
    let dbd = Dbd::parse(&dbd_text)?;

    match out_path {
        Some(p) => {
            let f = std::fs::File::create(&p).map_err(|e| format!("{p}: {e}"))?;
            let mut w = std::io::BufWriter::new(f);
            table::write_csv(&db2, &dbd, &mut w)?;
            w.flush().map_err(|e| e.to_string())
        }
        None => {
            let stdout = std::io::stdout();
            let mut w = std::io::BufWriter::new(stdout.lock());
            table::write_csv(&db2, &dbd, &mut w)?;
            w.flush().map_err(|e| e.to_string())
        }
    }
}

fn info(args: &[String]) -> Result<(), String> {
    let [path] = args else {
        return Err(USAGE.into());
    };
    let db2 = load_db2(path)?;
    let h = &db2.header;
    println!("schema        {} (v{})", h.schema, h.version);
    println!("table_hash    {:08X}", h.table_hash);
    println!("layout_hash   {:08X}", h.layout_hash);
    println!(
        "records       {} ({} after copy table)",
        h.record_count,
        db2.rows.len()
    );
    println!(
        "fields        {} ({} bytes/record)",
        h.field_count, h.record_size
    );
    println!("ids           {}..{}", h.min_id, h.max_id);
    println!(
        "locale        {:#x}   flags {:#06x}   id_index {}",
        h.locale, h.flags, h.id_index
    );
    println!("sections      {}", h.section_count);
    for (i, s) in db2.sections.iter().enumerate() {
        println!(
            "  [{i}] offset {} records {} strings {} ids {} copies {} rel {} sparse-ids {}{}",
            s.file_offset,
            s.record_count,
            s.string_table_size,
            s.id_list_size / 4,
            s.copy_table_count,
            s.relationship_data_size,
            s.offset_map_id_count,
            if s.tact_key_hash != 0 {
                "  [encrypted]"
            } else {
                ""
            },
        );
    }
    println!("storage fields:");
    for (i, f) in db2.infos.iter().enumerate() {
        println!(
            "  [{i:2}] {:?} offset {:4} bits {:3} extra {:6} args {:?}",
            f.compression, f.offset_bits, f.size_bits, f.additional_data_size, f.args
        );
    }
    Ok(())
}

fn diffcsv(args: &[String]) -> Result<(), String> {
    let [a_path, b_path] = args else {
        return Err(USAGE.into());
    };
    let a = read_csv(a_path)?;
    let b = read_csv(b_path)?;

    // wago (via DBCD) renames columns that collide with C# members, e.g.
    // Index -> _Index; compare names with leading underscores stripped.
    let norm = |h: &[String]| -> Vec<String> {
        h.iter()
            .map(|c| c.trim_start_matches('_').to_string())
            .collect()
    };
    if norm(&a.0) != norm(&b.0) {
        return Err(format!(
            "headers differ:\n  {}\n  {}",
            a.0.join(","),
            b.0.join(",")
        ));
    }
    let id_col = norm(&a.0)
        .iter()
        .position(|c| c == "ID")
        .ok_or("no ID column; cannot key rows")?;

    let key =
        |rows: &[Vec<String>]| -> Result<std::collections::HashMap<i64, Vec<String>>, String> {
            rows.iter()
                .map(|r| {
                    let id = r[id_col]
                        .parse::<i64>()
                        .map_err(|_| format!("bad id {:?}", r[id_col]))?;
                    Ok((id, r.clone()))
                })
                .collect()
        };
    let (am, bm) = (key(&a.1)?, key(&b.1)?);

    let mut bad = 0u64;
    for (id, ar) in &am {
        match bm.get(id) {
            None => {
                bad += 1;
                eprintln!("row {id}: only in {a_path}");
            }
            Some(br) => {
                for (ci, (x, y)) in ar.iter().zip(br).enumerate() {
                    if x != y && !float_eq(x, y) {
                        bad += 1;
                        eprintln!("row {id} col {}: {x:?} vs {y:?}", a.0[ci]);
                    }
                }
            }
        }
    }
    for id in bm.keys() {
        if !am.contains_key(id) {
            bad += 1;
            eprintln!("row {id}: only in {b_path}");
        }
    }

    if bad > 0 {
        return Err(format!("{bad} differences across {} rows", am.len()));
    }
    println!(
        "identical: {} rows, {} columns (floats compared by f32 bits)",
        am.len(),
        a.0.len()
    );
    Ok(())
}

/// Numeric cell equality. wago formats floats as `sprintf('%.14G',
/// round($v, 11))`, so most of their text parses back to the identical f32;
/// values below ~1e-7 lose real precision to the round-at-11-decimals step,
/// hence the 5e-12 absolute tolerance (that rounding's exact error bound).
/// NaN never matches, which is fine for table data.
fn float_eq(x: &str, y: &str) -> bool {
    if let (Ok(a), Ok(b)) = (x.parse::<f32>(), y.parse::<f32>())
        && a.to_bits() == b.to_bits()
    {
        return true;
    }
    // The tolerance must compare the doubles as written: re-parsing wago's
    // text through f32 first would quantize it by more than the bound.
    let (Ok(a), Ok(b)) = (x.parse::<f64>(), y.parse::<f64>()) else {
        return false;
    };
    // Half-way values sit exactly on the 5e-12 rounding bound; leave room
    // for that and for the subtraction's own float noise.
    (a - b).abs() < 5.1e-12
}

/// Minimal RFC-4180-ish reader (quotes, escaped quotes, CRLF); enough for
/// our own output and wago exports.
fn read_csv(path: &str) -> Result<(Vec<String>, Vec<Vec<String>>), String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("{path}: {e}"))?;
    let mut rows: Vec<Vec<String>> = Vec::new();
    let mut row: Vec<String> = Vec::new();
    let mut cell = String::new();
    let mut chars = text.chars().peekable();
    let mut quoted = false;
    loop {
        let Some(c) = chars.next() else {
            if quoted {
                return Err(format!("{path}: unterminated quote"));
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
        return Err(format!("{path}: empty"));
    }
    let header = rows.remove(0);
    for (i, r) in rows.iter().enumerate() {
        if r.len() != header.len() {
            return Err(format!(
                "{path}: row {} has {} cells, header {}",
                i + 2,
                r.len(),
                header.len()
            ));
        }
    }
    Ok((header, rows))
}

fn load_db2(path: &str) -> Result<wdc5::Db2, String> {
    let data = std::fs::read(path).map_err(|e| format!("{path}: {e}"))?;
    wdc5::Db2::parse(data).map_err(|e| format!("{path}: {e}"))
}
