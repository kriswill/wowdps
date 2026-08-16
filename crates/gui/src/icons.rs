//! The class/spec icon cache reader: the game's own class crests and spec
//! icons from the per-machine cache `$XDG_DATA_HOME/wowdps/class-icons.bin`
//! (written by `tools/gen-icons.sh`). Entirely optional — no file, or a file
//! from a future format version, means every lookup answers `None` and the
//! UI falls back to the drawn class-colored discs. Extracted Blizzard art
//! therefore never has to live in the repository.
//!
//! File layout (all LE), written by `tools/extract/src/icongen.rs`:
//!   "WDCI" | u32 version=1 | u32 icon_px | u32 n_class | u32 n_spec
//!   n_spec × (u32 spec_id, u32 tile_index)   — sorted by spec_id
//!   tiles: n_class crests first, in `Class` code order (a crest's tile
//!   index IS its class code), then the spec tiles the index points at.
//!
//! Unlike the 58 MiB spell-icon cache (read tile-by-tile on demand), this
//! file is ~200 KiB: it is read whole on first use and handles are built
//! lazily from the in-memory copy.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use iced::widget::image::Handle;
use wowdps_model::Class;

struct Cache {
    icon_px: u32,
    n_class: u32,
    /// (spec id, tile index), sorted by spec id.
    index: Vec<(u32, u32)>,
    tiles: Vec<u8>,
    handles: Mutex<HashMap<u32, Handle>>,
}

fn cache_path() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")))?;
    Some(base.join("wowdps/class-icons.bin"))
}

fn open() -> Option<Cache> {
    open_at(&cache_path()?)
}

/// The whole reader behind a path parameter, so tests can point it at absent,
/// truncated or garbage files without touching the real per-machine cache.
fn open_at(path: &Path) -> Option<Cache> {
    let bytes = std::fs::read(path).ok()?;
    if bytes.get(..4) != Some(b"WDCI") {
        return None;
    }
    let at = |b: &[u8], i: usize| b.get(i).copied().unwrap_or(0);
    let word = |i: usize| -> u32 {
        u32::from_le_bytes([
            at(&bytes, i),
            at(&bytes, i + 1),
            at(&bytes, i + 2),
            at(&bytes, i + 3),
        ])
    };
    if word(4) != 1 {
        return None; // future format: draw discs rather than garbage
    }
    let icon_px = word(8);
    let n_class = word(12);
    let n_spec = word(16) as usize;
    if icon_px == 0 || icon_px > 256 || n_class > 64 || n_spec > 1024 {
        return None;
    }
    let index: Vec<(u32, u32)> = (0..n_spec)
        .map(|i| (word(20 + i * 8), word(24 + i * 8)))
        .collect();
    let tiles = bytes.get(20 + n_spec * 8..)?.to_vec();
    Some(Cache {
        icon_px,
        n_class,
        index,
        tiles,
        handles: Mutex::new(HashMap::new()),
    })
}

fn cache() -> Option<&'static Cache> {
    static CACHE: OnceLock<Option<Cache>> = OnceLock::new();
    CACHE.get_or_init(open).as_ref()
}

impl Cache {
    fn handle(&self, tile: u32) -> Option<Handle> {
        let size = (self.icon_px * self.icon_px * 4) as usize;
        let rgba = self
            .tiles
            .get(tile as usize * size..(tile as usize + 1) * size)?;
        let mut handles = self.handles.lock().unwrap_or_else(|e| e.into_inner());
        Some(
            handles
                .entry(tile)
                .or_insert_with(|| Handle::from_rgba(self.icon_px, self.icon_px, rgba.to_vec()))
                .clone(),
        )
    }
}

/// Tile index of a class crest — by construction, its `Class` code (the
/// generator writes crests in this exact order).
fn class_code(class: Class) -> u32 {
    match class {
        Class::Warrior => 0,
        Class::Paladin => 1,
        Class::Hunter => 2,
        Class::Rogue => 3,
        Class::Priest => 4,
        Class::DeathKnight => 5,
        Class::Shaman => 6,
        Class::Mage => 7,
        Class::Warlock => 8,
        Class::Monk => 9,
        Class::Druid => 10,
        Class::DemonHunter => 11,
        Class::Evoker => 12,
    }
}

/// The class crest (interface/icons/classicon_*), or `None` without a cache.
pub(crate) fn class_handle(class: Class) -> Option<Handle> {
    let c = cache()?;
    let code = class_code(class);
    if code >= c.n_class {
        return None;
    }
    c.handle(code)
}

/// The spec's own icon, by Blizzard specID (ChrSpecialization).
pub(crate) fn spec_handle(spec_id: u32) -> Option<Handle> {
    let c = cache()?;
    let i = c.index.binary_search_by_key(&spec_id, |e| e.0).ok()?;
    c.handle(c.index.get(i)?.1)
}

#[cfg(test)]
mod tests {
    //! The cache is per-machine and entirely optional: a machine without it
    //! (or with a half-written or corrupt one) must render fine, so absent,
    //! truncated and garbage files all have to answer `None` — never panic,
    //! never allocate off a lying header. Everything goes through the
    //! `open_at` seam against temp files; the real cache is never touched.

    use super::*;

    /// A temp file that cleans up after itself even when the test passes on
    /// an earlier return path. Name carries the pid so parallel test
    /// processes never collide.
    struct TempFile(PathBuf);

    impl TempFile {
        fn new(name: &str, bytes: &[u8]) -> Self {
            let path = std::env::temp_dir().join(format!(
                "wowdps-icons-test-{}-{name}.bin",
                std::process::id()
            ));
            std::fs::write(&path, bytes).expect("temp file write");
            Self(path)
        }
    }

    impl Drop for TempFile {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.0);
        }
    }

    fn word(v: u32) -> [u8; 4] {
        v.to_le_bytes()
    }

    /// A minimal valid cache: 2×2 px, one class crest, one spec entry
    /// (spec 62 → tile 1), both tiles present (16 RGBA bytes each).
    fn valid_bytes() -> Vec<u8> {
        let mut b = Vec::new();
        b.extend_from_slice(b"WDCI");
        b.extend_from_slice(&word(1)); // version
        b.extend_from_slice(&word(2)); // icon_px
        b.extend_from_slice(&word(1)); // n_class
        b.extend_from_slice(&word(1)); // n_spec
        b.extend_from_slice(&word(62)); // spec id
        b.extend_from_slice(&word(1)); // tile index
        b.extend_from_slice(&[0xAB; 32]); // 2 tiles × 16 bytes
        b
    }

    #[test]
    fn an_absent_cache_file_opens_as_none() {
        let path = std::env::temp_dir().join(format!(
            "wowdps-icons-test-{}-definitely-absent.bin",
            std::process::id()
        ));
        assert!(open_at(&path).is_none());
    }

    #[test]
    fn garbage_bytes_open_as_none() {
        // Wrong magic entirely.
        let f = TempFile::new("garbage", &[0xDE, 0xAD, 0xBE, 0xEF, 0x00, 0x01]);
        assert!(open_at(&f.0).is_none());
        // Deterministic pseudo-noise, no seed from the environment.
        let noise: Vec<u8> = (0u32..512)
            .map(|i| (i.wrapping_mul(2654435761) >> 24) as u8)
            .collect();
        let f = TempFile::new("noise", &noise);
        assert!(open_at(&f.0).is_none());
    }

    #[test]
    fn a_header_truncated_after_the_magic_opens_as_none() {
        // Only the magic: the padded version word reads 0, not 1.
        let f = TempFile::new("magic-only", b"WDCI");
        assert!(open_at(&f.0).is_none());
        // Magic + version but nothing else: icon_px pads to 0, rejected.
        let mut b = b"WDCI".to_vec();
        b.extend_from_slice(&word(1));
        let f = TempFile::new("no-dims", &b);
        assert!(open_at(&f.0).is_none());
    }

    #[test]
    fn a_file_truncated_mid_index_opens_as_none_not_a_bogus_allocation() {
        let full = valid_bytes();
        // Cut inside the spec index (header is 20 bytes, index 8 more).
        let f = TempFile::new("mid-index", &full[..24]);
        assert!(open_at(&f.0).is_none());
    }

    #[test]
    fn a_lying_header_count_is_rejected_not_allocated() {
        // n_spec far over the sanity cap: must be refused outright.
        let mut b = Vec::new();
        b.extend_from_slice(b"WDCI");
        b.extend_from_slice(&word(1));
        b.extend_from_slice(&word(2));
        b.extend_from_slice(&word(1));
        b.extend_from_slice(&word(u32::MAX)); // n_spec lie
        let f = TempFile::new("count-lie", &b);
        assert!(open_at(&f.0).is_none());
    }

    #[test]
    fn a_file_truncated_mid_tile_yields_no_handle_and_no_panic() {
        let full = valid_bytes();
        // Keep the header + index + 3 bytes of the first 16-byte tile.
        let f = TempFile::new("mid-tile", &full[..28 + 3]);
        let c = open_at(&f.0).expect("header and index are intact");
        // Neither tile is complete: lookups answer None instead of slicing
        // out of bounds.
        assert!(c.handle(0).is_none());
        assert!(c.handle(1).is_none());
    }

    #[test]
    fn a_complete_cache_serves_both_crest_and_spec_tiles() {
        let f = TempFile::new("valid", &valid_bytes());
        let c = open_at(&f.0).expect("valid cache");
        assert_eq!(c.icon_px, 2);
        assert_eq!(c.n_class, 1);
        assert_eq!(c.index, vec![(62, 1)]);
        assert!(c.handle(0).is_some(), "class crest tile");
        assert!(c.handle(1).is_some(), "spec icon tile");
        assert!(c.handle(2).is_none(), "past the last tile");
    }
}
