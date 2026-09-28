//! User configuration: `~/.config/wowdps/config.toml` (XDG-aware).
//!
//! Written back whenever the user changes something durable — dragging the
//! overlay tab, zooming — so the next launch picks up where they left off.
//! A missing or unparsable file falls back to defaults; saving is best-effort
//! (a read-only config dir must never crash a damage meter mid-raid).

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// Which screen edge the overlay tab pins to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Edge {
    Left,
    Right,
    Top,
    Bottom,
}

impl Edge {
    /// Side edges position along Y; top/bottom edges along X.
    pub fn is_vertical(self) -> bool {
        matches!(self, Edge::Left | Edge::Right)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    /// Screen edge the overlay tab pins to.
    pub edge: Edge,
    /// Pixels along that edge (from the top for left/right, from the left
    /// for top/bottom). Updated by dragging the tab.
    pub offset: i32,
    /// Expanded overlay panel size.
    pub width: u32,
    pub height: u32,
    /// Whole-UI scale, shared by the window and the overlay.
    pub zoom: f32,
    /// Wayland output to pin the overlay to (e.g. "DP-3"). `None` uses the
    /// output that is active when the overlay starts.
    pub monitor: Option<String>,
    /// Hyprland only: hide the overlay whenever the game's workspace is not
    /// on any screen. No effect under other compositors.
    pub follow_game: bool,
    /// Case-insensitive substring identifying the game window, matched
    /// against its Hyprland class and title.
    pub game_match: String,
    /// Overlay: also show the instance's Σ overall under the current fight's
    /// rows (the footer Σ toggle; remembered across launches).
    pub overlay_split: bool,
    /// Window background opacity, 0..=1 — the same translucent look the
    /// overlay panel has. 1.0 is fully opaque.
    pub window_alpha: f32,
    /// Number meter rows by their sort position (window and overlay).
    /// Toggled from the window's ⚙ options panel.
    pub show_ranks: bool,
    /// Strip "-Realm" from player names on the meter: a home-realm raid reads
    /// cleaner without twenty copies of the same suffix.
    #[serde(default)]
    pub hide_realms: bool,
    /// What Home calls the window it scopes itself to. Free text: the store
    /// knows nothing about seasons, so this is the user's own label.
    pub season_label: String,
    /// `YYYY-MM-DD`, UTC. `None` = no lower bound, i.e. the whole store.
    pub season_start: Option<String>,
    /// `YYYY-MM-DD`, UTC, exclusive. `None` = open-ended.
    pub season_end: Option<String>,
    /// The character the window is LOCKED to (a player guid), picked with
    /// the character picker and remembered across launches. Home's stats
    /// and links are about this character alone, and their row wears the
    /// "you"; `None` = the owner the newest stored card names.
    #[serde(default)]
    pub character: Option<String>,
    /// The locked character's class by its in-game name ("Death Knight"),
    /// written whenever the window learns it — so a `chrome = "class"`
    /// window wears the right colour on its first frame, before Home or the
    /// meter has named anyone. `None` until learned.
    #[serde(default)]
    pub character_class: Option<String>,
    /// `gold` (the default) or `class`: what the window's chrome is drawn
    /// in. A plain string for the reason `density` is one.
    pub chrome: String,
    /// `comfortable` / `compact`. A plain string, not an enum: a typo in a
    /// hand-edited file must fall back to the default, not make the whole
    /// config unparsable and block every save after it.
    pub density: String,
    /// Open Home at launch when nothing is live.
    pub home_on_start: bool,
    /// Every key this struct does not own — the daemon's `logs_dir`,
    /// `game_process`, `auto_overlay`, `history_*` and anything a future
    /// version adds — round-trips through a save untouched. Without this a
    /// GUI save rewrote the whole file and erased them.
    #[serde(flatten)]
    pub extra: toml::Table,
    /// The file existed but did not parse, so `extra` is EMPTY rather than
    /// the keys it holds: a save would erase them. The daemon's tolerant
    /// subset reader may still accept such a file (a duplicated key, an
    /// unquoted string), so `save` refuses until the file is repaired.
    #[serde(skip)]
    pub load_failed: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            edge: Edge::Right,
            offset: 300,
            // Wide enough that "Spell (Pet Name)" drill labels keep clear of
            // the hits/crit/total columns at the default zoom.
            width: 410,
            height: 460,
            zoom: 1.25,
            monitor: None,
            follow_game: true,
            game_match: "world of warcraft".to_string(),
            overlay_split: false,
            window_alpha: 0.92,
            show_ranks: true,
            hide_realms: false,
            season_label: "this season".to_string(),
            season_start: None,
            season_end: None,
            character: None,
            character_class: None,
            chrome: crate::theme::Chrome::default().name().to_string(),
            density: crate::theme::Density::default().name().to_string(),
            home_on_start: true,
            extra: toml::Table::new(),
            load_failed: false,
        }
    }
}

#[cfg(test)]
thread_local! {
    /// A test's own config file: tests run on threads of their own, and a
    /// test that reads the file back must not see another test's save.
    static TEST_PATH: std::cell::RefCell<Option<PathBuf>> = const { std::cell::RefCell::new(None) };
}

impl Config {
    /// Point [`Config::path`] at `path` for the calling thread (a test's).
    #[cfg(test)]
    pub(crate) fn use_path_on_this_thread(path: Option<PathBuf>) {
        TEST_PATH.with(|p| *p.borrow_mut() = path);
    }

    pub fn path() -> PathBuf {
        #[cfg(test)]
        if let Some(path) = TEST_PATH.with(|p| p.borrow().clone()) {
            return path;
        }
        let base = std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .filter(|p| p.is_absolute())
            .unwrap_or_else(|| {
                let home = std::env::var_os("HOME").unwrap_or_default();
                PathBuf::from(home).join(".config")
            });
        base.join("wowdps").join("config.toml")
    }

    /// The daemon's `history_characters` — "Name-Realm" strings naming the
    /// characters that are "me". The GUI does not own the key (it rides in
    /// `extra`, untouched through a save), but Home needs it: a character
    /// the store has no cards for this season would otherwise vanish from
    /// its own dashboard.
    pub fn history_characters(&self) -> Vec<String> {
        // The daemon's reader takes either shape — a TOML array, or one
        // string of comma-separated names, which is how a hand-edited
        // config usually spells it — so this one must too, or the GUI
        // silently sees no characters at all.
        match self.extra.get("history_characters") {
            Some(toml::Value::Array(a)) => a
                .iter()
                .filter_map(toml::Value::as_str)
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect(),
            Some(toml::Value::String(s)) => s
                .split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect(),
            _ => Vec::new(),
        }
    }

    /// The configured density, or the default when the name is not one we
    /// know — a typo changes the spacing, it does not break the launch.
    pub fn density(&self) -> crate::theme::Density {
        crate::theme::Density::from_name(&self.density).unwrap_or_default()
    }

    /// The configured chrome; a name we do not know is the default, gold.
    pub fn chrome(&self) -> crate::theme::Chrome {
        crate::theme::Chrome::from_name(self.chrome.trim()).unwrap_or_default()
    }

    /// The remembered class of the locked character, when it names one.
    pub fn character_class(&self) -> Option<wowdps_model::Class> {
        self.character_class
            .as_deref()
            .and_then(crate::theme::class_named)
    }

    pub fn load() -> Self {
        Self::load_from(&Self::path())
    }

    fn load_from(path: &std::path::Path) -> Self {
        match std::fs::read_to_string(path) {
            Ok(text) => toml::from_str(&text).unwrap_or_else(|e| {
                eprintln!(
                    "wowdps: {}: {e}; using defaults (and not saving over it)",
                    path.display()
                );
                Self {
                    load_failed: true,
                    ..Self::default()
                }
            }),
            Err(_) => Self::default(),
        }
    }

    /// Best-effort: config trouble must never take down the meter.
    pub fn save(&self) {
        self.save_to(&Self::path());
    }

    /// Remember the locked character's class — and touch nothing else.
    ///
    /// The window learns the class on its own, with no gesture behind it
    /// (the owner turning up on the meter), and a whole-struct `save` then
    /// would write back everything the window read at launch: the overlay
    /// process shares this file, and a drag or zoom it saved since would
    /// be undone behind the user's back. So this re-reads the file, sets
    /// the one key, and writes that back.
    pub fn store_character_class(class: Option<String>) {
        Self::store_character_class_at(&Self::path(), class);
    }

    fn store_character_class_at(path: &std::path::Path, class: Option<String>) {
        // An EMPTY file that exists is another writer caught mid-save (an
        // overlay built before saves were atomic truncates, then writes):
        // read as defaults and written back, it would lose everything that
        // writer was saving. The class waits for the next time it is learned.
        if std::fs::metadata(path).is_ok_and(|m| m.len() == 0) {
            return;
        }
        let mut disk = Self::load_from(path);
        if disk.character_class == class {
            return;
        }
        disk.character_class = class;
        disk.save_to(path);
    }

    fn save_to(&self, path: &std::path::Path) {
        if self.load_failed {
            eprintln!(
                "wowdps: not saving {}: it did not parse on load and a save would drop its other keys",
                path.display()
            );
            return;
        }
        let write = || -> std::io::Result<()> {
            if let Some(dir) = path.parent() {
                std::fs::create_dir_all(dir)?;
            }
            // Serialization of a plain struct of scalars cannot fail in
            // practice; if it ever did, it is a save failure like any other.
            let text = toml::to_string_pretty(self)
                .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
            write_atomic(path, &text)
        };
        if let Err(e) = write() {
            eprintln!("wowdps: could not save {}: {e}", path.display());
        }
    }
}

/// Write `text` to `path` so no reader ever sees half of it: into a sibling
/// temporary file, then renamed over the target — the daemon's
/// `cache::write_atomic`, the pattern every durable file here follows. The
/// window and the overlay share this file, and a read-modify-write that
/// caught the other mid-`write` read a truncated file as the defaults.
/// A config that is a symlink (a dotfiles checkout) keeps its link: the
/// rename lands on what the link points at.
fn write_atomic(path: &std::path::Path, text: &str) -> std::io::Result<()> {
    static SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let target = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let seq = SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let mut name = target.file_name().unwrap_or_default().to_os_string();
    name.push(format!(".{}-{seq}.tmp", std::process::id()));
    let tmp = target.with_file_name(name);
    let written = std::fs::write(&tmp, text).and_then(|()| std::fs::rename(&tmp, &target));
    if written.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    written
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_path(tag: &str) -> PathBuf {
        std::env::temp_dir().join(format!("wowdps-config-{tag}-{}", std::process::id()))
    }

    #[test]
    fn a_missing_file_yields_defaults() {
        let cfg = Config::load_from(std::path::Path::new("/nonexistent/config.toml"));
        assert_eq!(cfg, Config::default());
    }

    #[test]
    fn saved_config_round_trips() {
        let dir = temp_path("roundtrip");
        let path = dir.join("wowdps").join("config.toml");
        let cfg = Config {
            edge: Edge::Left,
            offset: 512,
            width: 400,
            height: 600,
            zoom: 1.5,
            monitor: Some("DP-3".to_string()),
            follow_game: false,
            game_match: "wow.exe".to_string(),
            overlay_split: true,
            window_alpha: 0.8,
            show_ranks: false,
            hide_realms: true,
            season_label: "season 3".to_string(),
            season_start: Some("2026-08-12".to_string()),
            season_end: None,
            character: Some("Player-1234-ABCDEF".to_string()),
            character_class: Some("Death Knight".to_string()),
            chrome: "class".to_string(),
            density: "compact".to_string(),
            home_on_start: false,
            extra: toml::Table::new(),
            load_failed: false,
        };
        cfg.save_to(&path);
        assert_eq!(Config::load_from(&path), cfg);
        std::fs::remove_dir_all(&dir).ok();
    }

    /// The chrome is gold unless the file says `class`: missing, unknown
    /// and misspelt all read gold, and a save writes the name back as read.
    #[test]
    fn the_chrome_defaults_to_gold_and_round_trips() {
        use crate::theme::Chrome;
        assert_eq!(Config::default().chrome(), Chrome::Gold);
        assert_eq!(Config::default().chrome, "gold");
        let dir = temp_path("chrome");
        let path = dir.join("config.toml");
        std::fs::create_dir_all(&dir).unwrap();
        for (text, want) in [
            ("zoom = 1.0\n", Chrome::Gold),
            ("chrome = \"gold\"\n", Chrome::Gold),
            ("chrome = \"class\"\n", Chrome::Class),
            ("chrome = \"purple\"\n", Chrome::Gold),
        ] {
            std::fs::write(&path, text).unwrap();
            let cfg = Config::load_from(&path);
            assert!(!cfg.load_failed, "{text}");
            assert_eq!(cfg.chrome(), want, "{text}");
            cfg.save_to(&path);
            assert_eq!(Config::load_from(&path), cfg, "{text} round-trips");
        }
        // The remembered class reads by its in-game name.
        std::fs::write(
            &path,
            "chrome = \"class\"\ncharacter_class = \"Demon Hunter\"\n",
        )
        .unwrap();
        let cfg = Config::load_from(&path);
        assert_eq!(
            cfg.character_class(),
            Some(wowdps_model::Class::DemonHunter)
        );
        assert_eq!(
            Config {
                character_class: Some("Bard".to_string()),
                ..Config::default()
            }
            .character_class(),
            None
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    /// The learned class is written into the file AS IT IS NOW: whatever
    /// another process saved since this one read it survives.
    #[test]
    fn storing_the_class_keeps_what_another_process_saved() {
        let dir = temp_path("class-only");
        let path = dir.join("config.toml");
        std::fs::create_dir_all(&dir).unwrap();
        let launch = Config {
            offset: 100,
            ..Config::default()
        };
        launch.save_to(&path);
        // The overlay is dragged after the window read the file.
        let mut dragged = Config::load_from(&path);
        dragged.offset = 777;
        dragged.save_to(&path);
        Config::store_character_class_at(&path, Some("Warlock".to_string()));
        let now = Config::load_from(&path);
        assert_eq!(now.offset, 777, "the overlay's drag survives");
        assert_eq!(now.character_class.as_deref(), Some("Warlock"));
        // A file that did not parse is not rewritten.
        std::fs::write(&path, "offset = [broken\n").unwrap();
        Config::store_character_class_at(&path, Some("Mage".to_string()));
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "offset = [broken\n"
        );
        // Nor is an EMPTY one: another writer caught mid-save, whose
        // placement a write-back of the defaults would erase.
        std::fs::write(&path, "").unwrap();
        Config::store_character_class_at(&path, Some("Mage".to_string()));
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "");
        std::fs::remove_dir_all(&dir).ok();
    }

    /// A save replaces the file whole — a reader sees the old file or the
    /// new one, never a truncated one — leaves no temporary behind, and
    /// writes through a symlinked config to its target.
    #[test]
    fn a_save_is_atomic_and_keeps_a_symlink() {
        let dir = temp_path("atomic");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let real = dir.join("real.toml");
        let link = dir.join("config.toml");
        Config::default().save_to(&real);
        std::os::unix::fs::symlink(&real, &link).unwrap();
        let cfg = Config {
            offset: 42,
            ..Config::default()
        };
        cfg.save_to(&link);
        assert!(
            std::fs::symlink_metadata(&link)
                .unwrap()
                .file_type()
                .is_symlink(),
            "the link is still a link"
        );
        assert_eq!(Config::load_from(&real).offset, 42, "written through it");
        let names: Vec<String> = std::fs::read_dir(&dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        assert!(
            names.iter().all(|n| !n.ends_with(".tmp")),
            "no temporary left: {names:?}"
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn partial_files_fill_in_defaults() {
        let dir = temp_path("partial");
        let path = dir.join("config.toml");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(&path, "edge = \"bottom\"\noffset = 42\n").unwrap();
        let cfg = Config::load_from(&path);
        assert_eq!(cfg.edge, Edge::Bottom);
        assert_eq!(cfg.offset, 42);
        assert_eq!(cfg.width, Config::default().width);
        assert_eq!(cfg.zoom, Config::default().zoom);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn garbage_falls_back_to_defaults() {
        let dir = temp_path("garbage");
        let path = dir.join("config.toml");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(&path, "edge = 17 this is not toml").unwrap();
        let cfg = Config::load_from(&path);
        assert!(cfg.load_failed);
        assert_eq!(
            cfg,
            Config {
                load_failed: true,
                ..Config::default()
            }
        );
        std::fs::remove_dir_all(&dir).ok();
    }
}

#[cfg(test)]
mod passthrough {
    use super::*;

    /// The daemon owns keys in the same file (`logs_dir`, `history_*`);
    /// a GUI save must carry them through untouched — it used to rewrite
    /// the whole file from this struct and erase them.
    #[test]
    fn a_save_preserves_keys_the_gui_does_not_own() {
        let dir = std::env::temp_dir().join(format!("wowdps-config-extra-{}", std::process::id()));
        let path = dir.join("wowdps").join("config.toml");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(
            &path,
            "logs_dir = \"/games/wow/Logs\"\nhistory_enabled = false\nhistory_keep_per_encounter = 50\nzoom = 2.0\n",
        )
        .unwrap();
        let mut cfg = Config::load_from(&path);
        assert_eq!(cfg.zoom, 2.0);
        cfg.zoom = 1.0;
        cfg.save_to(&path);
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains("logs_dir = \"/games/wow/Logs\""), "{text}");
        assert!(text.contains("history_enabled = false"), "{text}");
        assert!(text.contains("history_keep_per_encounter = 50"), "{text}");
        assert!(text.contains("zoom = 1.0"), "{text}");
        assert_eq!(Config::load_from(&path).zoom, 1.0);
        std::fs::remove_dir_all(&dir).ok();
    }

    /// A file the strict parser rejects (here: a duplicated key, which the
    /// daemon's tolerant reader accepts) must not be rewritten from the
    /// defaults — that would erase every key the GUI does not own.
    #[test]
    fn a_file_that_failed_to_parse_is_never_saved_over() {
        let dir = std::env::temp_dir().join(format!("wowdps-config-broken-{}", std::process::id()));
        let path = dir.join("wowdps").join("config.toml");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let text = "logs_dir = \"/a\"\nlogs_dir = \"/b\"\nhistory_characters = [\"Me\"]\n";
        std::fs::write(&path, text).unwrap();
        let mut cfg = Config::load_from(&path);
        assert!(cfg.load_failed);
        cfg.zoom = 3.0;
        cfg.save_to(&path);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), text);
        std::fs::remove_dir_all(&dir).ok();
    }

    /// `history_characters` reads as an array or as one comma-separated
    /// string — the daemon takes both, so the window must too.
    #[test]
    fn history_characters_read_either_spelling() {
        let mut cfg = Config::default();
        cfg.extra.insert(
            "history_characters".to_string(),
            toml::Value::String("A-Realm-US, B-Realm-US ,,".to_string()),
        );
        assert_eq!(cfg.history_characters(), vec!["A-Realm-US", "B-Realm-US"]);
        cfg.extra.insert(
            "history_characters".to_string(),
            toml::Value::Array(vec![toml::Value::String("C-Realm-US".to_string())]),
        );
        assert_eq!(cfg.history_characters(), vec!["C-Realm-US"]);
        cfg.extra.remove("history_characters");
        assert!(cfg.history_characters().is_empty());
    }
}
