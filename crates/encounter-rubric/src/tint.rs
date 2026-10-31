//! The rubric's colors: the names a place's, a ground's, a line's, a
//! volley's, a flash's and a range's `tint`, and a mark's `ring` and `dot`,
//! are written in. A reader paints each in its theme's own hue for it;
//! `crate::check` refuses any other name, so a misspelled color is found
//! in review rather than drawn as the foe's.

/// Every color a rubric may name.
pub const NAMES: &[&str] = &[
    "green", "red", "blue", "orange", "teal", "cyan", "purple", "pink", "gold", "yellow", "amber",
    "white", "ice",
];

/// Whether `name` is one of the rubric's colors (`NAMES`), exactly as a
/// file writes it.
pub fn known(name: &str) -> bool {
    NAMES.contains(&name)
}
