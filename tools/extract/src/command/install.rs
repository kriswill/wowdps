//! Where the game's files come from, as a command line names them: the
//! install, its TACT keys and the WoWDBDefs schemas (`Install`), and the
//! client tables read through them (`load_table`, `fetch_db2`,
//! `read_dbd`).

use std::path::{Path, PathBuf};

use super::args::Args;
use super::read_text;
use crate::dbd::Dbd;
use crate::game::Game;
use crate::table;
use crate::wdc5::Db2;

/// What a command reading tables says without `--dbd-dir`.
const NO_DBD_DIR: &str = "--dbd-dir DIR is needed: the WoWDBDefs schemas of the tables read";

/// Where the game's files come from: the install (`--wow-dir DIR`, or the
/// one word a command leaves, else found by `game::locate`), the TACT keys
/// for its encrypted files (`--keys FILE`) and the WoWDBDefs schemas its
/// tables decode by (`--dbd-dir DIR`), and the tables the command reads.
pub struct Install {
    pub dir: Option<PathBuf>,
    pub keys: Option<PathBuf>,
    pub dbd_dir: Option<PathBuf>,
    /// The command's tables (`Cmd::tables`), each once, in the order it
    /// loads them: none for a command that reads no table.
    pub tables: Vec<&'static str>,
}

impl Install {
    /// The install's directory and keys, for a command that reads its
    /// files and no table. Taken after the command's own options: the
    /// directory may be the one positional word left.
    pub fn files(args: &mut Args) -> Result<Install, String> {
        Install::take(args, false)
    }

    /// `files`, the schemas' directory and the tables the command reads,
    /// for a command that reads tables.
    pub fn with_tables(args: &mut Args) -> Result<Install, String> {
        Install::take(args, true)
    }

    fn take(args: &mut Args, tables: bool) -> Result<Install, String> {
        let dir = args.path(&["--wow-dir"])?;
        let keys = args.path(&["--keys"])?;
        let dbd_dir = if tables {
            let dir = args.path(&["--dbd-dir"])?;
            Some(dir.ok_or(NO_DBD_DIR)?)
        } else {
            None
        };
        let dir = match dir {
            Some(d) => Some(d),
            None => args.positional().map(PathBuf::from),
        };
        let tables = if tables {
            args.cmd().table_names()
        } else {
            Vec::new()
        };
        Ok(Install {
            dir,
            keys,
            dbd_dir,
            tables,
        })
    }

    /// The install's storage, opened.
    pub fn open(&self) -> Result<Game, String> {
        let dir = match &self.dir {
            Some(d) => d.clone(),
            None => crate::game::locate()?,
        };
        Game::open(&dir, self.keys.as_deref())
    }

    /// The schemas' directory, which a command reading tables needs.
    pub fn dbd_dir(&self) -> Result<&Path, String> {
        self.dbd_dir.as_deref().ok_or_else(|| NO_DBD_DIR.into())
    }

    /// The install opened and the command's tables read from it, as the
    /// command declares them (`Cmd::reads`): the schemas' directory asked
    /// for first, before the install is looked for.
    pub fn open_tables(&self) -> Result<(Game, table::Tables), String> {
        let dbd_dir = self.dbd_dir()?;
        let game = self.open()?;
        let tables = load_tables(&game, dbd_dir, &self.tables)?;
        Ok((game, tables))
    }
}

/// A client table's file from the install, by its FileDataID, parsed.
pub fn fetch_db2(game: &Game, name: &str, fdid: u32) -> Result<Db2, String> {
    Db2::parse(game.fetch_fdid(fdid, 0x2)?).map_err(|e| format!("{name}: {e}"))
}

/// A table's WoWDBDefs schema, read from its `.dbd` file and parsed.
pub fn read_dbd(path: &Path, name: &str) -> Result<Dbd, String> {
    Dbd::parse(&read_text(path)?).map_err(|e| format!("{name}: {e}"))
}

/// Fetch one table from the install by name (its FileDataID from
/// `table::FILE_IDS`) and decode it to cells.
pub fn load_table(game: &Game, dbd_dir: &Path, name: &str) -> Result<table::Csv, String> {
    let csv = table::load(game, dbd_dir, name)?;
    eprintln!("{name}: {} rows", csv.rows.len());
    Ok(csv)
}

/// `load_table` for each of `names`, in order.
pub fn load_tables(game: &Game, dbd_dir: &Path, names: &[&str]) -> Result<table::Tables, String> {
    let mut tables = table::Tables::default();
    for &name in names {
        tables.insert(name, load_table(game, dbd_dir, name)?);
    }
    Ok(tables)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A command reading two tables, with nothing to run.
    const CMD: crate::command::Cmd = crate::command::cmd("cmd", "cmd [-o OUT]", |_| Ok(()))
        .reads(&[&["Map", "Light"], &["Map"]]);

    fn args(words: &[&str]) -> Args {
        Args::new("bin", words.iter().map(|w| w.to_string()).collect(), &CMD)
    }

    #[test]
    fn the_install_is_its_flag_or_the_one_word_left() {
        let mut a = args(&["--keys", "k.txt", "/wow", "--dbd-dir", "dbd"]);
        let i = Install::with_tables(&mut a).unwrap();
        assert_eq!(i.dir, Some(PathBuf::from("/wow")));
        assert_eq!(i.keys, Some(PathBuf::from("k.txt")));
        assert_eq!(i.dbd_dir, Some(PathBuf::from("dbd")));
        assert_eq!(i.tables, ["Map", "Light"], "the command's, each once");
        assert!(a.finish().is_ok());
        let mut a = args(&["--wow-dir", "/wow", "stray"]);
        let i = Install::files(&mut a).unwrap();
        assert_eq!(i.dir, Some(PathBuf::from("/wow")));
        assert!(a.finish().is_err(), "the word left is no install");
        let i = Install::files(&mut args(&[])).unwrap();
        assert!(i.dir.is_none() && i.keys.is_none() && i.tables.is_empty());
        assert!(i.dbd_dir().unwrap_err().contains("--dbd-dir"));
        let e = Install::with_tables(&mut args(&["/wow"])).err().unwrap();
        assert!(e.contains("--dbd-dir"), "{e}");
        let mut a = args(&["--dbd-dir", "dbd"]);
        Install::files(&mut a).unwrap();
        assert!(
            a.finish().is_err(),
            "a command reading no table takes no --dbd-dir"
        );
    }
}
