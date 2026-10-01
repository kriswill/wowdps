//! The inventory tab: a pasted profile's equipped gear, bag items and
//! currencies, or (v19) the logged gear a combat-log loadout carried — ids
//! only, so `item {id}` is the honest label there. The words are
//! gui-logic's (`gear_slot`, `extras`, `item_name`, `currency_kind`).

use gpui_kit::prelude::*;
use gpui_kit::{AnyElement, Div, TestSupportExt as _, div, px};
use wowdps_gui_logic::simc;
use wowdps_gui_logic::talents as logic;

use super::Paint;

/// One item line: slot, name, what it carries, item level.
fn item_line(p: &Paint, slot: String, name: String, extras: String, ilvl: String) -> Div {
    div()
        .flex()
        .items_center()
        .gap(px(8.))
        .child(p.text(slot, 12., p.w(|t| t.ink_2)).w(px(80.)).flex_none())
        .child(
            div()
                .flex_1()
                .min_w_0()
                .child(p.text(name, 12., p.w(|t| t.ink))),
        )
        .child(p.text(extras, 12., p.w(|t| t.ink_2)))
        .child(
            div()
                .w(px(36.))
                .flex_none()
                .flex()
                .justify_end()
                .child(p.text(ilvl, 12., p.w(|t| t.good))),
        )
}

fn section(p: &Paint, title: &'static str) -> Div {
    p.text(title, 12., p.w(|t| t.gold_dim))
}

fn scroll(list: Div) -> AnyElement {
    div()
        .id("talent-inventory")
        .test_support()
        .size_full()
        .overflow_y_scroll()
        .child(list)
        .into_any_element()
}

fn item(p: &Paint, i: &simc::Item) -> Div {
    item_line(
        p,
        i.slot.clone(),
        logic::item_name(i),
        logic::extras(i.enchant_id.is_some(), i.gem_ids.len()),
        i.ilvl.map(|v| v.to_string()).unwrap_or_default(),
    )
}

/// A pasted profile's inventory.
pub(crate) fn profile(p: &Paint, profile: &simc::Profile) -> AnyElement {
    let mut col = div().flex().flex_col().gap(px(4.));
    if !profile.equipped.is_empty() {
        col = col.child(section(p, "equipped"));
        col = col.children(profile.equipped.iter().map(|i| item(p, i)));
    }
    if !profile.bags.is_empty() {
        col = col.child(section(p, "in bags").mt(px(6.)));
        col = col.children(profile.bags.iter().map(|i| item(p, i)));
    }
    if !profile.currencies.is_empty() {
        col = col.child(section(p, "currencies").mt(px(6.)));
        col = col.children(profile.currencies.iter().map(|c| {
            div()
                .flex()
                .gap(px(8.))
                .child(
                    p.text(logic::currency_kind(c), 12., p.w(|t| t.ink_2))
                        .w(px(70.)),
                )
                .child(p.text(c.id.to_string(), 12., p.w(|t| t.ink)).w(px(80.)))
                .child(p.text(format!("× {}", c.amount), 12., p.w(|t| t.ink)))
        }));
    }
    scroll(col)
}

/// v19: the logged gear, in the log's slot order.
pub(crate) fn logged(p: &Paint, gear: &[wowdps_model::GearItem]) -> AnyElement {
    let rows = gear
        .iter()
        .enumerate()
        // Empty slots log as zeroed tuples; a row of zeros says nothing.
        .filter(|(_, g)| g.item_id != 0)
        .map(|(i, g)| {
            item_line(
                p,
                logic::gear_slot(i, gear.len()).to_string(),
                format!("item {}", g.item_id),
                logic::extras(!g.enchants.is_empty(), g.gems.len()),
                if g.ilvl > 0 {
                    g.ilvl.to_string()
                } else {
                    String::new()
                },
            )
        });
    scroll(
        div()
            .flex()
            .flex_col()
            .gap(px(4.))
            .child(section(p, "equipped — from combat log"))
            .children(rows),
    )
}
