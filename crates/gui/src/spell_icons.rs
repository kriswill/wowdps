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
use std::path::PathBuf;
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
    let mut file = File::open(cache_path()?).ok()?;
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

/// The icon for a spell id, or `None` (no cache, unknown spell, short read).
/// Handles are cached; cloning one is cheap.
pub(crate) fn handle(spell_id: u32) -> Option<Handle> {
    if spell_id == 0 {
        return None;
    }
    let c = cache()?;
    let i = c.index.binary_search_by_key(&spell_id, |e| e.0).ok()?;
    let tile = c.index.get(i)?.1;
    let mut handles = c.handles.lock().unwrap_or_else(|e| e.into_inner());
    handles
        .entry(tile)
        .or_insert_with(|| {
            let mut buf = vec![0u8; c.tile_bytes];
            let mut file = c.file.lock().unwrap_or_else(|e| e.into_inner());
            file.seek(SeekFrom::Start(
                c.tiles_at + tile as u64 * c.tile_bytes as u64,
            ))
            .ok()
            .and_then(|_| file.read_exact(&mut buf).ok())
            .map(|()| Handle::from_rgba(c.icon_px, c.icon_px, buf))
        })
        .clone()
}
