//! Shared command-line handling: the `wowdps` dispatcher's flags. Hand-rolled
//! so the binaries carry no argument-parsing dependency. Also hosts the
//! machine-agnostic discovery of the game install, used when the config
//! carries no `logs_dir`.

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

/// What `wowdps` was asked to do. Exactly one mode: daemon, gui launcher,
/// stop, status — or, with none of those, the TUI client.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Args {
    /// `--file`/`--logs`: overrides the daemon's config `logs_dir`. On the
    /// client path it is forwarded to the daemon it spawns — and it is a hard
    /// error if a running daemon follows something else.
    pub source: Option<SourceSpec>,
    pub daemon: bool,
    pub gui: bool,
    pub overlay: bool,
    pub linger: bool,
    pub stop: bool,
    pub status: bool,
}

/// `Ok(None)` means "help was requested, don't start".
pub fn parse_args<I: IntoIterator<Item = String>>(args: I) -> Result<Option<Args>, String> {
    let mut out = Args::default();
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
            "--daemon" => out.daemon = true,
            "--gui" => out.gui = true,
            "--overlay" => out.overlay = true,
            "--linger" => out.linger = true,
            "--stop" => out.stop = true,
            "--status" => out.status = true,
            other => return Err(format!("unknown argument {other:?}")),
        }
    }

    out.source = match (file, logs) {
        (Some(_), Some(_)) => return Err("--file and --logs are mutually exclusive".to_string()),
        (Some(f), None) => Some(SourceSpec::File(f)),
        (None, Some(d)) => Some(SourceSpec::Dir(d)),
        (None, None) => None,
    };

    if out.linger && !out.daemon {
        return Err("--linger only makes sense with --daemon".to_string());
    }
    let modes = [out.daemon, out.gui, out.stop, out.status]
        .iter()
        .filter(|&&m| m)
        .count();
    if modes > 1 {
        return Err("pick one of --daemon, --gui, --stop, --status".to_string());
    }
    if (out.stop || out.status) && out.source.is_some() {
        return Err("--stop/--status take no source flags".to_string());
    }

    Ok(Some(out))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(args: &[&str]) -> Result<Option<Args>, String> {
        parse_args(args.iter().map(|s| s.to_string()))
    }

    fn ok(args: &[&str]) -> Args {
        parse(args).unwrap().unwrap()
    }

    #[test]
    fn no_arguments_is_the_plain_tui_client() {
        let args = ok(&[]);
        assert_eq!(args, Args::default());
        assert!(
            args.source.is_none(),
            "the daemon's config picks the source"
        );
    }

    #[test]
    fn file_and_logs_select_their_sources() {
        assert_eq!(
            ok(&["--file", "/tmp/a.txt"]).source,
            Some(SourceSpec::File(PathBuf::from("/tmp/a.txt")))
        );
        assert_eq!(
            ok(&["--logs", "/tmp/logs"]).source,
            Some(SourceSpec::Dir(PathBuf::from("/tmp/logs")))
        );
        assert_eq!(
            ok(&["-f", "/tmp/a.txt"]).source,
            Some(SourceSpec::File(PathBuf::from("/tmp/a.txt")))
        );
    }

    #[test]
    fn paths_with_spaces_survive_intact() {
        let path = "/home/user/Program Files (x86)/World of Warcraft/_retail_/Logs";
        assert_eq!(
            ok(&["--logs", path]).source,
            Some(SourceSpec::Dir(PathBuf::from(path)))
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
    fn the_daemon_mode_takes_linger_and_a_source() {
        let args = ok(&["--daemon", "--linger", "--file", "/tmp/a.txt"]);
        assert!(args.daemon && args.linger);
        assert_eq!(
            args.source,
            Some(SourceSpec::File(PathBuf::from("/tmp/a.txt")))
        );
    }

    #[test]
    fn modes_are_mutually_exclusive() {
        assert!(parse(&["--daemon", "--gui"]).is_err());
        assert!(parse(&["--stop", "--status"]).is_err());
        assert!(parse(&["--gui", "--status"]).is_err());
    }

    #[test]
    fn linger_without_daemon_is_rejected() {
        assert!(parse(&["--linger"]).is_err());
    }

    #[test]
    fn stop_and_status_take_no_source() {
        assert!(parse(&["--stop", "--file", "/tmp/a.txt"]).is_err());
        assert!(parse(&["--status", "--logs", "/tmp"]).is_err());
        assert!(ok(&["--stop"]).stop);
        assert!(ok(&["--status"]).status);
    }

    #[test]
    fn help_asks_for_nothing() {
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
            ok(&["--file", "a", "--file", "b"]).source,
            Some(SourceSpec::File(PathBuf::from("b")))
        );
    }
}
