//! The talent viewer's palette: the game's own talent frame — gold paths
//! and frames, green for what can be taken, teal for what was granted,
//! the dark game tooltip with its white, grey, yellow and blue words.
//! `gold`'s values are the iced viewer's, pixel for pixel; a theme may
//! recolour the chrome (frames, paths, the backing plates) but the
//! tooltip's words keep the game's meanings.

use super::color::Color;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TalentTokens {
    /// Taken frames, lit paths and their arrows, pane titles, a full
    /// pane's points.
    pub gold: Color,
    /// An unlit path between two nodes.
    pub path: Color,
    /// The dark plate under a shaped icon, so its clipped corners read as
    /// the shape over bright background art.
    pub backing: Color,
    /// A node's stand-in when no icon cache exists, drawn at a low alpha.
    pub blank: Color,
    /// A frame granted for free.
    pub granted: Color,
    /// A frame that can be taken now.
    pub available: Color,
    /// A frame out of reach.
    pub locked: Color,
    /// The rank badge's plate and words.
    pub badge: Color,
    pub badge_ink: Color,
    /// The ring round the hovered node, and round a hovered picker option.
    pub hover_ring: Color,
    /// The open choice picker's plate, and an option ring at rest.
    pub picker: Color,
    pub option_ring: Color,
    /// The tooltip's box and border.
    pub tip: Color,
    pub tip_edge: Color,
    /// Its words: names and costs, kinds and ranks, the description, a
    /// trailing restriction, a rank not reached.
    pub tip_ink: Color,
    pub tip_meta: Color,
    pub tip_desc: Color,
    pub tip_note: Color,
    pub tip_unreached: Color,
    /// The wash over the spec's background painting, so the trees read.
    pub veil: Color,
    /// The hero tree's backplate and its border.
    pub plate: Color,
    pub plate_edge: Color,
    /// A chip's fill and border, at rest and pressed.
    pub chip: Color,
    pub chip_on: Color,
    pub chip_edge: Color,
    pub chip_edge_on: Color,
}

/// The iced viewer's palette: the game's talent gold toned for a dark
/// theme (close to its `ffd100`), its tooltip navy and its words.
pub const TALENTS_GOLD: TalentTokens = TalentTokens {
    gold: Color::rgb(0.94, 0.78, 0.31),
    path: Color::rgba(0.75, 0.78, 0.85, 0.22),
    backing: Color::rgba(0.0, 0.0, 0.0, 0.60),
    blank: Color::WHITE,
    granted: Color::rgb(0.35, 0.80, 0.75),
    available: Color::rgb(0.30, 0.85, 0.35),
    locked: Color::rgba(0.72, 0.74, 0.80, 0.35),
    badge: Color::rgba(0.0, 0.0, 0.0, 0.88),
    badge_ink: Color::WHITE,
    hover_ring: Color::WHITE,
    picker: Color::rgba(0.04, 0.04, 0.08, 0.96),
    option_ring: Color::rgba(0.85, 0.87, 0.92, 0.55),
    tip: Color::rgba(0.02, 0.03, 0.12, 0.97),
    tip_edge: Color::rgba(1.0, 1.0, 1.0, 0.30),
    tip_ink: Color::WHITE,
    tip_meta: Color::rgb(0.62, 0.64, 0.70),
    tip_desc: Color::rgb(1.0, 0.84, 0.25),
    tip_note: Color::rgb(0.42, 0.62, 1.0),
    tip_unreached: Color::rgb(0.55, 0.58, 0.64),
    veil: Color::rgba(0.02, 0.02, 0.04, 0.35),
    plate: Color::rgba(0.0, 0.0, 0.0, 0.35),
    plate_edge: Color::rgba(1.0, 1.0, 1.0, 0.12),
    chip: Color::rgba(1.0, 1.0, 1.0, 0.05),
    chip_on: Color::rgba(1.0, 1.0, 1.0, 0.12),
    chip_edge: Color::rgba(1.0, 1.0, 1.0, 0.15),
    chip_edge_on: Color::rgba(1.0, 1.0, 1.0, 0.4),
};

/// `frost`'s tree: the taken paths and frames in its icy accent, the
/// plates a cold slate; what can be taken, what was granted and the
/// tooltip's words keep the game's colours.
pub const TALENTS_FROST: TalentTokens = TalentTokens {
    gold: Color::hex(0x8FD0F2),
    path: Color::rgba(0.70, 0.80, 0.90, 0.22),
    backing: Color::rgba(0.01, 0.03, 0.06, 0.62),
    picker: Color::rgba(0.03, 0.06, 0.10, 0.96),
    option_ring: Color::rgba(0.80, 0.90, 0.98, 0.55),
    tip: Color::rgba(0.02, 0.05, 0.10, 0.97),
    veil: Color::rgba(0.01, 0.03, 0.06, 0.40),
    plate: Color::rgba(0.01, 0.04, 0.08, 0.40),
    ..TALENTS_GOLD
};

#[cfg(test)]
mod tests {
    use crate::theme::{AA_CONTRAST, Color, THEMES, contrast};

    /// Every theme's tooltip words read on its tooltip box, and its rank
    /// badges' on their plate (each composited over black, the darkest
    /// thing under them).
    #[test]
    fn every_theme_s_talent_words_read() {
        for def in THEMES {
            let t = def.talents;
            let tip = t.tip.over(Color::BLACK);
            for (name, ink) in [
                ("tip_ink", t.tip_ink),
                ("tip_meta", t.tip_meta),
                ("tip_desc", t.tip_desc),
                ("tip_note", t.tip_note),
                ("tip_unreached", t.tip_unreached),
            ] {
                let c = contrast(ink, tip);
                assert!(
                    c >= AA_CONTRAST,
                    "{}: {name} on the tooltip is {c:.2}:1",
                    def.name
                );
            }
            let badge = t.badge.over(Color::BLACK);
            assert!(contrast(t.badge_ink, badge) >= AA_CONTRAST, "{}", def.name);
        }
    }
}
