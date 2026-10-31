//! The extractor's command line, which every extractor binary runs its
//! commands on: a command (`Cmd`: its name, its usage, what runs it, the
//! client tables it reads), the words given to it (`args`), the install
//! its files come from (`install`), and a binary's whole list
//! (`Commands`: its usage, the `tables` command, the dispatch).

pub mod args;
pub mod install;

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use args::{Args, usage_error};

/// An error about a file, named by its path: `.map_err(at(path))`.
pub fn at<E: std::fmt::Display>(path: &Path) -> impl Fn(E) -> String + '_ {
    move |e| format!("{}: {e}", path.display())
}

/// A file read as text, its error naming it.
pub fn read_text(path: &Path) -> Result<String, String> {
    std::fs::read_to_string(path).map_err(at(path))
}

/// A directory made with its parents, its error naming it.
pub fn make_dir(dir: &Path) -> Result<(), String> {
    std::fs::create_dir_all(dir).map_err(at(dir))
}

/// `$XDG_DATA_HOME/wowdps/<name>` (or the ~/.local/share fallback, through
/// `wowdps_proto::dirs`) — where the per-machine caches live.
pub fn data_path(name: &str) -> Result<PathBuf, String> {
    wowdps_proto::dirs::data_path(name).ok_or_else(|| "no XDG_DATA_HOME or HOME".to_string())
}

/// One command: its name, its usage (the words after the binary's name, a
/// continuation line indented to stand under them), what runs it, and the
/// client tables it reads. Each module lists its own (`COMMANDS`), beside
/// the code that reads its flags; a binary gathers them (`Commands`).
pub struct Cmd {
    pub name: &'static str,
    pub usage: &'static str,
    pub run: fn(Args) -> Result<(), String>,
    /// The tables it reads, in the lists it loads them from, in order:
    /// what the `tables` command names, for tools/lib.sh to fetch the
    /// schemas of, and what its runner reads (`Install::open_tables`, from
    /// the `Args` it is given). None for a command that reads no table.
    pub tables: &'static [&'static [&'static str]],
}

/// A command that reads no table.
pub const fn cmd(
    name: &'static str,
    usage: &'static str,
    run: fn(Args) -> Result<(), String>,
) -> Cmd {
    Cmd {
        name,
        usage,
        run,
        tables: &[],
    }
}

impl Cmd {
    /// The command, reading `tables`.
    pub const fn reads(self, tables: &'static [&'static [&'static str]]) -> Cmd {
        Cmd { tables, ..self }
    }

    /// The tables it reads, each once, in the order it loads them.
    pub fn table_names(&self) -> Vec<&'static str> {
        let mut names: Vec<&'static str> = Vec::new();
        for &name in self.tables.iter().flat_map(|t| t.iter()) {
            if !names.contains(&name) {
                names.push(name);
            }
        }
        names
    }
}

/// The `tables` command every binary answers about its own commands
/// (`Commands::tables`), run by `run`.
pub const fn tables_cmd(run: fn(Args) -> Result<(), String>) -> Cmd {
    cmd(
        "tables",
        "tables COMMAND   (the client tables it reads, one a line: tools/lib.sh fetches their schemas)",
        run,
    )
}

/// What the usage says after the commands.
const NOTES: &str = "fetch and the gen-* commands read the local install's CASC storage (no
network): wow-dir (or --wow-dir DIR) is the folder containing .build.info
and Data/. When omitted, the install is located via $WOWDPS_WOW_DIR, the
wowdps config's logs_dir, or a scan of the install roots (Steam compatdata
on Linux, /Applications and ~/Applications on macOS). --keys takes TACT
keys in wowdev TACTKeys format; without a key, encrypted chunks decode to
zeroes exactly like the game client. The gen-* commands expect --dbd-dir
to hold <Table>.dbd schemas for the tables they read (the tools/gen-*.sh
wrappers download them).";

/// A binary's commands: its name, and every command it runs, in the order
/// its usage lists them (each module's own list, its `tables` last).
pub struct Commands {
    pub bin: &'static str,
    pub lists: &'static [&'static [Cmd]],
}

impl Commands {
    /// Every command, in the order the usage lists them.
    pub fn iter(&self) -> impl Iterator<Item = &'static Cmd> + use<> {
        let lists: &'static [&'static [Cmd]] = self.lists;
        lists.iter().flat_map(|c| c.iter())
    }

    /// The command named `name`.
    pub fn find(&self, name: &str) -> Option<&'static Cmd> {
        self.iter().find(|c| c.name == name)
    }

    /// The usage of every command, then the notes.
    pub fn usage(&self) -> String {
        let mut out = String::from("usage:\n");
        for c in self.iter() {
            out.push_str(&format!("  {} {}\n", self.bin, c.usage));
        }
        out.push('\n');
        out.push_str(NOTES);
        out
    }

    /// The words a command was given, as it takes them.
    pub fn args(&self, words: Vec<String>, cmd: &'static Cmd) -> Args {
        Args::new(self.bin, words, cmd)
    }

    /// The `tables` command: the client tables a command reads, one a
    /// line, in the order it loads them; none for a command that reads no
    /// table.
    pub fn tables(&self, mut a: Args) -> Result<(), String> {
        let name = a.positional();
        let usage = a.usage();
        a.finish()?;
        let name = name.ok_or_else(|| usage_error(usage, "tables needs a command"))?;
        let c = self
            .find(&name)
            .ok_or_else(|| format!("tables: no command {name:?}"))?;
        for t in c.table_names() {
            println!("{t}");
        }
        Ok(())
    }

    /// The binary's `main`: the command its first word names, run on the
    /// words after it; no such command, the usage. An error is said on
    /// stderr under the binary's name.
    pub fn main(&self) -> ExitCode {
        let mut words = std::env::args().skip(1);
        let name = words.next().unwrap_or_default();
        let ran = match self.find(&name) {
            Some(cmd) => (cmd.run)(self.args(words.collect(), cmd)),
            None => Err(self.usage()),
        };
        match ran {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("{}: {e}", self.bin);
                ExitCode::FAILURE
            }
        }
    }
}
