//! The view tabs under the fight header (the prototype's `.vtabs`, the
//! iced window's `view_tabs_with` over `nav::view_strip`): the views in the
//! prototype's order and words, each a line icon and its name, the active
//! one ink over a 2 px accent underline, the row filter at the row's end,
//! all over one hairline the full width.
//!
//! A window too narrow for the strip shows part of it with the ACTIVE tab
//! whole in sight (the iced `reveal`): GPUI's `scroll_to_item` is the same
//! least move (`reveal::nearest`), asked whenever the active tab or the
//! window's width changed, and a wheel moves the strip along in between.
//! An edge with more of the strip past it fades into the ground.
//!
//! The delights: the underline glides from tab to tab on a spring, and the
//! filter widens with focus over a short ease — both settled at once under
//! reduced motion, onto the iced window's pixels.

use std::time::Duration;

use gpui_kit::base::{Easing, Spring, Transition, spring, transition};
use gpui_kit::component::input::Input;
use gpui_kit::prelude::*;
use gpui_kit::{
    Context, Div, ElementId, Focusable as _, MouseButton, ScrollWheelEvent, SharedString,
    TestSupportExt as _, Window, div, linear_color_stop, linear_gradient, point, px,
};
use wowdps_gui_logic::glyph::Glyph;
use wowdps_gui_logic::labels::{WINDOW_VIEWS, window_view_name};
use wowdps_gui_logic::reveal::{FADE, LINE, cut_edges};
use wowdps_gui_logic::theme::TAB_ICON_ALPHA;
use wowdps_model::View;

use super::Gui;
use super::chrome::kbd;
use super::paint::{glyph, glyph_ink};
use super::w::{MEDIUM, W};

/// A tab's sides (`.vtab{padding:0 9px}`) and its icon's gap.
const TAB_PAD: f32 = 9.0;
const TAB_GAP: f32 = 6.0;
/// The row's inset (`.vtabs{padding:0 12px 0 10px}`) and the gap before
/// the filter.
const INSET_LEFT: f32 = 10.0;
const INSET_RIGHT: f32 = 12.0;
const FILTER_SPACE: f32 = 8.0;
/// The filter (`.filter{height:28px}`, its text 92 px wide, 140 with
/// focus), its inset, and its clear mark's target and glyph.
const FILTER_H: f32 = 28.0;
pub const FILTER_W: f32 = 92.0;
pub const FILTER_W_FOCUSED: f32 = 140.0;
const FILTER_INSET: f32 = 8.0;
const FILTER_CLEAR: f32 = 24.0;
const FILTER_CLEAR_GLYPH: f32 = 12.0;
/// The underline's spring, and the filter's widening.
const GLIDE: Spring = Spring::new(Duration::from_millis(300)).with_epsilon(0.1);
const WIDEN: Duration = Duration::from_millis(160);

/// A view tab's id.
pub fn tab_id(view: View) -> ElementId {
    ElementId::from((
        ElementId::Name("view-tab".into()),
        SharedString::from(window_view_name(view)),
    ))
}

/// The strip and the filter, over the hairline.
pub fn view(gui: &Gui, w: &W, window: &mut Window, cx: &mut Context<Gui>) -> impl IntoElement {
    let view = gui.session.read(cx).state().view;
    let active_at = WINDOW_VIEWS.iter().position(|v| *v == view);
    let handle = gui.tab_scroll.clone();
    // The active tab whole in sight whenever it, or the window, changed.
    let key = (view, (w.width * 4.0).round() as i64);
    if gui.tab_revealed.get() != Some(key)
        && let Some(i) = active_at
    {
        handle.scroll_to_item(i);
        gui.tab_revealed.set(Some(key));
    }
    let underline_at = active_at.and_then(|i| handle.bounds_for_item(i)).map(|b| {
        let strip = handle.bounds();
        (
            f32::from(b.origin.x - strip.origin.x),
            f32::from(b.size.width),
        )
    });
    let tabs = WINDOW_VIEWS
        .iter()
        .map(|&v| tab(w, v, v == view, underline_at.is_none(), cx));
    let strip = div()
        .id("tab-strip")
        .flex()
        .size_full()
        .overflow_x_scroll()
        .track_scroll(&handle)
        .children(tabs);
    // The underline, gliding: drawn on its own over the strip once the
    // tabs have been laid out, at the active tab's place (less how far the
    // strip is scrolled) on a spring.
    let scroll = f32::from(handle.offset().x);
    let underline = underline_at.map(|(x, width)| {
        let x = spring(("tab-underline", "x"), x, GLIDE, window, cx);
        let width = spring(("tab-underline", "w"), width, GLIDE, window, cx);
        div()
            .absolute()
            .bottom_0()
            .left(px(x + scroll))
            .w(px(width))
            .h(w.z(2.))
            .bg(w.accent())
    });
    let width = f32::from(handle.bounds().size.width);
    let (cut_left, cut_right) = cut_edges(-scroll, width, width + f32::from(handle.max_offset().x));
    let fade = |left: bool| {
        let ground = w.c(|t| t.ground);
        div()
            .absolute()
            .top_0()
            .bottom_0()
            .w(w.z(FADE))
            .when(left, |d| d.left_0())
            .when(!left, |d| d.right_0())
            .bg(linear_gradient(
                if left { 90. } else { 270. },
                linear_color_stop(ground, 0.),
                linear_color_stop(ground.opacity(0.), 1.),
            ))
    };
    let wheel = handle.clone();
    let window_onto_strip = div()
        .id("tab-window")
        .flex_1()
        .min_w_0()
        .h_full()
        .relative()
        .child(strip)
        .children(underline)
        .when(cut_left, |d| d.child(fade(true)))
        .when(cut_right, |d| d.child(fade(false)))
        .on_scroll_wheel(move |e: &ScrollWheelEvent, window, _| {
            // A notch down moves the strip along to its later tabs.
            let along = match e.delta {
                gpui_kit::ScrollDelta::Lines(l) => l.y * LINE,
                gpui_kit::ScrollDelta::Pixels(p) => f32::from(p.y),
            };
            let max = f32::from(wheel.max_offset().x).max(0.);
            let x = (f32::from(wheel.offset().x) + along).clamp(-max, 0.);
            wheel.set_offset(point(px(x), wheel.offset().y));
            window.refresh();
        });
    div()
        .id("view-tabs")
        .w_full()
        .flex_none()
        .flex()
        .flex_col()
        .child(
            div()
                .h(w.z(w.pitch.tab))
                .w_full()
                .flex()
                .items_center()
                .gap(w.z(FILTER_SPACE))
                .pl(w.z(INSET_LEFT))
                .pr(w.z(INSET_RIGHT))
                .child(window_onto_strip)
                .child(filter_box(gui, w, window, cx)),
        )
        .child(super::chrome::hairline(w))
}

/// One tab (`.vtab`): its line icon at 85 % and its name at 500, ink when
/// active or under the pointer, secondary otherwise. `underline`: draw the
/// active one's accent line here (before the strip is laid out, or when
/// motion is settled, the glide's line stands exactly where this would).
fn tab(
    w: &W,
    v: View,
    active: bool,
    underline: bool,
    cx: &mut Context<Gui>,
) -> impl IntoElement + use<> {
    let ink = if active {
        w.c(|t| t.ink)
    } else {
        w.c(|t| t.ink_2)
    };
    div()
        .id(tab_id(v))
        .test_support()
        .aria_selected(active)
        .flex_none()
        .h_full()
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
                .gap(w.z(TAB_GAP))
                .px(w.z(TAB_PAD))
                .child(
                    div()
                        .opacity(TAB_ICON_ALPHA)
                        .child(glyph_ink(Glyph::of_view(v), w.z(w.size.tab_icon))),
                )
                .child(w.words(window_view_name(v), w.size.tab, MEDIUM)),
        )
        .child(
            div()
                .h(w.z(2.))
                .w_full()
                .when(active && underline, |d| d.bg(w.accent())),
        )
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |this, _, _, cx| this.pick_view(v, cx)),
        )
}

/// The row filter (`.filter`): a search glyph, the field and its `/`
/// keycap — no frame until the pointer finds it, the ground inside the
/// floating edge with focus, and wider then. A clear mark rides between
/// the field and the keycap while there is text.
fn filter_box(gui: &Gui, w: &W, window: &mut Window, cx: &mut Context<Gui>) -> Div {
    let focused = gui.filter.focus_handle(cx).is_focused(window);
    let text_w = transition(
        ("row-filter", "width"),
        if focused { FILTER_W_FOCUSED } else { FILTER_W },
        Transition::new(WIDEN).easing(Easing::EaseOut),
        window,
        cx,
    );
    let clearing = !gui.filter_text.is_empty();
    div()
        .flex_none()
        .h(w.z(FILTER_H))
        .flex()
        .items_center()
        .gap(w.z(6.))
        .px(w.z(FILTER_INSET))
        .rounded(w.z(6.))
        .border(w.z(1.))
        .when(focused, |d| {
            d.bg(w.c(|t| t.ground)).border_color(w.c(|t| t.edge))
        })
        .when(!focused, |d| {
            d.border_color(gpui_kit::transparent_black())
                .hover(|s| s.border_color(w.c(|t| t.line)))
        })
        .child(glyph(Glyph::Search, w.z(w.size.icon), w.c(|t| t.ink_3)))
        .child(
            div()
                .id("row-filter")
                .test_support()
                .key_context("Filter")
                .w(w.z(text_w))
                .text_size(w.z(w.size.filter))
                .on_action(cx.listener(|this, _: &super::FilterDone, window, cx| {
                    this.filter_done(window, cx)
                }))
                // Kit pads its field by its size; the box is the inset here.
                .child(Input::new(&gui.filter).appearance(false).px_0().py_0()),
        )
        .when(clearing, |d| {
            d.child(
                div()
                    .id("row-filter-clear")
                    .test_support()
                    .size(w.z(FILTER_CLEAR))
                    .flex()
                    .items_center()
                    .justify_center()
                    .cursor_pointer()
                    .child(glyph(
                        Glyph::Close,
                        w.z(FILTER_CLEAR_GLYPH),
                        w.c(|t| t.ink_2),
                    ))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this, _, window, cx| this.clear_filter(window, cx)),
                    ),
            )
        })
        .child(kbd(w, "/"))
}
