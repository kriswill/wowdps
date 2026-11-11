//! The games' emblems for the window's edition switch (`gen-emblems`):
//! each game as the launcher's play selector shows it, read from the
//! launcher's own files on this machine into `edition-emblems.bin`, a
//! per-machine cache beside the class icons that gui-logic's `emblems`
//! reads. Never committed: the emblems are Blizzard's art.
//!
//! The launcher keeps what it downloads in a content-addressed cache,
//! `Cache/<h0h1>/<h2h3>/<h>` under its local data folder (`h` is the MD5
//! of the file's bytes, in lowercase hex). Its catalog for the game is one
//! of those files: a JSON object whose `fragment_id` is
//! `world_of_warcraft` and whose `files.default` names each of its
//! pictures by key, with the hash it is cached under. The play selector's
//! emblem of a game is the key `world_of_warcraft#WOW_HUB_PLAY_SELECTOR_ICON_`
//! followed by the game's word ([`GAMES`]): `MAINLINE` for Retail, whose
//! emblem is the current expansion's, and `FOREVER` for Forever. Both are
//! SVG documents, kept here as the launcher has them; the GUI rasterizes
//! each at the size it draws it. Where the launcher has kept several
//! catalogs (one per update it has seen), the newest file is read.
//!
//! File layout (all LE), read by gui-logic's `emblems` (a layout change
//! moves both and the version in both):
//!   "WDEM" | u32 version=1 | u32 n
//!   n × (u32 project_id, u32 len)   — sorted by project id
//!   the n SVG documents, back to back in the index's order

use std::path::{Path, PathBuf};
use std::time::SystemTime;

use wowdps_core::model::Edition;
use wowdps_proto::json::{self, Json};

/// The cache's name in the data home.
pub const FILE: &str = "edition-emblems.bin";
/// The cache's magic and layout version.
pub const MAGIC: &[u8; 4] = b"WDEM";
pub const VERSION: u32 = 1;

/// The catalog's key for a game's play-selector emblem, before the game's
/// word.
const KEY: &str = "world_of_warcraft#WOW_HUB_PLAY_SELECTOR_ICON_";
/// The catalog's `fragment_id`.
const FRAGMENT: &str = "world_of_warcraft";
/// The largest file read as a catalog: the game's is about 100 KiB.
const CATALOG_MAX: u64 = 8 << 20;
/// The largest emblem kept: the launcher's are a few KiB.
const SVG_MAX: u64 = 256 << 10;

/// Each game the switch shows and the play selector's word for it.
pub const GAMES: [(Edition, &str); 2] =
    [(Edition::Retail, "MAINLINE"), (Edition::Forever, "FOREVER")];

/// The launcher's local data folders on this machine, each holding its
/// `Cache/`: under the Wine prefix the install sits in (every user's
/// `AppData/Local/Battle.net`), and Windows' own `%LOCALAPPDATA%`. Each
/// once, however a symlink spells it.
pub fn launcher_dirs(install: Option<&Path>) -> Vec<PathBuf> {
    let mut found: Vec<PathBuf> = Vec::new();
    let mut add = |dir: PathBuf| {
        if dir.join("Cache").is_dir() {
            let dir = dir.canonicalize().unwrap_or(dir);
            if !found.contains(&dir) {
                found.push(dir);
            }
        }
    };
    if let Some(drive_c) = install.and_then(|i| i.ancestors().find(|p| p.ends_with("drive_c")))
        && let Ok(users) = std::fs::read_dir(drive_c.join("users"))
    {
        let mut users: Vec<PathBuf> = users.flatten().map(|e| e.path()).collect();
        users.sort();
        for user in users {
            add(user.join("AppData").join("Local").join("Battle.net"));
        }
    }
    if let Some(local) = std::env::var_os("LOCALAPPDATA") {
        add(PathBuf::from(local).join("Battle.net"));
    }
    found
}

/// The cache under a launcher folder: its `Cache/`, or the folder itself
/// when it is the cache.
pub fn cache_dir(launcher: &Path) -> PathBuf {
    let cache = launcher.join("Cache");
    if cache.is_dir() {
        cache
    } else {
        launcher.to_path_buf()
    }
}

/// One catalog of the game's in a launcher's cache: where it is, when the
/// launcher wrote it, and the hash each game's emblem is cached under.
#[derive(Debug, Clone, PartialEq)]
pub struct Catalog {
    pub path: PathBuf,
    pub modified: SystemTime,
    pub emblems: Vec<(Edition, String)>,
}

/// The emblems' hashes a catalog's text names, when it is the game's
/// catalog; `None` for any other file.
pub fn read_catalog(text: &str) -> Option<Vec<(Edition, String)>> {
    if !text.contains(KEY) {
        return None;
    }
    let doc = json::parse(text).ok()?;
    if doc.get("fragment_id").and_then(Json::as_str) != Some(FRAGMENT) {
        return None;
    }
    let files = doc.get("files")?.get("default")?;
    let emblems = GAMES
        .iter()
        .filter_map(|(edition, word)| {
            let hash = files.get(&format!("{KEY}{word}"))?.get("hash")?.as_str()?;
            is_hash(hash).then(|| (*edition, hash.to_string()))
        })
        .collect();
    Some(emblems)
}

/// Every catalog of the game's in a launcher's cache, its files read two
/// folders down; anything else there is passed over.
pub fn catalogs(cache: &Path) -> Vec<Catalog> {
    let mut found = Vec::new();
    for a in subdirs(cache) {
        for b in subdirs(&a) {
            let Ok(entries) = std::fs::read_dir(&b) else {
                continue;
            };
            for entry in entries.flatten() {
                let path = entry.path();
                let Ok(meta) = entry.metadata() else {
                    continue;
                };
                if !meta.is_file() || meta.len() > CATALOG_MAX || !starts_json(&path) {
                    continue;
                }
                let Ok(text) = std::fs::read_to_string(&path) else {
                    continue;
                };
                if let Some(emblems) = read_catalog(&text) {
                    let modified = meta.modified().unwrap_or(SystemTime::UNIX_EPOCH);
                    found.push(Catalog {
                        path,
                        modified,
                        emblems,
                    });
                }
            }
        }
    }
    found.sort_by(|x, y| x.path.cmp(&y.path));
    found
}

fn subdirs(dir: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut dirs: Vec<PathBuf> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_dir())
        .collect();
    dirs.sort();
    dirs
}

/// Whether a file's first byte opens a JSON object: the cheap test that
/// passes over the pictures before any is read whole.
fn starts_json(path: &Path) -> bool {
    use std::io::Read;
    let mut first = [0u8; 1];
    std::fs::File::open(path)
        .and_then(|mut f| f.read_exact(&mut first))
        .is_ok()
        && first == *b"{"
}

/// An MD5 in lowercase hex, as the cache names its files.
fn is_hash(s: &str) -> bool {
    s.len() == 32
        && s.bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// Where the cache keeps the file of `hash`.
pub fn blob_path(cache: &Path, hash: &str) -> Option<PathBuf> {
    if !is_hash(hash) {
        return None;
    }
    let (a, b) = (hash.get(..2)?, hash.get(2..4)?);
    Some(cache.join(a).join(b).join(hash))
}

/// Whether `bytes` are an SVG document: UTF-8 whose first element, past
/// an XML declaration, a BOM or comments, is `<svg`, closed by `</svg>`.
pub fn is_svg(bytes: &[u8]) -> bool {
    let Ok(text) = std::str::from_utf8(bytes) else {
        return false;
    };
    let mut rest = text.trim_start_matches('\u{feff}').trim_start();
    loop {
        if let Some(after) = rest.strip_prefix("<?") {
            let Some((_, tail)) = after.split_once("?>") else {
                return false;
            };
            rest = tail.trim_start();
        } else if let Some(after) = rest.strip_prefix("<!--") {
            let Some((_, tail)) = after.split_once("-->") else {
                return false;
            };
            rest = tail.trim_start();
        } else {
            break;
        }
    }
    rest.starts_with("<svg") && text.trim_end().ends_with("</svg>")
}

/// What a run found: each game's emblem it could read, the games it could
/// not, and the catalog it read them by.
#[derive(Debug)]
pub struct Emblems {
    pub found: Vec<(Edition, Vec<u8>)>,
    pub missing: Vec<Edition>,
    pub catalog: PathBuf,
}

/// The emblems of the newest catalog in any of the launcher folders: each
/// game's file, read from the cache that catalog is in and kept when it is
/// an SVG document. An error when no folder holds a catalog, or the
/// catalog leads to no emblem at all.
pub fn generate(launchers: &[PathBuf]) -> Result<Emblems, String> {
    let newest = launchers
        .iter()
        .flat_map(|l| {
            let cache = cache_dir(l);
            catalogs(&cache)
                .into_iter()
                .map(move |c| (cache.clone(), c))
        })
        .max_by(|(_, x), (_, y)| x.modified.cmp(&y.modified));
    let Some((cache, catalog)) = newest else {
        return Err(format!(
            "no catalog of the game's in the launcher's cache ({}): open the game's page \
             in the launcher once, or pass --launcher DIR (the folder holding its Cache/)",
            launchers
                .iter()
                .map(|p| p.display().to_string())
                .collect::<Vec<_>>()
                .join(", ")
        ));
    };
    let mut found = Vec::new();
    let mut missing = Vec::new();
    for (edition, _) in GAMES {
        let bytes = catalog
            .emblems
            .iter()
            .find(|(e, _)| *e == edition)
            .and_then(|(_, hash)| blob_path(&cache, hash))
            .filter(|p| std::fs::metadata(p).is_ok_and(|m| m.is_file() && m.len() <= SVG_MAX))
            .and_then(|p| std::fs::read(p).ok())
            .filter(|b| is_svg(b));
        match bytes {
            Some(b) => found.push((edition, b)),
            None => missing.push(edition),
        }
    }
    if found.is_empty() {
        return Err(format!(
            "{}: the launcher's catalog names no emblem this cache still holds",
            catalog.path.display()
        ));
    }
    Ok(Emblems {
        found,
        missing,
        catalog: catalog.path,
    })
}

/// The cache's bytes: the header, the index sorted by project id, then
/// each document.
pub fn encode(emblems: &[(Edition, Vec<u8>)]) -> Vec<u8> {
    let mut sorted: Vec<&(Edition, Vec<u8>)> = emblems.iter().collect();
    sorted.sort_by_key(|(e, _)| e.project_id());
    let mut out = Vec::new();
    out.extend_from_slice(MAGIC);
    out.extend_from_slice(&VERSION.to_le_bytes());
    out.extend_from_slice(&(sorted.len() as u32).to_le_bytes());
    for (edition, svg) in &sorted {
        out.extend_from_slice(&u32::from(edition.project_id()).to_le_bytes());
        out.extend_from_slice(&(svg.len() as u32).to_le_bytes());
    }
    for (_, svg) in &sorted {
        out.extend_from_slice(svg);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A drawn stand-in for an emblem: a disc, no game art.
    const DISC: &str = r##"<svg width="4" height="4" viewBox="0 0 4 4" xmlns="http://www.w3.org/2000/svg"><circle cx="2" cy="2" r="2" fill="#808080"/></svg>"##;
    const RING: &str = r##"<?xml version="1.0"?>
<svg width="4" height="4" viewBox="0 0 4 4" xmlns="http://www.w3.org/2000/svg"><circle cx="2" cy="2" r="1.5" stroke="#808080" fill="none"/></svg>
"##;
    const H_RETAIL: &str = "0123456789abcdef0123456789abcdef";
    const H_FOREVER: &str = "fedcba9876543210fedcba9876543210";
    const H_OLD: &str = "00000000000000000000000000000000";

    fn scratch(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("wowdps-emblems-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("the scratch dir");
        dir
    }

    fn put(cache: &Path, hash: &str, bytes: &[u8]) -> PathBuf {
        let path = blob_path(cache, hash).expect("a hash");
        std::fs::create_dir_all(path.parent().expect("a parent")).expect("the blob's dir");
        std::fs::write(&path, bytes).expect("the blob");
        path
    }

    /// A catalog naming `retail` and `forever` as the play selector's
    /// emblems, among other files, its keys sorted as the launcher's are.
    fn catalog(fragment: &str, retail: &str, forever: Option<&str>) -> String {
        let forever = forever
            .map(|h| {
                format!(
                    r#""world_of_warcraft#WOW_HUB_PLAY_SELECTOR_ICON_FOREVER":{{"hash":"{h}","name":"resources/f.svg"}},"#
                )
            })
            .unwrap_or_default();
        format!(
            r#"{{"categories":{{}},"files":{{"default":{{{forever}"world_of_warcraft#WOW_HUB_PLAY_SELECTOR_ICON_MAINLINE":{{"hash":"{retail}","name":"resources/m.svg"}},"world_of_warcraft#WOW_ICON_SMALL":{{"hash":"{H_OLD}","name":"resources/s.png"}}}}}},"fragment_id":"{fragment}","version":30}}"#
        )
    }

    fn age(path: &Path, secs: u64) {
        let at = SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1_700_000_000 + secs);
        std::fs::File::options()
            .write(true)
            .open(path)
            .and_then(|f| f.set_modified(at))
            .expect("the mtime");
    }

    #[test]
    fn a_catalog_names_each_game_s_emblem() {
        let got = read_catalog(&catalog(FRAGMENT, H_RETAIL, Some(H_FOREVER)));
        assert_eq!(
            got,
            Some(vec![
                (Edition::Retail, H_RETAIL.to_string()),
                (Edition::Forever, H_FOREVER.to_string())
            ])
        );
        // Another fragment, or no selector key, is not the game's catalog.
        assert_eq!(read_catalog(&catalog("overwatch", H_RETAIL, None)), None);
        assert_eq!(read_catalog(r#"{"fragment_id":"world_of_warcraft"}"#), None);
        assert_eq!(read_catalog("not json"), None);
        // A hash that is no MD5 names nothing.
        let bad = catalog(FRAGMENT, "../../etc/passwd0000000000000000", None);
        assert_eq!(read_catalog(&bad), Some(vec![]));
    }

    #[test]
    fn the_newest_catalog_leads_to_each_svg() {
        let launcher = scratch("newest");
        let cache = launcher.join("Cache");
        std::fs::create_dir_all(&cache).expect("the cache");
        let old = put(
            &cache,
            "1111aaaa1111aaaa1111aaaa1111aaaa",
            catalog(FRAGMENT, H_OLD, None).as_bytes(),
        );
        let new = put(
            &cache,
            "2222bbbb2222bbbb2222bbbb2222bbbb",
            catalog(FRAGMENT, H_RETAIL, Some(H_FOREVER)).as_bytes(),
        );
        age(&old, 10);
        age(&new, 20);
        put(&cache, H_OLD, b"<svg>old</svg>");
        put(&cache, H_RETAIL, DISC.as_bytes());
        put(&cache, H_FOREVER, RING.as_bytes());
        // A picture and some other JSON beside them are passed over.
        put(&cache, "3333cccc3333cccc3333cccc3333cccc", b"\x89PNG\r\n");
        put(
            &cache,
            "4444dddd4444dddd4444dddd4444dddd",
            br#"{"fragment_id":"x"}"#,
        );

        assert_eq!(catalogs(&cache).len(), 2);
        let g = generate(std::slice::from_ref(&launcher)).expect("emblems");
        assert_eq!(g.catalog, new);
        assert_eq!(
            g.found,
            vec![
                (Edition::Retail, DISC.as_bytes().to_vec()),
                (Edition::Forever, RING.as_bytes().to_vec())
            ]
        );
        assert!(g.missing.is_empty());
        let _ = std::fs::remove_dir_all(&launcher);
    }

    /// A game whose emblem the cache no longer holds, or holds as no SVG,
    /// is missing, and the others are kept; none at all is an error, and so
    /// is a cache with no catalog.
    #[test]
    fn a_missing_emblem_is_left_out() {
        let launcher = scratch("missing");
        let cache = launcher.join("Cache");
        std::fs::create_dir_all(&cache).expect("the cache");
        put(
            &cache,
            "2222bbbb2222bbbb2222bbbb2222bbbb",
            catalog(FRAGMENT, H_RETAIL, Some(H_FOREVER)).as_bytes(),
        );
        put(&cache, H_RETAIL, DISC.as_bytes());
        let g = generate(std::slice::from_ref(&launcher)).expect("Retail's");
        assert_eq!(g.found.len(), 1);
        assert_eq!(g.missing, vec![Edition::Forever]);

        put(&cache, H_FOREVER, b"\x89PNG not an svg");
        let g = generate(std::slice::from_ref(&launcher)).expect("Retail's");
        assert_eq!(g.missing, vec![Edition::Forever]);

        std::fs::remove_file(blob_path(&cache, H_RETAIL).expect("a hash")).expect("removed");
        assert!(generate(std::slice::from_ref(&launcher)).is_err());

        let empty = scratch("empty");
        let err = generate(std::slice::from_ref(&empty)).expect_err("no catalog");
        assert!(err.contains("--launcher"), "{err}");
        let _ = std::fs::remove_dir_all(&launcher);
        let _ = std::fs::remove_dir_all(&empty);
    }

    /// Under a Wine prefix the launcher's folder is found from the
    /// install's, once for a user that is a symlink to another.
    #[test]
    fn the_launcher_is_found_beside_the_install() {
        let root = scratch("prefix");
        let drive_c = root.join("pfx").join("drive_c");
        let install = drive_c
            .join("Program Files (x86)")
            .join("World of Warcraft");
        std::fs::create_dir_all(&install).expect("the install");
        let user = drive_c.join("users").join("player");
        let launcher = user.join("AppData").join("Local").join("Battle.net");
        std::fs::create_dir_all(launcher.join("Cache")).expect("the cache");
        #[cfg(unix)]
        std::os::unix::fs::symlink(&user, drive_c.join("users").join("alias")).expect("a symlink");
        let found = launcher_dirs(Some(&install));
        let found: Vec<&PathBuf> = found
            .iter()
            .filter(|p| p.starts_with(root.canonicalize().expect("the root")))
            .collect();
        assert_eq!(found, vec![&launcher.canonicalize().expect("the launcher")]);
        assert!(cache_dir(&launcher).ends_with("Cache"));
        assert_eq!(cache_dir(&launcher.join("Cache")), launcher.join("Cache"));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn an_svg_is_known_by_its_first_element() {
        assert!(is_svg(DISC.as_bytes()));
        assert!(is_svg(RING.as_bytes()));
        assert!(is_svg(
            "\u{feff}<!-- made by hand --><svg></svg>".as_bytes()
        ));
        assert!(!is_svg(b"<html><svg></svg></html>"));
        assert!(!is_svg(b"<svg>"));
        assert!(!is_svg(b"<?xml unterminated <svg></svg>"));
        assert!(!is_svg(&[0xff, 0xfe, b'<']));
    }

    /// The layout gui-logic's reader reads: the header, the index by
    /// project id, the documents in its order.
    #[test]
    fn the_cache_is_laid_out_by_project_id() {
        let bytes = encode(&[
            (Edition::Forever, b"<svg>f</svg>".to_vec()),
            (Edition::Retail, b"<svg/>".to_vec()),
        ]);
        let u32_at = |at: usize| u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap());
        assert_eq!(&bytes[..4], MAGIC);
        assert_eq!(u32_at(4), VERSION);
        assert_eq!(u32_at(8), 2);
        assert_eq!((u32_at(12), u32_at(16)), (1, 6));
        assert_eq!((u32_at(20), u32_at(24)), (18, 12));
        assert_eq!(&bytes[28..], b"<svg/><svg>f</svg>");
    }
}
