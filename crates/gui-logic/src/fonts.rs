//! The bundled type: the OFL faces under `crates/gui-logic/fonts/`
//! (provenance, pinned upstream commits and the tabular bakes in its
//! `README.md`) and the family names they register under. Assets, not
//! dependencies. The GUI registers every one at start, whatever the theme,
//! so a switch never waits on a face; a theme names the ones it draws in
//! (`theme::Faces`), and a config may name any face installed instead.

/// Barlow Semi Condensed with its tabular figures baked into the default
/// digits, renamed so an installed proportional copy is never the face
/// picked: `navy`'s names and numbers in one voice, every column lining up.
pub const BARLOW: &str = "Barlow Semi Condensed Tabular";
/// `navy`'s encounter titles and wordmark, and only those: the nod to the
/// game's Friz Quadrata.
pub const MARCELLUS: &str = "Marcellus";
/// Saira at width 80 (between its Condensed and Semi Condensed), tabular
/// figures baked in the same way: `onyx`'s instrument lettering, at the
/// width that sets Barlow's measure, so no column moves.
pub const SAIRA: &str = "Saira Tabular";
/// `onyx`'s titles and wordmark: an extended face, the engraving on a
/// bezel.
pub const MICHROMA: &str = "Michroma";

/// `navy`'s faces under their old names.
pub const UI_FAMILY: &str = BARLOW;
pub const TITLE_FAMILY: &str = MARCELLUS;

/// Every face the GUI loads: Barlow and Saira at 400, 500 and 600, then
/// Marcellus and Michroma.
pub const FONTS: [&[u8]; 8] = [
    include_bytes!("../fonts/BarlowSemiCondensedTabular-Regular.ttf"),
    include_bytes!("../fonts/BarlowSemiCondensedTabular-Medium.ttf"),
    include_bytes!("../fonts/BarlowSemiCondensedTabular-SemiBold.ttf"),
    include_bytes!("../fonts/Marcellus-Regular.ttf"),
    include_bytes!("../fonts/SairaTabular-Regular.ttf"),
    include_bytes!("../fonts/SairaTabular-Medium.ttf"),
    include_bytes!("../fonts/SairaTabular-SemiBold.ttf"),
    include_bytes!("../fonts/Michroma-Regular.ttf"),
];
