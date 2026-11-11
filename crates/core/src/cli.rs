//! Shared command-line handling: the `wowdps` dispatcher's subcommands.
//! Hand-rolled so the binaries carry no argument-parsing dependency. Also
//! hosts the machine-agnostic discovery of the game install, used when the
//! config carries no `logs_dir`.

use std::path::{Path, PathBuf};

use wowdps_model::Edition;

pub use crate::tail::SourceSpec;

/// The editions whose product folder an install is known to keep them in
/// (`Edition::product_dir`): what the game watcher can name a running
/// client by, and what an install may hold beside Retail. Model's, where
/// `wowdps_model::product_of` reads them for the daemon and the overlay.
pub use wowdps_model::PRODUCTS;

/// The editions the daemon follows in an install: a `logs_dir` inside an
/// install whose product folder is one of these names the install, and
/// each other one's `Logs` folder is followed beside it. Retail alone for
/// now: Forever's logs are read by `wowdps history import` and by tests
/// that name its folder, never followed on their own. Following Forever
/// is adding it here.
pub const FOLLOWED: &[Edition] = &[Edition::Retail];

/// A WoW install root: the folder holding `.build.info` and `Data/data`.
pub fn is_wow_install(p: &Path) -> bool {
    p.join(".build.info").is_file() && p.join("Data").join("data").is_dir()
}

/// The install a logs folder sits in and the product folder's name:
/// `<install>/<product>/Logs`, the install validated. `None` for a folder
/// anywhere else (a copied folder, a test's).
fn install_of(logs_dir: &Path) -> Option<(PathBuf, String)> {
    if logs_dir.file_name()?.to_str()? != "Logs" {
        return None;
    }
    let product = logs_dir.parent()?;
    let root = product.parent()?;
    let name = product.file_name()?.to_str()?.to_string();
    is_wow_install(root).then(|| (root.to_path_buf(), name))
}

/// The edition a logs folder's product folder names: `Retail` for
/// `<install>/_retail_/Logs`, `Forever` for `_forever_`; `None` for a
/// folder outside an install or in a product folder not in [`PRODUCTS`]
/// (a test realm, a beta). The folder's word, not the log's: the header
/// decides a log's edition once read.
pub fn product_edition(logs_dir: &Path) -> Option<Edition> {
    let (_, product) = install_of(logs_dir)?;
    PRODUCTS
        .into_iter()
        .find(|e| e.product_dir() == Some(product.as_str()))
}

/// Each of `editions`' logs folder in `install`, one per edition with a
/// known product folder, in the order given — whether or not it exists
/// yet: a folder is followed from when it first holds a log.
pub fn install_logs(install: &Path, editions: &[Edition]) -> Vec<(Edition, PathBuf)> {
    editions
        .iter()
        .filter_map(|e| Some((*e, install.join(e.product_dir()?).join("Logs"))))
        .collect()
}

/// The logs folders followed beside `logs_dir`: when it is a followed
/// product's folder inside an install, every other edition of `followed`
/// in that install ([`install_logs`]); else none — a folder outside an
/// install, or one of a product not followed, is followed alone, as it
/// always was.
pub fn sibling_logs(logs_dir: &Path, followed: &[Edition]) -> Vec<(Edition, PathBuf)> {
    let Some((install, _)) = install_of(logs_dir) else {
        return Vec::new();
    };
    let Some(own) = product_edition(logs_dir) else {
        return Vec::new();
    };
    if !followed.contains(&own) {
        return Vec::new();
    }
    let others: Vec<Edition> = followed.iter().copied().filter(|e| *e != own).collect();
    install_logs(&install, &others)
}

/// The retail logs directory to tail when no `logs_dir` is configured:
/// `$WOWDPS_WOW_DIR` when it names an install, else the most recently
/// updated install found under the conventional install roots (Battle.net's
/// on macOS, Steam's Proton prefixes elsewhere). `None` when
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

/// WoW installs under `home`'s conventional roots, deduplicated (the
/// roots are often symlinks to one another): the Battle.net folders on
/// macOS (`/Applications`, `~/Applications`), the Steam roots' Proton
/// prefixes elsewhere.
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
        /// `--edition <name>`: the one edition the TUI shows, whichever
        /// runs; `None` follows the live edition.
        edition: Option<Edition>,
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
        return Ok(Some(Cmd::Tui {
            source: None,
            edition: None,
        }));
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
            let mut edition = None;
            let mut extra = extra.into_iter();
            while let Some(arg) = extra.next() {
                match arg.as_str() {
                    // Last wins, as the source flags do.
                    "--edition" => {
                        let name = extra.next().ok_or_else(|| {
                            format!("--edition needs a name: {}", edition_names())
                        })?;
                        edition = Some(edition_arg(&name)?);
                    }
                    other => return Err(unknown(other)),
                }
            }
            Cmd::Tui { source, edition }
        }
    };
    Ok(Some(cmd))
}

/// `--edition`'s name: an edition an install keeps a product folder of
/// ([`PRODUCTS`]), by its [`Edition::key`] in any case. Anything else is
/// refused with the names it takes.
fn edition_arg(name: &str) -> Result<Edition, String> {
    let want = name.trim().to_lowercase();
    PRODUCTS
        .into_iter()
        .find(|e| e.key() == want)
        .ok_or_else(|| format!("unknown edition {name:?}: {}", edition_names()))
}

/// `--edition takes retail or forever`, from [`PRODUCTS`].
fn edition_names() -> String {
    let keys: Vec<&str> = PRODUCTS.iter().map(|e| e.key()).collect();
    match keys.split_last() {
        Some((last, [])) => format!("--edition takes {last}"),
        Some((last, rest)) => format!("--edition takes {} or {last}", rest.join(", ")),
        None => "--edition takes no name".to_string(),
    }
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
        assert_eq!(
            ok(&[]),
            Cmd::Tui {
                source: None,
                edition: None
            }
        );
    }

    #[test]
    fn file_and_logs_select_the_tui_sources() {
        assert_eq!(
            ok(&["--file", "/tmp/a.txt"]),
            Cmd::Tui {
                source: Some(SourceSpec::File(PathBuf::from("/tmp/a.txt"))),
                edition: None
            }
        );
        assert_eq!(
            ok(&["--logs", "/tmp/logs"]),
            Cmd::Tui {
                source: Some(SourceSpec::Dir(PathBuf::from("/tmp/logs"))),
                edition: None
            }
        );
        assert_eq!(
            ok(&["-f", "/tmp/a.txt"]),
            Cmd::Tui {
                source: Some(SourceSpec::File(PathBuf::from("/tmp/a.txt"))),
                edition: None
            }
        );
    }

    /// v49: `--edition` pins the TUI to one edition by name, in any case,
    /// beside a source or alone; without it the TUI follows.
    #[test]
    fn edition_pins_the_tui_to_one_edition() {
        assert_eq!(
            ok(&["--edition", "forever"]),
            Cmd::Tui {
                source: None,
                edition: Some(Edition::Forever)
            }
        );
        assert_eq!(
            ok(&["--logs", "/tmp/logs", "--edition", "Retail"]),
            Cmd::Tui {
                source: Some(SourceSpec::Dir(PathBuf::from("/tmp/logs"))),
                edition: Some(Edition::Retail)
            }
        );
        // The last one wins, as the source flags do.
        assert_eq!(
            ok(&["--edition", "retail", "--edition", "forever"]),
            Cmd::Tui {
                source: None,
                edition: Some(Edition::Forever)
            }
        );
    }

    /// A name `--edition` does not take is refused with the names it does;
    /// so is a missing one. It is the TUI's alone.
    #[test]
    fn a_bad_edition_is_refused_with_the_names_it_takes() {
        for bad in ["classic", "tbc", "1", ""] {
            let err = parse(&["--edition", bad]).unwrap_err();
            assert_eq!(
                err,
                format!("unknown edition {bad:?}: --edition takes retail or forever"),
            );
        }
        assert_eq!(
            parse(&["--edition"]).unwrap_err(),
            "--edition needs a name: --edition takes retail or forever"
        );
        assert!(parse(&["daemon", "--edition", "forever"]).is_err());
        assert!(parse(&["gui", "--edition", "forever"]).is_err());
    }

    #[test]
    fn paths_with_spaces_survive_intact() {
        let path = "/home/user/Program Files (x86)/World of Warcraft/_retail_/Logs";
        assert_eq!(
            ok(&["--logs", path]),
            Cmd::Tui {
                source: Some(SourceSpec::Dir(PathBuf::from(path))),
                edition: None
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

    /// A logs folder inside an install names its product's edition and the
    /// followed editions' folders beside it; one outside an install, or of
    /// a product not followed, is followed alone. Discovery follows Retail
    /// alone today: an install's Forever folder is never a sibling.
    #[test]
    fn a_logs_folder_names_its_install_and_the_followed_products_beside_it() {
        let root = std::env::temp_dir().join(format!("wowdps-cli-sib-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let wow = root.join("World of Warcraft");
        std::fs::create_dir_all(wow.join("Data").join("data")).unwrap();
        std::fs::write(wow.join(".build.info"), "x").unwrap();
        let retail = wow.join("_retail_").join("Logs");
        let forever = wow.join("_forever_").join("Logs");
        let beta = wow.join("_classic_beta_").join("Logs");

        assert_eq!(product_edition(&retail), Some(Edition::Retail));
        assert_eq!(product_edition(&forever), Some(Edition::Forever));
        assert_eq!(product_edition(&beta), None, "a product not known");
        assert_eq!(
            product_edition(&wow.join("_retail_")),
            None,
            "not a Logs folder"
        );
        assert_eq!(product_edition(&root.join("_retail_").join("Logs")), None);

        // Today: Retail alone is followed, so no folder has a sibling.
        assert_eq!(FOLLOWED, &[Edition::Retail]);
        assert!(sibling_logs(&retail, FOLLOWED).is_empty());
        assert!(
            sibling_logs(&forever, FOLLOWED).is_empty(),
            "Forever's folder alone"
        );
        // Both followed: each names the other, whether it exists yet or not.
        let both = [Edition::Retail, Edition::Forever];
        assert_eq!(
            sibling_logs(&retail, &both),
            vec![(Edition::Forever, forever.clone())]
        );
        assert_eq!(
            sibling_logs(&forever, &both),
            vec![(Edition::Retail, retail.clone())]
        );
        assert!(
            sibling_logs(&beta, &both).is_empty(),
            "a product not followed"
        );
        assert!(sibling_logs(Path::new("/tmp/Logs"), &both).is_empty());
        assert_eq!(
            install_logs(
                &wow,
                &[Edition::Forever, Edition::Unknown(7), Edition::Retail]
            ),
            vec![(Edition::Forever, forever), (Edition::Retail, retail)],
            "an edition with no known folder has none"
        );
        let _ = std::fs::remove_dir_all(&root);
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
                source: Some(SourceSpec::File(PathBuf::from("b"))),
                edition: None
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
        // nothing (the compatdata roots live under the empty home); on
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
