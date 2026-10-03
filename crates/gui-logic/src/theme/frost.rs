//! `frost`: a cold night — slate-blue panels, frost-white ink and an icy
//! accent where `navy` has the game's gold. Outcomes, legendary and the
//! overlay keep their meanings and their colours.

use super::color::Color;
use super::defs::{Def, WindowTokens};
use super::navy::NAVY;
use super::talent_tokens::TALENTS_FROST;

const FROST_TOKEN: Color = Color::hex(0x8FD0F2);

pub const FROST: Def = Def {
    name: "frost",
    label: "Frost",
    accent_label: "Frost",
    window: WindowTokens {
        ground: Color::hex(0x0A1016),
        surface: Color::hex(0x101923),
        raise: Color::hex(0x172433),
        line: Color::hex(0x1D2B3B),
        edge: Color::hex(0x31465C),
        ink: Color::hex(0xE6EEF4),
        ink_2: Color::hex(0xA2B3C4),
        ink_3: Color::hex(0x687C90),
        ink_3_text: Color::hex(0x7F94A8),
        accent: FROST_TOKEN,
        label_ink: Color::hex(0x7FAFC9),
        accent_ink: Color::hex(0x061620),
        hover: Color::rgba8(150, 210, 255, 0.06),
        thumb: Color::hex(0x243446),
        scrim: Color::rgba8(3, 7, 12, 0.55),
        rail_scrim: Color::rgba8(3, 7, 12, 0.45),
        selection: FROST_TOKEN.alpha(0.3),
        check: Color::hex(0x23333B),
        glass: Color::hex(0x101923),
        bracket: Color::hex(0x31465C),
        dial: Color::hex(0x687C90),
        ..NAVY.window
    },
    talents: TALENTS_FROST,
    ..NAVY
};
