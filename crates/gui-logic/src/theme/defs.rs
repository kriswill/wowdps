//! A theme as data (spec §6.1): every colour, face, size and corner a
//! wowdps GUI draws with, so no surface names a literal. The built-in
//! `navy` IS the window redesign's Tokens (`docs/design/window-redesign.html`)
//! and the overlay's palette as it has always been; `onyx` and `frost` are
//! more, and a config may override any of their tokens or define its own
//! themes on top of them ([`super::Registry`]). The GUI maps whichever is
//! active onto GPUI Kit's `Theme` and its own `Look`
//! (`crates/gui/src/theme.rs`).
//!
//! Every token group is written by [`tokens!`], so each field is a key a
//! config's `[themes.<name>.<group>]` table can set, by the same name.

use super::color::Color;
use super::metrics::{Bars, Pitches, Shadows, Shape, Sizes};
use super::talent_tokens::TalentTokens;
use super::text::Text;
use super::tokens::tokens;

pub use super::frost::FROST;
pub use super::navy::NAVY;
pub use super::onyx::ONYX;

tokens! {
/// The window's surfaces and inks — the prototype's Tokens, one field each.
pub struct WindowTokens: Color {
    /// The window and the meter.
    ground,
    /// Rail, inspector, top bar — every panel and card.
    surface,
    /// Selection, inputs.
    raise,
    /// Hairlines.
    line,
    /// A floating surface's 1 px border: a menu, a sheet, a card.
    edge,
    /// Values and names.
    ink,
    /// Secondary words and numbers.
    ink_2,
    /// Hints, disabled tabs, connectors: glyphs, never a figure.
    ink_3,
    /// `ink_3`'s role for words a reader must read: lifted to clear AA.
    ink_3_text,
    /// The interface colour: active, focus — the chrome when it is the
    /// theme's own (`chrome = "theme"`).
    accent,
    /// Labels and column heads (not to be confused with a theme's `label`,
    /// its name in the ⚙ card).
    label_ink,
    /// Ink drawn ON the accent.
    accent_ink,
    /// Kill, timed, heals.
    good,
    /// Wipe, over time, a death.
    bad,
    /// A personal best, and nothing else.
    legendary,
    /// The pointer's wash on a row.
    hover,
    /// A scrollbar's thumb.
    thumb,
    /// The scrim under a modal.
    scrim,
    /// The lighter scrim under the pull rail's drawer.
    rail_scrim,
    /// The heat scale's middle (the R21 stack matrix): a data colour.
    amber,
    /// A bar's empty track under a row.
    track,
    /// A key's run against its timers: a shade brighter than `track`.
    par_track,
    /// The inspector graph's lane under its spans (`.lane{background:
    /// rgba(255,255,255,.028)}`): fainter than a bar's track.
    lane_track,
    /// A graph's drag-selected window while the drag is in flight.
    drag_fill,
    /// The outline of the span under the pointer (`.span:hover{outline:
    /// 1px solid #fff}`).
    span_lit,
    /// A bar or a disc for a player whose class is not known yet.
    classless,
    /// An enemy with no class: an Enemies row's bar and skull disc.
    hostile,
    /// A checked box's tick, on the accent.
    check,
    /// The selected row's name: a step brighter than ink (`.trow.sel .nm`).
    name_lit,
    /// A field's selected text.
    selection,
    /// A shadow's ink under a small piece: the rail drawer's, a lettered
    /// square's letter.
    shade,
    /// A death recap line's health strip, under the health left.
    health_track,
    /// Ink on a class colour: the tag on a crest drawn without art.
    on_class,
    /// A death on a timeline: the hairline up from a skull on the ribbon, and
    /// the stripes of the inspector graph's hatch (at their own alpha times
    /// this one's); the hatch's dashed edge and its words stay `bad`.
    death_line,
    death_hatch,
    /// A key's run against its timers on Home: timed, and over.
    par_timed,
    par_over,
    /// The rail's drawer's edge over the stage, beside its shadow: none
    /// where the shadow tells the drawer from the stage, a hairline where
    /// the ground is too dark for a shadow to show.
    drawer_edge,
    /// What floats over the window — a menu, a card, the palette, a
    /// tooltip — when the theme draws it as glass (`effects.glass`): a
    /// translucent fill the window shows faintly through.
    glass,
    /// The glass's sheen: the top of a faint gradient down its face.
    glass_sheen,
    /// The glass's rim: a specular hairline along its top edge.
    glass_rim,
    /// The reticle brackets at an instrument's corners
    /// (`effects.brackets`).
    bracket,
    /// The sub-dial's tick track round the inspector's player
    /// (`effects.dial`), and the ribbon's fine ticks (`effects.fine_ticks`).
    dial,
}
}

tokens! {
/// The overlay's palette: white and dim on its dark panel, and the yellow
/// that means live, Σ, crit and "look here" there (and nowhere in the
/// window). Each role is named for what it marks; `navy`'s values are the
/// iced overlay's, pixel for pixel (`docs/plan-gui-new.md` phase 2).
pub struct OverlayTokens: Color {
    /// The panel's and the tab's fill, drawn at the alpha each surface
    /// asks for (0.92 the panel, 0.85 the tab).
    panel,
    /// The panel's and the tab's 1 px border.
    edge,
    /// A floating card's fill (the options card, the view menu).
    card,
    /// A card's 1 px border, and an idle disc's ring.
    card_edge,
    /// Words drawn with no colour of their own: names, labels, the clock.
    text,
    /// Values, the watched disc's ring.
    ink,
    /// Captions, secondary words, an idle control.
    dim,
    /// A row's rate column.
    rate,
    good,
    bad,
    /// Live, Σ, crit, "look here", a control that is on.
    yellow,
    /// The pointer's wash on a row.
    hover,
    /// The pointer's wash on a menu item.
    menu_hover,
    /// A bar's empty track.
    track,
    /// Health left, on a recap line.
    health,
    /// An enemy with no class.
    hostile,
    /// A control that is on: the options card's checked box (iced's
    /// TokyoNight primary, which the iced overlay's controls wore).
    control,
    /// The mark drawn on `control`.
    control_ink,
    /// A row with no known class.
    classless,
    /// A graph's plot area.
    plot,
    /// A graph's baseline, so an empty graph still reads as one.
    rule,
    /// A graph's time cursor, and its icon's ring when hovered is `ink`.
    cursor,
    /// A drag's selection on a graph, and its two edges.
    select,
    select_edge,
    /// A stat card's fill and border (the ability drill's numbers).
    stat_card,
    stat_card_edge,
    /// A list's scrollbar: its rail and its thumb (iced's TokyoNight
    /// scroller, which the iced overlay's lists wore).
    rail,
    thumb,
    /// A recap line's health strip, under the health left.
    health_track,
    /// A recap line's words and its health left, set over the line's bar.
    recap_ink,
    recap_hp,
    /// The instance strip's member pip with no outcome yet.
    pip,
    /// A member pip's fill: a kill, a wipe, the live pull.
    pip_kill,
    pip_wipe,
    pip_live,
    /// The instance strip's chip words, and the watched pip's ring.
    pip_lit,
    /// The ring round a picked disc (a comparison's side).
    picked,
    /// Ink on a class colour: the tag on a disc drawn without art.
    on_bar,
    /// The panel's sheen and rim when the theme draws it as glass
    /// (`effects.glass`): see the window's `glass_sheen` / `glass_rim`.
    glass_sheen,
    glass_rim,
}
}

tokens! {
/// Colours that carry data rather than chrome: the drill graph's stacked
/// bands, an icon-less ability's lettered square, the foe's disc and the
/// timeline marks. Each set is chosen to be told apart at a pixel or two
/// on the theme's grounds, so a theme moves them only with care — class
/// colours are data too, and no theme touches those (`Class::rgb`).
pub struct DataTokens: Color {
    /// The stacked bands, in slot order (blue, orange, aqua, violet,
    /// magenta, green in `navy`): a categorical palette validated by the
    /// data-viz checks on the window's surface — every two that touch an
    /// adjacent, separated pair (`inspect::stack`).
    stack_1,
    stack_2,
    stack_3,
    stack_4,
    stack_5,
    stack_6,
    /// The band of everything else: neutral, 3:1 on the surface.
    stack_other,
    /// The line along the rest's upper edge — the player's whole curve.
    /// Clear: the panel's 2 px gap every band has.
    stack_other_edge,
    /// The lettered square's hues for an ability without an icon, one per
    /// name (`inspect::list`).
    glyph_1,
    glyph_2,
    glyph_3,
    glyph_4,
    glyph_5,
    glyph_6,
    glyph_7,
    /// A foe's disc: a lit sphere of this colour.
    foe,
    /// The timeline marks (R12, R18, R23; `graph::mark_color`): a trinket
    /// use, a trinket proc, a consumable, an external, mitigation and
    /// defensives, support buffs, an offensive cooldown, the time spent
    /// dead, a healing cooldown — distinct hues, because at graph width a
    /// mark is a pixel or two wide and a shade is invisible.
    mark_use,
    mark_proc,
    mark_consumable,
    mark_external,
    mark_mitigation,
    mark_support,
    mark_cooldown,
    mark_dead,
    mark_healing_cd,
}
}

impl DataTokens {
    /// The six stacked bands' hues, in slot order.
    pub fn stack(&self) -> [Color; 6] {
        [
            self.stack_1,
            self.stack_2,
            self.stack_3,
            self.stack_4,
            self.stack_5,
            self.stack_6,
        ]
    }

    /// The seven lettered squares' hues.
    pub fn glyphs(&self) -> [Color; 7] {
        [
            self.glyph_1,
            self.glyph_2,
            self.glyph_3,
            self.glyph_4,
            self.glyph_5,
            self.glyph_6,
            self.glyph_7,
        ]
    }
}

/// The families a theme draws in. A family must be named: GPUI resolves
/// no generic names (`sans-serif`, `monospace`; spike S6). The bundled
/// faces (`crate::fonts`) are always registered; a config may name any
/// family installed on the machine. Its words are [`Text`], not `Copy`, so
/// it is written out rather than by `tokens!`.
#[derive(Debug, Clone, PartialEq)]
pub struct Faces {
    /// The window's names and numbers. Its digits should be tabular.
    pub ui: Text,
    /// Encounter titles, the wordmark and Home's place names.
    pub title: Text,
    /// The overlay's words.
    pub overlay: Text,
    /// The overlay's numbers.
    pub overlay_num: Text,
}

impl Faces {
    /// Every face, as a config names it.
    pub const NAMES: &'static [&'static str] = &["ui", "title", "overlay", "overlay_num"];

    /// The face a config names.
    pub fn get(&self, name: &str) -> Option<&Text> {
        match name {
            "ui" => Some(&self.ui),
            "title" => Some(&self.title),
            "overlay" => Some(&self.overlay),
            "overlay_num" => Some(&self.overlay_num),
            _ => None,
        }
    }

    /// The face a config names, to set.
    pub fn get_mut(&mut self, name: &str) -> Option<&mut Text> {
        match name {
            "ui" => Some(&mut self.ui),
            "title" => Some(&mut self.title),
            "overlay" => Some(&mut self.overlay),
            "overlay_num" => Some(&mut self.overlay_num),
            _ => None,
        }
    }
}

tokens! {
/// What a theme adds beyond colour and corners — each off in `navy`, so it
/// draws the prototype's pixels, and each a switch a config may throw.
pub struct Effects: bool {
    /// What floats over the window (the palette, the menus, the sheet, the
    /// toast, the tooltips) and the overlay panel are smoked glass: the
    /// `glass` fill under a sheen, with a specular rim along the top.
    glass,
    /// Reticle brackets at the corners of the instruments: the ribbon and
    /// the inspector's graph.
    brackets,
    /// A chronograph sub-dial round the inspector's player disc: a 60-tick
    /// bezel with the player's meter bar wrapped round it — their amount
    /// against the top row's, swept as an arc in their class colour to a
    /// hand.
    dial,
    /// A fine tick track under the ribbon's minute ticks, one per 10 s
    /// (its buckets), like a watch's chapter ring.
    fine_ticks,
    /// A pressed action (Compare once pinned, "Stop comparing", a pressed
    /// mode) is a raised key — the `raise` fill, the accent as a hairline
    /// edge and a lit bar along its foot, its words in ink — rather than a
    /// block filled with the accent, which on a dark theme with a white
    /// accent is the loudest thing on the stage.
    quiet_press,
}
}

/// One theme: an owned value. A built-in lives in a static for the life of
/// the process; one built from a config is freed with the last thing
/// holding it — the registry it came from, the look that wears it.
#[derive(Debug, Clone, PartialEq)]
pub struct Def {
    /// What config `theme` spells it as: lowercase, no spaces.
    pub name: Text,
    /// What the ⚙ card calls it.
    pub label: Text,
    /// What the ⚙ card calls its own chrome, beside "Your class".
    pub accent_label: Text,
    /// Dark ground, light ink. Every built-in is dark today.
    pub dark: bool,
    pub window: WindowTokens,
    pub overlay: OverlayTokens,
    /// The talent viewer's tree and tooltip.
    pub talents: TalentTokens,
    pub data: DataTokens,
    pub faces: Faces,
    /// The type scale.
    pub size: Sizes,
    /// The row pitches.
    pub pitch: Pitches,
    /// The corners.
    pub shape: Shape,
    /// How much class colour a bar shows.
    pub bars: Bars,
    pub effects: Effects,
    pub shadows: Shadows,
}

/// Every built-in theme, in the ⚙ card's order: the default first.
pub fn themes() -> [&'static Def; 3] {
    [&ONYX, &NAVY, &FROST]
}

/// The default theme, `onyx`: what a config that names none draws in, what
/// a name no theme answers to falls back to, and what a theme of a
/// config's own starts from when it says no `base`. (A config that still
/// says `gold` — every config the GUI saved before there were themes says
/// it — keeps the look it had: `gold` is `navy`'s old name.)
pub fn default_def() -> &'static Def {
    &ONYX
}

/// Names a config may still spell a built-in by: `gold` was `navy`'s name
/// until there were themes to tell apart.
const ALIASES: [(&str, &str); 1] = [("gold", "navy")];

/// The built-in theme `name` spells (any case, an old alias too).
pub fn builtin(name: &str) -> Option<&'static Def> {
    let name = name.trim();
    let name = ALIASES
        .iter()
        .find(|(old, _)| old.eq_ignore_ascii_case(name))
        .map_or(name, |(_, new)| *new);
    themes()
        .into_iter()
        .find(|d| d.name.eq_ignore_ascii_case(name))
}

/// The built-in theme config `theme` names; an unknown name is not an
/// error, it is the default. (A config's own themes are
/// [`super::Registry`]'s.)
pub fn def_named(name: &str) -> &'static Def {
    builtin(name).unwrap_or_else(default_def)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::color::{AA_CONTRAST, contrast};

    /// Every built-in theme's words read on every fill a row can wear —
    /// ground, panel, the pointer's wash, the selection — and the faint ink
    /// does not, which is why its text grade exists. Labels read on the
    /// grounds, ink on the accent reads, and so does every word on the
    /// glass a theme floats its menus on (over its ground, the darkest
    /// thing under it, and over a raised row, the lightest).
    #[test]
    fn every_theme_reads() {
        for def in themes() {
            let w = def.window;
            let mut fills = vec![
                ("ground", w.ground),
                ("surface", w.surface),
                ("hover", w.hover.over(w.ground)),
                ("raise", w.raise),
            ];
            if def.effects.glass {
                fills.push(("glass", w.glass.over(w.ground)));
                fills.push(("glass over raise", w.glass.over(w.raise)));
                fills.push(("sheen", w.glass_sheen.over(w.glass.over(w.raise))));
            }
            for (name, ink) in [
                ("ink", w.ink),
                ("ink_2", w.ink_2),
                ("ink_3_text", w.ink_3_text),
            ] {
                for (fill, bg) in &fills {
                    let c = contrast(ink, *bg);
                    assert!(
                        c >= AA_CONTRAST,
                        "{}: {name} on {fill} is {c:.2}:1",
                        def.name
                    );
                }
            }
            for (fill, bg) in [("ground", w.ground), ("surface", w.surface)] {
                for (name, ink) in [("label_ink", w.label_ink), ("good", w.good), ("bad", w.bad)] {
                    let c = contrast(ink, bg);
                    assert!(
                        c >= AA_CONTRAST,
                        "{}: {name} on {fill} is {c:.2}:1",
                        def.name
                    );
                }
            }
            let c = contrast(w.accent_ink, w.accent);
            assert!(
                c >= AA_CONTRAST,
                "{}: ink on the accent is {c:.2}:1",
                def.name
            );
            assert!(def.dark, "{}: every built-in is dark today", def.name);
        }
    }

    /// The overlay's words read on its panel as it is drawn over a black
    /// game (the panel's own alpha, 0.92), and on a card.
    #[test]
    fn every_overlay_reads() {
        for def in themes() {
            let o = def.overlay;
            let panel = o.panel.alpha(0.92).over(Color::BLACK);
            let card = o.card.over(Color::BLACK);
            for (name, ink) in [
                ("text", o.text),
                ("ink", o.ink),
                ("dim", o.dim),
                ("yellow", o.yellow),
                ("good", o.good),
                ("bad", o.bad),
            ] {
                for (fill, bg) in [("panel", panel), ("card", card)] {
                    let c = contrast(ink, bg);
                    assert!(
                        c >= AA_CONTRAST,
                        "{}: overlay {name} on {fill} is {c:.2}:1",
                        def.name
                    );
                }
            }
        }
    }

    #[test]
    fn themes_are_named_and_an_unknown_name_is_the_default() {
        assert_eq!(themes()[0].name, "onyx", "the default comes first");
        assert_eq!(default_def().name, "onyx");
        for def in themes() {
            assert_eq!(def_named(&def.name), def);
            assert_eq!(def_named(&def.name.to_uppercase()), def);
            assert!(!def.label.is_empty() && !def.accent_label.is_empty());
            assert_eq!(
                def.name.as_str(),
                def.name.to_lowercase(),
                "names are lowercase"
            );
        }
        assert_eq!(def_named("purple").name, "onyx", "the default");
        assert_eq!(builtin("purple"), None);
        assert_eq!(def_named(" Gold ").name, "navy", "the old name still reads");
        let mut names: Vec<&str> = themes().iter().map(|d| d.name.as_str()).collect();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), themes().len());
    }

    /// The token tables name every field once, and a name reads back
    /// what it sets.
    #[test]
    fn every_token_is_named() {
        let mut w = NAVY.window;
        assert!(WindowTokens::NAMES.contains(&"accent"));
        for name in WindowTokens::NAMES {
            let before = w.get(name);
            assert!(before.is_some(), "{name}");
            if let Some(slot) = w.get_mut(name) {
                *slot = Color::hex(0x123456);
            }
            assert_eq!(w.get(name), Some(Color::hex(0x123456)), "{name}");
        }
        assert_eq!(w.get("gold"), None, "the accent is no longer gold");
        let mut seen = std::collections::HashSet::new();
        for name in WindowTokens::NAMES
            .iter()
            .chain(OverlayTokens::NAMES)
            .chain(DataTokens::NAMES)
        {
            assert!(!name.is_empty());
            seen.insert(*name);
        }
        assert!(Effects::NAMES.len() == 5 && Faces::NAMES.len() == 4);
    }
}
