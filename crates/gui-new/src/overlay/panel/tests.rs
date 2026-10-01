//! The overlay's tests: gestures through Kit's harness over the daemon's
//! mock, and `overlay_shots`, which photographs each state for review
//! beside the iced guard's pictures (the guard proper is step 2.6).

use std::cell::Cell;

use gpui_kit::test::TestWindowExt as _;
use gpui_kit::{
    AnyWindowHandle, AppContext, Entity, HeadlessAppContext, Pixels, Size, TestAppContext, px, size,
};
use wowdps_gui_logic::config::Config;
use wowdps_gui_logic::timeline;
use wowdps_model::{Action, View};

use super::Overlay;
use crate::session::Session;
use crate::testkit::{self, MockLink};

/// The panel's default size at zoom 1.25.
pub(crate) const PANEL: (f32, f32) = (410., 460.);

thread_local! {
    static CONFIGS: Cell<u32> = const { Cell::new(0) };
}

/// A config file of the test's own, so a gesture that remembers a setting
/// never writes the user's.
fn own_config() {
    let n = CONFIGS.with(|c| {
        c.set(c.get() + 1);
        c.get()
    });
    let dir = std::env::temp_dir().join(format!(
        "wowdps-gui-new-overlay-{}-{:?}-{n}",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = std::fs::create_dir_all(&dir);
    Config::use_path_on_this_thread(Some(dir.join("config.toml")));
}

/// The overlay's config in a test: Hyprland following off, the rest the
/// defaults (zoom 1.25, a 410 × 460 panel, ranks shown, the right edge).
pub(crate) fn config() -> Config {
    Config {
        follow_game: false,
        ..Config::default()
    }
}

/// An overlay over the fixture's mock, expanded, with its session.
pub(crate) struct Rig {
    pub window: AnyWindowHandle,
    pub overlay: Entity<Overlay>,
    pub session: Entity<Session>,
}

/// A fixture's daemon: `sample.txt`'s ([`MockLink::fixture`]) or one of
/// core's other fixtures.
type Link = fn() -> MockLink;

fn fixture(name: &str) -> MockLink {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../core/fixtures")
        .join(name);
    MockLink::new(wowdps_daemon::mock::MockDaemon::fixture_at(&path))
}

/// R26's fixture: a Destruction Warlock whose abilities group.
fn tree_link() -> MockLink {
    fixture("tree.txt")
}

/// R17's fixture: a tank taking hits, every miss kind.
fn taken_link() -> MockLink {
    fixture("taken.txt")
}

/// R13's fixture: arena matches, the teams split.
fn arena_link() -> MockLink {
    fixture("arena.txt")
}

/// `sample.txt` mid-pull: its last fight still open.
fn live_link() -> MockLink {
    MockLink::new(wowdps_daemon::mock::MockDaemon::fixture_live())
}

fn build(cx: &mut gpui_kit::App, link: Link) -> Entity<Overlay> {
    let session = cx.new(|_| Session::new(link()));
    // The Σ split's connection: its own mock over the same fixture, as the
    // daemon's second session sees the same log.
    let split: super::AuxMaker = std::rc::Rc::new(move |cx| {
        let state = wowdps_proto::ClientState::with_top_n(Some(super::AUX_TOP_N));
        Ok(cx.new(|_| Session::with_state(Box::new(link()), state)))
    });
    cx.new(|cx| {
        let mut overlay = Overlay::new(session, config(), split, cx);
        overlay.expanded = true;
        overlay
    })
}

/// Pump the session until nothing more arrives: what the daemon would
/// answer, settled.
pub(crate) fn settle<C: AppContext>(cx: &mut C, session: &Entity<Session>) {
    for _ in 0..12 {
        if !cx.update_entity(session, |s, cx| s.pump(cx)) {
            break;
        }
    }
}

/// Settle the overlay's session and the Σ split's (made on first want),
/// until neither hears anything more.
pub(crate) fn settle_all<C: AppContext>(cx: &mut C, rig: &Rig) {
    for _ in 0..12 {
        let main = cx.update_entity(&rig.session, |s, cx| s.pump(cx));
        let aux = cx.read_entity(&rig.overlay, |o, _| o.aux.clone());
        let split = aux.is_some_and(|a| cx.update_entity(&a, |s, cx| s.pump(cx)));
        if !(main || split) {
            break;
        }
    }
}

/// Apply a meter action through the session, as a gesture would, and
/// settle.
pub(crate) fn apply<C: AppContext>(cx: &mut C, rig: &Rig, action: Action) {
    cx.update_entity(&rig.session, |s, cx| s.act(|st| st.apply(action), cx));
    settle_all(cx, rig);
}

/// The guard's "kill" state: the fixture's newest pull opened, then two
/// older — "The Ashen Warden", a kill inside the fixture's one raid visit.
fn to_the_kill<C: AppContext>(cx: &mut C, rig: &Rig) {
    settle(cx, &rig.session);
    apply(cx, rig, Action::OlderSegment);
    apply(cx, rig, Action::OlderSegment);
}

/// The overlay over `link`'s fixture, settled on its newest pull.
fn rig_on(cx: &mut TestAppContext, link: Link) -> Rig {
    own_config();
    let (window, overlay) =
        testkit::open(cx, size(px(PANEL.0), px(PANEL.1)), |_, cx| build(cx, link));
    let session = overlay.read_with(cx, |o, _| o.session().clone());
    let rig = Rig {
        window,
        overlay,
        session,
    };
    settle_all(cx, &rig);
    rig
}

pub(crate) fn kill(cx: &mut TestAppContext) -> Rig {
    let rig = rig_on(cx, MockLink::fixture);
    to_the_kill(cx, &rig);
    rig
}

/// The kill's top row drilled open.
fn drilled<C: AppContext>(cx: &mut C, rig: &Rig) {
    to_the_kill(cx, rig);
    apply(cx, rig, Action::Open);
}

/// R26: `tree.txt`'s boss kill drilled into its Warlock, whose abilities
/// group — Wither with its proc, two pets under their summons, two
/// trinkets standing alone.
fn tree_drilled<C: AppContext>(cx: &mut C, rig: &Rig) {
    settle_all(cx, rig);
    for _ in 0..4 {
        let name = cx.read_entity(&rig.session, |s, _| s.state().segment_name());
        if name.as_deref() == Some("Tree Test Boss") {
            break;
        }
        apply(cx, rig, Action::OlderSegment);
    }
    cx.update_entity(&rig.session, |s, cx| {
        s.act(
            |st| {
                let i = st
                    .rows()
                    .iter()
                    .position(|r| r.label.starts_with("Vexxa"))
                    .expect("the Warlock has a row");
                st.select_row(i)
            },
            cx,
        )
    });
    apply(cx, rig, Action::Open);
}

/// R17: `taken.txt`'s boss on the Taken view, its tank drilled.
fn taken_drilled<C: AppContext>(cx: &mut C, rig: &Rig) {
    settle_all(cx, rig);
    apply(cx, rig, Action::OlderSegment);
    apply(cx, rig, Action::SetView(View::Taken));
    apply(cx, rig, Action::Open);
}

/// R9: the kill's one death, its recap open.
fn deaths_drilled<C: AppContext>(cx: &mut C, rig: &Rig) {
    to_the_kill(cx, rig);
    apply(cx, rig, Action::SetView(View::Deaths));
    apply(cx, rig, Action::Open);
}

/// v16: the kill's top row drilled, then its top ability.
fn spell_drilled<C: AppContext>(cx: &mut C, rig: &Rig) {
    drilled(cx, rig);
    apply(cx, rig, Action::Open);
}

/// R24: the kill's top enemy drilled — its attackers.
fn enemies_drilled<C: AppContext>(cx: &mut C, rig: &Rig) {
    to_the_kill(cx, rig);
    apply(cx, rig, Action::SetView(View::EnemyTaken));
    apply(cx, rig, Action::Open);
}

/// R13: the first arena match (newest first) whose meter splits the teams.
fn arena_split<C: AppContext>(cx: &mut C, rig: &Rig) {
    settle_all(cx, rig);
    for _ in 0..8 {
        let split = cx.read_entity(&rig.session, |s, _| {
            wowdps_gui_logic::table::enemy_split(&s.state().rows()).is_some()
        });
        if split {
            return;
        }
        apply(cx, rig, Action::OlderSegment);
    }
    panic!("no match in arena.txt splits the teams");
}

fn drill_state<C: AppContext>(cx: &mut C, rig: &Rig) -> (bool, Option<String>) {
    cx.read_entity(&rig.session, |s, _| {
        let st = s.state();
        (st.drill.is_some(), st.drill_spell().map(|(_, l)| l.clone()))
    })
}

fn segment_name(cx: &mut TestAppContext, rig: &Rig) -> Option<String> {
    rig.session.read_with(cx, |s, _| s.state().segment_name())
}

#[gpui_kit::test]
fn the_overlay_opens_on_the_newest_pull_and_steps_to_the_kill(cx: &mut TestAppContext) {
    let rig = kill(cx);
    assert_eq!(segment_name(cx, &rig).as_deref(), Some("The Ashen Warden"));
    rig.session.read_with(cx, |s, _| {
        assert!(!s.state().following_live(), "parked on an older pull");
        assert_eq!(s.state().view, View::Damage);
    });
}

#[gpui_kit::test]
fn the_view_name_cycles_and_its_menu_picks(cx: &mut TestAppContext) {
    let rig = kill(cx);
    cx.update_window(rig.window, |_, window, cx| {
        window.render_frame(cx);
        window.click("view-name", cx);
    })
    .unwrap();
    settle(cx, &rig.session);
    let view = |cx: &mut TestAppContext| rig.session.read_with(cx, |s, _| s.state().view);
    assert_eq!(view(cx), View::Healing, "a left press cycles");

    cx.update_window(rig.window, |_, window, cx| {
        window.render_frame(cx);
        window.right_click("view-name", cx);
        window.render_frame(cx);
        assert!(
            window.try_find("view-menu").is_some(),
            "a right press opens the menu"
        );
        window.click(
            gpui_kit::ElementId::from((
                gpui_kit::ElementId::Name("view".into()),
                gpui_kit::SharedString::from("Deaths"),
            )),
            cx,
        );
        window.render_frame(cx);
        assert!(window.try_find("view-menu").is_none(), "a pick closes it");
    })
    .unwrap();
    settle(cx, &rig.session);
    assert_eq!(view(cx), View::Deaths);
}

#[gpui_kit::test]
fn the_options_card_toggles_ranks_and_remembers_them(cx: &mut TestAppContext) {
    let rig = kill(cx);
    cx.update_window(rig.window, |_, window, cx| {
        window.render_frame(cx);
        window.click("options", cx);
        window.render_frame(cx);
        assert_eq!(window.find("row-ranks").selected(), Some(true));
        window.click("row-ranks", cx);
        window.render_frame(cx);
        assert_eq!(window.find("row-ranks").selected(), Some(false));
    })
    .unwrap();
    rig.overlay.read_with(cx, |o, _| assert!(!o.cfg.show_ranks));
    assert!(!Config::load().show_ranks, "remembered, by itself, on disk");
}

#[gpui_kit::test]
fn a_row_opens_its_drill_and_a_badge_picks_its_player(cx: &mut TestAppContext) {
    let rig = kill(cx);
    let keys: Vec<String> = rig.session.read_with(cx, |s, _| {
        s.state().rows().into_iter().map(|r| r.key).collect()
    });
    cx.update_window(rig.window, |_, window, cx| {
        window.render_frame(cx);
        window.click(
            gpui_kit::ElementId::from((
                gpui_kit::ElementId::Name("pick".into()),
                gpui_kit::SharedString::from(keys[1].clone()),
            )),
            cx,
        );
    })
    .unwrap();
    settle(cx, &rig.session);
    rig.session.read_with(cx, |s, _| {
        assert_eq!(s.state().compare_slot(&keys[1]), Some(0), "the badge picks");
        assert!(s.state().drill.is_none(), "and opens nothing");
    });

    cx.update_window(rig.window, |_, window, cx| {
        window.render_frame(cx);
        window.click(crate::meter::row_id(&keys[0]), cx);
    })
    .unwrap();
    settle(cx, &rig.session);
    rig.session.read_with(cx, |s, _| {
        let drill = s.state().drill.as_ref().expect("the row opens its drill");
        assert_eq!(drill.key, keys[0]);
    });
}

#[gpui_kit::test]
fn the_header_collapses_the_panel_and_the_tab_expands_it(cx: &mut TestAppContext) {
    let rig = kill(cx);
    let tab = Size {
        width: px(33.),
        height: px(120.),
    };
    cx.update_window(rig.window, |_, window, cx| {
        window.render_frame(cx);
        window.click("header", cx);
        window.render_frame(cx);
        assert!(window.try_find("tab").is_some(), "collapsed to the tab");
        assert_eq!(window.bounds().size, tab, "the surface shrinks to it");
        window.click("tab", cx);
        window.render_frame(cx);
        assert!(window.try_find("header").is_some(), "expanded again");
    })
    .unwrap();
}

/// The watched visit's scrub order (Σ, then members oldest first) and the
/// watched position.
fn visit(cx: &mut TestAppContext, rig: &Rig) -> (Vec<usize>, Option<usize>) {
    rig.session.read_with(cx, |s, _| {
        let state = s.state();
        let pos = timeline::watched_pos(state);
        let blocks = timeline::blocks(state.entries());
        let block = pos
            .and_then(|p| timeline::block_of(&blocks, p))
            .and_then(|b| blocks.get(b))
            .expect("the kill is in a visit");
        let order = block
            .overall
            .into_iter()
            .chain(block.members.clone())
            .collect();
        (order, pos)
    })
}

#[gpui_kit::test]
fn the_strip_goes_to_its_pull_and_the_chip_and_wheel_scrub_the_visit(cx: &mut TestAppContext) {
    let rig = kill(cx);
    let (order, _) = visit(cx, &rig);
    assert!(order.len() >= 4, "a Σ and at least three members");

    let press = |cx: &mut TestAppContext, id: gpui_kit::ElementId| {
        cx.update_window(rig.window, |_, window, cx| {
            window.render_frame(cx);
            window.click(id, cx);
        })
        .unwrap();
        settle_all(cx, &rig);
    };
    press(cx, ("strip", 0usize).into());
    assert_eq!(visit(cx, &rig).1, Some(order[0]), "the strip's Σ");
    press(cx, "chip-next".into());
    assert_eq!(
        visit(cx, &rig).1,
        Some(order[1]),
        "› steps to the oldest member"
    );
    press(cx, "chip-next".into());
    press(cx, "chip-prev".into());
    assert_eq!(visit(cx, &rig).1, Some(order[1]), "‹ steps back");

    let wheel = |cx: &mut TestAppContext, y: f32| {
        cx.update_window(rig.window, |_, window, cx| {
            window.render_frame(cx);
            window.scroll(
                "strip",
                gpui_kit::ScrollDelta::Lines(gpui_kit::point(0., y)),
                cx,
            );
        })
        .unwrap();
        settle_all(cx, &rig);
    };
    wheel(cx, -2.0);
    assert_eq!(visit(cx, &rig).1, Some(order[3]), "down is newer");
    wheel(cx, 0.5);
    wheel(cx, 0.5);
    assert_eq!(
        visit(cx, &rig).1,
        Some(order[2]),
        "half notches add up to one, up and older"
    );
}

#[gpui_kit::test]
fn the_footer_sigma_splits_the_visits_overall_under_the_fight(cx: &mut TestAppContext) {
    let rig = kill(cx);
    cx.update_window(rig.window, |_, window, cx| {
        window.render_frame(cx);
        assert_eq!(window.find("split").selected(), Some(false));
        window.click("split", cx);
    })
    .unwrap();
    settle_all(cx, &rig);
    let (order, _) = visit(cx, &rig);
    let sigma = rig
        .session
        .read_with(cx, |s, _| s.state().entries()[order[0]].id);
    rig.overlay.read_with(cx, |o, cx| {
        assert!(o.split && o.cfg.overlay_split);
        assert_eq!(
            o.aux_watch,
            Some((sigma, View::Damage)),
            "the split watches the Σ"
        );
        let (rows, _) = o.aux_rows(cx).expect("the split has the Σ's rows");
        assert!(!rows.is_empty() && rows.len() <= super::AUX_TOP_N as usize);
    });
    assert!(
        Config::load().overlay_split,
        "remembered, by itself, on disk"
    );

    // A view switch re-points the split; watching the Σ itself wants none.
    apply(cx, &rig, Action::SetView(View::Healing));
    rig.overlay.read_with(cx, |o, _| {
        assert_eq!(o.aux_watch, Some((sigma, View::Healing)))
    });
    cx.update_window(rig.window, |_, window, cx| {
        window.render_frame(cx);
        window.click(("strip", 0usize), cx);
    })
    .unwrap();
    settle_all(cx, &rig);
    rig.overlay.read_with(cx, |o, cx| {
        assert_eq!(o.aux_watch, None, "nothing to repeat under the Σ");
        assert!(o.aux_rows(cx).is_none());
    });
}

fn press_in(cx: &mut TestAppContext, rig: &Rig, id: impl Into<gpui_kit::ElementId>, right: bool) {
    let id = id.into();
    cx.update_window(rig.window, |_, window, cx| {
        window.render_frame(cx);
        if right {
            window.right_click(id, cx);
        } else {
            window.click(id, cx);
        }
    })
    .unwrap();
    settle_all(cx, rig);
}

#[gpui_kit::test]
fn a_drill_descends_into_an_ability_and_a_right_press_backs_out_one_level(cx: &mut TestAppContext) {
    let rig = kill(cx);
    apply(cx, &rig, Action::Open);
    cx.update_window(rig.window, |_, window, cx| {
        window.render_frame(cx);
        assert!(
            window.try_find("drill-graph").is_some(),
            "the drill's graph"
        );
        assert!(window.try_find("graph-mode").is_some(), "and its mode");
    })
    .unwrap();
    press_in(cx, &rig, ("drill-line", 0usize), false);
    assert!(drill_state(cx, &rig).1.is_some(), "an ability is open");

    press_in(cx, &rig, "body", true);
    assert_eq!(drill_state(cx, &rig), (true, None), "the ability closes");
    press_in(cx, &rig, "body", true);
    assert!(!drill_state(cx, &rig).0, "then the drill");
    press_in(cx, &rig, "body", true);
    rig.session.read_with(cx, |s, _| {
        assert_eq!(
            s.state().screen,
            wowdps_model::Screen::Meter,
            "never past it"
        )
    });
}

#[gpui_kit::test]
fn the_drill_graph_zooms_on_a_drag_and_a_right_press_resets_it(cx: &mut TestAppContext) {
    let rig = kill(cx);
    apply(cx, &rig, Action::Open);
    let range = |cx: &mut TestAppContext| rig.session.read_with(cx, |s, _| s.state().drill_range());
    cx.update_window(rig.window, |_, window, cx| {
        window.render_frame(cx);
        let b = window.find("drill-graph").bounds();
        let y = b.origin.y + px(40.);
        window.drag(
            gpui_kit::point(b.origin.x + px(40.), y),
            gpui_kit::point(b.origin.x + px(200.), y),
            cx,
        );
    })
    .unwrap();
    settle_all(cx, &rig);
    let (lo, hi) = range(cx).expect("a drag zooms");
    assert!(lo < hi);

    press_in(cx, &rig, "drill-graph", true);
    assert_eq!(range(cx), None, "a right press on the graph resets");
    assert!(drill_state(cx, &rig).0, "and never backs out of the drill");

    let mode = |cx: &mut TestAppContext| rig.session.read_with(cx, |s, _| s.state().graph_mode());
    let before = mode(cx);
    press_in(cx, &rig, "graph-mode", false);
    assert_ne!(mode(cx), before, "the footer toggles the curve");
}

#[gpui_kit::test]
fn a_tree_drill_folds_its_groups_and_opens_its_abilities(cx: &mut TestAppContext) {
    let rig = rig_on(cx, tree_link);
    tree_drilled(cx, &rig);
    let lines = |cx: &mut TestAppContext| {
        let open = rig.overlay.read_with(cx, |o, _| o.tree_open.clone());
        rig.session.read_with(cx, |s, _| {
            let st = s.state();
            wowdps_gui_logic::tree::lines(&st.breakdown().0, &st.drill_tree(), &open, None)
        })
    };
    let group = lines(cx)
        .iter()
        .position(|l| matches!(l.node, wowdps_gui_logic::tree::Node::Group(_)))
        .expect("the Warlock's drill groups");
    let shut = lines(cx).len();
    press_in(cx, &rig, ("drill-line", group), false);
    assert!(lines(cx).len() > shut, "a press opens the group");
    assert_eq!(drill_state(cx, &rig).1, None, "and opens no ability");

    let row = lines(cx)
        .iter()
        .position(|l| l.depth > 0 && l.opens.is_some())
        .expect("a row under the group");
    press_in(cx, &rig, ("drill-line", row), false);
    assert!(drill_state(cx, &rig).1.is_some(), "a row opens its ability");
}

#[gpui_kit::test]
fn an_enemys_drill_lists_its_attackers_and_one_opens_its_abilities(cx: &mut TestAppContext) {
    let rig = kill(cx);
    apply(cx, &rig, Action::SetView(View::EnemyTaken));
    apply(cx, &rig, Action::Open);
    let attackers = rig
        .session
        .read_with(cx, |s, _| s.state().breakdown().1.len());
    assert!(attackers > 0, "the enemy has attackers");
    press_in(cx, &rig, ("drill-line", 0usize), false);
    assert!(drill_state(cx, &rig).1.is_some(), "an attacker opens");
}

// ---- pictures for review --------------------------------------------------------

/// What the iced guard composites the transparent overlay over: iced's
/// TokyoNight ground. The real overlay sits over the game; the pictures
/// sit over this, so the two GUIs' are compared on one backdrop.
struct Backdrop(Entity<Overlay>);

impl gpui_kit::Render for Backdrop {
    fn render(
        &mut self,
        _: &mut gpui_kit::Window,
        _: &mut gpui_kit::Context<Self>,
    ) -> impl gpui_kit::IntoElement {
        use gpui_kit::{ParentElement as _, Styled as _};
        gpui_kit::div()
            .size_full()
            .bg(gpui_kit::rgb(0x1A1B26))
            .child(self.0.clone())
    }
}

/// How a picture's state is reached from a fresh overlay over its fixture.
type Reach = fn(&mut HeadlessAppContext, &Rig);

/// One picture: the iced guard's name, the fixture, how it is reached, and
/// the surface size the iced overlay would have.
struct Shot {
    name: &'static str,
    link: Link,
    reach: Reach,
    at: Size<Pixels>,
}

fn pose(cx: &mut HeadlessAppContext, rig: &Rig, f: impl FnOnce(&mut Overlay)) {
    cx.update_entity(&rig.overlay, |o, cx| {
        f(o);
        cx.notify();
    });
}

/// Every state the overlay draws so far, by the iced guard's names.
fn states() -> Vec<Shot> {
    let panel = size(px(PANEL.0), px(PANEL.1));
    let shot = |name, link: Link, reach: Reach| Shot {
        name,
        link,
        reach,
        at: panel,
    };
    let kill: Link = MockLink::fixture;
    vec![
        shot("meter", kill, to_the_kill),
        shot("hover", kill, |cx, rig| {
            to_the_kill(cx, rig);
            pose(cx, rig, |o| o.row_hover = Some(1));
        }),
        shot("options", kill, |cx, rig| {
            to_the_kill(cx, rig);
            pose(cx, rig, |o| o.options_open = true);
        }),
        shot("view-menu", kill, |cx, rig| {
            to_the_kill(cx, rig);
            pose(cx, rig, |o| {
                o.view_menu = true;
                o.view_hover = Some(View::Deaths);
            });
        }),
        shot("enemies", kill, |cx, rig| {
            to_the_kill(cx, rig);
            apply(cx, rig, Action::SetView(View::EnemyTaken));
        }),
        shot("interrupts", kill, |cx, rig| {
            to_the_kill(cx, rig);
            apply(cx, rig, Action::SetView(View::Interrupts));
        }),
        shot("split", kill, |cx, rig| {
            to_the_kill(cx, rig);
            cx.update_entity(&rig.overlay, |o, cx| {
                o.split = true;
                o.sync_aux(cx);
                cx.notify();
            });
            settle_all(cx, rig);
        }),
        shot("live", live_link, settle_all),
        shot("arena", arena_link, arena_split),
        shot("drill", kill, drilled),
        shot("drill-hover", kill, |cx, rig| {
            drilled(cx, rig);
            pose(cx, rig, |o| o.row_hover = Some(0));
        }),
        shot("spell-drill", kill, spell_drilled),
        shot("taken-drill", taken_link, taken_drilled),
        shot("deaths-drill", kill, deaths_drilled),
        shot("enemies-drill", kill, enemies_drilled),
        shot("tree-drill", tree_link, tree_drilled),
        shot("tree-drill-open", tree_link, |cx, rig| {
            tree_drilled(cx, rig);
            let keys = cx.read_entity(&rig.session, |s, _| {
                let st = s.state();
                let all = wowdps_gui_logic::tree::lines(
                    &st.breakdown().0,
                    &st.drill_tree(),
                    &Default::default(),
                    None,
                );
                all.into_iter()
                    .filter(|l| matches!(l.node, wowdps_gui_logic::tree::Node::Group(_)))
                    .filter_map(|l| l.fold_key)
                    .collect::<Vec<_>>()
            });
            assert!(!keys.is_empty(), "the drill has groups to open");
            pose(cx, rig, |o| o.tree_open.extend(keys));
        }),
        Shot {
            // Four notches up, the panel grown with it: an off-grid zoom,
            // so every size the panel derives from it is fractional.
            name: "zoomed",
            link: kill,
            reach: |cx, rig| {
                to_the_kill(cx, rig);
                pose(cx, rig, |o| {
                    let ratio = (o.cfg.zoom + 0.2) / o.cfg.zoom;
                    o.cfg.zoom += 0.2;
                    o.cfg.width = (o.cfg.width as f32 * ratio).round() as u32;
                    o.cfg.height = (o.cfg.height as f32 * ratio).round() as u32;
                });
            },
            at: size(
                px((PANEL.0 * 1.45 / 1.25).round()),
                px((PANEL.1 * 1.45 / 1.25).round()),
            ),
        },
        Shot {
            name: "collapsed",
            link: kill,
            reach: |cx, rig| {
                to_the_kill(cx, rig);
                pose(cx, rig, |o| o.expanded = false);
            },
            at: size(px(33.), px(120.)),
        },
    ]
}

/// The overlay's states as pictures, for review beside the iced guard's
/// (`WOWDPS_GUARD_PNG` there, `WOWDPS_SHOTS_DIR` here). Run by hand:
/// `WOWDPS_SHOTS_DIR=/tmp/s cargo test -p wowdps-gui-new overlay_shots -- --ignored`.
#[test]
#[ignore = "needs a wgpu adapter"]
fn overlay_shots() {
    let dir = std::env::var_os("WOWDPS_SHOTS_DIR").map(std::path::PathBuf::from);
    let only = std::env::var("WOWDPS_SHOTS_ONLY").ok();
    for Shot {
        name,
        link,
        reach,
        at,
    } in states()
    {
        if only
            .as_deref()
            .is_some_and(|o| !o.split(',').any(|n| n == name))
        {
            continue;
        }
        own_config();
        let mut cx = testkit::headless();
        cx.update(|cx| cx.set_reduce_motion(true));
        cx.update(|cx| {
            let faces = wowdps_gui_logic::fonts::FONTS
                .iter()
                .map(|b| std::borrow::Cow::Borrowed(*b))
                .collect();
            let _ = cx.text_system().add_fonts(faces);
        });
        let mut held = None;
        let (window, _) = testkit::open_headless(&mut cx, at, |_, cx| {
            let overlay = build(cx, link);
            held = Some(overlay.clone());
            cx.new(|_| Backdrop(overlay))
        });
        let overlay = held.expect("the overlay was built");
        let session = cx.update(|cx| overlay.read(cx).session().clone());
        let rig = Rig {
            window,
            overlay,
            session,
        };
        reach(&mut cx, &rig);
        cx.run_until_parked();
        let shot = cx.capture_screenshot(window).expect("a headless renderer");
        if let Some(dir) = &dir {
            let path = dir.join(format!("overlay-{name}.png"));
            shot.save(&path).expect("png written");
            eprintln!("overlay_shots: {}", path.display());
        }
    }
}
