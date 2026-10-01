//! What every window renderer draws with (the iced window's `theme` module
//! and `Look::WINDOW`): the active theme's tokens, type scale and pitches,
//! the chrome accent, the window's two faces, its zoom and its width.
//!
//! The iced window zooms by its app scale factor; GPUI has none, so a size
//! here is `w.z(…)`: logical pixels at the window's zoom (spec §8, "Zoom").
//! Every breakpoint reads `w.width`, the window's width at zoom 1, so a
//! zoom chord re-lays the window out without a resize.

use gpui_kit::prelude::*;
use gpui_kit::{App, Div, FontWeight, Hsla, Pixels, SharedString, div, px};
use wowdps_gui_logic::theme::{
    self as gl, Accent, NARROW_WINDOW, Pitches, Sizes, TILE_WINDOW, WindowTokens, YOU_CONTRAST,
};
use wowdps_model::Class;

use crate::theme::{Look, hsla};

/// The window's three weights: Barlow at 400, 500 and 600.
pub const REGULAR: FontWeight = FontWeight::NORMAL;
pub const MEDIUM: FontWeight = FontWeight::MEDIUM;
pub const SEMIBOLD: FontWeight = FontWeight::SEMIBOLD;

/// How wide the window is, by the prototype's breakpoints (inclusive).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fit {
    /// 820 px and under: one column, the inspector pushed.
    Narrow,
    /// 821–1180: the rail a drawer, the inspector 410 beside the meter.
    Tile,
    /// Wider: the rail docked, the inspector 520.
    Wide,
}

impl Fit {
    pub fn of(width: f32) -> Self {
        if width <= NARROW_WINDOW {
            Fit::Narrow
        } else if width <= TILE_WINDOW {
            Fit::Tile
        } else {
            Fit::Wide
        }
    }
}

/// The window's render context.
#[derive(Clone, Copy, Debug)]
pub struct W {
    pub zoom: f32,
    pub t: WindowTokens,
    pub size: Sizes,
    pub pitch: Pitches,
    /// The chrome: the theme's gold or the owner's class.
    pub accent: Accent,
    /// Names and numbers (the tabular Barlow).
    pub ui: &'static str,
    /// Encounter titles and the wordmark (Marcellus).
    pub title: &'static str,
    /// The window's width at zoom 1, in logical pixels.
    pub width: f32,
}

impl W {
    pub fn new(zoom: f32, width: f32, cx: &App) -> Self {
        let look = Look::global(cx);
        Self {
            zoom,
            t: look.def.window,
            size: look.def.size,
            pitch: look.def.pitch,
            accent: look.accent,
            ui: look.def.faces.ui,
            title: look.def.faces.title,
            width,
        }
    }

    /// `v` logical pixels at the window's zoom.
    pub fn z(&self, v: f32) -> Pixels {
        px(v * self.zoom)
    }

    /// A token as GPUI's.
    pub fn c(&self, pick: impl FnOnce(&WindowTokens) -> gl::Color) -> Hsla {
        hsla(pick(&self.t))
    }

    /// The accent's base, as GPUI's.
    pub fn accent(&self) -> Hsla {
        hsla(self.accent.base)
    }

    pub fn fit(&self) -> Fit {
        Fit::of(self.width)
    }

    pub fn narrow(&self) -> bool {
        self.fit() == Fit::Narrow
    }

    /// One line of words in the window's face: `size` (zoomed), `color`,
    /// `weight`, never wrapping.
    pub fn text(
        &self,
        words: impl Into<SharedString>,
        size: f32,
        color: Hsla,
        weight: FontWeight,
    ) -> Div {
        div()
            .font_family(self.ui)
            .font_weight(weight)
            .text_size(self.z(size))
            .text_color(color)
            .whitespace_nowrap()
            .child(words.into())
    }

    /// One line of words that takes its colour from its parent: the label
    /// of a control whose hover brightens it.
    pub fn words(&self, words: impl Into<SharedString>, size: f32, weight: FontWeight) -> Div {
        div()
            .font_family(self.ui)
            .font_weight(weight)
            .text_size(self.z(size))
            .whitespace_nowrap()
            .child(words.into())
    }

    /// One line in the title face (Marcellus).
    pub fn title_text(&self, words: impl Into<SharedString>, size: f32, color: Hsla) -> Div {
        self.text(words, size, color, REGULAR)
            .font_family(self.title)
    }

    /// A class colour as data — a bar, a disc, a skull: `Class::rgb`
    /// exactly, or the classless grey.
    pub fn class_rgb(&self, class: Option<Class>) -> gl::Color {
        class.map_or(self.t.classless, gl::Color::of_class)
    }

    /// The OWNER's name as text (`--you-text`): lifted until it clears AAA
    /// on the surface, a step brighter than any other player.
    pub fn you_text(&self, class: Option<Class>) -> Hsla {
        match class {
            Some(c) => hsla(gl::class_text_on(c, self.t.surface, YOU_CONTRAST)),
            None => self.c(|t| t.ink),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Fit;

    /// The prototype's frames: 1440 wide, 960 a tile, 460 narrow — and
    /// `max-width` is inclusive at both breakpoints.
    #[test]
    fn the_fits_are_the_prototype_s() {
        assert_eq!(Fit::of(1440.0), Fit::Wide);
        assert_eq!(Fit::of(1181.0), Fit::Wide);
        assert_eq!(Fit::of(1180.0), Fit::Tile);
        assert_eq!(Fit::of(960.0), Fit::Tile);
        assert_eq!(Fit::of(821.0), Fit::Tile);
        assert_eq!(Fit::of(820.0), Fit::Narrow);
        assert_eq!(Fit::of(460.0), Fit::Narrow);
    }
}
