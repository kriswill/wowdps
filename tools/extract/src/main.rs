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
}
