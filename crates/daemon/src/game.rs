//! Is the game running, and which edition of it? A 3-second sweep for a
//! case-insensitive substring of each process's program: its comm, or its
//! argv[0], the path it runs as, which under wine is the client's Windows
//! path (`Z:\...\_retail_\Wow.exe`) however Proton launches it. Never the
//! arguments after it: a shell, an editor or a script whose arguments merely
//! mention the pattern is not the game. macOS asks `ps` for every process's
//! argv[0]; every other system sweeps /proc, so one without it never sees
//! the game.
//!
//! Every edition's client is a `Wow.exe` (`Wow` on macOS), so the pattern
//! finds them all; the product folder in a matched process's argv[0]
//! (`_retail_`, `_forever_`: `wowdps_core::cli::PRODUCTS`) names which, by
//! model's `product_of`, which the overlay names a game window's process by.

use std::sync::mpsc::Sender;
use std::thread;
use std::time::Duration;

#[cfg(any(not(target_os = "macos"), test))]
use std::path::Path;

use wowdps_core::cli::PRODUCTS;
use wowdps_model::Edition;
/// Model's, where the overlay reads a game window's process by it too.
pub use wowdps_model::product_of;

use crate::hub::HubMsg;

pub const POLL: Duration = Duration::from_secs(3);

/// v49: what the watcher saw running: the editions the matched processes'
/// programs name by their product folder, and whether a matched process
/// named none (an install laid out some other way, a client the build does
/// not know). Nothing matched is the default.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Running {
    /// Each edition named, once, in [`PRODUCTS`] order.
    pub editions: Vec<Edition>,
    /// A matched process whose program names no known product folder.
    pub unnamed: bool,
}

impl Running {
    /// A game the watcher cannot name.
    pub fn unnamed() -> Self {
        Self {
            editions: Vec::new(),
            unnamed: true,
        }
    }

    /// These editions' clients.
    pub fn of(editions: &[Edition]) -> Self {
        let mut r = Self::default();
        for e in PRODUCTS {
            if editions.contains(&e) {
                r.editions.push(e);
            }
        }
        r
    }

    /// Any game at all: what the overlay is shown by, as it always was.
    pub fn any(&self) -> bool {
        self.unnamed || !self.editions.is_empty()
    }

    /// Whether the client of `edition` (as it is reckoned) runs, by name.
    pub fn names(&self, edition: Edition) -> bool {
        self.editions
            .iter()
            .any(|e| e.counts_as() == edition.counts_as())
    }

    /// File one matched process by its program (argv[0]): a product folder
    /// among its path components names the edition.
    fn note(&mut self, program: &[u8]) {
        match product_of(program) {
            Some(e) => {
                if !self.editions.contains(&e) {
                    self.editions.push(e);
                    self.editions
                        .sort_by_key(|x| PRODUCTS.iter().position(|p| p == x));
                }
            }
            None => self.unnamed = true,
        }
    }
}

#[cfg(not(target_os = "macos"))]
pub fn game_running(pattern: &str) -> Running {
    scan_proc(Path::new("/proc"), pattern)
}

/// macOS has no /proc to sweep: `ps` prints every process's argv[0] (its
/// `comm` there is argv[0] in full, the path the program runs as, not the
/// kernel's short name), one a line, which [`scan_ps`] reads.
#[cfg(target_os = "macos")]
pub fn game_running(pattern: &str) -> Running {
    if pattern.is_empty() {
        return Running::default();
    }
    let Ok(out) = std::process::Command::new("/bin/ps")
        .args(["-axww", "-o", "comm="])
        .stderr(std::process::Stdio::null())
        .output()
    else {
        return Running::default();
    };
    if !out.status.success() {
        return Running::default();
    }
    scan_ps(&String::from_utf8_lossy(&out.stdout), pattern)
}

/// Testable core of macOS's sweep: `ps -o comm=`'s output, one program a
/// line, the pattern matched against each whole.
#[cfg(any(target_os = "macos", test))]
pub fn scan_ps(out: &str, pattern: &str) -> Running {
    let mut running = Running::default();
    for program in out.lines() {
        let program = program.trim_start().as_bytes();
        if matches(pattern, b"", program) {
            running.note(program);
        }
    }
    running
}

/// Testable core: sweep `root` as if it were /proc, every process: two
/// editions' clients may run at once.
#[cfg(any(not(target_os = "macos"), test))]
pub fn scan_proc(root: &std::path::Path, pattern: &str) -> Running {
    let mut running = Running::default();
    let Ok(entries) = std::fs::read_dir(root) else {
        return running;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(name) = name.to_str() else { continue };
        if !name.bytes().all(|b| b.is_ascii_digit()) {
            continue;
        }
        let dir = entry.path();
        let comm = std::fs::read(dir.join("comm")).unwrap_or_default();
        let cmdline = std::fs::read(dir.join("cmdline")).unwrap_or_default();
        let program = program(&cmdline);
        if matches(pattern, &comm, program) {
            running.note(program);
        }
    }
    running
}

/// The program a NUL-separated command line runs: its argv[0], whole (wine
/// writes the client's Windows path there, spaces and all), never the
/// arguments after it.
fn program(cmdline: &[u8]) -> &[u8] {
    cmdline.split(|&b| b == 0).next().unwrap_or_default()
}

/// Whether a process is the game: the pattern, case-insensitively, a
/// substring of its comm (`Wow.exe` under wine, which names the process
/// after the client) or of its whole `program` (argv[0]), so a file name
/// (`wow.exe`), a folder (`world of warcraft`) and a path fragment
/// (`_retail_\wow.exe`) all match the client. Nothing else on its command
/// line counts.
pub fn matches(pattern: &str, comm: &[u8], program: &[u8]) -> bool {
    if pattern.is_empty() {
        return false;
    }
    let pat = pattern.to_ascii_lowercase();
    let hay = |bytes: &[u8]| String::from_utf8_lossy(bytes).to_ascii_lowercase();
    hay(comm).contains(&pat) || hay(program).contains(&pat)
}

/// Watch for transitions — a game appearing or going, and (v49) one
/// edition's client giving way to another's even when no sweep saw none
/// running; the initial state is sent too, so the hub never has to guess.
pub fn spawn_watcher(pattern: String, hub: Sender<HubMsg>, poll: Duration) {
    thread::spawn(move || {
        let mut last: Option<Running> = None;
        loop {
            let now = game_running(&pattern);
            if last.as_ref() != Some(&now) {
                last = Some(now.clone());
                if hub.send(HubMsg::Game(now)).is_err() {
                    return;
                }
            }
            thread::sleep(poll);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matching_is_case_insensitive_and_covers_wine_paths() {
        let cmdline = b"Z:\\Games\\World of Warcraft\\_retail_\\Wow.exe\0-launch\0";
        assert_eq!(
            program(cmdline),
            b"Z:\\Games\\World of Warcraft\\_retail_\\Wow.exe"
        );
        assert!(matches("wow.exe", b"wine64-preloader", program(cmdline)));
        assert!(matches("WOW.EXE", b"", program(cmdline)));
        assert!(matches("wow.exe", b"Wow.exe\n", b""), "comm alone");
        assert!(!matches("wow.exe", b"wineserver", b"/usr/bin/wineserver"));
        assert!(!matches("", b"anything", b"anything"));
        assert_eq!(program(b""), b"", "an unread command line");
    }

    /// A process is the game by the program it is, never by what its
    /// arguments say: the file name, a folder and a path fragment of the
    /// client's argv[0] all find it, and the same words after argv[0] find
    /// nothing.
    #[test]
    fn only_the_program_matches_never_its_arguments() {
        let client = b"Z:\\games\\World of Warcraft\\_forever_\\Wow.exe";
        for pattern in ["wow.exe", "world of warcraft", "_forever_\\wow.exe"] {
            assert!(matches(pattern, b"", client), "{pattern}");
            assert!(
                !matches(pattern, b"zsh", b"/run/current-system/sw/bin/zsh"),
                "{pattern}"
            );
        }
        // A shell running a search for the client, the search itself, an
        // editor on a file named after it: none is the game.
        for cmdline in [
            &b"/run/current-system/sw/bin/zsh\0-c\0pgrep -ai wow.exe\0"[..],
            b"pgrep\0-ai\0wow.exe\0",
            b"nvim\0notes-wow.exe.txt\0",
            b"/usr/bin/python3\0tools/check.py\0Z:\\games\\World of Warcraft\\_retail_\\WoW.exe\0",
        ] {
            assert!(!matches("wow.exe", b"", program(cmdline)));
            assert!(!matches("world of warcraft", b"", program(cmdline)));
        }
    }

    /// Forever's client as it runs under wine, its argv[0] the install's
    /// `_forever_\Wow.exe`: the default pattern finds it, as a `World of
    /// Warcraft` pattern does, and the watcher names it Forever — no
    /// pattern of its own is needed.
    #[test]
    fn forevers_client_is_found_and_named_by_its_product_folder() {
        let cmdline = b"Z:\\games\\World of Warcraft\\_forever_\\Wow.exe\0";
        #[cfg(not(target_os = "macos"))]
        assert!(matches(
            crate::config::default_game_process(),
            b"Wow.exe",
            program(cmdline)
        ));
        assert!(matches("world of warcraft", b"Wow.exe", program(cmdline)));
        assert_eq!(product_of(program(cmdline)), Some(Edition::Forever));
        let root = fake_proc(
            "forever",
            &[
                ("1", "systemd", "/sbin/init\0"),
                (
                    "700",
                    "Wow.exe",
                    "Z:\\games\\World of Warcraft\\_forever_\\Wow.exe\0",
                ),
            ],
        );
        for pattern in ["wow.exe", "world of warcraft"] {
            assert_eq!(
                scan_proc(&root, pattern),
                Running::of(&[Edition::Forever]),
                "{pattern}"
            );
        }
        let _ = std::fs::remove_dir_all(&root);
        // Retail's beside it, spelled `WoW.exe`: the same pattern finds
        // both, each named by its own product folder.
        let both = fake_proc(
            "both",
            &[
                (
                    "700",
                    "Wow.exe",
                    "Z:\\games\\World of Warcraft\\_forever_\\Wow.exe\0",
                ),
                (
                    "800",
                    "WoW.exe",
                    "Z:\\games\\World of Warcraft\\_retail_\\WoW.exe\0",
                ),
            ],
        );
        for pattern in ["wow.exe", "world of warcraft"] {
            assert_eq!(
                scan_proc(&both, pattern),
                Running::of(&[Edition::Retail, Edition::Forever]),
                "{pattern}"
            );
        }
        let _ = std::fs::remove_dir_all(&both);
    }

    fn fake_proc(tag: &str, procs: &[(&str, &str, &str)]) -> std::path::PathBuf {
        let root = std::env::temp_dir().join(format!("wowdps-proc-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        for (pid, comm, cmdline) in procs {
            let d = root.join(pid);
            std::fs::create_dir_all(&d).unwrap();
            std::fs::write(d.join("comm"), comm).unwrap();
            std::fs::write(d.join("cmdline"), cmdline).unwrap();
        }
        // Non-numeric entries are skipped, not read.
        std::fs::create_dir_all(root.join("self")).unwrap();
        root
    }

    #[test]
    fn scan_finds_the_game_in_a_fake_proc() {
        let root = fake_proc(
            "one",
            &[
                ("1", "systemd", "/sbin/init\0"),
                ("4321", "wine64-preloader", "Z:\\wow\\Wow.exe\0"),
            ],
        );
        assert_eq!(scan_proc(&root, "wow.exe"), Running::unnamed());
        assert!(scan_proc(&root, "wow.exe").any());
        assert_eq!(scan_proc(&root, "diablo.exe"), Running::default());
        assert!(!scan_proc(&root, "diablo.exe").any());
        let _ = std::fs::remove_dir_all(&root);
    }

    /// v49: two editions' clients at once are both named, in the products'
    /// order, each once however many of its processes match.
    #[test]
    fn scan_names_every_running_edition() {
        let root = fake_proc(
            "two",
            &[
                ("1", "systemd", "/sbin/init\0"),
                (
                    "500",
                    "Wow.exe",
                    "Z:\\Games\\World of Warcraft\\_forever_\\Wow.exe\0",
                ),
                (
                    "600",
                    "wine64-preloader",
                    "Z:\\Games\\World of Warcraft\\_retail_\\Wow.exe\0-launch\0",
                ),
                (
                    "601",
                    "Wow.exe",
                    "Z:\\Games\\World of Warcraft\\_retail_\\Wow.exe\0",
                ),
            ],
        );
        let r = scan_proc(&root, "wow.exe");
        assert_eq!(r, Running::of(&[Edition::Retail, Edition::Forever]));
        assert_eq!(r.editions, [Edition::Retail, Edition::Forever]);
        assert!(!r.unnamed);
        assert!(r.names(Edition::Forever) && r.names(Edition::Retail));
        assert!(
            r.names(Edition::Unknown(0)),
            "a log naming none counts as Retail"
        );
        assert!(!r.names(Edition::Classic(2)));
        let _ = std::fs::remove_dir_all(&root);
    }

    /// Processes that only mention the client: a shell running a search for
    /// it, the search itself, an editor on a file named after it, and a
    /// wrapper whose argument is the client's path (Proton's script, which
    /// starts the client as a process of its own). Before, each was a game,
    /// and the ones naming no product folder an unnamed one, which counts
    /// for every source.
    const NOT_THE_GAME: &[(&str, &str, &str)] = &[
        ("1", "systemd", "/sbin/init\0"),
        (
            "900",
            "zsh",
            "/run/current-system/sw/bin/zsh\0-c\0pgrep -ai wow.exe\0",
        ),
        ("901", "pgrep", "pgrep\0-ai\0wow.exe\0"),
        ("902", "nvim", "nvim\0notes-wow.exe.txt\0"),
        (
            "903",
            "python3",
            "/usr/bin/python3\0/opt/proton/proton\0waitforexitandrun\0\
             Z:\\games\\World of Warcraft\\_retail_\\WoW.exe\0",
        ),
        (
            "904",
            "python3",
            "/usr/bin/python3\0/opt/proton/proton\0run\0/games/wow/Wow.exe\0",
        ),
    ];

    /// The clients as wine runs them: comm the client's name, argv[0] its
    /// Windows path, then its arguments and the NULs wine pads the old
    /// argument space with.
    const CLIENTS: &[(&str, &str, &str)] = &[
        (
            "700",
            "Wow.exe",
            "Z:\\games\\World of Warcraft\\_forever_\\Wow.exe\0-launcherlogin\0-uid\0wowf\0\0\0\0\0",
        ),
        (
            "800",
            "WoW.exe",
            "Z:\\games\\World of Warcraft\\_retail_\\WoW.exe\0-launcherlogin\0-uid\0wow\0\0\0\0\0",
        ),
    ];

    #[test]
    fn a_process_that_only_mentions_the_client_is_not_the_game() {
        let root = fake_proc("mentions", NOT_THE_GAME);
        for pattern in ["wow.exe", "world of warcraft", "_retail_\\wow.exe"] {
            assert_eq!(scan_proc(&root, pattern), Running::default(), "{pattern}");
        }
        let _ = std::fs::remove_dir_all(&root);

        // Beside the clients the same processes change nothing: each client
        // is named once by its own argv[0], the wrapper adds neither a
        // second Retail nor an unnamed game.
        let procs: Vec<_> = NOT_THE_GAME.iter().chain(CLIENTS).copied().collect();
        let root = fake_proc("mentions-beside", &procs);
        for pattern in ["wow.exe", "world of warcraft"] {
            let r = scan_proc(&root, pattern);
            assert_eq!(
                r,
                Running::of(&[Edition::Retail, Edition::Forever]),
                "{pattern}"
            );
            assert!(!r.unnamed, "{pattern}");
        }
        assert_eq!(
            scan_proc(&root, "_forever_\\wow.exe"),
            Running::of(&[Edition::Forever]),
            "a path fragment finds the client whose argv[0] holds it"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    /// The edition is read from argv[0] alone: an argument naming another
    /// product's folder does not name the client.
    #[test]
    fn the_edition_is_named_by_argv0_alone() {
        let root = fake_proc(
            "argv0",
            &[(
                "700",
                "Wow.exe",
                "Z:\\wow\\Wow.exe\0-config\0Z:\\games\\World of Warcraft\\_retail_\\WTF\\Config.wtf\0",
            )],
        );
        assert_eq!(scan_proc(&root, "wow.exe"), Running::unnamed());
        let _ = std::fs::remove_dir_all(&root);
    }

    /// macOS's sweep reads `ps`'s argv[0] a line: the game's bundle path
    /// names it, and a line is whole (its spaces are the path's).
    #[test]
    fn the_ps_sweep_reads_each_program_whole() {
        let out = "/sbin/launchd\n\
                   /Applications/World of Warcraft/_retail_/World of Warcraft.app/Contents/MacOS/World of Warcraft\n\
                   /Volumes/T7/World of Warcraft/_forever_/World of Warcraft.app/Contents/MacOS/World of Warcraft\n\
                   -zsh\n\
                   grep\n\
                   /usr/bin/vim\n";
        let r = scan_ps(out, "World of Warcraft.app");
        assert_eq!(r, Running::of(&[Edition::Retail, Edition::Forever]));
        assert!(!r.unnamed);
        assert_eq!(
            scan_ps("/sbin/launchd\n-zsh\ngrep\n", "World of Warcraft.app"),
            Running::default(),
            "a search for the bundle is not the game"
        );
        assert_eq!(
            scan_ps(
                "  /Games/World of Warcraft.app/Contents/MacOS/World of Warcraft\n",
                "world of warcraft.app"
            ),
            Running::unnamed(),
            "case-insensitive; a bundle outside a product folder is unnamed"
        );
        assert_eq!(scan_ps(out, ""), Running::default());
    }

    /// The watcher reports the initial verdict at once and only transitions
    /// after that; hanging up the hub ends it.
    #[test]
    fn the_watcher_reports_the_initial_state_then_only_changes() {
        let (tx, rx) = std::sync::mpsc::channel();
        // No process on this machine carries this name.
        let pattern = "wowdps-no-such-process-7f3a9c".to_string();
        assert!(!game_running(&pattern).any());
        spawn_watcher(pattern, tx, Duration::from_millis(10));
        match rx.recv_timeout(Duration::from_secs(5)) {
            Ok(HubMsg::Game(running)) => assert_eq!(running, Running::default()),
            Ok(_) => panic!("the watcher only sends Game"),
            Err(e) => panic!("no initial verdict: {e}"),
        }
        assert!(
            rx.recv_timeout(Duration::from_millis(100)).is_err(),
            "an unchanged verdict is not resent"
        );
        drop(rx);
    }

    #[test]
    fn a_missing_proc_root_means_no_game() {
        assert!(!scan_proc(Path::new("/nonexistent/wowdps-proc"), "wow.exe").any());
    }
}
