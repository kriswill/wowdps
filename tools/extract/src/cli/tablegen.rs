//! The committed tables the meter reads, generated from the install:
//! `gen-class-spells`, `gen-item-spells`, `gen-keystone-timers` and
//! `gen-open-world-maps`, and the census-backed `gen-role-spells`,
//! `gen-absorb-spells` and `gen-proc-spells` with their review twins — each
//! through one runner (`generate`) — and the proc pairs the proc census
//! reads (`proc-pairs`). Each edition of the game has its own committed
//! tables where one has been generated for it (Retail's for every
//! generator, Forever's under `crates/core/src/forever/` for four), and a
//! run writes its own edition's by default.

use std::path::{Path, PathBuf};

use super::args::Args;
use super::install::Install;
use super::owners::owned_only;
use super::{Cmd, cmd, read_text};
use wowdps_core::model::Edition;
use wowdps_extract::atomic::write_if_changed;
use wowdps_extract::game::default_product;
use wowdps_extract::table::Tables;
use wowdps_extract::{
    absorbgen, classgen, itemgen, keystonegen, mapgen, placegen, procgen, rolegen,
};

/// This module's commands, as the usage lists them.
pub const COMMANDS: &[Cmd] = &[
    cmd(
        "gen-class-spells",
        "gen-class-spells [wow-dir] --dbd-dir <dir>
                       [--checkout DIR] [-o class_spells.rs] [--keys tactkeys.txt] [--product wow]",
        class_spells,
    )
    .reads(&[CLASS_SPELLS.tables]),
    cmd(
        "gen-keystone-timers",
        "gen-keystone-timers [wow-dir] --dbd-dir <dir>
                       [--checkout DIR] [-o keystone_timers.rs] [--keys tactkeys.txt] [--product wow]",
        keystone_timers,
    )
    .reads(&[KEYSTONE_TIMERS.tables]),
    cmd(
        "gen-open-world-maps",
        "gen-open-world-maps [wow-dir] --dbd-dir <dir>
                       [--checkout DIR] [-o open_world_maps.rs] [--keys tactkeys.txt] [--product wow]",
        open_world_maps,
    )
    .reads(&[OPEN_WORLD_MAPS.tables]),
    cmd(
        "gen-item-spells",
        "gen-item-spells [wow-dir] --dbd-dir <dir>
                       [--checkout DIR] [-o item_spells.rs] [--keys tactkeys.txt] [--product wow]",
        item_spells,
    )
    .reads(&[ITEM_SPELLS.tables]),
    cmd(
        "gen-role-spells",
        "gen-role-spells [wow-dir] --dbd-dir <dir> --census <csv>
                       [--checkout DIR] [-o role_spells.rs] [--keys tactkeys.txt] [--product wow]",
        role_spells,
    )
    .reads(&[ROLE_SPELLS.tables]),
    cmd(
        "gen-absorb-spells",
        "gen-absorb-spells [wow-dir] --dbd-dir <dir> --census <csv>
                       [--checkout DIR] [-o absorb_spells.rs] [--keys tactkeys.txt] [--product wow]",
        absorb_spells,
    )
    .reads(&[ABSORB_SPELLS.tables]),
    cmd(
        "gen-proc-spells",
        "gen-proc-spells [wow-dir] --dbd-dir <dir> --census <csv>
                       [--checkout DIR] [-o proc_spells.rs] [--keys tactkeys.txt] [--product wow]",
        proc_spells,
    )
    .reads(&[PROC_SPELLS.tables]),
    cmd(
        "gen-placed-spells",
        "gen-placed-spells [wow-dir] --dbd-dir <dir> --census <csv>
                       [--checkout DIR] [-o placed_spells.rs] [--keys tactkeys.txt] [--product wow]",
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

/// A table generator: its command, where each edition's run writes the
/// table unless `-o` says, the client tables it reads, and, where a
/// real-log census backs it, the census `--census` names (the committed
/// one, for the error that asks for it).
struct Table {
    command: &'static str,
    /// Retail's committed table, by its path in a checkout.
    out: &'static str,
    /// Every other edition's committed table, where one is generated.
    editions: &'static [(Edition, &'static str)],
    tables: &'static [&'static str],
    /// The client tables a product's run reads, where the generator reads
    /// fewer than `tables` for some product (every one of `tables` is the
    /// command's, whose schemas are fetched for any run).
    reads: Option<fn(&str) -> &'static [&'static str]>,
    census: Option<&'static str>,
}

impl Table {
    /// The committed table `product`'s run writes unless `-o` says: its
    /// edition's, `None` for an edition with none.
    fn own(&self, product: &str) -> Option<&'static str> {
        if product == default_product() {
            return Some(self.out);
        }
        self.editions
            .iter()
            .find(|(e, _)| e.product() == Some(product))
            .map(|&(_, path)| path)
    }
}

const CLASS_SPELLS: Table = Table {
    command: "gen-class-spells",
    out: "crates/core/src/class_spells.rs",
    editions: &[(Edition::Forever, "crates/core/src/forever/class_spells.rs")],
    tables: &classgen::TABLES,
    reads: Some(class_tables),
    census: None,
};

/// The tables a product's class rules read (`classgen::Rules`).
fn class_tables(product: &str) -> &'static [&'static str] {
    classgen::Rules::for_product(product).tables()
}

const ITEM_SPELLS: Table = Table {
    command: "gen-item-spells",
    out: "crates/core/src/item_spells.rs",
    editions: &[(Edition::Forever, "crates/core/src/forever/item_spells.rs")],
    tables: &itemgen::TABLES,
    reads: None,
    census: None,
};

const KEYSTONE_TIMERS: Table = Table {
    command: "gen-keystone-timers",
    out: "crates/core/src/keystone_timers.rs",
    editions: &[(
        Edition::Forever,
        "crates/core/src/forever/keystone_timers.rs",
    )],
    tables: &[keystonegen::TABLE],
    reads: None,
    census: None,
};

const OPEN_WORLD_MAPS: Table = Table {
    command: "gen-open-world-maps",
    out: "crates/core/src/open_world_maps.rs",
    editions: &[(
        Edition::Forever,
        "crates/core/src/forever/open_world_maps.rs",
    )],
    tables: &[mapgen::TABLE],
    reads: None,
    census: None,
};

const ROLE_SPELLS: Table = Table {
    command: "gen-role-spells",
    out: "crates/core/src/role_spells.rs",
    editions: &[],
    tables: &rolegen::TABLES,
    reads: None,
    census: Some("tools/role-spells-census.csv"),
};

const ABSORB_SPELLS: Table = Table {
    command: "gen-absorb-spells",
    out: "crates/core/src/absorb_spells.rs",
    editions: &[],
    tables: &absorbgen::TABLES,
    reads: None,
    census: Some("tools/absorb-spells-census.csv"),
};

const PROC_SPELLS: Table = Table {
    command: "gen-proc-spells",
    out: "crates/core/src/proc_spells.rs",
    editions: &[],
    tables: &procgen::TABLES,
    reads: None,
    census: Some("tools/proc-spells-census.csv"),
};

const PLACED_SPELLS: Table = Table {
    command: "gen-placed-spells",
    out: "crates/proto/src/placed/placed_spells.rs",
    editions: &[],
    tables: &placegen::TABLES,
    reads: None,
    census: Some("tools/placed-spells-census.csv"),
};

/// Every table generator.
const TABLES: [&Table; 8] = [
    &CLASS_SPELLS,
    &ITEM_SPELLS,
    &KEYSTONE_TIMERS,
    &OPEN_WORLD_MAPS,
    &ROLE_SPELLS,
    &ABSORB_SPELLS,
    &PROC_SPELLS,
    &PLACED_SPELLS,
];

/// Every committed table and, where a census backs it, its review twin,
/// by its path in a checkout, with the edition whose client it is made
/// from: what no other edition's run may write (`owners::owned_only`).
pub fn committed() -> Vec<(Edition, PathBuf)> {
    let mut paths = Vec::new();
    for t in TABLES {
        let editions = std::iter::once((Edition::Retail, t.out)).chain(t.editions.iter().copied());
        for (edition, out) in editions {
            paths.push((edition, PathBuf::from(out)));
            if t.census.is_some() {
                paths.push((edition, review_twin(Path::new(out))));
            }
        }
    }
    paths
}

/// What a generator made: the table's text, its review twin's where a
/// census backs it, and the summary's words after the table's path.
struct Made {
    content: String,
    expected: Option<String>,
    summary: String,
}

/// Run a table generator: its arguments taken; the census read and handed
/// to `prepare` (with the product read) before the install is opened, so a
/// census that does not read stops it first; the client tables the
/// product's run reads; the generator `prepare` returned called with them
/// and the build (with the product beside it for any but Retail's,
/// `Game::build_label`); and the table written where its bytes change, its
/// review twin beside it, made exactly where a census backs the table
/// (`twin_as_checked`). With no `-o` a run writes its edition's committed
/// table (under `--checkout DIR`, as the scripts pass it, else under the
/// working directory); an edition with none needs `-o`. An `-o` wins over
/// `--checkout`: the scripts pass `--checkout` and the caller's words after
/// it, so a caller's `-o` is the one place written. Each committed
/// table and twin is its edition's: another product's output may not be
/// written to it, however `-o` spells it, nor may the twin named from the
/// table's place be another's (`owners::owned_only`).
fn generate<G>(
    mut a: Args,
    t: Table,
    prepare: impl FnOnce(&str, &str) -> Result<G, String>,
) -> Result<(), String>
where
    G: FnOnce(&Tables, &str) -> Result<Made, String>,
{
    let out = a.path(&["-o", "--out"])?;
    let checkout = a.path(&["--checkout"])?;
    let census_path = match t.census {
        Some(_) => a.path(&["--census"])?,
        None => None,
    };
    let mut install = Install::with_tables(&mut a)?;
    a.finish()?;
    if let Some(reads) = t.reads {
        let only = reads(&install.product);
        install.tables.retain(|name| only.contains(name));
    }
    let place = format!("the committed {}", t.out);
    let own = t.own(&install.product).map(|rel| match &checkout {
        Some(dir) => dir.join(rel),
        None => PathBuf::from(rel),
    });
    let out = out.or(own);
    // A census-backed table writes its review twin beside it, the twin's
    // name made from the table's: both are checked before anything is read.
    let twin = out
        .as_deref()
        .filter(|_| t.census.is_some())
        .map(review_twin);
    owned_only(&install, out.as_deref(), twin.as_deref(), &place)?;
    // `owned_only` refuses a run with no place: no `-o`, and no committed
    // table of its edition's.
    let out = out.ok_or_else(|| format!("{}: -o names no place", t.command))?;
    let census = match (t.census, census_path) {
        (Some(_), Some(path)) => read_text(&path)?,
        (Some(committed), None) => {
            return Err(format!("{} requires --census <{committed}>", t.command));
        }
        (None, _) => String::new(),
    };
    let make = prepare(&census, &install.product)?;
    let (game, tables) = install.open_tables()?;

    let made = make(&tables, &game.build_label())?;
    twin_as_checked(&t, &made)?;
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

/// The guard checks a review twin's place when a census backs the table,
/// and the writer writes one when the generator made one: the two must
/// agree before anything is written, or a generator that made a twin its
/// table has no census for would write it where nothing checked.
fn twin_as_checked(t: &Table, made: &Made) -> Result<(), String> {
    match (t.census.is_some(), made.expected.is_some()) {
        (false, true) => Err(format!(
            "{}: made a review twin for {}, which no census backs, so its place was never checked",
            t.command, t.out
        )),
        (true, false) => Err(format!(
            "{}: made no review twin for {}, which a census backs",
            t.command, t.out
        )),
        _ => Ok(()),
    }
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
    generate(a, CLASS_SPELLS, |_, product| {
        let rules = classgen::Rules::for_product(product);
        Ok(move |tables: &Tables, build: &str| {
            let g = match rules {
                classgen::Rules::Specs => classgen::generate(tables, build)?,
                classgen::Rules::Masks => classgen::generate_masks(tables, build)?,
            };
            let mut summary = format!(
                "{} spells ({} spec-unique), {} ambiguous dropped",
                g.spells, g.spec_unique, g.ambiguous
            );
            if rules == classgen::Rules::Masks {
                summary.push_str(&format!(
                    ", {} class-line rows with no class bit skipped",
                    g.unclassed
                ));
            }
            Ok(Made {
                summary,
                content: g.content,
                expected: None,
            })
        })
    })
}

pub fn item_spells(a: Args) -> Result<(), String> {
    generate(a, ITEM_SPELLS, |_, _| {
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
    generate(a, KEYSTONE_TIMERS, |_, _| {
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
    generate(a, OPEN_WORLD_MAPS, |_, _| {
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
    generate(a, ROLE_SPELLS, |text, _| {
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
    generate(a, ABSORB_SPELLS, |text, _| {
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
    generate(a, PROC_SPELLS, |text, _| {
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
    generate(a, PLACED_SPELLS, |text, _| {
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

#[cfg(test)]
mod tests {
    use super::*;

    /// A twin is let through exactly where a census backs the table, as
    /// the guard checked it; the other way round, for every table, is an
    /// error that names the generator.
    #[test]
    fn a_twin_is_written_only_where_the_guard_checked_one() {
        let made = |twin: bool| Made {
            content: String::new(),
            expected: twin.then(|| "twin".to_string()),
            summary: String::new(),
        };
        for t in TABLES {
            let backed = t.census.is_some();
            assert!(twin_as_checked(t, &made(backed)).is_ok(), "{}", t.command);
            let e = twin_as_checked(t, &made(!backed)).unwrap_err();
            assert!(e.starts_with(&format!("{}: made ", t.command)), "{e}");
        }
    }

    /// A run writes its own edition's table by default: Retail's for
    /// Retail, Forever's for the four generators that have one, none for
    /// the rest or for a product no edition names.
    #[test]
    fn each_product_writes_its_own_committed_table() {
        for t in TABLES {
            assert_eq!(t.own("wow"), Some(t.out), "{}", t.command);
            let forever = t.own("wowf");
            match t.command {
                "gen-class-spells"
                | "gen-item-spells"
                | "gen-keystone-timers"
                | "gen-open-world-maps" => {
                    let rel = t
                        .out
                        .replace("crates/core/src/", "crates/core/src/forever/");
                    assert_eq!(forever, Some(rel.as_str()), "{}", t.command);
                }
                _ => assert_eq!(forever, None, "{}", t.command),
            }
            assert_eq!(t.own("wow_classic"), None, "{}", t.command);
            assert_eq!(t.own(""), None, "{}", t.command);
        }
    }

    /// Every committed file is listed once, under one edition, and a
    /// review twin only where a census backs the table.
    #[test]
    fn every_committed_file_has_one_edition() {
        let committed = committed();
        for (i, (_, path)) in committed.iter().enumerate() {
            let twins = committed.iter().skip(i + 1).filter(|(_, p)| p == path);
            assert_eq!(twins.count(), 0, "{} twice", path.display());
        }
        let of = |e: Edition| committed.iter().filter(|(o, _)| *o == e).count();
        assert_eq!((of(Edition::Retail), of(Edition::Forever)), (12, 4));
        assert_eq!(committed.len(), 16);
    }

    /// Retail's class run reads the tables it always did, in their order;
    /// Forever's the class masks' alone.
    #[test]
    fn a_class_run_reads_its_products_tables() {
        let reads = CLASS_SPELLS.reads.unwrap();
        assert_eq!(reads("wow"), &classgen::TABLES[..8]);
        assert_eq!(
            reads("wowf"),
            ["SkillLine", "SkillLineAbility", "ChrClasses"]
        );
        assert!(TABLES.iter().filter(|t| t.reads.is_some()).count() == 1);
    }
}
