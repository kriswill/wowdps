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

#[derive(Debug)]
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

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("wowdps-game-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn make_install(wow: &Path) {
        std::fs::create_dir_all(wow.join("Data").join("data")).unwrap();
        std::fs::write(wow.join(".build.info"), "x").unwrap();
    }

    /// `locate` reads process-global environment, so every branch runs in
    /// this one test, in order.
    #[test]
    fn locate_tries_env_then_config_then_the_steam_scan() {
        let dir = scratch("locate");
        let wow = dir.join("World of Warcraft");
        make_install(&wow);
        let home = dir.join("home");
        let cfg = dir.join("cfg");
        std::fs::create_dir_all(&home).unwrap();
        std::fs::create_dir_all(cfg.join("wowdps")).unwrap();
        // SAFETY: this test is the only reader/writer of these variables
        // in the process, and it holds no other threads of its own.
        unsafe {
            std::env::set_var("HOME", &home);
            std::env::set_var("XDG_CONFIG_HOME", &cfg);
            std::env::set_var("WOWDPS_WOW_DIR", dir.join("nope"));
        }
        assert!(locate().unwrap_err().contains("not a WoW install"));
        unsafe { std::env::set_var("WOWDPS_WOW_DIR", &wow) };
        assert_eq!(locate().unwrap(), wow);

        unsafe { std::env::remove_var("WOWDPS_WOW_DIR") };
        // Nothing configured, no Steam roots: a clear error.
        assert!(locate().unwrap_err().contains("no WoW install found"));
        std::fs::write(
            cfg.join("wowdps").join("config.toml"),
            format!(
                "logs_dir = \"{}\"\n",
                wow.join("_retail_").join("Logs").display()
            ),
        )
        .unwrap();
        assert_eq!(locate().unwrap(), wow);
        std::fs::remove_file(cfg.join("wowdps").join("config.toml")).unwrap();

        // One Steam install: found; two: ambiguous.
        let compat = home.join(".steam/steam/steamapps/compatdata");
        let steam1 = compat.join("1/pfx/drive_c/Program Files (x86)/World of Warcraft");
        make_install(&steam1);
        assert_eq!(locate().unwrap(), steam1.canonicalize().unwrap());
        let steam2 = compat.join("2/pfx/drive_c/Program Files (x86)/World of Warcraft");
        make_install(&steam2);
        assert!(locate().unwrap_err().contains("multiple WoW installs"));

        unsafe {
            std::env::remove_var("XDG_CONFIG_HOME");
        }
        std::fs::remove_dir_all(&dir).unwrap();
    }

    fn blte_plain(payload: &[u8]) -> Vec<u8> {
        let mut d = b"BLTE\0\0\0\0N".to_vec();
        d.extend_from_slice(payload);
        d
    }

    /// An encoding manifest with one 1 KiB page mapping each
    /// `(ckey, ekey)` pair; ckeys must ascend from the first.
    fn encoding(pairs: &[([u8; 16], [u8; 16])]) -> Vec<u8> {
        let mut d = Vec::new();
        d.extend_from_slice(b"EN\x01\x10\x10");
        d.extend_from_slice(&1u16.to_be_bytes());
        d.extend_from_slice(&1u16.to_be_bytes());
        d.extend_from_slice(&1u32.to_be_bytes());
        d.extend_from_slice(&0u32.to_be_bytes());
        d.push(0);
        d.extend_from_slice(&0u32.to_be_bytes());
        d.extend_from_slice(&pairs[0].0);
        d.extend_from_slice(&[0u8; 16]);
        let mut page = Vec::new();
        for (ckey, ekey) in pairs {
            page.push(1);
            page.extend_from_slice(&[0u8; 5]);
            page.extend_from_slice(ckey);
            page.extend_from_slice(ekey);
        }
        page.resize(1024, 0);
        d.extend_from_slice(&page);
        d
    }

    /// A version-1 root manifest with one enUS block of `(fdid, ckey)`.
    fn root(files: &[(u32, [u8; 16])]) -> Vec<u8> {
        let mut d = Vec::new();
        d.extend_from_slice(b"TSFM");
        d.extend_from_slice(&24u32.to_le_bytes());
        d.extend_from_slice(&1u32.to_le_bytes());
        d.extend_from_slice(&(files.len() as u32).to_le_bytes());
        d.extend_from_slice(&(files.len() as u32).to_le_bytes());
        d.extend_from_slice(&0u32.to_le_bytes());
        d.extend_from_slice(&(files.len() as u32).to_le_bytes());
        d.extend_from_slice(&0u32.to_le_bytes()); // content flags
        d.extend_from_slice(&0x2u32.to_le_bytes()); // enUS
        let mut last: i64 = -1;
        for (fdid, _) in files {
            let delta = i64::from(*fdid) - last - 1;
            d.extend_from_slice(&(delta as i32).to_le_bytes());
            last = i64::from(*fdid);
        }
        for (_, ckey) in files {
            d.extend_from_slice(ckey);
        }
        for _ in files {
            d.extend_from_slice(&0u64.to_le_bytes());
        }
        d
    }

    fn key(tag: u8) -> [u8; 16] {
        let mut k = [0u8; 16];
        k[0] = tag;
        k
    }

    /// A complete fake install: .build.info, build config, keyring config,
    /// sixteen journals and one archive holding the encoding, root and one
    /// file, all BLTE-plain.
    fn fake_install(wow: &Path) -> [u8; 16] {
        let (enc_ckey, enc_ekey) = (key(0x10), key(0xA0));
        let (root_ckey, root_ekey) = (key(0x20), key(0xB0));
        let (file_ckey, file_ekey) = (key(0x30), key(0xC0));
        let build_hash = "aabb".repeat(8);
        let keyring_hash = "ccdd".repeat(8);

        std::fs::create_dir_all(wow.join("Data").join("data")).unwrap();
        std::fs::write(
            wow.join(".build.info"),
            format!(
                "Build Key!HEX:16|Version!STRING:0|Product!STRING:0|KeyRing!HEX:16\n\
                 {build_hash}|12.0.0.1|wow|{keyring_hash}\n"
            ),
        )
        .unwrap();
        let cfg = |hash: &str, text: String| {
            let p = tact::config_path(&wow.join("Data"), hash);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, text).unwrap();
        };
        cfg(
            &build_hash,
            format!(
                "root = {}\nencoding = {} {}\n",
                casc::hex(&root_ckey),
                casc::hex(&enc_ckey),
                casc::hex(&enc_ekey)
            ),
        );
        cfg(
            &keyring_hash,
            "key-4eb4869f95f23b53 = c9316739348dcc033aa8112f9a3acf5d\n".to_string(),
        );

        let blobs = [
            (
                enc_ekey,
                blte_plain(&encoding(&[(root_ckey, root_ekey), (file_ckey, file_ekey)])),
            ),
            (root_ekey, blte_plain(&root(&[(41, file_ckey)]))),
            (file_ekey, blte_plain(b"hello file")),
        ];
        let mut archive = Vec::new();
        let mut entries = Vec::new();
        for (ekey, blob) in &blobs {
            let total = (0x1E + blob.len()) as u32;
            entries.push((*ekey, archive.len() as u64, total));
            archive.extend_from_slice(&casc::tests::archive_entry(ekey, total, blob));
        }
        let data = wow.join("Data").join("data");
        std::fs::write(data.join("data.000"), &archive).unwrap();
        for b in 0..16u8 {
            let mine: Vec<_> = entries
                .iter()
                .filter(|e| casc::bucket(&e.0) == b)
                .copied()
                .collect();
            std::fs::write(
                data.join(format!("{b:02x}00000001.idx")),
                casc::tests::idx_bytes(b, &mine),
            )
            .unwrap();
        }
        file_ckey
    }

    #[test]
    fn open_resolves_files_through_encoding_and_root() {
        let dir = scratch("open");
        let wow = dir.join("wow");
        let file_ckey = fake_install(&wow);
        let extra_keys = dir.join("keys.txt");
        std::fs::write(
            &extra_keys,
            "FA505078126ACB3E BDC51862ABED79B2DE48C8E7E66C6200\n",
        )
        .unwrap();

        let game = Game::open(&wow, Some(&extra_keys)).unwrap();
        assert_eq!(game.build, "12.0.0.1");
        assert_eq!(game.keys.len(), 4);
        assert_eq!(game.root().len(), 24 + 12 + 4 + 16 + 8);
        assert_eq!(game.fetch_fdid(41, 0x2).unwrap(), b"hello file");
        assert_eq!(game.fetch_ckey(&file_ckey).unwrap(), b"hello file");
        assert!(
            game.fetch_fdid(42, 0x2)
                .unwrap_err()
                .contains("not found in root manifest")
        );
        assert!(
            game.fetch_ckey(&key(0x99))
                .unwrap_err()
                .contains("not in encoding table")
        );
        // Without the keyring the install still opens.
        let game = Game::open(&wow, None).unwrap();
        assert_eq!(game.keys.len(), 2);

        // Breakage along the chain surfaces as errors.
        assert!(
            Game::open(&dir.join("absent"), None)
                .unwrap_err()
                .contains(".build.info")
        );
        assert!(Game::open(&wow, Some(&dir.join("nokeys"))).is_err());
        std::fs::remove_dir_all(wow.join("Data").join("config")).unwrap();
        assert!(Game::open(&wow, None).is_err());
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
