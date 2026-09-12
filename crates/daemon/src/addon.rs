//! The wowdps addon (`addon/` at the repo root, embedded here): a few lines
//! of Lua the game runs, which write every raid member's guild into its
//! SavedVariables — the one fact the combat log never carries. This module
//! owns its life on disk: finding the game's product directory from the
//! logs directory the daemon already tails, rendering the TOC against the
//! install's own build, installing (`wowdps addon install`), and telling a
//! current copy from a stale one so the daemon can rewrite the latter on
//! start. It never installs on its own — that is the user's call once —
//! and never touches an addon that is not ours.

use std::io;
use std::path::{Path, PathBuf};

use wowdps_core::cli::is_wow_install;

use crate::cache::write_atomic;

/// The addon's folder and file stem under `Interface/AddOns/`.
pub const NAME: &str = "wowdps";
/// The version stamped into the TOC — the daemon's own, so a stale copy
/// is one written by an older daemon.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
pub const LUA: &str = include_str!("../../../addon/wowdps.lua");
const TOC_TEMPLATE: &str = include_str!("../../../addon/wowdps.toc.in");

/// The product directory (`<install>/_retail_`) a logs directory belongs
/// to: the logs sit at `<product>/Logs`, and the product's parent is the
/// install root (`.build.info` + `Data/data`). `None` for a directory that
/// is not inside an install — a test fixture, a copied log folder — so
/// nothing is ever written next to the wrong thing.
pub fn product_dir(logs_dir: &Path) -> Option<PathBuf> {
    let product = logs_dir.parent()?;
    let root = product.parent()?;
    (logs_dir.file_name()?.to_str()? == "Logs" && is_wow_install(root))
        .then(|| product.to_path_buf())
}

/// The `## Interface:` number the install expects: its `.build.info`
/// `Version` (`12.1.0.69587`) as `120100`. The active `wow` (retail) row
/// wins; any active row otherwise — the file lists one row per product.
pub fn interface_version(product: &Path) -> Option<u32> {
    let root = product.parent()?;
    let text = std::fs::read_to_string(root.join(".build.info")).ok()?;
    build_version(&text).and_then(|v| interface_of(&v))
}

/// The `Version` column of the active retail row of a `.build.info`.
pub fn build_version(text: &str) -> Option<String> {
    let mut lines = text.lines();
    let header: Vec<&str> = lines.next()?.split('|').collect();
    let col = |name: &str| {
        header
            .iter()
            .position(|h| h.split('!').next() == Some(name))
    };
    let (version, product, active) = (col("Version")?, col("Product"), col("Active"));
    let rows: Vec<Vec<&str>> = lines
        .filter(|l| !l.trim().is_empty())
        .map(|l| l.split('|').collect())
        .collect();
    let is_active = |r: &Vec<&str>| active.is_none_or(|a| r.get(a) == Some(&"1"));
    let is_retail = |r: &Vec<&str>| product.is_none_or(|p| r.get(p) == Some(&"wow"));
    rows.iter()
        .find(|r| is_active(r) && is_retail(r))
        .or_else(|| rows.iter().find(|r| is_active(r)))
        .or_else(|| rows.first())
        .and_then(|r| r.get(version))
        .map(|v| v.to_string())
}

/// `12.1.0.69587` → `120100`: major × 10000 + minor × 100 + patch.
pub fn interface_of(version: &str) -> Option<u32> {
    let mut parts = version.split('.').map(|p| p.parse::<u32>().ok());
    let major = parts.next()??;
    let minor = parts.next()??;
    let patch = parts.next()??;
    Some(major * 10_000 + minor * 100 + patch)
}

/// The TOC for this build of the daemon against `interface`.
pub fn render_toc(interface: u32) -> String {
    TOC_TEMPLATE
        .replace("{interface}", &interface.to_string())
        .replace("{version}", VERSION)
}

/// Where the addon lives (or would) under `product`.
pub fn addon_dir(product: &Path) -> PathBuf {
    product.join("Interface").join("AddOns").join(NAME)
}

/// The addon as found in the game's AddOns folder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AddonState {
    /// No `wowdps` folder: never installed, and left that way.
    Missing,
    /// Both files match what this daemon would write; carries the version.
    Current(String),
    /// Present but not what this daemon would write (an older version, a
    /// build's new interface number, a hand edit); carries the version
    /// found, or `?` when the TOC does not say.
    Stale(String),
}

impl AddonState {
    /// The installed version, whatever its state.
    pub fn version(&self) -> Option<&str> {
        match self {
            AddonState::Missing => None,
            AddonState::Current(v) | AddonState::Stale(v) => Some(v),
        }
    }
}

/// The `## Version:` of a TOC, or `?`.
fn toc_version(toc: &str) -> String {
    toc.lines()
        .find_map(|l| l.strip_prefix("## Version:"))
        .map_or_else(|| "?".to_string(), |v| v.trim().to_string())
}

/// What is installed under `product` and whether it is what this daemon
/// would write. Without a readable `.build.info` the interface number
/// cannot be judged, so only the Lua and the version are compared.
pub fn inspect(product: &Path) -> AddonState {
    let dir = addon_dir(product);
    let Ok(toc) = std::fs::read_to_string(dir.join(format!("{NAME}.toc"))) else {
        return AddonState::Missing;
    };
    let version = toc_version(&toc);
    let lua = std::fs::read_to_string(dir.join(format!("{NAME}.lua"))).unwrap_or_default();
    let toc_current = match interface_version(product) {
        Some(interface) => toc == render_toc(interface),
        None => version == VERSION,
    };
    if toc_current && lua == LUA {
        AddonState::Current(version)
    } else {
        AddonState::Stale(version)
    }
}

/// Write the addon under `product` — both files, atomically, the TOC last
/// so a half-written install never loads. Needs the install's build to
/// stamp the interface number: without it the game would flag the addon
/// out of date, so the install is refused rather than guessed.
pub fn install(product: &Path) -> io::Result<String> {
    let interface = interface_version(product).ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::NotFound,
            format!(
                "cannot read the game version from {}/.build.info",
                product.parent().unwrap_or(product).display()
            ),
        )
    })?;
    let dir = addon_dir(product);
    std::fs::create_dir_all(&dir)?;
    write_atomic(&dir.join(format!("{NAME}.lua")), LUA.as_bytes())?;
    write_atomic(
        &dir.join(format!("{NAME}.toc")),
        render_toc(interface).as_bytes(),
    )?;
    Ok(VERSION.to_string())
}

/// The daemon's start-up rule: an installed addon that is out of date is
/// rewritten; one that was never installed is left missing — installing
/// is `wowdps addon install`, the user's decision. Returns what is there
/// afterwards.
pub fn ensure_current(product: &Path) -> io::Result<AddonState> {
    match inspect(product) {
        AddonState::Stale(_) => install(product).map(AddonState::Current),
        state => Ok(state),
    }
}

/// Every account's `wowdps.lua` under `product/WTF/Account/*/SavedVariables`
/// that exists, as `(account, path)`, account-sorted. The game writes it
/// on logout / reload / exit; an account that never ran the addon has none.
pub fn saved_variables(product: &Path) -> Vec<(String, PathBuf)> {
    let Ok(accounts) = std::fs::read_dir(product.join("WTF").join("Account")) else {
        return Vec::new();
    };
    let mut out: Vec<(String, PathBuf)> = accounts
        .flatten()
        .filter(|e| e.path().is_dir())
        .filter_map(|e| {
            let account = e.file_name().to_str()?.to_string();
            let path = e.path().join("SavedVariables").join(format!("{NAME}.lua"));
            path.is_file().then_some((account, path))
        })
        .collect();
    out.sort();
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fake_install(tag: &str, version: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!("wowdps-addon-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let wow = root.join("World of Warcraft");
        std::fs::create_dir_all(wow.join("Data").join("data")).unwrap();
        std::fs::create_dir_all(wow.join("_retail_").join("Logs")).unwrap();
        std::fs::write(
            wow.join(".build.info"),
            format!(
                "Branch!STRING:0|Active!DEC:1|Build Key!HEX:16|Version!STRING:0|Product!STRING:0\n\
                 us|0|aa|1.2.3.4|wow_classic\n\
                 us|1|bb|{version}|wow\n"
            ),
        )
        .unwrap();
        wow
    }

    #[test]
    fn the_product_dir_is_the_logs_dirs_parent_inside_an_install() {
        let wow = fake_install("product", "12.1.0.69587");
        let logs = wow.join("_retail_").join("Logs");
        assert_eq!(product_dir(&logs), Some(wow.join("_retail_")));
        // A logs folder that is not inside an install names nothing.
        assert_eq!(product_dir(&wow.join("Logs")), None);
        assert_eq!(product_dir(Path::new("/tmp/logs")), None);
        assert_eq!(product_dir(&wow.join("_retail_").join("Other")), None);
        let _ = std::fs::remove_dir_all(wow.parent().unwrap());
    }

    #[test]
    fn the_interface_number_comes_from_the_active_retail_row() {
        let wow = fake_install("interface", "12.1.0.69587");
        assert_eq!(interface_version(&wow.join("_retail_")), Some(120_100));
        assert_eq!(interface_of("11.0.7.58238"), Some(110_007));
        assert_eq!(interface_of("nope"), None);
        // Header-driven, so column order is irrelevant; the classic row
        // (active 0, another product) never wins over retail.
        let text = "Version!STRING:0|Product!STRING:0|Active!DEC:1\n1.15.5.1|wow_classic_era|1\n12.0.0.1|wow|1\n";
        assert_eq!(build_version(text).as_deref(), Some("12.0.0.1"));
        // Only a non-retail row: it still answers rather than nothing.
        let text = "Version!STRING:0|Product!STRING:0|Active!DEC:1\n1.15.5.1|wow_classic_era|1\n";
        assert_eq!(build_version(text).as_deref(), Some("1.15.5.1"));
        assert_eq!(build_version("Branch!STRING:0\nus\n"), None);
        assert_eq!(build_version(""), None);
        let _ = std::fs::remove_dir_all(wow.parent().unwrap());
    }

    #[test]
    fn install_inspect_and_the_start_up_rule() {
        let wow = fake_install("install", "12.1.0.69587");
        let product = wow.join("_retail_");
        assert_eq!(inspect(&product), AddonState::Missing);
        // Never installed: the daemon leaves it that way.
        assert_eq!(ensure_current(&product).unwrap(), AddonState::Missing);
        assert!(!addon_dir(&product).exists());

        assert_eq!(install(&product).unwrap(), VERSION);
        let dir = addon_dir(&product);
        let toc = std::fs::read_to_string(dir.join("wowdps.toc")).unwrap();
        assert!(toc.starts_with("## Interface: 120100\n"), "{toc}");
        assert!(toc.contains(&format!("## Version: {VERSION}\n")), "{toc}");
        assert!(toc.contains("## SavedVariables: WOWDPS_DATA\n"), "{toc}");
        assert!(toc.trim_end().ends_with("wowdps.lua"), "{toc}");
        assert_eq!(
            std::fs::read_to_string(dir.join("wowdps.lua")).unwrap(),
            LUA
        );
        assert_eq!(inspect(&product), AddonState::Current(VERSION.to_string()));
        assert_eq!(
            ensure_current(&product).unwrap(),
            AddonState::Current(VERSION.to_string())
        );

        // An older daemon's copy: stale by version, rewritten on start.
        std::fs::write(
            dir.join("wowdps.toc"),
            "## Interface: 120100\n## Version: 0.0.1\nwowdps.lua\n",
        )
        .unwrap();
        assert_eq!(inspect(&product), AddonState::Stale("0.0.1".to_string()));
        assert_eq!(
            ensure_current(&product).unwrap(),
            AddonState::Current(VERSION.to_string())
        );
        // A hand edit of the Lua: stale by content.
        std::fs::write(dir.join("wowdps.lua"), "-- edited\n").unwrap();
        assert_eq!(inspect(&product), AddonState::Stale(VERSION.to_string()));
        assert_eq!(
            ensure_current(&product).unwrap(),
            AddonState::Current(VERSION.to_string())
        );
        // The game updated: the interface number moved, the copy is stale.
        std::fs::write(
            wow.join(".build.info"),
            "Version!STRING:0|Product!STRING:0|Active!DEC:1\n12.2.0.70000|wow|1\n",
        )
        .unwrap();
        assert_eq!(inspect(&product), AddonState::Stale(VERSION.to_string()));
        ensure_current(&product).unwrap();
        assert!(
            std::fs::read_to_string(dir.join("wowdps.toc"))
                .unwrap()
                .starts_with("## Interface: 120200\n")
        );
        // A TOC with no version line reads `?`.
        std::fs::write(dir.join("wowdps.toc"), "## Interface: 1\n").unwrap();
        assert_eq!(inspect(&product), AddonState::Stale("?".to_string()));
        assert_eq!(AddonState::Missing.version(), None);
        assert_eq!(AddonState::Stale("x".into()).version(), Some("x"));

        // Without a readable build there is nothing to stamp: refused.
        std::fs::remove_file(wow.join(".build.info")).unwrap();
        assert!(install(&product).is_err());
        let _ = std::fs::remove_dir_all(wow.parent().unwrap());
    }

    #[test]
    fn saved_variables_are_found_per_account() {
        let wow = fake_install("sv", "12.1.0.69587");
        let product = wow.join("_retail_");
        assert!(saved_variables(&product).is_empty());
        let acct = |name: &str| product.join("WTF").join("Account").join(name);
        std::fs::create_dir_all(acct("ZED").join("SavedVariables")).unwrap();
        std::fs::write(
            acct("ZED").join("SavedVariables/wowdps.lua"),
            "WOWDPS_DATA = {}",
        )
        .unwrap();
        std::fs::create_dir_all(acct("ALPHA").join("SavedVariables")).unwrap();
        std::fs::write(acct("ALPHA").join("SavedVariables/wowdps.lua"), "").unwrap();
        // An account that never ran the addon, and a stray file, count for nothing.
        std::fs::create_dir_all(acct("NONE").join("SavedVariables")).unwrap();
        std::fs::write(acct("NONE").join("SavedVariables/Other.lua"), "").unwrap();
        std::fs::write(product.join("WTF").join("Account").join("file"), "").unwrap();
        let found = saved_variables(&product);
        assert_eq!(
            found,
            vec![
                (
                    "ALPHA".to_string(),
                    acct("ALPHA").join("SavedVariables/wowdps.lua")
                ),
                (
                    "ZED".to_string(),
                    acct("ZED").join("SavedVariables/wowdps.lua")
                ),
            ]
        );
        let _ = std::fs::remove_dir_all(wow.parent().unwrap());
    }
}
