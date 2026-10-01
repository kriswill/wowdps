//! The rail's gestures through Kit's harness, over the daemon's mock with
//! a history store: what it lists, the drawer at 1180 px and under, its
//! keys, and `[` `]` across nights — paging the store past the last card
//! in hand.

use gpui_kit::test::TestWindowExt as _;
use gpui_kit::{AppContext, ElementId, TestAppContext, px, size};
use wowdps_daemon::mock::MockDaemon;
use wowdps_gui_logic::config::Config;
use wowdps_gui_logic::rail::samples::{H, card, day, night, owned};
use wowdps_gui_logic::rail::{Pull, night_of};
use wowdps_model::{Action, Class};
use wowdps_proto::history::{FightCard, FightKind};
use wowdps_proto::{ClientMsg, DaemonMsg, HistoryAnswer, HistoryQuery, Reconnect};

use super::super::tests::{Rig, config, own_config, settle};
use super::super::{Gui, Place};
use crate::keys::Do;
use crate::session::{Link, Session};
use crate::testkit::{self, MockLink};

/// The fixture's night: its log was written on the evening of 27 July.
pub(crate) fn fixture_night() -> i64 {
    night("2026-07-27")
}

/// The fixture's first player, whose the stored cards are.
pub(crate) const OWNER: &str = "Thraxx-Nebula-US";

/// The fixture's mock with every fight of it stored, owned by [`OWNER`].
pub(crate) fn stored_mock() -> MockDaemon {
    MockDaemon::fixture()
        .with_characters(&[OWNER.to_string()])
        .with_history()
}

/// The window `w` × `h` over `link`, settled, its "tonight" the fixture's.
pub(crate) fn rig_over(cx: &mut TestAppContext, link: impl Link, w: f32, h: f32) -> Rig {
    rig_with(cx, link, config(), w, h)
}

/// [`rig_over`] with a config of the test's.
pub(crate) fn rig_with(
    cx: &mut TestAppContext,
    link: impl Link,
    cfg: Config,
    w: f32,
    h: f32,
) -> Rig {
    own_config();
    cx.update(crate::keys::bind);
    let (window, gui) = testkit::open(cx, size(px(w), px(h)), move |window, cx| {
        let session = cx.new(|_| Session::new(link));
        let gui = cx.new(|cx| Gui::new(session, cfg, window, cx));
        let focus = gui.read(cx).focus().clone();
        window.focus(&focus, cx);
        gui
    });
    gui.update(cx, |g, _| g.hist.tonight_pin = Some(fixture_night()));
    let session = gui.read_with(cx, |g, _| g.session().clone());
    settle(cx, &session);
    Rig {
        window,
        gui,
        session,
    }
}

/// Press the element `id`, then let the session's answers land.
pub(crate) fn press(cx: &mut TestAppContext, rig: &Rig, id: impl Into<ElementId>) {
    let id = id.into();
    cx.update_window(rig.window, |_, window, cx| {
        window.render_frame(cx);
        window.click(id, cx);
    })
    .unwrap();
    settle(cx, &rig.session);
}

/// Send `action` as its key would, then let the answers land.
pub(crate) fn key(cx: &mut TestAppContext, rig: &Rig, action: Action) {
    cx.update_window(rig.window, |_, window, cx| {
        window.dispatch_action(Box::new(Do(action)), cx);
        window.render_frame(cx);
    })
    .unwrap();
    settle(cx, &rig.session);
}

/// Is element `id` on screen?
pub(crate) fn shown(cx: &mut TestAppContext, rig: &Rig, id: impl Into<ElementId>) -> bool {
    let id = id.into();
    cx.update_window(rig.window, |_, window, cx| {
        window.render_frame(cx);
        window.try_find(id).is_some()
    })
    .unwrap()
}

fn current(cx: &mut TestAppContext, rig: &Rig) -> Option<Pull> {
    rig.gui.read_with(cx, |g, cx| g.current_pull(cx))
}

/// The mock, but the store's pages are `cards`, `page` at a time — the
/// newest first, `total` all of them — so a test can walk past the cards
/// in hand.
struct PagedLink {
    inner: MockLink,
    cards: Vec<FightCard>,
    page: usize,
    inbox: Vec<DaemonMsg>,
    asked: u32,
}

impl Link for PagedLink {
    fn send(&mut self, msg: &ClientMsg) {
        if let ClientMsg::GetHistory {
            req_id,
            query: HistoryQuery::Fights {
                after_id, limit, ..
            },
        } = msg
        {
            self.asked += 1;
            let start = after_id
                .as_ref()
                .and_then(|a| self.cards.iter().position(|c| &c.id == a))
                .map_or(0, |p| p + 1);
            let cards = self
                .cards
                .iter()
                .skip(start)
                .take((*limit as usize).min(self.page))
                .cloned()
                .collect();
            self.inbox.push(DaemonMsg::History {
                req_id: *req_id,
                answer: HistoryAnswer::Fights {
                    cards,
                    total: self.cards.len() as u32,
                },
            });
            return;
        }
        self.inner.send(msg);
    }

    fn poll(&mut self) -> Vec<DaemonMsg> {
        let mut out = self.inner.poll();
        out.append(&mut self.inbox);
        out
    }

    fn reconnect(&mut self) -> Reconnect {
        Reconnect::Connected
    }
}

/// Three earlier nights of two boss pulls each, newest first, a page of
/// three at a time: the second page holds the oldest night and a half.
fn earlier_nights() -> PagedLink {
    let mut cards = Vec::new();
    for (n, date) in ["2026-07-20", "2026-07-13", "2026-07-06"]
        .iter()
        .enumerate()
    {
        for k in 0..2 {
            let start = day(date) + 21 * H - k as i64 * H;
            let id = format!("n{n}p{k}");
            cards.push(owned(
                card(&id, FightKind::Encounter, "Boss", start, 300_000),
                "Player-1-A",
                Class::Mage,
            ));
        }
    }
    PagedLink {
        inner: MockLink::fixture(),
        cards,
        page: 3,
        inbox: Vec::new(),
        asked: 0,
    }
}

#[gpui_kit::test]
fn the_rail_lists_nights_visits_and_pulls(cx: &mut TestAppContext) {
    let rig = rig_over(cx, MockLink::new(stored_mock()), 1440., 900.);
    let rail = rig.gui.read_with(cx, |g, cx| g.rail(cx));
    assert_eq!(rail.nights.len(), 1, "the fixture's one night");
    assert_eq!(rail.nights[0].label, "Tonight");
    let visit = &rail.nights[0].visits[0];
    assert!(
        visit.title.contains("Sepulcher of the Ashen Vow"),
        "{}",
        visit.title
    );
    // A stored card of the tailed log is listed once, as the log's.
    assert!(
        rail.lines().all(|l| matches!(l.pull, Pull::Log(_))),
        "{:?}",
        rail.lines().map(|l| &l.pull).collect::<Vec<_>>()
    );
    assert!(shown(cx, &rig, "rail"), "docked above 1180 px");
    assert!(shown(cx, &rig, ("night", 0usize)));
    assert!(shown(cx, &rig, ("pull", 0usize)));
    // The current pull is lit; a press on another opens it.
    let first = rail.lines().next().map(|l| l.pull.clone());
    let second = rail.lines().nth(1).map(|l| l.pull.clone());
    assert_eq!(current(cx, &rig), first, "the log's newest pull");
    press(cx, &rig, ("pull", 1usize));
    assert_eq!(current(cx, &rig), second);
}

#[gpui_kit::test]
fn hide_trash_hides_the_trash_but_never_the_pull_on_the_stage(cx: &mut TestAppContext) {
    let rig = rig_over(cx, MockLink::new(stored_mock()), 1440., 900.);
    let count = |cx: &mut TestAppContext| {
        rig.gui.read_with(cx, |g, cx| {
            let rail = g.rail(cx);
            rail.lines()
                .filter(|l| !(g.hist.rail.hide_trash && l.trash))
                .count()
        })
    };
    let all = count(cx);
    press(cx, &rig, "hide-trash");
    assert!(rig.gui.read_with(cx, |g, _| g.hist.rail.hide_trash));
    assert!(count(cx) < all, "the fixture's trash goes");
}

#[gpui_kit::test]
fn the_drawer_opens_and_closes_at_1180(cx: &mut TestAppContext) {
    let rig = rig_over(cx, MockLink::new(stored_mock()), 1180., 880.);
    assert!(!shown(cx, &rig, "rail"), "no docked rail at 1180 px");
    assert!(!shown(cx, &rig, "rail-drawer"));
    press(cx, &rig, "rail-button");
    assert!(shown(cx, &rig, "rail-drawer"), "the header's list button");
    press(cx, &rig, "rail-scrim");
    assert!(!shown(cx, &rig, "rail-drawer"), "the scrim closes it");
    press(cx, &rig, "rail-button");
    key(cx, &rig, Action::Back);
    assert!(!shown(cx, &rig, "rail-drawer"), "Esc closes it");
    // One pixel wider, the rail is docked and there is no drawer.
    let wide = rig_over(cx, MockLink::new(stored_mock()), 1181., 880.);
    assert!(shown(cx, &wide, "rail"));
}

#[gpui_kit::test]
fn the_drawer_s_keys_walk_its_rows_and_enter_opens_one(cx: &mut TestAppContext) {
    let rig = rig_over(cx, MockLink::new(stored_mock()), 960., 880.);
    let lines: Vec<Pull> = rig.gui.read_with(cx, |g, cx| {
        g.rail(cx).lines().map(|l| l.pull.clone()).collect()
    });
    assert!(lines.len() >= 3, "{lines:?}");
    press(cx, &rig, "rail-button");
    key(cx, &rig, Action::Down);
    key(cx, &rig, Action::Down);
    key(cx, &rig, Action::Up);
    rig.gui.read_with(cx, |g, cx| {
        assert_eq!(g.hist.rail.cursor.as_ref(), Some(&lines[1]));
        assert_eq!(
            g.current_pull(cx).as_ref(),
            Some(&lines[0]),
            "not opened yet"
        );
    });
    key(cx, &rig, Action::Open);
    assert_eq!(current(cx, &rig).as_ref(), Some(&lines[1]));
    assert!(!shown(cx, &rig, "rail-drawer"), "Enter opens and closes");
    // `]` with the drawer open steps the stage and leaves it open.
    press(cx, &rig, "rail-button");
    key(cx, &rig, Action::NewerSegment);
    assert_eq!(current(cx, &rig).as_ref(), Some(&lines[0]));
    assert!(shown(cx, &rig, "rail-drawer"));
}

#[gpui_kit::test]
fn brackets_step_across_nights_and_page_the_store(cx: &mut TestAppContext) {
    let rig = rig_over(cx, earlier_nights(), 1440., 900.);
    rig.gui.read_with(cx, |g, _| {
        assert_eq!(g.hist.store.earlier.cards.len(), 3, "the first page");
        assert!(g.hist.store.earlier.more());
    });
    // Down the log's night to the stored nights under it.
    let mut steps = 0;
    while !matches!(current(cx, &rig), Some(Pull::Stored(_))) {
        key(cx, &rig, Action::OlderSegment);
        steps += 1;
        assert!(steps < 20, "never reached the stored nights");
    }
    let id = |cx: &mut TestAppContext| match current(cx, &rig) {
        Some(Pull::Stored(id)) => id,
        other => panic!("{other:?}"),
    };
    assert_eq!(id(cx), "n0p0", "the newest stored pull after the log's");
    key(cx, &rig, Action::OlderSegment);
    key(cx, &rig, Action::OlderSegment);
    assert_eq!(id(cx), "n1p0", "across a night");
    // The last card in hand: `[` asks for the next page and stays.
    key(cx, &rig, Action::OlderSegment);
    assert_eq!(id(cx), "n1p0");
    rig.gui.read_with(cx, |g, cx| {
        assert_eq!(g.hist.store.earlier.cards.len(), 6, "the second page");
        let rail = g.rail(cx);
        let n1 = rail.night_of_pull(&Pull::Stored("n1p0".into()));
        let n2 = rail.night_of_pull(&Pull::Stored("n2p0".into()));
        assert!(n1.is_some() && n2 > n1, "the oldest night is listed");
        assert_eq!(
            rail.nights[n2.unwrap_or_default()].label,
            "Monday, Jul 6",
            "a night by its local date"
        );
    });
    key(cx, &rig, Action::OlderSegment);
    assert_eq!(id(cx), "n1p1");
    key(cx, &rig, Action::OlderSegment);
    assert_eq!(id(cx), "n2p0");
    key(cx, &rig, Action::NewerSegment);
    assert_eq!(id(cx), "n1p1", "`]` back up");
    assert_eq!(night_of(day("2026-07-06") + 21 * H), night("2026-07-06"));
}

#[gpui_kit::test]
fn show_older_nights_asks_for_the_next_page(cx: &mut TestAppContext) {
    let rig = rig_over(cx, earlier_nights(), 1440., 900.);
    assert!(shown(cx, &rig, "older-nights"));
    press(cx, &rig, "older-nights");
    rig.gui.read_with(cx, |g, _| {
        assert_eq!(g.hist.store.earlier.cards.len(), 6);
        assert!(!g.hist.store.earlier.more(), "the store is all in hand");
    });
    assert!(!shown(cx, &rig, "older-nights"), "no more to ask for");
}

#[gpui_kit::test]
fn the_live_pill_leaves_a_stored_pull_for_the_log(cx: &mut TestAppContext) {
    let rig = rig_over(cx, earlier_nights(), 1440., 900.);
    while !matches!(current(cx, &rig), Some(Pull::Stored(_))) {
        key(cx, &rig, Action::OlderSegment);
    }
    cx.update_window(rig.window, |_, window, cx| {
        rig.gui.update(cx, |g, cx| g.go_live(window, cx));
    })
    .unwrap();
    settle(cx, &rig.session);
    assert!(matches!(current(cx, &rig), Some(Pull::Log(_))));
    rig.gui.read_with(cx, |g, _| {
        assert!(g.hist.store.stored.is_none());
        assert_eq!(g.place, Place::Fights);
    });
}
