//! The window's tests: gestures through Kit's harness over the daemon's
//! mock, as the overlay's are.

use std::cell::Cell;

use gpui_kit::test::TestWindowExt as _;
use gpui_kit::{
    AnyWindowHandle, AppContext, Background, ElementId, Entity, TestAppContext, px, size,
};
use wowdps_gui_logic::config::Config;
use wowdps_gui_logic::keys::Zoom;
use wowdps_gui_logic::raid::{raided, raided_deaths};
use wowdps_gui_logic::table::Col;
use wowdps_gui_logic::theme::{FROST, GOLD, PITCHES};
use wowdps_model::{Action, Screen, View};
use wowdps_proto::ClientState;

use super::{Gui, Place};
use crate::keys::{Do, ZoomTo};
use crate::session::Session;
use crate::testkit::{self, MockLink, NullLink};
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

fn press(cx: &mut TestAppContext, rig: &Rig, id: impl Into<ElementId>) {
    let id = id.into();
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

/// The window over a state built by hand (`wowdps_gui_logic::raid`), a
/// link that answers nothing beside it, so the state stays as built.
pub(crate) fn rig_over(cx: &mut TestAppContext, w: f32, h: f32, state: ClientState) -> Rig {
    own_config();
    cx.update(crate::keys::bind);
    let (window, gui) = testkit::open(cx, size(px(w), px(h)), |window, cx| {
        let session = cx.new(|_| Session::with_state(Box::new(NullLink), state));
        let gui = cx.new(|cx| Gui::new(session, config(), window, cx));
        let focus = gui.read(cx).focus().clone();
        window.focus(&focus, cx);
        gui
    });
    let session = gui.read_with(cx, |g, _| g.session().clone());
    Rig {
        window,
        gui,
        session,
    }
}

/// `/` hands the keys to the row filter, and while it holds them the
/// meter's keymap stands aside: "j" is a "j" in the field and moves
/// nobody. Esc clears the field and gives the keys back.
#[gpui_kit::test]
fn the_filter_keeps_the_meter_s_keys_while_it_has_them(cx: &mut TestAppContext) {
    let rig = rig(cx, 1440., 900.);
    let before = rig.session.read_with(cx, |s, _| s.state().row_sel);
    cx.update_window(rig.window, |_, window, cx| {
        window.render_frame(cx);
        window.press("/", cx);
        window.render_frame(cx);
        window.press("j", cx);
        window.render_frame(cx);
    })
    .unwrap();
    settle(cx, &rig.session);
    rig.gui.read_with(cx, |g, cx| {
        assert_eq!(
            g.filter.read(cx).value().as_ref(),
            "j",
            "typed into the field"
        );
        assert_eq!(g.filter_text, "j", "and the meter is filtered by it");
    });
    rig.session.read_with(cx, |s, _| {
        assert_eq!(s.state().row_sel, before, "nobody moved")
    });
    cx.update_window(rig.window, |_, window, cx| {
        window.press("escape", cx);
        window.render_frame(cx);
    })
    .unwrap();
    rig.gui.read_with(cx, |g, cx| {
        assert_eq!(g.filter.read(cx).value().as_ref(), "", "Esc cleared it");
        assert_eq!(g.filter_text, "");
    });
    cx.update_window(rig.window, |_, window, cx| {
        window.press("j", cx);
    })
    .unwrap();
    settle(cx, &rig.session);
    rig.session.read_with(cx, |s, _| {
        assert_eq!(s.state().row_sel, before + 1, "the meter's keys again")
    });
}

/// A view tab's press is its key: the view switches.
#[gpui_kit::test]
fn a_view_tab_switches_the_view(cx: &mut TestAppContext) {
    let rig = rig(cx, 1440., 900.);
    press(cx, &rig, super::tabs::tab_id(View::Healing));
    rig.session
        .read_with(cx, |s, _| assert_eq!(s.state().view, View::Healing));
    press(cx, &rig, super::tabs::tab_id(View::Deaths));
    rig.session
        .read_with(cx, |s, _| assert_eq!(s.state().view, View::Deaths));
}

/// A numeric heading sorts descending, then ascending, then gives the
/// daemon its order back; another heading starts over, descending.
#[gpui_kit::test]
fn a_heading_sorts_down_up_and_back(cx: &mut TestAppContext) {
    let rig = rig(cx, 1440., 900.);
    let head = |name: &'static str| ElementId::from((ElementId::Name("sort".into()), name));
    let sort = |cx: &mut TestAppContext| rig.gui.read_with(cx, |g, _| g.sort);
    press(cx, &rig, head("Amount"));
    assert_eq!(sort(cx), Some((Col::Amount, true)));
    press(cx, &rig, head("Amount"));
    assert_eq!(sort(cx), Some((Col::Amount, false)));
    press(cx, &rig, head("Amount"));
    assert_eq!(sort(cx), None);
    press(cx, &rig, head("Amount"));
    press(cx, &rig, head("Share"));
    assert_eq!(sort(cx), Some((Col::Pct, true)));
}

/// A row's press selects its player, and the inspector follows; in a
/// narrow window, where it is not beside the meter, the press pushes it.
#[gpui_kit::test]
fn a_row_s_press_selects_and_a_narrow_one_pushes(cx: &mut TestAppContext) {
    for (width, pushed) in [(1440., false), (460., true)] {
        let rig = rig(cx, width, 860.);
        let key = rig
            .session
            .read_with(cx, |s, _| s.state().rows().get(1).map(|r| r.key.clone()))
            .expect("the fixture's pull has two rows");
        press(cx, &rig, crate::meter::row_id(&key));
        rig.session.read_with(cx, |s, _| {
            assert_eq!(s.state().row_sel, 1, "{width}");
            assert_eq!(s.state().inspecting(), pushed, "{width}");
        });
    }
}

/// The "you" chip's press selects the owner's row — the row the daemon
/// marked `mine` — even when the filter hid it: the filter gives way,
/// the field's own text with it.
#[gpui_kit::test]
fn the_you_chip_selects_the_owner(cx: &mut TestAppContext) {
    let rig = rig_over(cx, 1440., 900., raided(25));
    cx.update_window(rig.window, |_, window, cx| {
        window.render_frame(cx);
        window.press("/", cx);
        window.render_frame(cx);
        for key in ["r", "a", "i", "d", "e", "r", "2"] {
            window.press(key, cx);
        }
        window.render_frame(cx);
    })
    .unwrap();
    rig.gui
        .read_with(cx, |g, _| assert_eq!(g.filter_text, "raider2"));
    press(cx, &rig, "you-chip");
    rig.session.read_with(cx, |s, _| {
        let state = s.state();
        let picked = state.rows().get(state.row_sel).map(|r| r.label.clone());
        assert_eq!(picked.as_deref(), Some("Raider16-Realm-US"));
    });
    rig.gui.read_with(cx, |g, cx| {
        assert_eq!(g.filter_text, "", "the filter gave way");
        assert_eq!(g.filter.read(cx).value().as_ref(), "", "the field too");
    });
}

/// A line of the Deaths table opens that death: the Deaths view drilled
/// into its window.
#[gpui_kit::test]
fn a_death_s_line_opens_its_recap(cx: &mut TestAppContext) {
    let rig = rig_over(cx, 1440., 900., raided_deaths(25));
    let line = ElementId::from((ElementId::Name("death".into()), "Player-1-5#0"));
    press(cx, &rig, line);
    rig.session.read_with(cx, |s, _| {
        let state = s.state();
        assert_eq!(state.view, View::Deaths);
        assert_eq!(
            state.drill.as_ref().map(|d| d.key.as_str()),
            Some("Player-1-5")
        );
        assert_eq!(state.death_request(), Some(0));
    });
}

/// The chrome budget (the iced window's `the_chrome_leaves_a_raid_its_rows`,
/// which this answers to): a 25-player Heroic kill at the prototype's wide
/// frame in the window's own fonts, its raid timeline in hand — the first
/// row starts no more than 290 px down (the ribbon's 86 px included), 18
/// rows show without a scroll, the total pins under the list flush with
/// the window's bottom, and the owner's chip is on the header, in their
/// role's place.
#[test]
fn the_chrome_leaves_a_raid_its_rows() {
    let mut cx = super::shots::app();
    own_config();
    let cfg = Config {
        hide_realms: true,
        ..config()
    };
    let at = size(px(1440.), px(900.));
    let (window, gui) = testkit::open_headless(&mut cx, at, |window, cx| {
        let session = cx.new(|_| Session::with_state(Box::new(NullLink), raided(25)));
        cx.new(|cx| Gui::new(session, cfg, window, cx))
    });
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        window.render_frame(cx);
        let list = window.find("meter-list").bounds();
        assert!(
            list.origin.y <= px(290.),
            "the first row starts {:?} down",
            list.origin.y
        );
        let rows = (list.size.height / px(PITCHES.row)).floor();
        assert!(rows >= 18., "{rows} rows show ({:?})", list.size.height);
        // The eighteenth row is whole inside the list's view.
        let eighteenth = window.find(crate::meter::row_id("Player-1-17")).bounds();
        assert!(
            eighteenth.bottom() <= list.bottom(),
            "{eighteenth:?} in {list:?}"
        );
        // The total pins under the list, flush with the window's bottom.
        let total = window.find("meter-total").bounds();
        assert!(
            (total.origin.y - list.bottom()).abs() < px(1.),
            "{total:?} under {list:?}"
        );
        assert!((total.bottom() - at.height).abs() < px(1.), "{total:?}");
        // The owner's chip is on the header, in their role's place.
        assert!(window.try_find("you-chip").is_some(), "the owner's chip");
        let w = super::w::W::new(1.0, 1440., cx);
        let head = super::fight_head::Head::of(gui.read(cx), &w, cx);
        let you = head.stats.and_then(|s| s.you).expect("the owner's chip");
        assert_eq!(you.name, "Raider16");
        assert!(you.words.ends_with(" dps"), "{}", you.words);
    })
    .unwrap();
}

/// Send `action` as its key would, `times` over.
fn keys(cx: &mut TestAppContext, rig: &Rig, action: Action, times: usize) {
    for _ in 0..times {
        cx.update_window(rig.window, |_, window, cx| {
            window.dispatch_action(Box::new(Do(action)), cx);
            window.render_frame(cx);
        })
        .unwrap();
    }
}

/// Past the fold the list follows the selection: twenty steps down a
/// 25-player raid and the twenty-first row is whole in sight.
#[gpui_kit::test]
fn the_meter_follows_a_step_past_its_fold(cx: &mut TestAppContext) {
    let rig = rig_over(cx, 1440., 700., raided(25));
    keys(cx, &rig, Action::Down, 20);
    rig.session
        .read_with(cx, |s, _| assert_eq!(s.state().row_sel, 20));
    cx.update_window(rig.window, |_, window, cx| {
        window.render_frame(cx);
        let list = window.find("meter-list").bounds();
        let row = window.find(crate::meter::row_id("Player-1-20")).bounds();
        assert!(
            row.top() >= list.top() && row.bottom() <= list.bottom(),
            "{row:?} in {list:?}"
        );
    })
    .unwrap();
}

/// Under a sort the keys walk the rows as drawn: ascending, the row above
/// the biggest is the second biggest, not the daemon's row above it.
#[gpui_kit::test]
fn a_sorted_meter_s_keys_walk_it_as_drawn(cx: &mut TestAppContext) {
    let rig = rig_over(cx, 1440., 900., raided(25));
    let head = ElementId::from((ElementId::Name("sort".into()), "Amount"));
    press(cx, &rig, head.clone());
    press(cx, &rig, head);
    rig.gui
        .read_with(cx, |g, _| assert_eq!(g.sort, Some((Col::Amount, false))));
    rig.session
        .read_with(cx, |s, _| assert_eq!(s.state().row_sel, 0));
    keys(cx, &rig, Action::Up, 1);
    rig.session
        .read_with(cx, |s, _| assert_eq!(s.state().row_sel, 1, "up the screen"));
}

/// On the Deaths view the keys walk the deaths in the order they happened,
/// each step that death's recap; a filter that hides every death leaves
/// them nowhere to go.
#[gpui_kit::test]
fn the_deaths_table_s_keys_walk_the_deaths(cx: &mut TestAppContext) {
    let rig = rig_over(cx, 1440., 900., raided_deaths(25));
    let drilled = |cx: &mut TestAppContext| {
        rig.session.read_with(cx, |s, _| {
            let state = s.state();
            (
                state.drill.as_ref().map(|d| d.key.clone()),
                state.death_request(),
            )
        })
    };
    keys(cx, &rig, Action::Down, 1);
    assert_eq!(drilled(cx), (Some("Player-1-5".into()), Some(0)));
    keys(cx, &rig, Action::Down, 1);
    assert_eq!(drilled(cx), (Some("Player-1-7".into()), Some(0)));
    keys(cx, &rig, Action::Up, 1);
    assert_eq!(drilled(cx), (Some("Player-1-5".into()), Some(0)));
    rig.gui
        .update(cx, |g, cx| g.set_filter("nobody".into(), cx));
    keys(cx, &rig, Action::Down, 1);
    assert_eq!(drilled(cx), (Some("Player-1-5".into()), Some(0)), "stayed");
}
