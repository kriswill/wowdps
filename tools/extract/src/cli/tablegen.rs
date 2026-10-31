//! The committed tables the meter reads, generated from the install:
//! `gen-class-spells`, `gen-item-spells`, `gen-keystone-timers` and
//! `gen-open-world-maps`, and the census-backed `gen-role-spells`,
//! `gen-absorb-spells` and `gen-proc-spells` with their review twins — each
//! through one runner (`generate`) — and the proc pairs the proc census
//! reads (`proc-pairs`).

use std::path::{Path, PathBuf};

use super::args::Args;
use super::install::Install;
use super::{Cmd, cmd, read_text};
use wowdps_extract::atomic::write_if_changed;
use wowdps_extract::table::Tables;
use wowdps_extract::{
    absorbgen, classgen, itemgen, keystonegen, mapgen, placegen, procgen, rolegen,
};

/// This module's commands, as the usage lists them.
pub const COMMANDS: &[Cmd] = &[
    cmd(
        "gen-class-spells",
        "gen-class-spells [wow-dir] --dbd-dir <dir>
                       [-o class_spells.rs] [--keys tactkeys.txt]",
        class_spells,
    )
    .reads(&[CLASS_SPELLS.tables]),
    cmd(
        "gen-keystone-timers",
        "gen-keystone-timers [wow-dir] --dbd-dir <dir>
                       [-o keystone_timers.rs] [--keys tactkeys.txt]",
        keystone_timers,
    )
    .reads(&[KEYSTONE_TIMERS.tables]),
    cmd(
        "gen-open-world-maps",
        "gen-open-world-maps [wow-dir] --dbd-dir <dir>
                       [-o open_world_maps.rs] [--keys tactkeys.txt]",
        open_world_maps,
    )
    .reads(&[OPEN_WORLD_MAPS.tables]),
    cmd(
        "gen-item-spells",
        "gen-item-spells [wow-dir] --dbd-dir <dir>
                       [-o item_spells.rs] [--keys tactkeys.txt]",
        item_spells,
    )
    .reads(&[ITEM_SPELLS.tables]),
    cmd(
        "gen-role-spells",
        "gen-role-spells [wow-dir] --dbd-dir <dir> --census <csv>
                       [-o role_spells.rs] [--keys tactkeys.txt]",
        role_spells,
    )
    .reads(&[ROLE_SPELLS.tables]),
    cmd(
        "gen-absorb-spells",
        "gen-absorb-spells [wow-dir] --dbd-dir <dir> --census <csv>
                       [-o absorb_spells.rs] [--keys tactkeys.txt]",
        absorb_spells,
    )
    .reads(&[ABSORB_SPELLS.tables]),
    cmd(
        "gen-proc-spells",
        "gen-proc-spells [wow-dir] --dbd-dir <dir> --census <csv>
                       [-o proc_spells.rs] [--keys tactkeys.txt]",
        proc_spells,
    )
    .reads(&[PROC_SPELLS.tables]),
    cmd(
        "gen-placed-spells",
        "gen-placed-spells [wow-dir] --dbd-dir <dir> --census <csv>
                       [-o placed_spells.rs] [--keys tactkeys.txt]",
        placed_spells,
    )
    .reads(&[PLACED_SPELLS.tables]),
    cmd(
        "placed-spells-ids",
        "placed-spells-ids   (every spell the placed-spell census counts)",
        placed_spells_ids,
    ),
    cmd(
        "proc-pairs",
        "proc-pairs     (the curated proc/driver pairs, for the census)",
        proc_pairs,
    ),
];

/// A table generator: its command, where it writes the table unless `-o`
/// says, the client tables it reads, and, where a real-log census backs
/// it, the census `--census` names (the committed one, for the error that
/// asks for it).
struct Table {
    command: &'static str,
    out: &'static str,
    tables: &'static [&'static str],
    census: Option<&'static str>,
}

const CLASS_SPELLS: Table = Table {
    command: "gen-class-spells",
    out: "crates/core/src/class_spells.rs",
    tables: &classgen::TABLES,
    census: None,
};

const ITEM_SPELLS: Table = Table {
    command: "gen-item-spells",
    out: "crates/core/src/item_spells.rs",
    tables: &itemgen::TABLES,
    census: None,
};

const KEYSTONE_TIMERS: Table = Table {
    command: "gen-keystone-timers",
    out: "crates/core/src/keystone_timers.rs",
    tables: &[keystonegen::TABLE],
    census: None,
};

const OPEN_WORLD_MAPS: Table = Table {
    command: "gen-open-world-maps",
    out: "crates/core/src/open_world_maps.rs",
    tables: &[mapgen::TABLE],
    census: None,
};

const ROLE_SPELLS: Table = Table {
    command: "gen-role-spells",
    out: "crates/core/src/role_spells.rs",
    tables: &rolegen::TABLES,
    census: Some("tools/role-spells-census.csv"),
};

const ABSORB_SPELLS: Table = Table {
    command: "gen-absorb-spells",
    out: "crates/core/src/absorb_spells.rs",
    tables: &absorbgen::TABLES,
    census: Some("tools/absorb-spells-census.csv"),
};

const PROC_SPELLS: Table = Table {
    command: "gen-proc-spells",
    out: "crates/core/src/proc_spells.rs",
    tables: &procgen::TABLES,
    census: Some("tools/proc-spells-census.csv"),
};

const PLACED_SPELLS: Table = Table {
    command: "gen-placed-spells",
    out: "crates/proto/src/placed/placed_spells.rs",
    tables: &placegen::TABLES,
    census: Some("tools/placed-spells-census.csv"),
};

/// What a generator made: the table's text, its review twin's where a
/// census backs it, and the summary's words after the table's path.
struct Made {
    content: String,
    expected: Option<String>,
    summary: String,
}

/// Run a table generator: its arguments taken; the census read and handed
/// to `prepare` before the install is opened, so a census that does not
/// read stops it first; the client tables read; the generator `prepare`
/// returned called with them and the build; and the table written where
/// its bytes change, its review twin beside it.
fn generate<G>(
    mut a: Args,
    t: Table,
    prepare: impl FnOnce(&str) -> Result<G, String>,
) -> Result<(), String>
where
    G: FnOnce(&Tables, &str) -> Result<Made, String>,
{
    let out = a
        .path(&["-o", "--out"])?
        .unwrap_or_else(|| PathBuf::from(t.out));
    let census_path = match t.census {
        Some(_) => a.path(&["--census"])?,
        None => None,
    };
    let install = Install::with_tables(&mut a)?;
    a.finish()?;
    let census = match (t.census, census_path) {
        (Some(_), Some(path)) => read_text(&path)?,
        (Some(committed), None) => {
            return Err(format!("{} requires --census <{committed}>", t.command));
        }
        (None, _) => String::new(),
    };
    let make = prepare(&census)?;
    let (game, tables) = install.open_tables()?;

    let made = make(&tables, &game.build)?;
    let note = put(&out, &made.content)?;
    let twin = match &made.expected {
        Some(expected) => {
            let path = review_twin(&out);
            let note = put(&path, expected)?;
            format!(" -> {}{note}", path.display())
        }
        None => String::new(),
    };
    eprintln!("{}{note}: {}{twin}", out.display(), made.summary);
    Ok(())
}

/// Write a generated repository file only when its bytes change (so cargo
/// rebuilds nothing for a regenerate that changed nothing), and the word
/// the summary line gives it: nothing, or " (unchanged)".
fn put(path: &Path, text: &str) -> Result<&'static str, String> {
    Ok(if write_if_changed(path, text)? {
        ""
    } else {
        " (unchanged)"
    })
}

/// A generated table's review twin beside it: `role_spells.rs` →
/// `role_spells.expected.md`, any other name with `.expected.md` added.
fn review_twin(out: &Path) -> PathBuf {
    if out.extension().is_some_and(|x| x == "rs") {
        return out.with_extension("expected.md");
    }
    let mut name = out.as_os_str().to_owned();
    name.push(".expected.md");
    PathBuf::from(name)
}

pub fn class_spells(a: Args) -> Result<(), String> {
    generate(a, CLASS_SPELLS, |_| {
        Ok(|tables: &Tables, build: &str| {
            let g = classgen::generate(tables, build)?;
            Ok(Made {
                summary: format!(
                    "{} spells ({} spec-unique), {} ambiguous dropped",
                    g.spells, g.spec_unique, g.ambiguous
                ),
                content: g.content,
                expected: None,
            })
        })
    })
}

pub fn item_spells(a: Args) -> Result<(), String> {
    generate(a, ITEM_SPELLS, |_| {
        Ok(|tables: &Tables, build: &str| {
            let g = itemgen::generate(tables, build)?;
            Ok(Made {
                summary: format!(
                    "{} item spells ({} trinket, {} via trigger chase, {} owned by one trinket)",
                    g.spells, g.trinkets, g.chased, g.owned
                ),
                content: g.content,
                expected: None,
            })
        })
    })
}

pub fn keystone_timers(a: Args) -> Result<(), String> {
    generate(a, KEYSTONE_TIMERS, |_| {
        Ok(|tables: &Tables, build: &str| {
            let g = keystonegen::generate(tables.get(keystonegen::TABLE)?, build)?;
            Ok(Made {
                summary: format!("{} dungeons, build {build}", g.dungeons),
                content: g.content,
                expected: None,
            })
        })
    })
}

pub fn open_world_maps(a: Args) -> Result<(), String> {
    generate(a, OPEN_WORLD_MAPS, |_| {
        Ok(|tables: &Tables, build: &str| {
            let g = mapgen::generate(tables.get(mapgen::TABLE)?, build)?;
            Ok(Made {
                summary: format!("{} open-world maps, build {build}", g.maps),
                content: g.content,
                expected: None,
            })
        })
    })
}

pub fn role_spells(a: Args) -> Result<(), String> {
    generate(a, ROLE_SPELLS, |text| {
        let census = rolegen::Census::parse(text)?;
        Ok(move |tables: &Tables, build: &str| {
            let g = rolegen::generate(tables, &census, build)?;
            Ok(Made {
                summary: format!(
                    "{} role spells, census over {} log(s)",
                    g.spells,
                    census.logs.len()
                ),
                content: g.content,
                expected: Some(g.expected),
            })
        })
    })
}

pub fn absorb_spells(a: Args) -> Result<(), String> {
    generate(a, ABSORB_SPELLS, |text| {
        // The same `id,name,<log>...` shape as the role census.
        let census = rolegen::Census::parse(text)?;
        Ok(move |tables: &Tables, build: &str| {
            let g = absorbgen::generate(tables, &census, build)?;
            Ok(Made {
                summary: format!(
                    "{} absorb spells, {} census id(s) outside the table over {} log(s)",
                    g.spells,
                    g.unknown.len(),
                    census.logs.len()
                ),
                content: g.content,
                expected: Some(g.expected),
            })
        })
    })
}

pub fn proc_spells(a: Args) -> Result<(), String> {
    generate(a, PROC_SPELLS, |text| {
        let census = procgen::Census::parse(text)?;
        Ok(move |tables: &Tables, build: &str| {
            let g = procgen::generate(tables, &census, build)?;
            Ok(Made {
                summary: format!(
                    "{} procs, census over {} log(s)",
                    g.procs,
                    census.logs.len()
                ),
                content: g.content,
                expected: Some(g.expected),
            })
        })
    })
}

/// The curated proc/driver pairs, for the census (`procgen::pairs`).
pub fn proc_pairs(a: Args) -> Result<(), String> {
    a.finish()?;
    print!("{}", procgen::pairs());
    Ok(())
}

pub fn placed_spells(a: Args) -> Result<(), String> {
    generate(a, PLACED_SPELLS, |text| {
        let census = placegen::Census::parse(text)?;
        Ok(move |tables: &Tables, build: &str| {
            let g = placegen::generate(tables, &census, build)?;
            Ok(Made {
                summary: format!(
                    "{} placed spells, census over {} log(s)",
                    g.spells,
                    census.logs.len()
                ),
                content: g.content,
                expected: Some(g.expected),
            })
        })
    })
}

/// Every spell the placed-spell census counts (`placegen::census_ids`).
pub fn placed_spells_ids(a: Args) -> Result<(), String> {
    a.finish()?;
    print!("{}", placegen::census_ids());
    Ok(())
}
