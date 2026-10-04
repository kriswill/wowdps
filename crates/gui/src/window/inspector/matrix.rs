//! R21's stack ledger behind the Taken drill's "Stacks" tab (layout D of
//! the design study; the iced window's `taken::stack_matrix`): one table
//! per debuff — rows the abilities that hit the player, columns the open
//! debuff's stack level, cells the average hit, heat-shaded against the
//! ROW so reading across one is the whole ruling: how much worse does this
//! get per stack. The derivation (level 0 per debuff) and the heat are
//! gui-logic's (`inspect::matrix`); this draws them on a panel with its
//! foot: how level 0 is had, and the hits the ledger dropped.
//!
//! The delight, gone under reduced motion: each cell stands on a faint wash
//! of its heat, so the ladder reads as a heat map before a number is read,
//! and a row lights under the pointer. Every cell's tooltip says how many
//! hits its average is over.

use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::prelude::*;
use gpui_kit::{
    AnyElement, App, Div, ElementId, Hsla, SharedString, TestSupportExt as _, div, relative,
};
use wowdps_gui_logic::inspect::matrix::{FOOT, dropped_words, heat};
use wowdps_gui_logic::theme::Density;
use wowdps_model::fmt::{commas, human};

pub use wowdps_gui_logic::inspect::matrix::Matrix;

use crate::theme::hsla;
use crate::window::w::{REGULAR, SEMIBOLD, W};

/// A row's cell gap, a level's column and the hits column (the iced
/// table's), the gaps between the lines and the tables, and the panel's
/// corner.
const GAP: f32 = 6.0;
const LEVEL_W: f32 = 56.0;
const HITS_W: f32 = 44.0;
const LINE_GAP: f32 = 2.0;
const TABLE_GAP: f32 = 8.0;
const FOOT_GAP: f32 = 8.0;
const RADIUS: f32 = 8.0;
/// The window's line height (its root's, iced's default).
const LEADING: f32 = 1.3;
/// The delight's heat wash under a cell, and its corner.
const WASH_ALPHA: f32 = 0.13;
const WASH_RADIUS: f32 = 3.0;

/// The matrices drawn as one panel named `id`: a titled table per debuff,
/// then the foot — `None` when the ledger is empty (no debuff was open
/// while a hit landed).
pub fn matrix(
    id: impl Into<ElementId>,
    matrices: &[Matrix],
    dropped: u32,
    w: &W,
    cx: &App,
) -> Option<AnyElement> {
    if matrices.is_empty() {
        return None;
    }
    let id: ElementId = id.into();
    let delight = !cx.reduce_motion();
    let label = w.c(|t| t.label_ink);
    let ink = w.c(|t| t.ink);
    let ink_2 = w.c(|t| t.ink_2);
    let faint = w.c(|t| t.ink_3);
    // A number standing at the right of its column.
    let cell = |words: String, color: Hsla, width: f32| {
        div()
            .w(w.z(width))
            .flex_none()
            .flex()
            .justify_end()
            .child(w.text(words, w.size.micro, color, REGULAR))
    };
    let mut body = div().flex().flex_col().gap(w.z(TABLE_GAP));
    for (mi, m) in matrices.iter().enumerate() {
        let mut head = div().flex().items_center().gap(w.z(GAP)).child(
            div().flex_1().min_w_0().overflow_hidden().child(w.text(
                format!("{} · stacks", m.aura),
                w.size.small,
                ink,
                SEMIBOLD,
            )),
        );
        for level in 0..=m.max_level {
            head = head.child(cell(level.to_string(), label, LEVEL_W));
        }
        head = head.child(cell("hits".to_string(), label, HITS_W));
        let mut table = div().flex().flex_col().gap(w.z(LINE_GAP)).child(head);
        for (ri, r) in m.rows.iter().enumerate() {
            let mut line = div()
                .id(ElementId::named_usize(format!("stack-row-{mi}"), ri))
                .flex()
                .items_center()
                .gap(w.z(GAP))
                .rounded(w.r(WASH_RADIUS))
                .when(delight, |d| d.hover(|s| s.bg(w.c(|t| t.hover))))
                .child(div().flex_1().min_w_0().overflow_hidden().child(w.text(
                    r.label.clone(),
                    w.size.micro,
                    ink,
                    REGULAR,
                )));
            for (li, c) in r.cells.iter().enumerate() {
                line = line.child(match c {
                    Some((hits, avg)) => {
                        let color = heat(r.heat_t(*avg), &w.t);
                        let words: SharedString = format!(
                            "{} {} at {li}, averaging {}",
                            commas(u64::from(*hits)),
                            if *hits == 1 { "hit" } else { "hits" },
                            commas(*avg)
                        )
                        .into();
                        cell(human(*avg), hsla(color), LEVEL_W)
                            .id(ElementId::named_usize(format!("stack-cell-{mi}-{ri}"), li))
                            .test_support()
                            .rounded(w.r(WASH_RADIUS))
                            .when(delight, |d| d.bg(hsla(color.alpha(WASH_ALPHA))))
                            .tooltip(move |window, cx| {
                                Tooltip::new(words.clone()).build(window, cx)
                            })
                            .into_any_element()
                    }
                    None => cell("—".to_string(), faint, LEVEL_W).into_any_element(),
                });
            }
            line = line.child(cell(r.hits().to_string(), ink_2, HITS_W));
            table = table.child(line);
        }
        body = body.child(table);
    }
    // The foot as iced's row lays it: the rule's words at their own width
    // (wrapping only past the row's), the dropped hits in what is left.
    let mut foot = div().flex().items_start().gap(w.z(FOOT_GAP)).child(
        div().min_w_0().child(
            w.text(FOOT, w.size.tiny, ink_2, REGULAR)
                .whitespace_normal(),
        ),
    );
    if let Some(words) = dropped_words(dropped) {
        foot = foot.child(
            div()
                .flex_1()
                .min_w_0()
                .child(w.text(words, w.size.tiny, ink, REGULAR).whitespace_normal()),
        );
    }
    body = body.child(foot);
    Some(
        panel(w)
            .id(id)
            .test_support()
            .child(body)
            .into_any_element(),
    )
}

/// The panel the matrices stand on (`surface_style(8)`): the surface under
/// a hairline, padded as a comfortable row is — iced draws a border inside
/// its padding and GPUI's adds to it, so the padding gives the border's
/// width back. The window's line height, so the component stands alone.
fn panel(w: &W) -> Div {
    div()
        .w_full()
        .line_height(relative(LEADING))
        .p(w.z(w.pitch.pad_of(Density::Comfortable) - 1.))
        .bg(w.c(|t| t.surface))
        .border(w.z(1.))
        .border_color(w.c(|t| t.line))
        .rounded(w.r(RADIUS))
}

#[cfg(test)]
mod tests {
    use gpui_kit::test::TestWindowExt as _;
    use gpui_kit::{
        AppContext as _, Context, ElementId, IntoElement, ParentElement as _, Render, Styled as _,
        TestAppContext, Window, div, px, size,
    };
    use wowdps_gui_logic::inspect::matrix::{matrices, samples::ledger};
    use wowdps_gui_logic::theme::NAVY;

    use super::{Matrix, matrix};
    use crate::testkit;
    use crate::window::w::W;

    struct Host(Vec<Matrix>);

    impl Render for Host {
        fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            let w = W::new(1.0, 1440.0, cx);
            div()
                .w(px(488.))
                .children(matrix("stacks", &self.0, 3, &w, cx))
        }
    }

    /// The ledger draws one table per debuff with a cell per level — level
    /// 0 derived — and nothing at all when no debuff was open.
    #[gpui_kit::test]
    fn the_ledger_draws_a_table_per_debuff(cx: &mut TestAppContext) {
        let (d, c, b) = ledger();
        let m = matrices(&d, &c, &b);
        let (window, _) = testkit::open(cx, size(px(520.), px(240.)), move |_, cx| {
            crate::theme::apply(&NAVY, None, cx);
            cx.new(|_| Host(m))
        });
        cx.update_window(window, |_, window, cx| {
            window.render_frame(cx);
            assert!(window.try_find("stacks").is_some());
            for level in 0..=3 {
                let cell = ElementId::named_usize("stack-cell-0-0", level);
                assert!(window.try_find(cell).is_some(), "level {level}");
            }
            let w = W::new(1.0, 1440.0, cx);
            assert!(matrix("none", &[], 0, &w, cx).is_none());
        })
        .unwrap();
    }
}
