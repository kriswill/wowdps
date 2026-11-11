//! The edition menu, opened from the top bar's edition switch: Follow the
//! game, ruled off from one row per edition the window knows — its name
//! ("Midnight", "Forever"), the edition behind a name that is not its own
//! in quieter words, the build in its tooltip — the window's choice lit.
//! Drawn at the window's root over a scrim that takes the press that
//! closes it, hung under the switch, as the character menu hangs under the
//! picker.

use gpui_kit::prelude::*;
use gpui_kit::{
    AnyElement, Context, ElementId, MouseButton, SharedString, TestSupportExt as _, Window, div,
};
use wowdps_gui_logic::editions::Entry;
use wowdps_proto::EditionRef;

use super::super::Gui;
use super::super::chrome::{hairline, tip};
use super::super::w::Floating;
use super::super::w::{REGULAR, SEMIBOLD, W};
use super::menu::{GAP, PAD_Y, RADIUS, ROW_GAP, RULE_Y, SIDE, row};
use super::{BORDER, enter};
use crate::images;

/// The menu's width: narrower than the character menu's, its rows being a
/// word or two.
const MENU_W: f32 = 230.0;

/// One entry's line: its emblem when one is cached, its name, its note.
fn line(e: &Entry, w: &W) -> gpui_kit::Div {
    let emblem = match e.choice {
        EditionRef::Pin(edition) => images::edition_emblem(edition, w.device(w.z(w.size.body))),
        EditionRef::Follow => None,
    };
    let weight = if e.choice == EditionRef::Follow {
        REGULAR
    } else {
        SEMIBOLD
    };
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
            weight,
        )))
        .children(
            e.note
                .clone()
                .map(|note| w.text(note, w.size.small, w.c(|t| t.ink_3_text), REGULAR)),
        )
}

/// The menu over the window, under the switch.
pub fn view(gui: &Gui, w: &W, window: &mut Window, cx: &mut Context<Gui>) -> AnyElement {
    let Some(entries) = gui.edition_entries(cx) else {
        return div().into_any_element();
    };
    let mut list = div()
        .w(w.z(MENU_W - 2.0 * BORDER))
        .flex()
        .flex_col()
        .gap(w.z(ROW_GAP));
    for (i, e) in entries.iter().enumerate() {
        let choice = e.choice;
        let id = ElementId::Name(SharedString::from(e.id.clone()));
        let entry = tip(
            row(e.name.clone().into(), id, e.on, line(e, w), w),
            e.tip.clone(),
        )
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |this, _, _, cx| {
                cx.stop_propagation();
                this.pick_edition(choice, cx);
            }),
        );
        list = list.child(entry);
        // Follow is ruled off from the editions, as the character menu's
        // follow item is from the characters.
        if i == 0 {
            list = list.child(div().py(w.z(RULE_Y)).child(hairline(w)));
        }
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
