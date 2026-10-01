//! The window's text field: Kit's unstyled `Input` (gpui-base's, on the
//! same `InputState` as Kit's styled one) composed in the window's own
//! frame, its colours projected from the window's tokens on every render.
//!
//! Kit's styled `Input` paints a placeholder in the theme's one
//! `muted_foreground` (ink 2, which Kit's tooltips, tabs and keycaps also
//! read) and forces its own text size (`text_sm`, whatever the zoom). The
//! iced fields paint their hint in ink 3's text grade at the field's own
//! size, so the window's fields come here instead: the caret, selection,
//! IME, horizontal scroll and clipping stay Kit's engine's; the colours
//! and the size are ours. The placeholder's words stay on the state
//! (`InputState::placeholder`), and so does the field's key context
//! (`Input`), so `Meter && !Input` and `Filter > Input` / `Palette >
//! Input` bind as before.

use gpui_kit::base::input::{Input as BaseInput, InputEditorStyle};
use gpui_kit::component::input::InputState;
use gpui_kit::prelude::*;
use gpui_kit::{
    App, Div, ElementId, Entity, Interactivity, Stateful, StyleRefinement, TestSupportExt as _,
    Window, div, relative,
};

use super::w::W;

/// A field's line, as Kit's own frame sets it (`Rems(1.25)`): here
/// relative to the field's text, so it follows the zoom.
const LINE: f32 = 1.25;

/// A text field over `state`, in the window's face, ink and size.
#[derive(IntoElement)]
pub struct Field {
    base: Stateful<Div>,
    state: Entity<InputState>,
    w: W,
    /// The text's size at zoom 1.
    size: f32,
}

impl Field {
    /// The field `id` over `state`, at the window's frame size until
    /// [`Field::size`] says otherwise.
    pub fn new(id: impl Into<ElementId>, state: &Entity<InputState>, w: &W) -> Self {
        Self {
            base: div().id(id),
            state: state.clone(),
            w: *w,
            size: w.size.frame,
        }
    }

    /// The text's size at zoom 1.
    pub fn size(mut self, size: f32) -> Self {
        self.size = size;
        self
    }
}

impl Styled for Field {
    fn style(&mut self) -> &mut StyleRefinement {
        self.base.style()
    }
}

impl InteractiveElement for Field {
    fn interactivity(&mut self) -> &mut Interactivity {
        self.base.interactivity()
    }
}

impl StatefulInteractiveElement for Field {}

impl RenderOnce for Field {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let w = self.w;
        // Projected every render: a theme switch repaints the window, and
        // the next frame's field wears the new tokens.
        let style = InputEditorStyle {
            foreground: w.c(|t| t.ink),
            // The placeholder.
            muted_foreground: w.c(|t| t.ink_3_text),
            caret: w.c(|t| t.ink),
            selection: w.c(|t| t.selection),
            ..InputEditorStyle::default()
        };
        self.state.update(cx, |s, _| s.set_editor_style(style));
        self.base
            .test_support()
            .flex()
            .items_center()
            .min_w_0()
            .font_family(w.ui)
            .text_size(w.z(self.size))
            .line_height(relative(LINE))
            .text_color(w.c(|t| t.ink))
            .child(BaseInput::new(&self.state))
    }
}
