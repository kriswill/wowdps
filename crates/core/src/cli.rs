//! Shared command-line handling: the `wowdps` dispatcher's subcommands.
//! Hand-rolled so the binaries carry no argument-parsing dependency. Also
//! hosts the machine-agnostic discovery of the game install, used when the
//! config carries no `logs_dir`: Battle.net's folders on macOS; elsewhere
//! Steam's Proton prefixes and the Wine prefixes under `~/.local`,
//! `~/Games`, `~/.wine` and flatpak app data.

use std::path::{Path, PathBuf};

pub use crate::tail::SourceSpec;

/// A WoW install root: the folder holding `.build.info` and `Data/data`.
pub fn is_wow_install(p: &Path) -> bool {
    p.join(".build.info").is_file() && p.join("Data").join("data").is_dir()
}

/// The retail logs directory to tail when no `logs_dir` is configured:
/// `$WOWDPS_WOW_DIR` when it names an install, else the most recently
/// updated install found under the conventional install roots (Battle.net's
/// on macOS, Steam's Proton prefixes and Wine prefixes elsewhere). `None`
/// when nothing is found — the daemon reports that instead of tailing a
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

/// WoW installs under `home`'s conventional roots, deduplicated (the
/// roots are often symlinks to one another, and the prefix walk reaches
/// Steam's prefixes a second time): the Battle.net folders on macOS
/// (`/Applications`, `~/Applications`); elsewhere the Steam roots' Proton
/// prefixes, then every Wine prefix that `wine_prefixes` finds under
/// `WINE_PREFIX_ROOTS`, each searched by `prefix_candidates`.
pub fn discover_wow_installs(home: &Path) -> Vec<PathBuf> {
    let mut found: Vec<PathBuf> = Vec::new();
    fn consider(candidate: PathBuf, found: &mut Vec<PathBuf>) {
        if is_wow_install(&candidate) {
            let canonical = candidate.canonicalize().unwrap_or(candidate);
            if !found.contains(&canonical) {
                found.push(canonical);
            }
        }
    }
    #[cfg(target_os = "macos")]
    for root in [PathBuf::from("/Applications"), home.join("Applications")] {
        consider(root.join("World of Warcraft"), &mut found);
    }
    #[cfg(not(target_os = "macos"))]
    {
        let roots = [
            home.join(".local/share/Steam"),
            home.join(".steam/steam"),
            home.join(".steam/root"),
            home.join(".var/app/com.valvesoftware.Steam/.local/share/Steam"),
        ];
        for root in roots {
            let Ok(entries) = std::fs::read_dir(root.join("steamapps").join("compatdata")) else {
                continue;
            };
            for entry in entries.flatten() {
                let candidate = entry
                    .path()
                    .join("pfx/drive_c/Program Files (x86)/World of Warcraft");
                consider(candidate, &mut found);
            }
        }
        for prefix in wine_prefixes(WINE_PREFIX_ROOTS.map(|root| home.join(root))) {
            for candidate in prefix_candidates(&prefix) {
                consider(candidate, &mut found);
            }
        }
    }
    found
}

/// Where the prefix walk starts, under home: launcher data in `~/.local`
/// (Steam's and hand-made Proton prefixes, Bottles), `~/Games` (Lutris',
/// Heroic's and umu's default), `~/.wine` (plain Wine's default prefix,
/// itself a prefix) and `~/.var/app` (the same launchers as flatpaks).
#[cfg(not(target_os = "macos"))]
const WINE_PREFIX_ROOTS: [&str; 4] = [".local", "Games", ".wine", ".var/app"];

/// How far below a walk root a prefix's `drive_c` may sit. Steam's
/// `~/.local/share/Steam/steamapps/compatdata/<id>/pfx/drive_c` is 7 levels
/// below `~/.local`, the deepest common layout (Heroic's
/// `~/Games/Heroic/Prefixes/default/<name>/pfx/drive_c` is 6 below
/// `~/Games`). A flatpak'd Steam sits deeper; the Steam scan covers it.
#[cfg(not(target_os = "macos"))]
const DRIVE_C_MAX_DEPTH: usize = 7;

/// How many directories the walk may visit, all roots together, so a huge
/// tree (a container store, a game library) can't stall startup. A typical
/// home needs a few thousand, and breadth-first order finds shallow
/// prefixes before the budget runs out.
#[cfg(not(target_os = "macos"))]
const PREFIX_WALK_BUDGET: usize = 20_000;

/// Wine prefixes (directories holding `drive_c`) under `roots`, walked
/// breadth-first across all of them at once. A root that is itself a
/// prefix counts (`~/.wine`), and a root may be a symlink; below a root no
/// symlink is followed (links loop, or lead out of home), a prefix is never
/// descended into, and an unreadable directory is skipped.
#[cfg(not(target_os = "macos"))]
fn wine_prefixes(roots: impl IntoIterator<Item = PathBuf>) -> Vec<PathBuf> {
    let mut prefixes = Vec::new();
    let mut queue: std::collections::VecDeque<(PathBuf, usize)> =
        roots.into_iter().map(|root| (root, 0)).collect();
    let mut budget = PREFIX_WALK_BUDGET;
    while let Some((dir, depth)) = queue.pop_front() {
        let Some(left) = budget.checked_sub(1) else {
            break;
        };
        budget = left;
        if dir.join("drive_c").is_dir() {
            prefixes.push(dir);
            continue;
        }
        // A child's own drive_c would sit at depth + 2: past the cap, the
        // children are not worth reading.
        if depth + 2 > DRIVE_C_MAX_DEPTH {
            continue;
        }
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            // `file_type` doesn't follow symlinks, so a link is never walked.
            if entry.file_type().is_ok_and(|t| t.is_dir()) {
                queue.push_back((entry.path(), depth + 1));
            }
        }
    }
    prefixes
}

/// Where a prefix may hold the game: `drive_c/World of Warcraft`, or one
/// folder down (`Program Files (x86)`, `Program Files`, a custom `Games`).
#[cfg(not(target_os = "macos"))]
fn prefix_candidates(prefix: &Path) -> Vec<PathBuf> {
    let drive_c = prefix.join("drive_c");
    let mut candidates = vec![drive_c.join("World of Warcraft")];
    if let Ok(entries) = std::fs::read_dir(&drive_c) {
        candidates.extend(
            entries
                .flatten()
                .map(|e| e.path().join("World of Warcraft")),
        );
    }
    candidates
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
    /// `wowdps addon [status | install]`: the wowdps addon in the game's
    /// AddOns folder — report it, or write it (the one time the user must
    /// ask; the daemon only ever updates a copy that is already there).
    Addon {
        install: bool,
    },
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
                "addon" => {
                    let install = match args.next().as_deref() {
                        None | Some("status") => false,
                        Some("install") => true,
                        Some(other) => {
                            return Err(format!(
                                "wowdps addon takes `status` or `install` (got {other:?})"
                            ));
                        }
                    };
                    if let Some(other) = args.next() {
                        return Err(format!("wowdps addon: unexpected argument {other:?}"));
                    }
                    Cmd::Addon { install }
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
    match arg {
        // The flags-were-modes era: point at the new spelling instead of a
        // generic unknown-argument error, wherever in the line the flag sits.
        old @ ("--daemon" | "--gui" | "--stop" | "--status") => {
            let sub = old.trim_start_matches('-');
            format!("{old} is now a subcommand: wowdps {sub}")
        }
        _ => format!("unknown argument {arg:?}"),
    }
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

    // The compatdata roots exist only off macOS, where discovery checks
    // /Applications — a machine with the game installed would leak into
    // these assertions.
    #[cfg(not(target_os = "macos"))]
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

    /// A fresh, empty home for one prefix-walk test.
    #[cfg(not(target_os = "macos"))]
    fn scratch_home(name: &str) -> PathBuf {
        let home =
            std::env::temp_dir().join(format!("wowdps-cli-wine-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&home);
        std::fs::create_dir_all(&home).unwrap();
        home
    }

    /// An install at `wow`, returned canonical (as discovery reports it).
    #[cfg(not(target_os = "macos"))]
    fn mk_install(wow: &Path) -> PathBuf {
        std::fs::create_dir_all(wow.join("Data").join("data")).unwrap();
        std::fs::write(wow.join(".build.info"), "x").unwrap();
        wow.canonicalize().unwrap()
    }

    #[cfg(not(target_os = "macos"))]
    #[test]
    fn the_prefix_walk_finds_a_launchers_proton_prefix() {
        let home = scratch_home("proton");
        let wow = mk_install(
            &home
                .join(".local/share/emberlaunch/pfx/drive_c/Program Files (x86)/World of Warcraft"),
        );
        assert_eq!(discover_wow_installs(&home), vec![wow]);
        std::fs::remove_dir_all(&home).unwrap();
    }

    #[cfg(not(target_os = "macos"))]
    #[test]
    fn the_prefix_walk_finds_a_plain_wine_prefix() {
        // No `pfx` level: the prefix is the launcher's game folder itself.
        let home = scratch_home("plain");
        let wow =
            mk_install(&home.join("Games/mossy-vale/drive_c/Program Files/World of Warcraft"));
        assert_eq!(discover_wow_installs(&home), vec![wow]);
        std::fs::remove_dir_all(&home).unwrap();
    }

    #[cfg(not(target_os = "macos"))]
    #[test]
    fn a_root_that_is_a_prefix_is_searched_in_any_drive_c_folder() {
        // ~/.wine is the prefix itself, and the game sits in a folder of
        // the player's own naming rather than a Program Files.
        let home = scratch_home("root");
        let wow = mk_install(&home.join(".wine/drive_c/Games/World of Warcraft"));
        assert_eq!(discover_wow_installs(&home), vec![wow]);
        std::fs::remove_dir_all(&home).unwrap();
    }

    #[cfg(not(target_os = "macos"))]
    #[test]
    fn the_prefix_walk_finds_a_flatpak_prefix_holding_the_game_at_drive_c() {
        let home = scratch_home("flatpak");
        let wow =
            mk_install(&home.join(
                ".var/app/org.example.Cellar/data/cellar/bottle-one/drive_c/World of Warcraft",
            ));
        assert_eq!(discover_wow_installs(&home), vec![wow]);
        std::fs::remove_dir_all(&home).unwrap();
    }

    #[cfg(not(target_os = "macos"))]
    #[test]
    fn an_install_reached_twice_is_reported_once() {
        // The Steam scan reaches this install through both Steam roots (one
        // a symlink to the other, as on a real machine), and the walk
        // reaches it again under ~/.local.
        let home = scratch_home("twice");
        let wow = mk_install(&home.join(
            ".local/share/Steam/steamapps/compatdata/4242/pfx/drive_c/Program Files (x86)/World of Warcraft",
        ));
        std::fs::create_dir_all(home.join(".steam")).unwrap();
        std::os::unix::fs::symlink(home.join(".local/share/Steam"), home.join(".steam/steam"))
            .unwrap();
        assert_eq!(wine_prefixes([home.join(".local")]).len(), 1);
        assert_eq!(discover_wow_installs(&home), vec![wow]);
        std::fs::remove_dir_all(&home).unwrap();
    }

    #[cfg(not(target_os = "macos"))]
    #[test]
    fn the_prefix_walk_needs_a_drive_c_and_follows_no_link_below_a_root() {
        let home = scratch_home("skips");
        // Installs outside any drive_c: loose under a root, in a folder
        // that only looks like a prefix's, and two folders down in one.
        mk_install(&home.join("Games/World of Warcraft"));
        mk_install(&home.join("Games/loose/Program Files/World of Warcraft"));
        mk_install(&home.join(".local/share/deep/drive_c/a/b/World of Warcraft"));
        // A real prefix outside every root, linked in below one.
        let elsewhere = home.join("elsewhere");
        let wow = mk_install(&elsewhere.join("pfx/drive_c/Program Files/World of Warcraft"));
        std::os::unix::fs::symlink(&elsewhere, home.join(".local/share/linked")).unwrap();
        assert!(discover_wow_installs(&home).is_empty());

        // A root may itself be a link: ~/.wine pointing at that prefix counts.
        std::os::unix::fs::symlink(elsewhere.join("pfx"), home.join(".wine")).unwrap();
        assert_eq!(discover_wow_installs(&home), vec![wow]);
        std::fs::remove_dir_all(&home).unwrap();
    }

    #[cfg(not(target_os = "macos"))]
    #[test]
    fn the_prefix_walk_stops_at_the_depth_cap() {
        let home = scratch_home("depth");
        // drive_c 7 levels below ~/Games: found; 8 levels: past the cap.
        let at_cap = mk_install(&home.join("Games/a/b/c/d/e/f/drive_c/World of Warcraft"));
        mk_install(&home.join("Games/z/b/c/d/e/f/g/drive_c/World of Warcraft"));
        assert_eq!(discover_wow_installs(&home), vec![at_cap]);
        std::fs::remove_dir_all(&home).unwrap();
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn install_discovery_checks_the_applications_roots() {
        // An install under a temp HOME's Applications folder is found; the
        // real /Applications is validated on the machine that has the game.
        let home = std::env::temp_dir().join(format!("wowdps-cli-mac-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&home);
        std::fs::create_dir_all(home.join("Applications/World of Warcraft/Data/data")).unwrap();
        std::fs::write(home.join("Applications/World of Warcraft/.build.info"), "x").unwrap();
        // Discovery canonicalizes what it finds, and the temp dir sits
        // behind a symlink on macOS (/var is /private/var), so both sides
        // are compared canonical.
        let want = home
            .join("Applications/World of Warcraft")
            .canonicalize()
            .unwrap();
        let found = discover_wow_installs(&home);
        // A machine with the game in /Applications finds that install too,
        // and it also ends in "Applications/World of Warcraft": only the temp
        // install itself proves ~/Applications was scanned.
        let mine = found
            .iter()
            .find(|p| p.canonicalize().ok().as_ref() == Some(&want))
            .unwrap_or_else(|| panic!("{} is not in {found:?}", want.display()));
        assert!(is_wow_install(mine));
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
    fn addon_takes_status_or_install_and_nothing_more() {
        assert_eq!(ok(&["addon"]), Cmd::Addon { install: false });
        assert_eq!(ok(&["addon", "status"]), Cmd::Addon { install: false });
        assert_eq!(ok(&["addon", "install"]), Cmd::Addon { install: true });
        assert!(parse(&["addon", "remove"]).is_err());
        assert!(parse(&["addon", "install", "--force"]).is_err());
    }

    #[test]
    fn old_mode_flags_point_at_their_subcommands() {
        for old in ["--daemon", "--gui", "--stop", "--status"] {
            let hint = format!("wowdps {}", old.trim_start_matches('-'));
            let err = parse(&[old]).unwrap_err();
            assert!(err.contains(&hint), "{old}: {err}");
            // The hint isn't position-sensitive: a retired flag after other
            // arguments still points at its subcommand.
            let err = parse(&["--file", "/tmp/a.txt", old]).unwrap_err();
            assert!(err.contains(&hint), "trailing {old}: {err}");
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

    /// `$WOWDPS_WOW_DIR` names the install directly; without it (or with a
    /// non-install), discovery under `$HOME` decides — and an empty home
    /// yields nothing rather than a made-up path.
    #[test]
    fn default_logs_dir_honors_the_override_then_discovery() {
        let root = std::env::temp_dir().join(format!("wowdps-cli-env-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let wow = root.join("wow");
        std::fs::create_dir_all(wow.join("Data").join("data")).unwrap();
        std::fs::write(wow.join(".build.info"), "x").unwrap();
        let home = root.join("home");
        std::fs::create_dir_all(&home).unwrap();

        // Env is process-global; this is the only test touching these two.
        let saved_home = std::env::var_os("HOME");
        unsafe {
            std::env::set_var("WOWDPS_WOW_DIR", &wow);
            std::env::set_var("HOME", &home);
        }
        assert_eq!(default_logs_dir(), Some(wow.join("_retail_").join("Logs")));

        // Not an install: fall through to discovery. On Linux it finds
        // nothing (the Steam roots and the prefix walk's roots all live
        // under the empty home); on
        // macOS it scans the Applications roots a real machine may fill,
        // so only Linux asserts the fall-through.
        unsafe { std::env::set_var("WOWDPS_WOW_DIR", &home) };
        #[cfg(not(target_os = "macos"))]
        assert_eq!(default_logs_dir(), None);
        unsafe { std::env::remove_var("WOWDPS_WOW_DIR") };
        #[cfg(not(target_os = "macos"))]
        assert_eq!(default_logs_dir(), None);
        match saved_home {
            Some(h) => unsafe { std::env::set_var("HOME", h) },
            None => unsafe { std::env::remove_var("HOME") },
        }
        let _ = std::fs::remove_dir_all(&root);
    }
}
