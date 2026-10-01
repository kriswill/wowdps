//! The ⚙ card (the iced window's `view::options_panel`): durable
//! presentation toggles — row ranks, realm names — and the chrome's colour,
//! each written to the config the moment it changes, its one key alone
//! (`Config::store`), so a zoom or an overlay drag saved since launch is
//! never written back over. Hung under the bar's gear; it takes its own
//! presses, and the pointer leaving it closes it.

use gpui_kit::prelude::*;
use gpui_kit::{
    AnyElement, Context, Div, ElementId, MouseButton, SharedString, Stateful, TestSupportExt as _,
    Window, div,
};
use wowdps_gui_logic::glyph::Glyph;
use wowdps_gui_logic::theme::{Chrome, SHADOW_MENU, accent_wash};

use super::super::Gui;
use super::super::paint::paint_glyph_at;
use super::super::w::{REGULAR, SEMIBOLD, W};
use super::{BORDER, enter};
use crate::theme::hsla;

/// Where the card hangs: under the bar, its right edge 40 px in from the
/// window's — left of the help button, under the gear.
const RIGHT: f32 = 40.0;
/// The card's insets and corners, and the gap between its lines.
const PAD: f32 = 10.0;
const RADIUS: f32 = 8.0;
const GAP: f32 = 8.0;
/// A checkbox (iced's: a 16 px box, 2 px corners, its label 8 px after
/// it), and its tick: the glyph table's check at 13 px, stroked at 3 of
/// its 16 units, centred — iced's icon-font tick, measured.
const BOX: f32 = 16.0;
const BOX_RADIUS: f32 = 2.0;
const BOX_GAP: f32 = 8.0;
const TICK: f32 = 3.0;
const TICK_SIDE: f32 = 13.0;
/// A chip (`nav::chip`): 3 × 10 inside a 13 px-cornered hairline, 6 px
/// between two.
const CHIP_PAD: (f32, f32) = (3.0, 10.0);
const CHIP_RADIUS: f32 = 13.0;
const CHIP_GAP: f32 = 6.0;

/// The card over the window, under the gear.
pub fn view(gui: &Gui, w: &W, window: &mut Window, cx: &mut Context<Gui>) -> AnyElement {
    let current = gui.cfg.chrome();
    let card = div()
        .id("options")
        .test_support()
        .occlude()
        .flex()
        .flex_col()
        .gap(w.z(GAP))
        .p(w.z(PAD - BORDER))
        .bg(w.c(|t| t.surface))
        .border(w.z(1.))
        .border_color(w.c(|t| t.edge))
        .rounded(w.z(RADIUS))
        .shadow(vec![w.shadow(SHADOW_MENU)])
        .child(w.text("Options", w.size.label, w.c(|t| t.gold_dim), SEMIBOLD))
        .child(
            checkbox("option-ranks", "Row ranks", gui.cfg.show_ranks, w).on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| {
                    let on = !this.cfg.show_ranks;
                    this.set_show_ranks(on, cx);
                }),
            ),
        )
        .child(
            checkbox("option-realms", "Hide realm names", gui.cfg.hide_realms, w).on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| {
                    let on = !this.cfg.hide_realms;
                    this.set_hide_realms(on, cx);
                }),
            ),
        )
        .child(w.text("Chrome", w.size.label, w.c(|t| t.gold_dim), REGULAR))
        .child(
            div()
                .flex()
                .gap(w.z(CHIP_GAP))
                .child(chrome_chip("Game gold", Chrome::Gold, current, w, cx))
                .child(chrome_chip("Your class", Chrome::Class, current, w, cx)),
        )
        // Presses on the card are its own; the pointer wandering off it
        // closes it.
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .on_hover(cx.listener(|this, over: &bool, _, cx| {
            if !*over && this.cards.options {
                this.cards.options = false;
                cx.notify();
            }
        }));
    div()
        .absolute()
        .top(w.z(w.pitch.top_bar))
        .right(w.z(RIGHT))
        .child(enter("options-enter", card, w, window, cx))
        .into_any_element()
}

/// A checkbox and its label (iced's `checkbox`, the window theme's gold):
/// checked, the box is gold with its tick; unchecked, the ground inside a
/// gold edge. The whole line is the target.
fn checkbox(
    id: &'static str,
    label: &'static str,
    on: bool,
    w: &W,
) -> gpui_kit::base::ObservedElement<Stateful<Div>> {
    let gold = w.c(|t| t.gold);
    let tick = w.c(|t| t.check);
    let side = w.z(BOX);
    let mark = gpui_kit::canvas(
        |_, _, _| {},
        move |b, (), window, _| paint_glyph_at(window, Glyph::Check, b, tick, TICK),
    )
    .size(w.z(TICK_SIDE));
    let face = div()
        .size(side)
        .flex_none()
        .rounded(w.z(BOX_RADIUS))
        .border(w.z(1.))
        .border_color(gold)
        .flex()
        .items_center()
        .justify_center()
        .when(on, |d| d.bg(gold).child(mark))
        .when(!on, |d| d.bg(w.c(|t| t.ground)));
    div()
        .id(ElementId::Name(SharedString::from(id)))
        .test_support()
        .aria_selected(on)
        .flex()
        .items_center()
        .gap(w.z(BOX_GAP))
        .cursor_pointer()
        .child(face)
        .child(w.text(label, w.size.body, w.c(|t| t.ink), REGULAR))
}

/// One of the chrome's two chips (`nav::chip`): the pressed one in the
/// accent's edge over a wash of it, its label in ink; the other in a
/// hairline, its label secondary.
fn chrome_chip(
    label: &'static str,
    chrome: Chrome,
    current: Chrome,
    w: &W,
    cx: &mut Context<Gui>,
) -> impl IntoElement {
    let on = chrome == current;
    div()
        .id(ElementId::Name(SharedString::from(format!(
            "chrome-{}",
            chrome.name()
        ))))
        .test_support()
        .aria_selected(on)
        .flex_none()
        .py(w.z(CHIP_PAD.0 - BORDER))
        .px(w.z(CHIP_PAD.1 - BORDER))
        .rounded(w.z(CHIP_RADIUS))
        .border(w.z(1.))
        .cursor_pointer()
        .when(on, |d| {
            d.bg(hsla(accent_wash(w.accent))).border_color(w.accent())
        })
        .when(!on, |d| d.border_color(w.c(|t| t.line)))
        .child(w.text(
            label,
            w.size.micro,
            if on { w.c(|t| t.ink) } else { w.c(|t| t.ink_2) },
            REGULAR,
        ))
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |this, _, _, cx| this.set_chrome(chrome, cx)),
        )
}
