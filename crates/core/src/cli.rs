//! Shared command-line handling: the `wowdps` dispatcher's subcommands.
//! Hand-rolled so the binaries carry no argument-parsing dependency. Also
//! hosts the machine-agnostic discovery of the game install, used when the
//! config carries no `logs_dir`.

use std::path::{Path, PathBuf};

pub use crate::tail::SourceSpec;

/// A WoW install root: the folder holding `.build.info` and `Data/data`.
pub fn is_wow_install(p: &Path) -> bool {
    p.join(".build.info").is_file() && p.join("Data").join("data").is_dir()
}

/// The retail logs directory to tail when no `logs_dir` is configured:
/// `$WOWDPS_WOW_DIR` when it names an install, else the most recently
/// updated install found under the conventional Steam roots. `None` when
/// nothing is found — the daemon reports that instead of tailing a
/// made-up path.
pub fn default_logs_dir() -> Option<PathBuf> {
    if let Some(dir) = std::env::var_os("WOWDPS_WOW_DIR") {
        let dir = PathBuf::from(dir);
        if is_wow_install(&dir) {
            return Some(retail_logs(&dir));
        }
    }
    let home = std::env::var_os("HOME").map(PathBuf::from)?;
    newest_install(discover_wow_installs(&home)).map(|w| retail_logs(&w))
}

fn retail_logs(wow: &Path) -> PathBuf {
    wow.join("_retail_").join("Logs")
}

/// WoW install roots under `home`'s conventional Steam locations' Proton
/// prefixes, deduplicated (the roots are often symlinks to one another).
pub fn discover_wow_installs(home: &Path) -> Vec<PathBuf> {
    let roots = [
        home.join(".local/share/Steam"),
        home.join(".steam/steam"),
        home.join(".steam/root"),
        home.join(".var/app/com.valvesoftware.Steam/.local/share/Steam"),
    ];
    let mut found: Vec<PathBuf> = Vec::new();
    for root in roots {
        let Ok(entries) = std::fs::read_dir(root.join("steamapps").join("compatdata")) else {
            continue;
        };
        for entry in entries.flatten() {
            let candidate = entry
                .path()
                .join("pfx/drive_c/Program Files (x86)/World of Warcraft");
            if is_wow_install(&candidate) {
                let canonical = candidate.canonicalize().unwrap_or(candidate);
                if !found.contains(&canonical) {
                    found.push(canonical);
                }
            }
        }
    }
    found
}

/// Of several installs, the one whose `.build.info` changed last — the
/// launcher rewrites it on update, so this tracks the install being played.
fn newest_install(installs: Vec<PathBuf>) -> Option<PathBuf> {
    installs.into_iter().max_by_key(|w| {
        std::fs::metadata(w.join(".build.info"))
            .and_then(|m| m.modified())
            .ok()
    })
}

/// What `wowdps` was asked to do: a git-style subcommand, or (with no
/// subcommand) the TUI client. A first word that isn't a known subcommand
/// dispatches to an external `wowdps-<name>` binary, args passed verbatim.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Cmd {
    Tui {
        source: Option<SourceSpec>,
    },
    Daemon {
        source: Option<SourceSpec>,
        linger: bool,
    },
    Gui {
        source: Option<SourceSpec>,
    },
    Stop,
    Status,
    External {
        name: String,
        args: Vec<String>,
    },
}

/// `Ok(None)` means "help was requested, don't start".
pub fn parse_args<I: IntoIterator<Item = String>>(args: I) -> Result<Option<Cmd>, String> {
    let mut args = args.into_iter();

    let Some(first) = args.next() else {
        return Ok(Some(Cmd::Tui { source: None }));
    };
    let cmd = match first.as_str() {
        "help" | "-h" | "--help" => return Ok(None),
        // The flags-were-modes era: point at the new spelling instead of a
        // generic unknown-argument error.
        old @ ("--daemon" | "--gui" | "--stop" | "--status") => {
            let sub = old.trim_start_matches('-');
            return Err(format!("{old} is now a subcommand: wowdps {sub}"));
        }
        word if !word.starts_with('-') => {
            match word {
                "daemon" => {
                    let (source, extra) = parse_tail(args)?;
                    let mut linger = false;
                    for arg in extra {
                        match arg.as_str() {
                            "--linger" => linger = true,
                            other => return Err(unknown(other)),
                        }
                    }
                    Cmd::Daemon { source, linger }
                }
                "gui" => {
                    let (source, extra) = parse_tail(args)?;
                    if let Some(other) = extra.first() {
                        return Err(unknown(other));
                    }
                    Cmd::Gui { source }
                }
                "stop" | "status" => {
                    if let Some(other) = args.next() {
                        return Err(format!("wowdps {word} takes no arguments (got {other:?})"));
                    }
                    if word == "stop" {
                        Cmd::Stop
                    } else {
                        Cmd::Status
                    }
                }
                // Not ours: `wowdps foo ...` runs `wowdps-foo ...`, which owns
                // its own arguments — pass the tail through untouched.
                _ => Cmd::External {
                    name: word.to_string(),
                    args: args.collect(),
                },
            }
        }
        // A leading flag is the TUI client's.
        _ => {
            let (source, extra) = parse_tail(std::iter::once(first.clone()).chain(args))?;
            if let Some(other) = extra.first() {
                return Err(unknown(other));
            }
            Cmd::Tui { source }
        }
    };
    Ok(Some(cmd))
}

fn unknown(arg: &str) -> String {
    format!("unknown argument {arg:?}")
}

/// Consume a mode's tail: `--file`/`--logs` are parsed here (mutually
/// exclusive, last of a kind wins, paths taken verbatim — the default logs
/// dir contains spaces). Everything else is returned for the mode to judge;
/// an unjudged leftover becomes an error, which prints the usage anyway.
fn parse_tail<I: Iterator<Item = String>>(
    mut args: I,
) -> Result<(Option<SourceSpec>, Vec<String>), String> {
    let mut file: Option<PathBuf> = None;
    let mut logs: Option<PathBuf> = None;
    let mut extra = Vec::new();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "-f" | "--file" => {
                file = Some(PathBuf::from(args.next().ok_or("--file needs a path")?));
            }
            "-l" | "--logs" => {
                logs = Some(PathBuf::from(
                    args.next().ok_or("--logs needs a directory")?,
                ));
            }
            _ => extra.push(arg),
        }
    }
    let source = match (file, logs) {
        (Some(_), Some(_)) => return Err("--file and --logs are mutually exclusive".to_string()),
        (Some(f), None) => Some(SourceSpec::File(f)),
        (None, Some(d)) => Some(SourceSpec::Dir(d)),
        (None, None) => None,
    };
    Ok((source, extra))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(args: &[&str]) -> Result<Option<Cmd>, String> {
        parse_args(args.iter().map(|s| s.to_string()))
    }

    fn ok(args: &[&str]) -> Cmd {
        parse(args).unwrap().unwrap()
    }

    #[test]
    fn no_arguments_is_the_plain_tui_client() {
        // The daemon's config picks the source.
        assert_eq!(ok(&[]), Cmd::Tui { source: None });
    }

    #[test]
    fn file_and_logs_select_the_tui_sources() {
        assert_eq!(
            ok(&["--file", "/tmp/a.txt"]),
            Cmd::Tui {
                source: Some(SourceSpec::File(PathBuf::from("/tmp/a.txt")))
            }
        );
        assert_eq!(
            ok(&["--logs", "/tmp/logs"]),
            Cmd::Tui {
                source: Some(SourceSpec::Dir(PathBuf::from("/tmp/logs")))
            }
        );
        assert_eq!(
            ok(&["-f", "/tmp/a.txt"]),
            Cmd::Tui {
                source: Some(SourceSpec::File(PathBuf::from("/tmp/a.txt")))
            }
        );
    }

    #[test]
    fn paths_with_spaces_survive_intact() {
        let path = "/home/user/Program Files (x86)/World of Warcraft/_retail_/Logs";
        assert_eq!(
            ok(&["--logs", path]),
            Cmd::Tui {
                source: Some(SourceSpec::Dir(PathBuf::from(path)))
            }
        );
    }

    #[test]
    fn install_discovery_scans_steam_prefixes_and_prefers_the_newest() {
        let home = std::env::temp_dir().join(format!("wowdps-cli-test-{}", std::process::id()));
        let mk = |steam_root: &str, appid: &str| {
            let wow = home
                .join(steam_root)
                .join("steamapps/compatdata")
                .join(appid)
                .join("pfx/drive_c/Program Files (x86)/World of Warcraft");
            std::fs::create_dir_all(wow.join("Data").join("data")).unwrap();
            std::fs::write(wow.join(".build.info"), "x").unwrap();
            wow
        };

        assert!(discover_wow_installs(&home).is_empty());

        let old = mk(".local/share/Steam", "111");
        assert!(!is_wow_install(&home)); // only real install roots qualify
        assert!(is_wow_install(&old));

        let new = mk(".local/share/Steam", "222");
        // Make "old" genuinely older than "new".
        let past = std::time::SystemTime::now() - std::time::Duration::from_secs(3600);
        std::fs::File::options()
            .write(true)
            .open(old.join(".build.info"))
            .unwrap()
            .set_modified(past)
            .unwrap();

        let found = discover_wow_installs(&home);
        assert_eq!(found.len(), 2);
        let picked = newest_install(found).unwrap();
        assert_eq!(picked, new);
        assert!(
            retail_logs(&picked).ends_with("World of Warcraft/_retail_/Logs"),
            "{}",
            retail_logs(&picked).display()
        );

        std::fs::remove_dir_all(&home).unwrap();
    }

    #[test]
    fn the_daemon_subcommand_takes_linger_and_a_source() {
        assert_eq!(
            ok(&["daemon", "--linger", "--file", "/tmp/a.txt"]),
            Cmd::Daemon {
                source: Some(SourceSpec::File(PathBuf::from("/tmp/a.txt"))),
                linger: true
            }
        );
        assert_eq!(
            ok(&["daemon"]),
            Cmd::Daemon {
                source: None,
                linger: false
            }
        );
    }

    #[test]
    fn the_gui_subcommand_takes_a_source_but_no_linger() {
        assert_eq!(
            ok(&["gui", "--logs", "/tmp/logs"]),
            Cmd::Gui {
                source: Some(SourceSpec::Dir(PathBuf::from("/tmp/logs")))
            }
        );
        assert!(parse(&["gui", "--linger"]).is_err());
    }

    #[test]
    fn linger_outside_the_daemon_subcommand_is_rejected() {
        assert!(parse(&["--linger"]).is_err());
        assert!(parse(&["--file", "/tmp/a.txt", "--linger"]).is_err());
    }

    #[test]
    fn stop_and_status_take_nothing() {
        assert_eq!(ok(&["stop"]), Cmd::Stop);
        assert_eq!(ok(&["status"]), Cmd::Status);
        assert!(parse(&["stop", "--file", "/tmp/a.txt"]).is_err());
        assert!(parse(&["status", "extra"]).is_err());
    }

    #[test]
    fn old_mode_flags_point_at_their_subcommands() {
        for old in ["--daemon", "--gui", "--stop", "--status"] {
            let err = parse(&[old]).unwrap_err();
            assert!(
                err.contains(&format!("wowdps {}", old.trim_start_matches('-'))),
                "{old}: {err}"
            );
        }
    }

    #[test]
    fn an_unknown_word_dispatches_externally_with_its_tail_verbatim() {
        assert_eq!(
            ok(&["extract", "csv", "--file", "Spell"]),
            Cmd::External {
                name: "extract".to_string(),
                // Even flags we know are passed through: the tail is not ours.
                args: vec!["csv".to_string(), "--file".to_string(), "Spell".to_string()]
            }
        );
        assert_eq!(
            ok(&["gen-icons"]),
            Cmd::External {
                name: "gen-icons".to_string(),
                args: vec![]
            }
        );
    }

    #[test]
    fn help_asks_for_nothing() {
        assert_eq!(parse(&["--help"]), Ok(None));
        assert_eq!(parse(&["-h"]), Ok(None));
        assert_eq!(parse(&["help"]), Ok(None));
    }

    #[test]
    fn conflicting_and_incomplete_arguments_are_rejected() {
        assert!(parse(&["--file", "a", "--logs", "b"]).is_err());
        assert!(parse(&["--file"]).is_err());
        assert!(parse(&["--logs"]).is_err());
        assert!(parse(&["--nonsense"]).is_err());
        assert!(parse(&["daemon", "--nonsense"]).is_err());
    }

    #[test]
    fn the_last_source_flag_wins_over_an_earlier_one_of_the_same_kind() {
        assert_eq!(
            ok(&["--file", "a", "--file", "b"]),
            Cmd::Tui {
                source: Some(SourceSpec::File(PathBuf::from("b")))
            }
        );
    }
}
