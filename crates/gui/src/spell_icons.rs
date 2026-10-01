//! Ability icons as iced handles: gui-logic's reader of the per-machine
//! `spell-icons.bin` (`wowdps_gui_logic::spell_icons`), held once for the
//! process. Each tile becomes one iced handle on its first read and the
//! reader's memo hands back clones of it — a clone keeps the image id, so
//! iced uploads an icon once, not every frame. No cache means no icons.

use std::sync::OnceLock;

use iced::widget::image::Handle;
use wowdps_gui_logic::spell_icons::SpellIcons;

pub(crate) use wowdps_gui_logic::spell_icons::IconStyle;

use crate::icons::make;

fn cache() -> Option<&'static SpellIcons<Handle>> {
    static CACHE: OnceLock<Option<SpellIcons<Handle>>> = OnceLock::new();
    CACHE.get_or_init(SpellIcons::open).as_ref()
}

/// The icon for a spell id, or `None` (no cache, unknown spell, short read).
/// Handles are cached; cloning one is cheap.
pub(crate) fn handle(spell_id: u32) -> Option<Handle> {
    if spell_id == 0 {
        return None;
    }
    cache()?.lookup(spell_id, make)
}

/// The talent viewer's cut of an icon: shaped to the node (square/circle/
/// octagon) and desaturated when the talent is untaken.
pub(crate) fn styled(spell_id: u32, style: IconStyle, gray: bool) -> Option<Handle> {
    if spell_id == 0 {
        return None;
    }
    cache()?.lookup_styled(spell_id, style, gray, make)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The frame-count probe: a tile looked up frame after frame comes back
    /// as the SAME iced image, so iced uploads it once. A handle rebuilt per
    /// lookup would draw identical pixels — no picture can tell — while
    /// re-uploading the texture every frame.
    #[test]
    fn a_tile_is_one_iced_image_across_frames() {
        let mut b = Vec::new();
        for w in [1u32, 2, 1, 1] {
            b.extend_from_slice(&w.to_le_bytes()); // version, px, spells, tiles
        }
        b.splice(0..0, *b"WDPI");
        b.extend_from_slice(&100u32.to_le_bytes()); // spell 100 -> tile 0
        b.extend_from_slice(&0u32.to_le_bytes());
        b.extend_from_slice(&[0x7F; 16]); // one 2×2 tile
        let path = std::env::temp_dir().join(format!(
            "wowdps-gui-spell-icons-probe-{}.bin",
            std::process::id()
        ));
        std::fs::write(&path, &b).unwrap();
        let icons: SpellIcons<Handle> = SpellIcons::open_at(&path).expect("a valid cache");
        let frames: Vec<_> = (0..3)
            .map(|_| icons.lookup(100, make).expect("the tile").id())
            .collect();
        assert!(frames.windows(2).all(|w| w[0] == w[1]), "{frames:?}");
        let styled = icons
            .lookup_styled(100, IconStyle::Circle, true, make)
            .unwrap();
        assert_ne!(
            styled.id(),
            frames[0],
            "a talent-viewer cut is its own image"
        );
        let _ = std::fs::remove_file(&path);
    }
}
