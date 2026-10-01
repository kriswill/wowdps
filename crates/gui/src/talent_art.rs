//! The talent viewer's artwork as iced handles: gui-logic's reader of the
//! per-machine `talent-art.bin` (`wowdps_gui_logic::talent_art`), held once
//! for the process, each painting made into one iced handle on first use
//! and cloned ever after. No cache means plain panels.

use std::sync::OnceLock;

use iced::widget::image::Handle;
use wowdps_gui_logic::talent_art::TalentArt;

use crate::icons::make;

fn cache() -> Option<&'static TalentArt<Handle>> {
    static CACHE: OnceLock<Option<TalentArt<Handle>>> = OnceLock::new();
    CACHE.get_or_init(TalentArt::open).as_ref()
}

/// The spec's whole background painting with its pixel size: class art on
/// its left half, spec art on its right. Drawn as one full-width backdrop
/// under the trees (the caller needs the aspect for cover-fitting).
pub(crate) fn background(spec_id: u32) -> Option<(Handle, u16, u16)> {
    cache()?.background(spec_id, make)
}

/// A hero tree's round medallion.
pub(crate) fn medallion(subtree_id: u32) -> Option<Handle> {
    cache()?.medallion(subtree_id, make)
}

/// The golden ring the game frames the medallion with.
pub(crate) fn ring() -> Option<Handle> {
    cache()?.ring(make)
}
