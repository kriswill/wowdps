//! Is the game running, and which edition of it? A 3-second sweep for a
//! case-insensitive substring of comm or cmdline — which is what finds
//! wine's `Z:\...\Wow.exe` however Proton launches it. macOS asks `pgrep`;
//! every other system sweeps /proc, so one without it never sees the game.
//!
//! Every edition's client is a `Wow.exe` (`Wow` on macOS), so the pattern
//! finds them all; the product folder in a matched process's command line
//! (`_retail_`, `_forever_`: `wowdps_core::cli::PRODUCTS`) names which.

use std::sync::mpsc::Sender;
use std::thread;
use std::time::Duration;

#[cfg(any(not(target_os = "macos"), test))]
use std::path::Path;

use wowdps_core::cli::PRODUCTS;
use wowdps_model::Edition;

use crate::hub::HubMsg;

pub const POLL: Duration = Duration::from_secs(3);

/// v49: what the watcher saw running: the editions the matched processes'
/// command lines name by their product folder, and whether a matched
/// process named none (an install laid out some other way, a client the
/// build does not know). Nothing matched is the default.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Running {
    /// Each edition named, once, in [`PRODUCTS`] order.
    pub editions: Vec<Edition>,
    /// A matched process whose command line names no known product folder.
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

    /// File one matched process by its command line.
    fn note(&mut self, cmdline: &[u8]) {
        match product_of(cmdline) {
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

/// The edition a command line names by a product folder among its path
/// components (`…\_retail_\Wow.exe`, `/…/_forever_/…`), case-insensitively;
/// `None` when it names none of [`PRODUCTS`].
pub fn product_of(cmdline: &[u8]) -> Option<Edition> {
    let hay = String::from_utf8_lossy(cmdline).to_ascii_lowercase();
    let parts: Vec<&str> = hay.split(['\\', '/', '\0']).collect();
    PRODUCTS.into_iter().find(|e| {
        e.product_dir()
            .is_some_and(|dir| parts.iter().any(|p| p.trim() == dir))
    })
}

#[cfg(not(target_os = "macos"))]
pub fn game_running(pattern: &str) -> Running {
    scan_proc(Path::new("/proc"), pattern)
}

/// macOS has no /proc to sweep: `pgrep -fil` is the same case-insensitive
/// substring against every process's full argument list, and prints each
/// match's pid and argument list. pgrep reads its pattern as an ERE, so
/// metacharacters match a little more than a literal substring would; the
/// default `World of Warcraft.app` only loosens the dot.
#[cfg(target_os = "macos")]
pub fn game_running(pattern: &str) -> Running {
    if pattern.is_empty() {
        return Running::default();
    }
    let Ok(out) = std::process::Command::new("/usr/bin/pgrep")
        .args(["-fil", "--", pattern])
        .stderr(std::process::Stdio::null())
        .output()
    else {
        return Running::default();
    };
    if !out.status.success() {
        return Running::default();
    }
    let mut running = Running::default();
    for line in String::from_utf8_lossy(&out.stdout).lines() {
        running.note(line.as_bytes());
    }
    if !running.any() {
        running.unnamed = true;
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
        if matches(pattern, &comm, &cmdline) {
            running.note(&cmdline);
        }
    }
    running
}

/// Case-insensitive substring over comm and the NUL-separated cmdline.
#[cfg(any(not(target_os = "macos"), test))]
pub fn matches(pattern: &str, comm: &[u8], cmdline: &[u8]) -> bool {
    if pattern.is_empty() {
        return false;
    }
    let pat = pattern.to_ascii_lowercase();
    let hay = |bytes: &[u8]| {
        String::from_utf8_lossy(bytes)
            .to_ascii_lowercase()
            .replace('\0', " ")
    };
    hay(comm).contains(&pat) || hay(cmdline).contains(&pat)
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
        assert!(matches("wow.exe", b"wine64-preloader", cmdline));
        assert!(matches("WOW.EXE", b"", cmdline));
        assert!(!matches("wow.exe", b"wineserver", b"/usr/bin/wineserver\0"));
        assert!(!matches("", b"anything", b"anything"));
    }

    /// v49: the product folder among the command line's path components
    /// names the edition — wine's and macOS's spellings, any case — and a
    /// folder that merely contains the word, or none at all, names nothing.
    #[test]
    fn the_product_folder_names_the_edition() {
        assert_eq!(
            product_of(b"Z:\\Games\\World of Warcraft\\_retail_\\Wow.exe\0-launch\0"),
            Some(Edition::Retail)
        );
        assert_eq!(
            product_of(b"C:\\Program Files (x86)\\World of Warcraft\\_FOREVER_\\Wow.exe\0"),
            Some(Edition::Forever)
        );
        assert_eq!(
            product_of(
                b"/Applications/World of Warcraft/_retail_/World of Warcraft.app/Contents/MacOS/World of Warcraft"
            ),
            Some(Edition::Retail)
        );
        assert_eq!(product_of(b"Z:\\wow\\Wow.exe\0"), None);
        assert_eq!(product_of(b"Z:\\wow\\_retail_old\\Wow.exe\0"), None);
        assert_eq!(product_of(b"Z:\\wow\\_classic_beta_\\Wow.exe\0"), None);
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
