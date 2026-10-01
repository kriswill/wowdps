//! The inspector drawn (the iced window's `Insp::view`, the prototype's
//! `.insp`): one scrolling column on the panel's surface — the head (disc,
//! name, the line under it), the numbers, the actions; an opened ability's
//! strip; R17's mitigation line; the graph; R9's death chips; the tabs; and
//! the list, the recap or the pair under them, a note last. A body held
//! from the last player while this one's is on its way wears a veil.

use gpui_kit::prelude::*;
use gpui_kit::{
    AnyElement, Context, Div, ElementId, MouseButton, SharedString, TestSupportExt as _, Window,
    div, img,
};
use wowdps_gui_logic::drill::school_name;
use wowdps_gui_logic::inspect::nums::{Num, ability_nums};
use wowdps_gui_logic::theme as gl;
use wowdps_model::Pane;

use super::model::{Ability, Act, Body, Disc, Graph, Head, Insp, Press, Tail};
use super::{list, recap};
use crate::theme::hsla;
use crate::window::Gui;
use crate::window::chrome::{class_icon, hairline, icon_button, tip};
use crate::window::paint::glyph;
use crate::window::w::{Fit, MEDIUM, REGULAR, SEMIBOLD, W};

/// The head (`.ihead{padding:14px 16px 12px;gap:9px}`): the disc, the name
/// (`.iname b{21px 600}`), the line under it (`small{14px}`).
const HEAD_PAD: (f32, f32, f32, f32) = (14.0, 16.0, 12.0, 16.0);
const HEAD_GAP: f32 = 9.0;
const DISC: f32 = 34.0;
const NAME_GAP: f32 = 10.0;
const NAME_PX: f32 = 21.0;
const SUB_PX: f32 = 14.0;
/// The numbers (`.inums{gap:8px}`, `.inum .l{13px}`, `.v{17px 500}`,
/// `.v small{13px}`).
const NUMS_GAP: f32 = 8.0;
const NUM_LABEL_PX: f32 = 13.0;
const NUM_PX: f32 = 17.0;
const NUM_SMALL_PX: f32 = 13.0;
const NUM_TAIL_GAP: f32 = 4.0;
/// The actions (`.iacts{gap:6px}`, `.btn{height:26px;padding-inline:9px;
/// border-radius:5px;font-size:13.5px;gap:6px}`).
const ACTS_GAP: f32 = 6.0;
const BTN_H: f32 = 26.0;
const BTN_PAD_X: f32 = 9.0;
const BTN_RADIUS: f32 = 5.0;
const BTN_PX: f32 = 13.5;
const BTN_ICON: f32 = 14.0;
/// A section (`.igraph{padding:10px 16px}`), and the graph's top line.
const SECTION_PAD: (f32, f32) = (10.0, 16.0);
const TOP_GAP: f32 = 4.0;
const TOP_PX: f32 = 13.0;
const LEGEND_GAP: f32 = 12.0;
const SWATCH_GAP: f32 = 5.0;
/// The ability strip's crumb.
const CRUMB_PX: f32 = 13.0;
const CRUMB_MARK_PX: f32 = 11.0;
const CRUMB_ICON: f32 = 14.0;
const CRUMB_NAME_PX: f32 = 14.0;
const CRUMB_GAP: f32 = 6.0;
const STRIP_GAP: f32 = 8.0;
/// The mitigation line (`.mit{gap:6px 14px;font-size:13.5px}`).
const MIT_GAP: f32 = 14.0;
const MIT_ROW_GAP: f32 = 6.0;
const MIT_PX: f32 = 13.5;
const MIT_WORD_GAP: f32 = 4.0;
/// The tabs (`.itabs{gap:2px;padding:0 10px}`, `.itab{height:34px;
/// padding-inline:8px;font-weight:500;border-bottom:2px}`).
const TAB_H: f32 = 34.0;
const TAB_PAD_X: f32 = 8.0;
const TAB_LINE: f32 = 2.0;
const TAB_GAP: f32 = 2.0;
const TABS_PAD_X: f32 = 10.0;
const TAB_PX: f32 = 14.0;
/// How much of the surface veils a body held from the last player.
const STALE_VEIL: f32 = 0.55;

/// A press's listener on the window.
pub fn on(
    press: Press,
    cx: &Context<Gui>,
) -> impl Fn(&gpui_kit::MouseDownEvent, &mut Window, &mut gpui_kit::App) + 'static {
    cx.listener(move |this, _, window, cx| this.insp_press(press.clone(), window, cx))
}

/// The inspector `width` wide (`None`: the whole stage, pushed).
pub fn view(
    insp: &Insp,
    gui: &Gui,
    w: &W,
    pushed: bool,
    window: &mut Window,
    cx: &mut Context<Gui>,
) -> AnyElement {
    let fit = w.fit();
    let narrow_list = gui.inspector_width(w) <= wowdps_gui_logic::inspect::NARROW_LIST;
    let mut col = div().w_full().flex().flex_col();
    if !insp.head.name.is_empty() {
        col = col
            .child(head_block(insp, w, fit, pushed, cx))
            .child(hairline(w));
    } else if pushed {
        col = col.child(
            div()
                .pt(w.z(HEAD_PAD.0))
                .px(w.z(HEAD_PAD.1))
                .pb(w.z(HEAD_PAD.2))
                .child(back_button(w, cx)),
        );
    }
    if let Some(a) = &insp.ability {
        col = col.child(section(ability_strip(a, w, fit, cx), w));
    }
    if !insp.mit.is_empty() {
        col = col.child(veiled(section(mit_line(&insp.mit, w), w), insp.stale, w));
    }
    if let Some(g) = &insp.graph {
        col = col.child(veiled(
            section(graph_block(g, gui, w, cx), w),
            insp.stale,
            w,
        ));
    }
    if let Some(d) = &insp.deaths {
        col = col.children(super::chips(d, w, cx));
    }
    if let Some((words, up)) = insp.tabs {
        col = col.child(tabs(words, up, insp.stacks.as_ref().map(|s| s.on), w, cx));
    }
    let keep = list::Keep {
        scroll: gui.insp.scroll.clone(),
        pending: gui.insp.reveal.clone(),
    };
    let body: AnyElement = match (&insp.stacks, &insp.body) {
        (Some(s), _) if s.on => super::matrix(s, w, cx),
        (_, Body::Nothing) => div().into_any_element(),
        (_, Body::One(l)) => list::view(l, narrow_list, &keep, w, cx).into_any_element(),
        (_, Body::Recap(r)) => recap::view(r, fit, w, window, cx).into_any_element(),
        (_, Body::Pair(pair)) => {
            let (a, b) = pair.as_ref();
            if fit == Fit::Wide {
                div()
                    .flex()
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .child(list::view(a, false, &keep, w, cx)),
                    )
                    .child(crate::window::chrome::vrule(w))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .child(list::view(b, false, &keep, w, cx)),
                    )
                    .into_any_element()
            } else {
                div()
                    .flex()
                    .flex_col()
                    .child(list::view(a, narrow_list, &keep, w, cx))
                    .child(hairline(w))
                    .child(list::view(b, narrow_list, &keep, w, cx))
                    .into_any_element()
            }
        }
    };
    col = col.child(veiled(div().child(body), insp.stale, w));
    if let Some(note) = &insp.note {
        col = col.child(div().py(w.z(10.)).px(w.z(16.)).child(w.text(
            note.clone(),
            w.size.small,
            w.c(|t| t.ink_2),
            REGULAR,
        )));
    }
    // The scrollbar keeps its own lane, as the meter's does: the last
    // column is never under it.
    div()
        .relative()
        .size_full()
        .child(
            div()
                .id("inspector-scroll")
                .test_support()
                .size_full()
                .overflow_y_scroll()
                .track_scroll(&gui.insp.scroll)
                .child(col.pr(w.z(w.pitch.scroll_lane))),
        )
        .child(crate::scrollbar::bar(w.scrollbar(), &gui.insp.scroll))
        .into_any_element()
}

/// A section: its content padded, a hairline under it.
fn section(content: impl IntoElement, w: &W) -> Div {
    div()
        .flex()
        .flex_col()
        .child(
            div()
                .py(w.z(SECTION_PAD.0))
                .px(w.z(SECTION_PAD.1))
                .child(content),
        )
        .child(hairline(w))
}

/// `el`, under a veil of the surface when it is the last player's.
fn veiled(el: Div, stale: bool, w: &W) -> Div {
    if !stale {
        return el;
    }
    el.relative().child(
        div()
            .absolute()
            .inset_0()
            .bg(w.c(|t| t.surface.alpha(STALE_VEIL))),
    )
}

/// A pushed inspector's way back.
fn back_button(w: &W, cx: &Context<Gui>) -> impl IntoElement {
    tip(
        icon_button(
            w,
            "inspector-back",
            wowdps_gui_logic::glyph::Glyph::ChevronLeft,
            true,
        )
        .on_mouse_down(MouseButton::Left, on(Press::Uninspect, cx)),
        "Back to the meter (Esc)",
    )
}

fn head_block(insp: &Insp, w: &W, fit: Fit, pushed: bool, cx: &Context<Gui>) -> AnyElement {
    let h = &insp.head;
    let mut top = div().flex().items_center().gap(w.z(NAME_GAP));
    if pushed {
        top = top.child(back_button(w, cx));
    }
    for d in &h.discs {
        top = top.child(match *d {
            Disc::Player(class, spec) => class_icon(w, class, spec, w.z(DISC), false),
            Disc::Enemy => list::foe_disc(w.z(DISC)),
        });
    }
    top = top.child(
        div()
            .flex_1()
            .min_w_0()
            .overflow_hidden()
            .flex()
            .flex_col()
            .child(name_line(h, w))
            .when(!h.sub.is_empty(), |d| {
                d.child(
                    div()
                        .font_family(w.ui)
                        .text_size(w.z(SUB_PX))
                        .text_color(w.c(|t| t.ink_2))
                        .child(h.sub.clone()),
                )
            }),
    );
    let mut block = div()
        .id("inspector-head")
        .test_support()
        .flex()
        .flex_col()
        .gap(w.z(HEAD_GAP))
        .pt(w.z(HEAD_PAD.0))
        .px(w.z(HEAD_PAD.1))
        .pb(w.z(HEAD_PAD.2))
        .child(top);
    if !insp.nums.is_empty() {
        block = block.child(nums_grid(&insp.nums, per_row(fit), w));
    }
    if !insp.acts.is_empty() {
        block = block.child(
            div()
                .flex()
                .flex_wrap()
                .gap(w.z(ACTS_GAP))
                .children(insp.acts.iter().map(|a| act_button(a, w, cx))),
        );
    }
    block.into_any_element()
}

/// The name, piece by piece; every piece but the last gives way first.
fn name_line(h: &Head, w: &W) -> Div {
    div()
        .flex()
        .items_center()
        .overflow_hidden()
        .whitespace_nowrap()
        .line_height(gpui_kit::relative(1.1))
        .children(h.name.iter().enumerate().map(|(i, p)| {
            let last = i + 1 == h.name.len();
            div()
                .font_family(w.ui)
                .font_weight(if p.semibold { SEMIBOLD } else { REGULAR })
                .text_size(w.z(NAME_PX))
                .text_color(hsla(p.ink))
                .when(!last, |d| {
                    d.flex_shrink_1()
                        .min_w_0()
                        .overflow_hidden()
                        .text_ellipsis()
                })
                .when(last, |d| d.flex_shrink_0())
                .child(p.words.clone())
        }))
}

/// Four numbers across in a wide inspector, two otherwise.
fn per_row(fit: Fit) -> usize {
    if fit == Fit::Wide { 4 } else { 2 }
}

/// The numbers (`.inums`): rows of `per` cells sharing the width, a short
/// last row padded so its cells keep the grid's.
fn nums_grid(nums: &[Num], per: usize, w: &W) -> Div {
    let mut grid = div().flex().flex_col().gap(w.z(NUMS_GAP));
    for chunk in nums.chunks(per.max(1)) {
        let mut line = div().flex().gap(w.z(NUMS_GAP));
        for n in chunk {
            line = line.child(num_cell(n, w));
        }
        for _ in chunk.len()..per {
            line = line.child(div().flex_1());
        }
        grid = grid.child(line);
    }
    grid
}

fn num_cell(n: &Num, w: &W) -> Div {
    let mut value = div().flex().items_baseline();
    if !n.value.is_empty() {
        value = value.child(w.text(n.value.clone(), NUM_PX, w.c(|t| t.ink), MEDIUM));
    }
    if !n.small.is_empty() {
        value = value.child(
            w.text(n.small.clone(), NUM_SMALL_PX, w.c(|t| t.ink_2), REGULAR)
                .when(!n.value.is_empty(), |d| d.ml(w.z(NUM_TAIL_GAP))),
        );
    }
    div()
        .flex_1()
        .min_w_0()
        .overflow_hidden()
        .flex()
        .flex_col()
        .gap(w.z(1.))
        .child(w.text(n.label.clone(), NUM_LABEL_PX, w.c(|t| t.gold_dim), REGULAR))
        .child(value)
}

/// One action (`.btn`): pressed in the accent, inert faint, else an edge
/// that brightens under the pointer.
fn act_button(a: &Act, w: &W, cx: &Context<Gui>) -> AnyElement {
    let id = ElementId::from((
        ElementId::Name("act".into()),
        SharedString::from(a.words.clone()),
    ));
    let (fill, edge, ink, icon) = if a.pressed {
        (
            Some(hsla(w.accent.base)),
            hsla(w.accent.base),
            hsla(w.accent.ink),
            hsla(w.accent.ink),
        )
    } else if a.press.is_none() {
        (None, w.c(|t| t.line), w.c(|t| t.ink_3), w.c(|t| t.ink_3))
    } else {
        (None, w.c(|t| t.edge), w.c(|t| t.ink_2), w.c(|t| t.ink_2))
    };
    let mut btn = div()
        .id(id)
        .test_support()
        .aria_selected(a.pressed)
        .h(w.z(BTN_H))
        // iced draws a button's border inside its padding; GPUI's is outside.
        .px(w.z(BTN_PAD_X - 1.))
        .flex()
        .items_center()
        .gap(w.z(ACTS_GAP))
        .rounded(w.z(BTN_RADIUS))
        .border_1()
        .border_color(edge)
        .text_color(ink)
        .when_some(fill, |d, f| d.bg(f))
        .child(glyph(a.glyph, w.z(BTN_ICON), icon))
        .child(w.words(a.words.clone(), BTN_PX, REGULAR));
    if let Some(press) = a.press.clone() {
        btn = btn
            .cursor_pointer()
            .when(!a.pressed, |d| {
                d.hover(|s| s.border_color(w.c(|t| t.ink_3)).text_color(w.c(|t| t.ink)))
            })
            .on_mouse_down(MouseButton::Left, on(press, cx));
    }
    tip(btn, a.tip).into_any_element()
}

/// The graph section: its top line (the measure, and the zoom, the rate's
/// bucket or a pair's legend), then the plot.
fn graph_block(g: &Graph, gui: &Gui, w: &W, cx: &mut Context<Gui>) -> Div {
    let faint = w.c(|t| t.ink_3_text);
    let tail: AnyElement = match &g.tail {
        Tail::Words(words) => w
            .text(words.clone(), TOP_PX, faint, REGULAR)
            .into_any_element(),
        Tail::Legend(entries) => div()
            .flex()
            .items_center()
            .gap(w.z(LEGEND_GAP))
            .children(entries.iter().map(|(name, color, dashed)| {
                div()
                    .flex()
                    .items_center()
                    .gap(w.z(SWATCH_GAP))
                    .child(swatch(*color, *dashed, w))
                    .child(w.text(name.clone(), TOP_PX, w.c(|t| t.ink_2), REGULAR))
            }))
            .into_any_element(),
    };
    div()
        .flex()
        .flex_col()
        .gap(w.z(TOP_GAP))
        .child(
            div()
                .flex()
                .items_center()
                .child(w.text(g.lead.clone(), TOP_PX, faint, REGULAR))
                .child(div().flex_1())
                .child(tail),
        )
        .child(super::plot(g, gui, w, cx))
}

/// A legend's swatch: a 10 × 3 bar, or a dashed one as two bits.
fn swatch(color: gl::Color, dashed: bool, w: &W) -> Div {
    let bit = |width: f32| {
        div()
            .w(w.z(width))
            .h(w.z(3.))
            .rounded(w.z(2.))
            .bg(hsla(color))
    };
    if dashed {
        div().flex().gap(w.z(2.)).child(bit(4.)).child(bit(4.))
    } else {
        div().child(bit(10.))
    }
}

/// An opened ability's strip: back, whose, "▸", its icon, its name, its
/// school; then its numbers.
fn ability_strip(a: &Ability, w: &W, fit: Fit, cx: &Context<Gui>) -> Div {
    let mut crumb = div()
        .flex()
        .items_center()
        .gap(w.z(CRUMB_GAP))
        .child(tip(
            icon_button(
                w,
                "ability-back",
                wowdps_gui_logic::glyph::Glyph::ChevronLeft,
                true,
            )
            .on_mouse_down(MouseButton::Left, on(Press::CloseAbility, cx)),
            "Back to the list (Esc)",
        ))
        .child(w.text(
            a.who.clone(),
            CRUMB_PX,
            a.who_ink.map_or(w.c(|t| t.ink_2), hsla),
            REGULAR,
        ))
        .child(w.text("▸", CRUMB_MARK_PX, w.c(|t| t.ink_3_text), REGULAR));
    if let Some(tile) = a
        .row
        .as_ref()
        .and_then(|r| crate::images::spell_icon(r.spell_id))
    {
        crumb = crumb.child(img(tile).size(w.z(CRUMB_ICON)).flex_none());
    }
    crumb = crumb.child(
        div()
            .flex_1()
            .min_w_0()
            .overflow_hidden()
            .text_ellipsis()
            .child(w.text(a.label.clone(), CRUMB_NAME_PX, w.c(|t| t.ink), MEDIUM)),
    );
    if let Some(school) = a.row.as_ref().and_then(|r| school_name(r.school)) {
        crumb = crumb.child(w.text(school, CRUMB_PX, w.c(|t| t.ink_2), REGULAR));
    }
    let mut strip = div().flex().flex_col().gap(w.z(STRIP_GAP)).child(crumb);
    if let Some(r) = &a.row {
        strip = strip.child(nums_grid(
            &ability_nums(r, a.view, a.tally),
            per_row(fit),
            w,
        ));
    }
    strip
}

/// R17's line: each piece's lead, its figure set apart, and its tail
/// straight after; wrapping.
fn mit_line(pieces: &[(String, String, String)], w: &W) -> Div {
    div()
        .flex()
        .flex_wrap()
        .gap_x(w.z(MIT_GAP))
        .gap_y(w.z(MIT_ROW_GAP))
        .children(pieces.iter().map(|(lead, figure, tail)| {
            div()
                .flex()
                .items_baseline()
                .gap(w.z(MIT_WORD_GAP))
                .child(w.text(lead.clone(), MIT_PX, w.c(|t| t.ink_2), REGULAR))
                .child(
                    div()
                        .flex()
                        .items_baseline()
                        .child(w.text(figure.clone(), MIT_PX, w.c(|t| t.ink), MEDIUM))
                        .when(!tail.is_empty(), |d| {
                            d.child(w.text(tail.clone(), MIT_PX, w.c(|t| t.ink_2), REGULAR))
                        }),
                )
        }))
}

/// The tabs (`.itabs`): the two list tabs, and R21's Stacks with a ledger.
fn tabs(words: [&'static str; 2], up: Pane, stacks: Option<bool>, w: &W, cx: &Context<Gui>) -> Div {
    let stacks_on = stacks == Some(true);
    let tab = |id: &'static str, label: &'static str, active: bool, press: Press| {
        div()
            .id(id)
            .test_support()
            .aria_selected(active)
            .h(w.z(TAB_H))
            .flex_none()
            .flex()
            .flex_col()
            .cursor_pointer()
            .text_color(if active {
                w.c(|t| t.ink)
            } else {
                w.c(|t| t.ink_2)
            })
            .hover(|s| s.text_color(w.c(|t| t.ink)))
            .child(
                div()
                    .flex_1()
                    .flex()
                    .items_center()
                    .px(w.z(TAB_PAD_X))
                    .child(w.words(label, TAB_PX, MEDIUM)),
            )
            .child(
                div()
                    .h(w.z(TAB_LINE))
                    .w_full()
                    .when(active, |d| d.bg(w.accent())),
            )
            .on_mouse_down(MouseButton::Left, on(press, cx))
    };
    let mut strip = div()
        .flex()
        .gap(w.z(TAB_GAP))
        .px(w.z(TABS_PAD_X))
        .child(tab(
            "tab-spell",
            words[0],
            !stacks_on && up == Pane::Spell,
            Press::Tab(Pane::Spell),
        ))
        .child(tab(
            "tab-target",
            words[1],
            !stacks_on && up == Pane::Target,
            Press::Tab(Pane::Target),
        ));
    if stacks.is_some() {
        strip = strip.child(tab(
            "tab-stacks",
            "Stacks",
            stacks_on,
            Press::ShowStacks(true),
        ));
    }
    div().flex().flex_col().child(strip).child(hairline(w))
}
