//! A theme as data (spec §6.1): every colour and face a wowdps GUI draws
//! with, so no surface names a literal. The built-in `gold` IS the window
//! redesign's Tokens (`docs/design/window-redesign.html`) and the overlay's
//! palette as it has always been; more definitions are themes. The iced
//! GUI reads `GOLD` for its constants; gui-new maps whichever is active
//! onto GPUI Kit's `Theme` and its own `Look`.

use super::color::Color;
use super::metrics::{PITCHES, Pitches, SIZES, Sizes};
use super::talent_tokens::{TALENTS_FROST, TALENTS_GOLD, TalentTokens};

/// The window's surfaces and inks — the prototype's Tokens, one field each.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WindowTokens {
    /// The window and the meter (tooltip navy).
    pub ground: Color,
    /// Rail, inspector, top bar — every panel and card.
    pub surface: Color,
    /// Selection, inputs.
    pub raise: Color,
    /// Hairlines.
    pub line: Color,
    /// A floating surface's 1 px border: a menu, a sheet, a card.
    pub edge: Color,
    /// Values and names (parchment).
    pub ink: Color,
    /// Secondary words and numbers.
    pub ink_2: Color,
    /// Hints, disabled tabs, connectors: glyphs, never a figure.
    pub ink_3: Color,
    /// `ink_3`'s role for words a reader must read: lifted to clear AA.
    pub ink_3_text: Color,
    /// The interface colour: active, focus.
    pub gold: Color,
    /// Labels and column heads.
    pub gold_dim: Color,
    /// Ink drawn ON gold.
    pub gold_ink: Color,
    /// Kill, timed, heals.
    pub good: Color,
    /// Wipe, over time, a death.
    pub bad: Color,
    /// A personal best, and nothing else.
    pub legendary: Color,
    /// The pointer's wash on a row.
    pub hover: Color,
    /// A scrollbar's thumb.
    pub thumb: Color,
    /// The scrim under a modal.
    pub scrim: Color,
    /// The lighter scrim under the pull rail's drawer.
    pub rail_scrim: Color,
    /// The heat scale's middle (the R21 stack matrix): a data colour.
    pub amber: Color,
    /// A bar's empty track under a row.
    pub track: Color,
    /// A key's run against its timers: a shade brighter than `track`.
    pub par_track: Color,
    /// The inspector graph's lane under its spans (`.lane{background:
    /// rgba(255,255,255,.028)}`): fainter than a bar's track.
    pub lane_track: Color,
    /// A graph's drag-selected window while the drag is in flight.
    pub drag_fill: Color,
    /// The outline of the span under the pointer (`.span:hover{outline:
    /// 1px solid #fff}`).
    pub span_lit: Color,
    /// A bar or a disc for a player whose class is not known yet.
    pub classless: Color,
    /// An enemy with no class: an Enemies row's bar and skull disc.
    pub hostile: Color,
    /// A checked box's tick, on gold: the gold at a quarter of its light
    /// (what the iced window's checkbox draws, `primary.strong.text`).
    pub check: Color,
    /// The selected row's name: a step brighter than ink (`.trow.sel .nm`).
    pub name_lit: Color,
    /// A field's selected text.
    pub selection: Color,
}

/// The overlay's palette: white and dim on its dark panel, and the yellow
/// that means live, Σ, crit and "look here" there (and nowhere in the
/// window). Each role is named for what it marks; the values are the
/// iced overlay's, pixel for pixel (`docs/plan-gui-new.md` phase 2).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct OverlayTokens {
    /// The panel's and the tab's fill, drawn at the alpha each surface
    /// asks for (0.92 the panel, 0.85 the tab).
    pub panel: Color,
    /// The panel's and the tab's 1 px border.
    pub edge: Color,
    /// A floating card's fill (the options card, the view menu).
    pub card: Color,
    /// A card's 1 px border, and an idle disc's ring.
    pub card_edge: Color,
    /// Words drawn with no colour of their own: names, labels, the clock.
    pub text: Color,
    /// Values, the watched disc's ring.
    pub ink: Color,
    /// Captions, secondary words, an idle control.
    pub dim: Color,
    /// A row's rate column.
    pub rate: Color,
    pub good: Color,
    pub bad: Color,
    /// Live, Σ, crit, "look here", a control that is on.
    pub yellow: Color,
    /// The pointer's wash on a row.
    pub hover: Color,
    /// The pointer's wash on a menu item.
    pub menu_hover: Color,
    /// A bar's empty track.
    pub track: Color,
    /// Health left, on a recap line.
    pub health: Color,
    /// An enemy with no class.
    pub hostile: Color,
    /// A control that is on: the options card's checked box (iced's
    /// TokyoNight primary, which the iced overlay's controls wore).
    pub control: Color,
    /// The mark drawn on `control`.
    pub control_ink: Color,
    /// A row with no known class.
    pub classless: Color,
    /// A graph's plot area.
    pub plot: Color,
    /// A graph's baseline, so an empty graph still reads as one.
    pub rule: Color,
    /// A graph's time cursor, and its icon's ring when hovered is `ink`.
    pub cursor: Color,
    /// A drag's selection on a graph, and its two edges.
    pub select: Color,
    pub select_edge: Color,
    /// A stat card's fill and border (the ability drill's numbers).
    pub stat_card: Color,
    pub stat_card_edge: Color,
    /// A list's scrollbar: its rail and its thumb (iced's TokyoNight
    /// scroller, which the iced overlay's lists wore).
    pub rail: Color,
    pub thumb: Color,
    /// A recap line's health strip, under the health left.
    pub health_track: Color,
}

/// The families a theme draws in. A family must be named: GPUI resolves
/// no generic names (`sans-serif`, `monospace`; spike S6).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Faces {
    /// The window's names and numbers.
    pub ui: &'static str,
    /// Encounter titles and the wordmark.
    pub title: &'static str,
    /// The overlay's words.
    pub overlay: &'static str,
    /// The overlay's numbers.
    pub overlay_num: &'static str,
}

/// One theme.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Def {
    /// What config `theme` spells it as.
    pub name: &'static str,
    /// Dark ground, light ink. Every built-in is dark today.
    pub dark: bool,
    pub window: WindowTokens,
    pub overlay: OverlayTokens,
    /// The talent viewer's tree and tooltip.
    pub talents: TalentTokens,
    pub faces: Faces,
    /// The type scale.
    pub size: Sizes,
    /// The row pitches.
    pub pitch: Pitches,
    /// A control's corner radius (a chip's is a pill whatever this says).
    pub radius: f32,
}

const GOLD_TOKEN: Color = Color::hex(0xF2C14B);

/// The window redesign's Tokens and the overlay as it has always looked.
pub const GOLD: Def = Def {
    name: "gold",
    dark: true,
    window: WindowTokens {
        ground: Color::hex(0x0A0E18),
        surface: Color::hex(0x10162A),
        raise: Color::hex(0x182137),
        line: Color::hex(0x1E2740),
        edge: Color::hex(0x33405F),
        ink: Color::hex(0xEDE9DF),
        ink_2: Color::hex(0xA6ACC2),
        ink_3: Color::hex(0x6C7492),
        ink_3_text: Color::hex(0x8089AA),
        gold: GOLD_TOKEN,
        gold_dim: Color::hex(0xBD9A45),
        gold_ink: Color::hex(0x1B1406),
        good: Color::hex(0x58D08A),
        bad: Color::hex(0xFF5C63),
        legendary: Color::hex(0xFF8000),
        hover: Color::rgba8(150, 170, 255, 0.055),
        thumb: Color::hex(0x26304A),
        scrim: Color::rgba8(4, 6, 12, 0.55),
        rail_scrim: Color::rgba8(4, 6, 12, 0.45),
        amber: Color::hex(0xE3B341),
        track: Color::rgba(1.0, 1.0, 1.0, 0.04),
        par_track: Color::rgba(1.0, 1.0, 1.0, 0.06),
        lane_track: Color::rgba(1.0, 1.0, 1.0, 0.028),
        drag_fill: Color::rgba(1.0, 1.0, 1.0, 0.10),
        span_lit: Color::WHITE,
        selection: GOLD_TOKEN.alpha(0.3),
        classless: Color::rgb(0.42, 0.44, 0.52),
        hostile: Color::rgb(0.80, 0.30, 0.32),
        name_lit: Color::WHITE,
        check: Color::hex(0x3B2D0C),
    },
    overlay: OverlayTokens {
        panel: Color::hex(0x16161E),
        edge: Color::rgba(1.0, 1.0, 1.0, 0.15),
        card: Color::rgba(0.09, 0.10, 0.14, 0.97),
        card_edge: Color::rgba(1.0, 1.0, 1.0, 0.25),
        // iced's TokyoNight `text`, every overlay word without a colour.
        text: Color::hex(0x9AA5CE),
        ink: Color::WHITE,
        dim: Color::rgb(0.55, 0.57, 0.62),
        rate: Color::rgba(1.0, 1.0, 1.0, 0.75),
        good: Color::rgb(0.60, 0.76, 0.47),
        bad: Color::rgb(0.88, 0.42, 0.46),
        yellow: Color::rgb(0.90, 0.75, 0.48),
        hover: Color::rgba(1.0, 1.0, 1.0, 0.07),
        menu_hover: Color::rgba(1.0, 1.0, 1.0, 0.14),
        track: Color::rgba(1.0, 1.0, 1.0, 0.04),
        health: Color::rgb(0.35, 0.78, 0.42),
        hostile: Color::rgb(0.80, 0.30, 0.32),
        control: Color::hex(0x2AC3DE),
        control_ink: Color::hex(0x1A1B26),
        classless: Color::rgb(0.42, 0.44, 0.52),
        plot: Color::rgba(1.0, 1.0, 1.0, 0.04),
        rule: Color::rgba(1.0, 1.0, 1.0, 0.15),
        cursor: Color::rgba(1.0, 1.0, 1.0, 0.45),
        select: Color::rgba(1.0, 1.0, 1.0, 0.12),
        select_edge: Color::rgba(1.0, 1.0, 1.0, 0.6),
        stat_card: Color::rgba(1.0, 1.0, 1.0, 0.05),
        stat_card_edge: Color::rgba(1.0, 1.0, 1.0, 0.12),
        health_track: Color::rgba(1.0, 1.0, 1.0, 0.06),
        rail: Color::hex(0x303249),
        thumb: Color::hex(0x494B6F),
    },
    talents: TALENTS_GOLD,
    faces: Faces {
        ui: crate::fonts::UI_FAMILY,
        title: crate::fonts::TITLE_FAMILY,
        overlay: "Noto Sans",
        overlay_num: "Noto Sans Mono",
    },
    size: SIZES,
    pitch: PITCHES,
    radius: 6.0,
};

const FROST_TOKEN: Color = Color::hex(0x8FD0F2);

/// A cold night: slate-blue panels, frost-white ink and an icy accent where
/// `gold` has the game's gold. Outcomes, legendary and the overlay keep
/// their meanings and their colours.
pub const FROST: Def = Def {
    name: "frost",
    dark: true,
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
        gold: FROST_TOKEN,
        gold_dim: Color::hex(0x7FAFC9),
        gold_ink: Color::hex(0x061620),
        good: Color::hex(0x58D08A),
        bad: Color::hex(0xFF5C63),
        legendary: Color::hex(0xFF8000),
        hover: Color::rgba8(150, 210, 255, 0.06),
        thumb: Color::hex(0x243446),
        scrim: Color::rgba8(3, 7, 12, 0.55),
        rail_scrim: Color::rgba8(3, 7, 12, 0.45),
        amber: Color::hex(0xE3B341),
        track: Color::rgba(1.0, 1.0, 1.0, 0.04),
        par_track: Color::rgba(1.0, 1.0, 1.0, 0.06),
        lane_track: GOLD.window.lane_track,
        drag_fill: GOLD.window.drag_fill,
        span_lit: GOLD.window.span_lit,
        selection: FROST_TOKEN.alpha(0.3),
        classless: GOLD.window.classless,
        hostile: GOLD.window.hostile,
        name_lit: GOLD.window.name_lit,
        check: Color::hex(0x23333B),
    },
    overlay: GOLD.overlay,
    talents: TALENTS_FROST,
    faces: GOLD.faces,
    size: SIZES,
    pitch: PITCHES,
    radius: 6.0,
};

/// Every built-in theme, `gold` first (the default).
pub const THEMES: [&Def; 2] = [&GOLD, &FROST];

/// The built-in theme config `theme` names; an unknown name is not an
/// error, it is `gold`.
pub fn def_named(name: &str) -> &'static Def {
    THEMES
        .into_iter()
        .find(|d| d.name.eq_ignore_ascii_case(name.trim()))
        .unwrap_or(&GOLD)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::color::{AA_CONTRAST, contrast};

    /// Every built-in theme's words read on every fill a row can wear —
    /// ground, panel, the pointer's wash, the selection — and the faint ink
    /// does not, which is why its text grade exists. Gold-dim labels read
    /// on the grounds, and ink on the accent reads.
    #[test]
    fn every_theme_reads() {
        for def in THEMES {
            let w = def.window;
            let fills = [
                ("ground", w.ground),
                ("surface", w.surface),
                ("hover", w.hover.over(w.ground)),
                ("raise", w.raise),
            ];
            for (name, ink) in [
                ("ink", w.ink),
                ("ink_2", w.ink_2),
                ("ink_3_text", w.ink_3_text),
            ] {
                for (fill, bg) in fills {
                    let c = contrast(ink, bg);
                    assert!(
                        c >= AA_CONTRAST,
                        "{}: {name} on {fill} is {c:.2}:1",
                        def.name
                    );
                }
            }
            for (fill, bg) in [("ground", w.ground), ("surface", w.surface)] {
                let c = contrast(w.gold_dim, bg);
                assert!(
                    c >= AA_CONTRAST,
                    "{}: gold_dim on {fill} is {c:.2}:1",
                    def.name
                );
            }
            let c = contrast(w.gold_ink, w.gold);
            assert!(
                c >= AA_CONTRAST,
                "{}: ink on the accent is {c:.2}:1",
                def.name
            );
            assert!(def.dark, "{}: every built-in is dark today", def.name);
        }
    }

    #[test]
    fn themes_are_named_and_an_unknown_name_is_gold() {
        assert_eq!(THEMES[0], &GOLD, "gold is the default");
        for def in THEMES {
            assert_eq!(def_named(def.name), def);
            assert_eq!(def_named(&def.name.to_uppercase()), def);
        }
        assert_eq!(def_named("purple"), &GOLD);
        let names: Vec<_> = THEMES.iter().map(|d| d.name).collect();
        let mut unique = names.clone();
        unique.dedup();
        assert_eq!(names, unique);
    }
}
