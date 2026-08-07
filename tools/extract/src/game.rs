//! One opened install: local storage, TACT keys, and the decoded
//! encoding/root manifests, ready to resolve FileDataIDs to file bytes.
//! Progress goes to stderr — this backs dev-time CLI commands.

use crate::blte::{self, Keys};
use crate::casc::{self, LocalStore};
use crate::tact;
use std::path::{Path, PathBuf};

/// Find the install without a hardcoded path. In order: `$WOWDPS_WOW_DIR`,
/// the wowdps config's `logs_dir` (walking up to the folder holding
/// `.build.info` — the daemon already knows where the game is), then a
/// scan of the conventional Steam compatdata roots (shared with the
/// daemon: `wowdps_core::cli`). Exactly one scan hit is required; several
/// installs must be disambiguated explicitly — unlike the daemon, a CLI
/// user is there to answer.
pub fn locate() -> Result<PathBuf, String> {
    if let Some(dir) = std::env::var_os("WOWDPS_WOW_DIR") {
        let dir = PathBuf::from(dir);
        if !wowdps_core::cli::is_wow_install(&dir) {
            return Err(format!(
                "WOWDPS_WOW_DIR={} is not a WoW install (no .build.info + Data/data)",
                dir.display()
            ));
        }
        eprintln!("install: {} (from WOWDPS_WOW_DIR)", dir.display());
        return Ok(dir);
    }

    if let Some(logs) = config_logs_dir()
        && let Some(dir) = logs
            .ancestors()
            .find(|p| wowdps_core::cli::is_wow_install(p))
    {
        eprintln!("install: {} (from wowdps config logs_dir)", dir.display());
        return Ok(dir.to_path_buf());
    }

    let found = std::env::var_os("HOME")
        .map(|h| wowdps_core::cli::discover_wow_installs(Path::new(&h)))
        .unwrap_or_default();
    match found.as_slice() {
        [] => Err(
            "no WoW install found: pass the World of Warcraft directory (the one \
                  holding .build.info and Data/), or set WOWDPS_WOW_DIR, or set logs_dir \
                  in ~/.config/wowdps/config.toml"
                .into(),
        ),
        [only] => {
            eprintln!("install: {} (from Steam scan)", only.display());
            Ok(only.clone())
        }
        _ => Err(format!(
            "multiple WoW installs found — pass one explicitly or set WOWDPS_WOW_DIR:\n  {}",
            found
                .iter()
                .map(|p| p.display().to_string())
                .collect::<Vec<_>>()
                .join("\n  ")
        )),
    }
}

/// `logs_dir` from `~/.config/wowdps/config.toml` (top-level keys only,
/// matching the daemon's section-aware toml-subset reader).
fn config_logs_dir() -> Option<PathBuf> {
    let config_home = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))?;
    let text = std::fs::read_to_string(config_home.join("wowdps").join("config.toml")).ok()?;
    logs_dir_from_config(&text).map(PathBuf::from)
}

fn logs_dir_from_config(text: &str) -> Option<String> {
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            break; // top-level keys end at the first section
        }
        if let Some(v) = line.strip_prefix("logs_dir")
            && let Some(v) = v.trim_start().strip_prefix('=')
        {
            return Some(v.trim().trim_matches('"').to_string());
        }
    }
    None
}

pub struct Game {
    store: LocalStore,
    keys: Keys,
    encoding: Vec<u8>,
    root: Vec<u8>,
    pub build: String,
}

impl Game {
    /// `wow_dir` is the folder holding `.build.info` and `Data/`. Keys come
    /// from the install's keyring config plus an optional TACTKeys file.
    pub fn open(wow_dir: &Path, keys_path: Option<&Path>) -> Result<Game, String> {
        let read_text = |p: &Path| -> Result<String, String> {
            std::fs::read_to_string(p).map_err(|e| format!("{}: {e}", p.display()))
        };

        let info = tact::BuildInfo::parse(&read_text(&wow_dir.join(".build.info"))?, "wow")?;
        eprintln!("build {} (config {})", info.version, info.build_key);

        let data_dir = wow_dir.join("Data");
        let cfg =
            tact::BuildConfig::parse(&read_text(&tact::config_path(&data_dir, &info.build_key))?)?;

        let mut keys = Keys::new();
        if let Some(ref keyring) = info.keyring
            && let Ok(text) = read_text(&tact::config_path(&data_dir, keyring))
        {
            let n = tact::load_keys(&text, &mut keys)?;
            eprintln!("keyring: {n} key(s)");
        }
        if let Some(p) = keys_path {
            let n = tact::load_keys(&read_text(p)?, &mut keys)?;
            eprintln!("{}: {n} key(s)", p.display());
        }

        let store = LocalStore::open(&data_dir.join("data"))?;
        eprintln!("local storage: {} entries", store.entry_count());

        let encoding = blte::decode(&store.read(&cfg.encoding_ekey)?, &keys)?;
        let root_ekey = tact::Encoding::new(&encoding)?
            .ekey(&cfg.root_ckey)
            .ok_or("root manifest not in encoding table")?;
        let root = blte::decode(&store.read(&root_ekey)?, &keys)?;
        eprintln!("root manifest: {} bytes", root.len());

        Ok(Game {
            store,
            keys,
            encoding,
            root,
            build: info.version,
        })
    }

    pub fn root(&self) -> &[u8] {
        &self.root
    }

    /// Decoded bytes of the file with this content key.
    pub fn fetch_ckey(&self, ckey: &[u8; 16]) -> Result<Vec<u8>, String> {
        let ekey = tact::Encoding::new(&self.encoding)?
            .ekey(ckey)
            .ok_or_else(|| format!("ckey {} not in encoding table", casc::hex(ckey)))?;
        blte::decode(&self.store.read(&ekey)?, &self.keys)
    }

    /// Decoded bytes of a FileDataID, preferring `locale_mask` blocks.
    pub fn fetch_fdid(&self, fdid: u32, locale_mask: u32) -> Result<Vec<u8>, String> {
        let m = tact::root_find(&self.root, Some(fdid), None, locale_mask)?
            .ok_or_else(|| format!("fdid {fdid} not found in root manifest"))?;
        self.fetch_ckey(&m.ckey)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn logs_dir_parsing() {
        assert_eq!(
            logs_dir_from_config("zoom = 1.5\nlogs_dir = \"/a/b/Logs\"\n"),
            Some("/a/b/Logs".into())
        );
        // Only top-level keys count; sections end the search.
        assert_eq!(
            logs_dir_from_config("[overlay]\nlogs_dir = \"/nope\"\n"),
            None
        );
        assert_eq!(logs_dir_from_config("edge = \"right\"\n"), None);
    }

    #[test]
    fn ancestor_walk_finds_the_install_root_from_a_logs_dir() {
        let dir = std::env::temp_dir().join(format!("wowdps-game-test-{}", std::process::id()));
        let wow = dir.join("World of Warcraft");
        std::fs::create_dir_all(wow.join("Data").join("data")).unwrap();
        std::fs::write(wow.join(".build.info"), "x").unwrap();

        // The daemon's logs_dir points below the install root.
        let logs = wow.join("_retail_").join("Logs");
        assert_eq!(
            logs.ancestors()
                .find(|p| wowdps_core::cli::is_wow_install(p)),
            Some(wow.as_path())
        );

        std::fs::remove_dir_all(&dir).unwrap();
    }
}
