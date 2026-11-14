//! The committed tables the meter reads, generated from the install:
//! `gen-class-spells`, `gen-item-spells`, `gen-keystone-timers`,
//! `gen-open-world-maps` and `gen-instance-maps`, and the census-backed `gen-role-spells`,
//! `gen-absorb-spells` and `gen-proc-spells` with their review twins — each
//! through one runner (`generate`) — and the proc pairs the proc census
//! reads (`proc-pairs`). Each edition of the game has its own committed
//! tables where one has been generated for it (Retail's for every
//! generator but the role signals', Forever's under
//! `crates/core/src/forever/` where `Table::editions` names one), and a run
//! writes its own edition's by default. A census-backed table's run reads
//! its edition's own committed census under `--checkout` (`Table::census`)
//! unless `--census` names another.

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
    absorbgen, classgen, itemgen, keystonegen, mapgen, placegen, procgen, rolegen, signalgen,
    spangen,
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
        "gen-instance-maps",
        "gen-instance-maps [wow-dir] --dbd-dir <dir>
                       [--checkout DIR] [-o instance_maps.rs] [--keys tactkeys.txt] [--product wow]",
        instance_maps,
    )
    .reads(&[INSTANCE_MAPS.tables]),
    cmd(
        "gen-item-spells",
        "gen-item-spells [wow-dir] --dbd-dir <dir>
                       [--checkout DIR] [-o item_spells.rs] [--keys tactkeys.txt] [--product wow]",
        item_spells,
    )
    .reads(&[ITEM_SPELLS.tables]),
    cmd(
        "gen-role-spells",
        "gen-role-spells [wow-dir] --dbd-dir <dir> [--census <csv>]
                       [--checkout DIR] [-o role_spells.rs] [--keys tactkeys.txt] [--product wow]",
        role_spells,
    )
    .reads(&[ROLE_SPELLS.tables]),
    cmd(
        "gen-absorb-spells",
        "gen-absorb-spells [wow-dir] --dbd-dir <dir> [--census <csv>]
                       [--checkout DIR] [-o absorb_spells.rs] [--keys tactkeys.txt] [--product wow]",
        absorb_spells,
    )
    .reads(&[ABSORB_SPELLS.tables]),
    cmd(
        "gen-proc-spells",
        "gen-proc-spells [wow-dir] --dbd-dir <dir> [--census <csv>]
                       [--checkout DIR] [-o proc_spells.rs] [--keys tactkeys.txt] [--product wow]",
        proc_spells,
    )
    .reads(&[PROC_SPELLS.tables]),
    cmd(
        "gen-placed-spells",
        "gen-placed-spells [wow-dir] --dbd-dir <dir> [--census <csv>]
                       [--checkout DIR] [-o placed_spells.rs] [--keys tactkeys.txt] [--product wow]",
        placed_spells,
    )
    .reads(&[PLACED_SPELLS.tables]),
    cmd(
        "gen-role-signals",
        "gen-role-signals [wow-dir] --dbd-dir <dir> [--census <csv>]
                       [--checkout DIR] [-o role_signals.rs] [--keys tactkeys.txt] [--product wowf]",
        role_signals,
    )
    .reads(&[ROLE_SIGNALS.tables]),
    cmd(
        "placed-spells-ids",
        "placed-spells-ids [--product wow]   (every spell the placed-spell census counts)",
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
/// real-log census backs it, each edition's committed census: what a run
/// reads under `--checkout` when no `--census` names another.
struct Table {
    command: &'static str,
    /// Retail's committed table, by its path in a checkout; `None` for a
    /// table only another edition has (R30's role signals: Retail's role is
    /// its spec's).
    out: Option<&'static str>,
    /// Every other edition's committed table, where one is generated.
    editions: &'static [(Edition, &'static str)],
    tables: &'static [&'static str],
    /// The client tables a product's run reads, where the generator reads
    /// fewer than `tables` for some product (every one of `tables` is the
    /// command's, whose schemas are fetched for any run).
    reads: Option<fn(&str) -> &'static [&'static str]>,
    /// Each edition's committed census, by its path in a checkout; empty
    /// for a table no census backs.
    census: &'static [(Edition, &'static str)],
}

impl Table {
    /// The committed census `product`'s run reads unless `--census` says:
    /// its edition's, `None` for an edition with none.
    fn census_of(&self, product: &str) -> Option<&'static str> {
        self.census
            .iter()
            .find(|(e, _)| e.product() == Some(product))
            .map(|&(_, path)| path)
    }

    /// A census backs the table, so a run writes its review twin.
    fn backed(&self) -> bool {
        !self.census.is_empty()
    }

    /// The committed table `product`'s run writes unless `-o` says: its
    /// edition's, `None` for an edition with none.
    fn own(&self, product: &str) -> Option<&'static str> {
        if product == default_product() {
            return self.out;
        }
        self.editions
            .iter()
            .find(|(e, _)| e.product() == Some(product))
            .map(|&(_, path)| path)
    }
}

const CLASS_SPELLS: Table = Table {
    command: "gen-class-spells",
    out: Some("crates/core/src/class_spells.rs"),
    editions: &[(Edition::Forever, "crates/core/src/forever/class_spells.rs")],
    tables: &classgen::TABLES,
    reads: Some(class_tables),
    census: &[],
};

/// The tables a product's class rules read (`classgen::Rules`).
fn class_tables(product: &str) -> &'static [&'static str] {
    classgen::Rules::for_product(product).tables()
}

const ITEM_SPELLS: Table = Table {
    command: "gen-item-spells",
    out: Some("crates/core/src/item_spells.rs"),
    editions: &[(Edition::Forever, "crates/core/src/forever/item_spells.rs")],
    tables: &itemgen::TABLES,
    reads: None,
    census: &[],
};

const KEYSTONE_TIMERS: Table = Table {
    command: "gen-keystone-timers",
    out: Some("crates/core/src/keystone_timers.rs"),
    editions: &[(
        Edition::Forever,
        "crates/core/src/forever/keystone_timers.rs",
    )],
    tables: &[keystonegen::TABLE],
    reads: None,
    census: &[],
};

const OPEN_WORLD_MAPS: Table = Table {
    command: "gen-open-world-maps",
    out: Some("crates/core/src/open_world_maps.rs"),
    editions: &[(
        Edition::Forever,
        "crates/core/src/forever/open_world_maps.rs",
    )],
    tables: &[mapgen::TABLE],
    reads: None,
    census: &[],
};

const INSTANCE_MAPS: Table = Table {
    command: "gen-instance-maps",
    out: Some("crates/core/src/instance_maps.rs"),
    editions: &[(Edition::Forever, "crates/core/src/forever/instance_maps.rs")],
    tables: &[mapgen::TABLE],
    reads: None,
    census: &[],
};

const ROLE_SPELLS: Table = Table {
    command: "gen-role-spells",
    out: Some("crates/core/src/role_spells.rs"),
    editions: &[(Edition::Forever, "crates/core/src/forever/role_spells.rs")],
    tables: &spangen::TABLES,
    reads: Some(role_tables),
    census: &[
        (Edition::Retail, "tools/role-spells-census.csv"),
        (Edition::Forever, "tools/forever/role-spells-census.csv"),
    ],
};

/// The tables a product's role-span run reads: Retail's list is by id
/// (`rolegen`), Forever's by name, expanded through the class families and
/// the casts' ties (`spangen`).
fn role_tables(product: &str) -> &'static [&'static str] {
    if product == "wowf" {
        &spangen::TABLES
    } else {
        &rolegen::TABLES
    }
}

const ABSORB_SPELLS: Table = Table {
    command: "gen-absorb-spells",
    out: Some("crates/core/src/absorb_spells.rs"),
    editions: &[(Edition::Forever, "crates/core/src/forever/absorb_spells.rs")],
    tables: &absorbgen::TABLES,
    reads: None,
    census: &[
        (Edition::Retail, "tools/absorb-spells-census.csv"),
        (Edition::Forever, "tools/forever/absorb-spells-census.csv"),
    ],
};

const PROC_SPELLS: Table = Table {
    command: "gen-proc-spells",
    out: Some("crates/core/src/proc_spells.rs"),
    editions: &[],
    tables: &procgen::TABLES,
    reads: None,
    census: &[(Edition::Retail, "tools/proc-spells-census.csv")],
};

const PLACED_SPELLS: Table = Table {
    command: "gen-placed-spells",
    out: Some("crates/proto/src/placed/placed_spells.rs"),
    editions: &[(
        Edition::Forever,
        "crates/proto/src/placed/forever_placed_spells.rs",
    )],
    tables: &placegen::TABLES,
    reads: None,
    census: &[
        (Edition::Retail, "tools/placed-spells-census.csv"),
        (Edition::Forever, "tools/forever/placed-spells-census.csv"),
    ],
};

const ROLE_SIGNALS: Table = Table {
    command: "gen-role-signals",
    out: None,
    editions: &[(Edition::Forever, "crates/core/src/forever/role_signals.rs")],
    tables: &signalgen::TABLES,
    reads: None,
    census: &[(Edition::Forever, "tools/role-signals-census.csv")],
};

/// Every table generator.
const TABLES: [&Table; 10] = [
    &CLASS_SPELLS,
    &ITEM_SPELLS,
    &KEYSTONE_TIMERS,
    &OPEN_WORLD_MAPS,
    &INSTANCE_MAPS,
    &ROLE_SPELLS,
    &ABSORB_SPELLS,
    &PROC_SPELLS,
    &PLACED_SPELLS,
    &ROLE_SIGNALS,
];

/// Every committed table and, where a census backs it, its review twin,
/// by its path in a checkout, with the edition whose client it is made
/// from: what no other edition's run may write (`owners::owned_only`).
pub fn committed() -> Vec<(Edition, PathBuf)> {
    let mut paths = Vec::new();
    for t in TABLES {
        let retail = t.out.map(|out| (Edition::Retail, out));
        let editions = retail.into_iter().chain(t.editions.iter().copied());
        for (edition, out) in editions {
            paths.push((edition, PathBuf::from(out)));
            if t.backed() {
                paths.push((edition, review_twin(Path::new(out))));
            }
        }
    }
    paths
}

/// Whether `command` writes a Retail table: every generator but the role
/// signals', which only Forever has (R30).
#[cfg(test)]
pub fn has_retail_table(command: &str) -> bool {
    TABLES
        .iter()
        .any(|t| t.command == command && t.out.is_some())
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
    let census_path = if t.backed() {
        a.path(&["--census"])?
    } else {
        None
    };
    let mut install = Install::with_tables(&mut a)?;
    a.finish()?;
    if let Some(reads) = t.reads {
        let only = reads(&install.product);
        install.tables.retain(|name| only.contains(name));
    }
    let own = t.own(&install.product).map(|rel| match &checkout {
        Some(dir) => dir.join(rel),
        None => PathBuf::from(rel),
    });
    let out = out.or(own);
    // A census-backed table writes its review twin beside it, the twin's
    // name made from the table's: both are checked before anything is read.
    let twin = out.as_deref().filter(|_| t.backed()).map(review_twin);
    match t.out {
        Some(retail) => {
            let place = format!("the committed {retail}");
            owned_only(&install, out.as_deref(), twin.as_deref(), &place)?;
        }
        // A table Retail has none of (R30's role signals): a run with no
        // place is told so, and one with a place is checked as any is.
        None if out.is_none() => {
            return Err(format!(
                "{}: product {} has no committed table of its own; -o names the place",
                t.command, install.product
            ));
        }
        None => owned_only(&install, out.as_deref(), twin.as_deref(), "")?,
    }
    // `owned_only` refuses a run with no place: no `-o`, and no committed
    // table of its edition's.
    let out = out.ok_or_else(|| format!("{}: -o names no place", t.command))?;
    // Under `--checkout` (as the scripts pass it) the product's committed
    // census, unless `--census` names another; without either, or for an
    // edition with no committed census, `--census` is required.
    let committed_census = t.census_of(&install.product);
    let census_path = census_path.or_else(|| Some(checkout.as_ref()?.join(committed_census?)));
    let census = match (t.backed(), census_path) {
        (true, Some(path)) => read_text(&path)?,
        (true, None) => {
            let named = committed_census
                .or_else(|| t.census.first().map(|&(_, p)| p))
                .unwrap_or("csv");
            return Err(format!("{} requires --census <{named}>", t.command));
        }
        (false, _) => String::new(),
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
    match (t.backed(), made.expected.is_some()) {
        (false, true) => Err(format!(
            "{}: made a review twin, which no census backs, so its place was never checked",
            t.command
        )),
        (true, false) => Err(format!(
            "{}: made no review twin, which a census backs",
            t.command
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

pub fn instance_maps(a: Args) -> Result<(), String> {
    generate(a, INSTANCE_MAPS, |_, _| {
        Ok(|tables: &Tables, build: &str| {
            let g = mapgen::generate_instanced(tables.get(mapgen::TABLE)?, build)?;
            Ok(Made {
                summary: format!("{} instanced maps, build {build}", g.maps),
                content: g.content,
                expected: None,
            })
        })
    })
}

pub fn role_spells(a: Args) -> Result<(), String> {
    generate(a, ROLE_SPELLS, |text, product| {
        let census = rolegen::Census::parse(text)?;
        // Forever's list is curated by name (its ranks are ids of their own).
        let by_name = product == "wowf";
        Ok(move |tables: &Tables, build: &str| {
            let g = if by_name {
                spangen::generate(tables, &census, build)?
            } else {
                rolegen::generate(tables, &census, build)?
            };
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
    generate(a, ABSORB_SPELLS, |text, product| {
        // The same `id,name,<log>...` shape as the role census.
        let census = rolegen::Census::parse(text)?;
        let rail = absorbgen::rail(product);
        Ok(move |tables: &Tables, build: &str| {
            let g = absorbgen::generate_for(tables, &census, build, rail)?;
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
    generate(a, PLACED_SPELLS, |text, product| {
        let census = placegen::Census::parse(text)?;
        let product = product.to_string();
        Ok(move |tables: &Tables, build: &str| {
            let g = placegen::generate_for(&product, tables, &census, build)?;
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

/// R30: Forever's role signals; Retail has none (its role is its spec's),
/// so only a Forever run has a committed place.
pub fn role_signals(a: Args) -> Result<(), String> {
    generate(a, ROLE_SIGNALS, |text, _| {
        // The same `id,name,<log>...` shape as the role census, of casts.
        let census = rolegen::Census::parse(text)?;
        Ok(move |tables: &Tables, build: &str| {
            let g = signalgen::generate(tables, &census, build)?;
            Ok(Made {
                summary: format!(
                    "{} role signals from {} names, census over {} log(s)",
                    g.spells,
                    g.entries,
                    census.logs.len()
                ),
                content: g.content,
                expected: Some(g.expected),
            })
        })
    })
}

/// Every spell the placed-spell census counts (`placegen::census_ids`).
/// A product's (`--product ROW`, else `$WOWDPS_PRODUCT`, else Retail's).
pub fn placed_spells_ids(mut a: Args) -> Result<(), String> {
    let named = a.opt(&["--product"])?;
    a.finish()?;
    let product = wowdps_extract::game::product(named);
    print!(
        "{}",
        placegen::census_ids_of(placegen::curated_for(&product))
    );
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
            let backed = t.backed();
            assert!(twin_as_checked(t, &made(backed)).is_ok(), "{}", t.command);
            let e = twin_as_checked(t, &made(!backed)).unwrap_err();
            assert!(e.starts_with(&format!("{}: made ", t.command)), "{e}");
        }
    }

    /// A run writes its own edition's table by default: Retail's for
    /// Retail, Forever's for the generators that have one, none for
    /// the rest or for a product no edition names. Every generator has a
    /// Retail table but the role signals', which only Forever has.
    #[test]
    fn each_product_writes_its_own_committed_table() {
        for t in TABLES {
            let forever = t.own("wowf");
            if t.command == "gen-role-signals" {
                assert_eq!(t.own("wow"), None);
                assert_eq!(forever, Some("crates/core/src/forever/role_signals.rs"));
                continue;
            }
            assert_eq!(t.own("wow"), t.out, "{}", t.command);
            let out = t.out.unwrap();
            if t.command == "gen-placed-spells" {
                assert_eq!(
                    forever,
                    Some("crates/proto/src/placed/forever_placed_spells.rs")
                );
                continue;
            }
            match t.command {
                "gen-class-spells"
                | "gen-item-spells"
                | "gen-keystone-timers"
                | "gen-open-world-maps"
                | "gen-instance-maps"
                | "gen-absorb-spells"
                | "gen-role-spells" => {
                    let rel = out.replace("crates/core/src/", "crates/core/src/forever/");
                    assert_eq!(forever, Some(rel.as_str()), "{}", t.command);
                }
                _ => assert_eq!(forever, None, "{}", t.command),
            }
            assert_eq!(t.own("wow_classic"), None, "{}", t.command);
            assert_eq!(t.own(""), None, "{}", t.command);
        }
    }

    /// A run reads its own edition's committed census: Retail's for
    /// Retail, Forever's where Forever has one, none for an edition with
    /// none (which must name one by `--census`).
    #[test]
    fn each_product_reads_its_own_census() {
        assert_eq!(
            ABSORB_SPELLS.census_of("wow"),
            Some("tools/absorb-spells-census.csv")
        );
        assert_eq!(
            ABSORB_SPELLS.census_of("wowf"),
            Some("tools/forever/absorb-spells-census.csv")
        );
        assert_eq!(ROLE_SIGNALS.census_of("wow"), None);
        assert_eq!(
            ROLE_SIGNALS.census_of("wowf"),
            Some("tools/role-signals-census.csv")
        );
        assert_eq!(CLASS_SPELLS.census_of("wow"), None);
        for t in TABLES {
            assert_eq!(
                t.backed(),
                t.census_of("wow").is_some() || t.census_of("wowf").is_some()
            );
            assert_eq!(t.census_of("wow_classic"), None, "{}", t.command);
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
        assert_eq!((of(Edition::Retail), of(Edition::Forever)), (13, 13));
        assert_eq!(committed.len(), 26);
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
        assert!(TABLES.iter().filter(|t| t.reads.is_some()).count() == 2);
        let roles = ROLE_SPELLS.reads.unwrap();
        assert_eq!(roles("wow"), rolegen::TABLES);
        assert_eq!(roles("wowf"), spangen::TABLES);
        assert!(roles("wow").iter().all(|t| ROLE_SPELLS.tables.contains(t)));
    }
}
