//! The spell-icon cache generator: every spell's icon, straight out of the
//! local install, into one flat file the GUI reads lazily at runtime.
//!
//! Unlike the class/spec atlas (52 icons, embedded in the binary), the combat
//! log can name ANY spell — enchant procs, set bonuses, trinket effects — so
//! this covers all of SpellMisc: ~400k spell ids mapping onto ~14k unique
//! icon files, deduplicated. Too big to embed, cheap to keep on disk
//! (`$XDG_DATA_HOME/wowdps/spell-icons.bin`, ~60 MiB), and entirely optional:
//! a GUI without the file simply draws no ability icons.
//!
//! File layout (all LE):
//!   "WDPI" | u32 version=1 | u32 icon_px | u32 n_spells | u32 n_tiles
//!   n_spells × (u32 spell_id, u32 tile_index)   — sorted by spell_id
//!   n_tiles  × (icon_px² × 4 bytes RGBA)
//!
//! Ability icons stay square (the game's own convention) — only the
//! class/spec atlas is circle-masked, because it replaced drawn discs.

use crate::blp;
use crate::game::Game;
use crate::table::Csv;
use std::collections::HashMap;

/// The one table consumed, with its FileDataID.
pub const TABLE: (&str, u32) = ("SpellMisc", 1003144);

const ICON: usize = 32;
pub const MAGIC: &[u8; 4] = b"WDPI";

#[derive(Debug)]
pub struct Generated {
    pub bytes: Vec<u8>,
    pub spells: usize,
    pub tiles: usize,
    /// Icon files that failed to fetch or decode (their spells are simply
    /// absent from the index).
    pub skipped: usize,
}

pub fn generate(game: &Game, misc: &Csv) -> Result<Generated, String> {
    let c_spell = misc.col("SpellID")?;
    let c_icon = misc.col("SpellIconFileDataID")?;
    let c_diff = misc.col("DifficultyID").ok();

    // spell id -> icon fdid; the base-difficulty row wins when a spell has
    // per-difficulty variants.
    let mut spell_icon: HashMap<u32, (u32, bool)> = HashMap::new();
    for row in &misc.rows {
        let (Some(spell), Some(icon)) = (
            row.get(c_spell).and_then(|s| s.parse::<u32>().ok()),
            row.get(c_icon).and_then(|s| s.parse::<u32>().ok()),
        ) else {
            continue;
        };
        if spell == 0 || icon == 0 {
            continue;
        }
        let base = c_diff.and_then(|c| row.get(c)).is_none_or(|d| d == "0");
        let slot = spell_icon.entry(spell).or_insert((icon, base));
        if base && !slot.1 {
            *slot = (icon, true);
        }
    }

    // Fetch + decode each unique icon once.
    let mut tile_of: HashMap<u32, u32> = HashMap::new();
    let mut tiles: Vec<u8> = Vec::new();
    let mut skipped_fdids: HashMap<u32, ()> = HashMap::new();
    let mut fdids: Vec<u32> = spell_icon.values().map(|(f, _)| *f).collect();
    fdids.sort_unstable();
    fdids.dedup();
    let total = fdids.len();
    for (i, fdid) in fdids.into_iter().enumerate() {
        if i % 2000 == 0 {
            eprintln!("icons: {i}/{total}");
        }
        let tile = game
            .fetch_fdid(fdid, 0x2)
            .map_err(|e| e.to_string())
            .and_then(|data| blp::decode(&data).map_err(|e| e.to_string()))
            .ok()
            .filter(|img| img.width == img.height && img.width % ICON == 0)
            .map(|img| blp::downscale(&img, img.width / ICON).rgba);
        match tile {
            Some(rgba) if rgba.len() == ICON * ICON * 4 => {
                tile_of.insert(fdid, (tiles.len() / (ICON * ICON * 4)) as u32);
                tiles.extend_from_slice(&rgba);
            }
            _ => {
                skipped_fdids.insert(fdid, ());
            }
        }
    }

    let mut index: Vec<(u32, u32)> = spell_icon
        .iter()
        .filter_map(|(spell, (fdid, _))| Some((*spell, *tile_of.get(fdid)?)))
        .collect();
    index.sort_unstable();

    let mut bytes = Vec::with_capacity(20 + index.len() * 8 + tiles.len());
    bytes.extend_from_slice(MAGIC);
    bytes.extend_from_slice(&1u32.to_le_bytes());
    bytes.extend_from_slice(&(ICON as u32).to_le_bytes());
    bytes.extend_from_slice(&(index.len() as u32).to_le_bytes());
    bytes.extend_from_slice(&((tiles.len() / (ICON * ICON * 4)) as u32).to_le_bytes());
    for (spell, tile) in &index {
        bytes.extend_from_slice(&spell.to_le_bytes());
        bytes.extend_from_slice(&tile.to_le_bytes());
    }
    let tiles_n = tiles.len() / (ICON * ICON * 4);
    bytes.extend_from_slice(&tiles);

    Ok(Generated {
        spells: index.len(),
        tiles: tiles_n,
        skipped: skipped_fdids.len(),
        bytes,
    })
}

#[cfg(test)]
mod tests {
    /// The header constants the GUI-side reader hard-codes.
    #[test]
    fn layout_constants() {
        assert_eq!(super::MAGIC, b"WDPI");
        assert_eq!(super::ICON, 32);
    }
}
