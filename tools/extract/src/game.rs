//! One opened install: local storage, TACT keys, and the decoded
//! encoding/root manifests, ready to resolve FileDataIDs to file bytes.
//! Progress goes to stderr — this backs dev-time CLI commands.

use crate::blte::{self, Keys};
use crate::casc::{self, LocalStore};
use crate::tact;
use std::path::Path;

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
