//! The toast (`.toast`; the iced window's `view::toast_card`): a passing
//! word centred at the stage's foot, in the raised fill inside the
//! floating edge, lifted by its shadow. It takes no pointer: the stage
//! under it keeps its hover and its clicks. The window's own rather than
//! Kit's `Notification`, whose stack stands in a corner with a close
//! button — the prototype's word is centred, brief and untouchable.

use gpui_kit::prelude::*;
use gpui_kit::{AnyElement, Context, TestSupportExt as _, Window, div};

use super::super::Gui;
use super::super::w::Floating;
use super::super::w::{REGULAR, W};
use super::{BORDER, enter};

/// The words' size, the insets, the corners, the distance from the
/// stage's foot and the least distance from either side.
const PX: f32 = 14.0;
const PAD: (f32, f32) = (8.0, 14.0);
const RADIUS: f32 = 8.0;
const BOTTOM: f32 = 18.0;
const SIDE: f32 = 16.0;

/// The toast over the stage, when there is a word to say.
pub fn view(gui: &Gui, w: &W, window: &mut Window, cx: &mut Context<Gui>) -> Option<AnyElement> {
    let toast = gui.cards_ui.toast.as_ref()?;
    let card = div()
        .id("toast")
        .test_support()
        .py(w.z(PAD.0 - BORDER))
        .px(w.z(PAD.1 - BORDER))
        .border(w.z(1.))
        .rounded(w.r(RADIUS))
        .floating(w, w.c(|t| t.raise), w.c(|t| t.edge), w.shadows.toast)
        .child(
            div()
                .font_family(w.ui.clone())
                .font_weight(REGULAR)
                .text_size(w.z(PX))
                .text_color(w.c(|t| t.ink))
                .child(toast.words.clone()),
        );
    let card = enter(format!("toast-enter-{}", toast.seq), card, w, window, cx);
    Some(
        div()
            .absolute()
            .left_0()
            .right_0()
            .bottom(w.z(BOTTOM))
            .px(w.z(SIDE))
            .flex()
            .justify_center()
            .child(card)
            .into_any_element(),
    )
}
