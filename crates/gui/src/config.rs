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
}

impl Default for Config {
    fn default() -> Self {
        Self {
            edge: Edge::Right,
            offset: 300,
            width: 380,
            height: 460,
            zoom: 1.25,
            monitor: None,
            follow_game: true,
            game_match: "world of warcraft".to_string(),
            overlay_split: false,
        }
    }
}

impl Config {
    pub fn path() -> PathBuf {
        let base = std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .filter(|p| p.is_absolute())
            .unwrap_or_else(|| {
                let home = std::env::var_os("HOME").unwrap_or_default();
                PathBuf::from(home).join(".config")
            });
        base.join("wowdps").join("config.toml")
    }

    pub fn load() -> Self {
        Self::load_from(&Self::path())
    }

    fn load_from(path: &std::path::Path) -> Self {
        match std::fs::read_to_string(path) {
            Ok(text) => toml::from_str(&text).unwrap_or_else(|e| {
                eprintln!("wowdps: {}: {e}; using defaults", path.display());
                Self::default()
            }),
            Err(_) => Self::default(),
        }
    }

    /// Best-effort: config trouble must never take down the meter.
    pub fn save(&self) {
        self.save_to(&Self::path());
    }

    fn save_to(&self, path: &std::path::Path) {
        let write = || -> std::io::Result<()> {
            if let Some(dir) = path.parent() {
                std::fs::create_dir_all(dir)?;
            }
            // Serialization of a plain struct of scalars cannot fail in
            // practice; if it ever did, it is a save failure like any other.
            let text = toml::to_string_pretty(self)
                .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
            std::fs::write(path, text)
        };
        if let Err(e) = write() {
            eprintln!("wowdps: could not save {}: {e}", path.display());
        }
    }
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
        };
        cfg.save_to(&path);
        assert_eq!(Config::load_from(&path), cfg);
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
        assert_eq!(Config::load_from(&path), Config::default());
        std::fs::remove_dir_all(&dir).ok();
    }
}
