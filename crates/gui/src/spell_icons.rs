//! The runtime spell-icon cache reader: ability icons for the ids that
//! by-spell rows carry (`Row::spell_id`, wire v9), lazily read from the
//! per-machine cache `$XDG_DATA_HOME/wowdps/spell-icons.bin` written by
//! `tools/gen-spell-icons.sh`.
//!
//! Entirely optional: no file (or a file from a future format version) means
//! `handle()` always answers `None` and the UI simply draws no ability
//! icons. The index (~3 MiB) loads once on first use; tiles are read from
//! the file on demand and their iced handles cached, so a scroll through a
//! long spell table costs one 4 KiB read per *distinct* spell, ever.
//!
//! File layout (all LE), written by `tools/extract/src/spellicongen.rs`:
//!   "WDPI" | u32 version=1 | u32 icon_px | u32 n_spells | u32 n_tiles
//!   n_spells × (u32 spell_id, u32 tile_index)   — sorted by spell_id
//!   n_tiles  × (icon_px² × 4 bytes RGBA)

use std::collections::HashMap;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use iced::widget::image::Handle;

struct Cache {
    /// (spell id, tile index), sorted by spell id.
    index: Vec<(u32, u32)>,
    file: Mutex<File>,
    tiles_at: u64,
    icon_px: u32,
    tile_bytes: usize,
    handles: Mutex<HashMap<u32, Option<Handle>>>,
}

fn cache_path() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")))?;
    Some(base.join("wowdps/spell-icons.bin"))
}

fn open() -> Option<Cache> {
    open_at(&cache_path()?)
}

/// The whole reader behind a path parameter, so tests can point it at absent,
/// truncated or garbage files without touching the real per-machine cache.
fn open_at(path: &Path) -> Option<Cache> {
    let mut file = File::open(path).ok()?;
    let mut head = [0u8; 20];
    file.read_exact(&mut head).ok()?;
    if head.get(..4) != Some(b"WDPI") {
        return None;
    }
    let at = |b: &[u8], i: usize| b.get(i).copied().unwrap_or(0);
    let word = |i: usize| -> u32 {
        u32::from_le_bytes([
            at(&head, i),
            at(&head, i + 1),
            at(&head, i + 2),
            at(&head, i + 3),
        ])
    };
    if word(4) != 1 {
        return None; // future format: draw nothing rather than garbage
    }
    let icon_px = word(8);
    let n_spells = word(12) as usize;
    if icon_px == 0 || icon_px > 256 || n_spells > 4_000_000 {
        return None;
    }
    let mut raw = vec![0u8; n_spells * 8];
    file.read_exact(&mut raw).ok()?;
    let index: Vec<(u32, u32)> = raw
        .chunks_exact(8)
        .map(|c| {
            (
                u32::from_le_bytes([at(c, 0), at(c, 1), at(c, 2), at(c, 3)]),
                u32::from_le_bytes([at(c, 4), at(c, 5), at(c, 6), at(c, 7)]),
            )
        })
        .collect();
    Some(Cache {
        tiles_at: 20 + index.len() as u64 * 8,
        index,
        file: Mutex::new(file),
        icon_px,
        tile_bytes: (icon_px * icon_px * 4) as usize,
        handles: Mutex::new(HashMap::new()),
    })
}

fn cache() -> Option<&'static Cache> {
    static CACHE: OnceLock<Option<Cache>> = OnceLock::new();
    CACHE.get_or_init(open).as_ref()
}

impl Cache {
    /// The lookup behind `handle()`, on the cache itself so tests can drive
    /// it against a temp file. A tile the file cannot actually serve (short
    /// read past a truncation) caches as `None` — asked once, failed forever,
    /// never a panic.
    fn lookup(&self, spell_id: u32) -> Option<Handle> {
        let i = self.index.binary_search_by_key(&spell_id, |e| e.0).ok()?;
        let tile = self.index.get(i)?.1;
        let mut handles = self.handles.lock().unwrap_or_else(|e| e.into_inner());
        handles
            .entry(tile)
            .or_insert_with(|| {
                let mut buf = vec![0u8; self.tile_bytes];
                let mut file = self.file.lock().unwrap_or_else(|e| e.into_inner());
                file.seek(SeekFrom::Start(
                    self.tiles_at + tile as u64 * self.tile_bytes as u64,
                ))
                .ok()
                .and_then(|_| file.read_exact(&mut buf).ok())
                .map(|()| Handle::from_rgba(self.icon_px, self.icon_px, buf))
            })
            .clone()
    }
}

/// The icon for a spell id, or `None` (no cache, unknown spell, short read).
/// Handles are cached; cloning one is cheap.
pub(crate) fn handle(spell_id: u32) -> Option<Handle> {
    if spell_id == 0 {
        return None;
    }
    cache()?.lookup(spell_id)
}

#[cfg(test)]
mod tests {
    //! Like `icons.rs`: the 58 MiB cache is per-machine and optional, and a
    //! half-generated or corrupt file must degrade to "no icons" — `None`
    //! from every lookup, no panic, no allocation sized by a lying header.
    //! Everything goes through the `open_at` seam against temp files.

    use super::*;

    struct TempFile(PathBuf);

    impl TempFile {
        fn new(name: &str, bytes: &[u8]) -> Self {
            let path = std::env::temp_dir().join(format!(
                "wowdps-spell-icons-test-{}-{name}.bin",
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

    /// A minimal valid cache: 2×2 px, two spells sharing detail — spell 100
    /// on tile 0, spell 200 on tile 1 — and both 16-byte tiles present.
    fn valid_bytes() -> Vec<u8> {
        let mut b = Vec::new();
        b.extend_from_slice(b"WDPI");
        b.extend_from_slice(&word(1)); // version
        b.extend_from_slice(&word(2)); // icon_px
        b.extend_from_slice(&word(2)); // n_spells
        b.extend_from_slice(&word(2)); // n_tiles
        b.extend_from_slice(&word(100)); // spell 100 -> tile 0
        b.extend_from_slice(&word(0));
        b.extend_from_slice(&word(200)); // spell 200 -> tile 1
        b.extend_from_slice(&word(1));
        b.extend_from_slice(&[0xCD; 32]); // 2 tiles × 16 bytes
        b
    }

    #[test]
    fn an_absent_cache_file_opens_as_none() {
        let path = std::env::temp_dir().join(format!(
            "wowdps-spell-icons-test-{}-definitely-absent.bin",
            std::process::id()
        ));
        assert!(open_at(&path).is_none());
    }

    #[test]
    fn garbage_bytes_open_as_none() {
        let f = TempFile::new("garbage", &[0xDE, 0xAD, 0xBE, 0xEF, 0x00, 0x01, 0x02, 0x03]);
        assert!(open_at(&f.0).is_none());
        // Deterministic pseudo-noise, no seed from the environment.
        let noise: Vec<u8> = (0u32..512)
            .map(|i| (i.wrapping_mul(2654435761) >> 24) as u8)
            .collect();
        let f = TempFile::new("noise", &noise);
        assert!(open_at(&f.0).is_none());
    }

    #[test]
    fn a_truncated_header_opens_as_none() {
        // Shorter than the 20-byte header: the read_exact fails cleanly.
        let f = TempFile::new("short-header", &valid_bytes()[..10]);
        assert!(open_at(&f.0).is_none());
    }

    #[test]
    fn a_file_truncated_mid_index_opens_as_none() {
        // Header promises 2 index entries (16 bytes); provide 5.
        let f = TempFile::new("mid-index", &valid_bytes()[..25]);
        assert!(open_at(&f.0).is_none());
    }

    #[test]
    fn a_lying_spell_count_is_rejected_not_allocated() {
        // n_spells over the 4M sanity cap: refused before any index read.
        let mut b = Vec::new();
        b.extend_from_slice(b"WDPI");
        b.extend_from_slice(&word(1));
        b.extend_from_slice(&word(2));
        b.extend_from_slice(&word(u32::MAX)); // n_spells lie
        b.extend_from_slice(&word(1));
        let f = TempFile::new("count-lie", &b);
        assert!(open_at(&f.0).is_none());
    }

    #[test]
    fn a_file_truncated_mid_tile_yields_no_handle_and_no_panic() {
        let full = valid_bytes();
        // Header (20) + index (16) + 3 bytes of the first tile.
        let f = TempFile::new("mid-tile", &full[..36 + 3]);
        let c = open_at(&f.0).expect("header and index are intact");
        assert!(c.lookup(100).is_none(), "tile 0 short read");
        assert!(c.lookup(200).is_none(), "tile 1 entirely absent");
        // The failed tile is cached as None: asking again is still clean.
        assert!(c.lookup(100).is_none());
    }

    #[test]
    fn a_complete_cache_serves_tiles_and_unknown_spells_answer_none() {
        let f = TempFile::new("valid", &valid_bytes());
        let c = open_at(&f.0).expect("valid cache");
        assert!(c.lookup(100).is_some());
        assert!(c.lookup(200).is_some());
        assert!(c.lookup(999).is_none(), "unknown spell id");
    }
}
