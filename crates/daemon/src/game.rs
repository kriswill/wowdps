//! Is the game running? A 3-second /proc sweep for a case-insensitive
//! substring of comm or cmdline — which is what finds wine's
//! `Z:\...\Wow.exe` however Proton launches it. Linux-only, deliberately.

use std::path::Path;
use std::sync::mpsc::Sender;
use std::thread;
use std::time::Duration;

use crate::hub::HubMsg;

pub const POLL: Duration = Duration::from_secs(3);

pub fn game_running(pattern: &str) -> bool {
    scan_proc(Path::new("/proc"), pattern)
}

/// Testable core: sweep `root` as if it were /proc.
pub fn scan_proc(root: &Path, pattern: &str) -> bool {
    let Ok(entries) = std::fs::read_dir(root) else {
        return false;
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
            return true;
        }
    }
    false
}

/// Case-insensitive substring over comm and the NUL-separated cmdline.
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

/// Watch for transitions; the initial state is sent too, so the hub never
/// has to guess.
pub fn spawn_watcher(pattern: String, hub: Sender<HubMsg>, poll: Duration) {
    thread::spawn(move || {
        let mut last: Option<bool> = None;
        loop {
            let now = game_running(&pattern);
            if last != Some(now) {
                last = Some(now);
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

    #[test]
    fn scan_finds_the_game_in_a_fake_proc() {
        let root = std::env::temp_dir().join(format!("wowdps-proc-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        for (pid, comm, cmdline) in [
            ("1", "systemd", "/sbin/init\0"),
            ("4321", "wine64-preloader", "Z:\\wow\\Wow.exe\0"),
        ] {
            let d = root.join(pid);
            std::fs::create_dir_all(&d).unwrap();
            std::fs::write(d.join("comm"), comm).unwrap();
            std::fs::write(d.join("cmdline"), cmdline).unwrap();
        }
        // Non-numeric entries are skipped, not read.
        std::fs::create_dir_all(root.join("self")).unwrap();

        assert!(scan_proc(&root, "wow.exe"));
        assert!(!scan_proc(&root, "diablo.exe"));
        let _ = std::fs::remove_dir_all(&root);
    }

    /// The watcher reports the initial verdict at once and only transitions
    /// after that; hanging up the hub ends it.
    #[test]
    fn the_watcher_reports_the_initial_state_then_only_changes() {
        let (tx, rx) = std::sync::mpsc::channel();
        // No process on this machine carries this name.
        let pattern = "wowdps-no-such-process-7f3a9c".to_string();
        assert!(!game_running(&pattern));
        spawn_watcher(pattern, tx, Duration::from_millis(10));
        match rx.recv_timeout(Duration::from_secs(5)) {
            Ok(HubMsg::Game(running)) => assert!(!running),
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
        assert!(!scan_proc(Path::new("/nonexistent/wowdps-proc"), "wow.exe"));
    }
}
