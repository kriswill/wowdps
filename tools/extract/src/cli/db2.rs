//! The client's tables and files as they are: `fetch` (any file of the
//! install's storage), `csv` and `info` (a `.db2` decoded, or its layout),
//! `diffcsv` (an export against wago.tools'), and `sql` (the game's tables
//! in DuckDB, cached per build: wowdps-db2).
//!
//! `csv` needs the matching WoWDBDefs definition (the layout is selected by
//! the file's layout hash); `info` dumps the header and per-field storage
//! layout without a schema, which is the first thing to look at when a
//! table refuses to decode. `fetch` resolves a FileDataID through the local
//! install's CASC storage, network-free. `diffcsv` compares an export
//! against a wago.tools CSV semantically: rows are keyed by the ID column,
//! and cells match on equal text or equal f32 bits — wago re-formats floats
//! through PHP (14 significant digits, ~11 decimals) while we print the
//! shortest round-trip form, so float text differs even when the value is
//! identical. It also tolerates wago's DBCD-inherited `_Index`-style column
//! renames.

use std::io::Write;
use std::path::Path;

use super::args::Args;
use super::install::{Install, fetch_db2, read_dbd};
use super::{Cmd, at, cmd, make_dir, read_text};
use wowdps_extract::atomic::write_atomic;
use wowdps_extract::dbd::Dbd;
use wowdps_extract::game::Game;
use wowdps_extract::{hash, table, tact, wdc5};

/// This module's commands, as the usage lists them.
pub const COMMANDS: &[Cmd] = &[
    cmd("csv", "csv <table.db2> --dbd <table.dbd> [-o out.csv]", csv),
    cmd("info", "info <table.db2>", info),
    cmd("diffcsv", "diffcsv <ours.csv> <theirs.csv>", diffcsv),
    cmd(
        "fetch",
        "fetch [wow-dir] (--fdid N | --file dbfilesclient/x.db2)
                       [-o out] [--keys tactkeys.txt] [--locale enUS]",
        fetch,
    ),
    cmd(
        "sql",
        "sql [Table...] [-c SQL] [--find WORD] [--tables] [--csv]
                       [--refresh] [--wow wow-dir]
                       (wowdps-db2: the game's tables in DuckDB, cached per build)",
        sql,
    ),
];

pub fn fetch(mut a: Args) -> Result<(), String> {
    let fdid = a.parse::<u32>(&["--fdid"])?;
    let file = a.opt(&["--file"])?;
    let out_path = a.opt(&["-o", "--out"])?;
    let locale = match a.opt(&["--locale"])? {
        Some(l) => tact::locale_mask(&l)?,
        None => 0x2, // enUS
    };
    let install = Install::files(&mut a)?;
    a.finish()?;
    if fdid.is_none() && file.is_none() {
        return Err("fetch requires --fdid or --file".into());
    }

    let game = install.open()?;
    let name_hash = file.as_deref().map(hash::name_hash);
    let m = tact::root_find(game.root(), fdid, name_hash, locale)?.ok_or_else(|| {
        // One of --file / --fdid is always set by the time we get here.
        let what = file.clone().unwrap_or_else(|| match fdid {
            Some(id) => format!("fdid {id}"),
            None => "file".to_string(),
        });
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
            write_atomic(Path::new(&p), &bytes)?;
            eprintln!("{p}: {} bytes", bytes.len());
            Ok(())
        }
        None => std::io::stdout()
            .write_all(&bytes)
            .map_err(|e| e.to_string()),
    }
}

pub fn csv(mut a: Args) -> Result<(), String> {
    let dbd_path = a.path(&["--dbd"])?;
    let out_path = a.path(&["-o", "--out"])?;
    let db2_path = a.positional();
    a.finish()?;
    let db2_path = db2_path.ok_or("csv needs <table.db2>")?;
    let dbd_path = dbd_path.ok_or("csv requires --dbd <table.dbd>")?;

    let db2 = load_db2(Path::new(&db2_path))?;
    let dbd_text = read_text(&dbd_path)?;
    let dbd = Dbd::parse(&dbd_text)?;

    match out_path {
        Some(p) => {
            let f = std::fs::File::create(&p).map_err(at(&p))?;
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

pub fn info(mut a: Args) -> Result<(), String> {
    let path = a.positional();
    a.finish()?;
    let path = path.ok_or("info needs <table.db2>")?;
    let db2 = load_db2(Path::new(&path))?;
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

pub fn diffcsv(mut args: Args) -> Result<(), String> {
    let paths = args.positionals();
    args.finish()?;
    let [a_path, b_path] = &paths[..] else {
        return Err("diffcsv needs <ours.csv> <theirs.csv>".into());
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
                    let cell = r
                        .get(id_col)
                        .ok_or_else(|| format!("row has no ID column {id_col}"))?;
                    let id = cell
                        .parse::<i64>()
                        .map_err(|_| format!("bad id {cell:?}"))?;
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
                        eprintln!(
                            "row {id} col {}: {x:?} vs {y:?}",
                            a.0.get(ci).map_or("?", String::as_str)
                        );
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
    let text = read_text(Path::new(path))?;
    let csv = table::parse_csv(&text).map_err(|e| format!("{path}: {e}"))?;
    Ok((csv.header, csv.rows))
}

fn load_db2(path: &Path) -> Result<wdc5::Db2, String> {
    let data = std::fs::read(path).map_err(at(path))?;
    wdc5::Db2::parse(data).map_err(at(path))
}

/// The game's tables as SQL (db2sql.rs): each table named is decoded from
/// the install into the build's cache once, then DuckDB opens in memory
/// with a view per cached table — interactive, or `-c SQL` and out.
pub fn sql(mut a: Args) -> Result<(), String> {
    use wowdps_extract::db2sql;
    let query = a.opt(&["-c", "--command"])?;
    let find = a.opt(&["--find"])?;
    let wow_dir = a.path(&["--wow"])?;
    let refresh = a.flag(&["--refresh"]);
    let list = a.flag(&["--tables"]);
    let csv_paths = a.flag(&["--csv"]);
    let names: Vec<String> = a
        .positionals()
        .iter()
        .flat_map(|name| name.split(',').filter(|n| !n.is_empty()).map(String::from))
        .collect();
    a.finish()?;
    let root = db2sql::cache_root()?;

    // The manifest: fetched when missing, a week old, asked to be, or
    // missing a table asked for.
    let manifest_path = root.join("manifest.json");
    let read_manifest = || -> Result<db2sql::Manifest, String> {
        std::fs::read_to_string(&manifest_path)
            .map(|t| db2sql::Manifest::parse(&t))
            .map_err(at(&manifest_path))
    };
    if refresh || db2sql::stale(&manifest_path, db2sql::MAX_AGE) {
        db2sql::fetch(db2sql::MANIFEST_URL, &manifest_path)?;
    }
    let mut manifest = read_manifest()?;
    if names.iter().any(|n| manifest.find(n).is_none()) && !refresh {
        db2sql::fetch(db2sql::MANIFEST_URL, &manifest_path)?;
        manifest = read_manifest()?;
    }
    if let Some(word) = find {
        for (name, fdid) in manifest.search(&word) {
            println!("{name}\t{fdid}");
        }
        return Ok(());
    }
    let tables: Vec<(String, u32)> = names
        .iter()
        .map(|n| {
            manifest
                .find(n)
                .map(|(name, fdid)| (name.to_string(), fdid))
                .ok_or(format!(
                    "no table {n:?} in WoWDBDefs' manifest (wowdps-extract sql --find WORD)"
                ))
        })
        .collect::<Result<_, _>>()?;

    // The build the install holds: its tables' place in the cache.
    let wow_dir = match wow_dir {
        Some(p) => p,
        None => wowdps_extract::game::locate()?,
    };
    let build = wowdps_extract::game::build_info(&wow_dir)?.version;
    let build_dir = db2sql::build_dir(&root, &build);
    let (csv_dir, pq_dir) = (build_dir.join("csv"), build_dir.join("parquet"));
    for d in [&csv_dir, &pq_dir] {
        make_dir(d)?;
    }

    // Each table not yet in the cache (or every one named, refreshed).
    let duckdb = |sql: &str| -> Result<(), String> {
        let out = std::process::Command::new("duckdb")
            .args(["-batch", "-c", sql])
            .output()
            .map_err(|e| format!("duckdb: {e} (a duckdb CLI must be on PATH)"))?;
        if out.status.success() {
            Ok(())
        } else {
            Err(format!(
                "duckdb: {}",
                String::from_utf8_lossy(&out.stderr).trim()
            ))
        }
    };
    let keys = root.join("tactkeys.txt");
    let mut game: Option<Game> = None;
    for (name, fdid) in &tables {
        let csv = csv_dir.join(format!("{name}.csv"));
        let parquet = pq_dir.join(format!("{name}.parquet"));
        if !refresh && csv.exists() && parquet.exists() {
            continue;
        }
        if game.is_none() {
            if refresh || db2sql::stale(&keys, db2sql::MAX_AGE) {
                db2sql::fetch(db2sql::KEYS_URL, &keys)?;
            }
            game = Some(Game::open(&wow_dir, Some(&keys))?);
        }
        let Some(g) = &game else {
            continue;
        };
        let db2 = fetch_db2(g, name, *fdid)?;
        // The schema; fetched again once if this build's layout is not in
        // the copy kept.
        let dbd_path = root.join("dbd").join(format!("{name}.dbd"));
        let fresh_dbd = refresh || !dbd_path.exists();
        if fresh_dbd {
            db2sql::fetch(&format!("{}{name}.dbd", db2sql::DBD_URL), &dbd_path)?;
        }
        let decode = |dbd_path: &Path| -> Result<Vec<u8>, String> {
            let dbd = read_dbd(dbd_path, name)?;
            let mut out = Vec::new();
            table::write_csv(&db2, &dbd, &mut out).map_err(|e| format!("{name}: {e}"))?;
            Ok(out)
        };
        let bytes = match decode(&dbd_path) {
            Ok(b) => b,
            Err(_) if !fresh_dbd => {
                db2sql::fetch(&format!("{}{name}.dbd", db2sql::DBD_URL), &dbd_path)?;
                decode(&dbd_path)?
            }
            Err(e) => return Err(e),
        };
        write_atomic(&csv, &bytes)?;
        let tmp = wowdps_extract::atomic::tmp_beside(&parquet);
        if let Err(e) = duckdb(&db2sql::to_parquet(&csv, &tmp)) {
            let _ = std::fs::remove_file(&tmp);
            return Err(e);
        }
        wowdps_extract::atomic::rename_into(&tmp, &parquet)?;
        eprintln!("{name}: {} (build {build})", csv.display());
    }

    let cached = db2sql::cached(&build_dir);
    if csv_paths {
        // The tables named, else every one cached.
        let names: Vec<&str> = if tables.is_empty() {
            cached.iter().map(|c| c.0.as_str()).collect()
        } else {
            tables.iter().map(|t| t.0.as_str()).collect()
        };
        for name in names {
            println!("{}", csv_dir.join(format!("{name}.csv")).display());
        }
        return Ok(());
    }
    if list {
        for (name, _) in &cached {
            println!("{name}");
        }
        return Ok(());
    }
    if cached.is_empty() {
        return Err(format!(
            "build {build} has no table cached yet: name one (wowdps-extract sql Map)"
        ));
    }
    let mut cmd = std::process::Command::new("duckdb");
    cmd.args(["-cmd", &db2sql::views(&cached)]);
    if let Some(q) = query {
        cmd.args(["-c", &q]);
    } else {
        eprintln!(
            "build {build}: {} table(s) as views ({})",
            cached.len(),
            cached
                .iter()
                .map(|c| c.0.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        );
    }
    // The shell is the user's now (exec: no child to wait on).
    use std::os::unix::process::CommandExt;
    Err(format!(
        "duckdb: {} (a duckdb CLI must be on PATH)",
        cmd.exec()
    ))
}
