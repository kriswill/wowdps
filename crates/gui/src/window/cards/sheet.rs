//! The `?` sheet (`.sheet`; the iced window's `nav::shortcut_sheet`):
//! "Keyboard", then every binding that works on this surface in the
//! prototype's four groups — move, views, the inspector, go to — each a
//! line of what it does and its keycaps on the right, the groups in as
//! many 180 px columns as the sheet holds. A key the pull on the stage
//! cannot answer is listed, dimmed: it works on the surface, not on this
//! pull. Over a dimmed scrim; any press anywhere, or any key, closes it.
//!
//! The keycaps are the window's own `kbd`, not Kit's `Kbd`: Kit's prints a
//! single letter in capitals and a chord as one cap, so `j` and `J`, `k`
//! and the Deaths view's `K`, would read alike — and the sheet is where a
//! reader learns which is which.

use gpui_kit::prelude::*;
use gpui_kit::{AnyElement, Context, Div, MouseButton, TestSupportExt as _, Window, div};
use wowdps_gui_logic::keys::{Binding, keycaps, sheet_groups};
use wowdps_gui_logic::labels::sentence;
use wowdps_gui_logic::theme::SHADOW_SHEET;

use super::super::Gui;
use super::super::chrome::kbd;
use super::super::w::{REGULAR, SEMIBOLD, W};
use super::{BORDER, enter};

/// The sheet's widest (`.sheet{width:min(640px, …)}`), the narrowest a
/// column is (`.cols{grid-template-columns:repeat(auto-fill,minmax(180px,
/// 1fr))}`) and the gap between two.
const SHEET_W: f32 = 640.0;
const COL: f32 = 180.0;
const COL_GAP: f32 = 22.0;
/// Where the card hangs (`.overlay`): 64 px down, 12 px clear of a narrow
/// window's edges.
const TOP: f32 = 64.0;
const SIDE: f32 = 12.0;
/// The card's insets, corners, the air between a line and its keycaps,
/// and between two keycaps.
const PAD: (f32, f32, f32) = (16.0, 18.0, 18.0);
const RADIUS: f32 = 10.0;
const CAPS_GAP: f32 = 10.0;
const KBD_GAP: f32 = 4.0;
/// The heading's lines (`.sheet h3{margin:0 0 2px}`), and the air between
/// its words and the first group: the words' 12 px and the group heading's
/// 10 px, less the two gaps the spacer stands between.
const HEAD_GAP: f32 = 2.0;
const LEAD: f32 = 12.0 + 10.0 - 2.0 * HEAD_GAP;
/// Between two rows of groups, from a row's last line to the next row's
/// headings: the line's 2 px, the grid's 4 px and the heading's 10 px.
const ROW_GAP: f32 = 2.0 + 4.0 + 10.0;
/// Between a group's lines.
const LINE_GAP: f32 = 4.0;

/// How many columns the sheet holds at a window `width` wide (zoom 1).
pub fn columns(width: f32, groups: usize) -> usize {
    let card = SHEET_W.min(width - 2.0 * SIDE);
    let inner = card - 2.0 * PAD.1;
    (((inner + COL_GAP) / (COL + COL_GAP)).floor() as usize).clamp(1, groups.max(1))
}

/// The sheet over the window.
pub fn view(gui: &Gui, w: &W, window: &mut Window, cx: &mut Context<Gui>) -> AnyElement {
    let surface = gui.surface(cx);
    let inert = gui.inert_keys(cx);
    let groups = sheet_groups(surface);
    let per_row = columns(w.width, groups.len());
    let mut grid = div().flex().flex_col().gap(w.z(ROW_GAP));
    for chunk in groups.chunks(per_row) {
        let mut line = div().flex().gap(w.z(COL_GAP));
        for (group, bindings) in chunk {
            line = line.child(group_lines(group, bindings, &inert, w).flex_1().min_w_0());
        }
        // A short last row keeps the columns above it: the same widths.
        for _ in chunk.len()..per_row {
            line = line.child(div().flex_1());
        }
        grid = grid.child(line);
    }
    // A window too short for every group scrolls them under the heading.
    let body = div()
        .id("sheet-body")
        .flex_shrink(1.)
        .min_h_0()
        .overflow_y_scroll()
        .track_scroll(&gui.cards_ui.sheet_scroll)
        .child(grid);
    let card = div()
        .id("sheet")
        .test_support()
        .w_full()
        .max_w(w.z(SHEET_W))
        .max_h_full()
        .flex()
        .flex_col()
        .gap(w.z(HEAD_GAP))
        .pt(w.z(PAD.0 - BORDER))
        .px(w.z(PAD.1 - BORDER))
        .pb(w.z(PAD.2 - BORDER))
        .bg(w.c(|t| t.surface))
        .border(w.z(1.))
        .border_color(w.c(|t| t.edge))
        .rounded(w.z(RADIUS))
        .shadow(vec![w.shadow(SHADOW_SHEET)])
        .child(w.text("Keyboard", w.size.title, w.c(|t| t.ink), SEMIBOLD))
        .child(w.text(
            format!(
                "What works on {}. Any key or click closes this.",
                surface.name()
            ),
            w.size.small,
            w.c(|t| t.ink_3_text),
            REGULAR,
        ))
        .child(div().h(w.z(LEAD)).flex_none())
        .child(super::super::chrome::scrolled(
            body,
            &gui.cards_ui.sheet_scroll,
            w,
        ));
    // The scrim is the dismiss target as much as the card is: a modal you
    // cannot click away from is a trap. The whole of it is opaque to the
    // pointer, so nothing under it hears a wheel or lights a hover.
    let card = enter("sheet-enter", card, w, window, cx);
    div()
        .id("sheet-scrim")
        .test_support()
        .absolute()
        .inset_0()
        .occlude()
        .bg(w.c(|t| t.scrim))
        .flex()
        .justify_center()
        .items_start()
        .pt(w.z(TOP))
        .px(w.z(SIDE))
        .pb(w.z(SIDE))
        .child(card)
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(|this, _, _, cx| this.close_menus(cx)),
        )
        .into_any_element()
}

/// One group: its gold-dim heading (`.sheet h4`), then a line per binding.
fn group_lines(group: &str, bindings: &[&Binding], inert: &[&str], w: &W) -> Div {
    let mut lines = div().flex().flex_col().gap(w.z(LINE_GAP)).child(w.text(
        sentence(group),
        w.size.micro,
        w.c(|t| t.gold_dim),
        SEMIBOLD,
    ));
    for b in bindings {
        let caps = keycaps(b.keys).into_iter().fold(
            div().flex_none().flex().items_center().gap(w.z(KBD_GAP)),
            |row, cap| row.child(kbd(w, cap)),
        );
        // A key the pull on the stage cannot answer: listed, dimmed.
        let ink = if inert.contains(&b.keys) {
            w.c(|t| t.ink_3)
        } else {
            w.c(|t| t.ink)
        };
        lines = lines.child(
            div()
                .flex()
                .items_center()
                .gap(w.z(CAPS_GAP))
                .child(
                    // A line too long for its column wraps under itself
                    // rather than lose its end under the keycaps.
                    div()
                        .flex_1()
                        .min_w_0()
                        .font_family(w.ui)
                        .font_weight(REGULAR)
                        .text_size(w.z(w.size.sheet_key))
                        .text_color(ink)
                        .child(sentence(b.what)),
                )
                .child(caps),
        );
    }
    lines
}
