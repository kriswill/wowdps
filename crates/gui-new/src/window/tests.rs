//! The window's tests: gestures through Kit's harness over the daemon's
//! mock, as the overlay's are.

use std::cell::Cell;

use gpui_kit::test::TestWindowExt as _;
use gpui_kit::{AnyWindowHandle, AppContext, Background, Entity, TestAppContext, px, size};
use wowdps_gui_logic::config::Config;
use wowdps_gui_logic::keys::Zoom;
use wowdps_gui_logic::theme::{FROST, GOLD};
use wowdps_model::{Action, Screen};

use super::{Gui, Place};
use crate::keys::{Do, ZoomTo};
use crate::session::Session;
use crate::testkit::{self, MockLink};
use crate::theme::hsla;

thread_local! {
    static CONFIGS: Cell<u32> = const { Cell::new(0) };
}

/// A config file of the test's own, so a gesture that remembers a setting
/// never writes the user's.
pub(crate) fn own_config() -> std::path::PathBuf {
    let n = CONFIGS.with(|c| {
        c.set(c.get() + 1);
        c.get()
    });
    let dir = std::env::temp_dir().join(format!(
        "wowdps-gui-new-window-{}-{:?}-{n}",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = std::fs::create_dir_all(&dir);
    let path = dir.join("config.toml");
    Config::use_path_on_this_thread(Some(path.clone()));
    path
}

/// The window's config in a test: the defaults, Home not on start.
pub(crate) fn config() -> Config {
    Config {
        home_on_start: false,
        // Zoom 1: a window's pixels are its logical widths, as the design
        // shots take them.
        zoom: 1.0,
        ..Config::default()
    }
}

/// A window over the fixture's mock, with its session.
pub(crate) struct Rig {
    pub window: AnyWindowHandle,
    pub gui: Entity<Gui>,
    pub session: Entity<Session>,
}

/// Pump the session until nothing more arrives.
pub(crate) fn settle<C: AppContext>(cx: &mut C, session: &Entity<Session>) {
    for _ in 0..12 {
        if !cx.update_entity(session, |s, cx| s.pump(cx)) {
            break;
        }
    }
}

/// The window, `w` × `h`, over the committed fixture, settled.
pub(crate) fn rig(cx: &mut TestAppContext, w: f32, h: f32) -> Rig {
    own_config();
    cx.update(crate::keys::bind);
    let (window, gui) = testkit::open(cx, size(px(w), px(h)), |window, cx| {
        let session = cx.new(|_| Session::new(MockLink::fixture()));
        let gui = cx.new(|cx| Gui::new(session, config(), window, cx));
        let focus = gui.read(cx).focus().clone();
        window.focus(&focus, cx);
        gui
    });
    let session = gui.read_with(cx, |g, _| g.session().clone());
    settle(cx, &session);
    Rig {
        window,
        gui,
        session,
    }
}

fn press(cx: &mut TestAppContext, rig: &Rig, id: &'static str) {
    cx.update_window(rig.window, |_, window, cx| {
        window.render_frame(cx);
        window.click(id, cx);
    })
    .unwrap();
    settle(cx, &rig.session);
}

#[gpui_kit::test]
fn the_window_opens_on_the_log_s_newest_pull(cx: &mut TestAppContext) {
    let rig = rig(cx, 1440., 900.);
    rig.session.read_with(cx, |s, _| {
        assert_eq!(s.state().screen, Screen::Meter, "never the list screen");
        assert!(s.state().following_live());
    });
}

#[gpui_kit::test]
fn the_places_go_home_and_back_to_the_pull(cx: &mut TestAppContext) {
    let rig = rig(cx, 1440., 900.);
    press(cx, &rig, "place-home");
    rig.gui
        .read_with(cx, |g, _| assert_eq!(g.place, Place::Home));
    press(cx, &rig, "place-fights");
    rig.gui
        .read_with(cx, |g, _| assert_eq!(g.place, Place::Fights));
}

#[gpui_kit::test]
fn the_gear_and_help_open_their_cards(cx: &mut TestAppContext) {
    let rig = rig(cx, 1440., 900.);
    press(cx, &rig, "top-gear");
    press(cx, &rig, "top-help");
    rig.gui.read_with(cx, |g, _| {
        assert!(g.cards.options, "the gear opens the options card");
        assert!(g.cards.sheet, "help opens the keyboard sheet");
    });
}

#[gpui_kit::test]
fn the_live_pill_brings_the_live_pull_back(cx: &mut TestAppContext) {
    let rig = rig(cx, 1440., 900.);
    rig.session
        .update(cx, |s, cx| s.act(|st| st.apply(Action::OlderSegment), cx));
    settle(cx, &rig.session);
    rig.session
        .read_with(cx, |s, _| assert!(!s.state().following_live()));
    press(cx, &rig, "top-live");
    rig.session
        .read_with(cx, |s, _| assert!(s.state().following_live(), "pinned"));
}

/// Ctrl = steps the zoom in, Ctrl - out to its floor, Ctrl 0 back to the
/// default — each remembered by itself in the config file.
#[gpui_kit::test]
fn zoom_chords_step_clamp_and_remember(cx: &mut TestAppContext) {
    let rig = rig(cx, 1440., 900.);
    let zoom = |cx: &mut TestAppContext, z: Zoom| {
        cx.update_window(rig.window, |_, window, cx| {
            window.dispatch_action(Box::new(ZoomTo(z)), cx)
        })
        .unwrap();
    };
    zoom(cx, Zoom::In);
    let z = rig.gui.read_with(cx, |g, _| g.cfg.zoom);
    assert!((z - 1.1).abs() < 1e-5, "{z}");
    assert!((Config::load().zoom - 1.1).abs() < 1e-5, "remembered");
    for _ in 0..40 {
        zoom(cx, Zoom::Out);
    }
    rig.gui.read_with(cx, |g, _| assert_eq!(g.cfg.zoom, 0.5));
    zoom(cx, Zoom::Reset);
    rig.gui
        .read_with(cx, |g, _| assert_eq!(g.cfg.zoom, Config::default().zoom));
}

/// `t` opens the talent viewer on the selected player — asking the daemon
/// for the build they wore — and its Esc gives the window back.
#[gpui_kit::test]
fn t_opens_the_talent_viewer_and_esc_closes_it(cx: &mut TestAppContext) {
    let rig = rig(cx, 1440., 900.);
    cx.update_window(rig.window, |_, window, cx| {
        window.render_frame(cx);
        window.press("t", cx);
    })
    .unwrap();
    rig.gui.read_with(cx, |g, _| {
        assert!(g.talents.is_some(), "the viewer is open")
    });
    // The meter's keys are the viewer's now: `j` moves nobody.
    let before = rig.session.read_with(cx, |s, _| s.state().row_sel);
    cx.update_window(rig.window, |_, window, cx| {
        window.render_frame(cx);
        window.press("j", cx);
        window.press("escape", cx);
        window.render_frame(cx);
    })
    .unwrap();
    rig.gui
        .read_with(cx, |g, _| assert!(g.talents.is_none(), "Esc closed it"));
    rig.session
        .read_with(cx, |s, _| assert_eq!(s.state().row_sel, before));
    // And the keys are the meter's again.
    cx.update_window(rig.window, |_, window, cx| {
        window.press("j", cx);
    })
    .unwrap();
    settle(cx, &rig.session);
    rig.session
        .read_with(cx, |s, _| assert_eq!(s.state().row_sel, before + 1));
}

/// A key of the shared keymap reaches the state through `Do`.
#[gpui_kit::test]
fn the_shared_keymap_reaches_the_state(cx: &mut TestAppContext) {
    let rig = rig(cx, 1440., 900.);
    cx.update_window(rig.window, |_, window, cx| {
        window.dispatch_action(
            Box::new(Do(Action::SetView(wowdps_model::View::Healing))),
            cx,
        )
    })
    .unwrap();
    settle(cx, &rig.session);
    rig.session.read_with(cx, |s, _| {
        assert_eq!(s.state().view, wowdps_model::View::Healing)
    });
}

/// The rail docks beside the stage only above 1180 px: a tile's is a
/// drawer.
#[gpui_kit::test]
fn the_rail_docks_above_1180_only(cx: &mut TestAppContext) {
    for (width, docked) in [(1440., true), (1181., true), (1180., false), (460., false)] {
        let rig = rig(cx, width, 800.);
        cx.update_window(rig.window, |_, window, cx| {
            window.render_frame(cx);
            assert_eq!(window.try_find("rail").is_some(), docked, "{width}");
        })
        .unwrap();
    }
}

/// One theme definition repaints the window: the top bar's surface is
/// gold's, then frost's, and never both.
#[gpui_kit::test]
fn a_theme_switch_repaints_the_window(cx: &mut TestAppContext) {
    let rig = rig(cx, 1440., 900.);
    let surfaces = |cx: &mut TestAppContext| {
        cx.update_window(rig.window, |_, window, cx| {
            window.render_frame(cx);
            let quads = window.painted_quads();
            let has = |c| {
                quads
                    .iter()
                    .any(|q| q.background == Background::from(hsla(c)))
            };
            (has(GOLD.window.surface), has(FROST.window.surface))
        })
        .unwrap()
    };
    cx.update(|cx| crate::theme::apply(&GOLD, None, cx));
    assert_eq!(surfaces(cx), (true, false));
    cx.update(|cx| crate::theme::apply(&FROST, None, cx));
    assert_eq!(surfaces(cx), (false, true));
}
