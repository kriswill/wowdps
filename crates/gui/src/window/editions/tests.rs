//! The edition switch, through the window: over a daemon of one edition
//! nothing of it is drawn or asked; over two, it scopes the meter, the
//! rail, Home and the picker, and remembers each edition's own keys.

use gpui_kit::test::TestWindowExt as _;
use gpui_kit::{AppContext, ElementId, TestAppContext};
use wowdps_daemon::mock::MockDaemon;
use wowdps_gui_logic::config::Config;
use wowdps_gui_logic::editions::FOLLOW;
use wowdps_model::{Edition, Screen};
use wowdps_proto::{ClientMsg, EditionRef, HistoryQuery};

use super::super::Place;
use super::super::tests::{Rig, press, rig_on, settle};
use crate::testkit::{EditionsLink, MockLink};

/// The fixtures' characters: Retail's and Forever's, each the owner of its
/// edition's stored fights.
const RETAIL_ME: &str = "Thraxx-Nebula-US";
const FOREVER_ME: &str = "Velkris-Nebula-US";

fn shown(cx: &mut TestAppContext, rig: &Rig, id: impl Into<ElementId>) -> bool {
    let id = id.into();
    cx.update_window(rig.window, |_, window, cx| {
        window.render_frame(cx);
        window.try_find(id).is_some()
    })
    .unwrap()
}

/// Press the switch, then one of its entries.
fn pick(cx: &mut TestAppContext, rig: &Rig, entry: &str) {
    press(cx, rig, "top-edition");
    assert!(shown(cx, rig, "edition-menu"), "the menu opens");
    press(cx, rig, ElementId::Name(entry.to_string().into()));
    settle(cx, &rig.session);
}

/// Every edition the rail's cards are of.
fn rail_editions(cx: &mut TestAppContext, rig: &Rig) -> Vec<Edition> {
    rig.gui.read_with(cx, |g, _| {
        let mut e: Vec<Edition> = g
            .hist
            .store
            .earlier
            .cards
            .iter()
            .map(|c| c.edition().counts_as())
            .collect();
        e.dedup();
        e
    })
}

fn two(cx: &mut TestAppContext) -> (Rig, EditionsLink) {
    let link = EditionsLink::owned(&[RETAIL_ME, FOREVER_ME]);
    let sent = std::rc::Rc::clone(&link.sent);
    let rig = rig_on(cx, 1440., 900., link);
    // The sent log, through a link of its own: the rig owns the real one.
    let mut probe = EditionsLink::new();
    probe.sent = sent;
    (rig, probe)
}

/// A daemon of one edition: no switch, and every history question the
/// window asks is the one it always asked — every edition's — but for the
/// census's own one-card probes.
#[gpui_kit::test]
fn one_edition_draws_no_switch_and_asks_as_before(cx: &mut TestAppContext) {
    let sent = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
    struct Spy(MockLink, std::rc::Rc<std::cell::RefCell<Vec<ClientMsg>>>);
    impl crate::session::Link for Spy {
        fn send(&mut self, msg: &ClientMsg) {
            self.1.borrow_mut().push(msg.clone());
            self.0.send(msg);
        }
        fn poll(&mut self) -> Vec<wowdps_proto::DaemonMsg> {
            self.0.poll()
        }
        fn reconnect(&mut self) -> wowdps_proto::Reconnect {
            self.0.reconnect()
        }
    }
    let link = Spy(
        MockLink::new(MockDaemon::fixture().with_history()),
        std::rc::Rc::clone(&sent),
    );
    let rig = rig_on(cx, 1440., 900., link);
    press(cx, &rig, "place-home");
    settle(cx, &rig.session);
    assert!(
        !shown(cx, &rig, "top-edition"),
        "no switch with one edition"
    );
    rig.gui.read_with(cx, |g, _| {
        assert_eq!(g.hist.store.census.known(), vec![Edition::Retail]);
        assert_eq!(g.hist.store.earlier.edition(), None);
        assert_eq!(g.hist.store.home.as_ref().and_then(|h| h.edition), None);
        assert_eq!(g.edition, EditionRef::Follow);
    });
    for msg in sent.borrow().iter() {
        if let ClientMsg::GetHistory {
            query: HistoryQuery::Fights { edition, limit, .. },
            ..
        } = msg
            && *limit > 1
        {
            assert_eq!(*edition, None, "a page asks for every edition: {msg:?}");
        }
        if let ClientMsg::Watch { edition, .. } = msg {
            assert_eq!(*edition, EditionRef::Follow, "the meter follows");
        }
    }
}

/// Two editions' fights in the store: the switch stands beside the
/// places, named by the live game ("Midnight"), and the rail lists the
/// edition the window is in alone. Pinning Forever moves the meter, the
/// rail and the switch's face; Follow brings Retail back.
#[gpui_kit::test]
fn the_switch_scopes_the_meter_and_the_rail(cx: &mut TestAppContext) {
    let (rig, sent) = two(cx);
    assert!(shown(cx, &rig, "top-edition"), "the switch is drawn");
    rig.gui.read_with(cx, |g, cx| {
        assert!(g.hist.store.census.several());
        assert_eq!(g.hist.store.earlier.edition(), Some(Edition::Retail));
        assert_eq!(g.edition_face(cx).name, "Midnight");
        assert!(g.edition_face(cx).following);
    });
    assert_eq!(rail_editions(cx, &rig), vec![Edition::Retail]);

    pick(cx, &rig, "edition-forever");
    rig.gui.read_with(cx, |g, cx| {
        assert!(!g.cards.edition, "the menu closes");
        assert_eq!(g.edition, EditionRef::Pin(Edition::Forever));
        assert_eq!(g.hist.edition, Edition::Forever);
        let st = g.session.read(cx).state();
        assert_eq!(st.edition_ref(), EditionRef::Pin(Edition::Forever));
        assert_eq!(st.edition(), Some(Edition::Forever));
        // No source of Forever's is followed: its live meter is empty.
        assert_eq!(st.segment_count(), 0);
        assert_eq!(g.hist.store.earlier.edition(), Some(Edition::Forever));
        assert_eq!(g.edition_face(cx).name, "Forever");
        assert!(!g.edition_face(cx).following);
    });
    assert_eq!(rail_editions(cx, &rig), vec![Edition::Forever]);
    assert!(
        sent.sent.borrow().iter().any(|m| matches!(
            m,
            ClientMsg::Watch {
                edition: EditionRef::Pin(Edition::Forever),
                ..
            }
        )),
        "the meter's Watch pins Forever"
    );

    pick(cx, &rig, "edition-follow");
    rig.gui.read_with(cx, |g, cx| {
        assert_eq!(g.edition, EditionRef::Follow);
        assert_eq!(g.hist.edition, Edition::Retail);
        let st = g.session.read(cx).state();
        assert_eq!(st.edition_ref(), EditionRef::Follow);
        assert_eq!(st.screen, Screen::Meter, "back on the log's newest pull");
    });
    assert_eq!(rail_editions(cx, &rig), vec![Edition::Retail]);
}

/// The menu lists Follow, then each edition by its name, the choice lit;
/// it is modal: any key closes it.
#[gpui_kit::test]
fn the_edition_menu_lists_follow_and_each_edition(cx: &mut TestAppContext) {
    let (rig, _) = two(cx);
    press(cx, &rig, "top-edition");
    for id in ["edition-follow", "edition-retail", "edition-forever"] {
        assert!(shown(cx, &rig, id), "{id} is listed");
    }
    rig.gui.read_with(cx, |g, cx| {
        let e = g.edition_entries(cx).expect("entries");
        let names: Vec<&str> = e.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, vec![FOLLOW, "Midnight", "Forever"]);
        assert!(e[0].on, "Follow is the default");
        assert!(g.modal(), "the menu holds the keys");
    });
    cx.update_window(rig.window, |_, window, cx| {
        window.press("x", cx);
        window.render_frame(cx);
    })
    .unwrap();
    rig.gui
        .read_with(cx, |g, _| assert!(!g.cards.edition, "any key closes it"));
    // The scrim's press closes it too.
    press(cx, &rig, "top-edition");
    press(cx, &rig, "edition-scrim");
    rig.gui.read_with(cx, |g, _| assert!(!g.cards.edition));
}

/// Home in Forever's window reads Forever's fights, offers Forever's
/// characters alone, and a pick there is Forever's own key — Retail's
/// `character` untouched, and Retail's pick back when the window is.
#[gpui_kit::test]
fn each_edition_keeps_its_own_character(cx: &mut TestAppContext) {
    let (rig, _) = two(cx);
    press(cx, &rig, "place-home");
    settle(cx, &rig.session);
    let retail_chars = rig.gui.read_with(cx, |g, _| {
        g.hist
            .known()
            .iter()
            .map(|c| c.name.clone())
            .collect::<Vec<_>>()
    });
    assert_eq!(retail_chars, vec![RETAIL_ME.to_string()]);

    pick(cx, &rig, "edition-forever");
    rig.gui.read_with(cx, |g, _| {
        assert_eq!(g.place, Place::Home, "Home stays up, scoped anew");
        let home = g.hist.store.home.as_ref().expect("Home is up");
        assert_eq!(home.edition, Some(Edition::Forever));
        assert!(home.answered && !home.cards.is_empty());
        assert!(
            home.cards.iter().all(|c| c.edition() == Edition::Forever),
            "Forever's fights alone"
        );
        let names: Vec<&str> = g.hist.known().iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, vec![FOREVER_ME], "Forever's characters alone");
    });
    let guid = rig
        .gui
        .read_with(cx, |g, _| g.hist.known().first().map(|c| c.guid.clone()))
        .expect("Forever's character");
    rig.gui
        .update(cx, |g, cx| g.scope_home(Some(guid.clone()), cx));
    settle(cx, &rig.session);
    // The test's own config file: `own_config`'s, which the rig set.
    let disk = Config::load();
    assert_eq!(disk.character_in(Edition::Forever), Some(guid.as_str()));
    assert_eq!(disk.character, None, "Retail's key untouched");
    rig.gui.read_with(cx, |g, cx| {
        assert_eq!(
            g.picked(cx).map(|p| p.name).as_deref(),
            Some(FOREVER_ME),
            "the picker names Forever's pick"
        );
    });

    pick(cx, &rig, "edition-retail");
    rig.gui.read_with(cx, |g, cx| {
        let home = g.hist.store.home.as_ref().expect("Home is up");
        assert_eq!(home.edition, Some(Edition::Retail));
        assert_eq!(home.scope, None, "Retail's own scope: every character");
        assert_ne!(
            g.picked(cx).map(|p| p.name).as_deref(),
            Some(FOREVER_ME),
            "Forever's pick is Forever's"
        );
    });
}

/// A Retail fight on the stage of Forever's window says whose it is.
#[gpui_kit::test]
fn a_retail_fight_in_forever_s_window_names_its_expansion(cx: &mut TestAppContext) {
    let (rig, _) = two(cx);
    let retail_kill = rig.gui.read_with(cx, |g, _| {
        g.hist
            .store
            .earlier
            .cards
            .iter()
            .find(|c| c.edition() == Edition::Retail && c.success == Some(true))
            .cloned()
            .expect("a Retail kill on the rail")
    });
    pick(cx, &rig, "edition-forever");
    rig.gui.update(cx, |g, cx| {
        g.hist.store.earlier.adopt(retail_kill.clone());
        let sent =
            g.hist
                .store
                .open_stored(retail_kill.id.clone(), wowdps_model::View::Damage, None);
        g.send_history(sent, cx);
    });
    settle(cx, &rig.session);
    let meta = rig.gui.read_with(cx, |g, cx| {
        let w = super::super::w::W::new(1.0, 1440., cx);
        super::super::fight_head::Head::of(g, &w, cx).meta
    });
    assert!(meta.starts_with("Midnight, "), "{meta}");
}

/// In a fight of Forever's, whose log carries no builds, the talent viewer
/// says so; in Retail's it says nothing of the kind.
#[gpui_kit::test]
fn the_talent_viewer_says_forever_logs_no_builds(cx: &mut TestAppContext) {
    let link = EditionsLink::owned(&[RETAIL_ME, FOREVER_ME]).with_forever_source();
    let rig = rig_on(cx, 1440., 900., link);
    open_talents(cx, &rig);
    assert_eq!(talents_gap(cx, &rig), None, "Retail logs its builds");
    close_talents(cx, &rig);

    pick(cx, &rig, "edition-forever");
    rig.session.read_with(cx, |s, _| {
        let st = s.state();
        assert_eq!(st.edition(), Some(Edition::Forever));
        assert_eq!(
            st.screen,
            Screen::Meter,
            "Forever's newest pull on the stage"
        );
        assert_eq!(st.segment_edition(), Some(Edition::Forever));
    });
    open_talents(cx, &rig);
    assert!(shown(cx, &rig, "talents-gap"));
    assert_eq!(
        talents_gap(cx, &rig).as_deref(),
        Some("Forever's combat log does not carry talents or gear.")
    );
}

fn open_talents(cx: &mut TestAppContext, rig: &Rig) {
    cx.update_window(rig.window, |_, window, cx| {
        window.dispatch_action(Box::new(crate::keys::Go(crate::keys::Gesture::Talents)), cx);
        window.render_frame(cx);
    })
    .unwrap();
    settle(cx, &rig.session);
}

fn close_talents(cx: &mut TestAppContext, rig: &Rig) {
    rig.gui.update(cx, |g, cx| {
        g.talents = None;
        cx.notify();
    });
}

fn talents_gap(cx: &mut TestAppContext, rig: &Rig) -> Option<String> {
    rig.gui.read_with(cx, |g, cx| {
        g.talents.as_ref().and_then(|v| v.read(cx).gap.clone())
    })
}
