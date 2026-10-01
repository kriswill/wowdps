//! The command palette's gestures through Kit's harness: Ctrl K opens it
//! with its field holding the keys, typing narrows it, the arrows move the
//! selection and Enter runs it, a press runs a line, and Esc, Ctrl K or a
//! press off the card closes it.

use gpui_kit::test::TestWindowExt as _;
use gpui_kit::{AppContext as _, ElementId, TestAppContext};
use wowdps_gui_logic::palette::{Group, Run};
use wowdps_gui_logic::raid::raided;
use wowdps_model::View;

use crate::window::tests::{Rig, press, rig, rig_over, settle};

/// Type `keys` (GPUI keystrokes) into the window, a frame after each.
fn typing(cx: &mut TestAppContext, rig: &Rig, keys: &[&str]) {
    cx.update_window(rig.window, |_, window, cx| {
        window.render_frame(cx);
        for k in keys {
            window.press(k, cx);
            window.render_frame(cx);
        }
    })
    .unwrap();
    settle(cx, &rig.session);
}

fn open(cx: &mut TestAppContext, rig: &Rig) -> bool {
    rig.gui.read_with(cx, |g, _| g.pal.is_some())
}

/// What the card lists now, and the selection.
fn listed(cx: &mut TestAppContext, rig: &Rig) -> (Vec<(Group, String)>, usize) {
    rig.gui.read_with(cx, |g, cx| {
        let items = g.palette_items(cx);
        let sel = g.pal.as_ref().map_or(0, |p| p.p.sel);
        (items.into_iter().map(|i| (i.group, i.title)).collect(), sel)
    })
}

/// Ctrl K opens the card, its field holding the keys: a letter the meter
/// would answer is typed instead, and narrows the list; Ctrl K closes it.
#[gpui_kit::test]
fn ctrl_k_opens_it_and_the_field_has_the_keys(cx: &mut TestAppContext) {
    let rig = rig_over(cx, 1440., 900., raided(25));
    typing(cx, &rig, &["ctrl-k"]);
    assert!(open(cx, &rig), "Ctrl K opened it");
    let (all, _) = listed(cx, &rig);
    assert!(all.iter().any(|(g, _)| *g == Group::Views), "{all:?}");
    let sel = rig.session.read_with(cx, |s, _| s.state().row_sel);
    typing(cx, &rig, &["j", "k"]);
    rig.session.read_with(cx, |s, _| {
        assert_eq!(s.state().row_sel, sel, "the meter kept still")
    });
    rig.gui.read_with(cx, |g, _| {
        assert_eq!(g.pal.as_ref().map(|p| p.p.query.as_str()), Some("jk"));
    });
    typing(cx, &rig, &["ctrl-k"]);
    assert!(!open(cx, &rig), "Ctrl K again closed it");
}

/// "heal" narrows to the Healing view; Enter runs it and the card closes.
#[gpui_kit::test]
fn typing_narrows_and_enter_runs_the_selection(cx: &mut TestAppContext) {
    let rig = rig_over(cx, 1440., 900., raided(25));
    typing(cx, &rig, &["ctrl-k", "h", "e", "a", "l"]);
    let (shown, sel) = listed(cx, &rig);
    assert_eq!(
        shown.get(sel).map(|(_, t)| t.as_str()),
        Some("Healing"),
        "{shown:?}"
    );
    typing(cx, &rig, &["enter"]);
    assert!(!open(cx, &rig), "Enter closed it");
    rig.session
        .read_with(cx, |s, _| assert_eq!(s.state().view, View::Healing));
}

/// The arrows move the selection and stop at the ends; Esc closes the
/// card and gives the keys back to the meter.
#[gpui_kit::test]
fn the_arrows_step_and_esc_closes(cx: &mut TestAppContext) {
    let rig = rig_over(cx, 1440., 900., raided(25));
    typing(cx, &rig, &["ctrl-k", "down", "down"]);
    assert_eq!(listed(cx, &rig).1, 2);
    typing(cx, &rig, &["up", "up", "up"]);
    assert_eq!(listed(cx, &rig).1, 0, "stopped at the top");
    typing(cx, &rig, &["ctrl-n"]);
    assert_eq!(listed(cx, &rig).1, 1, "Ctrl N steps down");
    typing(cx, &rig, &["escape"]);
    assert!(!open(cx, &rig), "Esc closed it");
    let sel = rig.session.read_with(cx, |s, _| s.state().row_sel);
    typing(cx, &rig, &["j"]);
    rig.session.read_with(cx, |s, _| {
        assert_eq!(s.state().row_sel, sel + 1, "the meter's keys again")
    });
}

/// A press on a line runs it; a press on the card's own heading closes
/// nothing, and one on the scrim closes it.
#[gpui_kit::test]
fn a_press_runs_a_line_and_one_off_the_card_closes_it(cx: &mut TestAppContext) {
    let rig = rig_over(cx, 1440., 900., raided(25));
    typing(cx, &rig, &["ctrl-k", "t", "a", "k"]);
    let (shown, _) = listed(cx, &rig);
    let taken = shown
        .iter()
        .position(|(_, t)| t == "Taken")
        .expect("the Taken view is listed");
    // On the first group's heading, under the field (46 px) and its
    // hairline: the card's own, no line's.
    cx.update_window(rig.window, |_, window, cx| {
        window.render_frame(cx);
        let heading = gpui_kit::point(gpui_kit::px(20.), gpui_kit::px(60.));
        window.click_at("palette", heading, cx);
    })
    .unwrap();
    assert!(open(cx, &rig), "a press on the card is its own");
    press(
        cx,
        &rig,
        ElementId::NamedInteger("palette-item".into(), taken as u64),
    );
    assert!(!open(cx, &rig), "running a line closed it");
    rig.session
        .read_with(cx, |s, _| assert_eq!(s.state().view, View::Taken));
    typing(cx, &rig, &["ctrl-k"]);
    cx.update_window(rig.window, |_, window, cx| {
        window.render_frame(cx);
        // The scrim's top left, clear of the card hung 64 px down.
        let corner = gpui_kit::point(gpui_kit::px(4.), gpui_kit::px(4.));
        window.click_at("palette-scrim", corner, cx);
    })
    .unwrap();
    assert!(!open(cx, &rig), "a press off the card closed it");
}

/// A player's line selects them — on Damage when the view was a count
/// they have no row on — and the inspector follows.
#[gpui_kit::test]
fn a_player_runs_to_the_stage(cx: &mut TestAppContext) {
    let rig = rig_over(cx, 1440., 900., raided(25));
    rig.gui
        .update(cx, |g, cx| g.pick_view(View::Interrupts, cx));
    settle(cx, &rig.session);
    typing(
        cx,
        &rig,
        &["ctrl-k", "r", "a", "i", "d", "e", "r", "1", "2"],
    );
    let run = rig.gui.read_with(cx, |g, cx| {
        let items = g.palette_items(cx);
        let sel = g.pal.as_ref().map_or(0, |p| p.p.sel);
        items.get(sel).map(|i| i.run.clone())
    });
    assert!(matches!(run, Some(Run::Player { .. })), "{run:?}");
    typing(cx, &rig, &["enter"]);
    rig.session.read_with(cx, |s, _| {
        let state = s.state();
        assert_eq!(state.view, View::Damage);
        assert_eq!(
            state.drill.as_ref().map(|d| d.key.as_str()),
            Some("Player-1-12")
        );
    });
}

/// Nothing matches: the card says so, and Enter runs nothing.
#[gpui_kit::test]
fn nothing_matching_is_said_and_runs_nothing(cx: &mut TestAppContext) {
    let rig = rig(cx, 1440., 900.);
    typing(cx, &rig, &["ctrl-k", "z", "z", "z", "q"]);
    assert!(listed(cx, &rig).0.is_empty());
    typing(cx, &rig, &["enter"]);
    assert!(open(cx, &rig), "nothing ran, nothing closed");
}
