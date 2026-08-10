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
use std::path::PathBuf;
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
    let bytes = std::fs::read(cache_path()?).ok()?;
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
