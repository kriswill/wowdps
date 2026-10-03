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
use wowdps_gui_logic::theme::{Chrome, Def, NAVY, accent_wash};

use super::super::Gui;
use super::super::paint::paint_glyph_at;
use super::super::w::Floating;
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
    // Its own copy: the chips need the context the registry is borrowed from.
    let themes = crate::theme::Themes::global(cx).clone();
    let active = gui.theme_def(cx);
    let themes_row = div()
        .flex()
        .flex_wrap()
        .max_w(w.z(w.pitch.menu_w))
        .gap(w.z(CHIP_GAP))
        .children(
            themes
                .themes()
                .iter()
                .map(|def| theme_chip(def, def.name == active.name, w, cx))
                .collect::<Vec<_>>(),
        );
    let problems = themes.warnings.len();
    let problem = themes.warnings.first().map(|first| {
        let more = match problems {
            0 | 1 => String::new(),
            n => format!(" (and {} more; the GUI's log has them all)", n - 1),
        };
        div()
            .max_w(w.z(w.pitch.menu_w))
            .font_family(w.ui.clone())
            .text_size(w.z(w.size.tiny))
            .text_color(w.c(|t| t.ink_3_text))
            .child(format!("Config: {first}{more}"))
    });
    let card = div()
        .id("options")
        .test_support()
        .occlude()
        .flex()
        .flex_col()
        .gap(w.z(GAP))
        .p(w.z(PAD - BORDER))
        .border(w.z(1.))
        .rounded(w.r(RADIUS))
        .floating(w, w.c(|t| t.surface), w.c(|t| t.edge), w.shadows.menu)
        .child(w.text("Options", w.size.label, w.c(|t| t.label_ink), SEMIBOLD))
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
        .child(w.text("Theme", w.size.label, w.c(|t| t.label_ink), REGULAR))
        .child(themes_row)
        .children(problem)
        .child(w.text("Chrome", w.size.label, w.c(|t| t.label_ink), REGULAR))
        .child(
            div()
                .flex()
                .gap(w.z(CHIP_GAP))
                .child(chrome_chip(
                    active.accent_label.to_string(),
                    Chrome::Theme,
                    current,
                    w,
                    cx,
                ))
                .child(chrome_chip(
                    "Your class".to_string(),
                    Chrome::Class,
                    current,
                    w,
                    cx,
                )),
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
    let gold = w.c(|t| t.accent);
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
        .rounded(w.r(BOX_RADIUS))
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
    label: String,
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
        .rounded(w.pill(CHIP_RADIUS))
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

/// A swatch's size and the stripe of accent along its foot.
const SWATCH: (f32, f32) = (18.0, 12.0);
const SWATCH_STRIPE: f32 = 3.0;

/// One theme's chip: a swatch of its ground with its accent along the foot,
/// and its name set in its own title face (sized so every face reads at
/// one height, as the wordmark's size relates each title face), pressed as
/// the chrome chips are.
fn theme_chip(
    def: &Def,
    on: bool,
    w: &W,
    cx: &mut Context<Gui>,
) -> gpui_kit::base::ObservedElement<Stateful<Div>> {
    let own = &def.window;
    let swatch = div()
        .w(w.z(SWATCH.0))
        .h(w.z(SWATCH.1))
        .flex_none()
        .flex()
        .flex_col()
        .justify_end()
        .overflow_hidden()
        .rounded(w.r(2.))
        .border(w.z(1.))
        .border_color(hsla(own.edge))
        .bg(hsla(own.ground))
        .child(div().w_full().h(w.z(SWATCH_STRIPE)).bg(hsla(own.accent)));
    let size = w.size.micro * def.size.mark / NAVY.size.mark;
    div()
        .id(ElementId::Name(SharedString::from(format!(
            "theme-{}",
            def.name
        ))))
        .test_support()
        .aria_selected(on)
        .flex_none()
        .flex()
        .items_center()
        .gap(w.z(CHIP_GAP))
        .py(w.z(CHIP_PAD.0 - BORDER))
        .pl(w.z(CHIP_PAD.0 + 1.0 - BORDER))
        .pr(w.z(CHIP_PAD.1 - BORDER))
        .rounded(w.pill(CHIP_RADIUS))
        .border(w.z(1.))
        .cursor_pointer()
        .when(on, |d| {
            d.bg(hsla(accent_wash(w.accent))).border_color(w.accent())
        })
        .when(!on, |d| d.border_color(w.c(|t| t.line)))
        .child(swatch)
        .child(
            div()
                .font_family(crate::theme::face(&def.faces.title))
                .text_size(w.z(size))
                .text_color(if on { w.c(|t| t.ink) } else { w.c(|t| t.ink_2) })
                .whitespace_nowrap()
                .child(def.label.to_string()),
        )
        .on_mouse_down(MouseButton::Left, {
            let name = def.name.clone();
            cx.listener(move |this, _, _, cx| this.set_theme(&name, cx))
        })
}
