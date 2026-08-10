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
//! Every icon is decoded at its native 64×64, box-downscaled to 32×32 and
//! circle-masked, so the GUI renders them exactly where the drawn discs sat.
//! Output is deterministic per build: same install in, same bytes out.
//!
//! File layout (all LE), sibling of spell-icons.bin's:
//!   "WDCI" | u32 version=1 | u32 icon_px | u32 n_class | u32 n_spec
//!   n_spec × (u32 spec_id, u32 tile_index)   — sorted by spec_id
//!   tiles: n_class crests first, in `Class` code order (classgen's
//!   CLASS_ORDER — a crest's tile index IS its class code), then the spec
//!   tiles the index points at.

use crate::game::Game;
use crate::table::Csv;
use crate::{blp, classgen};

/// The one table consumed, with its FileDataID.
pub const TABLE: (&str, u32) = ("ChrSpecialization", 1343390);

/// Icon edge in the emitted cache.
const ICON: usize = 32;
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

#[derive(Debug)]
pub struct Generated {
    pub bytes: Vec<u8>,
    pub classes: usize,
    pub specs: usize,
}

/// Fetch + decode one icon into a masked 32×32 RGBA tile.
fn tile(game: &Game, fdid: u32, what: &str) -> Result<Vec<u8>, String> {
    let data = game
        .fetch_fdid(fdid, 0x2)
        .map_err(|e| format!("{what} (fdid {fdid}): {e}"))?;
    let img = blp::decode(&data).map_err(|e| format!("{what} (fdid {fdid}): {e}"))?;
    if img.width % ICON != 0 || img.width != img.height {
        return Err(format!(
            "{what} (fdid {fdid}): unexpected {}x{}",
            img.width, img.height
        ));
    }
    let img = blp::downscale(&img, img.width / ICON);
    Ok(mask(img.rgba))
}

/// Soft circular alpha mask, so the square art sits where a disc did.
fn mask(mut rgba: Vec<u8>) -> Vec<u8> {
    let r = ICON as f32 / 2.0;
    for y in 0..ICON {
        for x in 0..ICON {
            let (dx, dy) = (x as f32 + 0.5 - r, y as f32 + 0.5 - r);
            let d = (dx * dx + dy * dy).sqrt();
            let keep = (r - d).clamp(0.0, 1.0);
            if keep < 1.0
                && let Some(a) = rgba.get_mut((y * ICON + x) * 4 + 3)
            {
                *a = (*a as f32 * keep) as u8;
            }
        }
    }
    rgba
}

pub fn generate(game: &Game, spec_csv: &Csv) -> Result<Generated, String> {
    let mut tiles: Vec<u8> = Vec::new();
    let mut push = |tile: Vec<u8>| -> u32 {
        let idx = (tiles.len() / (ICON * ICON * 4)) as u32;
        tiles.extend_from_slice(&tile);
        idx
    };

    // Class crests, in code order — a crest's tile index IS its class code.
    for (name, fdid) in CLASS_ICONS {
        push(tile(game, fdid, &format!("classicon {name}"))?);
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
        let idx = push(tile(game, fdid, &format!("spec {want} icon"))?);
        specs.push((want as u32, idx));
    }
    specs.sort_unstable();

    let mut bytes = Vec::with_capacity(20 + specs.len() * 8 + tiles.len());
    bytes.extend_from_slice(MAGIC);
    bytes.extend_from_slice(&1u32.to_le_bytes());
    bytes.extend_from_slice(&(ICON as u32).to_le_bytes());
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
        bytes,
    })
}
