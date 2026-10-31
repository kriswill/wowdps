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
//! A tile can be far larger than the disc it is drawn on (32 px from this
//! repository's gen-icons, 208 px from an upscaled cut of the same art),
//! and GPUI samples an image with no mipmaps: a 208 px tile drawn as an
//! 18 px disc would shimmer. So a lookup names the
//! size it is drawn at, in device pixels, and gets the tile halved to the
//! smallest level that still covers it (`level`): never minified more than
//! 2:1. Like the spell-icon cache, tiles are read from the file on first
//! use, each level made once.

use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};

use wowdps_model::Class;

use crate::lazy_tiles::{Rgba, Tiles, le_u32};

/// No level is halved below this edge: the smallest disc is larger.
const MIN_LEVEL: u32 = 8;

pub struct ClassIcons<H> {
    icon_px: u32,
    n_class: u32,
    /// (spec id, tile index), sorted by spec id.
    index: Vec<(u32, u32)>,
    tiles_at: u64,
    /// Keyed by (tile, halvings): each level is its own handle, made from
    /// one read of the whole tile.
    tiles: Tiles<(u32, u32), H>,
}

fn cache_path() -> Option<PathBuf> {
    wowdps_proto::talents::data_path("class-icons.bin")
}

/// The whole reader behind a path parameter, so tests can point it at absent,
/// truncated or garbage files without touching the real per-machine cache.
fn open_at<H: Clone>(path: &Path) -> Option<ClassIcons<H>> {
    let mut file = File::open(path).ok()?;
    let mut head = [0u8; 20];
    file.read_exact(&mut head).ok()?;
    if head.get(..4) != Some(b"WDCI") {
        return None;
    }
    if le_u32(&head, 4) != 1 {
        return None; // future format: draw discs rather than garbage
    }
    let icon_px = le_u32(&head, 8);
    let n_class = le_u32(&head, 12);
    let n_spec = le_u32(&head, 16) as usize;
    if icon_px == 0 || icon_px > 256 || n_class > 64 || n_spec > 1024 {
        return None;
    }
    let mut raw = vec![0u8; n_spec * 8];
    file.read_exact(&mut raw).ok()?;
    let index: Vec<(u32, u32)> = (0..n_spec)
        .map(|i| (le_u32(&raw, i * 8), le_u32(&raw, i * 8 + 4)))
        .collect();
    Some(ClassIcons {
        icon_px,
        n_class,
        tiles_at: 20 + index.len() as u64 * 8,
        index,
        tiles: Tiles::new(file),
    })
}

/// How many times a tile `icon_px` across is halved to be drawn `want`
/// device pixels across: the smallest level still at least `want`, no
/// smaller than `MIN_LEVEL`, the whole tile when even that is short.
pub fn level(icon_px: u32, want: f32) -> u32 {
    let (mut k, mut edge) = (0, icon_px);
    while edge / 2 >= MIN_LEVEL && (edge / 2) as f32 >= want {
        edge /= 2;
        k += 1;
    }
    k
}

/// A square RGBA tile `edge` across, halved `k` times: each pixel the
/// alpha-weighted mean of the 2×2 block under it (an odd edge drops its
/// last row and column), so a masked rim stays clean at every level.
fn halve(mut pixels: Vec<u8>, mut edge: u32, k: u32) -> Rgba {
    for _ in 0..k {
        let (from, to) = (edge as usize, (edge / 2) as usize);
        let mut out = Vec::with_capacity(to * to * 4);
        for y in 0..to {
            for x in 0..to {
                let mut sum = [0u32; 4];
                for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                    let at = ((2 * y + dy) * from + 2 * x + dx) * 4;
                    let [r, g, b, a] = pixels
                        .get(at..at + 4)
                        .and_then(|p| <[u8; 4]>::try_from(p).ok())
                        .unwrap_or_default();
                    let a = u32::from(a);
                    for (s, v) in sum.iter_mut().zip([r, g, b]) {
                        *s += u32::from(v) * a;
                    }
                    sum[3] += a;
                }
                let alpha = sum[3];
                let color = |s: u32| s.checked_div(alpha).unwrap_or(0) as u8;
                out.extend([
                    color(sum[0]),
                    color(sum[1]),
                    color(sum[2]),
                    (alpha / 4) as u8,
                ]);
            }
        }
        pixels = out;
        edge /= 2;
    }
    Rgba {
        w: edge,
        h: edge,
        pixels,
    }
}

/// Any RGBA tile and its halvings, largest first: each the alpha-weighted
/// mean of the 2×2 blocks of the one before (an odd side drops its last
/// row or column), as `halve` makes the class and spec icons' levels, down
/// to the last whose sides are both at least `MIN_LEVEL`. A square tile
/// `e` across gets every level `level(e, …)` picks, at the index it
/// picks; a picture drawn at many sizes (a portrait, a ring, a marker)
/// keeps them all and draws the one that covers it.
pub fn halvings(tile: Rgba) -> Vec<Rgba> {
    let mut out = vec![tile];
    while let Some(last) = out
        .last()
        .filter(|t| t.w / 2 >= MIN_LEVEL && t.h / 2 >= MIN_LEVEL)
    {
        let (from, w, h) = (
            last.w as usize,
            (last.w / 2) as usize,
            (last.h / 2) as usize,
        );
        let mut pixels = Vec::with_capacity(w * h * 4);
        for y in 0..h {
            for x in 0..w {
                let mut sum = [0u32; 4];
                for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                    let at = ((2 * y + dy) * from + 2 * x + dx) * 4;
                    let [r, g, b, a] = last
                        .pixels
                        .get(at..at + 4)
                        .and_then(|p| <[u8; 4]>::try_from(p).ok())
                        .unwrap_or_default();
                    let a = u32::from(a);
                    for (s, v) in sum.iter_mut().zip([r, g, b]) {
                        *s += u32::from(v) * a;
                    }
                    sum[3] += a;
                }
                let alpha = sum[3];
                let color = |s: u32| s.checked_div(alpha).unwrap_or(0) as u8;
                pixels.extend([
                    color(sum[0]),
                    color(sum[1]),
                    color(sum[2]),
                    (alpha / 4) as u8,
                ]);
            }
        }
        out.push(Rgba {
            w: w as u32,
            h: h as u32,
            pixels,
        });
    }
    out
}

impl<H: Clone> ClassIcons<H> {
    /// The per-machine cache, or `None` without one (or with one this build
    /// cannot read).
    pub fn open() -> Option<Self> {
        open_at(&cache_path()?)
    }

    /// The whole reader behind a path parameter, so tests can point it at
    /// absent, truncated or garbage files without touching the real
    /// per-machine cache.
    pub fn open_at(path: &Path) -> Option<Self> {
        open_at(path)
    }

    /// The edge of every level a tile is served at, largest first: what a
    /// caller that picks its own level at paint time (the replay) makes.
    pub fn sizes(&self) -> Vec<u32> {
        (0..=level(self.icon_px, 0.0))
            .map(|k| self.icon_px >> k)
            .collect()
    }

    /// A tile's handle at the level for `want` device pixels, `make`
    /// building it on first use. A tile the file cannot serve (short read
    /// past a truncation) caches as `None` — asked once, failed forever.
    fn handle(&self, tile: u32, want: f32, make: impl FnOnce(Rgba) -> H) -> Option<H> {
        let k = level(self.icon_px, want);
        let len = (self.icon_px * self.icon_px * 4) as usize;
        let offset = self.tiles_at + u64::from(tile) * len as u64;
        self.tiles.lookup((tile, k), offset, len, |buf| {
            make(halve(buf, self.icon_px, k))
        })
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

impl<H: Clone> ClassIcons<H> {
    /// The class crest (interface/icons/classicon_*), for a disc `want`
    /// device pixels across.
    pub fn class(&self, class: Class, want: f32, make: impl FnOnce(Rgba) -> H) -> Option<H> {
        let code = class_code(class);
        if code >= self.n_class {
            return None;
        }
        self.handle(code, want, make)
    }

    /// The spec's own icon, by Blizzard specID (ChrSpecialization), for a
    /// disc `want` device pixels across.
    pub fn spec(&self, spec_id: u32, want: f32, make: impl FnOnce(Rgba) -> H) -> Option<H> {
        let i = self.index.binary_search_by_key(&spec_id, |e| e.0).ok()?;
        self.handle(self.index.get(i)?.1, want, make)
    }
}

#[cfg(test)]
mod tests {
    //! The cache is per-machine and entirely optional: a machine without it
    //! (or with a half-written or corrupt one) must render fine, so absent,
    //! truncated and garbage files all have to answer `None` — never panic,
    //! never allocate off a lying header. Everything goes through the
    //! `open_at` seam against temp files; the real cache is never touched.

    use super::*;

    /// The reader with its tiles as they come, the handle a test can read.
    fn open_at(path: &Path) -> Option<ClassIcons<Rgba>> {
        super::open_at(path)
    }

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
        // Neither tile is complete: lookups answer None instead of reading
        // past the end.
        assert!(c.handle(0, 2.0, |t| t).is_none());
        assert!(c.handle(1, 2.0, |t| t).is_none());
    }

    #[test]
    fn a_complete_cache_serves_both_crest_and_spec_tiles() {
        let f = TempFile::new("valid", &valid_bytes());
        let c = open_at(&f.0).expect("valid cache");
        assert_eq!(c.icon_px, 2);
        assert_eq!(c.n_class, 1);
        assert_eq!(c.index, vec![(62, 1)]);
        assert!(c.handle(0, 2.0, |t| t).is_some(), "class crest tile");
        assert!(c.handle(1, 2.0, |t| t).is_some(), "spec icon tile");
        assert!(c.handle(2, 2.0, |t| t).is_none(), "past the last tile");
    }

    /// Each level of a tile is made into a handle once; every later ask
    /// clones it.
    #[test]
    fn a_tile_is_made_once_per_level() {
        let f = TempFile::new("made-once", &valid_bytes());
        let c: ClassIcons<usize> = ClassIcons::open_at(&f.0).expect("valid cache");
        let made = std::cell::Cell::new(0);
        let make = |_: Rgba| {
            made.set(made.get() + 1);
            made.get()
        };
        assert_eq!(c.spec(62, 2.0, make), Some(1));
        assert_eq!(c.spec(62, 2.0, make), Some(1));
        assert_eq!(c.class(Class::Warrior, 2.0, make), Some(2), "another tile");
        assert_eq!(c.class(Class::Warrior, 2.0, make), Some(2));
        assert_eq!(c.class(Class::Paladin, 2.0, make), None, "past n_class");
        assert_eq!(made.get(), 2);
    }

    /// The smallest level at least as large as the disc, never halved past
    /// `MIN_LEVEL`, the whole tile for a disc larger than it.
    #[test]
    fn a_disc_gets_the_smallest_level_that_covers_it() {
        assert_eq!(level(208, 300.0), 0, "larger than the tile: the tile");
        assert_eq!(level(208, 208.0), 0);
        assert_eq!(level(208, 150.0), 0, "104 would be stretched");
        assert_eq!(level(208, 104.0), 1);
        assert_eq!(level(208, 42.5), 2, "52 covers an inspector disc");
        assert_eq!(level(208, 22.5), 3, "26 covers a meter disc");
        assert_eq!(level(208, 13.0), 4);
        assert_eq!(level(208, 1.0), 4, "13 is the last above the floor");
        assert_eq!(level(208, f32::NAN), 0, "no size: the whole tile");
        assert_eq!(level(52, 20.0), 1);
        assert_eq!(level(2, 1.0), 0, "a tile under the floor is never halved");
    }

    /// A level is the alpha-weighted mean of the blocks under it: a clear
    /// pixel's color never bleeds into a masked rim.
    #[test]
    fn halving_weighs_color_by_alpha() {
        let px = |r: u8, a: u8| [r, 0, 0, a];
        // 2×2: two opaque reds of different strength, a half-clear one, and
        // a clear pixel whose green must not count.
        let tile: Vec<u8> = [px(200, 255), px(100, 255), [0, 255, 0, 0], px(40, 127)].concat();
        let h = halve(tile, 2, 1);
        assert_eq!((h.w, h.h), (1, 1));
        let red = (200 * 255 + 100 * 255 + 40 * 127) / (255 + 255 + 127);
        assert_eq!(
            h.pixels,
            vec![red as u8, 0, 0, ((255 + 255 + 127) / 4) as u8]
        );
        // A wholly clear block stays clear and black.
        let clear = [9, 9, 9, 0].repeat(4);
        assert_eq!(halve(clear, 2, 1).pixels, vec![0, 0, 0, 0]);
    }

    /// A 208 px tile halves to 104, 52, 26 and 13 (the odd edge's last row
    /// and column dropped): the levels `sizes` names.
    #[test]
    fn a_tile_halves_to_every_level() {
        let tile = vec![255u8; 208 * 208 * 4];
        for (k, edge) in [(0, 208), (1, 104), (2, 52), (3, 26), (4, 13)] {
            let h = halve(tile.clone(), 208, k);
            assert_eq!((h.w, h.h), (edge, edge));
            assert_eq!(h.pixels.len(), (edge * edge * 4) as usize);
            assert!(h.pixels.iter().all(|&v| v == 255), "opaque white stays");
        }
        let mut b = b"WDCI".to_vec();
        for v in [1, 208, 0, 0] {
            b.extend_from_slice(&word(v));
        }
        let f = TempFile::new("sizes", &b);
        let c = open_at(&f.0).expect("a header alone opens");
        assert_eq!(c.sizes(), vec![208, 104, 52, 26, 13]);
    }

    /// Any tile's halvings are `halve`'s levels: for a square tile, each
    /// level `level` picks, at the index it picks, the same pixels; a
    /// rectangle halves both sides, and stops once either would go under
    /// the floor.
    #[test]
    fn a_pictures_halvings_are_the_icons_levels() {
        let edge = 208u32;
        let pixels: Vec<u8> = (0..edge * edge * 4)
            .map(|i| (i.wrapping_mul(2_654_435_761) >> 24) as u8)
            .collect();
        let tile = Rgba {
            w: edge,
            h: edge,
            pixels: pixels.clone(),
        };
        let levels = halvings(tile.clone());
        assert_eq!(levels.len() as u32, level(edge, 1.0) + 1);
        assert_eq!(levels[0], tile);
        for (k, l) in levels.iter().enumerate() {
            assert_eq!(*l, halve(pixels.clone(), edge, k as u32), "level {k}");
        }
        let wide = halvings(Rgba {
            w: 40,
            h: 20,
            pixels: vec![255; 40 * 20 * 4],
        });
        let sizes: Vec<(u32, u32)> = wide.iter().map(|l| (l.w, l.h)).collect();
        assert_eq!(sizes, vec![(40, 20), (20, 10)]);
        assert!(wide[1].pixels.iter().all(|&v| v == 255));
        // A tile under the floor stays whole.
        assert_eq!(
            halvings(Rgba {
                w: 9,
                h: 9,
                pixels: vec![0; 9 * 9 * 4]
            })
            .len(),
            1
        );
    }
}
