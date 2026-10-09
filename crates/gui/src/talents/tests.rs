//! The viewer through Kit's harness, over gui-logic's talent fixture (a
//! synthetic Arcane tree: node 1 three ranks, choice node 2 gated at two
//! points, 3 → 6 in the spec half, hero tree 77): presses on the panes'
//! canvases at a node's own offset, the pointer moved over them, keys,
//! the clipboard and a loadout answered through a `Session`.

use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

use gpui_kit::test::TestWindowExt as _;
use gpui_kit::{
    AnyWindowHandle, AppContext as _, ClipboardItem, Entity, ExternalPaths, Focusable as _,
    InputEvent as _, Modifiers, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, Pixels,
    Point, TestAppContext, Window, point, px, size,
};
use wowdps_gui_logic::talents::fixture::{Sandbox, full_string, simc_paste, string_for};
use wowdps_gui_logic::talents::{Tab, picker_spots};
use wowdps_model::{GearItem, Loadout, TalentPick};
use wowdps_proto::{ClientMsg, ClientState, DaemonMsg, Reconnect, SegmentRef};

use super::{Player, TalentEvent, TalentViewer, bindings};
use crate::session::{Link, Session};
use crate::testkit;

const WIDE: (f32, f32) = (1440., 900.);

fn frosty() -> Player {
    Player {
        name: "Frosty-Proudmoore".to_string(),
        spec_id: Some(62),
        guid: "Player-1-0000AAAA".to_string(),
    }
}

struct Rig {
    window: AnyWindowHandle,
    viewer: Entity<TalentViewer>,
}

fn open_at(cx: &mut TestAppContext, at: (f32, f32), player: Option<Player>) -> Rig {
    cx.update(|cx| cx.bind_keys(bindings()));
    let (window, viewer) = testkit::open(cx, size(px(at.0), px(at.1)), move |window, cx| {
        cx.new(|cx| TalentViewer::open(player, window, cx))
    });
    // Motion is a delight on top: these tests read the settled pixels.
    cx.update(|cx| cx.set_reduce_motion(true));
    Rig { window, viewer }
}

fn open(cx: &mut TestAppContext, player: Option<Player>) -> Rig {
    open_at(cx, WIDE, player)
}

impl Rig {
    fn with<R>(
        &self,
        cx: &mut TestAppContext,
        f: impl FnOnce(&mut Window, &mut gpui_kit::App) -> R,
    ) -> R {
        cx.update_window(self.window, |_, window, cx| f(window, cx))
            .expect("the window is open")
    }

    fn read<R>(&self, cx: &mut TestAppContext, f: impl FnOnce(&TalentViewer) -> R) -> R {
        self.viewer.read_with(cx, |v, _| f(v))
    }

    /// A node's centre within its pane's canvas (`talent-pane-<pane>`), in
    /// the canvas's pixels.
    fn node_offset(&self, cx: &mut TestAppContext, id: u64) -> (usize, Point<Pixels>) {
        self.read(cx, |v| {
            let b = v.ui.build.as_ref().expect("a build");
            let s = v.last_fit;
            let panes = [
                Some(&b.class_pane),
                b.hero_pane.as_ref(),
                Some(&b.spec_pane),
            ];
            for (i, pane) in panes.into_iter().enumerate() {
                if let Some(n) = pane.and_then(|p| p.nodes.iter().find(|n| n.id == id)) {
                    return (i, point(px(n.x * s), px(n.y * s)));
                }
            }
            panic!("node {id} is not laid out")
        })
    }

    /// The pointer to `offset` within pane `pane`'s canvas, then — with a
    /// button — a press and a release there.
    fn pointer(
        &self,
        cx: &mut TestAppContext,
        pane: usize,
        offset: Point<Pixels>,
        button: Option<MouseButton>,
    ) {
        self.with(cx, |window, cx| {
            window.render_frame(cx);
            let origin = window.find(format!("talent-pane-{pane}")).bounds().origin;
            let position = origin + offset;
            window.dispatch_event(
                MouseMoveEvent {
                    position,
                    pressed_button: None,
                    modifiers: Modifiers::default(),
                }
                .to_platform_input(),
                cx,
            );
            window.render_frame(cx);
            if let Some(button) = button {
                window.dispatch_event(
                    MouseDownEvent {
                        button,
                        position,
                        modifiers: Modifiers::default(),
                        click_count: 1,
                        first_mouse: false,
                    }
                    .to_platform_input(),
                    cx,
                );
                window.dispatch_event(
                    MouseUpEvent {
                        button,
                        position,
                        modifiers: Modifiers::default(),
                        click_count: 1,
                    }
                    .to_platform_input(),
                    cx,
                );
                window.render_frame(cx);
            }
        });
    }

    fn press_node(&self, cx: &mut TestAppContext, id: u64, button: MouseButton) {
        let (pane, offset) = self.node_offset(cx, id);
        self.pointer(cx, pane, offset, Some(button));
    }

    fn ranks(&self, cx: &mut TestAppContext, id: u64) -> Option<u64> {
        self.read(cx, |v| v.ui.sels.get(&id).map(|s| s.ranks))
    }
}

#[gpui_kit::test]
fn a_row_opens_its_bare_tree_and_nobody_gets_the_paste_prompt(cx: &mut TestAppContext) {
    let _sb = Sandbox::new("gpui-open");
    let rig = open(cx, Some(frosty()));
    rig.with(cx, |window, cx| {
        window.render_frame(cx);
        assert!(window.try_find("talent-pane-0").is_some(), "the class pane");
        assert!(window.try_find("talent-pane-2").is_some(), "the spec pane");
        assert!(
            window.try_find("talent-pane-1").is_none(),
            "no hero pick, no hero pane"
        );
        assert!(window.try_find("talent-area").is_some());
        assert!(
            window.try_find("tab-inventory").is_none(),
            "nothing to show there"
        );
    });
    rig.read(cx, |v| {
        assert_eq!(v.ui.player.as_deref(), Some("Frosty-Proudmoore"));
        assert!(v.ui.sels.is_empty());
    });

    let nobody = open(cx, None);
    nobody.with(cx, |window, cx| {
        window.render_frame(cx);
        assert!(
            window.try_find("talent-pane-0").is_none(),
            "the paste prompt instead"
        );
        assert!(window.try_find("paste-simc").is_some());
    });
}

#[gpui_kit::test]
fn presses_take_ranks_refund_them_and_open_a_choice(cx: &mut TestAppContext) {
    let _sb = Sandbox::new("gpui-presses");
    let rig = open(cx, Some(frosty()));
    rig.press_node(cx, 1, MouseButton::Left);
    assert_eq!(rig.ranks(cx, 1), Some(1), "a left press takes the root");
    rig.press_node(cx, 1, MouseButton::Left);
    assert_eq!(rig.ranks(cx, 1), Some(2), "and adds a rank");
    rig.press_node(cx, 1, MouseButton::Right);
    assert_eq!(rig.ranks(cx, 1), Some(1), "a right press refunds one");
    rig.press_node(cx, 1, MouseButton::Left);

    // Two points open the gated octagon: a press opens its picker, a press
    // on its second tile picks "Right".
    rig.press_node(cx, 2, MouseButton::Left);
    assert_eq!(rig.read(cx, |v| v.ui.picker), Some(2), "the picker opens");
    let spot = rig.read(cx, |v| {
        let b = v.ui.build.as_ref().unwrap();
        let n2 = b.class_pane.nodes.iter().find(|n| n.id == 2).unwrap();
        picker_spots(&b.class_pane, n2)[1]
    });
    let s = rig.read(cx, |v| v.last_fit);
    rig.pointer(
        cx,
        0,
        point(px(spot.0 * s), px(spot.1 * s)),
        Some(MouseButton::Left),
    );
    rig.read(cx, |v| {
        assert_eq!(v.ui.picker, None);
        assert_eq!(v.ui.sels.get(&2).and_then(|s| s.choice_index), Some(1));
        assert!(v.ui.edited);
    });

    // A press on nothing is nothing.
    let empty = rig.read(cx, |v| {
        let p = &v.ui.build.as_ref().unwrap().class_pane;
        (p.w - 4.0, 4.0)
    });
    rig.pointer(
        cx,
        0,
        point(px(empty.0 * s), px(empty.1 * s)),
        Some(MouseButton::Left),
    );
    assert_eq!(rig.ranks(cx, 1), Some(2));
}

#[gpui_kit::test]
fn the_pointer_over_a_node_brings_its_tooltip_and_leaving_takes_it(cx: &mut TestAppContext) {
    let _sb = Sandbox::new("gpui-hover");
    let rig = open(cx, Some(frosty()));
    let (pane, offset) = rig.node_offset(cx, 1);
    rig.pointer(cx, pane, offset, None);
    rig.read(cx, |v| assert_eq!(v.ui.hover, Some((1, None))));
    rig.with(cx, |window, cx| {
        window.render_frame(cx);
        assert!(window.try_find("talent-tip").is_some(), "the tooltip is up");
    });
    rig.pointer(cx, pane, point(px(2.), px(2.)), None);
    rig.read(cx, |v| assert_eq!(v.ui.hover, None));
    rig.with(cx, |window, cx| {
        window.render_frame(cx);
        assert!(window.try_find("talent-tip").is_none(), "and gone");
    });
}

/// A link whose daemon answers `GetLoadout` with one loadout and ignores
/// everything else.
struct LoadoutLink {
    loadout: Loadout,
    inbox: Vec<DaemonMsg>,
    asked: Rc<RefCell<Vec<ClientMsg>>>,
}

impl Link for LoadoutLink {
    fn send(&mut self, msg: &ClientMsg) {
        if let ClientMsg::GetLoadout { req_id, guid, .. } = msg {
            self.asked.borrow_mut().push(msg.clone());
            self.inbox.push(DaemonMsg::Loadout {
                req_id: *req_id,
                guid: guid.clone(),
                loadout: Some(self.loadout.clone()),
            });
        }
    }

    fn poll(&mut self) -> Vec<DaemonMsg> {
        std::mem::take(&mut self.inbox)
    }

    fn reconnect(&mut self) -> Reconnect {
        Reconnect::Connected
    }
}

fn logged() -> Loadout {
    let pick = |node_id, entry_id, rank| TalentPick {
        node_id,
        entry_id,
        rank,
    };
    Loadout {
        spec_id: Some(62),
        talents: vec![
            pick(1, 101, 2),
            pick(2, 132, 1),
            pick(3, 104, 0),
            pick(4, 151, 1),
            pick(5, 106, 1),
        ],
        gear: vec![GearItem {
            item_id: 212095,
            ilvl: 639,
            enchants: vec![7328],
            bonus_ids: Vec::new(),
            gems: vec![1],
        }],
        stats: vec![],
        auras: vec![],
    }
}

#[gpui_kit::test]
fn the_logged_loadout_comes_back_through_the_session(cx: &mut TestAppContext) {
    let _sb = Sandbox::new("gpui-loadout");
    let asked = Rc::new(RefCell::new(Vec::new()));
    let link = LoadoutLink {
        loadout: logged(),
        inbox: Vec::new(),
        asked: Rc::clone(&asked),
    };
    let session = cx.new(|_| Session::with_state(Box::new(link), ClientState::new()));
    let rig = open(cx, Some(frosty()));
    rig.viewer.update(cx, |v, cx| {
        v.ask_loadout(
            &session,
            SegmentRef::Live,
            "Player-1-0000AAAA".to_string(),
            cx,
        )
    });
    assert!(
        matches!(asked.borrow().first(), Some(ClientMsg::GetLoadout { guid, .. }) if guid == "Player-1-0000AAAA"),
        "{:?}",
        asked.borrow()
    );
    cx.update_entity(&session, |s, cx| s.pump(cx));
    cx.run_until_parked();
    rig.read(cx, |v| {
        assert!(v.ui.logged, "the combat log's build is in");
        assert_eq!(v.ui.sels.get(&1).map(|s| s.ranks), Some(2));
        assert!(v.ui.sels.get(&3).is_some_and(|s| s.granted));
        assert_eq!(v.ui.hero, Some(77));
        assert!(v.pending.is_none());
    });
    rig.with(cx, |window, cx| {
        window.render_frame(cx);
        assert!(window.try_find("talent-pane-1").is_some(), "the hero pane");
        assert!(
            window.try_find("tab-inventory").is_some(),
            "the logged gear's tab"
        );
    });

    // An answer to another question changes nothing.
    rig.viewer.update(cx, |v, cx| {
        v.paste(&string_for(&[r#"{"node_id": 3}"#]), cx);
    });
    session.update(cx, |s, cx| {
        cx.emit(crate::session::Reply(DaemonMsg::Loadout {
            req_id: 1,
            guid: "x".to_string(),
            loadout: Some(logged()),
        }));
        let _ = s;
    });
    cx.run_until_parked();
    rig.read(cx, |v| {
        assert!(!v.ui.logged, "a stranger's answer is not adopted")
    });
}

#[gpui_kit::test]
fn esc_closes_and_tab_flips_to_the_inventory(cx: &mut TestAppContext) {
    let _sb = Sandbox::new("gpui-keys");
    let rig = open(cx, Some(frosty()));
    rig.viewer.update(cx, |v, cx| v.adopt_logged(&logged(), cx));
    let closed = Rc::new(RefCell::new(0));
    let seen = Rc::clone(&closed);
    let _sub = cx.update(|cx| {
        cx.subscribe(&rig.viewer, move |_, e: &TalentEvent, _| {
            if *e == TalentEvent::Close {
                *seen.borrow_mut() += 1;
            }
        })
    });
    let focus = rig.read(cx, |v| v.focus_handle().clone());
    rig.with(cx, |window, cx| {
        window.focus(&focus, cx);
        window.press("tab", cx);
    });
    assert_eq!(rig.read(cx, |v| v.ui.tab), Tab::Inventory);
    rig.with(cx, |window, cx| {
        window.render_frame(cx);
        assert!(window.try_find("talent-inventory").is_some());
        assert!(window.try_find("talent-pane-0").is_none());
        window.press("tab", cx);
        window.press("escape", cx);
    });
    assert_eq!(rig.read(cx, |v| v.ui.tab), Tab::Talents);
    assert_eq!(*closed.borrow(), 1, "Esc asks the host to close");
    rig.with(cx, |window, cx| window.click("talents-close", cx));
    assert_eq!(*closed.borrow(), 2, "and so does ✕");
}

#[gpui_kit::test]
fn the_clipboard_pastes_in_and_copies_out(cx: &mut TestAppContext) {
    let _sb = Sandbox::new("gpui-clipboard");
    let rig = open(cx, None);
    cx.update(|cx| cx.write_to_clipboard(ClipboardItem::new_string(full_string())));
    rig.with(cx, |window, cx| window.click("paste-simc", cx));
    rig.read(cx, |v| {
        assert_eq!(v.ui.error, None);
        assert_eq!(v.ui.hero, Some(77), "the pasted build");
    });
    cx.update(|cx| cx.write_to_clipboard(ClipboardItem::new_string(String::new())));
    rig.with(cx, |window, cx| {
        window.render_frame(cx);
        window.click("copy-string", cx);
        assert_eq!(
            window.find("copy-string").selected(),
            Some(true),
            "it says copied"
        );
    });
    let copied = cx.update(|cx| cx.read_from_clipboard().and_then(|i| i.text()));
    assert_eq!(
        copied,
        Some(full_string()),
        "the unedited build's own string"
    );
    cx.executor().advance_clock(Duration::from_secs(2));
    cx.run_until_parked();
    rig.with(cx, |window, cx| {
        window.render_frame(cx);
        assert_eq!(
            window.find("copy-string").selected(),
            Some(false),
            "and then not"
        );
    });
}

#[gpui_kit::test]
fn a_simc_export_brings_loadout_chips_and_an_inventory(cx: &mut TestAppContext) {
    let sb = Sandbox::new("gpui-simc");
    let rig = open(cx, Some(frosty()));
    let (a, b) = (full_string(), string_for(&[r#"{"node_id": 3}"#]));
    rig.viewer.update(cx, |v, cx| {
        v.paste(
            &simc_paste("Frosty", Some("proudmoore"), &[&a, &b], true),
            cx,
        );
    });
    assert!(
        sb.stored("Frosty-Proudmoore").is_some(),
        "kept for the next open"
    );
    rig.with(cx, |window, cx| {
        window.render_frame(cx);
        window.click(("loadout", 1usize), cx);
    });
    rig.read(cx, |v| assert_eq!(v.ui.loadout_sel, 1));
    assert_eq!(rig.ranks(cx, 3), Some(1), "the second loadout");
    rig.with(cx, |window, cx| {
        window.click("tab-inventory", cx);
        window.render_frame(cx);
        assert!(window.try_find("talent-inventory").is_some());
    });
}

#[gpui_kit::test]
fn a_dropped_file_is_read_as_a_paste(cx: &mut TestAppContext) {
    let sb = Sandbox::new("gpui-drop");
    let rig = open(cx, None);
    let path = sb.dir.join("export.simc");
    std::fs::create_dir_all(&sb.dir).unwrap();
    std::fs::write(&path, simc_paste("Frosty", None, &[&full_string()], false)).unwrap();
    rig.viewer.update(cx, |v, cx| {
        v.drop_files(&ExternalPaths(vec![path.clone()].into()), cx)
    });
    rig.read(cx, |v| {
        assert_eq!(v.ui.player.as_deref(), Some("Frosty"));
        assert_eq!(v.ui.hero, Some(77));
    });
    rig.viewer.update(cx, |v, cx| {
        v.drop_files(&ExternalPaths(vec![sb.dir.join("missing.simc")].into()), cx)
    });
    rig.read(cx, |v| {
        assert!(
            v.ui.error
                .as_deref()
                .is_some_and(|e| e.contains("cannot read"))
        );
    });
}

#[gpui_kit::test]
fn typing_a_string_and_enter_decode_it(cx: &mut TestAppContext) {
    let _sb = Sandbox::new("gpui-typing");
    let rig = open(cx, None);
    let focus = rig
        .viewer
        .read_with(cx, |v, cx| v.input.read(cx).focus_handle(cx));
    let s = full_string();
    rig.with(cx, |window, cx| {
        window.focus(&focus, cx);
        window.render_frame(cx);
        window.input(&s, cx);
        window.press("enter", cx);
    });
    rig.read(cx, |v| {
        assert_eq!(v.ui.input, s);
        assert_eq!(v.ui.hero, Some(77), "Enter decodes the line");
    });
}

#[gpui_kit::test]
fn a_narrow_window_fits_the_trees_before_it_scrolls(cx: &mut TestAppContext) {
    let _sb = Sandbox::new("gpui-fit");
    let rig = open_at(cx, (300., 700.), None);
    rig.viewer.update(cx, |v, cx| v.adopt_logged(&logged(), cx));
    rig.with(cx, |window, cx| window.render_frame(cx));
    let (fit, w) = rig.read(cx, |v| {
        (v.last_fit, v.ui.build.as_ref().unwrap().class_pane.w)
    });
    assert!((super::MIN_FIT..1.0).contains(&fit), "{fit}");
    rig.with(cx, |window, _| {
        let drawn = window.find("talent-pane-0").bounds().size.width;
        assert!(
            (f32::from(drawn) - w * fit).abs() < 0.5,
            "{drawn:?} vs {}",
            w * fit
        );
    });
    // A press still lands on its node at the smaller scale.
    rig.press_node(cx, 6, MouseButton::Left);
    assert_eq!(rig.ranks(cx, 6), Some(1));

    let wide = open(cx, None);
    wide.viewer
        .update(cx, |v, cx| v.adopt_logged(&logged(), cx));
    wide.with(cx, |window, cx| window.render_frame(cx));
    assert_eq!(
        wide.read(cx, |v| v.last_fit),
        1.0,
        "a wide window draws them whole"
    );
}

#[gpui_kit::test]
fn a_taken_node_ripples_and_the_ripple_ends(cx: &mut TestAppContext) {
    let _sb = Sandbox::new("gpui-ripple");
    let rig = open(cx, Some(frosty()));
    cx.update(|cx| cx.set_reduce_motion(false));
    rig.press_node(cx, 1, MouseButton::Left);
    rig.with(cx, |window, cx| window.render_frame(cx));
    assert_eq!(rig.read(cx, |v| v.ripples.len()), 1, "a ripple runs");
    cx.executor().advance_clock(Duration::from_millis(800));
    rig.with(cx, |window, cx| {
        window.render_frame(cx);
        window.render_frame(cx);
    });
    assert!(rig.read(cx, |v| v.ripples.is_empty()), "and ends");
    // A refund never ripples.
    rig.press_node(cx, 1, MouseButton::Right);
    rig.with(cx, |window, cx| window.render_frame(cx));
    assert!(rig.read(cx, |v| v.ripples.is_empty()));
}
