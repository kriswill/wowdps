//! The character menu (`.menu`; the iced window's `nav::character_menu`),
//! opened from the top bar's picker: the follow item, checked, a rule, then
//! every character the window knows you play — icon, class-coloured name,
//! when they last played — the one Home is scoped to lit. The follow item
//! says what it does and changes nothing; a character scopes Home. Drawn
//! at the window's root over a scrim that takes the press that closes it,
//! hung from the picker it belongs to.

use gpui_kit::prelude::*;
use gpui_kit::{
    AnyElement, Context, Div, ElementId, MouseButton, SharedString, TestSupportExt as _, Window,
    div,
};
use wowdps_gui_logic::glyph::Glyph;
use wowdps_gui_logic::home::{CharLine, FOLLOW, played_note};
use wowdps_gui_logic::labels::display_name;
use wowdps_gui_logic::theme::{AA_CONTRAST, SHADOW_MENU, class_text_on};

use super::super::Gui;
use super::super::chrome::{class_icon, hairline};
use super::super::paint::glyph;
use super::super::top_bar::PICKER_END;
use super::super::w::{REGULAR, SEMIBOLD, W};
use super::enter;
use crate::theme::hsla;

/// The menu's pieces (`.menu{border-radius:8px;padding:6px 0}`, `.mi{gap:
/// 10px;padding:7px 12px}`, `.menu hr{margin:6px 0}`): the lit row's
/// accent edge taking two of the row's 12 px, 2 px between rows, and never
/// nearer the window's left edge than 10 px.
const RADIUS: f32 = 8.0;
const PAD_Y: f32 = 6.0;
const GAP: f32 = 10.0;
const ROW_GAP: f32 = 2.0;
const ROW_PAD: (f32, f32) = (7.0, 10.0);
const EDGE: f32 = 2.0;
const RULE_Y: f32 = 6.0;
const ROW_RADIUS: f32 = 3.0;
const SIDE: f32 = 10.0;

/// A menu row's id: "follow", or the character's guid.
pub fn row_id(guid: Option<&str>) -> ElementId {
    ElementId::Name(SharedString::from(match guid {
        Some(g) => format!("menu-{g}"),
        None => "menu-follow".to_string(),
    }))
}

/// The menu over the window, under the picker.
pub fn view(gui: &Gui, w: &W, window: &mut Window, cx: &mut Context<Gui>) -> AnyElement {
    // Lit: the character Home is scoped to while it is up, else the one it
    // opens on.
    let selected = match (gui.place, gui.hist.store.home.as_ref()) {
        (super::super::Place::Home, Some(home)) => home.scope.clone(),
        _ => gui.cfg.character.clone(),
    };
    let tonight = gui.tonight();
    let hide_realms = gui.cfg.hide_realms;
    let follow = row(
        FOLLOW.into(),
        None,
        false,
        div()
            .flex()
            .items_center()
            .gap(w.z(GAP))
            .child(glyph(Glyph::Check, w.z(w.size.body), w.c(|t| t.ink)))
            .child(w.text(FOLLOW, w.size.body, w.c(|t| t.ink), REGULAR)),
        w,
    )
    .on_mouse_down(
        MouseButton::Left,
        cx.listener(|this, _, _, cx| {
            cx.stop_propagation();
            this.picker_follow(cx);
        }),
    );
    let mut list = div()
        .w(w.z(w.pitch.menu_w))
        .flex()
        .flex_col()
        .gap(w.z(ROW_GAP))
        .child(follow)
        // `.menu hr`: the follow item is ruled off from the characters.
        .child(div().py(w.z(RULE_Y)).child(hairline(w)));
    for c in &gui.hist.known {
        let on = selected.as_deref() == Some(c.guid.as_str());
        let guid = c.guid.clone();
        list = list.child(
            row(
                shown_name(c, hide_realms).into(),
                Some(&c.guid),
                on,
                character(c, hide_realms, tonight, w),
                w,
            )
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, _, _, cx| {
                    cx.stop_propagation();
                    this.pick_character(guid.clone(), cx);
                }),
            ),
        );
    }
    let card = div()
        .id("picker-menu")
        .test_support()
        .occlude()
        .py(w.z(PAD_Y))
        .bg(w.c(|t| t.surface))
        .border(w.z(1.))
        .border_color(w.c(|t| t.edge))
        .rounded(w.z(RADIUS))
        .shadow(vec![w.shadow(SHADOW_MENU)])
        .child(list);
    // A press on the scrim — anywhere but a row — closes the menu.
    div()
        .absolute()
        .inset_0()
        .child(
            div()
                .id("picker-scrim")
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
                .right(w.z(PICKER_END))
                .ml(w.z(SIDE))
                .child(enter("picker-enter", card, w, window, cx)),
        )
        .into_any_element()
}

/// A character's name as the menu shows it: the realm stripped when the
/// option says so.
fn shown_name(c: &CharLine, hide_realms: bool) -> String {
    if hide_realms {
        display_name(&c.name).to_string()
    } else {
        c.name.clone()
    }
}

/// A character's line: their spec icon, their name in their class's text
/// colour — a person keeps their colour when picked; the pick is said by
/// the raised row and its accent edge — and when they last played.
fn character(c: &CharLine, hide_realms: bool, tonight: i64, w: &W) -> Div {
    let name = shown_name(c, hide_realms);
    let ink = c.class.map_or(w.c(|t| t.ink_2), |class| {
        hsla(class_text_on(class, w.t.surface, AA_CONTRAST))
    });
    let last = (c.last_local_ms != 0).then_some(c.last_local_ms);
    div()
        .flex()
        .items_center()
        .gap(w.z(GAP))
        .child(class_icon(w, c.class, c.spec, w.z(w.size.body), false))
        .child(div().flex_1().min_w_0().overflow_hidden().child(w.text(
            name,
            w.size.body,
            ink,
            SEMIBOLD,
        )))
        .child(w.text(
            played_note(last, c.fights, tonight),
            w.size.small,
            w.c(|t| t.ink_3_text),
            REGULAR,
        ))
}

/// One row: the lit one raised with the accent's edge at its left; the
/// rest washed under the pointer, fainter and borderless, so a hover can
/// sit on the lit row without arguing with it.
fn row(
    label: SharedString,
    guid: Option<&str>,
    on: bool,
    line: Div,
    w: &W,
) -> gpui_kit::base::ObservedElement<gpui_kit::Stateful<Div>> {
    let accent = w.accent();
    div()
        .id(row_id(guid))
        .aria_label(label)
        .test_support()
        .aria_selected(on)
        .w_full()
        .flex()
        .rounded(w.z(ROW_RADIUS))
        .cursor_pointer()
        .when(on, |d| d.bg(w.c(|t| t.raise)))
        .when(!on, |d| d.hover(|s| s.bg(w.c(|t| t.hover))))
        .child(div().w(w.z(EDGE)).flex_none().when(on, |d| d.bg(accent)))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .py(w.z(ROW_PAD.0))
                .px(w.z(ROW_PAD.1))
                .child(line),
        )
}
