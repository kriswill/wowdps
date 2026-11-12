//! The game's art as GPUI images (spec §6): gui-logic's readers of the
//! per-machine caches (`class-icons.bin`, `spell-icons.bin`,
//! `edition-emblems.bin`), each held once for the process and generic over
//! the handle a GUI makes of a tile. This GUI's handle is an
//! `Arc<RenderImage>`, made on a tile's first use and cloned ever after
//! (the readers memoize), so GPUI uploads each texture once — the iced
//! GUI's per-frame re-upload trap (review finding 9) cannot happen here.
//! No cache means `None` everywhere, and the UI draws its class-coloured
//! discs, and each game its own mark, instead.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, OnceLock, PoisonError};

use gpui_kit::{DevicePixels, RenderImage, Size, SvgRenderer, SvgSize};
use wowdps_gui_logic::emblems::Emblems;
use wowdps_gui_logic::icons::ClassIcons;
use wowdps_gui_logic::lazy_tiles::Rgba;
use wowdps_gui_logic::spell_icons::SpellIcons;
use wowdps_model::{Class, Edition};

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

/// A test's stand-in for every game's emblem: its document and its tiles
/// by edge.
#[cfg(test)]
type StandIn = (&'static [u8], BTreeMap<u32, Option<Tile>>);

#[cfg(test)]
thread_local! {
    static NO_ART: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    static TEST_EMBLEM: std::cell::RefCell<Option<StandIn>> =
        const { std::cell::RefCell::new(None) };
}

/// Draw `svg` as every game's emblem on this thread (`None`: read the
/// cache again), so a test holds the switch's layout with an emblem on
/// any machine — the stand-in a drawn shape, never the game's art.
#[cfg(test)]
pub fn emblem_on_this_thread(svg: Option<&'static [u8]>) {
    TEST_EMBLEM.with(|t| *t.borrow_mut() = svg.map(|svg| (svg, BTreeMap::new())));
}

/// Draw no extracted game art on this thread, as a machine without the
/// caches draws: the render guard's pictures are committed, and Blizzard's
/// art never lands in the repository.
#[cfg(test)]
pub fn without_art(off: bool) {
    NO_ART.with(|c| c.set(off));
}

/// Whether the caches may be read here.
fn art_on() -> bool {
    #[cfg(test)]
    {
        !NO_ART.with(std::cell::Cell::get)
    }
    #[cfg(not(test))]
    {
        true
    }
}

fn class_icons() -> Option<&'static ClassIcons<Tile>> {
    static CACHE: OnceLock<Option<ClassIcons<Tile>>> = OnceLock::new();
    art_on().then(|| CACHE.get_or_init(ClassIcons::open).as_ref())?
}

fn spell_icons() -> Option<&'static SpellIcons<Tile>> {
    static CACHE: OnceLock<Option<SpellIcons<Tile>>> = OnceLock::new();
    art_on().then(|| CACHE.get_or_init(SpellIcons::open).as_ref())?
}

fn emblems() -> Option<&'static Emblems> {
    static CACHE: OnceLock<Option<Emblems>> = OnceLock::new();
    art_on().then(|| CACHE.get_or_init(Emblems::open).as_ref())?
}

/// The class crest for a disc `px` device pixels across (the cache's
/// level that covers it: GPUI samples without mipmaps), or `None` without
/// a cache.
pub fn class_icon(class: Class, px: f32) -> Option<Tile> {
    class_icons()?.class(class, px, make)
}

/// The spec's own icon, by Blizzard specID, for a disc `px` device pixels
/// across.
pub fn spec_icon(spec_id: u32, px: f32) -> Option<Tile> {
    class_icons()?.spec(spec_id, px, make)
}

/// An edition's emblem for a game's badge (`window::marks`), drawn in a
/// square `px` device pixels across: the launcher's own for that game
/// (`edition-emblems.bin`, an SVG document), rasterized so its longer side
/// is exactly the whole pixels that cover that square — GPUI samples
/// without mipmaps, and fits the image in the square without scaling it —
/// once per game and size. `None` without the cache or with no emblem of
/// that game in it: the badge holds our own mark.
pub fn edition_emblem(edition: Edition, px: f32) -> Option<Tile> {
    let edge = px.ceil().clamp(1.0, 1024.0) as u32;
    #[cfg(test)]
    if let Some(tile) = TEST_EMBLEM.with(|t| {
        t.borrow_mut().as_mut().map(|(svg, tiles)| {
            tiles
                .entry(edge)
                .or_insert_with(|| rasterize(svg, edge))
                .clone()
        })
    }) {
        return tile;
    }
    let svg = emblems()?.svg(edition)?;
    static TILES: Mutex<BTreeMap<(u8, u32), Option<Tile>>> = Mutex::new(BTreeMap::new());
    let key = (edition.canonical().counts_as().project_id(), edge);
    TILES
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .entry(key)
        .or_insert_with(|| rasterize(svg, edge))
        .clone()
}

thread_local! {
    /// The UI thread's one SVG renderer, so a document with text would
    /// load the font database once, not once per game and size. No asset
    /// source: an emblem names no file.
    static SVG: SvgRenderer = SvgRenderer::new(Arc::new(()));
}

/// An SVG document as one image whose longer side is `edge` device
/// pixels, its proportions kept, through GPUI's own SVG renderer; `None`
/// for a document it cannot read. GPUI sizes a document by its width, so a
/// taller one is drawn again at the width that makes its height `edge`.
fn rasterize(svg: &[u8], edge: u32) -> Option<Tile> {
    SVG.with(|renderer| {
        let tree = renderer.parse_svg(svg).ok()?;
        let at = |width: u32| -> Option<Tile> {
            let width = DevicePixels(i32::try_from(width.max(1)).ok()?);
            renderer
                .render_parsed(
                    &tree,
                    SvgSize::Size(Size {
                        width,
                        height: width,
                    }),
                )
                .ok()
        };
        let wide = at(edge)?;
        let tall = i64::from(wide.size(0).height.0);
        if tall <= i64::from(edge) {
            return Some(wide);
        }
        let width = (f64::from(edge) * f64::from(edge) / tall as f64).round() as u32;
        at(width)
    })
}

/// An ability's icon, read from the spell cache on first use.
pub fn spell_icon(spell_id: u32) -> Option<Tile> {
    spell_icons()?.lookup(spell_id, make)
}

/// The talent viewer's cut of an ability icon: shaped to its node (square,
/// circle, octagon) and desaturated for a talent the build did not take.
/// Each cut is its own tile, made once.
pub fn spell_styled(
    spell_id: u32,
    style: wowdps_gui_logic::spell_icons::IconStyle,
    gray: bool,
) -> Option<Tile> {
    if spell_id == 0 {
        return None;
    }
    spell_icons()?.lookup_styled(spell_id, style, gray, make)
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

    /// Drawn shapes of other proportions: 2 × 4 and 4 × 2.
    const TALL: &[u8] = br##"<svg width="2" height="4" viewBox="0 0 2 4" xmlns="http://www.w3.org/2000/svg"><rect width="2" height="4" fill="#ff0000"/></svg>"##;
    const WIDE: &[u8] = br##"<svg width="4" height="2" viewBox="0 0 4 2" xmlns="http://www.w3.org/2000/svg"><rect width="4" height="2" fill="#ff0000"/></svg>"##;

    /// A drawn disc in a 4 × 4 box: no game art.
    const DISC: &[u8] = br##"<svg width="4" height="4" viewBox="0 0 4 4" xmlns="http://www.w3.org/2000/svg"><circle cx="2" cy="2" r="2" fill="#ff0000"/></svg>"##;

    /// An emblem is rasterized at the whole device pixels that cover its
    /// disc, once per size: a second lookup is the same image. Its pixels
    /// arrive in BGRA, its corners clear. A document GPUI cannot read is
    /// no emblem.
    #[test]
    fn an_emblem_is_drawn_once_per_size() {
        emblem_on_this_thread(Some(DISC));
        let tile = edition_emblem(Edition::Forever, 17.5).expect("the stand-in");
        let size = tile.size(0);
        assert_eq!((size.width.0, size.height.0), (18, 18));
        let again = edition_emblem(Edition::Retail, 17.2).expect("the same size");
        assert!(Arc::ptr_eq(&tile, &again));
        let large = edition_emblem(Edition::Retail, 40.0).expect("a larger size");
        assert_eq!(large.size(0).width.0, 40);
        // A document of another shape keeps its proportions, its longer
        // side the square's: a tall one is not drawn at the square's width.
        emblem_on_this_thread(Some(TALL));
        let tall = edition_emblem(Edition::Retail, 18.0).expect("a tall stand-in");
        assert_eq!((tall.size(0).width.0, tall.size(0).height.0), (9, 18));
        emblem_on_this_thread(Some(WIDE));
        let wide = edition_emblem(Edition::Retail, 18.0).expect("a wide stand-in");
        assert_eq!((wide.size(0).width.0, wide.size(0).height.0), (18, 9));
        emblem_on_this_thread(Some(DISC));
        let px = tile.as_bytes(0).expect("one frame");
        let at = |x: usize, y: usize| &px[(y * 18 + x) * 4..(y * 18 + x) * 4 + 4];
        assert_eq!(at(0, 0)[3], 0, "a clear corner");
        assert_eq!(at(9, 9), &[0, 0, 255, 255][..], "red, in BGRA, opaque");
        emblem_on_this_thread(Some(&b"<svg"[..]));
        assert!(edition_emblem(Edition::Retail, 18.0).is_none());
        emblem_on_this_thread(None);
    }

    /// The caches memoize: a second lookup is the same image, so GPUI keeps
    /// one texture per tile. Skipped where the machine has no cache.
    #[test]
    fn a_tile_is_one_image_across_lookups() {
        let Some(first) = class_icon(Class::Mage, 40.0) else {
            return;
        };
        let again = class_icon(Class::Mage, 40.0).expect("the cache answered once");
        assert!(Arc::ptr_eq(&first, &again));
        assert_eq!(first.id, again.id);
        // A disc five times larger is drawn from a larger level of the same
        // tile, an image of its own — unless the cache holds no larger one
        // (a 32 px cache from before the upscale).
        let large = class_icon(Class::Mage, 200.0).expect("the same cache");
        let (small_w, large_w) = (first.size(0).width, large.size(0).width);
        assert!(large_w >= small_w);
        assert_eq!(Arc::ptr_eq(&first, &large), large_w == small_w);
    }
}
