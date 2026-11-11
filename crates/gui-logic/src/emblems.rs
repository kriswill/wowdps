//! The games' emblem cache reader: each game's emblem as the launcher's
//! play selector shows it, from the per-machine cache
//! `$XDG_DATA_HOME/wowdps/edition-emblems.bin` (written by
//! `tools/gen-emblems.sh` from the launcher's own files). Entirely
//! optional: no file, or a file from a future format version, means every
//! lookup answers `None` and the edition switch names each game alone.
//! The emblems are Blizzard's art and never live in the repository.
//!
//! Each emblem is the SVG document the launcher keeps, as it is: the GUI
//! rasterizes it at the size it draws it. The file is a few KiB, read
//! whole once.
//!
//! File layout (all LE), written by `tools/extract/src/emblemgen.rs`:
//!   "WDEM" | u32 version=1 | u32 n
//!   n × (u32 project_id, u32 len)   — sorted by project id
//!   the n SVG documents, back to back in the index's order

use std::path::{Path, PathBuf};

use wowdps_model::Edition;

use crate::lazy_tiles::le_u32;

/// The cache's name in the data home.
pub const FILE: &str = "edition-emblems.bin";
/// The largest file read: the launcher's emblems are a few KiB each.
const MAX_FILE: u64 = 4 << 20;
/// The most games a file indexes.
const MAX_GAMES: u32 = 64;

/// Every emblem the cache holds, by the edition's project id.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Emblems {
    svgs: Vec<(u8, Vec<u8>)>,
}

impl Emblems {
    /// The per-machine cache, or `None` without one.
    pub fn open() -> Option<Self> {
        open_at(&cache_path()?)
    }

    /// The SVG document of `edition`'s emblem: the one of the edition it
    /// counts as, so a fight that named no edition wears Retail's.
    pub fn svg(&self, edition: Edition) -> Option<&[u8]> {
        let id = edition.canonical().counts_as().project_id();
        self.svgs
            .iter()
            .find(|(p, _)| *p == id)
            .map(|(_, svg)| svg.as_slice())
    }
}

fn cache_path() -> Option<PathBuf> {
    wowdps_proto::talents::data_path(FILE)
}

/// The whole reader behind a path parameter, so tests can point it at
/// absent, truncated or garbage files without touching the real cache.
fn open_at(path: &Path) -> Option<Emblems> {
    let meta = std::fs::metadata(path).ok()?;
    if !meta.is_file() || meta.len() > MAX_FILE {
        return None;
    }
    parse(&std::fs::read(path).ok()?)
}

/// The cache's bytes read: `None` for a wrong magic, a version this build
/// does not know, an index past the file's end or documents that overrun
/// it, never a panic or an allocation off a lying header.
pub fn parse(bytes: &[u8]) -> Option<Emblems> {
    if bytes.get(..4) != Some(b"WDEM") || le_u32(bytes, 4) != 1 {
        return None;
    }
    let n = le_u32(bytes, 8);
    if n > MAX_GAMES {
        return None;
    }
    let index_end = 12usize.checked_add(n as usize * 8)?;
    if bytes.len() < index_end {
        return None;
    }
    let mut at = index_end;
    let mut svgs = Vec::with_capacity(n as usize);
    for i in 0..n as usize {
        let id = u8::try_from(le_u32(bytes, 12 + i * 8)).ok()?;
        let len = le_u32(bytes, 16 + i * 8) as usize;
        let end = at.checked_add(len)?;
        svgs.push((id, bytes.get(at..end)?.to_vec()));
        at = end;
    }
    Some(Emblems { svgs })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn word(v: u32) -> [u8; 4] {
        v.to_le_bytes()
    }

    /// Retail's and Forever's, as the generator lays them out.
    fn valid() -> Vec<u8> {
        let mut b = Vec::new();
        b.extend_from_slice(b"WDEM");
        b.extend_from_slice(&word(1));
        b.extend_from_slice(&word(2));
        b.extend_from_slice(&word(1));
        b.extend_from_slice(&word(6));
        b.extend_from_slice(&word(18));
        b.extend_from_slice(&word(12));
        b.extend_from_slice(b"<svg/><svg>f</svg>");
        b
    }

    #[test]
    fn each_game_has_its_own_emblem() {
        let e = parse(&valid()).expect("a valid cache");
        assert_eq!(e.svg(Edition::Retail), Some(&b"<svg/>"[..]));
        assert_eq!(e.svg(Edition::Forever), Some(&b"<svg>f</svg>"[..]));
        // A fight that named no edition counts as Retail and wears its.
        assert_eq!(e.svg(Edition::Unknown(0)), Some(&b"<svg/>"[..]));
        // A game the cache has no emblem for has none.
        assert_eq!(e.svg(Edition::Classic(2)), None);
        assert_eq!(e.svg(Edition::Unknown(40)), None);
    }

    #[test]
    fn absent_truncated_or_garbage_reads_as_none() {
        let absent = std::env::temp_dir().join(format!(
            "wowdps-emblems-test-{}-absent.bin",
            std::process::id()
        ));
        assert!(open_at(&absent).is_none());
        assert!(parse(&[]).is_none());
        assert!(parse(&[0xDE, 0xAD, 0xBE, 0xEF, 0, 1]).is_none());
        let good = valid();
        for cut in 0..good.len() {
            assert!(parse(&good[..cut]).is_none(), "cut at {cut}");
        }
        // A future version, and an index that claims more games than any
        // file holds, read as none.
        let mut future = good.clone();
        future[4] = 2;
        assert!(parse(&future).is_none());
        let mut lying = good.clone();
        lying[8..12].copy_from_slice(&word(u32::MAX));
        assert!(parse(&lying).is_none());
        // A project id past a byte is no edition's.
        let mut wide = good;
        wide[12..16].copy_from_slice(&word(256));
        assert!(parse(&wide).is_none());
    }

    #[test]
    fn a_file_on_disk_reads_whole() {
        let path = std::env::temp_dir().join(format!(
            "wowdps-emblems-test-{}-valid.bin",
            std::process::id()
        ));
        std::fs::write(&path, valid()).expect("the temp file");
        let e = open_at(&path);
        let _ = std::fs::remove_file(&path);
        assert_eq!(e, parse(&valid()));
        assert!(e.is_some());
    }
}
