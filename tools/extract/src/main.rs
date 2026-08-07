//! CLI for the local game-data extractor (see USAGE below for the full
//! command set).
//!
//! `csv` needs the matching WoWDBDefs definition (the layout is selected by
//! the file's layout hash); `info` dumps the header and per-field storage
//! layout without a schema, which is the first thing to look at when a
//! table refuses to decode. `fetch` resolves a FileDataID through the local
//! install's CASC storage; `gen-class-spells` runs the whole
//! class-attribution pipeline (tables from the install, rules in
//! classgen.rs) — both network-free. `diffcsv` compares an export against a
//! wago.tools CSV semantically: rows are keyed by the ID column, and cells
//! match on equal text or equal f32 bits — wago re-formats floats through
//! PHP (14 significant digits, ~11 decimals) while we print the shortest
//! round-trip form, so float text differs even when the value is identical.
//! It also tolerates wago's DBCD-inherited `_Index`-style column renames.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use wowdps_extract::{classgen, dbd::Dbd, game::Game, hash, keystonegen, table, tact, wdc5};

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
  wowdps-extract diffcsv <ours.csv> <theirs.csv>
  wowdps-extract fetch <wow-dir> (--fdid N | --file dbfilesclient/x.db2)
                       [-o out] [--keys tactkeys.txt] [--locale enUS]
  wowdps-extract gen-class-spells <wow-dir> --dbd-dir <dir>
                       [-o class_spells.rs] [--keys tactkeys.txt]
  wowdps-extract gen-keystone-timers <wow-dir> --dbd-dir <dir>
                       [-o keystone_timers.rs] [--keys tactkeys.txt]

fetch and the gen-* commands read the local install's CASC storage (no
network): <wow-dir> is the folder containing .build.info and Data/. --keys
takes TACT keys in wowdev TACTKeys format; without a key, encrypted chunks
decode to zeroes exactly like the game client. The gen-* commands expect
--dbd-dir to hold <Table>.dbd schemas for the tables they read (the
tools/gen-*.sh wrappers download them).";

fn run() -> Result<(), String> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("csv") => csv(&args[1..]),
        Some("info") => info(&args[1..]),
        Some("diffcsv") => diffcsv(&args[1..]),
        Some("fetch") => fetch(&args[1..]),
        Some("gen-class-spells") => gen_class_spells(&args[1..]),
        Some("gen-keystone-timers") => gen_keystone_timers(&args[1..]),
        _ => Err(USAGE.into()),
    }
}

/// Shared arguments of the gen-* subcommands.
struct GenArgs {
    wow_dir: PathBuf,
    dbd_dir: PathBuf,
    out_path: String,
    keys_path: Option<PathBuf>,
}

fn gen_args(args: &[String], default_out: &str) -> Result<GenArgs, String> {
    let mut wow_dir = None;
    let mut dbd_dir = None;
    let mut out_path = default_out.to_string();
    let mut keys_path = None;
    let mut it = args.iter();
    while let Some(a) = it.next() {
        let mut next = |what: &str| it.next().cloned().ok_or(format!("{what} needs a value"));
        match a.as_str() {
            "--dbd-dir" => dbd_dir = Some(PathBuf::from(next("--dbd-dir")?)),
            "-o" | "--out" => out_path = next("-o")?,
            "--keys" => keys_path = Some(PathBuf::from(next("--keys")?)),
            _ if wow_dir.is_none() => wow_dir = Some(PathBuf::from(a)),
            other => return Err(format!("unexpected argument {other:?}\n{USAGE}")),
        }
    }
    Ok(GenArgs {
        wow_dir: wow_dir.ok_or(USAGE)?,
        dbd_dir: dbd_dir.ok_or("gen-* commands require --dbd-dir")?,
        out_path,
        keys_path,
    })
}

/// Fetch one table from the install and decode it to cells.
fn load_table(game: &Game, dbd_dir: &Path, name: &str, fdid: u32) -> Result<table::Csv, String> {
    let db2 = wdc5::Db2::parse(game.fetch_fdid(fdid, 0x2)?).map_err(|e| format!("{name}: {e}"))?;
    let dbd_path = dbd_dir.join(format!("{name}.dbd"));
    let dbd_text =
        std::fs::read_to_string(&dbd_path).map_err(|e| format!("{}: {e}", dbd_path.display()))?;
    let dbd = Dbd::parse(&dbd_text).map_err(|e| format!("{name}: {e}"))?;
    let mut out = Vec::new();
    table::write_csv(&db2, &dbd, &mut out).map_err(|e| format!("{name}: {e}"))?;
    let text = String::from_utf8(out).map_err(|e| format!("{name}: {e}"))?;
    let csv = table::parse_csv(&text).map_err(|e| format!("{name}: {e}"))?;
    eprintln!("{name}: {} rows", csv.rows.len());
    Ok(csv)
}

fn gen_class_spells(args: &[String]) -> Result<(), String> {
    let a = gen_args(args, "crates/core/src/class_spells.rs")?;
    let game = Game::open(&a.wow_dir, a.keys_path.as_deref())?;
    let mut tables = std::collections::HashMap::new();
    for (name, fdid) in classgen::TABLES {
        tables.insert(name, load_table(&game, &a.dbd_dir, name, fdid)?);
    }

    let g = classgen::generate(&tables, &game.build)?;
    std::fs::write(&a.out_path, &g.content).map_err(|e| format!("{}: {e}", a.out_path))?;
    eprintln!(
        "{}: {} spells ({} spec-unique), {} ambiguous dropped",
        a.out_path, g.spells, g.spec_unique, g.ambiguous
    );
    Ok(())
}

fn gen_keystone_timers(args: &[String]) -> Result<(), String> {
    let a = gen_args(args, "crates/core/src/keystone_timers.rs")?;
    let game = Game::open(&a.wow_dir, a.keys_path.as_deref())?;
    let (name, fdid) = keystonegen::TABLE;
    let csv = load_table(&game, &a.dbd_dir, name, fdid)?;

    let g = keystonegen::generate(&csv, &game.build)?;
    std::fs::write(&a.out_path, &g.content).map_err(|e| format!("{}: {e}", a.out_path))?;
    eprintln!(
        "{}: {} dungeons, build {}",
        a.out_path, g.dungeons, game.build
    );
    Ok(())
}

fn fetch(args: &[String]) -> Result<(), String> {
    let mut wow_dir = None;
    let mut fdid = None;
    let mut file = None;
    let mut out_path = None;
    let mut keys_path = None;
    let mut locale = 0x2u32; // enUS
    let mut it = args.iter();
    while let Some(a) = it.next() {
        let mut next = |what: &str| it.next().cloned().ok_or(format!("{what} needs a value"));
        match a.as_str() {
            "--fdid" => fdid = Some(next("--fdid")?.parse::<u32>().map_err(|e| e.to_string())?),
            "--file" => file = Some(next("--file")?),
            "-o" | "--out" => out_path = Some(next("-o")?),
            "--keys" => keys_path = Some(next("--keys")?),
            "--locale" => locale = tact::locale_mask(&next("--locale")?)?,
            _ if wow_dir.is_none() => wow_dir = Some(a.clone()),
            other => return Err(format!("unexpected argument {other:?}\n{USAGE}")),
        }
    }
    let wow_dir = wow_dir.map(PathBuf::from).ok_or(USAGE)?;
    if fdid.is_none() && file.is_none() {
        return Err("fetch requires --fdid or --file".into());
    }

    let game = Game::open(&wow_dir, keys_path.as_deref().map(Path::new))?;
    let name_hash = file.as_deref().map(hash::name_hash);
    let m = tact::root_find(game.root(), fdid, name_hash, locale)?.ok_or_else(|| {
        let what = file
            .clone()
            .unwrap_or_else(|| format!("fdid {}", fdid.unwrap()));
        format!(
            "{what} not found in root manifest (a named lookup fails if the \
             file's block has no name hashes — try --fdid)"
        )
    })?;
    eprintln!(
        "fdid {} locale {} content {:#x}",
        m.fdid,
        tact::describe_locale(m.locale),
        m.content
    );
    let bytes = game.fetch_ckey(&m.ckey)?;

    match out_path {
        Some(p) => {
            std::fs::write(&p, &bytes).map_err(|e| format!("{p}: {e}"))?;
            eprintln!("{p}: {} bytes", bytes.len());
            Ok(())
        }
        None => std::io::stdout()
            .write_all(&bytes)
            .map_err(|e| e.to_string()),
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

fn read_csv(path: &str) -> Result<(Vec<String>, Vec<Vec<String>>), String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("{path}: {e}"))?;
    let csv = table::parse_csv(&text).map_err(|e| format!("{path}: {e}"))?;
    Ok((csv.header, csv.rows))
}

fn load_db2(path: &str) -> Result<wdc5::Db2, String> {
    let data = std::fs::read(path).map_err(|e| format!("{path}: {e}"))?;
    wdc5::Db2::parse(data).map_err(|e| format!("{path}: {e}"))
}
