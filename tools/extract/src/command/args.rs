//! The extractor's command line: the words after a command, which give
//! themselves up as the command takes them, so whatever is left at the end
//! is the caller's mistake (`Args::finish`).
//!
//! A command takes its options that carry a value first (`opt`, `opts`,
//! `parse`), then its switches (`flag`), then its positional words
//! (`positionals`, or `Install`'s install directory): a value is the
//! word after its flag whatever it looks like, and taken first it can never
//! be mistaken for a switch or a positional.

use std::path::PathBuf;
use std::str::FromStr;

use super::Cmd;

/// A command's words not yet taken, the command they were given to, and
/// the binary that runs it.
pub struct Args {
    words: Vec<String>,
    /// Its usage, for the errors that need it, and the tables it reads
    /// (`Install::with_tables`).
    cmd: &'static Cmd,
    bin: &'static str,
}

/// A command's usage as its binary says it: the binary's name, then the
/// command's own words (`Cmd::usage`).
#[derive(Debug, Clone, Copy)]
pub struct Usage {
    pub bin: &'static str,
    pub words: &'static str,
}

impl std::fmt::Display for Usage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} {}", self.bin, self.words)
    }
}

/// Whether a word is a flag: a dash and then anything but a digit or a
/// point (`-20,-20` is a stencil's corner, `-` a file name for stdin).
fn is_flag(word: &str) -> bool {
    let mut c = word.chars();
    c.next() == Some('-') && c.next().is_some_and(|c| !(c.is_ascii_digit() || c == '.'))
}

impl Args {
    /// `words` given to `cmd` of the binary `bin`.
    pub fn new(bin: &'static str, words: Vec<String>, cmd: &'static Cmd) -> Args {
        Args { words, cmd, bin }
    }

    /// The command the words were given to.
    pub fn cmd(&self) -> &'static Cmd {
        self.cmd
    }

    /// The command's usage lines, as its binary says them.
    pub fn usage(&self) -> Usage {
        Usage {
            bin: self.bin,
            words: self.cmd.usage,
        }
    }

    /// Whether any of `names` was given; every one taken.
    pub fn flag(&mut self, names: &[&str]) -> bool {
        let before = self.words.len();
        self.words.retain(|w| !names.contains(&w.as_str()));
        self.words.len() != before
    }

    /// Every value given to any of `names`, in order, each with its flag
    /// taken. A flag with no word after it is an error.
    pub fn opts(&mut self, names: &[&str]) -> Result<Vec<String>, String> {
        let mut values = Vec::new();
        let mut kept = Vec::with_capacity(self.words.len());
        let mut words = std::mem::take(&mut self.words).into_iter();
        while let Some(w) = words.next() {
            if names.contains(&w.as_str()) {
                values.push(words.next().ok_or(format!("{w} needs a value"))?);
            } else {
                kept.push(w);
            }
        }
        self.words = kept;
        Ok(values)
    }

    /// The value given to any of `names`, the last where several are.
    pub fn opt(&mut self, names: &[&str]) -> Result<Option<String>, String> {
        Ok(self.opts(names)?.pop())
    }

    /// `opt` as a path.
    pub fn path(&mut self, names: &[&str]) -> Result<Option<PathBuf>, String> {
        Ok(self.opt(names)?.map(PathBuf::from))
    }

    /// `opt` read as a `T`; a value that is not one is an error naming the
    /// flag and the value.
    pub fn parse<T: FromStr>(&mut self, names: &[&str]) -> Result<Option<T>, String>
    where
        T::Err: std::fmt::Display,
    {
        let flag = names.first().copied().unwrap_or_default();
        self.opt(names)?
            .map(|v| v.parse().map_err(|e| format!("{flag} {v:?}: {e}")))
            .transpose()
    }

    /// `opts`, each read as a `T`.
    pub fn parse_all<T: FromStr>(&mut self, names: &[&str]) -> Result<Vec<T>, String>
    where
        T::Err: std::fmt::Display,
    {
        let flag = names.first().copied().unwrap_or_default();
        self.opts(names)?
            .into_iter()
            .map(|v| v.parse().map_err(|e| format!("{flag} {v:?}: {e}")))
            .collect()
    }

    /// Every word left that is no flag, in order, each taken.
    pub fn positionals(&mut self) -> Vec<String> {
        let (flags, words) = std::mem::take(&mut self.words)
            .into_iter()
            .partition(|w| is_flag(w));
        self.words = flags;
        words
    }

    /// The first word left that is no flag, taken.
    pub fn positional(&mut self) -> Option<String> {
        let at = self.words.iter().position(|w| !is_flag(w))?;
        Some(self.words.remove(at))
    }

    /// Nothing may be left: a word no one took (an unknown flag, a value
    /// for a flag that takes none, a word too many) is an error naming it,
    /// with the command's usage.
    pub fn finish(self) -> Result<(), String> {
        match self.words.first() {
            None => Ok(()),
            Some(w) => Err(usage_error(
                self.usage(),
                format!("unexpected argument {w:?}"),
            )),
        }
    }
}

/// A command line's mistake, and the command's usage under it.
pub fn usage_error(usage: Usage, what: impl std::fmt::Display) -> String {
    format!(
        "{what}
usage:
  {usage}"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A command with nothing to run, for its words.
    const CMD: Cmd = crate::command::cmd("cmd", "cmd [-o OUT]", |_| Ok(()));

    fn args(words: &[&str]) -> Args {
        Args::new("bin", words.iter().map(|w| w.to_string()).collect(), &CMD)
    }

    #[test]
    fn a_flag_is_taken_wherever_it_stands_and_however_often() {
        let mut a = args(&["x", "--debug", "y", "--debug"]);
        assert!(a.flag(&["--debug"]));
        assert!(!a.flag(&["--debug"]), "taken once, gone");
        assert!(!a.flag(&["--gpu"]));
        assert_eq!(a.positionals(), ["x", "y"]);
        assert!(a.finish().is_ok());
    }

    #[test]
    fn an_option_takes_the_word_after_it_and_the_last_one_wins() {
        let mut a = args(&["-o", "a", "dir", "--out", "b"]);
        assert_eq!(a.opt(&["-o", "--out"]).unwrap().as_deref(), Some("b"));
        assert_eq!(a.opt(&["-o"]).unwrap(), None, "both taken");
        assert_eq!(a.positional().as_deref(), Some("dir"));
        assert!(a.finish().is_ok());
    }

    #[test]
    fn an_option_takes_its_value_whatever_it_looks_like() {
        // A value that looks like a flag is still the value.
        let mut a = args(&["--title", "--debug", "--margin", "-5"]);
        assert_eq!(a.opt(&["--title"]).unwrap().as_deref(), Some("--debug"));
        assert_eq!(a.parse::<f32>(&["--margin"]).unwrap(), Some(-5.0));
        assert!(!a.flag(&["--debug"]));
        assert!(a.finish().is_ok());
    }

    #[test]
    fn an_option_with_no_value_is_an_error() {
        let mut a = args(&["x", "-o"]);
        assert_eq!(a.opt(&["-o"]).unwrap_err(), "-o needs a value");
    }

    #[test]
    fn a_repeated_option_keeps_every_value_in_order() {
        let mut a = args(&["--fdid", "1", "x", "--fdid", "2", "--fdid", "3"]);
        assert_eq!(a.parse_all::<u32>(&["--fdid"]).unwrap(), [1, 2, 3]);
        assert_eq!(a.positionals(), ["x"]);
    }

    #[test]
    fn a_value_that_does_not_parse_names_its_flag_and_itself() {
        let mut a = args(&["--ppy", "eight"]);
        let e = a.parse::<f32>(&["--ppy"]).unwrap_err();
        assert!(e.starts_with("--ppy \"eight\": "), "{e}");
        let mut a = args(&["--fdid", "1", "--fdid", "x"]);
        assert!(
            a.parse_all::<u32>(&["--fdid"])
                .unwrap_err()
                .starts_with("--fdid \"x\": ")
        );
    }

    #[test]
    fn positionals_leave_flags_and_keep_negative_numbers() {
        let mut a = args(&["poly", "-20,-20", "20,-20", "--bogus", "|", "c:5,5,12", "-"]);
        assert_eq!(
            a.positionals(),
            ["poly", "-20,-20", "20,-20", "|", "c:5,5,12", "-"]
        );
        let e = a.finish().unwrap_err();
        assert!(e.starts_with("unexpected argument \"--bogus\""), "{e}");
        assert!(
            e.contains("bin cmd [-o OUT]"),
            "with the command's usage: {e}"
        );
    }

    #[test]
    fn a_leftover_word_is_an_error_too() {
        let mut a = args(&["dir", "extra"]);
        assert_eq!(a.positional().as_deref(), Some("dir"));
        assert!(
            a.finish()
                .unwrap_err()
                .starts_with("unexpected argument \"extra\"")
        );
        assert!(args(&[]).finish().is_ok());
    }
}
