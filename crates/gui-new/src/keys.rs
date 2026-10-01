//! The keymap on GPUI (spec §6, Keys): gui-logic's chord tables become
//! `KeyBinding`s registered once at start. The meter's actions ride in one
//! GPUI action, `Do`, carrying the model's `Action` as data, in the
//! `Meter` key context; the zoom chords in `ZoomTo`, window-wide. The
//! window-local gestures (`t`, `p`, Ctrl K, `/`, `m`, `~`, `H`, `?`) get
//! actions of their own with the screens that answer them.
//!
//! Spike S11: a typed character binds as ITSELF. On Linux a shift-/ arrives
//! as key `/` with key_char `?` and Shift held, and GPUI's matcher compares
//! the key_char with Ctrl and the platform key alone, so `?`, `~` and `+`
//! bind as written and `ctrl-+` fires on Ctrl+Shift+=. A capital letter
//! (`K`) parses as Shift plus the letter, which a typed Shift+K matches.

use gpui_kit::{App, KeyBinding};
use wowdps_gui_logic::keys::{ACTIONS, Chord, Named, ZOOM_CHORDS, Zoom, zoom_for};
use wowdps_model::Action;

/// A meter action, as GPUI dispatches it.
#[derive(Clone, Debug, PartialEq, gpui_kit::Action)]
#[action(namespace = wowdps, no_json)]
pub struct Do(pub Action);

/// A zoom step, window-wide.
#[derive(Clone, Debug, PartialEq, gpui_kit::Action)]
#[action(namespace = wowdps, no_json)]
pub struct ZoomTo(pub Zoom);

/// The key context the meter's bindings live in.
pub const METER: &str = "Meter";

/// Where the meter's bindings fire: in the meter's context, never while a
/// text field (Kit's `Input`, whose context is "Input") has the keys — so
/// typing "j" into the row filter types a "j" and moves no selection.
/// GPUI's `!` reads the WHOLE context stack, not the prefix a binding
/// matched at, which is what makes this hold for a field inside the meter.
pub const METER_KEYS: &str = "Meter && !Input";

/// The key context of a window whose menu is up (the `?` sheet, the ⚙
/// card, the character menu): no meter binding fires there, and any key
/// closes the menu — but Ctrl K, which opens the palette over it.
pub const MODAL: &str = "Modal";

/// A window-local gesture: a key the window answers itself, never the
/// shared keymap's (`keys::BINDINGS` marks them `window_local`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Gesture {
    /// `t`: the talent viewer on the selected player.
    Talents,
    /// `p`: pin the pull on the stage, or let it go.
    Pin,
    /// Ctrl K: the command palette.
    Jump,
    /// `/`: the row filter.
    Filter,
    /// `m`: the live pull.
    Live,
    /// `~`: Home.
    Home,
    /// `H`: the rail at the earlier nights.
    Earlier,
    /// `?`: the keyboard sheet.
    Sheet,
}

/// A window-local gesture, as GPUI dispatches it.
#[derive(Clone, Debug, PartialEq, gpui_kit::Action)]
#[action(namespace = wowdps, no_json)]
pub struct Go(pub Gesture);

/// The window-local gestures' chords, as typed (spike S11).
pub const GESTURES: [(&str, Gesture); 8] = [
    ("t", Gesture::Talents),
    ("p", Gesture::Pin),
    ("ctrl-k", Gesture::Jump),
    ("/", Gesture::Filter),
    ("m", Gesture::Live),
    ("~", Gesture::Home),
    ("H", Gesture::Earlier),
    ("?", Gesture::Sheet),
];

/// GPUI's keystroke string for a chord.
pub fn keystroke(chord: Chord<'_>) -> String {
    match chord {
        Chord::Char(c) => c.to_string(),
        Chord::Ctrl(c) => format!("ctrl-{c}"),
        Chord::Named(named) => match named {
            Named::ArrowDown => "down",
            Named::ArrowUp => "up",
            Named::ArrowLeft => "left",
            Named::ArrowRight => "right",
            Named::Enter => "enter",
            Named::Escape => "escape",
            Named::Tab => "tab",
        }
        .to_string(),
    }
}

/// Every binding the tables define.
pub fn bindings() -> Vec<KeyBinding> {
    let meter = ACTIONS
        .iter()
        .map(|&(chord, action)| KeyBinding::new(&keystroke(chord), Do(action), Some(METER_KEYS)));
    let local = GESTURES
        .iter()
        .map(|&(chord, gesture)| KeyBinding::new(chord, Go(gesture), Some(METER_KEYS)));
    let zoom = ZOOM_CHORDS.iter().filter_map(|&chord| {
        zoom_for(chord).map(|zoom| KeyBinding::new(&keystroke(chord), ZoomTo(zoom), None))
    });
    let over_menu = std::iter::once(KeyBinding::new("ctrl-k", Go(Gesture::Jump), Some(MODAL)));
    meter.chain(local).chain(zoom).chain(over_menu).collect()
}

/// Register the keymap — the meter's, the window's own and the talent
/// viewer's. Call once, before the first window opens.
pub fn bind(cx: &mut App) {
    cx.bind_keys(bindings());
    cx.bind_keys(crate::talents::bindings());
    cx.bind_keys(crate::window::bindings());
}

#[cfg(test)]
mod tests {
    use gpui_kit::prelude::*;
    use gpui_kit::test::TestWindowExt as _;
    use gpui_kit::{
        Context, FocusHandle, TestAppContext, TestSupportExt as _, Window, div, px, size,
    };
    use wowdps_gui_logic::keys::{ACTIONS, Chord, ZOOM_CHORDS, zoom_for};
    use wowdps_model::Action;

    use super::{Do, METER, ZoomTo, bind};
    use crate::testkit;

    /// A focused element in the meter's context that records what fires.
    struct Recorder {
        focus: FocusHandle,
        actions: Vec<Action>,
        zooms: Vec<wowdps_gui_logic::keys::Zoom>,
    }

    impl Render for Recorder {
        fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            div()
                .id("recorder")
                .test_support()
                .track_focus(&self.focus)
                .key_context(METER)
                .size_full()
                .on_action(cx.listener(|this, Do(action): &Do, _, _| this.actions.push(*action)))
                .on_action(cx.listener(|this, ZoomTo(zoom): &ZoomTo, _, _| this.zooms.push(*zoom)))
        }
    }

    /// The keystroke Linux delivers for a chord, in GPUI's test syntax
    /// (`key->key_char` when the two differ): a capital letter is Shift plus
    /// the lowercase key, and the shifted punctuation the tables could hold
    /// arrives as its unshifted key with the character beside it.
    fn typed(chord: Chord<'_>) -> String {
        let shifted = |c: &str| match c {
            "?" => Some("/"),
            "~" => Some("`"),
            "+" => Some("="),
            _ => None,
        };
        match chord {
            Chord::Char(c) if c.len() == 1 && c.chars().all(|ch| ch.is_ascii_uppercase()) => {
                format!("shift-{}->{c}", c.to_ascii_lowercase())
            }
            Chord::Char(c) => match shifted(c) {
                Some(base) => format!("shift-{base}->{c}"),
                None => c.to_string(),
            },
            Chord::Ctrl(c) => match shifted(c) {
                Some(base) => format!("ctrl-shift-{base}->{c}"),
                None => format!("ctrl-{c}"),
            },
            named => super::keystroke(named),
        }
    }

    /// Spike S11: every chord in gui-logic's tables, typed as Linux types
    /// it, fires its action through GPUI's own dispatch — Shift+K is
    /// Deaths, Ctrl+Shift+= zooms in like Ctrl+=.
    #[gpui_kit::test]
    fn every_chord_fires_its_action_as_linux_types_it(cx: &mut TestAppContext) {
        let (window, recorder) = testkit::open(cx, size(px(200.), px(100.)), |window, cx| {
            let focus = cx.focus_handle();
            window.focus(&focus, cx);
            cx.new(|_| Recorder {
                focus,
                actions: Vec::new(),
                zooms: Vec::new(),
            })
        });
        cx.update(bind);
        cx.update_window(window, |_, window, cx| {
            window.render_frame(cx);
            for &(chord, _) in ACTIONS {
                window.press(&typed(chord), cx);
            }
            for &chord in &ZOOM_CHORDS {
                window.press(&typed(chord), cx);
            }
        })
        .unwrap();
        recorder.read_with(cx, |recorder, _| {
            let want: Vec<Action> = ACTIONS.iter().map(|&(_, a)| a).collect();
            assert_eq!(recorder.actions, want);
            let zooms: Vec<_> = ZOOM_CHORDS.iter().filter_map(|c| zoom_for(*c)).collect();
            assert_eq!(recorder.zooms, zooms);
        });
    }

    /// The meter's context holding a text field, as the window's view tabs
    /// hold the row filter.
    struct Field {
        focus: FocusHandle,
        input: gpui_kit::Entity<gpui_kit::component::input::InputState>,
        actions: Vec<Action>,
    }

    impl Render for Field {
        fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            div()
                .id("meter")
                .track_focus(&self.focus)
                .key_context(METER)
                .size_full()
                .on_action(cx.listener(|this, Do(action): &Do, _, _| this.actions.push(*action)))
                .child(gpui_kit::component::input::Input::new(&self.input))
        }
    }

    /// While a text field has the keys, the meter's bindings stand aside:
    /// "j" typed into the filter is a "j" in the field and moves nobody —
    /// and with the keys back on the meter, it is the meter's again.
    #[gpui_kit::test]
    fn a_text_field_keeps_the_meter_s_keys_out(cx: &mut TestAppContext) {
        let (window, field) = testkit::open(cx, size(px(300.), px(100.)), |window, cx| {
            let input = cx.new(|cx| gpui_kit::component::input::InputState::new(window, cx));
            let focus = cx.focus_handle();
            input.update(cx, |s, cx| s.focus(window, cx));
            cx.new(|_| Field {
                focus,
                input,
                actions: Vec::new(),
            })
        });
        cx.update(bind);
        cx.update_window(window, |_, window, cx| {
            window.render_frame(cx);
            window.press("j", cx);
            window.press("k", cx);
        })
        .unwrap();
        field.read_with(cx, |f, cx| {
            assert_eq!(
                f.input.read(cx).value().as_ref(),
                "jk",
                "typed into the field"
            );
            assert!(f.actions.is_empty(), "the meter heard {:?}", f.actions);
        });
        cx.update_window(window, |_, window, cx| {
            let focus = field.read(cx).focus.clone();
            window.focus(&focus, cx);
            window.render_frame(cx);
            window.press("j", cx);
        })
        .unwrap();
        field.read_with(cx, |f, _| assert_eq!(f.actions, vec![Action::Down]));
    }
}
