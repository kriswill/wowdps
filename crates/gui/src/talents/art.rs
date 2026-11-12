//! The viewer's art, all from the per-machine caches: the spec's
//! background painting, the hero tree's medallion and the golden ring
//! (`talent-art.bin`), the class and spec crests (`class-icons.bin`) and
//! the node icons cut to their shapes (`spell-icons.bin`). The paintings,
//! medallions, ring and node icons are the shown game's own (each edition
//! its own file, `forever/talent-art.bin` beside Retail's): a game whose
//! cache was never made draws plain panels, never another game's art.
//! Without a cache every lookup is `None` and the viewer draws plain
//! panels; while a test has put its own dataset on the thread it consults
//! none, so every render exercises that path.

use std::sync::OnceLock;

use wowdps_gui_logic::spell_icons::IconStyle;
use wowdps_gui_logic::talent_art::TalentArt;
use wowdps_gui_logic::talents as logic;
use wowdps_model::{Class, Edition};

use crate::images::{self, Tile, make};

fn available() -> bool {
    !logic::under_test()
}

/// `edition`'s talent art, read once per game: Retail's and Forever's
/// each their own; any other edition has none.
fn talent_art(edition: Edition) -> Option<&'static TalentArt<Tile>> {
    static RETAIL: OnceLock<Option<TalentArt<Tile>>> = OnceLock::new();
    static FOREVER: OnceLock<Option<TalentArt<Tile>>> = OnceLock::new();
    let edition = edition.canonical().counts_as();
    let cache = match edition {
        Edition::Retail => &RETAIL,
        Edition::Forever => &FOREVER,
        Edition::Classic(_) | Edition::Unknown(_) => return None,
    };
    cache.get_or_init(|| TalentArt::open_in(edition)).as_ref()
}

/// The spec's painting and its pixel size, in `edition`'s art.
pub fn background(edition: Edition, spec_id: u32) -> Option<(Tile, u16, u16)> {
    available()
        .then(|| talent_art(edition)?.background(spec_id, make))
        .flatten()
}

pub fn medallion(edition: Edition, subtree_id: u32) -> Option<Tile> {
    available()
        .then(|| talent_art(edition)?.medallion(subtree_id, make))
        .flatten()
}

pub fn ring(edition: Edition) -> Option<Tile> {
    available()
        .then(|| talent_art(edition)?.ring(make))
        .flatten()
}

/// The class crest for a disc `px` device pixels across.
pub fn class_icon(class: Class, px: f32) -> Option<Tile> {
    available().then(|| images::class_icon(class, px)).flatten()
}

/// The spec's icon for a disc `px` device pixels across.
pub fn spec_icon(spec_id: u32, px: f32) -> Option<Tile> {
    available()
        .then(|| images::spec_icon(spec_id, px))
        .flatten()
}

/// A node's icon in `edition`'s spell icons, cut to its shape, grey when
/// untaken.
pub fn spell(edition: Edition, spell_id: u32, style: IconStyle, gray: bool) -> Option<Tile> {
    available()
        .then(|| images::spell_styled(edition, spell_id, style, gray))
        .flatten()
}
