//! The inspector's gestures through Kit's harness: its keys (the tree's
//! walk, Enter, ← →, Tab), its presses (a fold's caret, a line, a tab, an
//! action) and its scroll keeping the keyed line in sight.

use gpui_kit::test::TestWindowExt as _;
use gpui_kit::{AppContext as _, ElementId, TestAppContext};
use wowdps_gui_logic::raid::raided_deaths;
use wowdps_gui_logic::tree::{self, Node};
use wowdps_model::{Action, Pane};

use super::model;
use crate::testkit::MockLink;
use crate::window::tests::{Rig, keys, press, rig_on, rig_over, settle};

/// R26's fixture, the Warlock's drill on the stage.
fn tree_rig(cx: &mut TestAppContext, w: f32, h: f32) -> Rig {
    let rig = rig_on(cx, w, h, MockLink::at("tree.txt"));
    // The newest pull is trash after the boss: step back onto the boss.
    for _ in 0..4 {
        let name = rig.session.read_with(cx, |s, _| s.state().segment_name());
        if name.as_deref() == Some("Tree Test Boss") {
            break;
        }
        rig.session.update(cx, |s, cx| {
            s.act(|st| st.apply(Action::OlderSegment), cx);
        });
        settle(cx, &rig.session);
    }
    let warlock = rig.session.read_with(cx, |s, _| {
        s.state()
            .rows()
            .iter()
            .position(|r| r.label.starts_with("Vexxa"))
    });
    let warlock = warlock.expect("the tree fixture's Warlock");
    rig.session.update(cx, |s, cx| {
        s.act(|st| st.select_row(warlock), cx);
    });
    settle(cx, &rig.session);
    rig
}

/// Send `action` and let the session answer.
fn key(cx: &mut TestAppContext, rig: &Rig, action: Action) {
    keys(cx, rig, action, 1);
    settle(cx, &rig.session);
}

/// The ability tree's drawn lines and the one the keys rest on.
fn lines(cx: &mut TestAppContext, rig: &Rig) -> (Vec<tree::Line>, Option<usize>) {
    rig.gui.read_with(cx, |g, cx| {
        let app = g.session.read(cx).state();
        let lines = model::tree_lines(app, &g.insp).expect("a tree on the stage");
        let at = model::tree_keyed(app, &g.insp, &lines);
        (lines, at)
    })
}

fn group_at(lines: &[tree::Line]) -> usize {
    lines
        .iter()
        .position(|l| matches!(l.node, Node::Group(_)))
        .expect("the Warlock's abilities group")
}

/// Enter hands the keys to the inspector; j walks the tree's LINES, a
/// group's included, and Enter on a group folds it rather than opening
/// an ability it does not have.
#[gpui_kit::test]
fn enter_hands_the_keys_over_and_j_walks_the_tree(cx: &mut TestAppContext) {
    let rig = tree_rig(cx, 1440., 900.);
    key(cx, &rig, Action::Open);
    rig.session
        .read_with(cx, |s, _| assert!(s.state().inspecting()));
    let (shut, at) = lines(cx, &rig);
    assert_eq!(at, Some(0), "the keys start on the first line");
    let group = group_at(&shut);
    keys(cx, &rig, Action::Down, group);
    settle(cx, &rig.session);
    let (_, at) = lines(cx, &rig);
    assert_eq!(at, Some(group), "the walk rests on the group's line");
    key(cx, &rig, Action::Open);
    let (open, at) = lines(cx, &rig);
    assert!(open.len() > shut.len(), "Enter opened the group");
    assert_eq!(at, Some(group), "and the keys stayed on it");
    rig.session.read_with(cx, |s, _| {
        assert!(s.state().drill_spell().is_none(), "no ability opened");
    });
}

/// → opens the keyed group, ← shuts it, and ← inside a group goes to the
/// line that holds it; on the meter the arrows still step pulls.
#[gpui_kit::test]
fn the_arrows_fold_the_tree_the_keys_are_in(cx: &mut TestAppContext) {
    let rig = tree_rig(cx, 1440., 900.);
    key(cx, &rig, Action::Open);
    let (shut, _) = lines(cx, &rig);
    let group = group_at(&shut);
    keys(cx, &rig, Action::Down, group);
    key(cx, &rig, Action::NewerSegment);
    let (open, _) = lines(cx, &rig);
    assert!(open.len() > shut.len(), "→ opened it");
    key(cx, &rig, Action::Down);
    let (_, at) = lines(cx, &rig);
    assert_eq!(at, Some(group + 1), "into the group");
    key(cx, &rig, Action::OlderSegment);
    let (_, at) = lines(cx, &rig);
    assert_eq!(at, Some(group), "← went to the group's line");
    key(cx, &rig, Action::OlderSegment);
    let (again, _) = lines(cx, &rig);
    assert_eq!(again.len(), shut.len(), "← shut it");
}

/// A fold's caret opens its group; a line's press opens its ability.
#[gpui_kit::test]
fn a_caret_folds_and_a_line_opens_its_ability(cx: &mut TestAppContext) {
    let rig = tree_rig(cx, 1440., 900.);
    let (shut, _) = lines(cx, &rig);
    let key = shut[group_at(&shut)]
        .fold_key
        .clone()
        .expect("a group folds");
    press(
        cx,
        &rig,
        ElementId::from((ElementId::Name("fold".into()), key.as_str())),
    );
    let (open, _) = lines(cx, &rig);
    assert!(open.len() > shut.len(), "the caret opened the group");
    let row = open
        .iter()
        .position(|l| matches!(l.node, Node::Row(_)))
        .expect("a row's line");
    press(
        cx,
        &rig,
        ElementId::NamedInteger("iline".into(), row as u64),
    );
    rig.session.read_with(cx, |s, _| {
        assert!(s.state().drill_spell().is_some(), "the line opened it");
    });
}

/// The panes' tabs and Tab switch the list; the actions answer.
#[gpui_kit::test]
fn the_tabs_and_actions_answer(cx: &mut TestAppContext) {
    let rig = tree_rig(cx, 1440., 900.);
    press(cx, &rig, "tab-target");
    let pane = |cx: &mut TestAppContext| {
        rig.session
            .read_with(cx, |s, _| s.state().drill.as_ref().map(|d| d.pane))
    };
    assert_eq!(pane(cx), Some(Pane::Target));
    press(cx, &rig, "tab-spell");
    assert_eq!(pane(cx), Some(Pane::Spell));
    let mode = |cx: &mut TestAppContext| rig.session.read_with(cx, |s, _| s.state().graph_mode());
    let before = mode(cx);
    press(
        cx,
        &rig,
        ElementId::from((ElementId::Name("act".into()), "Per second")),
    );
    assert_ne!(mode(cx), before, "Per second toggled the graph");
}
/// Every group open and the keys walked to the last line in a short
/// window: the inspector scrolled, and the keyed line is whole in sight.
#[gpui_kit::test]
fn the_keyed_line_stays_in_sight(cx: &mut TestAppContext) {
    let rig = tree_rig(cx, 1440., 480.);
    let (shut, _) = lines(cx, &rig);
    let groups: Vec<String> = shut.iter().filter_map(|l| l.fold_key.clone()).collect();
    rig.gui.update(cx, |g, cx| {
        for k in &groups {
            g.fold(k, cx);
        }
    });
    key(cx, &rig, Action::Open);
    let (all, _) = lines(cx, &rig);
    let last = all.len() - 1;
    keys(cx, &rig, Action::Down, last);
    settle(cx, &rig.session);
    let (_, at) = lines(cx, &rig);
    assert_eq!(at, Some(last));
    let id = ElementId::NamedInteger("iline".into(), last as u64);
    cx.update_window(rig.window, |_, window, cx| {
        window.render_frame(cx);
        window.render_frame(cx);
        let view = window.find("inspector-scroll").bounds();
        let line = window.find(id).bounds();
        assert!(
            line.top() >= view.top() && line.bottom() <= view.bottom(),
            "{line:?} in {view:?}"
        );
    })
    .unwrap();
    let scrolled = rig.gui.read_with(cx, |g, _| g.insp.scroll.offset().y);
    assert!(
        f32::from(scrolled) < 0.0,
        "the inspector scrolled: {scrolled:?}"
    );
}

/// On the Deaths view, Enter beside the table keeps the keys with it; in
/// a narrow window it pushes the recap, where ← → are the recap's — with
/// one death window there is nowhere to step, and the key is swallowed
/// rather than leave the pull.
#[gpui_kit::test]
fn the_deaths_keys_stay_with_the_table_beside_the_recap(cx: &mut TestAppContext) {
    for (width, pushed) in [(1440., false), (460., true)] {
        let rig = rig_over(cx, width, 900., raided_deaths(25));
        key(cx, &rig, Action::Down);
        key(cx, &rig, Action::Open);
        rig.session.read_with(cx, |s, _| {
            assert_eq!(s.state().inspecting(), pushed, "{width}");
        });
        if pushed {
            let at = |cx: &mut TestAppContext| {
                rig.session.read_with(cx, |s, _| s.state().segment_name())
            };
            let before = at(cx);
            key(cx, &rig, Action::OlderSegment);
            assert_eq!(at(cx), before, "the recap swallowed ←");
        }
    }
}

/// Dispatch the window-local `f`.
fn widen_key(cx: &mut TestAppContext, rig: &Rig) {
    cx.update_window(rig.window, |_, window, cx| {
        window.dispatch_action(Box::new(crate::keys::Go(crate::keys::Gesture::Wide)), cx);
        window.render_frame(cx);
    })
    .unwrap();
}

fn widened(cx: &mut TestAppContext, rig: &Rig) -> bool {
    rig.gui.read_with(cx, |g, _| g.insp.wide)
}

/// The corner button widens the inspector over the stage under the tabs
/// — the meter set aside, both lists side by side with no tabs between
/// them, the graph taller — and narrows it back; `f` does the same, Esc
/// narrows it before the chain leaves the pull, and a narrow window, whose
/// inspector is pushed over everything already, has no button at all.
#[gpui_kit::test]
fn the_corner_button_widens_the_inspector_and_narrows_it(cx: &mut TestAppContext) {
    let rig = tree_rig(cx, 1440., 900.);
    let plot_h = |cx: &mut TestAppContext| {
        rig.gui.read_with(cx, |g, _| {
            g.insp_frame
                .as_ref()
                .and_then(|i| i.graph.as_ref())
                .map(|g| g.plot_h)
        })
    };
    let beside = plot_h(cx).expect("the Warlock's graph");
    press(cx, &rig, "inspector-widen");
    assert!(widened(cx, &rig));
    cx.update_window(rig.window, |_, window, cx| {
        window.render_frame(cx);
        assert!(
            window.try_find("meter-list").is_none(),
            "the meter is set aside"
        );
        assert!(
            window.try_find("tab-spell").is_none(),
            "both lists, no tabs"
        );
        assert!(
            window
                .try_find(ElementId::NamedInteger("iline".into(), 0))
                .is_some()
        );
        assert!(
            window
                .try_find(ElementId::NamedInteger("itarget".into(), 0))
                .is_some()
        );
        let insp = window.find("inspector").bounds();
        assert!(f32::from(insp.size.width) > 1000.0, "{insp:?}");
    })
    .unwrap();
    assert!(
        plot_h(cx).is_some_and(|h| h > beside),
        "the graph stands taller"
    );
    // Tab walks the two lists rather than a tab strip.
    let pane = |cx: &mut TestAppContext| {
        rig.session
            .read_with(cx, |s, _| s.state().drill.as_ref().map(|d| d.pane))
    };
    let before = pane(cx);
    key(cx, &rig, Action::SwapPane);
    assert_ne!(pane(cx), before, "Tab moved the keys to the other list");
    // j still walks the players behind it.
    let sel = |cx: &mut TestAppContext| rig.session.read_with(cx, |s, _| s.state().row_sel);
    let row = sel(cx);
    key(cx, &rig, Action::Down);
    assert_ne!(sel(cx), row, "the meter's keys walk the players");
    assert!(widened(cx, &rig), "and the inspector stays wide");

    press(cx, &rig, "inspector-widen");
    assert!(!widened(cx, &rig), "the button narrows it back");
    cx.update_window(rig.window, |_, window, cx| {
        window.render_frame(cx);
        assert!(window.try_find("meter-list").is_some(), "the meter is back");
    })
    .unwrap();
    widen_key(cx, &rig);
    assert!(widened(cx, &rig), "f widens");
    key(cx, &rig, Action::Back);
    assert!(!widened(cx, &rig), "Esc narrows before it leaves the pull");

    let narrow = tree_rig(cx, 460., 900.);
    key(cx, &narrow, Action::Open);
    cx.update_window(narrow.window, |_, window, cx| {
        window.render_frame(cx);
        assert!(window.try_find("inspector-widen").is_none());
    })
    .unwrap();
    widen_key(cx, &narrow);
    assert!(!widened(cx, &narrow), "f is nothing in a narrow window");
}

/// v38: a drag across the Damage graph scopes the ability list to the
/// window — the daemon's answer, echoed, Σ inside the fight's, the
/// heading naming the window — and a right press gives the fight back.
#[gpui_kit::test]
fn a_drag_on_the_graph_scopes_the_ability_list(cx: &mut TestAppContext) {
    let rig = tree_rig(cx, 1440., 900.);
    // The settled pixels: no zoom glide under the drag.
    cx.update(|cx| cx.set_reduce_motion(true));
    let sum = |cx: &mut TestAppContext| {
        rig.session.read_with(cx, |s, _| {
            s.state()
                .breakdown()
                .0
                .iter()
                .map(|r| r.amount)
                .sum::<u64>()
        })
    };
    let whole = sum(cx);
    assert!(whole > 0);
    press(cx, &rig, "inspector-widen");
    cx.update_window(rig.window, |_, window, cx| {
        window.render_frame(cx);
        let b = window.find("inspector-plot").bounds();
        let y = b.origin.y + gpui_kit::px(40.);
        let x = |f: f32| b.origin.x + b.size.width * f;
        window.drag(gpui_kit::point(x(0.45), y), gpui_kit::point(x(0.7), y), cx);
    })
    .unwrap();
    settle(cx, &rig.session);
    let (asked, shown) = rig.session.read_with(cx, |s, _| {
        (s.state().drill_range(), s.state().drill_shown_range())
    });
    assert!(asked.is_some(), "the drag zoomed");
    assert_eq!(shown, asked, "and the rows answer that window");
    let scoped = sum(cx);
    assert!(
        scoped > 0 && scoped < whole,
        "{scoped} of {whole} in {asked:?}"
    );
    let head = rig
        .gui
        .read_with(cx, |g, _| match g.insp_frame.as_ref().map(|i| &i.body) {
            Some(model::Body::Split(parts)) => match &parts.0 {
                model::Body::One(l) => l.head.clone(),
                other => panic!("{other:?}"),
            },
            other => panic!("{other:?}"),
        });
    assert!(
        head.starts_with("Ability, ") && head.contains('–'),
        "{head}"
    );

    cx.update_window(rig.window, |_, window, cx| {
        window.render_frame(cx);
        window.right_click("inspector-plot", cx);
    })
    .unwrap();
    settle(cx, &rig.session);
    assert_eq!(sum(cx), whole, "a right press gives the whole fight back");
}
