//! The overlay's drill rows (plan step 2.4), as the iced overlay draws
//! them (`view::overlay_drill_line`, `recap_row`, `spell_breadcrumb`,
//! `spell_stats`, `spell_target_list`): an ability drill's crumb, stat
//! cards and targets; a player drill's caption and its lines — R26's tree
//! on Damage and Healing, a death's recap, an enemy's attackers, else one
//! line per ability. The words are gui-logic's (`drill`, `tree`); the
//! pieces here are the overlay's pixels.

use gpui_kit::prelude::*;
use gpui_kit::{AnyElement, Div, Hsla, div, img, px, relative};
use wowdps_gui_logic::drill::{
    self as gd, OVERLAY_CARET_W, OVERLAY_DRILL_COLS, StatTone, school_name,
};
use wowdps_gui_logic::theme::{Color, school_color};
use wowdps_model::fmt::human;
use wowdps_model::{Row, View};

use super::ov::Ov;
use super::rows::{bar_color, fill, under_bar};
use crate::images;
use crate::theme::hsla;

/// What leads a line of a drill (R26): nothing on a flat drill, exactly as
/// it always drew; on a tree drill a caret column and an art column on
/// EVERY line, so a group's name and a lone row's start together.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lead {
    Flat,
    /// A row of a tree drill: both columns, the caret's blank.
    Row,
    /// A group of a tree drill, its caret open or shut.
    Group {
        open: bool,
    },
}

/// One ability line: `label` `indent` px in at zoom 1 after its `lead`,
/// then hits, crit and total in the drill's columns (a count view's one
/// count), over the ability's bar.
pub fn line(ov: &Ov, r: &Row, lead: Lead, label: &str, indent: f32, max: u64, counts: bool) -> Div {
    let (w_hits, w_crit, w_total) = OVERLAY_DRILL_COLS;
    let primary = ov.c(|t| t.ink);
    let secondary = ov.c(|t| t.rate);
    let caret = match lead {
        Lead::Flat => None,
        Lead::Row => Some(""),
        Lead::Group { open: true } => Some("▾"),
        Lead::Group { open: false } => Some("▸"),
    };
    let art = images::spell_icon(r.spell_id);
    let mut words = div()
        .flex()
        .items_center()
        .gap(px(4.))
        .pr(px(8.))
        .pl(px(8.) + ov.z(indent))
        .children(caret.map(|c| {
            ov.words(c, 11., ov.c(|t| t.dim))
                .w(ov.z(OVERLAY_CARET_W))
                .flex_none()
        }));
    words = match art {
        Some(tile) => words.child(img(tile).size(ov.z(12.)).flex_none()),
        None if lead != Lead::Flat => words.child(div().w(ov.z(12.)).flex_none()),
        None => words,
    };
    words = words.child(div().flex_1().min_w_0().overflow_hidden().child(ov.words(
        label.to_string(),
        12.,
        ov.c(|t| t.text),
    )));
    words = if counts {
        words.child(ov.metric(human(r.count), 12., primary, w_total))
    } else {
        // One size across the three: the total already leads by colour.
        words
            .child(ov.metric(human(r.count), 12., secondary, w_hits))
            .child(ov.metric(format!("{:.0}%", r.crit_pct()), 12., secondary, w_crit))
            .child(ov.metric(human(r.amount), 12., primary, w_total))
    };
    under_bar(ov, bar_color(ov, r), fill(r.amount, max), words, ov.z(20.))
}

/// The caption over a player drill: whose it is, and the columns' names
/// in the columns' widths — a recap's amount and health, a count view's
/// count, else hits, crit and total.
pub fn caption(ov: &Ov, who: &str, view: View) -> Div {
    let (w_hits, w_crit, w_total) = OVERLAY_DRILL_COLS;
    let dim = ov.c(|t| t.dim);
    let cap =
        |s: &'static str, width: f32| ov.nums(s, 9., dim).w(ov.z(width)).flex_none().text_right();
    let mut line = div()
        .flex()
        .items_center()
        .gap(px(4.))
        .px(px(8.))
        .child(ov.words(who.to_string(), 11., ov.c(|t| t.yellow)))
        .child(div().flex_1());
    line = if view == View::Deaths {
        line.child(cap("amount", 52.)).child(cap("hp", 40.))
    } else if gd::counts(view) {
        line.child(cap("count", w_total))
    } else {
        line.child(cap("hits", w_hits))
            .child(cap("crit", w_crit))
            .child(cap("total", w_total))
    };
    line
}

/// A death-recap line (R9): the event's bar on top — red for damage,
/// green for heals and consumed absorbs, a hotter red for the killing
/// blow, scaled to the recap's biggest — over a 3z strip of the health
/// left after it; the words over both.
pub fn recap(ov: &Ov, r: &Row, max: u64) -> Div {
    let t = &ov.t;
    let (color, alpha) = if r.gain {
        (t.good, 0.30)
    } else if r.extra > 0 {
        (t.bad, 0.55)
    } else {
        (t.bad, 0.30)
    };
    let part = |c: Color, pct: u16| {
        div()
            .h_full()
            .w(relative(f32::from(pct) / 100.0))
            .rounded(px(2.))
            .when(pct > 0, |d| d.bg(hsla(c)))
    };
    let health = match r.hp {
        Some((cur, max_hp)) => div()
            .w_full()
            .h(ov.z(3.))
            .flex_none()
            .bg(hsla(t.health_track))
            .child(part(t.health.alpha(0.55), gd::whole_pct(cur, max_hp))),
        None => div().w_full().h(ov.z(3.)).flex_none(),
    };
    let bars = div()
        .absolute()
        .inset_0()
        .flex()
        .flex_col()
        .child(
            div()
                .flex_1()
                .min_h_0()
                .w_full()
                .child(part(color.alpha(alpha), gd::whole_pct(r.amount, max))),
        )
        .child(health);
    let words = div()
        .absolute()
        .inset_0()
        .flex()
        .items_center()
        .gap(px(4.))
        .px(px(8.))
        .child(ov.words(gd::compact_recap_label(&r.label), 12., ov.c(|t| t.text)))
        .child(div().flex_1())
        .child(ov.metric(
            gd::recap_amount(r),
            12.,
            if r.gain { hsla(t.good) } else { hsla(t.ink) },
            52.,
        ))
        .child(ov.metric(gd::recap_hp(r), 11., hsla(t.dim), 40.));
    div()
        .relative()
        .w_full()
        .h(ov.z(20.))
        .rounded(px(3.))
        .child(bars)
        .child(words)
}

/// v16: the ability drill's crumb — "Player ▸ ⬚ Spell [School]", the
/// ability in its school's colour, the school as a chip on the right.
pub fn crumb(ov: &Ov, player: &str, spell: &str, row: Option<&Row>) -> Div {
    let name = player.split('-').next().unwrap_or(player).to_string();
    let school = row.and_then(|r| school_color(r.school));
    let mut line = div()
        .flex()
        .items_center()
        .gap(ov.z(6.))
        .py(px(2.))
        .px(px(8.))
        .child(ov.words(name, 13., ov.c(|t| t.yellow)))
        .child(ov.words("▸", 11., ov.c(|t| t.dim)));
    if let Some(tile) = row.and_then(|r| images::spell_icon(r.spell_id)) {
        line = line.child(img(tile).size(ov.z(14.)).flex_none());
    }
    line = line.child(div().flex_1().min_w_0().overflow_hidden().child(ov.words(
        spell.to_string(),
        13.,
        school.map_or(ov.c(|t| t.ink), hsla),
    )));
    if let Some((word, sc)) =
        row.and_then(|r| school_name(r.school).map(|n| (n, school_color(r.school))))
    {
        let sc = sc.unwrap_or(ov.t.dim);
        line = line.child(
            div()
                .flex_none()
                .py(ov.z(1.))
                .px(ov.z(5.))
                .rounded(px(3.))
                .bg(hsla(sc.alpha(0.10)))
                .border_1()
                .border_color(hsla(sc.alpha(0.55)))
                .child(ov.words(word, 9., hsla(sc))),
        );
    }
    line
}

/// v16: the ability drill's stat strip, one card per number, sharing the
/// panel's width.
pub fn stats(ov: &Ov, r: &Row, view: View) -> Div {
    let cards = gd::stat_cards(r, view).into_iter().map(|c| {
        let ink: Hsla = match c.tone {
            StatTone::Plain => ov.c(|t| t.ink),
            StatTone::Crit => ov.c(|t| t.yellow),
            StatTone::Bad => ov.c(|t| t.bad),
        };
        div()
            .flex_1()
            .min_w_0()
            .flex()
            .flex_col()
            .items_center()
            .gap(px(2.))
            .py(ov.z(5.))
            .px(ov.z(4.))
            .rounded(px(5.))
            .bg(ov.c(|t| t.stat_card))
            .border_1()
            .border_color(ov.c(|t| t.stat_card_edge))
            .child(ov.nums(c.value, 13., ink))
            .child(ov.words(c.label, 9., ov.c(|t| t.dim)))
    });
    div()
        .flex()
        .gap(ov.z(6.))
        .py(px(4.))
        .px(px(8.))
        .children(cards)
}

/// v17: who the ability landed on — the caption, then a row per target:
/// name, hits, amount and share over the target's bar.
pub fn targets(ov: &Ov, rows: &[Row], view: View) -> Vec<AnyElement> {
    let dim = ov.c(|t| t.dim);
    let head = div()
        .flex()
        .items_center()
        .py(px(2.))
        .px(px(8.))
        .child(ov.words(
            if view == View::EnemyTaken {
                "abilities"
            } else {
                "targets"
            },
            10.,
            dim,
        ))
        .child(div().flex_1())
        .child(ov.nums("hits · total · %", 9., dim))
        .into_any_element();
    let max = rows.first().map_or(1, |r| r.amount.max(1));
    std::iter::once(head)
        .chain(rows.iter().map(|r| {
            let words = div()
                .flex()
                .items_center()
                .gap(px(4.))
                .px(px(8.))
                .child(div().flex_1().min_w_0().overflow_hidden().child(ov.words(
                    r.label.clone(),
                    12.,
                    ov.c(|t| t.text),
                )))
                .child(ov.metric(human(r.count), 11., ov.c(|t| t.rate), 44.))
                .child(ov.metric(human(r.amount), 12., ov.c(|t| t.ink), 52.))
                .child(ov.metric(format!("{:.1}%", r.pct), 11., dim, 44.));
            under_bar(ov, bar_color(ov, r), fill(r.amount, max), words, ov.z(20.))
                .into_any_element()
        }))
        .collect()
}
