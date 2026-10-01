//! The window's small controls, as the iced window's `nav` draws them
//! (`docs/design/window-redesign.html`'s `.ibtn`, `kbd`, `.place`,
//! `.badge`, `.tip`): each takes the window's `W` and draws from its
//! tokens, never a literal colour.

use gpui_kit::base::ObservedElement;
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::prelude::*;
use gpui_kit::{Div, ElementId, Hsla, SharedString, Stateful, TestSupportExt as _, div};
use wowdps_gui_logic::glyph::Glyph;
use wowdps_gui_logic::labels::{Tone, sentence};

use super::paint::{dot, glyph, glyph_ink};
use super::w::{MEDIUM, REGULAR, SEMIBOLD, W};

/// An inert icon button keeps this much of the faint ink: told from a live
/// one by more than a step of ink.
pub const INERT_ALPHA: f32 = 0.6;

/// A keycap (`kbd`): the key in secondary ink at 11.5 px, weight 500, on
/// the raised fill, framed in the floating edge — 1 px, and 2 px along the
/// bottom, so it reads as a key standing on the sheet.
pub fn kbd(w: &W, key: impl Into<SharedString>) -> Div {
    div()
        .flex_none()
        .rounded(w.z(4.))
        .bg(w.c(|t| t.edge))
        .pt(w.z(1.))
        .px(w.z(1.))
        .pb(w.z(2.))
        .child(
            div()
                .rounded(w.z(3.))
                .bg(w.c(|t| t.raise))
                .px(w.z(5.))
                .child(
                    w.text(key, w.size.kbd, w.c(|t| t.ink_2), MEDIUM)
                        .line_height(w.z(17.)),
                ),
        )
}

/// An icon button (`.ibtn`): a line icon in secondary ink on a 30 px
/// target with its 6 px corners, washed and brightened to ink under the
/// pointer. `enabled: false` is the end of a list with nothing to step to:
/// the glyph faint and fainter still, no wash, no hand.
pub fn icon_button(
    w: &W,
    id: impl Into<ElementId>,
    icon: Glyph,
    enabled: bool,
) -> ObservedElement<Stateful<Div>> {
    let side = w.z(w.pitch.icon_button);
    let frame = div()
        .id(id)
        .test_support()
        .size(side)
        .flex_none()
        .flex()
        .items_center()
        .justify_center()
        .rounded(w.z(6.));
    if enabled {
        frame
            .cursor_pointer()
            .text_color(w.c(|t| t.ink_2))
            .hover(|s| s.bg(w.c(|t| t.hover)).text_color(w.c(|t| t.ink)))
            .child(glyph_ink(icon, w.z(w.size.icon)))
    } else {
        let faint = w.c(|t| t.ink_3.alpha(INERT_ALPHA));
        frame.child(glyph(icon, w.z(w.size.icon), faint))
    }
}

/// `words` under the pointer (`.tip`), Kit's tooltip in the theme's
/// floating surface.
pub fn tip<E: StatefulInteractiveElement>(el: E, words: &'static str) -> E {
    el.tooltip(move |window, cx| Tooltip::new(SharedString::from(words)).build(window, cx))
}

/// A top-bar place (`.place`): 15 px at 500, 42 tall with its 2 px
/// underline, the active one's in the accent; parchment when active or
/// under the pointer, else secondary.
pub fn place(
    w: &W,
    id: &'static str,
    label: &'static str,
    active: bool,
) -> ObservedElement<Stateful<Div>> {
    let ink = if active {
        w.c(|t| t.ink)
    } else {
        w.c(|t| t.ink_2)
    };
    div()
        .id(id)
        .test_support()
        .h(w.z(w.pitch.place))
        .flex_none()
        .flex()
        .flex_col()
        .cursor_pointer()
        .text_color(ink)
        .hover(|s| s.text_color(w.c(|t| t.ink)))
        .child(
            div()
                .flex_1()
                .flex()
                .items_center()
                .px(w.z(11.))
                .child(w.words(label, w.size.place, MEDIUM)),
        )
        .child(div().h(w.z(2.)).w_full().when(active, |d| d.bg(w.accent())))
}

/// An outcome badge (`.badge`: 600 at 13 px, padded 1 × 7 on a faint wash
/// of its own colour), its word in sentence case: Live leads with the red
/// dot. Never yellow.
#[expect(dead_code, reason = "the stage draws it from step 3.2")]
pub fn badge(w: &W, word: &str, tone: Tone) -> Div {
    let color: Hsla = match tone {
        Tone::Good => w.c(|t| t.good),
        Tone::Bad | Tone::Live | Tone::None => w.c(|t| t.bad),
    };
    div()
        .flex_none()
        .flex()
        .items_center()
        .gap(w.z(6.))
        .py(w.z(1.))
        .px(w.z(7.))
        .rounded(w.z(4.))
        .bg(color.opacity(0.12))
        .when(tone == Tone::Live, |d| d.child(dot(w.z(8.), color)))
        .child(w.text(sentence(word), w.size.micro, color, SEMIBOLD))
}

/// A full-width 1 px rule in the hairline ink.
#[expect(dead_code, reason = "the stage draws it from step 3.2")]
pub fn hairline(w: &W) -> Div {
    div().w_full().h(w.z(1.)).flex_none().bg(w.c(|t| t.line))
}

/// A full-height 1 px rule: between the meter and the inspector, and the
/// docked rail and the stage.
pub fn vrule(w: &W) -> Div {
    div().h_full().w(w.z(1.)).flex_none().bg(w.c(|t| t.line))
}

/// One of a line's quiet words: secondary ink, regular.
pub fn quiet(w: &W, words: impl Into<SharedString>, size: f32) -> Div {
    w.text(words, size, w.c(|t| t.ink_2), REGULAR)
}
