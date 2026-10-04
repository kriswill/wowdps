//! The live meter (the iced window's `meter_table` over `table.rs`): the
//! heading line, the rows and the pinned total, all on ONE column set
//! (`table::meter_set` on the prototype's own grid, `table::Grid::Meter`),
//! so a heading sits over its column by construction. Every numeric
//! heading sorts (desc → asc → the daemon's order); sorting and filtering
//! change what is DRAWN and never what a row's numbers mean — a row keeps
//! the daemon's index, so its rank, its share, its bar's scale and the
//! press it sends back all hold.
//!
//! A short list takes its own height and the total follows its last row;
//! a long one scrolls and the total pins under it. iced measured the
//! height (`responsive`); here the flex column does it: the list shrinks to
//! fit and scrolls, the total never shrinks.
//!
//! The delight: a bar eases to a new length over 280 ms, keyed by its
//! player, when the reader switches view or pull. While the pull is live
//! the bars step with its snapshots, as iced's do (`crate::ease`).

use gpui_kit::prelude::*;
use gpui_kit::{
    AnyElement, Context, Div, ElementId, MouseButton, SharedString, TestSupportExt as _, Window,
    div, linear_color_stop, linear_gradient, relative,
};
use wowdps_gui_logic::glyph::Glyph;
use wowdps_gui_logic::labels::{display_name, plural, realmless};
use wowdps_gui_logic::table::{
    Col, Grid, HEADS_INSET, METER_NARROW, TOTAL_INSET, enemy_split, meter_set, ordered, sort_of,
    total_cells,
};
use wowdps_gui_logic::theme::YOU_TAG_EDGE;
use wowdps_model::{Class, Role, Row, Screen, View};

use super::chrome::{class_icon, enemy_icon, hairline, tip};
use super::paint::glyph;
use super::w::{MEDIUM, REGULAR, SEMIBOLD, W};
use super::{Gui, Reveal};
use crate::ease::bar_frac;
use crate::meter::row_id;
use crate::theme::hsla;

/// The rank cell (`.trow .rk`, 26 px less its gap; 4 less at 820 px and
/// under) and the gap after it.
const RANK_W: f32 = 22.0;
const RANK_W_NARROW: f32 = RANK_W - 4.0;
const RANK_GAP: f32 = 6.0;
/// The rows' lead from the stage's edge, the icon's place inside the bar's
/// track, the heading line's own inset and the total's lead-in.
const ROW_LEAD: f32 = 12.0;
const ICON_X: f32 = 5.0;
const HEAD_PAD: f32 = 8.0;
const TOTAL_LEAD: f32 = 8.0 + wowdps_gui_logic::table::GAP;
/// The meter's class disc.
const DISC: f32 = 18.0;
/// The heading line (`.thead{padding:8px … 6px}`).
const HEADS_TOP: f32 = 8.0;
const HEADS_BOTTOM: f32 = 6.0;
/// The sort mark's air.
const ARROW_GAP: f32 = 2.0;
/// The bar under a row and the gap over it.
const BAR_H: f32 = 3.0;
/// What follows a name: the pair's A and B, a role glyph, the "you" tag.
const PIN_PX: f32 = 12.0;
const PIN_W: f32 = 9.0;
const ROLE_GLYPH: f32 = 13.0;
const YOU_TAG_W: f32 = 26.0;
const YOU_TAG_LINE: f32 = 15.0;
const TAG_GAP: f32 = 8.0;
/// The team divider's room (R13).
const DIVIDER_H: f32 = 20.0;

/// The meter's list, by the window's filter and sort.
pub struct Meter {
    /// Every row in the daemon's order: ranks, scale and total are the
    /// whole chart's.
    all: Vec<Row>,
    /// What is drawn, in the drawn order, each with its daemon index.
    drawn: Vec<(usize, Row)>,
    /// Each row's comparison slot, by daemon index.
    slots: Vec<Option<usize>>,
    selected: usize,
    view: View,
    sort: Option<(Col, bool)>,
    split: Option<usize>,
    enemies: bool,
    narrow: bool,
    owner: Option<(usize, Option<Class>)>,
    keys_away: bool,
    paused: bool,
    filtered: bool,
}

impl Meter {
    pub fn of(gui: &Gui, w: &W, cx: &gpui_kit::App) -> Self {
        let app = gui.fight(cx);
        let all = app.rows();
        let enemies = app.view == View::EnemyTaken;
        let sort = gui.meter_sort(app.view, w.narrow());
        let hide = gui.cfg.hide_realms;
        let drawn = ordered(all.clone(), &gui.filter_text, sort)
            .into_iter()
            .map(|(i, r)| {
                let mut shown = if hide {
                    Row {
                        label: if enemies {
                            realmless(&r.label)
                        } else {
                            display_name(&r.label).to_string()
                        },
                        ..r
                    }
                } else {
                    r
                };
                shown.enemy |= enemies;
                (i, shown)
            })
            .collect();
        Meter {
            slots: all.iter().map(|r| app.compare_slot(&r.key)).collect(),
            split: sort.is_none().then(|| enemy_split(&all)).flatten(),
            owner: gui
                .owner_in(&all, app.view)
                .map(|i| (i, all.get(i).and_then(|r| r.class))),
            all,
            drawn,
            selected: app.row_sel,
            view: app.view,
            sort,
            enemies,
            narrow: w.narrow(),
            filtered: !gui.filter_text.is_empty(),
            keys_away: app.inspecting(),
            paused: app.screen == Screen::Compare && app.is_live(),
        }
    }
}

/// The table: heads, hairline, rows, total.
pub fn view(
    m: Meter,
    gui: &Gui,
    w: &W,
    window: &mut Window,
    cx: &mut Context<Gui>,
) -> gpui_kit::Stateful<Div> {
    let cols = meter_set(m.view, m.narrow);
    let grid = Grid::Meter {
        view: m.view,
        narrow: m.narrow,
    };
    let rank_cell = if cols == METER_NARROW {
        RANK_W_NARROW
    } else {
        RANK_W
    };
    let rank_w = if gui.cfg.show_ranks {
        rank_cell + RANK_GAP
    } else {
        0.0
    };
    // Where the icon column starts: the heading over the names and the
    // total's label start there too.
    let who = ROW_LEAD + rank_w + ICON_X;
    let row_h = w.pitch.row_of(gui.cfg.density());
    let max = m.all.iter().map(|r| r.amount).max().unwrap_or(1);

    let mut list = div()
        .id("meter-list")
        .test_support()
        .flex()
        .flex_col()
        .flex_shrink(1.)
        .min_h_0()
        .overflow_y_scroll()
        .track_scroll(&gui.meter_scroll)
        .pr(w.z(w.pitch.scroll_lane));
    if m.drawn.is_empty() {
        list = list.child(div().pl(w.z(who)).py(w.z(4.)).child(w.text(
            "nothing to show for this view yet",
            w.size.body,
            w.c(|t| t.ink_2),
            REGULAR,
        )));
    }
    let mut divided = false;
    // A row to bring into sight: its place among the list's children.
    let reveal = gui.reveal.take();
    let mut child = 0;
    for (i, r) in &m.drawn {
        let i = *i;
        if !divided && m.split.is_some_and(|s| i >= s) {
            divided = true;
            list = list.child(team_divider(w));
            child += 1;
        }
        if matches!(&reveal, Some(Reveal::Row(key)) if *key == r.key) {
            gui.meter_scroll.scroll_to_item(child);
        }
        list = list.child(row_line(
            &m, gui, i, r, max, rank_cell, row_h, cols, grid, w, window, cx,
        ));
        child += 1;
    }
    // A row the answer in hand does not hold yet waits for the one that
    // will; once rows are drawn, it was found or never will be.
    if m.drawn.is_empty() && matches!(reveal, Some(Reveal::Row(_))) {
        gui.reveal.set(reveal);
    }
    div()
        .id("meter")
        .size_full()
        .flex()
        .flex_col()
        .child(heads(&m, cols, grid, who, w, cx))
        .child(hairline(w))
        .child(super::chrome::scrolled(list, &gui.meter_scroll, w))
        .child(total(&m, cols, grid, who, w))
}

/// The heading line: "Player" (or "Enemy") over the names, then a head per
/// column — each a press that sorts it, gold under the pointer and while
/// sorted, its arrow after the label.
fn heads(
    m: &Meter,
    cols: &[Col],
    grid: Grid,
    who: f32,
    w: &W,
    cx: &mut Context<Gui>,
) -> impl IntoElement {
    let gap = grid.gap();
    let mut lead = div()
        .flex_1()
        .min_w_0()
        .flex()
        .items_center()
        .gap(w.z(10.))
        .pl(w.z((who - HEAD_PAD).max(0.0)))
        .child(w.text(
            if m.enemies { "Enemy" } else { "Player" },
            w.size.label,
            w.c(|t| t.label_ink),
            REGULAR,
        ));
    if m.paused {
        lead = lead.child(w.text(
            "paused while comparing",
            w.size.label,
            w.c(|t| t.ink_3_text),
            REGULAR,
        ));
    }
    let mut line = div().flex_none().flex().w(w.z(grid.span(cols, 1.0) + gap));
    for &c in cols {
        let head = c.head(m.view);
        let sorted = sort_of(c, m.sort);
        let width = grid.width(c) + if sorted.is_some() { gap } else { 0.0 };
        if sorted.is_none() {
            line = line.child(div().w(w.z(gap)).flex_none());
        }
        let mut words = div()
            .w(w.z(width))
            .flex_none()
            .flex()
            .items_center()
            .justify_end()
            .gap(w.z(ARROW_GAP))
            .child(w.words(head, grid.head_px(w.size.label), REGULAR));
        if let Some(desc) = sorted {
            let arrow = if desc {
                Glyph::ArrowDown
            } else {
                Glyph::ArrowUp
            };
            words = words.child(glyph(
                arrow,
                w.z(grid.head_px(w.size.label)),
                w.c(|t| t.accent),
            ));
        }
        line = line.child(if head.is_empty() {
            words.text_color(w.c(|t| t.label_ink)).into_any_element()
        } else {
            div()
                .id(ElementId::from((
                    ElementId::Name("sort".into()),
                    SharedString::from(head),
                )))
                .test_support()
                .cursor_pointer()
                .text_color(if sorted.is_some() {
                    w.c(|t| t.accent)
                } else {
                    w.c(|t| t.label_ink)
                })
                .hover(|s| s.text_color(w.c(|t| t.accent)))
                .child(words)
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, _, _, cx| this.sort_by(c, cx)),
                )
                .into_any_element()
        });
    }
    div()
        .id("meter-heads")
        .flex_none()
        .w_full()
        .flex()
        .items_center()
        .pt(w.z(HEADS_TOP))
        .pb(w.z(HEADS_BOTTOM))
        .pl(w.z(HEADS_INSET))
        .pr(w.z(HEADS_INSET + w.pitch.scroll_lane))
        .child(lead)
        .child(line)
}

/// One row (`.trow`): the rank outside the bar, the class disc (a press
/// picks the player for a comparison), the name and what follows it, the
/// figures in their columns, all over a narrow bar of the class colour.
#[allow(clippy::too_many_arguments)]
fn row_line(
    m: &Meter,
    gui: &Gui,
    i: usize,
    r: &Row,
    max: u64,
    rank_cell: f32,
    row_h: f32,
    cols: &'static [Col],
    grid: Grid,
    w: &W,
    window: &mut Window,
    cx: &mut Context<Gui>,
) -> AnyElement {
    let selected = i == m.selected;
    let hovered = gui.row_hover == Some(super::RowHover::Meter(i));
    let mine = m.owner.filter(|(o, _)| *o == i).map(|(_, class)| class);
    let slot = m.slots.get(i).copied().flatten();
    let icon = if m.enemies {
        enemy_icon(w, w.z(DISC))
    } else {
        div()
            .id(ElementId::from((
                ElementId::Name("pick".into()),
                SharedString::from(r.key.clone()),
            )))
            .test_support()
            .child(class_icon(w, r.class, r.spec, w.z(DISC), false))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, _, _, cx| {
                    this.pick_compare(i, cx);
                    cx.stop_propagation();
                }),
            )
            .into_any_element()
    };
    // The selection is a lit bar and a bright name, never a frame.
    let name = div()
        .flex_shrink(1.)
        .min_w_0()
        .font_family(w.ui.clone())
        .font_weight(if selected { MEDIUM } else { REGULAR })
        .text_size(w.z(w.size.name))
        .text_color(if selected {
            w.c(|t| t.name_lit)
        } else {
            w.c(|t| t.ink)
        })
        .truncate()
        .child(r.label.clone());
    let labels = div()
        .flex_1()
        .min_w_0()
        .h_full()
        .flex()
        .items_center()
        .gap(w.z(10.))
        .pl(w.z(ICON_X))
        .pr(w.z(8.))
        .child(icon)
        .child(name)
        .children(name_tags(r.spec.map(|s| s.role()), mine, slot, w));
    let cells = div()
        .flex_none()
        .flex()
        .items_center()
        .gap(w.z(grid.gap()))
        .children(cols.iter().map(|&c| {
            let ink = match c.rank() {
                0 | 1 => w.c(|t| t.ink),
                _ => w.c(|t| t.ink_2),
            };
            w.text(
                c.cell(r),
                w.size.num,
                ink,
                if c.rank() == 0 { MEDIUM } else { REGULAR },
            )
            .w(w.z(grid.width(c)))
            .flex_none()
            .text_right()
            .overflow_hidden()
        }));
    let content = div()
        .flex_1()
        .min_h_0()
        .w_full()
        .flex()
        .items_center()
        .gap(w.z(grid.gap()))
        .pr(w.z(8.))
        .child(labels)
        .child(cells);
    let live = gui.fight(cx).is_live();
    let frac = bar_frac(
        ElementId::from((
            ElementId::Name("meter-bar".into()),
            SharedString::from(r.key.clone()),
        )),
        fill(r.amount, max),
        live,
        window,
        cx,
    );
    let color = bar_color(w, r);
    let (tail, head) = if selected {
        (w.bars.lit_from, w.bars.lit_to)
    } else {
        (w.bars.rest_from, w.bars.rest_to)
    };
    let bar = div()
        .w_full()
        .h(w.z(BAR_H))
        .flex_none()
        .rounded(w.r(2.))
        .bg(w.c(|t| t.track))
        .child(
            div()
                .h_full()
                .w(relative(frac))
                .rounded(w.r(2.))
                .when(frac > 0.0, |d| {
                    d.bg(linear_gradient(
                        90.,
                        linear_color_stop(color.opacity(tail), 0.),
                        linear_color_stop(color.opacity(head), 1.),
                    ))
                }),
        );
    let under = div()
        .flex_1()
        .min_w_0()
        .h_full()
        .flex()
        .flex_col()
        .gap(w.z(1.))
        .overflow_hidden()
        .rounded(w.r(3.))
        .when(selected, |d| d.bg(w.c(|t| t.raise)))
        .child(content)
        .child(bar);
    let mut line = div()
        .id(row_id(&r.key))
        .test_support()
        .aria_selected(selected)
        .aria_label(r.label.clone())
        .h(w.z(row_h))
        .flex_none()
        .w_full()
        .flex()
        .items_center()
        .gap(w.z(RANK_GAP))
        .pl(w.z(ROW_LEAD))
        .rounded(w.r(3.))
        .cursor_pointer();
    // The selection raises the whole line; while the keys are in the
    // inspector it steps back to the hover's weight.
    line = if selected && !m.keys_away {
        line.bg(w.c(|t| t.raise))
    } else if selected || hovered {
        line.bg(w.c(|t| t.hover))
    } else {
        line
    };
    if gui.cfg.show_ranks {
        line = line.child(
            w.text(
                (i + 1).to_string(),
                w.size.small,
                match mine {
                    Some(class) => w.you_text(class),
                    None => w.c(|t| t.ink_3_text),
                },
                if mine.is_some() { SEMIBOLD } else { REGULAR },
            )
            .w(w.z(rank_cell))
            .flex_none()
            .text_right(),
        );
    }
    line.child(under)
        .on_hover(cx.listener(move |this, over: &bool, _, cx| {
            let now = over.then_some(super::RowHover::Meter(i));
            if this.row_hover != now && (*over || this.row_hover == Some(super::RowHover::Meter(i)))
            {
                this.row_hover = now;
                cx.notify();
            }
        }))
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |this, _, window, cx| this.press_row(i, window, cx)),
        )
        .into_any_element()
}

/// What follows a name: the pair's A or B, a tank's shield or a healer's
/// cross in the faint ink, and the owner's "you" tag.
fn name_tags(
    role: Option<Role>,
    you: Option<Option<Class>>,
    slot: Option<usize>,
    w: &W,
) -> Option<Div> {
    let glyph_of = match role {
        Some(Role::Tank) => Some(Glyph::Shield),
        Some(Role::Healer) => Some(Glyph::Cross),
        Some(Role::Dps) | None => None,
    };
    let half = match slot {
        Some(0) => Some(("A", "Pinned for comparison (v to stop)")),
        Some(1) => Some(("B", "Compared with the pinned player")),
        _ => None,
    };
    if glyph_of.is_none() && you.is_none() && half.is_none() {
        return None;
    }
    let mut tags = div().flex_none().flex().items_center().gap(w.z(TAG_GAP));
    if let Some((letter, words)) = half {
        tags = tags.child(tip(
            div()
                .id(ElementId::Name(letter.into()))
                .w(w.z(PIN_W))
                .child(w.text(letter, PIN_PX, w.c(|t| t.label_ink), SEMIBOLD)),
            words,
        ));
    }
    if let Some(g) = glyph_of {
        tags = tags.child(glyph(g, w.z(ROLE_GLYPH), w.c(|t| t.ink_3)));
    }
    if let Some(class) = you {
        tags = tags.child(you_tag(class, w));
    }
    Some(tags)
}

/// The owner's tag (`.youtag`): "you" at 11.5 px, 600, in the owner's
/// text colour, on a 1 px frame of their class colour.
pub fn you_tag(class: Option<Class>, w: &W) -> Div {
    let edge = hsla(w.class_rgb(class)).opacity(YOU_TAG_EDGE);
    div()
        .w(w.z(YOU_TAG_W))
        .flex_none()
        .flex()
        .justify_center()
        .rounded(w.r(4.))
        .border(w.z(1.))
        .border_color(edge)
        .child(
            w.text(
                "you",
                w.size.you_tag,
                match class {
                    Some(_) => w.you_text(class),
                    None => w.c(|t| t.ink_2),
                },
                SEMIBOLD,
            )
            .line_height(w.z(YOU_TAG_LINE)),
        )
}

/// R13: the line between the teams in a PvP chart.
fn team_divider(w: &W) -> Div {
    let bad = w.c(|t| t.bad);
    let line = || div().flex_1().h(w.z(1.)).bg(bad.opacity(0.4));
    div()
        .h(w.z(DIVIDER_H))
        .flex_none()
        .flex()
        .items_center()
        .gap(w.z(8.))
        .px(w.z(ROW_LEAD))
        .child(line())
        .child(w.text("enemy team", w.size.micro, bad, REGULAR))
        .child(line())
}

/// The pinned total: the fold of OUR rows (an arena's other team left
/// out), or under a filter of the drawn ones, in the same columns.
fn total(m: &Meter, cols: &[Col], grid: Grid, who: f32, w: &W) -> impl IntoElement {
    let ours: Vec<Row> = if m.filtered {
        m.drawn
            .iter()
            .filter_map(|(i, _)| m.all.get(*i))
            .filter(|r| !r.enemy)
            .cloned()
            .collect()
    } else {
        m.all.iter().filter(|r| !r.enemy).cloned().collect()
    };
    let label = match (m.enemies, ours.len()) {
        (true, 1) => "Total, 1 enemy".to_string(),
        (true, n) => format!("Total, {n} enemies"),
        (false, n) => format!("Total, {}", plural(n, "player")),
    };
    let words = total_cells(cols, &ours, m.filtered);
    let lead_pad = (who - TOTAL_LEAD).max(0.0);
    div()
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
                .gap(w.z(grid.gap()))
                .pl(w.z(TOTAL_INSET))
                .pr(w.z(TOTAL_INSET + w.pitch.scroll_lane))
                .bg(w.c(|t| t.surface))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .pl(w.z(lead_pad + wowdps_gui_logic::table::GAP))
                        .child(
                            div()
                                .truncate()
                                .font_family(w.ui.clone())
                                .text_size(w.z(w.size.frame))
                                .text_color(w.c(|t| t.ink_2))
                                .child(label),
                        ),
                )
                .child(div().flex_none().flex().gap(w.z(grid.gap())).children(
                    cols.iter().zip(words).map(|(&c, s)| {
                        w.text(s, w.size.num, w.c(|t| t.ink), REGULAR)
                            .w(w.z(grid.width(c)))
                            .flex_none()
                            .text_right()
                    }),
                )),
        )
}

/// A bar's length: its share of the chart's longest, in whole percent as
/// the iced meter rounds it.
pub fn fill(amount: u64, max: u64) -> f32 {
    let pct = (amount as f64 / max.max(1) as f64 * 100.0)
        .round()
        .clamp(0.0, 100.0);
    pct as f32 / 100.0
}

/// A bar's colour: the hostile red for a classless enemy, the class's own,
/// else the classless grey.
fn bar_color(w: &W, r: &Row) -> gpui_kit::Hsla {
    if r.enemy && r.class.is_none() {
        return w.c(|t| t.hostile);
    }
    hsla(w.class_rgb(r.class))
}
