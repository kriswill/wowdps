//! The daemon's read of `~/.config/wowdps/config.toml` — the same file the
//! gui writes with the real `toml` crate. Hand-rolled subset reader to keep
//! the daemon stdlib-only: `[section]` headers are tracked (daemon keys live
//! at the top level only), values are bare bools/ints and double-quoted
//! strings, everything else is ignored.

use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq)]
pub struct Config {
    /// What to tail when no `--file`/`--logs` override is given. `None`
    /// means "not configured": the daemon then discovers the install
    /// (`wowdps_core::cli::default_logs_dir`) instead of assuming a path.
    pub logs_dir: Option<PathBuf>,
    /// Case-insensitive substring matched against /proc comm+cmdline.
    pub game_process: String,
    pub auto_overlay: bool,
    pub overlay_exit_grace_secs: u64,
    /// History store (roadmap item 1): write fight summaries at all.
    pub history_enabled: bool,
    /// Override of `$XDG_DATA_HOME/wowdps/history/v1`.
    pub history_dir: Option<PathBuf>,
    /// Also store out-of-encounter Trash segments.
    pub history_store_trash: bool,
    /// Cards + rows kept per (encounter id, difficulty).
    pub history_keep_per_encounter: u64,
    /// Details files kept per (encounter id, difficulty).
    pub history_keep_details_per_encounter: u64,
    /// A wipe this long (seconds) or longer gets a details file too;
    /// shorter wipes (and aborted fights) never do.
    pub history_details_min_wipe_secs: u64,
    /// "Name-Realm, …" that are "me"; empty = infer from COMBATANT_INFO.
    pub history_characters: Vec<String>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            logs_dir: None,
            game_process: "wow.exe".to_string(),
            auto_overlay: true,
            overlay_exit_grace_secs: 180,
            history_enabled: true,
            history_dir: None,
            history_store_trash: false,
            history_keep_per_encounter: 200,
            history_keep_details_per_encounter: 10,
            history_details_min_wipe_secs: 60,
            history_characters: Vec::new(),
        }
    }
}

impl Config {
    pub fn path() -> PathBuf {
        let base = std::env::var_os("XDG_CONFIG_HOME")
            .filter(|v| !v.is_empty())
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
            .unwrap_or_default();
        base.join("wowdps/config.toml")
    }

    pub fn load() -> Self {
        Self::load_from(&Self::path())
    }

    pub fn load_from(path: &Path) -> Self {
        match std::fs::read_to_string(path) {
            Ok(text) => Self::parse(&text),
            Err(_) => Self::default(),
        }
    }

    /// Tolerant by design: unknown keys, malformed lines and whole sections
    /// belong to other tools (or future versions) and are skipped, never an
    /// error.
    pub fn parse(text: &str) -> Self {
        let mut cfg = Self::default();
        let mut in_root = true;
        for raw in text.lines() {
            let line = raw.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            if line.starts_with('[') {
                // A naive matcher would happily read `game_process` out of a
                // future `[overlay]` table; section tracking is the point.
                in_root = false;
                continue;
            }
            if !in_root {
                continue;
            }
            let Some((key, value)) = line.split_once('=') else {
                continue;
            };
            let (key, value) = (key.trim(), strip_comment(value.trim()));
            match key {
                "logs_dir" => {
                    if let Some(s) = parse_string(value) {
                        cfg.logs_dir = Some(PathBuf::from(s));
                    }
                }
                "game_process" => {
                    if let Some(s) = parse_string(value) {
                        cfg.game_process = s;
                    }
                }
                "auto_overlay" => {
                    if let Some(b) = parse_bool(value) {
                        cfg.auto_overlay = b;
                    }
                }
                "overlay_exit_grace_secs" => {
                    if let Ok(n) = value.parse::<u64>() {
                        cfg.overlay_exit_grace_secs = n;
                    }
                }
                "history_enabled" => {
                    if let Some(b) = parse_bool(value) {
                        cfg.history_enabled = b;
                    }
                }
                "history_dir" => {
                    if let Some(s) = parse_string(value) {
                        cfg.history_dir = Some(PathBuf::from(s));
                    }
                }
                "history_store_trash" => {
                    if let Some(b) = parse_bool(value) {
                        cfg.history_store_trash = b;
                    }
                }
                "history_keep_per_encounter" => {
                    if let Ok(n) = value.parse::<u64>() {
                        cfg.history_keep_per_encounter = n;
                    }
                }
                "history_keep_details_per_encounter" => {
                    if let Ok(n) = value.parse::<u64>() {
                        cfg.history_keep_details_per_encounter = n;
                    }
                }
                "history_details_min_wipe_secs" => {
                    if let Ok(n) = value.parse::<u64>() {
                        cfg.history_details_min_wipe_secs = n;
                    }
                }
                // The reader has no list type: one comma-separated string.
                "history_characters" => {
                    if let Some(s) = parse_string(value) {
                        cfg.history_characters = s
                            .split(',')
                            .map(str::trim)
                            .filter(|c| !c.is_empty())
                            .map(str::to_string)
                            .collect();
                    }
                }
                _ => {}
            }
        }
        cfg
    }
}

/// Drop a trailing `# comment` — but never inside a quoted string.
fn strip_comment(value: &str) -> &str {
    let mut in_quotes = false;
    for (i, c) in value.char_indices() {
        match c {
            '"' => in_quotes = !in_quotes,
            '#' if !in_quotes => return value.get(..i).unwrap_or(value).trim_end(),
            _ => {}
        }
    }
    value
}

fn parse_bool(value: &str) -> Option<bool> {
    match value {
        "true" => Some(true),
        "false" => Some(false),
        _ => None,
    }
}

/// A double-quoted TOML basic string, with the two escapes that can appear
/// in paths (`\\` and `\"`) handled; anything fancier is ignored.
fn parse_string(value: &str) -> Option<String> {
    let inner = value.strip_prefix('"')?.strip_suffix('"')?;
    let mut out = String::with_capacity(inner.len());
    let mut chars = inner.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.next()? {
                '"' => out.push('"'),
                '\\' => out.push('\\'),
                other => {
                    out.push('\\');
                    out.push(other);
                }
            }
        } else {
            out.push(c);
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_missing_file_is_the_default_config() {
        let cfg = Config::load_from(Path::new("/nonexistent/wowdps.toml"));
        assert_eq!(cfg, Config::default());
        assert_eq!(cfg.game_process, "wow.exe");
        assert!(cfg.auto_overlay);
    }

    #[test]
    fn a_real_gui_written_file_parses() {
        // What the gui's `toml` crate actually emits today, plus daemon keys.
        let text = r#"
edge = "right"
offset = 0.35
width = 420.0
height = 640.0
zoom = 1.25
monitor = ""
follow_game = true
game_match = "wow"
logs_dir = "/games/World of Warcraft/_retail_/Logs"
game_process = "Wow.exe"
auto_overlay = false
overlay_exit_grace_secs = 60
"#;
        let cfg = Config::parse(text);
        assert_eq!(
            cfg.logs_dir,
            Some(PathBuf::from("/games/World of Warcraft/_retail_/Logs"))
        );
        assert_eq!(cfg.game_process, "Wow.exe");
        assert!(!cfg.auto_overlay);
        assert_eq!(cfg.overlay_exit_grace_secs, 60);
    }

    #[test]
    fn history_keys_parse_and_default() {
        let cfg = Config::parse(
            "history_details_min_wipe_secs = 90\nhistory_keep_details_per_encounter = 4\n",
        );
        assert_eq!(cfg.history_details_min_wipe_secs, 90);
        assert_eq!(cfg.history_keep_details_per_encounter, 4);
        assert_eq!(Config::default().history_details_min_wipe_secs, 60);
        // Wrong type: the default stands.
        let cfg = Config::parse(r#"history_details_min_wipe_secs = "ninety""#);
        assert_eq!(cfg.history_details_min_wipe_secs, 60);
    }

    #[test]
    fn keys_inside_sections_are_not_ours() {
        let text = r#"
auto_overlay = false

[overlay]
game_process = "notepad.exe"
auto_overlay = true
"#;
        let cfg = Config::parse(text);
        assert!(!cfg.auto_overlay, "root key read");
        assert_eq!(cfg.game_process, "wow.exe", "sectioned key ignored");
    }

    #[test]
    fn garbage_is_tolerated_not_fatal() {
        let text = r#"
this is not toml at all
game_process = 42
auto_overlay = "yes"
overlay_exit_grace_secs = ten
= = =
logs_dir = "/ok"   # with a trailing comment
"#;
        let cfg = Config::parse(text);
        assert_eq!(cfg.game_process, "wow.exe", "wrong type ignored");
        assert!(cfg.auto_overlay, "wrong type ignored");
        assert_eq!(cfg.overlay_exit_grace_secs, 180);
        assert_eq!(cfg.logs_dir, Some(PathBuf::from("/ok")));
    }

    #[test]
    fn quoted_strings_keep_hashes_and_escapes() {
        let cfg = Config::parse(r#"logs_dir = "/data/#logs/wow""#);
        assert_eq!(cfg.logs_dir, Some(PathBuf::from("/data/#logs/wow")));
        let cfg = Config::parse(r#"game_process = "a\"b""#);
        assert_eq!(cfg.game_process, "a\"b");
    }

    #[test]
    fn escapes_in_strings_are_unescaped_or_kept_verbatim() {
        let cfg = Config::parse(r#"logs_dir = "C:\\Games\\WoW""#);
        assert_eq!(cfg.logs_dir, Some(PathBuf::from(r"C:\Games\WoW")));
        // An escape we don't model stays as written — never dropped.
        let cfg = Config::parse(r#"game_process = "a\tb""#);
        assert_eq!(cfg.game_process, "a\\tb");
        // A dangling backslash makes the value unparseable: ignored.
        let cfg = Config::parse(r#"game_process = "a\""#);
        assert_eq!(cfg.game_process, "wow.exe");
    }

    /// The config file lives under `$XDG_CONFIG_HOME`, else `$HOME/.config`,
    /// and `load` reads whatever is there.
    #[test]
    fn the_path_follows_the_environment_and_load_reads_it() {
        let root = std::env::temp_dir().join(format!("wowdps-config-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("wowdps")).unwrap();
        // Env is process-global; this is the only test touching it.
        unsafe { std::env::set_var("XDG_CONFIG_HOME", &root) };
        assert_eq!(Config::path(), root.join("wowdps/config.toml"));
        assert_eq!(Config::load(), Config::default(), "no file yet");
        std::fs::write(
            Config::path(),
            "auto_overlay = false\noverlay_exit_grace_secs = 7\n",
        )
        .unwrap();
        let cfg = Config::load();
        assert!(!cfg.auto_overlay);
        assert_eq!(cfg.overlay_exit_grace_secs, 7);

        unsafe { std::env::set_var("XDG_CONFIG_HOME", "") };
        match std::env::var_os("HOME") {
            Some(home) => assert_eq!(
                Config::path(),
                PathBuf::from(home).join(".config/wowdps/config.toml")
            ),
            None => assert_eq!(Config::path(), PathBuf::from("wowdps/config.toml")),
        }
        unsafe { std::env::remove_var("XDG_CONFIG_HOME") };
        let _ = std::fs::remove_dir_all(&root);
    }
}
