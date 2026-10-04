//! The instance frame (plan step 2.3): the strip Σ–①─②─③–⚑ over the
//! watched visit, and the chip line under it, drawn as the iced overlay
//! draws them (`timeline::strip_in`, `overlay::chip`). The model — blocks,
//! items, wipe runs, the fan's offsets — is gui-logic's `timeline`.

use gpui_kit::prelude::*;
use gpui_kit::{AnyElement, Div, Hsla, div, px};
use wowdps_gui_logic::theme::Color;
use wowdps_gui_logic::timeline::{
    DISC, EMPHASIS, HIT_PAD_X, HIT_PAD_Y, Item, cascade_xs, gap_width, item_pos, natural_width,
};

use super::ov::Ov;
use crate::theme::hsla;

/// One strip element, with the entries position a press goes to.
pub struct Element {
    pub el: AnyElement,
    pub goto: Option<usize>,
}

/// The strip's elements for `items`, `selected` the watched position, laid
/// out inline when they fit `budget`, else fanned (later ones on top, the
/// watched one raised over all). `wrap` turns each element and its target
/// into the interactive element the caller wants (an id and a press).
pub fn strip(
    ov: &Ov,
    items: &[Item],
    selected: Option<usize>,
    budget: f32,
    wrap: impl Fn(usize, AnyElement, Option<usize>) -> AnyElement,
) -> Div {
    let z = ov.zoom;
    let watched = |item: &Item| item_pos(item).is_some() && item_pos(item) == selected;
    let widths: Vec<f32> = items
        .iter()
        .map(|i| natural_width(i, z, watched(i)))
        .collect();
    let focus = items
        .iter()
        .position(watched)
        .unwrap_or(items.len().saturating_sub(1));
    let pin_first = matches!(items.first(), Some(Item::Overall { .. }));
    let height = ov.z(DISC * EMPHASIS + 2.0 * HIT_PAD_Y);
    let els: Vec<AnyElement> = items
        .iter()
        .enumerate()
        .map(|(i, item)| {
            let Element { el, goto } = item_el(ov, item, selected);
            wrap(i, el, goto)
        })
        .collect();
    match cascade_xs(&widths, 1.5 * z, budget, focus, pin_first, z) {
        None => div()
            .h(height)
            .flex()
            .items_center()
            .gap(ov.z(1.5))
            .children(els),
        Some(xs) => {
            let mut layers: Vec<(f32, AnyElement)> = xs.into_iter().zip(els).collect();
            if let Some(at) = items.iter().position(watched) {
                let raised = layers.remove(at);
                layers.push(raised);
            }
            div()
                .relative()
                .w_full()
                .h(height)
                .children(layers.into_iter().map(|(x, el)| {
                    div()
                        .absolute()
                        .top_0()
                        .bottom_0()
                        .left(px(x))
                        .flex()
                        .items_center()
                        .child(el)
                }))
        }
    }
}

/// One element: its visual inside the padded hit box every clickable one
/// gets (the flag gets none and goes nowhere).
fn item_el(ov: &Ov, item: &Item, selected: Option<usize>) -> Element {
    let t = &ov.t;
    let hit = |el: AnyElement, pos: usize| Element {
        el: div()
            .py(ov.z(HIT_PAD_Y))
            .px(ov.z(HIT_PAD_X))
            .child(el)
            .into_any_element(),
        goto: Some(pos),
    };
    match *item {
        Item::Overall { pos, live } => hit(
            disc(
                ov,
                "Σ",
                t.yellow.alpha(0.20),
                t.yellow,
                selected == Some(pos),
                live,
            ),
            pos,
        ),
        Item::Boss {
            pos,
            num,
            success,
            live,
        } => {
            let fill = match (live, success) {
                (true, _) => t.pip_live,
                (_, Some(true)) => t.pip_kill,
                (_, Some(false)) => t.pip_wipe,
                (_, None) => t.pip,
            };
            hit(
                disc(
                    ov,
                    &num.to_string(),
                    fill,
                    t.ink,
                    selected == Some(pos),
                    live,
                ),
                pos,
            )
        }
        Item::Gap {
            pos,
            duration_ms,
            live,
            ..
        } => hit(gap_line(ov, duration_ms, selected == Some(pos), live), pos),
        Item::Wipes { pos, count } => hit(pill(ov, count), pos),
        Item::Flag { success } => Element {
            el: ov
                .words("⚑", 11., hsla(if success { t.good } else { t.bad }))
                .into_any_element(),
            goto: None,
        },
    }
}

/// A disc: a number or Σ on its fill, ringed — white and a step larger
/// when watched, yellow while live, else faint.
fn disc(ov: &Ov, label: &str, fill: Color, text: Color, selected: bool, live: bool) -> AnyElement {
    let emph = if selected { EMPHASIS } else { 1.0 };
    let dia = ov.z(DISC * emph);
    let t = &ov.t;
    let (ring, ring_w) = if selected {
        (t.ink, 1.5)
    } else if live {
        (t.yellow, 1.0)
    } else {
        (t.card_edge, 1.0)
    };
    div()
        .size(dia)
        .flex_none()
        .flex()
        .items_center()
        .justify_center()
        .rounded_full()
        .bg(hsla(fill))
        .border(px(ring_w))
        .border_color(hsla(ring))
        .child(ov.words(label.to_string(), 8.5 * emph, hsla(text)))
        .into_any_element()
}

/// A collapsed wipe run: a disc-tall pill reading ×N, wipe-red but flatter.
fn pill(ov: &Ov, count: usize) -> AnyElement {
    let dia = ov.z(DISC);
    div()
        .h(dia)
        .px(ov.z(4.))
        .flex()
        .items_center()
        .rounded(ov.pill(dia / 2.))
        .bg(ov.c(|t| t.bad.alpha(0.22)))
        .border_1()
        .border_color(ov.c(|t| t.card_edge))
        .child(ov.words(format!("×{count}"), 8.5, ov.c(|t| t.ink)))
        .into_any_element()
}

/// The trash connector: a thin line whose length hints at time spent,
/// centred in a disc-tall hit area.
fn gap_line(ov: &Ov, duration_ms: i64, selected: bool, live: bool) -> AnyElement {
    let w = gap_width(duration_ms, ov.zoom);
    let color: Hsla = if live {
        ov.c(|t| t.yellow)
    } else if selected {
        ov.c(|t| t.pip_lit)
    } else {
        ov.c(|t| t.dim)
    };
    div()
        .w(px(w))
        .h(ov.z(DISC))
        .flex()
        .items_center()
        .child(
            div()
                .w_full()
                .h(ov.z(if selected { 3.0 } else { 2.0 }))
                .rounded(ov.r(1.))
                .bg(color),
        )
        .into_any_element()
}
