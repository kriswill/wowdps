//! The edition menu, opened from the top bar's edition switch: one row per
//! game the window knows — its name ("Midnight", "Forever"), "Playing now"
//! in quieter words beside the game being played, the build in its
//! tooltip — the game shown lit. Nothing else: no mode to learn. Drawn at
//! the window's root over a scrim that takes the press that closes it,
//! hung under the switch, as the character menu hangs under the picker.

use gpui_kit::prelude::*;
use gpui_kit::{
    AnyElement, Context, ElementId, MouseButton, SharedString, TestSupportExt as _, Window, div,
};
use wowdps_gui_logic::editions::Entry;

use super::super::Gui;
use super::super::chrome::tip;
use super::super::w::Floating;
use super::super::w::{REGULAR, SEMIBOLD, W};
use super::menu::{GAP, PAD_Y, RADIUS, ROW_GAP, SIDE, row};
use super::{BORDER, enter};
use crate::images;

/// The menu's width: narrower than the character menu's, its rows being a
/// word or two.
const MENU_W: f32 = 230.0;

/// One game's line: its emblem when one is cached, its name, its note.
fn line(e: &Entry, w: &W) -> gpui_kit::Div {
    let emblem = images::edition_emblem(e.edition, w.device(w.z(w.size.body)));
    div()
        .flex()
        .items_center()
        .gap(w.z(GAP))
        .children(emblem.map(|tile| {
            gpui_kit::img(tile)
                .size(w.z(w.size.body))
                .flex_none()
                .into_any_element()
        }))
        .child(div().flex_1().min_w_0().overflow_hidden().child(w.text(
            e.name.clone(),
            w.size.body,
            w.c(|t| t.ink),
            SEMIBOLD,
        )))
        .children(
            e.note
                .map(|note| w.text(note, w.size.small, w.c(|t| t.ink_3_text), REGULAR)),
        )
}

/// The menu over the window, under the switch.
pub fn view(gui: &Gui, w: &W, window: &mut Window, cx: &mut Context<Gui>) -> AnyElement {
    let Some(entries) = gui.edition_entries() else {
        return div().into_any_element();
    };
    let mut list = div()
        .w(w.z(MENU_W - 2.0 * BORDER))
        .flex()
        .flex_col()
        .gap(w.z(ROW_GAP));
    for e in &entries {
        let edition = e.edition;
        let id = ElementId::Name(SharedString::from(e.id.clone()));
        let entry = tip(
            row(e.name.clone().into(), id, e.on, line(e, w), w),
            e.tip.clone(),
        )
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |this, _, _, cx| {
                cx.stop_propagation();
                this.pick_edition(edition, cx);
            }),
        );
        list = list.child(entry);
    }
    let card = div()
        .id("edition-menu")
        .test_support()
        .occlude()
        .py(w.z(PAD_Y - BORDER))
        .border(w.z(1.))
        .rounded(w.r(RADIUS))
        .floating(w, w.c(|t| t.surface), w.c(|t| t.edge), w.shadows.menu)
        .child(list);
    // Hung under the switch, its left edge on the switch's, kept inside
    // the window.
    let at = gui.edition_at.get();
    let side = f32::from(w.z(SIDE));
    let left = f32::from(at.origin.x)
        .min(w.width * w.zoom - f32::from(w.z(MENU_W)) - side)
        .max(side);
    div()
        .absolute()
        .inset_0()
        .child(
            div()
                .id("edition-scrim")
                .test_support()
                .absolute()
                .inset_0()
                .occlude()
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, _, _, cx| this.close_menus(cx)),
                ),
        )
        .child(
            div()
                .absolute()
                .top(w.z(w.pitch.top_bar))
                .left(gpui_kit::px(left))
                .child(enter("edition-enter", card, w, window, cx)),
        )
        .into_any_element()
}
