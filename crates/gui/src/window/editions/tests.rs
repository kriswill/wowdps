//! The edition switch, through the window: over a daemon of one edition
//! nothing of it is drawn or asked; over two, it lists the games, shows the
//! one picked — the one being played by default — scopes the meter, the
//! rail, Home and the picker, and remembers each game's own keys.

use std::cell::RefCell;
use std::rc::Rc;

use gpui_kit::test::TestWindowExt as _;
use gpui_kit::{AppContext, ElementId, TestAppContext, px, size};
use wowdps_daemon::mock::MockDaemon;
use wowdps_gui_logic::config::{Config, KeysOf};
use wowdps_gui_logic::editions::PLAYING_NOW;
use wowdps_model::{Edition, Screen};
use wowdps_proto::{ClientMsg, EditionRef, HistoryQuery};

use super::super::tests::{Rig, own_config, press, rig_on, settle};
use super::super::{Gui, Place};
use crate::session::Session;
use crate::testkit::{self, EditionsLink, MockLink};

/// The fixtures' characters: Retail's and Forever's, each the owner of its
/// edition's stored fights.
const RETAIL_ME: &str = "Thraxx-Nebula-US";
const FOREVER_ME: &str = "Velkris-Nebula-US";

type Sent = Rc<RefCell<Vec<ClientMsg>>>;

/// What a test keeps of its link once the rig owns it: what was sent, and
/// the live and running games it can move.
struct Handles {
    sent: Sent,
    live: Rc<std::cell::Cell<Edition>>,
    playing: Rc<std::cell::Cell<Option<Edition>>>,
}

fn handles(link: &EditionsLink) -> Handles {
    Handles {
        sent: Rc::clone(&link.sent),
        live: Rc::clone(&link.live),
        playing: Rc::clone(&link.playing),
    }
}

fn shown(cx: &mut TestAppContext, rig: &Rig, id: impl Into<ElementId>) -> bool {
    let id = id.into();
    cx.update_window(rig.window, |_, window, cx| {
        window.render_frame(cx);
        window.try_find(id).is_some()
    })
    .unwrap()
}

/// Press the switch, then one of its games.
fn pick(cx: &mut TestAppContext, rig: &Rig, entry: &str) {
    press(cx, rig, "top-edition");
    assert!(shown(cx, rig, "edition-menu"), "the menu opens");
    press(cx, rig, ElementId::Name(entry.to_string().into()));
    settle(cx, &rig.session);
}

/// Ask for `Status` as the running session does every second, and let
/// the window hear it.
fn status(cx: &mut TestAppContext, rig: &Rig) {
    rig.session
        .update(cx, |s, _| s.request(ClientMsg::GetStatus { req_id: 0 }));
    settle(cx, &rig.session);
    cx.update_window(rig.window, |_, window, cx| window.render_frame(cx))
        .unwrap();
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

/// The pages the window asked for since `from`: each one's edition filter.
fn pages_since(sent: &Sent, from: usize) -> Vec<Option<Edition>> {
    sent.borrow()
        .iter()
        .skip(from)
        .filter_map(|m| match m {
            ClientMsg::GetHistory {
                query: HistoryQuery::Fights { edition, limit, .. },
                ..
            } if *limit > 1 => Some(*edition),
            _ => None,
        })
        .collect()
}

fn two(cx: &mut TestAppContext, link: EditionsLink) -> (Rig, Handles) {
    let h = handles(&link);
    let rig = rig_on(cx, 1440., 900., link);
    (rig, h)
}

/// A daemon of one edition: no switch, and every history question the
/// window asks is the one it always asked — every edition's — but for the
/// census's own one-card probes.
#[gpui_kit::test]
fn one_edition_draws_no_switch_and_asks_as_before(cx: &mut TestAppContext) {
    let sent: Sent = Rc::default();
    struct Spy(MockLink, Sent);
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
        Rc::clone(&sent),
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
        assert_eq!(g.keys(), KeysOf::new(Edition::Retail, true));
    });
    assert!(
        pages_since(&sent, 0).iter().all(Option::is_none),
        "every page asks for every edition"
    );
    for msg in sent.borrow().iter() {
        if let ClientMsg::Watch { edition, .. } = msg {
            assert_eq!(*edition, EditionRef::Follow, "the meter follows");
        }
    }
}

/// Two editions' fights in the store, Retail played: the switch stands
/// beside the places named by the game shown, with the live dot while it
/// is the game being played, and the rail lists that game alone. Picking
/// Forever pins it — no Forever log is followed, so its newest stored pull
/// takes the stage — and picking Midnight again follows the game played.
#[gpui_kit::test]
fn the_switch_shows_one_game_at_a_time(cx: &mut TestAppContext) {
    let (rig, h) = two(cx, EditionsLink::owned(&[RETAIL_ME, FOREVER_ME]));
    h.playing.set(Some(Edition::Retail));
    status(cx, &rig);
    assert!(shown(cx, &rig, "top-edition"), "the switch is drawn");
    rig.gui.read_with(cx, |g, _| {
        assert!(g.hist.store.census.several());
        assert_eq!(g.hist.store.earlier.edition(), Some(Edition::Retail));
        let face = g.edition_face();
        assert_eq!((face.name.as_str(), face.playing), ("Midnight", true));
    });
    assert_eq!(rail_editions(cx, &rig), vec![Edition::Retail]);

    pick(cx, &rig, "edition-forever");
    rig.gui.read_with(cx, |g, cx| {
        assert!(!g.cards.edition, "the menu closes");
        assert_eq!(g.edition, EditionRef::Pin(Edition::Forever));
        assert_eq!(g.hist.edition, Edition::Forever);
        let st = g.session.read(cx).state();
        assert_eq!(st.edition_ref(), EditionRef::Pin(Edition::Forever));
        assert_eq!(st.segment_count(), 0, "no Forever log is followed");
        let stored = g
            .hist
            .store
            .stored
            .as_ref()
            .expect("a stored pull on stage");
        assert_eq!(
            stored.card.as_ref().map(|c| c.edition()),
            Some(Edition::Forever),
            "Forever's newest stored pull, not a blank"
        );
        let face = g.edition_face();
        assert_eq!((face.name.as_str(), face.playing), ("Forever", false));
    });
    assert_eq!(rail_editions(cx, &rig), vec![Edition::Forever]);
    assert!(
        h.sent.borrow().iter().any(|m| matches!(
            m,
            ClientMsg::Watch {
                edition: EditionRef::Pin(Edition::Forever),
                ..
            }
        )),
        "the meter's Watch pins Forever"
    );

    pick(cx, &rig, "edition-retail");
    rig.gui.read_with(cx, |g, cx| {
        assert_eq!(g.edition, EditionRef::Follow, "the game played is followed");
        assert_eq!(g.hist.edition, Edition::Retail);
        assert!(g.hist.store.stored.is_none());
        let st = g.session.read(cx).state();
        assert_eq!(st.edition_ref(), EditionRef::Follow);
        assert_eq!(st.screen, Screen::Meter, "back on the log's newest pull");
    });
    assert_eq!(rail_editions(cx, &rig), vec![Edition::Retail]);
}

/// The menu lists the games and nothing else — the one shown lit, the one
/// being played noted — and it is modal: any key closes it.
#[gpui_kit::test]
fn the_edition_menu_lists_the_games(cx: &mut TestAppContext) {
    let (rig, h) = two(cx, EditionsLink::owned(&[RETAIL_ME, FOREVER_ME]));
    press(cx, &rig, "top-edition");
    assert!(shown(cx, &rig, "edition-retail"));
    assert!(shown(cx, &rig, "edition-forever"));
    assert!(!shown(cx, &rig, "edition-follow"), "no mode to learn");
    rig.gui.read_with(cx, |g, _| {
        let e = g.edition_entries().expect("entries");
        let names: Vec<&str> = e.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, vec!["Midnight", "Forever"]);
        assert!(e[0].on, "the game shown");
        assert_eq!((e[0].note, e[1].note), (None, None), "no game running");
        assert!(g.modal(), "the menu holds the keys");
    });
    cx.update_window(rig.window, |_, window, cx| {
        window.press("x", cx);
        window.render_frame(cx);
    })
    .unwrap();
    rig.gui
        .read_with(cx, |g, _| assert!(!g.cards.edition, "any key closes it"));
    h.playing.set(Some(Edition::Retail));
    status(cx, &rig);
    rig.gui.read_with(cx, |g, _| {
        let e = g.edition_entries().expect("entries");
        assert_eq!((e[0].note, e[1].note), (Some(PLAYING_NOW), None));
    });
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
    let (rig, _) = two(cx, EditionsLink::owned(&[RETAIL_ME, FOREVER_ME]));
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
    assert_eq!(
        disk.character_in(KeysOf::new(Edition::Forever, false)),
        Some(guid.as_str())
    );
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

/// Review 1: Home read in Retail and closed, then Forever picked: whose
/// window it is comes from Forever's fights, never Home's Retail answer.
#[gpui_kit::test]
fn a_switch_with_home_closed_names_the_new_game_s_owner(cx: &mut TestAppContext) {
    let (rig, _) = two(cx, EditionsLink::owned(&[RETAIL_ME, FOREVER_ME]));
    press(cx, &rig, "place-home");
    settle(cx, &rig.session);
    rig.gui.read_with(cx, |g, _| {
        let owner = g.hist.store.panels.owner.as_ref().map(|o| o.name.clone());
        assert_eq!(owner.as_deref(), Some(RETAIL_ME), "Home named Retail's");
    });
    press(cx, &rig, "place-fights");
    pick(cx, &rig, "edition-forever");
    rig.gui.read_with(cx, |g, cx| {
        assert!(g.hist.store.home.is_none(), "Home stayed shut");
        assert!(g.hist.store.panels.owner.is_none(), "Retail's owner went");
        assert_eq!(
            g.picked(cx).map(|p| p.name).as_deref(),
            Some(FOREVER_ME),
            "the picker names Forever's character"
        );
    });
}

/// Review 2: Forever is the live game and Retail is shown; picking
/// Forever follows it, and nothing on the way asks for Retail's fights.
#[gpui_kit::test]
fn a_pick_that_follows_never_passes_through_retail(cx: &mut TestAppContext) {
    let link = EditionsLink::owned(&[RETAIL_ME, FOREVER_ME]).with_forever_source();
    link.live.set(Edition::Forever);
    let (rig, h) = two(cx, link);
    status(cx, &rig);
    rig.gui.read_with(cx, |g, _| {
        assert_eq!(g.hist.edition, Edition::Forever, "following the live game");
    });
    pick(cx, &rig, "edition-retail");
    rig.gui.read_with(cx, |g, _| {
        assert_eq!(g.edition, EditionRef::Pin(Edition::Retail));
        assert_eq!(g.hist.edition, Edition::Retail);
    });
    let from = h.sent.borrow().len();
    pick(cx, &rig, "edition-forever");
    rig.gui.read_with(cx, |g, cx| {
        assert_eq!(g.edition, EditionRef::Follow, "the game played is followed");
        assert_eq!(g.hist.edition, Edition::Forever);
        assert_eq!(g.session.read(cx).state().edition(), Some(Edition::Forever));
    });
    let pages = pages_since(&h.sent, from);
    assert!(!pages.is_empty(), "Forever's pages were asked for");
    assert!(
        pages.iter().all(|e| *e == Some(Edition::Forever)),
        "no page of Retail's on the way: {pages:?}"
    );
}

/// A pin whose game becomes the live one follows again once the live game
/// moves on: the window keeps showing what you play.
#[gpui_kit::test]
fn a_pin_that_catches_up_with_the_game_played_follows_it(cx: &mut TestAppContext) {
    let link = EditionsLink::owned(&[RETAIL_ME, FOREVER_ME]).with_forever_source();
    let (rig, h) = two(cx, link);
    pick(cx, &rig, "edition-forever");
    rig.gui.read_with(cx, |g, _| {
        assert_eq!(g.edition, EditionRef::Pin(Edition::Forever));
    });
    // Forever becomes the game played: nothing moves under the reader.
    h.live.set(Edition::Forever);
    h.playing.set(Some(Edition::Forever));
    status(cx, &rig);
    rig.gui.read_with(cx, |g, _| {
        assert_eq!(g.hist.edition, Edition::Forever);
        assert!(g.edition_face().playing, "the dot: this is the game played");
    });
    // Then Retail is: the window goes with it.
    h.live.set(Edition::Retail);
    h.playing.set(Some(Edition::Retail));
    status(cx, &rig);
    rig.gui.read_with(cx, |g, cx| {
        assert_eq!(g.edition, EditionRef::Follow);
        assert_eq!(g.hist.edition, Edition::Retail);
        assert_eq!(g.session.read(cx).state().edition(), Some(Edition::Retail));
        let face = g.edition_face();
        assert_eq!((face.name.as_str(), face.playing), ("Midnight", true));
    });
    // A pin that never caught up stays: Forever picked while Retail is
    // played stays Forever when Retail stops.
    pick(cx, &rig, "edition-forever");
    h.playing.set(None);
    status(cx, &rig);
    rig.gui.read_with(cx, |g, _| {
        assert_eq!(g.edition, EditionRef::Pin(Edition::Forever));
    });
}

/// A game with no fight to show — a Forever log followed with no pull in
/// it, and none stored — says how to get one, and its header names no
/// edition.
#[gpui_kit::test]
fn an_empty_game_says_how_to_get_a_fight(cx: &mut TestAppContext) {
    let dir = std::env::temp_dir().join(format!(
        "wowdps-gui-empty-forever-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = std::fs::create_dir_all(&dir);
    let log = dir.join("WoWCombatLog-empty.txt");
    let header = std::fs::read_to_string(EditionsLink::fixture("forever.txt"))
        .ok()
        .and_then(|t| t.lines().next().map(str::to_string))
        .expect("the fixture's header");
    std::fs::write(&log, format!("{header}\n")).expect("a log with no pull");
    let link = EditionsLink::stored(&[RETAIL_ME], false).with_forever_log(&log);
    let (rig, _) = two(cx, link);
    assert!(
        shown(cx, &rig, "top-edition"),
        "Forever is known by its log"
    );
    // Picked, and not yet answered: the window's own reset is no answer.
    rig.gui
        .update(cx, |g, cx| g.pick_edition(Edition::Forever, cx));
    rig.gui.read_with(cx, |g, cx| {
        assert_eq!(g.empty_game(cx), None, "the daemon has not said yet");
    });
    settle(cx, &rig.session);
    assert!(shown(cx, &rig, "stage-empty"));
    rig.gui.read_with(cx, |g, cx| {
        assert_eq!(
            g.empty_game(cx),
            Some((
                "No Forever fights yet".to_string(),
                "Play a fight in Forever with combat logging on and it shows up here.".to_string()
            ))
        );
        let w = super::super::w::W::new(1.0, 1440., cx);
        let meta = super::super::fight_head::Head::of(g, &w, cx).meta;
        assert!(!meta.contains("Midnight"), "{meta}");
    });
    let _ = std::fs::remove_dir_all(&dir);
}

/// In a fight of Forever's, whose log carries no builds, the talent viewer
/// says so; in Retail's it says nothing of the kind.
#[gpui_kit::test]
fn the_talent_viewer_says_forever_logs_no_builds(cx: &mut TestAppContext) {
    let link = EditionsLink::owned(&[RETAIL_ME, FOREVER_ME]).with_forever_source();
    let (rig, _) = two(cx, link);
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
    rig.gui.read_with(cx, |g, _| {
        assert!(
            g.hist.store.stored.is_none(),
            "a followed log shows its own pull, never a stored one"
        );
    });
    open_talents(cx, &rig);
    assert!(shown(cx, &rig, "talents-gap"));
    assert_eq!(
        talents_gap(cx, &rig).as_deref(),
        Some("Forever's combat log does not carry talents or gear.")
    );
}

/// One game alone — Forever's, a config written before editions — reads
/// the top-level keys its table lacks: Home opens on the configured
/// character, and history questions stay every edition's. The picker's
/// follow item lets that pick go where it is read: the top-level key.
#[gpui_kit::test]
fn one_game_alone_reads_the_top_level_keys(cx: &mut TestAppContext) {
    let path = own_config();
    cx.update(crate::keys::bind);
    let guid = "Player-6101-0A2B3C01".to_string();
    std::fs::write(
        &path,
        format!("zoom = 1.0\nhome_on_start = false\ncharacter = \"{guid}\"\n"),
    )
    .expect("the config");
    let cfg = Config::load();
    assert_eq!(cfg.character.as_deref(), Some(guid.as_str()));
    let mock = MockDaemon::fixture_at(&EditionsLink::fixture("forever.txt"))
        .with_characters(&[FOREVER_ME.to_string()])
        .with_history();
    let (window, gui) = testkit::open(cx, size(px(1440.), px(900.)), |window, cx| {
        let session = cx.new(|_| Session::new(MockLink::new(mock)));
        cx.new(|cx| Gui::new(session, cfg, window, cx))
    });
    let session = gui.read_with(cx, |g, _| g.session().clone());
    settle(cx, &session);
    let rig = Rig {
        window,
        gui,
        session,
    };
    assert!(!shown(cx, &rig, "top-edition"), "one game: no switch");
    rig.gui.read_with(cx, |g, _| {
        assert_eq!(g.hist.edition, Edition::Forever);
        assert_eq!(g.keys(), KeysOf::new(Edition::Forever, true));
        assert_eq!(g.cfg.character_in(g.keys()), Some(guid.as_str()));
    });
    press(cx, &rig, "place-home");
    settle(cx, &rig.session);
    rig.gui.read_with(cx, |g, _| {
        let home = g.hist.store.home.as_ref().expect("Home is up");
        assert_eq!(
            home.scope.as_deref(),
            Some(guid.as_str()),
            "the configured pick"
        );
        assert_eq!(home.edition, None, "every edition's, as before");
    });
    press(cx, &rig, "top-picker");
    press(cx, &rig, super::super::cards::menu_row_id(None));
    rig.gui.read_with(cx, |g, _| {
        assert_eq!(g.cfg.character_in(g.keys()), None, "the pick let go");
        let home = g.hist.store.home.as_ref().expect("Home is up");
        assert_eq!(home.scope, None, "every character");
    });
    let disk = Config::load();
    assert_eq!(disk.character, None, "cleared on disk, where it is read");
    assert!(disk.editions.is_empty());
}

/// Review 7: a census question lost with the connection (the daemon
/// restarted) is asked again once the link is back.
#[gpui_kit::test]
fn a_census_question_lost_to_a_restart_is_asked_again(cx: &mut TestAppContext) {
    use std::cell::Cell;
    use std::time::{Duration, Instant};
    struct Flaky {
        inner: EditionsLink,
        down: Rc<Cell<bool>>,
    }
    impl crate::session::Link for Flaky {
        fn send(&mut self, msg: &ClientMsg) {
            if !self.down.get() {
                self.inner.send(msg);
            }
        }
        fn poll(&mut self) -> Vec<wowdps_proto::DaemonMsg> {
            if self.down.get() {
                Vec::new()
            } else {
                self.inner.poll()
            }
        }
        fn reconnect(&mut self) -> wowdps_proto::Reconnect {
            if self.down.get() {
                wowdps_proto::Reconnect::Waiting
            } else {
                wowdps_proto::Reconnect::Connected
            }
        }
    }
    // Forever has no fight stored: the census keeps asking about it.
    let inner = EditionsLink::stored(&[RETAIL_ME], false);
    let h = handles(&inner);
    let down = Rc::new(Cell::new(false));
    let rig = rig_on(
        cx,
        1440.,
        900.,
        Flaky {
            inner,
            down: Rc::clone(&down),
        },
    );
    // The daemon goes away; a question goes out meanwhile and is lost.
    down.set(true);
    settle(cx, &rig.session);
    let before = h.sent.borrow().len();
    rig.gui.update(cx, |g, cx| {
        g.hist.store.census.store_changed();
        let sent = g
            .hist
            .store
            .requests(Instant::now() + Duration::from_secs(60));
        g.send_history(sent, cx);
    });
    assert_eq!(h.sent.borrow().len(), before, "nothing reached the daemon");
    // It is back: the question is asked again.
    down.set(false);
    settle(cx, &rig.session);
    let again = h.sent.borrow().iter().skip(before).any(|m| {
        matches!(
            m,
            ClientMsg::GetHistory {
                query: HistoryQuery::Fights {
                    edition: Some(Edition::Forever),
                    limit: 1,
                    ..
                },
                ..
            }
        )
    });
    assert!(again, "the lost question is asked again");
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
