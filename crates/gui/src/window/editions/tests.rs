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
use wowdps_gui_logic::editions::{FOREVER, PLAYING_NOW};
use wowdps_model::{Edition, Screen};
use wowdps_proto::{ClientMsg, EditionRef, HistoryQuery};

use super::super::tests::{Rig, own_config, press, rig_on, settle};
use super::super::{Gui, Place};
use crate::session::Session;
use crate::testkit::{self, EditionsLink, MockLink};

/// The fixtures' characters: Retail's and Forever's, each the owner of its
/// edition's stored fights.
const RETAIL_ME: &str = "Thraxx-Nebula-US";
const FOREVER_ME: &str = "Velkris-Duskmantle-Nebula-US";

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

/// Press one of the switch's games: its chip where the games sit side by
/// side, else the dropdown and then its row.
fn pick(cx: &mut TestAppContext, rig: &Rig, entry: &str) {
    let id = ElementId::Name(entry.to_string().into());
    if !shown(cx, rig, id.clone()) {
        press(cx, rig, "top-edition");
        assert!(shown(cx, rig, "edition-menu"), "the menu opens");
    }
    press(cx, rig, id);
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
        // The stored Forever pull's header tags its game.
        let w = super::super::w::W::new(1.0, 1440., cx);
        let tag = super::super::fight_head::Head::of(g, &w, cx).tag;
        assert_eq!(
            tag.map(|t| (t.name, t.mark)),
            Some(("Forever".into(), FOREVER))
        );
    });
    assert!(shown(cx, &rig, "head-edition"), "the tag is drawn");
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
        let w = super::super::w::W::new(1.0, 1440., cx);
        let tag = super::super::fight_head::Head::of(g, &w, cx).tag;
        assert_eq!(tag, None, "a Retail fight in Retail's window");
    });
    assert!(!shown(cx, &rig, "head-edition"));
    assert_eq!(rail_editions(cx, &rig), vec![Edition::Retail]);
}

/// A Forever fight's meter writes each player by both names, as the game
/// shows them, and two players who share a first name are two rows, each
/// by its own names — with realms or without, as the option says. Retail's
/// rows read as the log wrote them.
#[gpui_kit::test]
fn a_forever_meter_writes_both_names(cx: &mut TestAppContext) {
    let (rig, h) = two(cx, EditionsLink::owned(&[RETAIL_ME, FOREVER_ME]));
    h.playing.set(Some(Edition::Retail));
    status(cx, &rig);
    let labels = |cx: &mut TestAppContext| {
        rig.gui.read_with(cx, |g, cx| {
            let w = super::super::w::W::new(1.0, 1440., cx);
            super::super::table::Meter::of(g, &w, cx).drawn_labels()
        })
    };
    assert!(
        labels(cx).contains(&RETAIL_ME.to_string()),
        "{:?}",
        labels(cx)
    );
    pick(cx, &rig, "edition-forever");
    let shown = labels(cx);
    for name in [
        "Tharokk Emberfell-Nebula-US",
        "Tharokk Ironvale-Nebula-US",
        "Velkris Duskmantle-Nebula-US",
    ] {
        assert!(shown.contains(&name.to_string()), "{shown:?}");
    }
    rig.gui.update(cx, |g, _| g.cfg.hide_realms = true);
    let hidden = labels(cx);
    for name in [
        "Tharokk Emberfell",
        "Tharokk Ironvale",
        "Velkris Duskmantle",
    ] {
        assert!(hidden.contains(&name.to_string()), "{hidden:?}");
    }
    assert!(!hidden.contains(&"Tharokk".to_string()), "{hidden:?}");
}

/// Where the bar has room the games sit side by side, a chip each, every
/// one in sight: a press on one shows it, and there is no menu to open. A
/// narrow bar has the dropdown instead.
#[gpui_kit::test]
fn the_games_sit_side_by_side_where_they_fit(cx: &mut TestAppContext) {
    let (rig, h) = two(cx, EditionsLink::owned(&[RETAIL_ME, FOREVER_ME]));
    h.playing.set(Some(Edition::Retail));
    status(cx, &rig);
    assert!(shown(cx, &rig, "top-edition"), "the switch is drawn");
    assert!(shown(cx, &rig, "edition-retail") && shown(cx, &rig, "edition-forever"));
    rig.gui.read_with(cx, |g, _| assert!(g.edition_chips()));
    // The dropdown's press does nothing here: no menu is opened.
    cx.update_window(rig.window, |_, window, cx| {
        rig.gui.update(cx, |g, cx| {
            g.toggle_menu(super::super::cards::Menu::Edition, window, cx)
        });
        window.render_frame(cx);
    })
    .unwrap();
    rig.gui
        .read_with(cx, |g, _| assert!(!g.cards.edition && !g.modal()));
    assert!(!shown(cx, &rig, "edition-menu"));
    press(cx, &rig, "edition-forever");
    rig.gui.read_with(cx, |g, _| {
        assert_eq!(g.edition, EditionRef::Pin(Edition::Forever));
        assert_eq!(g.hist.edition, Edition::Forever);
    });
    press(cx, &rig, "edition-retail");
    rig.gui
        .read_with(cx, |g, _| assert_eq!(g.edition, EditionRef::Follow));

    let narrow = rig_on(
        cx,
        460.,
        860.,
        EditionsLink::owned(&[RETAIL_ME, FOREVER_ME]),
    );
    assert!(shown(cx, &narrow, "top-edition"));
    assert!(
        !shown(cx, &narrow, "edition-forever"),
        "no chips when narrow"
    );
    narrow.gui.read_with(cx, |g, _| assert!(!g.edition_chips()));
    pick(cx, &narrow, "edition-forever");
    narrow
        .gui
        .read_with(cx, |g, _| assert_eq!(g.hist.edition, Edition::Forever));
}

/// The menu lists the games and nothing else — the one shown lit, the one
/// being played noted — and it is modal: any key closes it. It is the
/// dropdown's, on a bar too narrow for the games side by side.
#[gpui_kit::test]
fn the_edition_menu_lists_the_games(cx: &mut TestAppContext) {
    let link = EditionsLink::owned(&[RETAIL_ME, FOREVER_ME]);
    let h = handles(&link);
    let rig = rig_on(cx, 460., 860., link);
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
        let head = super::super::fight_head::Head::of(g, &w, cx);
        assert!(!head.meta.contains("Midnight"), "{}", head.meta);
        assert_eq!(head.tag, None, "no fight, no tag");
    });
    assert!(shown(cx, &rig, "stage-empty-mark"), "the game's badge");
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
    assert_eq!(talents_edition(cx, &rig), Some(Edition::Retail));
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
    assert_eq!(
        talents_edition(cx, &rig),
        Some(Edition::Forever),
        "the viewer reads Forever's own trees"
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

/// The game whose trees the open talent viewer reads.
fn talents_edition(cx: &mut TestAppContext, rig: &Rig) -> Option<Edition> {
    rig.gui.read_with(cx, |g, cx| {
        g.talents.as_ref().map(|v| v.read(cx).ui.edition)
    })
}

/// A drawn stand-in for a game's emblem: a disc, never the game's art.
const DISC: &[u8] = br##"<svg width="4" height="4" viewBox="0 0 4 4" xmlns="http://www.w3.org/2000/svg"><circle cx="2" cy="2" r="2" fill="#808080"/></svg>"##;

/// The top bar's pieces beside the switch.
const BAR: [&str; 6] = [
    "place-home",
    "place-fights",
    "top-jump",
    "top-picker",
    "top-gear",
    "top-help",
];

/// What the bar is measured by: the switch, its chips (where the games
/// sit side by side), the bar's other pieces, the meter's top under it,
/// and the dropdown menu's rows (where the bar is narrow).
#[derive(Debug, PartialEq)]
struct Measure {
    switch: gpui_kit::Bounds<gpui_kit::Pixels>,
    chips: Vec<gpui_kit::Bounds<gpui_kit::Pixels>>,
    bar: Vec<gpui_kit::Bounds<gpui_kit::Pixels>>,
    meter_top: gpui_kit::Pixels,
    rows: Vec<gpui_kit::Bounds<gpui_kit::Pixels>>,
    /// How many badges hold an emblem rather than our mark, the menu's
    /// included.
    emblems: usize,
    /// The switch's form, as the bar drew it.
    form: Option<wowdps_gui_logic::editions::Form>,
    /// The live pill said "Live".
    live: bool,
}

impl Measure {
    /// Every place and size, without what the badges hold.
    fn geometry(&self) -> Measure {
        Measure {
            emblems: 0,
            form: self.form,
            live: self.live,
            switch: self.switch,
            chips: self.chips.clone(),
            bar: self.bar.clone(),
            meter_top: self.meter_top,
            rows: self.rows.clone(),
        }
    }
}

/// The window over two editions at `at`, in its real faces, measured with
/// `emblem` as every game's emblem (`None`: none cached, our marks).
fn measure(at: (f32, f32), emblem: Option<&'static [u8]>) -> Measure {
    measure_in(at, emblem, Edition::Retail, false, false)
}

/// [`measure`], with `playing` the game being played, Forever shown when
/// `forever`, and Retail's last pull still going when `live`.
fn measure_in(
    at: (f32, f32),
    emblem: Option<&'static [u8]>,
    playing: Edition,
    forever: bool,
    live: bool,
) -> Measure {
    use super::super::cards::Menu;
    use super::super::shots::{Shot, app};
    // No art but the stand-in in either run (it is read before the
    // cache), so the two differ by the badge's content alone on any
    // machine.
    crate::images::without_art(true);
    crate::images::emblem_on_this_thread(emblem);
    let mut cx = app();
    let path = own_config();
    // The theme named, as the app's: a pick repaints the window from the
    // config.
    std::fs::write(
        &path,
        format!(
            "zoom = 1.0\nhome_on_start = false\ntheme = \"{}\"\n",
            testkit::shots_theme().name
        ),
    )
    .expect("the config");
    let cfg = Config::load();
    let mut link = EditionsLink::owned(&[RETAIL_ME, FOREVER_ME]).with_forever_source();
    if live {
        link = link.with_retail_live(&[RETAIL_ME, FOREVER_ME]);
    }
    link.playing.set(Some(playing));
    let mut held = None;
    let (window, gui) = testkit::open_headless(&mut cx, size(px(at.0), px(at.1)), |window, cx| {
        let session =
            cx.new(|_| Session::with_state(Box::new(link), wowdps_proto::ClientState::new()));
        held = Some(session.clone());
        cx.new(|cx| Gui::new(session, cfg, window, cx))
    });
    let shot = Shot {
        window,
        gui,
        session: held.expect("the session was built"),
    };
    shot.settle(&mut cx);
    if forever {
        cx.update_entity(&shot.gui, |g, cx| g.pick_edition(Edition::Forever, cx));
        shot.settle(&mut cx);
    }
    let ids = ["edition-retail", "edition-forever"];
    let (switch, chips, bar, meter_top) = cx
        .update_window(window, |_, window, cx| {
            window.render_frame(cx);
            window.render_frame(cx);
            (
                window.find("top-edition").bounds(),
                ids.iter()
                    .filter_map(|id| window.try_find(ElementId::Name((*id).into())))
                    .map(|e| e.bounds())
                    .collect::<Vec<_>>(),
                BAR.iter()
                    .filter_map(|id| window.try_find(*id).map(|e| e.bounds()))
                    .collect::<Vec<_>>(),
                window.find("meter-list").bounds().origin.y,
            )
        })
        .expect("the window");
    let gui = shot.gui.clone();
    let (rows, emblems) = cx
        .update_window(window, |_, window, cx| {
            gui.update(cx, |g, cx| g.toggle_menu(Menu::Edition, window, cx));
            window.render_frame(cx);
            window.render_frame(cx);
            let emblems = window.find_all("badge-emblem").len();
            if window.try_find("edition-menu").is_none() {
                return (Vec::new(), emblems);
            }
            let rows = ids
                .iter()
                .filter_map(|id| window.try_find(ElementId::Name((*id).into())))
                .map(|e| e.bounds())
                .collect();
            (rows, emblems)
        })
        .expect("the window");
    let (form, live) = shot.gui.read_with(&cx, |g, cx| {
        let state = g.session.read(cx).state();
        let live = state.entries().last().is_some_and(|e| e.row.live);
        (g.edition_form.get(), live)
    });
    crate::images::emblem_on_this_thread(None);
    crate::images::without_art(false);
    Measure {
        switch,
        chips,
        bar,
        meter_top,
        rows,
        emblems,
        form,
        live,
    }
}

/// A game's badge holds its emblem where one is cached, else our mark,
/// and the bar is the same either way at each of the three frames: the
/// switch, its chips and the menu's rows keep their every pixel of place.
/// The games sit side by side where the bar has room — named at the wide
/// frame, compact at the tile frame (the game shown named, the other its
/// round badge), every chip as tall as the live pill and on the jump
/// box's line — and at the narrow frame the dropdown's menu lists them;
/// the bar's other pieces and the meter keep their line.
#[test]
#[cfg_attr(
    target_os = "macos",
    ignore = "gpui's mac platform must be created on the main thread"
)]
fn a_badge_keeps_the_switch_s_rhythm() {
    for (frame, w, h) in super::super::shots::SIZES {
        let marks = measure((w, h), None);
        let emblems = measure((w, h), Some(DISC));
        assert_eq!(marks.emblems, 0, "{frame}: our marks, no emblem");
        assert!(emblems.emblems >= 2, "{frame}: the emblems are drawn");
        assert_eq!(
            marks.geometry(),
            emblems.geometry(),
            "{frame}: an emblem or a mark, one rhythm"
        );
        if !marks
            .form
            .is_some_and(wowdps_gui_logic::editions::Form::chips)
        {
            assert!(marks.chips.is_empty(), "{frame}: the dropdown");
            assert_eq!(marks.rows.len(), 2, "{frame}: its menu lists both");
            assert_eq!(marks.rows[0].size, marks.rows[1].size);
        } else {
            assert_eq!(marks.chips.len(), 2, "{frame}: side by side");
            assert!(marks.rows.is_empty(), "{frame}: no menu");
            let jump = marks.bar[2];
            for chip in &marks.chips {
                assert_eq!(chip.size.height, px(28.), "{frame}: the pill's height");
                assert!(
                    (chip.center().y - jump.center().y).abs() < px(0.5),
                    "{frame}: on the jump box's line"
                );
                assert!(marks.switch.contains(&chip.center()), "{frame}");
            }
        }
        assert!(marks.bar.len() >= 2, "{frame}: the places at least");
        use wowdps_gui_logic::editions::Form;
        match frame {
            "wide" => assert_eq!(marks.form, Some(Form::Chips), "the wide frame"),
            "tile" => {
                assert_eq!(marks.form, Some(Form::ChipsCompact), "the tile frame");
                // The game not shown is its badge alone: a round chip.
                let quiet = marks.chips[1];
                assert_eq!(quiet.size.width, quiet.size.height, "a round chip");
                assert!(
                    marks.chips[0].size.width > quiet.size.width,
                    "the named chip"
                );
            }
            _ => assert!(
                !marks.form.is_some_and(Form::chips),
                "{frame}: the dropdown"
            ),
        }
    }
}

/// The switch holds still: its form is the window's width's alone, so
/// neither a pick nor a pull going live changes it. At every width from
/// the narrow frame to the wide one, with each game shown and with
/// Retail's last pull going or over, the form and the switch's place and
/// size are the same — the dropdown keeps room for the longest name, the
/// compact row for its named chip — every chip's too but where the
/// compact row's named chip follows a pick, and beside the chips the
/// search box keeps its place. Only the fill, the pip and that name move,
/// and the pill's own words.
#[test]
#[cfg_attr(
    target_os = "macos",
    ignore = "gpui's mac platform must be created on the main thread"
)]
fn the_switch_holds_still_whatever_is_shown_or_played() {
    let mut widths = vec![460.0, 640.0];
    widths.extend((21..=37).map(|w| w as f32 * 40.0));
    for width in widths {
        let states =
            [(false, false), (true, false), (false, true), (true, true)].map(|(forever, live)| {
                let m = measure_in((width, 900.0), None, Edition::Retail, forever, live);
                (m, forever, live)
            });
        assert!(states[2].0.live, "{width}: Retail's pull is going");
        let (first, ..) = &states[0];
        for (m, forever, live) in &states {
            let what = format!("{width}, Forever shown {forever}, live {live}");
            assert_eq!(m.form, first.form, "{what}: the form moved");
            assert_eq!(m.switch, first.switch, "{what}: the switch moved");
            // Compact, a pick moves the name within the row: the row holds,
            // and each game shown has its own chips, live or not.
            let compact = m
                .form
                .is_some_and(wowdps_gui_logic::editions::Form::compact);
            if !compact || !forever {
                assert_eq!(m.chips, first.chips, "{what}: a chip moved");
            } else {
                assert_eq!(m.chips, states[1].0.chips, "{what}: a chip moved");
            }
            if m.form.is_some_and(wowdps_gui_logic::editions::Form::chips) {
                assert_eq!(
                    m.bar[2].origin.x, first.bar[2].origin.x,
                    "{what}: the search box moved"
                );
            }
        }
    }
}

/// The bar's pieces after the places, in their order, as the sweep reads
/// them.
const AFTER: [&str; 6] = [
    "top-edition",
    "top-jump",
    "top-live",
    "top-picker",
    "top-gear",
    "top-help",
];

/// The bar as `bar_at` found it drawn.
struct Drawn {
    /// Its last piece's right edge, in the window's pixels.
    right: f32,
    /// Whether any two pieces after the places overlap.
    overlap: bool,
    /// The switch's form.
    form: Option<wowdps_gui_logic::editions::Form>,
    /// The width of the jump box, or of its glyph where it stands alone.
    jump: f32,
    /// The pieces after the places that were drawn (`AFTER`'s ids).
    shown: Vec<&'static str>,
}

/// The bar over `link` at `width` in its real faces, `cards` told to its
/// census (a Retail build of another expansion, a third game), in a
/// window of its own (the headless platform keeps a window's size).
fn bar_at(
    cx: &mut gpui_kit::HeadlessAppContext,
    link: impl crate::session::Link + 'static,
    cards: &[wowdps_proto::history::FightCard],
    width: f32,
) -> Drawn {
    use super::super::shots::Shot;
    let path = own_config();
    std::fs::write(
        &path,
        format!(
            "zoom = 1.0\nhome_on_start = false\ntheme = \"{}\"\n",
            testkit::shots_theme().name
        ),
    )
    .expect("the config");
    let cfg = Config::load();
    let mut held = None;
    let (window, gui) = testkit::open_headless(cx, size(px(width), px(900.)), |window, cx| {
        let session =
            cx.new(|_| Session::with_state(Box::new(link), wowdps_proto::ClientState::new()));
        held = Some(session.clone());
        cx.new(|cx| Gui::new(session, cfg, window, cx))
    });
    let shot = Shot {
        window,
        gui,
        session: held.expect("the session was built"),
    };
    shot.settle(cx);
    if !cards.is_empty() {
        cx.update_entity(&shot.gui, |g, _| {
            g.hist.store.census.learn(cards);
        });
    }
    let mut drawn = cx
        .update_window(window, |_, window, cx| {
            window.render_frame(cx);
            window.render_frame(cx);
            let pieces: Vec<(&'static str, gpui_kit::Bounds<gpui_kit::Pixels>)> = AFTER
                .iter()
                .filter_map(|id| window.try_find(*id).map(|e| (*id, e.bounds())))
                .collect();
            let overlap = pieces
                .windows(2)
                .any(|p| p[0].1.right() > p[1].1.left() + px(0.5));
            Drawn {
                right: pieces.last().map_or(0.0, |(_, b)| f32::from(b.right())),
                overlap,
                form: None,
                jump: pieces
                    .iter()
                    .find(|(id, _)| *id == "top-jump")
                    .map_or(0.0, |(_, b)| f32::from(b.size.width)),
                shown: pieces.iter().map(|(id, _)| *id).collect(),
            }
        })
        .expect("the window");
    let _ = cx.update_window(window, |_, window, _| window.remove_window());
    drawn.form = shot.gui.read_with(cx, |g, _| g.edition_form.get());
    drawn
}

/// The bar never runs past the window while it holds the switch: at every
/// width from the window's least (320 px) up, with two games, two with
/// Retail named by its longest expansion, and three, no piece after the
/// places overlaps the next and the last ends inside the window. The
/// switch takes the richest form that fits, and at the least widths the
/// bar folds: the pill's clock, the help button, the jump glyph. (One game
/// folds only where its bar would run past:
/// `one_game_folds_only_where_its_bar_would_run_past`.)
#[test]
#[cfg_attr(
    target_os = "macos",
    ignore = "gpui's mac platform must be created on the main thread"
)]
fn the_bar_never_runs_past_the_window_with_the_switch() {
    use wowdps_proto::history::FightCard;
    crate::images::without_art(true);
    let card = |edition: Edition, build: (u16, u16, u16)| FightCard {
        id: format!("sweep-{}", edition.key()),
        project_id: edition.project_id(),
        build,
        start_utc_ms: i64::MAX / 2,
        ..FightCard::default()
    };
    let mut cx = super::super::shots::app();
    // Every 40 px from the least, and the widest frame: each window opens
    // anew (the headless platform keeps a window's size).
    let mut widths: Vec<f32> = (8..=37).map(|w| w as f32 * 40.0).collect();
    widths.push(1500.0);
    let configs: [(&str, usize, Vec<FightCard>); 3] = [
        ("two games", 2, vec![]),
        (
            "two games, a long name",
            2,
            vec![card(Edition::Retail, (8, 3, 0))],
        ),
        (
            "three games, a long name",
            3,
            vec![
                card(Edition::Retail, (8, 3, 0)),
                card(Edition::Classic(2), (1, 15, 7)),
            ],
        ),
    ];
    for (what, games, cards) in configs {
        let mut forms = std::collections::BTreeSet::new();
        for width in &widths {
            let link = EditionsLink::owned(&[RETAIL_ME, FOREVER_ME]).with_forever_source();
            link.playing.set(Some(Edition::Retail));
            let drawn = bar_at(&mut cx, link, &cards, *width);
            assert!(!drawn.overlap, "{what} at {width}: two pieces overlap");
            let over = (drawn.right - width).max(0.0);
            assert_eq!(over, 0.0, "{what} at {width}: the bar runs past");
            forms.extend(drawn.form);
        }
        // Every form is reached somewhere from the least width to the
        // widest.
        assert_eq!(
            forms.len(),
            wowdps_gui_logic::editions::Form::ALL.len(),
            "{what}: {forms:?}"
        );
        let link = EditionsLink::owned(&[RETAIL_ME, FOREVER_ME]).with_forever_source();
        let known = bar_games(&mut cx, link, &cards);
        assert_eq!(known, games, "{what}");
    }
    crate::images::without_art(false);
}

/// How many games the switch lists over `link` with `cards` told.
fn bar_games(
    cx: &mut gpui_kit::HeadlessAppContext,
    link: impl crate::session::Link + 'static,
    cards: &[wowdps_proto::history::FightCard],
) -> usize {
    let path = own_config();
    std::fs::write(&path, "zoom = 1.0\nhome_on_start = false\n").expect("the config");
    let cfg = Config::load();
    let mut held = None;
    let (window, gui) = testkit::open_headless(cx, size(px(1440.), px(900.)), |window, cx| {
        let session =
            cx.new(|_| Session::with_state(Box::new(link), wowdps_proto::ClientState::new()));
        held = Some(session.clone());
        cx.new(|cx| Gui::new(session, cfg, window, cx))
    });
    let shot = super::super::shots::Shot {
        window,
        gui,
        session: held.expect("the session was built"),
    };
    shot.settle(cx);
    let games = cx.update_entity(&shot.gui, |g, _| {
        g.hist.store.census.learn(cards);
        g.edition_entries().map_or(0, |e| e.len())
    });
    let _ = cx.update_window(window, |_, window, _| window.remove_window());
    games
}

/// One game's bar over `MockDaemon`'s fixture, a character picked, at
/// `width`, drawn `unfolded` as main drew it or folded as it must, after a
/// paint that folded it `folded` times (as a narrower window would have):
/// how far its last piece runs past the window (0 inside it), and how
/// often it folded.
fn one_game_at(
    cx: &mut gpui_kit::HeadlessAppContext,
    width: f32,
    unfolded: bool,
    folded: Option<usize>,
) -> (f32, usize) {
    use super::super::shots::Shot;
    super::super::top_bar::unfolded(unfolded);
    let path = own_config();
    std::fs::write(
        &path,
        format!(
            "zoom = 1.0\nhome_on_start = false\ntheme = \"{}\"\n",
            testkit::shots_theme().name
        ),
    )
    .expect("the config");
    let cfg = Config::load();
    let mock = MockDaemon::fixture()
        .with_characters(&[RETAIL_ME.to_string()])
        .with_history();
    let mut held = None;
    let (window, gui) = testkit::open_headless(cx, size(px(width), px(900.)), |window, cx| {
        let session = cx.new(|_| {
            Session::with_state(
                Box::new(MockLink::new(mock)),
                wowdps_proto::ClientState::new(),
            )
        });
        held = Some(session.clone());
        cx.new(|cx| Gui::new(session, cfg, window, cx))
    });
    let shot = Shot {
        window,
        gui,
        session: held.expect("the session was built"),
    };
    shot.settle(cx);
    if folded.is_some() {
        cx.update_entity(&shot.gui, |g, cx| {
            g.bar_fold.set(folded);
            cx.notify();
        });
    }
    let over = cx
        .update_window(window, |_, window, cx| {
            window.render_frame(cx);
            window.render_frame(cx);
            AFTER
                .iter()
                .filter_map(|id| window.try_find(*id).map(|e| f32::from(e.bounds().right())))
                .fold(0.0, f32::max)
                - width
        })
        .expect("the window");
    let fold = shot
        .gui
        .read_with(cx, |g, _| g.bar_fold.get().unwrap_or_default());
    let _ = cx.update_window(window, |_, window, _| window.remove_window());
    super::super::top_bar::unfolded(false);
    (over.max(0.0), fold)
}

/// With one game known the bar is main's wherever main's fit: it folds
/// only at a width where, drawn unfolded as main draws it, it runs past
/// the window — and there it folds to fit. Each swept width is held to the
/// unfolded bar drawn at it, and so are the widths at the unfolded bar's
/// least window, measured from its layout (its jump glyph's room taken
/// in), with its right padding whole and with its help button at the
/// edge. No fold holds: a window folded once is main's again at every
/// width where main's keeps every piece.
#[test]
#[cfg_attr(
    target_os = "macos",
    ignore = "gpui's mac platform must be created on the main thread"
)]
fn one_game_folds_only_where_its_bar_would_run_past() {
    crate::images::without_art(true);
    let mut cx = super::super::shots::app();
    // The unfolded narrow bar's least window, from its layout at 800 px
    // less the room its jump glyph leaves free after the places: `padded`
    // with its right padding whole, `edge` its help button at the window's
    // edge (main's bar gives up the padding first, every piece still in).
    let (edge, padded) = {
        super::super::top_bar::unfolded(true);
        let path = own_config();
        std::fs::write(&path, "zoom = 1.0\nhome_on_start = false\n").expect("the config");
        let cfg = Config::load();
        let mock = MockDaemon::fixture()
            .with_characters(&[RETAIL_ME.to_string()])
            .with_history();
        let (window, gui) =
            testkit::open_headless(&mut cx, size(px(800.), px(900.)), |window, cx| {
                let session = cx.new(|_| {
                    Session::with_state(
                        Box::new(MockLink::new(mock)),
                        wowdps_proto::ClientState::new(),
                    )
                });
                cx.new(|cx| Gui::new(session, cfg, window, cx))
            });
        let session = gui.read_with(&cx, |g, _| g.session().clone());
        for _ in 0..16 {
            if !cx.update_entity(&session, |s, cx| s.pump(cx)) {
                break;
            }
        }
        let (free, help) = cx
            .update_window(window, |_, window, cx| {
                window.render_frame(cx);
                window.render_frame(cx);
                let fights = window.find("place-fights").bounds();
                let jump = window.find("top-jump").bounds();
                let help = window.find("top-help").bounds();
                (
                    f32::from(jump.left()) - f32::from(fights.right()) - 2.0,
                    f32::from(help.right()),
                )
            })
            .expect("the window");
        let _ = cx.update_window(window, |_, window, _| window.remove_window());
        super::super::top_bar::unfolded(false);
        (help - free, 800.0 - free)
    };
    assert!(
        edge > 320.0 && edge < padded && padded < 800.0,
        "the unfolded bar's least window: {edge}, padded {padded}"
    );
    let mut widths: Vec<f32> = (8..=37).map(|w| w as f32 * 40.0).collect();
    widths.push(1500.0);
    for at in [edge, padded] {
        widths.extend([at.floor() - 1.0, at.floor(), at.ceil(), at.ceil() + 1.0]);
    }
    for width in widths {
        let (main, _) = one_game_at(&mut cx, width, true, None);
        let (over, fold) = one_game_at(&mut cx, width, false, None);
        if width == edge.floor() - 1.0 {
            assert!(
                main > 0.0,
                "{width}: main's bar fits below its least window"
            );
        }
        if main == 0.0 {
            assert_eq!(fold, 0, "{width}: main's bar fits, yet it folded");
            assert_eq!(over, 0.0, "{width}");
        } else {
            assert!(fold > 0, "{width}: main's bar runs past {main}, unfolded");
            assert_eq!(over, 0.0, "{width}: folded, it still runs past");
        }
    }
    // No fold holds: a bar folded as deep as it goes (a narrower window's)
    // is main's again at every width from its least window holding every
    // piece, through the room the switch's hysteresis would ask.
    let deepest = Some(super::super::top_bar::FOLDS);
    let mut width = edge.ceil();
    while width <= edge.ceil() + wowdps_gui_logic::editions::UNFOLD_SPARE + 1.0 {
        let (over, fold) = one_game_at(&mut cx, width, false, deepest);
        assert_eq!(
            (fold, over),
            (0, 0.0),
            "{width}: a fold held where main's fits"
        );
        width += 1.0;
    }
    crate::images::without_art(false);
}

/// Each of the switch's forms from the least width that draws it, in a
/// fresh window over two games, with the bar at its widest for a game
/// (the pill's words and the picker's name past their caps, a clock of
/// "88:88"'s digits: `top_bar::widest`): there every piece and the bar's
/// right padding are inside the window, nothing overlaps and, beside the
/// named chips, the search box is whole; a pixel narrower the switch takes
/// a poorer form. The form is chosen by the bar's measure, so this holds
/// only while the measure is the layout's: words measured a fraction
/// short of the width they are laid out at draw a form a pixel or more too
/// early, into the padding or the search box. In the narrow frame the
/// named dropdown and its badge alone; above it every form.
#[test]
#[cfg_attr(
    target_os = "macos",
    ignore = "gpui's mac platform must be created on the main thread"
)]
fn each_form_s_threshold_holds_the_bar_at_its_widest() {
    use wowdps_gui_logic::editions::Form;
    crate::images::without_art(true);
    super::super::top_bar::widest(true);
    let mut cx = super::super::shots::app();
    let mut at = |width: f32| {
        let link = EditionsLink::owned(&[RETAIL_ME, FOREVER_ME]).with_forever_source();
        link.playing.set(Some(Edition::Retail));
        bar_at(&mut cx, link, &[], width)
    };
    let rank = |d: &Drawn| d.form.map_or(usize::MAX, |f| f as usize);
    let narrow = wowdps_gui_logic::theme::NARROW_WINDOW;
    // Each form richer than the badge alone, and the frame it is reached
    // in: the named dropdown's in the narrow frame, every form's above it.
    let mut thresholds = vec![(Form::Dropdown, 320.0, narrow)];
    thresholds.extend(Form::ALL[..4].iter().map(|f| (*f, narrow + 1.0, 1600.0)));
    for (form, least, most) in thresholds {
        // The least width in the frame that draws this form or a richer.
        let (mut lo, mut hi) = (least, most);
        assert!(rank(&at(hi)) <= form as usize, "{form:?} at {hi}");
        assert!(rank(&at(lo)) > form as usize, "{form:?} at {lo}");
        while hi - lo > 1.0 {
            let mid = ((lo + hi) / 2.0).floor();
            if rank(&at(mid)) <= form as usize {
                hi = mid;
            } else {
                lo = mid;
            }
        }
        let drawn = at(hi);
        assert_eq!(drawn.form, Some(form), "at {hi}");
        for id in ["top-live", "top-picker", "top-help"] {
            assert!(drawn.shown.contains(&id), "{form:?} at {hi}: no {id}");
        }
        assert!(!drawn.overlap, "{form:?} at {hi}: two pieces overlap");
        let pad = hi - drawn.right;
        assert!(
            pad >= super::super::top_bar::PAD_RIGHT - 0.5,
            "{form:?} at {hi}: the bar runs {} px into its padding",
            super::super::top_bar::PAD_RIGHT - pad
        );
        if form == Form::Chips {
            assert!(
                drawn.jump >= 200.0 - 0.5,
                "at {hi}: the search box is {} px",
                drawn.jump
            );
        }
        assert!(
            rank(&at(hi - 1.0)) > form as usize,
            "{form:?}: {} draws it too",
            hi - 1.0
        );
    }
    super::super::top_bar::widest(false);
    crate::images::without_art(false);
}
