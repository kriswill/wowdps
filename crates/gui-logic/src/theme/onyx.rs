//! `onyx`: a black-dial chronograph with a pilot's HUD. True black is the
//! dial, near-white the lume, a ramp of neutral greys the engraving; the
//! only chroma is data — class colours, the outcomes, a personal best, the
//! validated graph hues. What floats over the dial is smoked glass, the
//! instruments wear reticle brackets and a fine tick track, the inspector's
//! player sits in a sub-dial, and every corner is machined. Its faces are
//! Saira at width 80 with tabular figures, "Saira Tabular" (instrument
//! lettering at the measure of `navy`'s Barlow, so no column moves) and Michroma, an extended
//! face, for the names engraved on the bezel: titles and the wordmark.

use super::color::{Color, INK_DARK};
use super::defs::{DataTokens, Def, Effects, Faces, OverlayTokens, WindowTokens};
use super::metrics::{Bars, PITCHES, SHADOWS, SIZES, Shape, Sizes};
use super::navy::DATA;
use super::talent_tokens::TALENTS_ONYX;

/// Lume: values, names, the accent's neighbour.
const LUME: Color = Color::hex(0xF2F3F5);
/// The kill green: a pale lume, the one green the theme has.
const GOOD: Color = Color::hex(0x73E0A9);

pub const ONYX: Def = Def {
    name: "onyx",
    label: "Onyx",
    accent_label: "White",
    dark: true,
    window: WindowTokens {
        ground: Color::hex(0x000000),
        surface: Color::hex(0x0A0B0D),
        raise: Color::hex(0x17191D),
        line: Color::hex(0x24272C),
        edge: Color::hex(0x34373D),
        ink: LUME,
        ink_2: Color::hex(0xB8BCC3),
        ink_3: Color::hex(0x5C6168),
        ink_3_text: Color::hex(0x868B93),
        accent: Color::WHITE,
        label_ink: Color::hex(0x9CA1A9),
        accent_ink: Color::hex(0x050506),
        good: GOOD,
        bad: Color::hex(0xFF5D5D),
        legendary: Color::hex(0xFF8000),
        hover: Color::rgba(1.0, 1.0, 1.0, 0.045),
        thumb: Color::hex(0x26282D),
        scrim: Color::rgba(0.0, 0.0, 0.0, 0.62),
        rail_scrim: Color::rgba(0.0, 0.0, 0.0, 0.5),
        // The heat scale runs green, salmon, red: no semantic yellow.
        amber: Color::hex(0xFF9B73),
        track: Color::rgba(1.0, 1.0, 1.0, 0.05),
        par_track: Color::rgba(1.0, 1.0, 1.0, 0.075),
        lane_track: Color::rgba(1.0, 1.0, 1.0, 0.03),
        drag_fill: Color::rgba(1.0, 1.0, 1.0, 0.09),
        span_lit: Color::WHITE,
        classless: Color::hex(0x6A6E76),
        hostile: Color::hex(0xB8474B),
        check: Color::hex(0x000000),
        name_lit: Color::WHITE,
        selection: Color::rgba(1.0, 1.0, 1.0, 0.22),
        shade: Color::BLACK,
        health_track: Color::rgba(1.0, 1.0, 1.0, 0.07),
        on_class: INK_DARK.alpha(0.85),
        // A death is told by its skull and its words; the marks recede.
        death_line: Color::hex(0x5C6168),
        death_hatch: Color::rgba(1.0, 1.0, 1.0, 0.375),
        par_timed: Color::hex(0xB8BCC3),
        par_over: Color::hex(0xFF5D5D),
        drawer_edge: Color::hex(0x34373D),
        glass: Color::rgba8(14, 15, 18, 0.98),
        glass_sheen: Color::rgba(1.0, 1.0, 1.0, 0.08),
        glass_rim: Color::rgba(1.0, 1.0, 1.0, 0.30),
        bracket: Color::hex(0x6A6F77),
        dial: Color::hex(0x6E737B),
    },
    overlay: OverlayTokens {
        panel: Color::hex(0x000000),
        edge: Color::rgba(1.0, 1.0, 1.0, 0.14),
        card: Color::rgba(0.04, 0.042, 0.05, 0.96),
        card_edge: Color::rgba(1.0, 1.0, 1.0, 0.22),
        text: Color::hex(0xB4B8BF),
        ink: Color::hex(0xEDEFF2),
        dim: Color::hex(0x8A8F97),
        rate: Color::rgba(1.0, 1.0, 1.0, 0.72),
        good: GOOD,
        bad: Color::hex(0xFF6B6B),
        // "Look here" is pure white, a step over the values' lume.
        yellow: Color::WHITE,
        hover: Color::rgba(1.0, 1.0, 1.0, 0.06),
        menu_hover: Color::rgba(1.0, 1.0, 1.0, 0.12),
        track: Color::rgba(1.0, 1.0, 1.0, 0.05),
        health: GOOD,
        hostile: Color::hex(0xB8474B),
        control: Color::WHITE,
        control_ink: Color::hex(0x000000),
        classless: Color::hex(0x6A6E76),
        plot: Color::rgba(1.0, 1.0, 1.0, 0.035),
        rule: Color::rgba(1.0, 1.0, 1.0, 0.16),
        cursor: Color::rgba(1.0, 1.0, 1.0, 0.5),
        select: Color::rgba(1.0, 1.0, 1.0, 0.10),
        select_edge: Color::rgba(1.0, 1.0, 1.0, 0.6),
        stat_card: Color::rgba(1.0, 1.0, 1.0, 0.04),
        stat_card_edge: Color::rgba(1.0, 1.0, 1.0, 0.12),
        health_track: Color::rgba(1.0, 1.0, 1.0, 0.06),
        // Over a heal's or a hit's bar the words need the brighter inks.
        recap_ink: Color::hex(0xEDEFF2),
        recap_hp: Color::hex(0xB4B8BF),
        rail: Color::hex(0x1E2024),
        thumb: Color::hex(0x44474E),
        pip: Color::rgba(1.0, 1.0, 1.0, 0.08),
        pip_kill: GOOD.alpha(0.18),
        pip_wipe: Color::hex(0xFF6B6B).alpha(0.20),
        pip_live: Color::rgba(1.0, 1.0, 1.0, 0.32),
        pip_lit: Color::rgba(1.0, 1.0, 1.0, 0.92),
        picked: Color::WHITE,
        on_bar: Color::rgba(0.0, 0.0, 0.0, 0.85),
        glass_sheen: Color::rgba(1.0, 1.0, 1.0, 0.05),
        glass_rim: Color::rgba(1.0, 1.0, 1.0, 0.26),
    },
    talents: TALENTS_ONYX,
    // The validated hues stand inside the player's whole curve: the rest is
    // graphite smoke under a steel line, the silhouette, never a seventh
    // entity in a colour of its own.
    data: DataTokens {
        stack_other: Color::hex(0x2A2D32),
        stack_other_edge: Color::hex(0xA4A8AF),
        ..DATA
    },
    faces: Faces {
        ui: crate::fonts::SAIRA,
        title: crate::fonts::MICHROMA,
        overlay: crate::fonts::SAIRA,
        overlay_num: crate::fonts::SAIRA,
    },
    // Michroma is wide: its titles are set smaller to stand where
    // Marcellus's did.
    size: Sizes {
        encounter: 19.0,
        encounter_narrow: 14.0,
        home_title: 20.0,
        home_place: 16.0,
        mark: 14.0,
        ..SIZES
    },
    pitch: PITCHES,
    shape: Shape {
        scale: 0.3,
        chip: 2.0,
    },
    // On black a faded class colour turns to mud: the bars show more of it.
    bars: Bars {
        rest_from: 0.45,
        rest_to: 0.85,
        lit_from: 0.75,
        lit_to: 1.0,
        list: 0.85,
    },
    effects: Effects {
        glass: true,
        brackets: true,
        dial: true,
        fine_ticks: true,
        quiet_press: true,
    },
    shadows: SHADOWS,
};
