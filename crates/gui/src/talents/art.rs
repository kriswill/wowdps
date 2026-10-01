//! The viewer's art, all from the per-machine caches: the spec's
//! background painting, the hero tree's medallion and the golden ring
//! (`talent-art.bin`), the class and spec crests (`class-icons.bin`) and
//! the node icons cut to their shapes (`spell-icons.bin`). Without a cache
//! every lookup is `None` and the viewer draws plain panels; while a test
//! has put its own dataset on the thread it consults none, so every render
//! exercises that path.

use std::sync::OnceLock;

use wowdps_gui_logic::spell_icons::IconStyle;
use wowdps_gui_logic::talent_art::TalentArt;
use wowdps_gui_logic::talents as logic;
use wowdps_model::Class;

use crate::images::{self, Tile, make};

fn available() -> bool {
    !logic::under_test()
}

fn talent_art() -> Option<&'static TalentArt<Tile>> {
    static CACHE: OnceLock<Option<TalentArt<Tile>>> = OnceLock::new();
    CACHE.get_or_init(TalentArt::open).as_ref()
}

/// The spec's painting and its pixel size.
pub fn background(spec_id: u32) -> Option<(Tile, u16, u16)> {
    available()
        .then(|| talent_art()?.background(spec_id, make))
        .flatten()
}

pub fn medallion(subtree_id: u32) -> Option<Tile> {
    available()
        .then(|| talent_art()?.medallion(subtree_id, make))
        .flatten()
}

pub fn ring() -> Option<Tile> {
    available().then(|| talent_art()?.ring(make)).flatten()
}

pub fn class_icon(class: Class) -> Option<Tile> {
    available().then(|| images::class_icon(class)).flatten()
}

pub fn spec_icon(spec_id: u32) -> Option<Tile> {
    available().then(|| images::spec_icon(spec_id)).flatten()
}

/// A node's icon, cut to its shape, grey when untaken.
pub fn spell(spell_id: u32, style: IconStyle, gray: bool) -> Option<Tile> {
    available()
        .then(|| images::spell_styled(spell_id, style, gray))
        .flatten()
}
