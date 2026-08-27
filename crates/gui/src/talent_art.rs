//! The talent-art cache reader: the game's own talent-UI artwork — per-spec
//! pane background paintings, hero-tree medallions, and the golden medallion
//! ring — lazily read from the per-machine cache
//! `$XDG_DATA_HOME/wowdps/talent-art.bin` written by
//! `tools/gen-talent-art.sh`.
//!
//! Entirely optional, like the icon caches: no file (or a future version)
//! means every lookup answers `None` and the talent viewer draws plain
//! panels. Tiles are large (a background is ~1.2 MiB), so this follows
//! `spell_icons.rs`: a small index up front, one seek-and-read per distinct
//! tile, handles memoized.
//!
//! File layout (all LE), written by `tools/extract/src/artgen.rs`:
//!   "WDTA" | u32 version=1 | u32 count
//!   count × (u8 kind, u32 id, u16 w, u16 h, u64 offset)   sorted (kind, id)
//!   RGBA tiles at the recorded offsets (from file start)

use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use iced::widget::image::Handle;

use crate::lazy_tiles::{Tiles, le_u32};

const KIND_BACKGROUND: u8 = 0;
const KIND_MEDALLION: u8 = 1;
const KIND_CHROME: u8 = 2;
const CHROME_RING: u32 = 0;

struct Entry {
    kind: u8,
    id: u32,
    w: u16,
    h: u16,
    offset: u64,
}

struct Cache {
    /// Sorted by (kind, id).
    index: Vec<Entry>,
    tiles: Tiles<(u8, u32)>,
}

fn cache_path() -> Option<PathBuf> {
    wowdps_proto::talents::data_path("talent-art.bin")
}

fn open() -> Option<Cache> {
    open_at(&cache_path()?)
}

/// The whole reader behind a path parameter, so tests can point it at
/// absent, truncated or garbage files without the real per-machine cache.
fn open_at(path: &Path) -> Option<Cache> {
    let mut file = File::open(path).ok()?;
    let mut head = [0u8; 12];
    file.read_exact(&mut head).ok()?;
    if head.get(..4) != Some(b"WDTA") {
        return None;
    }
    if le_u32(&head, 4) != 1 {
        return None; // future format: draw nothing rather than garbage
    }
    let count = le_u32(&head, 8) as usize;
    if count > 4096 {
        return None;
    }
    let mut raw = vec![0u8; count * 17];
    file.read_exact(&mut raw).ok()?;
    let mut index = Vec::with_capacity(count);
    for &[
        k,
        i0,
        i1,
        i2,
        i3,
        w0,
        w1,
        h0,
        h1,
        o0,
        o1,
        o2,
        o3,
        o4,
        o5,
        o6,
        o7,
    ] in raw.as_chunks::<17>().0
    {
        let entry = Entry {
            kind: k,
            id: u32::from_le_bytes([i0, i1, i2, i3]),
            w: u16::from_le_bytes([w0, w1]),
            h: u16::from_le_bytes([h0, h1]),
            offset: u64::from_le_bytes([o0, o1, o2, o3, o4, o5, o6, o7]),
        };
        if entry.w == 0 || entry.h == 0 || entry.w > 4096 || entry.h > 4096 {
            return None;
        }
        index.push(entry);
    }
    Some(Cache {
        index,
        tiles: Tiles::new(file),
    })
}

fn cache() -> Option<&'static Cache> {
    static CACHE: OnceLock<Option<Cache>> = OnceLock::new();
    CACHE.get_or_init(open).as_ref()
}

impl Cache {
    /// A tile the file cannot serve (short read past a truncation) caches
    /// as `None` — asked once, failed forever, never a panic. Returns the
    /// handle with the tile's pixel size.
    fn lookup(&self, kind: u8, id: u32) -> Option<(Handle, u16, u16)> {
        let i = self
            .index
            .binary_search_by_key(&(kind, id), |e| (e.kind, e.id))
            .ok()?;
        let e = self.index.get(i)?;
        let (w, h, offset) = (e.w, e.h, e.offset);
        let bytes = w as usize * h as usize * 4;
        self.tiles
            .lookup((kind, id), offset, bytes, |buf| {
                Handle::from_rgba(u32::from(w), u32::from(h), buf)
            })
            .map(|handle| (handle, w, h))
    }
}

/// The spec's whole background painting with its pixel size: class art on
/// its left half, spec art on its right. Drawn as one full-width backdrop
/// under the trees (the caller needs the aspect for cover-fitting).
pub(crate) fn background(spec_id: u32) -> Option<(Handle, u16, u16)> {
    cache()?.lookup(KIND_BACKGROUND, spec_id)
}

/// A hero tree's round medallion.
pub(crate) fn medallion(subtree_id: u32) -> Option<Handle> {
    cache()?
        .lookup(KIND_MEDALLION, subtree_id)
        .map(|(h, _, _)| h)
}

/// The golden ring the game frames the medallion with.
pub(crate) fn ring() -> Option<Handle> {
    cache()?.lookup(KIND_CHROME, CHROME_RING).map(|(h, _, _)| h)
}

#[cfg(test)]
mod tests {
    //! Same posture as the icon caches: absent/garbage/truncated files
    //! degrade to `None` everywhere — no panic, no lying-header allocation.

    use super::*;

    struct TempFile(PathBuf);

    impl TempFile {
        fn new(name: &str, bytes: &[u8]) -> Self {
            let path = std::env::temp_dir().join(format!(
                "wowdps-talent-art-test-{}-{name}.bin",
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

    /// Two entries: a 2×1 background for spec 266 and a 1×1 medallion for
    /// subtree 59.
    fn valid_bytes() -> Vec<u8> {
        let mut b = Vec::new();
        b.extend_from_slice(b"WDTA");
        b.extend_from_slice(&1u32.to_le_bytes());
        b.extend_from_slice(&2u32.to_le_bytes());
        let payload_at = 12 + 2 * 17;
        for (kind, id, w, h, off) in [
            (0u8, 266u32, 2u16, 1u16, payload_at as u64),
            (1, 59, 1, 1, payload_at as u64 + 8),
        ] {
            b.push(kind);
            b.extend_from_slice(&id.to_le_bytes());
            b.extend_from_slice(&w.to_le_bytes());
            b.extend_from_slice(&h.to_le_bytes());
            b.extend_from_slice(&off.to_le_bytes());
        }
        b.extend_from_slice(&[0xAB; 12]); // 8 bg bytes + 4 medallion bytes
        b
    }

    #[test]
    fn absent_and_garbage_open_as_none() {
        let path = std::env::temp_dir().join(format!(
            "wowdps-talent-art-test-{}-absent.bin",
            std::process::id()
        ));
        assert!(open_at(&path).is_none());
        let f = TempFile::new("garbage", &[0xDE, 0xAD, 0xBE, 0xEF, 1, 0, 0, 0, 1, 0, 0, 0]);
        assert!(open_at(&f.0).is_none());
    }

    #[test]
    fn truncations_degrade_cleanly() {
        let full = valid_bytes();
        let f = TempFile::new("short-header", &full[..8]);
        assert!(open_at(&f.0).is_none());
        let f = TempFile::new("mid-index", &full[..20]);
        assert!(open_at(&f.0).is_none());
        // Index intact, payload cut mid-tile: lookups answer None forever.
        let f = TempFile::new("mid-tile", &full[..12 + 34 + 3]);
        let c = open_at(&f.0).expect("index is intact");
        assert!(c.lookup(0, 266).is_none());
        assert!(c.lookup(0, 266).is_none(), "failure is cached, not retried");
    }

    #[test]
    fn a_lying_count_or_size_is_rejected() {
        let mut b = valid_bytes();
        b.splice(8..12, u32::MAX.to_le_bytes());
        let f = TempFile::new("count-lie", &b);
        assert!(open_at(&f.0).is_none());
        let mut b = valid_bytes();
        b.splice(17..19, 5000u16.to_le_bytes()); // width 5000 in entry 0
        let f = TempFile::new("size-lie", &b);
        assert!(open_at(&f.0).is_none());
    }

    #[test]
    fn a_complete_cache_serves_art_with_dims() {
        let f = TempFile::new("valid", &valid_bytes());
        let c = open_at(&f.0).expect("valid cache");
        let (_, w, h) = c.lookup(0, 266).expect("background");
        assert_eq!((w, h), (2, 1));
        assert!(c.lookup(1, 59).is_some());
        assert!(c.lookup(0, 999).is_none(), "unknown spec");
        assert!(c.lookup(2, 0).is_none(), "no chrome in this fixture");
    }
}
