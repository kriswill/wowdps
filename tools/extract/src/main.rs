//! CLI for the local game-data extractor: one command a run, each listed
//! with its usage by the module under `cli/` that runs it; the usage text
//! is made from those lists (`wowdps_extract::command::Commands`).

mod cli;

use std::process::ExitCode;

use cli::args::Args;
use cli::{Cmd, caches, creatures, db2, tablegen};
use wowdps_extract::command::{Commands, tables_cmd};

fn main() -> ExitCode {
    EXTRACT.main()
}

/// Every command, in the order the usage lists them: each module's own.
static EXTRACT: Commands = Commands {
    bin: "wowdps-extract",
    lists: &[
        db2::COMMANDS,
        tablegen::COMMANDS,
        caches::COMMANDS,
        creatures::COMMANDS,
        MAIN,
    ],
};

/// The commands `main.rs` answers itself: about the others.
const MAIN: &[Cmd] = &[tables_cmd(tables)];

fn tables(a: Args) -> Result<(), String> {
    EXTRACT.tables(a)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_command_is_listed_once_under_its_own_name() {
        let usage = EXTRACT.usage();
        for (i, c) in EXTRACT.iter().enumerate() {
            assert!(c.usage.starts_with(c.name), "{}: {}", c.name, c.usage);
            assert!(
                EXTRACT.iter().skip(i + 1).all(|o| o.name != c.name),
                "{} twice",
                c.name
            );
            assert!(usage.contains(&format!("  wowdps-extract {}", c.usage)));
        }
    }

    /// A command reads tables exactly when it takes `--dbd-dir`, and each
    /// table it names has a FileDataID to be fetched by.
    #[test]
    fn a_command_names_the_tables_it_reads() {
        for c in EXTRACT.iter() {
            assert_eq!(
                c.usage.contains("--dbd-dir"),
                !c.table_names().is_empty(),
                "{}",
                c.name
            );
            for t in c.table_names() {
                assert!(wowdps_extract::table::file_id(t).is_ok(), "{}: {t}", c.name);
            }
        }
    }

    /// A flag no command knows is refused before the command reads or
    /// writes anything (the install, the network, the disk), and the
    /// refusal names it with the command's usage.
    #[test]
    fn every_command_refuses_a_flag_it_does_not_know() {
        for c in EXTRACT.iter() {
            let words = ["--dbd-dir", "/nonexistent", "--no-such-flag"];
            let args = EXTRACT.args(words.map(String::from).to_vec(), c);
            let e = (c.run)(args).err().unwrap_or_default();
            assert!(e.starts_with("unexpected argument"), "{}: {e}", c.name);
            assert!(
                e.contains(&format!("wowdps-extract {}", c.usage)),
                "{}: {e}",
                c.name
            );
        }
    }

    /// What every refusal of product wowf ends with.
    const REFUSED: &str = "is Retail's (product wow); product wowf needs -o naming another place";

    /// The error `name` gives for `words` (empty where it ran through).
    fn run(name: &str, words: &[&str]) -> String {
        let c = EXTRACT.find(name).unwrap();
        let words = words.iter().map(|w| w.to_string()).collect();
        (c.run)(EXTRACT.args(words, c)).err().unwrap_or_default()
    }

    /// A committed file of this checkout, by its absolute path.
    fn committed(rel: &str) -> String {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        root.join(rel).display().to_string()
    }

    /// Every generator refuses another product's output where Retail's
    /// lives before it reads anything (the install, a census, the store,
    /// the network): with no `-o` where it has no table of that product's
    /// own (its own committed table or data-home cache is Retail's), and
    /// with an `-o` naming any of Retail's committed tables or review
    /// twins, not only its own. A generator with a Forever table goes
    /// through with no `-o`, as far as the absent install, and writes
    /// nothing.
    #[test]
    fn every_generator_keeps_another_product_off_retails_outputs() {
        let gens: Vec<_> = EXTRACT
            .iter()
            .filter(|c| c.name.starts_with("gen-"))
            .collect();
        assert_eq!(gens.len(), 13);
        let others = [
            committed("crates/core/src/item_spells.rs"),
            committed("crates/core/src/proc_spells.expected.md"),
        ];
        let absent = ["--wow-dir", "/nonexistent", "--dbd-dir", "/nonexistent"];
        let forevers = [
            "gen-class-spells",
            "gen-item-spells",
            "gen-keystone-timers",
            "gen-open-world-maps",
        ];
        for c in gens {
            let e = run(c.name, &[&["--product", "wowf"][..], &absent[..]].concat());
            if forevers.contains(&c.name) {
                assert!(
                    e.contains(".build.info") && !e.contains("needs -o"),
                    "{}: {e}",
                    c.name
                );
            } else {
                assert!(e.ends_with(REFUSED), "{}: {e}", c.name);
            }
            for other in &others {
                let other = other.as_str();
                let words = [&["--product", "wowf", "-o", other][..], &absent[..]].concat();
                let e = run(c.name, &words);
                assert!(e.starts_with(&format!("-o {other}: ")), "{}: {e}", c.name);
                assert!(e.ends_with(REFUSED), "{}: {e}", c.name);
            }
        }
        assert!(
            !std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("crates")
                .exists(),
            "nothing written under the working directory"
        );
    }

    /// An `-o` elsewhere lets another product through, as far as the
    /// install (absent here, so nothing is read or written); of two `-o`,
    /// the later is the one checked, as it is the one written. The
    /// whole-icons pass, which writes no cache, is refused up front when
    /// the cache its run after cuts would be Retail's.
    #[test]
    fn an_o_elsewhere_gets_through_and_the_later_o_wins() {
        let elsewhere = std::env::temp_dir()
            .join(format!("wowdps-extract-elsewhere-{}", std::process::id()))
            .join("out");
        let (elsewhere, mine) = (
            elsewhere.display().to_string(),
            committed("crates/core/src/class_spells.rs"),
        );
        let (elsewhere, mine) = (elsewhere.as_str(), mine.as_str());
        /// Product wowf, `words`, and an install that is not there.
        fn with<'a>(words: &[&'a str]) -> Vec<&'a str> {
            let absent = ["--wow-dir", "/nonexistent", "--dbd-dir", "/nonexistent"];
            [&["--product", "wowf"][..], words, &absent[..]].concat()
        }
        let through = |e: &str| e.contains(".build.info") && !e.contains("Retail's");

        let e = run("gen-class-spells", &with(&["-o", mine, "-o", elsewhere]));
        assert!(through(&e), "{e}");
        let e = run("gen-class-spells", &with(&["-o", elsewhere, "-o", mine]));
        assert!(e.ends_with(REFUSED), "{e}");
        let e = run("gen-spell-icons", &with(&["-o", elsewhere]));
        assert!(through(&e), "{e}");

        let e = run("gen-icons", &with(&["--whole", elsewhere, "--whole-only"]));
        assert!(e.ends_with(REFUSED), "{e}");
        let words = ["--whole", elsewhere, "--whole-only", "-o", elsewhere];
        let e = run("gen-icons", &with(&words));
        assert!(through(&e), "{e}");
        assert!(!std::path::Path::new(elsewhere).exists(), "nothing written");
    }

    /// `-o` wins over `--checkout`: the scripts pass the checkout and then
    /// the caller's words, so a caller's `-o` is the place checked (and,
    /// past the absent install, written). With the checkout alone a run
    /// goes to its own edition's table, or is refused where it has none.
    #[test]
    fn an_o_wins_over_the_checkout() {
        let checkout = committed("");
        let forever = committed("crates/core/src/forever/class_spells.rs");
        let elsewhere = std::env::temp_dir()
            .join(format!("wowdps-extract-checkout-{}", std::process::id()))
            .join("out");
        let elsewhere = elsewhere.display().to_string();
        let absent = ["--wow-dir", "/nonexistent", "--dbd-dir", "/nonexistent"];
        let with = |words: &[&str]| -> Vec<String> {
            let mut all: Vec<String> = ["--checkout", checkout.as_str()]
                .iter()
                .map(|w| w.to_string())
                .collect();
            all.extend(words.iter().map(|w| w.to_string()));
            all.extend(absent.iter().map(|w| w.to_string()));
            all
        };
        let go = |name: &str, words: &[&str]| {
            let words = with(words);
            let refs: Vec<&str> = words.iter().map(String::as_str).collect();
            run(name, &refs)
        };
        let through = |e: &str| e.contains(".build.info") && !e.contains("needs -o");
        let e = go("gen-class-spells", &["-o", &forever]);
        assert!(
            e.ends_with("is Forever's (product wowf); product wow needs -o naming another place"),
            "{e}"
        );
        let e = go("gen-class-spells", &["--product", "wowf", "-o", &elsewhere]);
        assert!(through(&e), "{e}");
        let e = go("gen-class-spells", &["--product", "wowf"]);
        assert!(through(&e), "{e}");
        let e = go("gen-class-spells", &[]);
        assert!(through(&e), "{e}");
        let e = go("gen-open-world-maps", &["--product", "wowf"]);
        assert!(through(&e), "{e}");
        let e = go("gen-role-spells", &["--product", "wowf"]);
        assert!(e.ends_with(REFUSED), "{e}");
        assert!(
            !std::path::Path::new(&elsewhere).exists(),
            "nothing written"
        );
    }

    /// A census-backed generator writes its review twin beside `-o`, its
    /// name made from `-o`'s, so an `-o` whose twin would be a committed
    /// one is refused: the generator's own (`proc_spells`, no extension)
    /// or another's (`gen-role-spells` onto proc's twin, or proto's placed
    /// one). A generator with no twin goes through with the same `-o`, as
    /// far as the absent install, and writes nothing: shown in a scratch
    /// checkout (its mark made), so a run let through could reach no file
    /// of this one.
    #[test]
    fn a_twin_named_from_o_is_checked_too() {
        let absent = ["--wow-dir", "/nonexistent", "--dbd-dir", "/nonexistent"];
        let cases = [
            (
                "gen-proc-spells",
                "crates/core/src/proc_spells",
                ".expected.md",
            ),
            (
                "gen-role-spells",
                "crates/core/src/proc_spells",
                ".expected.md",
            ),
            (
                "gen-role-spells",
                "crates/proto/src/placed/placed_spells",
                ".expected.md",
            ),
        ];
        for (name, stem, ext) in cases {
            let (path, twin) = (committed(stem), committed(&format!("{stem}{ext}")));
            let words = [&["--product", "wowf", "-o", path.as_str()][..], &absent[..]].concat();
            let e = run(name, &words);
            let head = format!("-o {path} writes {twin} beside it: the committed {stem}{ext} ");
            assert!(e.starts_with(&head), "{name}: {e}");
            assert!(e.ends_with(REFUSED), "{name}: {e}");
        }
        let scratch = std::env::temp_dir().join(format!(
            "wowdps-extract-twin-checkout-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&scratch);
        let mark = scratch.join(cli::owners::CHECKOUT_MARK);
        std::fs::create_dir_all(mark.parent().unwrap()).unwrap();
        std::fs::write(&mark, "").unwrap();
        let path = scratch.join("crates/core/src/proc_spells");
        let path = path.display().to_string();
        let words = [&["--product", "wowf", "-o", path.as_str()][..], &absent[..]].concat();
        let e = run("gen-proc-spells", &words);
        assert!(e.ends_with(REFUSED), "the scratch checkout's twin: {e}");
        let e = run("gen-class-spells", &words);
        assert!(e.contains(".build.info") && !e.contains("Retail's"), "{e}");
        assert!(!scratch.join("crates/core/src").exists(), "nothing written");
        std::fs::remove_dir_all(&scratch).unwrap();
    }

    /// Every generated table in the checkout, found by the header its
    /// generator writes into it rather than by `tablegen`'s list (so a
    /// table added without its entry there fails here), is claimed by
    /// exactly one edition's entry in that list, and its header names that
    /// edition's product where it is not Retail's; every table generator
    /// has its one Retail table. Each table and twin is refused to every
    /// other edition's run, and its own goes through to the (absent)
    /// install.
    #[test]
    fn every_generated_table_is_one_editions() {
        use std::io::BufRead;
        use wowdps_core::model::Edition;
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        // (first line, second line, path) of every generated Rust file
        // under crates/.
        let mut generated = Vec::new();
        let mut dirs = vec![root.join("crates")];
        while let Some(dir) = dirs.pop() {
            for entry in std::fs::read_dir(&dir).unwrap() {
                let entry = entry.unwrap();
                let path = entry.path();
                if entry.file_type().unwrap().is_dir() {
                    dirs.push(path);
                } else if path.extension().is_some_and(|x| x == "rs") {
                    let file = std::fs::File::open(&path).unwrap();
                    let mut lines = std::io::BufReader::new(file).lines();
                    let first = lines.next().and_then(Result::ok).unwrap_or_default();
                    let second = lines.next().and_then(Result::ok).unwrap_or_default();
                    if first.starts_with("//! GENERATED by tools/gen-") {
                        generated.push((first, second, path));
                    }
                }
            }
        }
        let gens: Vec<&Cmd> = tablegen::COMMANDS
            .iter()
            .filter(|c| c.name.starts_with("gen-"))
            .collect();
        let committed = tablegen::committed();
        let tables = committed
            .iter()
            .filter(|(_, rel)| rel.extension().is_some_and(|x| x == "rs"));
        assert_eq!(generated.len(), tables.count(), "{generated:?}");
        let editions = [Edition::Retail, Edition::Forever];
        let absent = ["--wow-dir", "/nonexistent", "--dbd-dir", "/nonexistent"];
        for (first, second, table) in &generated {
            let claims: Vec<_> = committed
                .iter()
                .filter(|(_, rel)| table.ends_with(rel))
                .collect();
            assert_eq!(claims.len(), 1, "{}: {claims:?}", table.display());
            let owner = claims[0].0;
            let product = owner.product().unwrap();
            let named = second.contains(&format!("(product {product})"));
            assert_eq!(
                named,
                owner != Edition::Retail,
                "{}: {second}",
                table.display()
            );
            let c = gens
                .iter()
                .find(|c| first.starts_with(&format!("//! GENERATED by tools/{}.sh ", c.name)))
                .unwrap();
            let twin = table.with_extension("expected.md");
            let files = [Some(table.clone()), twin.is_file().then_some(twin)];
            for file in files.into_iter().flatten() {
                let listed = committed
                    .iter()
                    .any(|(e, rel)| *e == owner && file.ends_with(rel));
                assert!(listed, "{}: {} not listed", c.name, file.display());
                let f = file.display().to_string();
                for e in editions {
                    let mine = e.product().unwrap();
                    let words = [&["--product", mine, "-o", &f][..], &absent[..]].concat();
                    let err = run(c.name, &words);
                    if e == owner {
                        // Through the guard: to the absent install, or for
                        // a census-backed table to the census it lacks.
                        let through =
                            err.contains(".build.info") || err.contains("requires --census");
                        assert!(through && !err.contains("needs -o"), "{}: {err}", c.name);
                    } else {
                        let tail = format!(
                            "is {}'s (product {product}); product {mine} needs -o naming another place",
                            owner.label()
                        );
                        assert!(err.ends_with(&tail), "{}: {err}", c.name);
                    }
                }
            }
        }
        for c in gens {
            let header = format!("//! GENERATED by tools/{}.sh ", c.name);
            let retail = generated
                .iter()
                .filter(|(first, second, _)| {
                    first.starts_with(&header) && !second.contains("(product ")
                })
                .count();
            assert_eq!(retail, 1, "{}", c.name);
        }
    }
}
