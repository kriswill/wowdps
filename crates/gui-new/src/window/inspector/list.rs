//! The inspector's lists (the iced window's `inspector/list.rs`, the
//! prototype's `.ilist`): a heading line, then the rows in the drawn order —
//! each a lead (an ability's icon or lettered square and its name, the pet
//! after it; a person's disc and name, "you" on the owner) then its figures
//! in their columns, over a 2 px bar. A Damage or Healing player's
//! abilities draw as R26's tree. The selected line is raised, the keys'
//! wears the accent down its edge, the pointer's a faint wash.

use std::cell::Cell;
use std::rc::Rc;
use std::sync::Arc;

use gpui_kit::prelude::*;
use gpui_kit::{
    AnyElement, Context, Div, ElementId, MouseButton, Pixels, RenderImage, ScrollHandle,
    TestSupportExt as _, canvas, div, img, relative,
};
use wowdps_gui_logic::glyph::Glyph;
use wowdps_gui_logic::inspect::list::{FOE, hue, sphere};
use wowdps_gui_logic::table::{self as gt, Col, Grid, split_pet};
use wowdps_gui_logic::{theme as gl, tree};
use wowdps_model::Row;

use super::model::{Bar, Lead, LinePress, List, Press};
use super::view::on;
use crate::theme::hsla;
use crate::window::Gui;
use crate::window::chrome::class_icon;
use crate::window::paint::glyph;
use crate::window::table::you_tag;
use crate::window::w::{MEDIUM, REGULAR, SEMIBOLD, W};

/// A row (`.irow{height:29px}`), its bar (`.ibar{height:2px;opacity:.55}`).
const ROW_H: f32 = 29.0;
const BAR_H: f32 = 2.0;
const BAR_ALPHA: f32 = 0.55;
const KEYED_EDGE: f32 = 2.0;
/// The ability's square, a person's disc, the gap after either.
const ICON: f32 = 17.0;
const DISC: f32 = 20.0;
const ICON_GAP: f32 = 7.0;
const LETTER_PX: f32 = 10.0;
const SQ_RADIUS: f32 = 3.0;
const NAME_PX: f32 = 14.0;
const PET_PX: f32 = 13.0;
const PET_GAP: f32 = 4.0;
/// R26: the fold's caret and its slot, the gap after it, the indent.
const CARET: f32 = 10.0;
const CARET_SLOT: f32 = 12.0;
const CARET_GAP: f32 = 5.0;
const INDENT: f32 = 14.0;
const YOU_GAP: f32 = 6.0;
/// The heading line's padding (`.ihrow{padding:6px 16px 4px}`) and the
/// list's (`.ilist{padding:4px 0 14px}`).
const HEAD_PAD: (f32, f32) = (6.0, 4.0);
const LIST_PAD: (f32, f32) = (4.0, 14.0);
const EMPTY_PAD_Y: f32 = 6.0;
const NOTE_PAD_Y: f32 = 2.0;

/// The list, `narrow` in an inspector 440 px or narrower.
pub fn view(l: &List, narrow: bool, keep: &Keep, w: &W, cx: &Context<Gui>) -> Div {
    let (cols, grid) = l.kind.columns(l.view, narrow);
    let mut list = div()
        .flex()
        .flex_col()
        .pt(w.z(LIST_PAD.0))
        .pb(w.z(LIST_PAD.1))
        .child(heads(l, &cols, grid, w, cx));
    if let Some(note) = &l.note {
        list = list.child(div().py(w.z(NOTE_PAD_Y)).px(w.z(l.side)).child(w.text(
            note.clone(),
            w.size.small,
            w.c(|t| t.ink_2),
            REGULAR,
        )));
    }
    let empty = |w: &W| {
        div().py(w.z(EMPTY_PAD_Y)).px(w.z(l.side)).child(w.text(
            "Nothing yet",
            13.,
            w.c(|t| t.ink_2),
            REGULAR,
        ))
    };
    if let Some(lines) = &l.tree {
        // A group's sum can pass any one row: bars scale by the largest
        // top-level line.
        let max = lines
            .iter()
            .filter(|ln| ln.depth == 0)
            .map(|ln| ln.row.amount)
            .max()
            .unwrap_or(1)
            .max(1);
        if lines.is_empty() {
            list = list.child(empty(w));
        }
        for (at, ln) in lines.iter().enumerate() {
            list = list.child(tree_line(l, at, ln, max, &cols, grid, keep, w, cx));
        }
        return list;
    }
    let max = l.rows.iter().map(|r| r.amount).max().unwrap_or(1).max(1);
    let drawn = gt::sorted(l.rows.iter().cloned().enumerate().collect(), l.sort);
    if drawn.is_empty() {
        list = list.child(empty(w));
    }
    for (i, r) in drawn {
        let lead = match l.lead {
            Lead::Spell => {
                let (name, pet) = split_pet(&r.label);
                spell_words(name, pet, r.spell_id, w)
            }
            Lead::Person => person_lead(&r, l.you == Some(i), w),
        };
        let press = match l.press {
            LinePress::Nothing => None,
            LinePress::Spell => Some(Press::SpellRow(i)),
            LinePress::Attacker => Some(Press::AttackerRow(i)),
            LinePress::Pair => Some(Press::CompareSpell(r.key.clone(), r.label.clone())),
        };
        let hue = l.hues.get(&r.key).copied();
        list = list.child(line(
            l, i, &r, lead, press, hue, max, &cols, grid, keep, w, cx,
        ));
    }
    list
}

/// The heading line: the list's head over the names, then a head per
/// column — the ability list's sort on a press.
fn heads(l: &List, cols: &[Col], grid: Grid, w: &W, cx: &Context<Gui>) -> Div {
    let gap = grid.gap();
    let head_ink = l.head_ink.map_or(w.c(|t| t.gold_dim), hsla);
    let mut line = div()
        .flex()
        .items_center()
        .gap(w.z(gap))
        .pt(w.z(HEAD_PAD.0))
        .pb(w.z(HEAD_PAD.1))
        .px(w.z(l.side))
        .child(div().flex_1().min_w_0().overflow_hidden().child(w.text(
            l.head.clone(),
            grid.head_px(),
            head_ink,
            if l.head_ink.is_some() {
                SEMIBOLD
            } else {
                REGULAR
            },
        )));
    for &c in cols {
        let sorted = l.sort.filter(|(s, _)| *s == c).map(|(_, desc)| desc);
        let mut words = div()
            .w(w.z(grid.width(c)))
            .flex_none()
            .flex()
            .items_center()
            .justify_end()
            .gap(w.z(2.))
            .child(w.words(c.head(l.view), grid.head_px(), REGULAR));
        if let Some(desc) = sorted {
            words = words.child(glyph(
                if desc {
                    Glyph::ArrowDown
                } else {
                    Glyph::ArrowUp
                },
                w.z(grid.head_px()),
                w.c(|t| t.gold),
            ));
        }
        line = line.child(if l.sortable {
            div()
                .id(ElementId::from((
                    ElementId::Name("isort".into()),
                    gpui_kit::SharedString::from(c.head(l.view)),
                )))
                .cursor_pointer()
                .text_color(if sorted.is_some() {
                    w.c(|t| t.gold)
                } else {
                    w.c(|t| t.gold_dim)
                })
                .hover(|s| s.text_color(w.c(|t| t.gold)))
                .child(words)
                .on_mouse_down(MouseButton::Left, on(Press::Sort(c), cx))
                .into_any_element()
        } else {
            words.text_color(w.c(|t| t.gold_dim)).into_any_element()
        });
    }
    line
}

/// A figure's ink (`.num` / `.num.faint`): the amount and the rate in ink,
/// the rest in ink 2.
fn cell_ink(c: Col, w: &W) -> gpui_kit::Hsla {
    match c.rank() {
        0 | 1 => w.c(|t| t.ink),
        _ => w.c(|t| t.ink_2),
    }
}

/// R26: a tree line — indented by depth, a caret where it folds (an empty
/// slot of its width where it does not), then its words.
#[allow(clippy::too_many_arguments)]
fn tree_line(
    l: &List,
    at: usize,
    ln: &tree::Line,
    max: u64,
    cols: &[Col],
    grid: Grid,
    keep: &Keep,
    w: &W,
    cx: &Context<Gui>,
) -> AnyElement {
    let caret: AnyElement = match (ln.fold, &ln.fold_key) {
        (Some(open), Some(key)) => div()
            .id(ElementId::from((
                ElementId::Name("fold".into()),
                gpui_kit::SharedString::from(key.clone()),
            )))
            .test_support()
            .size(w.z(CARET_SLOT))
            .flex_none()
            .flex()
            .items_center()
            .justify_center()
            .cursor_pointer()
            .child(glyph(
                if open {
                    Glyph::ChevronDown
                } else {
                    Glyph::ChevronRight
                },
                w.z(CARET),
                w.c(|t| t.ink_2),
            ))
            .on_mouse_down(MouseButton::Left, {
                let fold = on(Press::Fold(key.clone()), cx);
                move |e, window, cx| {
                    fold(e, window, cx);
                    cx.stop_propagation();
                }
            })
            .into_any_element(),
        _ => div().size(w.z(CARET_SLOT)).flex_none().into_any_element(),
    };
    let lead = div()
        .flex()
        .items_center()
        .gap(w.z(CARET_GAP))
        .min_w_0()
        // iced's row spaces no void element: a top-level line has no indent
        // and no gap before its caret.
        .when(ln.depth > 0, |d| {
            d.child(div().w(w.z(INDENT * f32::from(ln.depth))).flex_none())
        })
        .child(caret)
        .child(spell_words(
            &ln.name,
            ln.tail.as_deref(),
            ln.row.spell_id,
            w,
        ))
        .into_any_element();
    let press = match (&ln.opens, &ln.fold_key, l.press) {
        (None, Some(key), _) => Some(Press::Fold(key.clone())),
        (Some(i), _, LinePress::Spell) => Some(Press::SpellRow(*i)),
        _ => None,
    };
    let hue = l.hues.get(&ln.entry).copied();
    line(
        l, at, &ln.row, lead, press, hue, max, cols, grid, keep, w, cx,
    )
}

/// A drawn line: `lead`, then the columns, over the row's bar.
#[allow(clippy::too_many_arguments)]
fn line(
    l: &List,
    i: usize,
    r: &Row,
    lead: AnyElement,
    press: Option<Press>,
    hue: Option<gl::Color>,
    max: u64,
    cols: &[Col],
    grid: Grid,
    keep: &Keep,
    w: &W,
    cx: &Context<Gui>,
) -> AnyElement {
    let color = match (hue, l.bar) {
        (Some(h), _) => h,
        (None, Bar::Of(c)) => c,
        (None, Bar::Own) => r.class.map_or(w.t.hostile, gl::Color::of_class),
    };
    // A band's hue is its legend: solid, as the band is drawn.
    let bar_alpha = if hue.is_some() { 1.0 } else { BAR_ALPHA };
    let share = (r.amount as f64 / max as f64).clamp(0.0, 1.0) as f32;
    let mut figures = div().flex_none().flex().gap(w.z(grid.gap()));
    for &c in cols {
        figures = figures.child(
            div()
                .w(w.z(grid.width(c)))
                .flex_none()
                .flex()
                .justify_end()
                .child(w.text(c.cell(r), w.size.num, cell_ink(c, w), REGULAR)),
        );
    }
    let selected = l.selected == Some(i);
    let keyed = selected && l.keyed;
    let hovered = l.hover == Some(i)
        || (l.press == LinePress::Pair && l.pair_hover.as_deref() == Some(r.key.as_str()));
    let id = ElementId::NamedInteger(
        match l.pane {
            wowdps_model::Pane::Spell => "iline".into(),
            wowdps_model::Pane::Target => "itarget".into(),
        },
        i as u64,
    );
    let pane = l.pane;
    let key = r.key.clone();
    let pair = l.press == LinePress::Pair;
    let mut body = div()
        .id(id)
        .test_support()
        .aria_selected(selected)
        .relative()
        .h(w.z(ROW_H))
        .px(w.z(l.side))
        .flex()
        .flex_col()
        .when(selected, |d| d.bg(w.c(|t| t.raise)))
        .when(!selected && hovered, |d| d.bg(w.c(|t| t.hover)))
        .child(
            div()
                .flex_1()
                .min_h_0()
                .flex()
                .items_center()
                .gap(w.z(grid.gap()))
                .child(div().flex_1().min_w_0().overflow_hidden().child(lead))
                .child(figures),
        )
        .child(
            div().h(w.z(BAR_H)).w_full().flex_none().child(
                div()
                    .h_full()
                    .w(relative(share))
                    .rounded(w.z(1.))
                    .bg(hsla(color.alpha(bar_alpha))),
            ),
        )
        .child(div().h(w.z(BAR_H)).flex_none())
        .on_hover(cx.listener(move |this, over: &bool, _, cx| {
            if pair {
                let now = over.then(|| key.clone());
                if this.insp.spell_hover != now
                    && (*over || this.insp.spell_hover.as_ref() == Some(&key))
                {
                    this.insp.spell_hover = now;
                    cx.notify();
                }
            } else {
                let now = over.then_some((pane, i));
                if this.insp.hover != now && (*over || this.insp.hover == Some((pane, i))) {
                    this.insp.hover = now;
                    cx.notify();
                }
            }
        }));
    if keyed {
        // The keys are here: the accent down the row's edge, over it.
        body = body.child(
            div()
                .absolute()
                .top_0()
                .left_0()
                .bottom_0()
                .w(w.z(KEYED_EDGE))
                .bg(w.accent()),
        );
        if keep.pending.get() {
            body = body.child(keep.probe());
        }
    }
    if let Some(press) = press {
        body = body
            .cursor_pointer()
            .on_mouse_down(MouseButton::Left, on(press, cx));
    }
    body.into_any_element()
}

/// An ability's icon (or a square lettered with its initial in the name's
/// hue), its name, and the pet's or the part's words after it, quieter.
fn spell_words(name: &str, pet: Option<&str>, spell_id: u32, w: &W) -> AnyElement {
    let icon: AnyElement = match crate::images::spell_icon(spell_id) {
        Some(tile) => img(tile).size(w.z(ICON)).flex_none().into_any_element(),
        None => div()
            .size(w.z(ICON))
            .flex_none()
            .flex()
            .items_center()
            .justify_center()
            .rounded(w.z(SQ_RADIUS))
            .bg(hsla(hue(name)))
            .child(w.text(
                name.chars().next().map(String::from).unwrap_or_default(),
                LETTER_PX,
                hsla(gl::Color::rgba(0.0, 0.0, 0.0, 0.6)),
                SEMIBOLD,
            ))
            .into_any_element(),
    };
    let mut words = div()
        .flex_1()
        .min_w_0()
        .flex()
        .items_baseline()
        .overflow_hidden()
        .whitespace_nowrap()
        .child(
            div()
                .flex_shrink_0()
                .max_w_full()
                .overflow_hidden()
                .text_ellipsis()
                .child(w.text(name.to_string(), NAME_PX, w.c(|t| t.ink), REGULAR)),
        );
    if let Some(pet) = pet {
        words = words.child(
            div()
                .ml(w.z(PET_GAP))
                .min_w_0()
                .overflow_hidden()
                .text_ellipsis()
                .child(w.text(pet.to_string(), PET_PX, w.c(|t| t.ink_3_text), REGULAR)),
        );
    }
    div()
        .flex()
        .items_center()
        .gap(w.z(ICON_GAP))
        .min_w_0()
        .child(icon)
        .child(words)
        .into_any_element()
}

/// A person's lead: their disc (a creature's the foe's) and their name —
/// the owner's with their "you" tag after it.
fn person_lead(r: &Row, you: bool, w: &W) -> AnyElement {
    let disc = if r.class.is_some() {
        class_icon(w, r.class, r.spec, w.z(DISC), false)
    } else {
        foe_disc(w.z(DISC))
    };
    div()
        .flex()
        .items_center()
        .gap(w.z(ICON_GAP))
        .min_w_0()
        .child(disc)
        .child(
            div()
                .min_w_0()
                .overflow_hidden()
                .text_ellipsis()
                .child(w.text(r.label.clone(), NAME_PX, w.c(|t| t.ink), MEDIUM)),
        )
        .when(you, |d| {
            d.child(div().ml(w.z(YOU_GAP - ICON_GAP)).child(you_tag(r.class, w)))
        })
        .into_any_element()
}

/// A foe's disc, `d` across: the prototype's lit sphere, rasterised once
/// per size at twice it.
pub fn foe_disc(d: Pixels) -> AnyElement {
    img(foe_image(d)).size(d).flex_none().into_any_element()
}

fn foe_image(d: Pixels) -> Arc<RenderImage> {
    use std::collections::HashMap;
    use std::sync::{Mutex, OnceLock};
    static CACHE: OnceLock<Mutex<HashMap<u32, Arc<RenderImage>>>> = OnceLock::new();
    let n = (f32::from(d) * 2.0).round().max(2.0) as u32;
    let cache = CACHE.get_or_init(Default::default);
    let mut tiles = cache.lock().unwrap_or_else(|e| e.into_inner());
    tiles
        .entry(n)
        .or_insert_with(|| {
            crate::images::make(wowdps_gui_logic::lazy_tiles::Rgba {
                w: n,
                h: n,
                pixels: sphere(n, FOE),
            })
        })
        .clone()
}

/// The keyed line kept in the inspector's sight: the inspector scrolls as
/// a whole, its lists deep inside it, so a step brings the line in once it
/// is laid out rather than through the scroll's own children.
pub struct Keep {
    pub scroll: ScrollHandle,
    pub pending: Rc<Cell<bool>>,
}

impl Keep {
    /// A probe over the keyed line: after layout, the least scroll that
    /// shows the line whole, asked once.
    pub fn probe(&self) -> AnyElement {
        let scroll = self.scroll.clone();
        let pending = self.pending.clone();
        canvas(
            move |line, window, _| {
                if !pending.replace(false) {
                    return;
                }
                let view = scroll.bounds();
                let mut offset = scroll.offset();
                if line.top() < view.top() {
                    offset.y += view.top() - line.top();
                } else if line.bottom() > view.bottom() {
                    offset.y -= line.bottom() - view.bottom();
                } else {
                    return;
                }
                scroll.set_offset(offset);
                window.refresh();
            },
            |_, _, _, _| {},
        )
        .absolute()
        .top_0()
        .left_0()
        .size_full()
        .into_any_element()
    }
}
