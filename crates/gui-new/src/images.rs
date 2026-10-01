//! The game's art as GPUI images (spec §6): gui-logic's readers of the
//! per-machine caches (`class-icons.bin`, `spell-icons.bin`), each held
//! once for the process and generic over the handle a GUI makes of a
//! tile. gui-new's handle is an `Arc<RenderImage>`, made on a tile's first
//! use and cloned ever after (the readers memoize), so GPUI uploads each
//! texture once — the iced GUI's per-frame re-upload trap (review finding
//! 9) cannot happen here. No cache means `None` everywhere, and the UI
//! draws its class-coloured discs instead.

use std::sync::{Arc, OnceLock};

use gpui_kit::RenderImage;
use wowdps_gui_logic::icons::ClassIcons;
use wowdps_gui_logic::lazy_tiles::Rgba;
use wowdps_gui_logic::spell_icons::SpellIcons;
use wowdps_model::Class;

/// One decoded tile, ready to paint.
pub type Tile = Arc<RenderImage>;

/// A tile as GPUI wants it: BGRA, one frame. A tile whose pixels do not
/// fill its size (the readers validate, so never in practice) becomes one
/// transparent pixel rather than a panic.
pub fn make(tile: Rgba) -> Tile {
    let Rgba { w, h, mut pixels } = tile;
    for px in pixels.as_chunks_mut::<4>().0 {
        px.swap(0, 2);
    }
    let buffer =
        image::RgbaImage::from_raw(w, h, pixels).unwrap_or_else(|| image::RgbaImage::new(1, 1));
    Arc::new(RenderImage::new(vec![image::Frame::new(buffer)]))
}

fn class_icons() -> Option<&'static ClassIcons<Tile>> {
    static CACHE: OnceLock<Option<ClassIcons<Tile>>> = OnceLock::new();
    CACHE.get_or_init(ClassIcons::open).as_ref()
}

#[cfg_attr(
    not(test),
    expect(dead_code, reason = "the drill's ability rows draw these from phase 2")
)]
fn spell_icons() -> Option<&'static SpellIcons<Tile>> {
    static CACHE: OnceLock<Option<SpellIcons<Tile>>> = OnceLock::new();
    CACHE.get_or_init(SpellIcons::open).as_ref()
}

/// The class crest, or `None` without a cache.
pub fn class_icon(class: Class) -> Option<Tile> {
    class_icons()?.class(class, make)
}

/// The spec's own icon, by Blizzard specID.
pub fn spec_icon(spec_id: u32) -> Option<Tile> {
    class_icons()?.spec(spec_id, make)
}

/// An ability's icon, read from the spell cache on first use.
#[cfg_attr(
    not(test),
    expect(dead_code, reason = "the drill's ability rows draw these from phase 2")
)]
pub fn spell_icon(spell_id: u32) -> Option<Tile> {
    spell_icons()?.lookup(spell_id, make)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// GPUI paints BGRA: a red RGBA pixel must arrive with its blue and
    /// red swapped, alpha and green untouched.
    #[test]
    fn a_tile_arrives_in_bgra() {
        let tile = make(Rgba {
            w: 1,
            h: 2,
            pixels: vec![255, 10, 20, 200, 1, 2, 3, 4],
        });
        assert_eq!(tile.as_bytes(0), Some(&[20, 10, 255, 200, 3, 2, 1, 4][..]));
        let short = make(Rgba {
            w: 4,
            h: 4,
            pixels: vec![0; 3],
        });
        assert_eq!(
            short.as_bytes(0).map(<[u8]>::len),
            Some(4),
            "a pixel, not a panic"
        );
    }

    /// The caches memoize: a second lookup is the same image, so GPUI keeps
    /// one texture per tile. Skipped where the machine has no cache.
    #[test]
    fn a_tile_is_one_image_across_lookups() {
        let Some(first) = class_icon(Class::Mage) else {
            return;
        };
        let again = class_icon(Class::Mage).expect("the cache answered once");
        assert!(Arc::ptr_eq(&first, &again));
        assert_eq!(first.id, again.id);
    }
}
