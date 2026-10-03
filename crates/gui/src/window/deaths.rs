//! The Deaths view's table (R25; the prototype's `.v-deaths`, the iced
//! window's `deaths.rs`): every death in the order it happened, from the
//! snapshot's raid timeline — its time, who died, the killing blow and who
//! dealt it, when a rez raised them, the hit and its overkill in the
//! outcome red. A row is a press away from that death's recap. What a
//! death is called, which the filter keeps and which one is open are
//! gui-logic's (`deaths`).

use gpui_kit::prelude::*;
use gpui_kit::{
    Context, Div, ElementId, MouseButton, SharedString, TestSupportExt as _, div, relative,
};
use wowdps_gui_logic::deaths::{
    ENEMY, Pick, drawn, empty_words, is_mine, selected, total_words, words,
};
use wowdps_gui_logic::labels::display_name;
use wowdps_gui_logic::table::TOTAL_INSET;
use wowdps_model::fmt::{commas, duration};
use wowdps_model::{RaidDeath, View};

use super::chrome::{class_icon, hairline};
use super::table::you_tag;
use super::w::{MEDIUM, REGULAR, W};
use super::{Gui, Reveal};

/// The columns (`.v-deaths{--cols:46px minmax(0,150px) minmax(0,1fr) 74px
/// 74px}`; at 820 px and under `40px minmax(0,.8fr) minmax(0,1fr)`, the hit
/// and the overkill gone), their gap, and the lead and trail of a line.
const TIME_W: f32 = 46.0;
const TIME_W_NARROW: f32 = 40.0;
const PLAYER_W: f32 = 150.0;
const NUM_W: f32 = 74.0;
const COL_GAP: f32 = 12.0;
const LEAD: f32 = 4.0;
const TRAIL: f32 = 16.0;
const HEADS_TOP: f32 = 8.0;
const HEADS_BOTTOM: f32 = 6.0;
/// A player's disc, the gap after it, the killing blow's pieces and its
/// size.
const DISC: f32 = 20.0;
const WHO_GAP: f32 = 8.0;
const KB_GAP: f32 = 6.0;
const BLOW_PX: f32 = 14.0;
const EMPTY_PAD: f32 = 20.0;

/// The deaths the table draws.
pub struct Table {
    all: Vec<RaidDeath>,
    drawn: Vec<usize>,
    selected: Option<usize>,
    keys_away: bool,
    hide_realms: bool,
    filter: String,
    owner: Option<String>,
}

impl Table {
    /// The table for the stage — `None` off the Deaths view and without a
    /// raid timeline, where the count table stands.
    pub fn of(gui: &Gui, cx: &gpui_kit::App) -> Option<Self> {
        let app = gui.fight(cx);
        if app.view != View::Deaths {
            return None;
        }
        let raid = app.raid()?;
        Some(Table {
            drawn: drawn(raid, &gui.filter_text),
            selected: selected(app, raid),
            keys_away: app.inspecting(),
            hide_realms: gui.cfg.hide_realms,
            filter: gui.filter_text.clone(),
            owner: gui.death_owner(raid),
            all: raid.deaths.clone(),
        })
    }
}

/// One line's cells, in the columns at the window's breakpoint.
fn cells(narrow: bool, w: &W, cells: Vec<Div>) -> Div {
    let mut line = div().w_full().flex().items_center().gap(w.z(COL_GAP));
    for (i, cell) in cells.into_iter().enumerate() {
        let cell = cell.min_w_0().overflow_hidden();
        line = line.child(match (i, narrow) {
            (0, false) => cell.w(w.z(TIME_W)).flex_none(),
            (0, true) => cell.w(w.z(TIME_W_NARROW)).flex_none(),
            (1, false) => cell.w(w.z(PLAYER_W)).flex_none(),
            // `minmax(0,.8fr)` beside the blow's `1fr`.
            (1, true) => cell.flex_grow(0.8).flex_basis(relative(0.)).flex_shrink(1.),
            (2, true) => cell.flex_grow(1.).flex_basis(relative(0.)).flex_shrink(1.),
            (2, false) => cell.flex_1(),
            _ => cell.w(w.z(NUM_W)).flex_none().flex().justify_end(),
        });
    }
    line
}

/// The table: heads, hairline, the deaths, the total.
pub fn view(t: Table, gui: &Gui, w: &W, cx: &mut Context<Gui>) -> impl IntoElement {
    let narrow = w.narrow();
    let head = |words: &'static str| w.text(words, w.size.label, w.c(|t| t.label_ink), REGULAR);
    let mut heads = vec![head("Time"), head("Player"), head("Killing blow")];
    if !narrow {
        heads.push(head("Hit"));
        heads.push(head("Overkill"));
    }
    let heads = div()
        .flex_none()
        .pt(w.z(HEADS_TOP))
        .pb(w.z(HEADS_BOTTOM))
        .pl(w.z(LEAD))
        .pr(w.z(TRAIL))
        .child(cells(narrow, w, heads));
    let row_h = w.pitch.row_of(gui.cfg.density());
    let mut list = div()
        .id("deaths-list")
        .test_support()
        .flex()
        .flex_col()
        .flex_shrink(1.)
        .min_h_0()
        .overflow_y_scroll()
        .track_scroll(&gui.meter_scroll);
    if t.drawn.is_empty() {
        list = list.child(
            div().p(w.z(EMPTY_PAD)).child(
                w.text(
                    empty_words(!t.all.is_empty(), &t.filter),
                    w.size.body,
                    w.c(|t| t.ink_2),
                    REGULAR,
                )
                .whitespace_normal(),
            ),
        );
    }
    // A death to bring into sight: its place among the list's children.
    let reveal = gui.reveal.take();
    let mut child = 0;
    for &i in &t.drawn {
        if let Some(d) = t.all.get(i) {
            if matches!(&reveal, Some(Reveal::Death(key, at)) if *key == d.guid && *at == d.index) {
                gui.meter_scroll.scroll_to_item(child);
            }
            list = list.child(line(&t, gui, i, d, narrow, row_h, w, cx));
            child += 1;
        }
    }
    // The label stands over the Player column.
    let time_w = if narrow { TIME_W_NARROW } else { TIME_W };
    let lead_pad = (LEAD + time_w + COL_GAP - TOTAL_INSET).max(0.0);
    let total = div()
        .id("meter-total")
        .test_support()
        .flex_none()
        .w_full()
        .flex()
        .flex_col()
        .child(hairline(w))
        .child(
            div()
                .h(w.z(w.pitch.total - 1.0))
                .w_full()
                .flex()
                .items_center()
                .pl(w.z(TOTAL_INSET + lead_pad))
                .pr(w.z(TOTAL_INSET + w.pitch.scroll_lane))
                .bg(w.c(|t| t.surface))
                .child(
                    div()
                        .truncate()
                        .font_family(w.ui)
                        .text_size(w.z(w.size.frame))
                        .text_color(w.c(|t| t.ink_2))
                        .child(total_words(&t.all)),
                ),
        );
    div()
        .id("deaths")
        .size_full()
        .flex()
        .flex_col()
        .child(heads)
        .child(hairline(w))
        .child(super::chrome::scrolled(list, &gui.meter_scroll, w))
        .child(total)
}

/// One death (`.trow`): its time, who (their disc, their name and "you"
/// after it, or the enemy team's word), the killing blow with its source
/// and rez after it, the hit and the overkill.
#[allow(clippy::too_many_arguments)]
fn line(
    t: &Table,
    gui: &Gui,
    i: usize,
    d: &RaidDeath,
    narrow: bool,
    row_h: f32,
    w: &W,
    cx: &mut Context<Gui>,
) -> impl IntoElement {
    let on = t.selected == Some(i);
    let hovered = gui.row_hover == Some(super::RowHover::Death(i));
    let name = if t.hide_realms {
        display_name(&d.name).to_string()
    } else {
        d.name.clone()
    };
    let mine = is_mine(d, t.owner.as_deref());
    let mut who = div()
        .flex()
        .items_center()
        .gap(w.z(WHO_GAP))
        .child(class_icon(w, d.class, d.spec, w.z(DISC), false))
        .child(
            div()
                .flex_shrink(1.)
                .min_w_0()
                .font_family(w.ui)
                .font_weight(if on { MEDIUM } else { REGULAR })
                .text_size(w.z(w.size.name))
                .text_color(if on {
                    w.c(|t| t.name_lit)
                } else {
                    w.c(|t| t.ink)
                })
                .truncate()
                .child(name),
        );
    if mine {
        who = who.child(you_tag(d.class, w));
    }
    if d.enemy {
        who = who.child(w.text(ENEMY, w.size.small, w.c(|t| t.ink_3_text), REGULAR));
    }
    let wd = words(d, t.hide_realms);
    // The blow, then ONE quieter run after it, which gives way first: the
    // blow is what the column is about.
    let mut kb = div().flex().items_center().gap(w.z(KB_GAP)).child(
        div()
            .flex_shrink(1.)
            .min_w_0()
            .font_family(w.ui)
            .text_size(w.z(BLOW_PX))
            .text_color(if wd.note {
                w.c(|t| t.ink_3_text)
            } else {
                w.c(|t| t.ink)
            })
            .truncate()
            .child(wd.blow.clone()),
    );
    if let Some(after) = wd.after() {
        kb = kb.child(
            div()
                // Gives every letter, down to its "…", before the blow gives
                // one.
                .flex_shrink(1000.)
                .min_w_0()
                .font_family(w.ui)
                .text_size(w.z(w.size.small))
                .text_color(w.c(|t| t.ink_3_text))
                .truncate()
                .child(after),
        );
    }
    let mut row = vec![
        w.text(duration(d.at_ms), w.size.body, w.c(|t| t.ink_2), REGULAR),
        who,
        kb,
    ];
    if !narrow {
        let hit = if d.hit > 0 && !wd.cheat {
            commas(d.hit)
        } else {
            String::new()
        };
        row.push(w.text(hit, w.size.num, w.c(|t| t.ink), REGULAR));
        row.push(w.text(
            d.overkill.map(commas).unwrap_or_default(),
            w.size.small,
            w.c(|t| t.bad),
            REGULAR,
        ));
    }
    let pick = Pick::of(d);
    let mut el = div()
        .id(ElementId::from((
            ElementId::Name("death".into()),
            SharedString::from(format!("{}#{}", d.guid, d.index)),
        )))
        .test_support()
        .aria_selected(on)
        .h(w.z(row_h))
        .flex_none()
        .w_full()
        .flex()
        .items_center()
        .pl(w.z(LEAD))
        .pr(w.z(TRAIL))
        .rounded(w.r(3.))
        .cursor_pointer();
    el = if on && !t.keys_away {
        el.bg(w.c(|t| t.raise))
    } else if on || hovered {
        el.bg(w.c(|t| t.hover))
    } else {
        el
    };
    el.child(cells(narrow, w, row))
        .on_hover(cx.listener(move |this, over: &bool, _, cx| {
            let now = over.then_some(super::RowHover::Death(i));
            if this.row_hover != now && (*over || this.row_hover == Some(super::RowHover::Death(i)))
            {
                this.row_hover = now;
                cx.notify();
            }
        }))
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |this, _, window, cx| this.open_death(pick.clone(), window, cx)),
        )
}
