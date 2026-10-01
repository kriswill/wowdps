//! The class/spec icons as iced handles: gui-logic's reader of the
//! per-machine `class-icons.bin` (`wowdps_gui_logic::icons`), held once for
//! the process, each tile made into one iced handle on first use and that
//! handle cloned ever after — a clone keeps the image id, so iced uploads
//! a crest once, not every frame. No cache means `None` everywhere and the
//! UI's drawn class-coloured discs.

use std::sync::OnceLock;

use iced::widget::image::Handle;
use wowdps_gui_logic::icons::ClassIcons;
use wowdps_gui_logic::lazy_tiles::Rgba;
use wowdps_model::Class;

fn cache() -> Option<&'static ClassIcons<Handle>> {
    static CACHE: OnceLock<Option<ClassIcons<Handle>>> = OnceLock::new();
    CACHE.get_or_init(ClassIcons::open).as_ref()
}

/// The iced handle for a tile, built once per tile by the reader's memo.
pub(crate) fn make(t: Rgba) -> Handle {
    Handle::from_rgba(t.w, t.h, t.pixels)
}

/// The class crest (interface/icons/classicon_*), or `None` without a cache.
pub(crate) fn class_handle(class: Class) -> Option<Handle> {
    cache()?.class(class, make)
}

/// The spec's own icon, by Blizzard specID (ChrSpecialization).
pub(crate) fn spec_handle(spec_id: u32) -> Option<Handle> {
    cache()?.spec(spec_id, make)
}
