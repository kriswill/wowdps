//! The shared skeleton of the lazy-seek cache readers (`spell_icons.rs`,
//! `talent_art.rs`): the LE header words and the memoized seek-and-read
//! behind every tile lookup. Each reader keeps its own header parse and
//! index shape; the failure semantics — poisoned locks recovered, a tile
//! the file cannot serve cached as `None` forever, never a panic — live
//! here exactly once.

use std::collections::HashMap;
use std::fs::File;
use std::hash::Hash;
use std::io::{Read, Seek, SeekFrom};
use std::sync::Mutex;

use iced::widget::image::Handle;

/// A little-endian u32 out of a header buffer; bytes past the end read as
/// zero, so a short buffer fails the caller's validation instead of
/// panicking.
pub(crate) fn le_u32(b: &[u8], i: usize) -> u32 {
    let at = |i: usize| b.get(i).copied().unwrap_or(0);
    u32::from_le_bytes([at(i), at(i + 1), at(i + 2), at(i + 3)])
}

/// The tile side of a lazy cache: one open file plus the handle memo. A
/// tile the file cannot serve (short read past a truncation) caches as
/// `None` — asked once, failed forever.
pub(crate) struct Tiles<K> {
    file: Mutex<File>,
    handles: Mutex<HashMap<K, Option<Handle>>>,
}

impl<K: Eq + Hash + Copy> Tiles<K> {
    pub(crate) fn new(file: File) -> Self {
        Self {
            file: Mutex::new(file),
            handles: Mutex::new(HashMap::new()),
        }
    }

    /// The memoized seek-and-read: `len` bytes at `offset`, turned into a
    /// handle by `make` on the first successful read.
    pub(crate) fn lookup(
        &self,
        key: K,
        offset: u64,
        len: usize,
        make: impl FnOnce(Vec<u8>) -> Handle,
    ) -> Option<Handle> {
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
