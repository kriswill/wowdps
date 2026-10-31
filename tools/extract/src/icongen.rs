//! The class/spec icon cache generator: the game's own class crests and spec
//! icons, decoded from BLP into `$XDG_DATA_HOME/wowdps/class-icons.bin` — a
//! PER-MACHINE cache the GUI reads at runtime, never committed (it is
//! Blizzard artwork; the drawn class-colored discs remain the fallback on a
//! machine that never generated it).
//!
//! Sources:
//!   * class crests — the classic `interface/icons/classicon_*.blp` files,
//!     addressed by FileDataID (from wowdev/wow-listfile; stable forever)
//!   * spec icons — ChrSpecialization.SpellIconFileID per player spec
//!
//! Every icon is 64×64 in the game. A disc drawn larger than that blurs, so
//! `tools/gen-icons.sh` writes each one out whole (`--whole`), upscales them
//! 4× with Real-ESRGAN's illustration model, and builds the cache from the
//! upscales (`--upscaled`); without the upscaler the cache is the art's own
//! pixels. Either way each icon loses the bevel the game frames it with
//! (`CROP_OF_64`) and is circle-masked, so the art sits where the drawn
//! discs did: 208 px tiles from the upscales, 52 px from the game's own.
//! The GUI halves a tile to the level it draws at (gui-logic's `icons`).
//! Output is deterministic per input: same install (and upscales) in, same
//! bytes out.
//!
//! File layout (all LE), sibling of spell-icons.bin's:
//!   "WDCI" | u32 version=1 | u32 icon_px | u32 n_class | u32 n_spec
//!   n_spec × (u32 spec_id, u32 tile_index)   — sorted by spec_id
//!   tiles: n_class crests first, in `Class` code order (classgen's
//!   CLASS_ORDER — a crest's tile index IS its class code), then the spec
//!   tiles the index points at.

use std::path::Path;

use crate::game::Game;
use crate::img::{Image, downscale_box, mask_disc};
use crate::table::Csv;
use crate::{blp, classgen, png};

/// The one table consumed (its FileDataID: `table::FILE_IDS`).
pub const TABLE: &str = "ChrSpecialization";

/// The game's icon edge: every source is reduced to it, and it is what
/// `--whole` writes for the upscaler.
const NATIVE: usize = 64;
/// What the upscaler multiplies an edge by (`realesrgan-ncnn-vulkan -s 4`).
const UPSCALE: usize = 4;
/// Of each 64 px of edge, what each side loses: the bevel every icon wears
/// (a light rim over a dark line, 4 px), which a disc must not, and which
/// the upscaler sharpens into a bright ring. Six, so a 208 px tile halves
/// evenly to 13.
const CROP_OF_64: usize = 6;
pub const MAGIC: &[u8; 4] = b"WDCI";

/// `interface/icons/classicon_*.blp` FileDataIDs, in `Class` code order
/// (classgen::CLASS_ORDER — the order the generated `match` spells out).
const CLASS_ICONS: [(&str, u32); 13] = [
    ("Warrior", 626008),
    ("Paladin", 626003),
    ("Hunter", 626000),
    ("Rogue", 626005),
    ("Priest", 626004),
    ("DeathKnight", 625998),
    ("Shaman", 626006),
    ("Mage", 626001),
    ("Warlock", 626007),
    ("Monk", 626002),
    ("Druid", 625999),
    ("DemonHunter", 1260827),
    ("Evoker", 4574311),
];

/// Where the art goes out and comes back: `whole` receives every icon at
/// its native 64 px as `<fdid>.png`; `upscaled` holds their 4× upscales by
/// the same names, the tiles' source when given.
#[derive(Debug, Default, Clone, Copy)]
pub struct Dirs<'a> {
    pub whole: Option<&'a Path>,
    pub upscaled: Option<&'a Path>,
}

#[derive(Debug)]
pub struct Generated {
    pub bytes: Vec<u8>,
    pub classes: usize,
    pub specs: usize,
    pub icon_px: usize,
}

/// The game's icon as decoded, reduced to `NATIVE` should a later build
/// ship it larger.
fn native(game: &Game, fdid: u32, what: &str) -> Result<Image, String> {
    let data = game
        .fetch_fdid(fdid, 0x2)
        .map_err(|e| format!("{what} (fdid {fdid}): {e}"))?;
    let img = blp::decode(&data).map_err(|e| format!("{what} (fdid {fdid}): {e}"))?;
    if img.width % NATIVE != 0 || img.width != img.height {
        return Err(format!(
            "{what} (fdid {fdid}): unexpected {}x{}",
            img.width, img.height
        ));
    }
    Ok(downscale_box(&img, img.width / NATIVE))
}

/// The upscale of `fdid` from `dir`: exactly `UPSCALE` times the native
/// edge, or an error naming it (a partial set must not mix tile sizes).
fn upscaled(dir: &Path, fdid: u32, what: &str) -> Result<Image, String> {
    let path = dir.join(format!("{fdid}.png"));
    let img = png::read(&path).map_err(|e| format!("{what}: {e}"))?;
    let edge = NATIVE * UPSCALE;
    if img.width != edge || img.height != edge {
        return Err(format!(
            "{what}: {} is {}x{}, not the {edge}x{edge} upscale",
            path.display(),
            img.width,
            img.height
        ));
    }
    Ok(img)
}

/// The square inside the bevel; `None` for a picture too small to hold it.
fn crop(img: &Image) -> Option<Image> {
    let cut = img.width * CROP_OF_64 / NATIVE;
    let edge = img.width - 2 * cut;
    img.crop(cut, cut, edge, edge)
}

/// One icon's tile: written whole for the upscaler when asked, then cut
/// from its upscale (or its own pixels) and masked to a disc.
fn tile(game: &Game, dirs: Dirs, fdid: u32, what: &str) -> Result<Vec<u8>, String> {
    let img = native(game, fdid, what)?;
    if let Some(dir) = dirs.whole {
        png::write(&dir.join(format!("{fdid}.png")), &img)?;
    }
    let img = match dirs.upscaled {
        Some(dir) => upscaled(dir, fdid, what)?,
        None => img,
    };
    let mut img = crop(&img).ok_or_else(|| {
        format!(
            "{what} (fdid {fdid}): {}x{} is no square to crop",
            img.width, img.height
        )
    })?;
    mask_disc(&mut img);
    Ok(img.rgba)
}

/// The tile edge a run writes: the cropped upscale, or the cropped native.
fn icon_px(dirs: Dirs) -> usize {
    let edge = if dirs.upscaled.is_some() {
        NATIVE * UPSCALE
    } else {
        NATIVE
    };
    edge - 2 * (edge * CROP_OF_64 / NATIVE)
}

pub fn generate(game: &Game, spec_csv: &Csv, dirs: Dirs) -> Result<Generated, String> {
    let icon = icon_px(dirs);
    let mut tiles: Vec<u8> = Vec::new();
    let mut push = |tile: Vec<u8>| -> u32 {
        let idx = (tiles.len() / (icon * icon * 4)) as u32;
        tiles.extend_from_slice(&tile);
        idx
    };

    // Class crests, in code order — a crest's tile index IS its class code.
    for (name, fdid) in CLASS_ICONS {
        push(tile(game, dirs, fdid, &format!("classicon {name}"))?);
    }

    // Spec icons, from the table.
    let (c_id, c_icon) = (spec_csv.col("ID")?, spec_csv.col("SpellIconFileID")?);
    let mut specs: Vec<(u32, u32)> = Vec::new();
    for (want, _) in classgen::SPEC_CLASS {
        let row = spec_csv
            .rows
            .iter()
            .find(|r| r.get(c_id).map(String::as_str) == Some(&want.to_string()))
            .ok_or_else(|| format!("ChrSpecialization: no row for spec {want}"))?;
        let fdid: u32 = row
            .get(c_icon)
            .and_then(|s| s.parse().ok())
            .ok_or_else(|| format!("spec {want}: bad SpellIconFileID"))?;
        let idx = push(tile(game, dirs, fdid, &format!("spec {want} icon"))?);
        specs.push((want as u32, idx));
    }
    specs.sort_unstable();

    let mut bytes = Vec::with_capacity(20 + specs.len() * 8 + tiles.len());
    bytes.extend_from_slice(MAGIC);
    bytes.extend_from_slice(&1u32.to_le_bytes());
    bytes.extend_from_slice(&(icon as u32).to_le_bytes());
    bytes.extend_from_slice(&(CLASS_ICONS.len() as u32).to_le_bytes());
    bytes.extend_from_slice(&(specs.len() as u32).to_le_bytes());
    for (spec, tile) in &specs {
        bytes.extend_from_slice(&spec.to_le_bytes());
        bytes.extend_from_slice(&tile.to_le_bytes());
    }
    bytes.extend_from_slice(&tiles);

    Ok(Generated {
        classes: CLASS_ICONS.len(),
        specs: specs.len(),
        icon_px: icon,
        bytes,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A tile is the art inside the bevel: 52 px of the game's 64, 208 of
    /// the upscale's 256 — an edge that halves evenly to 13.
    #[test]
    fn a_tile_is_the_art_inside_the_bevel() {
        assert_eq!(icon_px(Dirs::default()), 52);
        let up = Dirs {
            upscaled: Some(Path::new("x4")),
            ..Dirs::default()
        };
        assert_eq!(icon_px(up), 208);
        assert_eq!(208 >> 4, 13);
        // The crop takes the same 6/64 from every side.
        let rgba: Vec<u8> = (0..64 * 64)
            .flat_map(|i| {
                let (x, y) = (i % 64, i / 64);
                [x as u8, y as u8, 0, 255]
            })
            .collect();
        let c = crop(&Image {
            width: 64,
            height: 64,
            rgba,
        })
        .unwrap();
        assert_eq!((c.width, c.height, c.rgba.len()), (52, 52, 52 * 52 * 4));
        assert_eq!(c.rgba.get(..2), Some(&[6, 6][..]), "first pixel");
        assert_eq!(
            c.rgba.get(c.rgba.len() - 4..c.rgba.len() - 2),
            Some(&[57, 57][..])
        );
    }

    /// The mask clears the corners and keeps the middle whole.
    #[test]
    fn the_mask_is_a_disc() {
        let mut m = Image {
            width: 52,
            height: 52,
            rgba: vec![255; 52 * 52 * 4],
        };
        mask_disc(&mut m);
        let alpha = |x: usize, y: usize| m.rgba[(y * 52 + x) * 4 + 3];
        assert_eq!(alpha(0, 0), 0, "a corner");
        assert_eq!(alpha(26, 26), 255, "the middle");
        assert_eq!(alpha(26, 1), 255, "inside the top edge");
    }
}
