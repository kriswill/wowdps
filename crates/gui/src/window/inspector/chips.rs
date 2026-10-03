//! R9's death navigator over a Deaths drill (the iced window's
//! `taken::death_chips`): "N deaths", the windows the daemon no longer
//! keeps named in words, one chip per kept death window — the Deaths
//! view's own skull and its clock time, the shown one lit by the accent's
//! edge on its wash — and the keys that step them at the far end. Nothing
//! when there is only the one death to show.
//!
//! The delight: a chip under the pointer brightens to the ink and its edge
//! to the floating one, as a control does. At rest the strip is iced's.

use std::rc::Rc;

use gpui_kit::prelude::*;
use gpui_kit::{
    AnyElement, App, ElementId, MouseButton, TestSupportExt as _, Window, div, relative,
};
use wowdps_gui_logic::glyph::Glyph;
use wowdps_gui_logic::theme::accent_wash;
use wowdps_model::fmt::duration;
use wowdps_proto::DeathWindow;

use crate::theme::hsla;
use crate::window::paint::glyph_ink;
use crate::window::w::{REGULAR, W};

/// What a chip's press picks: the death window's index.
pub type OnPick = Rc<dyn Fn(u32, &mut Window, &mut App)>;

/// The strip's gap, a chip's padding (`[3, 10]` — iced draws the border
/// inside it, GPUI adds it outside, so a chip pads one less), its pill's
/// corner, the skull and the gap after it.
const GAP: f32 = 6.0;
const PAD: (f32, f32) = (10.0, 3.0);
const RADIUS: f32 = 13.0;
const SKULL: f32 = 13.0;
const SKULL_GAP: f32 = 5.0;
/// The window's line height (its root's, iced's default), so the strip
/// stands alone.
const LEADING: f32 = 1.3;

/// The chip strip for `deaths`, `shown` lit, `dropped` older windows named
/// in words; `None` for a single death with nothing dropped.
pub fn chips(
    deaths: &[DeathWindow],
    shown: Option<u32>,
    dropped: u32,
    w: &W,
    on_pick: OnPick,
) -> Option<AnyElement> {
    if deaths.len() < 2 && dropped == 0 {
        return None;
    }
    let ink_2 = w.c(|t| t.ink_2);
    let mut strip = div()
        .id("death-chips")
        .line_height(relative(LEADING))
        .test_support()
        .w_full()
        .flex()
        .items_center()
        .gap(w.z(GAP))
        .child(w.text(
            format!("{} deaths", deaths.len() + dropped as usize),
            w.size.tiny,
            ink_2,
            REGULAR,
        ));
    if dropped > 0 {
        strip = strip.child(w.text(
            format!("{dropped} older not kept"),
            w.size.tiny,
            ink_2,
            REGULAR,
        ));
    }
    for d in deaths {
        let on = shown == Some(d.index);
        let index = d.index;
        let pick = on_pick.clone();
        let ink = if on { w.c(|t| t.ink) } else { ink_2 };
        let chip = div()
            .id(ElementId::named_usize("death-chip", d.index as usize))
            .test_support()
            .flex_none()
            .flex()
            .items_center()
            .gap(w.z(SKULL_GAP))
            .px(w.z(PAD.0 - 1.))
            .py(w.z(PAD.1 - 1.))
            .rounded(w.pill(RADIUS))
            .border(w.z(1.))
            .border_color(if on { w.accent() } else { w.c(|t| t.line) })
            .when(on, |c| c.bg(hsla(accent_wash(w.accent))))
            .cursor_pointer()
            .text_color(ink)
            .when(!on, |c| {
                c.hover(|s| s.text_color(w.c(|t| t.ink)).border_color(w.c(|t| t.edge)))
            })
            .child(glyph_ink(Glyph::Skull, w.z(SKULL)))
            .child(w.words(duration(d.at_ms), w.size.micro, REGULAR))
            .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                pick(index, window, cx);
                cx.stop_propagation();
            });
        strip = strip.child(chip);
    }
    Some(
        strip
            .child(div().flex_1())
            .child(w.text("← → step", w.size.tiny, w.c(|t| t.ink_3_text), REGULAR))
            .into_any_element(),
    )
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::rc::Rc;

    use gpui_kit::test::TestWindowExt as _;
    use gpui_kit::{
        App, AppContext as _, Context, ElementId, IntoElement, ParentElement as _, Render,
        Styled as _, TestAppContext, Window, div, px, size,
    };
    use wowdps_gui_logic::theme::NAVY;
    use wowdps_proto::DeathWindow;

    use super::{OnPick, chips};
    use crate::testkit;
    use crate::window::w::W;

    fn deaths() -> Vec<DeathWindow> {
        [64_000, 158_000, 231_000]
            .into_iter()
            .enumerate()
            .map(|(i, at_ms)| DeathWindow {
                index: i as u32,
                at_ms,
            })
            .collect()
    }

    struct Host(Rc<RefCell<Vec<u32>>>);

    impl Render for Host {
        fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            let w = W::new(1.0, 1440.0, cx);
            let picked = self.0.clone();
            div().w(px(488.)).children(chips(
                &deaths(),
                Some(2),
                1,
                &w,
                Rc::new(move |i, _, _| picked.borrow_mut().push(i)),
            ))
        }
    }

    /// A chip's press picks its death window; one death with nothing
    /// dropped draws no strip, and a dropped window brings it back.
    #[gpui_kit::test]
    fn a_chip_picks_its_window_and_one_death_needs_none(cx: &mut TestAppContext) {
        let picked: Rc<RefCell<Vec<u32>>> = Rc::default();
        let p = picked.clone();
        let (window, _) = testkit::open(cx, size(px(520.), px(80.)), move |_, cx| {
            crate::theme::apply(&NAVY, None, cx);
            cx.new(|_| Host(p))
        });
        cx.update_window(window, |_, window, cx| {
            window.click(ElementId::named_usize("death-chip", 1), cx);
            let w = W::new(1.0, 1440.0, cx);
            let none: OnPick = Rc::new(|_: u32, _: &mut Window, _: &mut App| {});
            assert!(chips(&deaths()[..1], Some(0), 0, &w, none.clone()).is_none());
            assert!(chips(&deaths()[..1], Some(0), 2, &w, none).is_some());
        })
        .unwrap();
        assert_eq!(*picked.borrow(), vec![1]);
    }
}
