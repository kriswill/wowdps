//! Shared command-line handling: every frontend takes the same
//! `--file`/`--logs` flags and follows the same default logs directory.

use std::path::PathBuf;

use crate::tail::SourceSpec;

/// Verified location of the retail WoW logs under this machine's Proton prefix.
pub const DEFAULT_LOGS_DIR: &str = "/home/k/.local/share/Steam/steamapps/compatdata/3082075026/pfx/drive_c/Program Files (x86)/World of Warcraft/_retail_/Logs";

/// `Ok(None)` means "help was requested, don't start". Hand-rolled so the
/// frontends carry no argument-parsing dependency.
pub fn parse_args<I: IntoIterator<Item = String>>(args: I) -> Result<Option<SourceSpec>, String> {
    let mut file: Option<PathBuf> = None;
    let mut logs: Option<PathBuf> = None;
    let mut args = args.into_iter();

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "-h" | "--help" => return Ok(None),
            // Paths are taken verbatim: the default logs dir contains spaces.
            "-f" | "--file" => {
                file = Some(PathBuf::from(args.next().ok_or("--file needs a path")?));
            }
            "-l" | "--logs" => {
                logs = Some(PathBuf::from(
                    args.next().ok_or("--logs needs a directory")?,
                ));
            }
            other => return Err(format!("unknown argument {other:?}")),
        }
    }

    match (file, logs) {
        (Some(_), Some(_)) => Err("--file and --logs are mutually exclusive".to_string()),
        (Some(f), None) => Ok(Some(SourceSpec::File(f))),
        (None, Some(d)) => Ok(Some(SourceSpec::Dir(d))),
        (None, None) => Ok(Some(SourceSpec::Dir(PathBuf::from(DEFAULT_LOGS_DIR)))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(args: &[&str]) -> Result<Option<SourceSpec>, String> {
        parse_args(args.iter().map(|s| s.to_string()))
    }

    #[test]
    fn no_arguments_follows_the_default_logs_directory() {
        assert_eq!(
            parse(&[]),
            Ok(Some(SourceSpec::Dir(PathBuf::from(DEFAULT_LOGS_DIR))))
        );
    }

    #[test]
    fn file_and_logs_select_their_sources() {
        assert_eq!(
            parse(&["--file", "/tmp/a.txt"]),
            Ok(Some(SourceSpec::File(PathBuf::from("/tmp/a.txt"))))
        );
        assert_eq!(
            parse(&["--logs", "/tmp/logs"]),
            Ok(Some(SourceSpec::Dir(PathBuf::from("/tmp/logs"))))
        );
        assert_eq!(
            parse(&["-f", "/tmp/a.txt"]),
            Ok(Some(SourceSpec::File(PathBuf::from("/tmp/a.txt"))))
        );
    }

    #[test]
    fn paths_with_spaces_survive_intact() {
        let path = "/home/k/Program Files (x86)/World of Warcraft/_retail_/Logs";
        assert_eq!(
            parse(&["--logs", path]),
            Ok(Some(SourceSpec::Dir(PathBuf::from(path))))
        );
    }

    #[test]
    fn the_default_logs_directory_is_the_proton_wow_path() {
        assert!(DEFAULT_LOGS_DIR.ends_with("/World of Warcraft/_retail_/Logs"));
    }

    #[test]
    fn help_asks_for_no_source() {
        assert_eq!(parse(&["--help"]), Ok(None));
        assert_eq!(parse(&["-h"]), Ok(None));
    }

    #[test]
    fn conflicting_and_incomplete_arguments_are_rejected() {
        assert!(parse(&["--file", "a", "--logs", "b"]).is_err());
        assert!(parse(&["--file"]).is_err());
        assert!(parse(&["--logs"]).is_err());
        assert!(parse(&["--nonsense"]).is_err());
        assert!(parse(&["stray"]).is_err());
    }

    #[test]
    fn the_last_source_flag_wins_over_an_earlier_one_of_the_same_kind() {
        assert_eq!(
            parse(&["--file", "a", "--file", "b"]),
            Ok(Some(SourceSpec::File(PathBuf::from("b"))))
        );
    }
}
