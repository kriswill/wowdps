//! The window's bundled type: the OFL faces under `crates/gui-logic/fonts/`
//! (provenance, pinned upstream commits and the tabular bake in its
//! `README.md`) and the family names they register under. Assets, not
//! dependencies. The GUI registers them at start; the overlay draws in the
//! system UI face, never these.

/// Barlow Semi Condensed with its tabular figures baked into the default
/// digits, renamed so an installed proportional copy is never the face
/// picked: names and numbers in one voice, every column lining up.
pub const UI_FAMILY: &str = "Barlow Semi Condensed Tabular";
/// Encounter titles and the wordmark, and only those: the nod to the
/// game's Friz Quadrata.
pub const TITLE_FAMILY: &str = "Marcellus";

/// Every face the window loads: Barlow at 400, 500 and 600, then Marcellus.
pub const FONTS: [&[u8]; 4] = [
    include_bytes!("../fonts/BarlowSemiCondensedTabular-Regular.ttf"),
    include_bytes!("../fonts/BarlowSemiCondensedTabular-Medium.ttf"),
    include_bytes!("../fonts/BarlowSemiCondensedTabular-SemiBold.ttf"),
    include_bytes!("../fonts/Marcellus-Regular.ttf"),
];
