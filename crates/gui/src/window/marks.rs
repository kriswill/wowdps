//! A game's badge: the round shape every place that names a game wears
//! beside the name — the edition switch's chips, its dropdown and menu, a
//! fight's tag, an empty game's stage. It holds the game's emblem where
//! this machine's cache has one (`images::edition_emblem`), else our own
//! mark (gui-logic's `editions::Mark`, a glyph of the window's line-icon
//! table) cut out of a disc of the game's color. Both are the same size,
//! so a row of them keeps its rhythm whichever it holds, and the color
//! round them is ours either way (`editions::Tone`).

use gpui_kit::prelude::*;
use gpui_kit::{AnyElement, Bounds, Hsla, Pixels, canvas, div, img};
use wowdps_gui_logic::editions::{Mark, ink_on};
use wowdps_gui_logic::theme::Color;
use wowdps_model::Edition;

use super::paint::{fill_disc, paint_glyph_at};
use super::w::W;
use crate::images;
use crate::theme::hsla;

/// The share of the badge the mark's 16-unit box spans: the mark's own
/// shapes then sit inside the disc with a margin all round.
const MARK_BOX: f32 = 0.8;
/// The mark's stroke, in its box's units: a touch heavier than the line
/// icons' 1.5, so it holds as a cut-out at a tag's 13 px.
const MARK_STROKE: f32 = 1.7;
/// An emblem on a fill sits this much of the badge across, on its disc.
const EMBLEM_INSET: f32 = 0.84;

/// Where a badge sits, which decides its colors.
#[derive(Debug, Clone, Copy)]
pub enum Seat {
    /// On the bar, a menu or the stage, whose color under it is `under`:
    /// our mark cut out of a disc of the game's color (or of the bar's
    /// ink, where the chrome looks too like it); an emblem as it is.
    Bare { under: Hsla },
    /// On the game shown's chip, filled with the game's color `fill`: our
    /// mark cut out of a disc of the ink that reads on the fill; an emblem
    /// on a disc of the bar's color, so a fill of a like hue never
    /// swallows it.
    Fill { fill: Color },
}

/// The badge of `edition`, whose mark is `mark`, `side` across.
pub fn badge(w: &W, edition: Edition, mark: Mark, side: Pixels, seat: Seat) -> AnyElement {
    // An emblem stands only for a game's own mark: never under the neutral
    // ring an expansion we drew nothing for wears.
    let inset = match seat {
        Seat::Bare { .. } => side,
        Seat::Fill { .. } => side * EMBLEM_INSET,
    };
    let emblem = mark
        .own()
        .then(|| images::edition_emblem(edition, w.device(inset)))
        .flatten();
    if let Some(tile) = emblem {
        return match seat {
            Seat::Bare { .. } => img(tile).size(side).flex_none().into_any_element(),
            Seat::Fill { .. } => div()
                .size(side)
                .flex_none()
                .flex()
                .items_center()
                .justify_center()
                .rounded_full()
                .bg(w.c(|t| t.surface))
                .child(img(tile).size(inset).flex_none())
                .into_any_element(),
        };
    }
    let (disc, cut) = match seat {
        Seat::Bare { under } => (hsla(mark.tone.mark_color(&w.t, w.accent.base)), under),
        Seat::Fill { fill } => (hsla(ink_on(fill, &w.t)), hsla(fill)),
    };
    let glyph = mark.glyph;
    canvas(
        |_, _, _| {},
        move |b, (), window, _| paint_badge(window, glyph, b, disc, cut),
    )
    .size(side)
    .flex_none()
    .into_any_element()
}

/// Our mark's badge in `b`: a disc of `disc` across its short side, the
/// glyph stroked over it in `cut`.
pub fn paint_badge(
    window: &mut gpui_kit::Window,
    glyph: wowdps_gui_logic::glyph::Glyph,
    b: Bounds<Pixels>,
    disc: Hsla,
    cut: Hsla,
) {
    let side = b.size.width.min(b.size.height);
    fill_disc(window, b.center(), side / 2., disc);
    let inner = side * MARK_BOX;
    let at = Bounds::new(
        gpui_kit::point(b.center().x - inner / 2., b.center().y - inner / 2.),
        gpui_kit::size(inner, inner),
    );
    paint_glyph_at(window, glyph, at, cut, MARK_STROKE);
}
