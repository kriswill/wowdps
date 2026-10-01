//! The shared skeleton of the lazy-seek cache readers (`spell_icons.rs`,
//! `talent_art.rs`): the LE header words and the memoized seek-and-read
//! behind every tile lookup. Each reader keeps its own header parse and
//! index shape; the failure semantics — poisoned locks recovered, a tile
//! the file cannot serve cached as `None` forever, never a panic — live
//! here exactly once.
//!
//! The memo holds whatever a GUI makes of a tile's bytes (`H`: iced's
//! image handle, GPUI's render image), built once per tile. A GUI that
//! built a fresh handle per lookup would hand its renderer a new image
//! every frame — identical pixels, a re-upload each time.

use std::collections::HashMap;
use std::fs::File;
use std::hash::Hash;
use std::io::{Read, Seek, SeekFrom};
use std::sync::Mutex;

/// A decoded tile as the caches store it: `w × h` pixels, four bytes each
/// (RGBA), row-major — what a GUI turns into an image of its own.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rgba {
    pub w: u32,
    pub h: u32,
    pub pixels: Vec<u8>,
}

/// A little-endian u32 out of a header buffer; bytes past the end read as
/// zero, so a short buffer fails the caller's validation instead of
/// panicking.
pub fn le_u32(b: &[u8], i: usize) -> u32 {
    let at = |i: usize| b.get(i).copied().unwrap_or(0);
    u32::from_le_bytes([at(i), at(i + 1), at(i + 2), at(i + 3)])
}

/// The tile side of a lazy cache: one open file plus the handle memo. A
/// tile the file cannot serve (short read past a truncation) caches as
/// `None` — asked once, failed forever.
pub struct Tiles<K, H> {
    file: Mutex<File>,
    handles: Mutex<HashMap<K, Option<H>>>,
}

impl<K: Eq + Hash + Copy, H: Clone> Tiles<K, H> {
    pub fn new(file: File) -> Self {
        Self {
            file: Mutex::new(file),
            handles: Mutex::new(HashMap::new()),
        }
    }

    /// The memoized seek-and-read: `len` bytes at `offset`, turned into a
    /// handle by `make` on the first successful read.
    pub fn lookup(
        &self,
        key: K,
        offset: u64,
        len: usize,
        make: impl FnOnce(Vec<u8>) -> H,
    ) -> Option<H> {
        let mut handles = self.handles.lock().unwrap_or_else(|e| e.into_inner());
        handles
            .entry(key)
            .or_insert_with(|| {
                let mut buf = vec![0u8; len];
                let mut file = self.file.lock().unwrap_or_else(|e| e.into_inner());
                file.seek(SeekFrom::Start(offset))
                    .ok()
                    .and_then(|_| file.read_exact(&mut buf).ok())
                    .map(|()| make(buf))
            })
            .clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    /// The memo is the point: a tile is read and made ONCE, and every
    /// later lookup hands back a clone of that one handle — the frame-count
    /// probe the move asked for, since a handle rebuilt per lookup draws
    /// the same pixels and no picture can catch it.
    #[test]
    fn a_tile_is_made_once_and_every_lookup_clones_it() {
        let path =
            std::env::temp_dir().join(format!("wowdps-lazy-tiles-test-{}.bin", std::process::id()));
        std::fs::write(&path, [1u8, 2, 3, 4, 5, 6, 7, 8]).unwrap();
        let tiles: Tiles<u32, (usize, Vec<u8>)> = Tiles::new(File::open(&path).unwrap());
        let made = Cell::new(0);
        let make = |buf: Vec<u8>| {
            made.set(made.get() + 1);
            (made.get(), buf)
        };
        let first = tiles.lookup(7, 4, 4, make);
        for _ in 0..3 {
            assert_eq!(tiles.lookup(7, 4, 4, make), first, "the same handle");
        }
        assert_eq!(first, Some((1, vec![5, 6, 7, 8])));
        assert_eq!(made.get(), 1, "made once for four lookups");
        // A short read is remembered as a miss, also without a second read.
        assert_eq!(tiles.lookup(9, 6, 4, make), None);
        assert_eq!(tiles.lookup(9, 6, 4, make), None);
        assert_eq!(made.get(), 1);
        let _ = std::fs::remove_file(&path);
    }
}
