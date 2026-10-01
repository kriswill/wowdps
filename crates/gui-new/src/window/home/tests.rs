//! Home through Kit's harness, over the fixture's mock with its fights
//! stored: the week it derives and the panels it draws, a scope chip
//! remembered, its jump points, and Home at launch.

use gpui_kit::{AppContext as _, ElementId, TestAppContext};
use wowdps_gui_logic::config::Config;
use wowdps_gui_logic::rail::Pull;
use wowdps_proto::HistoryStatus;

use super::super::Place;
use super::super::rail::tests::{OWNER, key, press, rig_over, rig_with, shown, stored_mock};
use super::super::tests::{Rig, config, settle};
use crate::testkit::MockLink;

/// The fixture's first player: the stored cards' owner.
const OWNER_GUID: &str = "Player-1168-0A1B2C01";

fn home(cx: &mut TestAppContext) -> Rig {
    let rig = rig_over(cx, MockLink::new(stored_mock()), 1440., 900.);
    press(cx, &rig, "place-home");
    rig
}

#[gpui_kit::test]
fn home_derives_its_week_and_panels(cx: &mut TestAppContext) {
    let rig = home(cx);
    rig.gui.read_with(cx, |g, _| {
        assert_eq!(g.place, Place::Home);
        let home = g.hist.store.home.as_ref().expect("Home is up");
        assert!(home.answered && !home.cards.is_empty(), "the week is read");
        let panels = &g.hist.store.panels;
        let owner = panels.owner.as_ref().expect("the store names the owner");
        assert_eq!(owner.name, OWNER);
        let night = panels.night.as_ref().expect("the owner's night");
        assert!(!night.pulls.is_empty());
        assert_eq!(panels.raids.len(), 1, "one raid at one difficulty");
        assert!(
            g.hist.known.iter().any(|c| c.guid == OWNER_GUID),
            "the owner is remembered for the chips"
        );
    });
    for id in ["home", "home-night", "home-keys", "home-trend"] {
        assert!(shown(cx, &rig, id), "{id}");
    }
    assert!(shown(cx, &rig, ("home-raid", 0usize)));
    assert!(shown(cx, &rig, ("tile", 0usize)), "a tile per pull");
    assert!(shown(cx, &rig, ("slope-dot", 0usize)), "a dot per pull");
    assert!(shown(cx, &rig, ("boss", 0usize)), "a row per boss");
    assert!(shown(cx, &rig, ("boss-dot", 0usize)), "a dot per pull");
    // The picker names the character played last, as the store does.
    rig.gui.read_with(cx, |g, cx| {
        assert_eq!(g.picked(cx).map(|p| p.guid).as_deref(), Some(OWNER_GUID));
    });
}

#[gpui_kit::test]
fn a_scope_chip_is_remembered(cx: &mut TestAppContext) {
    let rig = home(cx);
    let chip = ElementId::Name(format!("scope-{OWNER_GUID}").into());
    assert!(shown(cx, &rig, "scope-all"));
    assert!(shown(cx, &rig, chip.clone()));
    press(cx, &rig, chip);
    rig.gui.read_with(cx, |g, _| {
        assert_eq!(g.cfg.character.as_deref(), Some(OWNER_GUID));
        let home = g.hist.store.home.as_ref().expect("Home is up");
        assert_eq!(home.scope.as_deref(), Some(OWNER_GUID));
    });
    assert_eq!(
        Config::load().character.as_deref(),
        Some(OWNER_GUID),
        "the scope Home opens on next time"
    );
    // Leaving Home and coming back lands on the same scope.
    press(cx, &rig, "place-fights");
    press(cx, &rig, "place-home");
    rig.gui.read_with(cx, |g, _| {
        let home = g.hist.store.home.as_ref().expect("Home is up");
        assert_eq!(home.scope.as_deref(), Some(OWNER_GUID));
    });
    press(cx, &rig, "scope-all");
    assert_eq!(Config::load().character, None);
}

#[gpui_kit::test]
fn home_s_jump_points_open_their_pulls(cx: &mut TestAppContext) {
    let rig = home(cx);
    press(cx, &rig, ("tile", 0usize));
    rig.gui.read_with(cx, |g, cx| {
        assert_eq!(g.place, Place::Fights, "Home stands aside");
        assert!(g.hist.store.home.is_none(), "its paging stops");
        // A stored card of the tailed log opens as the log's pull.
        assert!(matches!(g.current_pull(cx), Some(Pull::Log(_))));
    });
    press(cx, &rig, "place-home");
    press(cx, &rig, ("boss", 0usize));
    rig.gui
        .read_with(cx, |g, _| assert_eq!(g.place, Place::Fights));
    press(cx, &rig, "place-home");
    press(cx, &rig, ("slope-dot", 0usize));
    rig.gui
        .read_with(cx, |g, _| assert_eq!(g.place, Place::Fights));
}

#[gpui_kit::test]
fn home_keeps_the_meter_s_keys_and_brackets_start_the_rail(cx: &mut TestAppContext) {
    let rig = home(cx);
    let row = rig.session.read_with(cx, |s, _| s.state().row_sel);
    key(cx, &rig, wowdps_model::Action::Down);
    rig.session.read_with(cx, |s, _| {
        assert_eq!(s.state().row_sel, row, "j is not Home's")
    });
    rig.gui
        .read_with(cx, |g, _| assert_eq!(g.place, Place::Home));
    key(cx, &rig, wowdps_model::Action::OlderSegment);
    rig.gui.read_with(cx, |g, cx| {
        assert_eq!(g.place, Place::Fights, "`[` from Home opens the rail's top");
        assert!(g.current_pull(cx).is_some());
    });
}

#[gpui_kit::test]
fn home_opens_at_launch_when_nothing_is_live(cx: &mut TestAppContext) {
    let cfg = Config {
        home_on_start: true,
        ..config()
    };
    let rig = rig_with(cx, MockLink::new(stored_mock()), cfg, 1440., 900.);
    settle(cx, &rig.session);
    rig.gui.read_with(cx, |g, _| {
        assert_eq!(g.place, Place::Home);
        assert!(g.hist.store.home.is_some());
    });
}

#[gpui_kit::test]
fn a_store_that_is_off_says_so_and_draws_no_panels(cx: &mut TestAppContext) {
    let rig = home(cx);
    rig.gui.update(cx, |g, cx| {
        g.hist.store.on_status(&HistoryStatus {
            enabled: false,
            error: Some("no data dir".to_string()),
            ..HistoryStatus::default()
        });
        cx.notify();
    });
    assert!(shown(cx, &rig, "home-night"), "the night card says why");
    assert!(
        !shown(cx, &rig, "home-keys"),
        "no panel says none of nothing"
    );
}

#[gpui_kit::test]
fn home_lays_its_panels_out_in_one_column_narrow(cx: &mut TestAppContext) {
    let rig = rig_over(cx, MockLink::new(stored_mock()), 460., 860.);
    press(cx, &rig, "place-home");
    for id in ["home-night", "home-keys", "home-trend"] {
        assert!(shown(cx, &rig, id), "{id}");
    }
    let x = |cx: &mut TestAppContext, id: &'static str| {
        cx.update_window(rig.window, |_, window, cx| {
            use gpui_kit::test::TestWindowExt as _;
            window.render_frame(cx);
            window.find(id).bounds().origin.x
        })
        .unwrap()
    };
    assert_eq!(x(cx, "home-keys"), x(cx, "home-trend"), "one column");
}
