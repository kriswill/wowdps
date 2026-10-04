//! What every overlay renderer draws with: the zoom, the overlay's palette
//! and its two faces, and the overlay's small drawn pieces (a dot, the
//! trash can, the staleness radar).
//!
//! The overlay zooms by hand, as the iced one does: a size the iced overlay
//! multiplied by `cfg.zoom` is `ov.z(…)` here, and a size it left fixed
//! (most paddings and gaps) is a plain `px(…)`. The porting reference's
//! table (`overlay-anatomy.md` §3.19) is the rule for which is which.

use std::f32::consts::PI;

use gpui_kit::prelude::*;
use gpui_kit::{
    App, Div, Hsla, PathBuilder, Pixels, SharedString, canvas, div, point, px, relative,
};
use wowdps_gui_logic::theme::{self as gl, Bars, DataTokens, Effects, OverlayTokens, Shape};

use crate::theme::{Look, hsla};

/// The overlay's render context.
#[derive(Clone, Debug)]
pub struct Ov {
    pub zoom: f32,
    pub t: OverlayTokens,
    /// The face of the overlay's words.
    pub sans: SharedString,
    /// The face of its numbers.
    pub mono: SharedString,
    /// The corners: the overlay's are the theme's shape too.
    pub shape: Shape,
    /// How much class colour a row's bar shows.
    pub bars: Bars,
    /// What the theme adds; the overlay wears its glass.
    pub fx: Effects,
    /// The colours that carry data: the graph's marks.
    pub data: DataTokens,
}

impl Ov {
    pub fn new(zoom: f32, cx: &App) -> Self {
        let look = Look::global(cx);
        Self {
            zoom,
            t: look.def.overlay,
            sans: crate::theme::face(&look.def.faces.overlay),
            mono: crate::theme::face(&look.def.faces.overlay_num),
            shape: look.def.shape,
            bars: look.def.bars,
            fx: look.def.effects,
            data: look.def.data,
        }
    }

    /// `v` logical pixels at the overlay's zoom.
    pub fn z(&self, v: f32) -> Pixels {
        px(v * self.zoom)
    }

    /// A palette colour as GPUI's.
    pub fn c(&self, pick: impl FnOnce(&OverlayTokens) -> gl::Color) -> Hsla {
        hsla(pick(&self.t))
    }

    /// A corner the iced overlay drew at `v` px (never zoomed), at the
    /// theme's shape.
    pub fn r(&self, v: f32) -> Pixels {
        px(self.shape.radius(v))
    }

    /// A pill's corner, `v` its half-height, at the theme's shape.
    pub fn pill(&self, v: Pixels) -> Pixels {
        px(self.shape.pill(f32::from(v)))
    }

    /// A card's face (the options card, the view menu): `card` inside a
    /// `card_edge` hairline as the iced overlay drew it — or, where the
    /// theme is glass, the card under the panel's sheen with its rim.
    pub fn card_face<E: Styled>(&self, el: E) -> E {
        let el = el.border_1().border_color(self.c(|t| t.card_edge));
        if !self.fx.glass {
            return el.bg(self.c(|t| t.card));
        }
        let card = self.t.card;
        let sheen = self.t.glass_sheen.over(card.alpha(1.0)).alpha(card.a);
        el.bg(gpui_kit::linear_gradient(
            180.,
            gpui_kit::linear_color_stop(hsla(sheen), 0.),
            gpui_kit::linear_color_stop(hsla(card), 0.30),
        ))
        .shadow(vec![gpui_kit::BoxShadow {
            color: hsla(self.t.glass_rim),
            offset: point(px(0.), px(1.)),
            blur_radius: px(0.),
            spread_radius: px(0.),
            inset: true,
        }])
    }

    /// The panel's or the tab's face: the panel colour at `alpha` inside its
    /// edge, as the iced overlay drew it — or, where the theme is glass,
    /// the same smoke under a faint sheen with a specular rim along its top
    /// (an inset hairline), so it reads as glass over the game.
    pub fn glass<E: Styled>(&self, el: E, alpha: f32) -> E {
        let fill = self.t.panel.alpha(alpha);
        let el = el.border_1().border_color(self.c(|t| t.edge));
        if !self.fx.glass {
            return el.bg(hsla(fill));
        }
        let sheen = self.t.glass_sheen.over(self.t.panel).alpha(alpha);
        el.bg(gpui_kit::linear_gradient(
            180.,
            gpui_kit::linear_color_stop(hsla(sheen), 0.),
            gpui_kit::linear_color_stop(hsla(fill), 0.30),
        ))
        .shadow(vec![gpui_kit::BoxShadow {
            color: hsla(self.t.glass_rim),
            offset: point(px(0.), px(1.)),
            blur_radius: px(0.),
            spread_radius: px(0.),
            inset: true,
        }])
    }

    /// The overlay's scrollbar: a square thumb on its rail.
    pub fn scrollbar(&self) -> crate::scrollbar::Style {
        crate::scrollbar::Style {
            rail: Some(self.c(|t| t.rail)),
            thumb: self.c(|t| t.thumb),
            width: px(crate::scrollbar::WIDTH),
            radius: px(0.),
        }
    }

    /// Words in the overlay's face, `size` ×z.
    pub fn words(&self, text: impl Into<SharedString>, size: f32, color: Hsla) -> Div {
        div()
            .font_family(self.sans.clone())
            .text_size(self.z(size))
            .text_color(color)
            .whitespace_nowrap()
            .child(text.into())
    }

    /// Numbers in the overlay's monospace face, `size` ×z.
    pub fn nums(&self, text: impl Into<SharedString>, size: f32, color: Hsla) -> Div {
        div()
            .font_family(self.mono.clone())
            .text_size(self.z(size))
            .text_color(color)
            .whitespace_nowrap()
            .child(text.into())
    }

    /// A right-aligned numeric cell `width` ×z wide: a column of figures
    /// that never moves (the iced overlay's `metric`).
    pub fn metric(&self, text: impl Into<SharedString>, size: f32, color: Hsla, width: f32) -> Div {
        self.nums(text, size, color)
            .w(self.z(width))
            .flex_none()
            .text_right()
            .overflow_hidden()
    }

    /// A filled circle `d` across (the "live" dot).
    pub fn dot(&self, color: Hsla, d: Pixels) -> impl IntoElement {
        canvas(
            |_, _, _| {},
            move |b, (), window, _| {
                let r = b.size.width.min(b.size.height) / 2.;
                let c = b.center();
                let mut path = PathBuilder::fill();
                circle(&mut path, c.x, c.y, r);
                if let Ok(path) = path.build() {
                    window.paint_path(path, color);
                }
            },
        )
        .size(d)
        .flex_none()
    }

    /// The trash can `d` across, in 1.1 px round strokes: handle, lid, a
    /// tapered body and one rib.
    pub fn trash(&self, color: Hsla, d: Pixels) -> impl IntoElement {
        canvas(
            |_, _, _| {},
            move |b, (), window, _| {
                let (x0, y0, w, h) = (b.origin.x, b.origin.y, b.size.width, b.size.height);
                let at = |fx: f32, fy: f32| point(x0 + w * fx, y0 + h * fy);
                let cx_ = 0.5;
                let mut path = PathBuilder::stroke(px(1.1));
                // Handle.
                path.move_to(at(cx_ - 0.14, 0.24));
                path.line_to(at(cx_ - 0.14, 0.10));
                path.line_to(at(cx_ + 0.14, 0.10));
                path.line_to(at(cx_ + 0.14, 0.24));
                // Lid.
                path.move_to(at(0.14, 0.24));
                path.line_to(at(0.86, 0.24));
                // Body.
                path.move_to(at(0.22, 0.24));
                path.line_to(at(0.30, 0.90));
                path.line_to(at(0.70, 0.90));
                path.line_to(at(0.78, 0.24));
                // Rib.
                path.move_to(at(cx_, 0.40));
                path.line_to(at(cx_, 0.74));
                if let Ok(path) = path.build() {
                    window.paint_path(path, color);
                }
            },
        )
        .size(d)
        .flex_none()
    }

    /// The staleness radar `d` across, as the iced overlay sweeps it: a
    /// trail of stacked sectors behind the hand whose accumulated alpha is
    /// 0.95·e^(−2.2·f), the ring over it, and the hand (two soft strokes and
    /// a whitened core) at `angle`, 0 at twelve o'clock, clockwise.
    pub fn radar(&self, angle: f32, color: gl::Color, d: Pixels) -> impl IntoElement {
        canvas(
            |_, _, _| {},
            move |b, (), window, _| {
                const TRAIL: f32 = PI * 1.4;
                const BANDS: usize = 14;
                let c = b.center();
                let r = b.size.width.min(b.size.height) / 2. - px(1.);
                let tip = |a: f32, len: Pixels| {
                    let s = a - PI / 2.0;
                    point(c.x + len * s.cos(), c.y + len * s.sin())
                };
                let tint = |alpha: f32| hsla(color.alpha(alpha));
                let profile = |f: f32| 0.95 * (-2.2 * f.clamp(0.0, 1.0)).exp();
                let step = TRAIL / BANDS as f32;
                for i in 0..BANDS {
                    let here = profile((i as f32 + 0.5) / BANDS as f32);
                    let next = if i + 1 < BANDS {
                        profile((i as f32 + 1.5) / BANDS as f32)
                    } else {
                        0.0
                    };
                    let alpha = ((here - next) / (1.0 - next)).clamp(0.0, 1.0);
                    let from = angle - step * (i as f32 + 1.0);
                    let mut sector = PathBuilder::fill();
                    sector.move_to(c);
                    for k in 0..=12 {
                        sector.line_to(tip(from + (angle - from) * (k as f32 / 12.0), r));
                    }
                    sector.close();
                    if let Ok(path) = sector.build() {
                        window.paint_path(path, tint(alpha));
                    }
                }
                let mut ring = PathBuilder::stroke(px(1.));
                circle(&mut ring, c.x, c.y, r);
                if let Ok(path) = ring.build() {
                    window.paint_path(path, tint(0.6));
                }
                for (width, alpha) in [(3.6, 0.18), (2.2, 0.38)] {
                    let mut hand = PathBuilder::stroke(px(width));
                    hand.move_to(c);
                    hand.line_to(tip(angle, r));
                    if let Ok(path) = hand.build() {
                        window.paint_path(path, tint(alpha));
                    }
                }
                let mut core = PathBuilder::stroke(px(1.2));
                core.move_to(c);
                core.line_to(tip(angle, r));
                if let Ok(path) = core.build() {
                    window.paint_path(path, hsla(color.lighten(0.55).alpha(0.95)));
                }
            },
        )
        .size(d)
        .flex_none()
    }
}

/// A full circle as path segments (32 of them: smooth at any size the
/// overlay draws).
pub fn circle(path: &mut PathBuilder, cx: Pixels, cy: Pixels, r: Pixels) {
    for k in 0..=32 {
        let a = k as f32 / 32.0 * 2.0 * PI;
        let p = point(cx + r * a.cos(), cy + r * a.sin());
        if k == 0 {
            path.move_to(p);
        } else {
            path.line_to(p);
        }
    }
    path.close();
}

/// A bar's fill as the overlay draws it: a left-to-right ramp of `color`
/// from 16% to 55%.
pub fn bar_ramp(color: Hsla, bars: &Bars) -> gpui_kit::Background {
    gpui_kit::linear_gradient(
        90.,
        gpui_kit::linear_color_stop(color.opacity(bars.rest_from), 0.),
        gpui_kit::linear_color_stop(color.opacity(bars.rest_to), 1.),
    )
}

/// `frac` (0..=1) of a track as a relative length.
pub fn share(frac: f32) -> gpui_kit::DefiniteLength {
    relative(frac.clamp(0.0, 1.0))
}

/// [`Ov::card_face`] in a builder chain.
pub trait Card: Styled + Sized {
    fn card(self, ov: &Ov) -> Self {
        ov.card_face(self)
    }
}

impl<E: Styled> Card for E {}
