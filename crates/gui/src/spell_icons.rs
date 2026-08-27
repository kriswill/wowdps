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

use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use iced::widget::image::Handle;

use crate::lazy_tiles::{Tiles, le_u32};

/// How the talent viewer wants an ability icon cut: the game's node
/// shapes — square active abilities, circular passives, octagonal choice
/// nodes — each optionally desaturated for a talent the build did not
/// take. `Square` + colored is the plain tile every other caller uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum IconStyle {
    Square,
    Circle,
    Octagon,
}

impl IconStyle {
    fn code(self, gray: bool) -> u8 {
        let shape = match self {
            IconStyle::Square => 0,
            IconStyle::Circle => 1,
            IconStyle::Octagon => 2,
        };
        shape | if gray { 4 } else { 0 }
    }
}

struct Cache {
    /// (spell id, tile index), sorted by spell id.
    index: Vec<(u32, u32)>,
    tiles_at: u64,
    icon_px: u32,
    tile_bytes: usize,
    /// Keyed by (tile, style code): the plain tile and its talent-viewer
    /// variants are distinct handles built from one read.
    tiles: Tiles<(u32, u8)>,
}

fn cache_path() -> Option<PathBuf> {
    wowdps_proto::talents::data_path("spell-icons.bin")
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
    if le_u32(&head, 4) != 1 {
        return None; // future format: draw nothing rather than garbage
    }
    let icon_px = le_u32(&head, 8);
    let n_spells = le_u32(&head, 12) as usize;
    if icon_px == 0 || icon_px > 256 || n_spells > 4_000_000 {
        return None;
    }
    let mut raw = vec![0u8; n_spells * 8];
    file.read_exact(&mut raw).ok()?;
    let index: Vec<(u32, u32)> = raw
        .as_chunks::<8>()
        .0
        .iter()
        .map(|&[a0, a1, a2, a3, b0, b1, b2, b3]| {
            (
                u32::from_le_bytes([a0, a1, a2, a3]),
                u32::from_le_bytes([b0, b1, b2, b3]),
            )
        })
        .collect();
    Some(Cache {
        tiles_at: 20 + index.len() as u64 * 8,
        index,
        icon_px,
        tile_bytes: (icon_px * icon_px * 4) as usize,
        tiles: Tiles::new(file),
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
        self.lookup_styled(spell_id, IconStyle::Square, false)
    }

    fn lookup_styled(&self, spell_id: u32, style: IconStyle, gray: bool) -> Option<Handle> {
        let i = self.index.binary_search_by_key(&spell_id, |e| e.0).ok()?;
        let tile = self.index.get(i)?.1;
        self.tiles.lookup(
            (tile, style.code(gray)),
            self.tiles_at + tile as u64 * self.tile_bytes as u64,
            self.tile_bytes,
            |mut buf| {
                restyle(&mut buf, self.icon_px as usize, style, gray);
                Handle::from_rgba(self.icon_px, self.icon_px, buf)
            },
        )
    }
}

/// Desaturate and/or alpha-mask a square RGBA tile in place. The masks are
/// soft-edged (one antialiased pixel), the same trick as the class-icon
/// generator's circle.
fn restyle(rgba: &mut [u8], px: usize, style: IconStyle, gray: bool) {
    let half = px as f32 / 2.0;
    // The octagon's corner cut, from the corner along both edges.
    let cut = px as f32 * 0.29;
    for (i, p) in rgba.chunks_exact_mut(4).enumerate() {
        let [r, g, b, a] = p else { continue };
        if gray {
            let luma = (0.299 * f32::from(*r) + 0.587 * f32::from(*g) + 0.114 * f32::from(*b))
                // Dimmed too: an untaken talent recedes, not just grays.
                * 0.62;
            let v = luma.round().clamp(0.0, 255.0) as u8;
            (*r, *g, *b) = (v, v, v);
        }
        let (x, y) = ((i % px) as f32 + 0.5, (i / px) as f32 + 0.5);
        let keep = match style {
            IconStyle::Square => 1.0,
            IconStyle::Circle => {
                let d = (x - half).hypot(y - half);
                (half - d).clamp(0.0, 1.0)
            }
            IconStyle::Octagon => {
                // Distance in from the two edges meeting at the nearest
                // corner; below the cut line the pixel is outside.
                let (u, v) = (x.min(px as f32 - x), y.min(px as f32 - y));
                (u + v - cut).clamp(0.0, 1.0)
            }
        };
        if keep < 1.0 {
            *a = (f32::from(*a) * keep) as u8;
        }
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

/// The talent viewer's cut of an icon: shaped to the node (square/circle/
/// octagon) and desaturated when the talent is untaken.
pub(crate) fn styled(spell_id: u32, style: IconStyle, gray: bool) -> Option<Handle> {
    if spell_id == 0 {
        return None;
    }
    cache()?.lookup_styled(spell_id, style, gray)
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
