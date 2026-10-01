//! Pictures of the viewer for review beside the iced viewer's design shots,
//! and the real dataset laid out spec by spec. Both are ignored: one needs
//! a wgpu adapter, the other this machine's `talents.json`.
//!
//! `talent_shots` writes `talents-<state>.png` into `$WOWDPS_SHOTS_DIR`:
//!
//! - over gui-logic's synthetic fixture (no art, a sandboxed dataset): the
//!   logged build, its tooltip, a choice's picker, the inventory, frost;
//! - with `$WOWDPS_SHOTS_LOG` (a combat log the daemon's mock reads, the
//!   iced shots' `coiled-altar-night.txt`): the owner's logged build from
//!   the newest pull of `$WOWDPS_SHOTS_FIGHT` (default "The Coiled
//!   Altar"), owner `$WOWDPS_SHOTS_OWNER` (default "Tranqlock"), against
//!   the real dataset and art, at the iced shots' three sizes — what the
//!   iced `wide-` / `tile-` / `narrow-talents.png` show.
//!
//! `WOWDPS_SHOTS_DIR=/tmp/s cargo test -p wowdps-gui talent_shots -- --ignored`

use std::path::{Path, PathBuf};

use gpui_kit::test::TestWindowExt as _;
use gpui_kit::{AppContext as _, Entity, HeadlessAppContext, TestAppContext, px, size};
use wowdps_gui_logic::talents::fixture::{Sandbox, full_string};
use wowdps_gui_logic::talents::{self as logic, Msg};
use wowdps_gui_logic::theme as gl;
use wowdps_model::{GearItem, Loadout, SegmentKind, TalentPick};
use wowdps_proto::SegmentRef;

use super::{Player, TalentViewer};
use crate::session::Session;
use crate::testkit::{self, MockLink};

/// The iced design shots' three sizes.
const SIZES: [(&str, f32, f32); 3] = [
    ("wide", 1440., 900.),
    ("tile", 960., 880.),
    ("narrow", 460., 860.),
];

/// The fixture's logged build: every frame state (taken, granted,
/// available, out of reach), a lit path, a choice, a hero pick, gear.
fn fixture_loadout() -> Loadout {
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
            gems: vec![1, 2],
        }],
    }
}

/// A headless app with the window's fonts and `def` as the theme.
fn app(def: &'static gl::Def) -> HeadlessAppContext {
    let mut cx = testkit::headless();
    cx.update(|cx| cx.set_reduce_motion(true));
    cx.update(|cx| {
        let faces = wowdps_gui_logic::fonts::FONTS
            .iter()
            .map(|b| std::borrow::Cow::Borrowed(*b))
            .collect();
        let _ = cx.text_system().add_fonts(faces);
        crate::theme::apply(def, None, cx);
    });
    cx
}

/// Open a viewer at `at`, pose it, settle, and write it to `dir/name`.
fn shoot(
    cx: &mut HeadlessAppContext,
    dir: Option<&Path>,
    name: &str,
    at: (f32, f32),
    player: Option<Player>,
    pose: impl FnOnce(&mut HeadlessAppContext, gpui_kit::AnyWindowHandle, &Entity<TalentViewer>),
) {
    let mut held = None;
    let (window, _) = testkit::open_headless(cx, size(px(at.0), px(at.1)), |window, cx| {
        let viewer = cx.new(|cx| TalentViewer::open(player, window, cx));
        held = Some(viewer.clone());
        viewer
    });
    let viewer = held.expect("the viewer was built");
    // One frame first: the panes and the tree area learn their bounds.
    let _ = cx.update_window(window, |_, window, cx| window.render_frame(cx));
    pose(cx, window, &viewer);
    cx.run_until_parked();
    let _ = cx.update_window(window, |_, window, cx| window.render_frame(cx));
    let shot = cx.capture_screenshot(window).expect("a headless renderer");
    if let Some(dir) = dir {
        let path = dir.join(format!("talents-{name}.png"));
        shot.save(&path).expect("png written");
        eprintln!("talent_shots: {}", path.display());
    }
}

/// Hover node `id` as the pointer would: its window centre into the
/// viewer.
fn hover(
    cx: &mut HeadlessAppContext,
    window: gpui_kit::AnyWindowHandle,
    viewer: &Entity<TalentViewer>,
    id: u64,
) {
    let (pane, x, y, s) = cx.update(|cx| {
        let v = viewer.read(cx);
        let b = v.ui.build.as_ref().expect("a build");
        let panes = [
            Some(&b.class_pane),
            b.hero_pane.as_ref(),
            Some(&b.spec_pane),
        ];
        panes
            .into_iter()
            .enumerate()
            .find_map(|(i, p)| {
                p.and_then(|p| p.nodes.iter().find(|n| n.id == id))
                    .map(|n| (i, n.x, n.y, v.last_fit))
            })
            .expect("the node is laid out")
    });
    let origin = cx
        .update_window(window, |_, window, cx| {
            window.render_frame(cx);
            window.find(format!("talent-pane-{pane}")).bounds().origin
        })
        .expect("the window is open");
    let at = (f32::from(origin.x) + x * s, f32::from(origin.y) + y * s);
    cx.update_entity(viewer, |v, cx| {
        v.apply(Msg::HoverSet(id, None, at.0, at.1), cx)
    });
}

#[test]
#[ignore = "needs a wgpu adapter"]
fn talent_shots() {
    let dir = std::env::var_os("WOWDPS_SHOTS_DIR").map(PathBuf::from);
    let dir = dir.as_deref();
    let frosty = || {
        Some(Player {
            name: "Frosty-Proudmoore".to_string(),
            spec_id: Some(62),
            guid: "Player-1-0000AAAA".to_string(),
        })
    };
    let logged = |cx: &mut HeadlessAppContext, _, v: &Entity<TalentViewer>| {
        cx.update_entity(v, |v, cx| v.adopt_logged(&fixture_loadout(), cx));
    };

    {
        let _sb = Sandbox::new("gpui-shots");
        let mut cx = app(&gl::GOLD);
        shoot(&mut cx, dir, "fixture", (900., 640.), frosty(), logged);
        shoot(
            &mut cx,
            dir,
            "fixture-tooltip",
            (900., 640.),
            frosty(),
            |cx, w, v| {
                logged(cx, w, v);
                hover(cx, w, v, 1);
            },
        );
        shoot(
            &mut cx,
            dir,
            "fixture-picker",
            (900., 640.),
            frosty(),
            |cx, w, v| {
                logged(cx, w, v);
                cx.update_entity(v, |v, cx| v.apply(Msg::NodeClick(2), cx));
            },
        );
        shoot(
            &mut cx,
            dir,
            "fixture-inventory",
            (900., 640.),
            frosty(),
            |cx, w, v| {
                logged(cx, w, v);
                cx.update_entity(v, |v, cx| v.apply(Msg::ToggleTab, cx));
            },
        );
        shoot(
            &mut cx,
            dir,
            "fixture-paste",
            (900., 640.),
            None,
            |cx, _, v| {
                cx.update_entity(v, |v, cx| v.paste(&full_string(), cx));
            },
        );
        shoot(
            &mut cx,
            dir,
            "fixture-empty",
            (900., 640.),
            None,
            |_, _, _| {},
        );
        let mut frost = app(&gl::FROST);
        shoot(
            &mut frost,
            dir,
            "fixture-frost",
            (900., 640.),
            frosty(),
            logged,
        );
    }

    // The real thing: the owner's logged build from a real log, beside the
    // iced viewer's `talents` design shots.
    let Some(log) = std::env::var_os("WOWDPS_SHOTS_LOG").map(PathBuf::from) else {
        eprintln!("talent_shots: set WOWDPS_SHOTS_LOG for the real-log states");
        return;
    };
    let fight = std::env::var("WOWDPS_SHOTS_FIGHT").unwrap_or_else(|_| "The Coiled Altar".into());
    let owner = std::env::var("WOWDPS_SHOTS_OWNER").unwrap_or_else(|_| "Tranqlock".into());
    let mut cx = app(&gl::GOLD);
    let session = cx.update(|cx| {
        cx.new(|_| {
            Session::with_state(
                Box::new(MockLink::new(wowdps_daemon::mock::MockDaemon::fixture_at(
                    &log,
                ))),
                wowdps_proto::ClientState::new(),
            )
        })
    });
    let settle = |cx: &mut HeadlessAppContext| {
        for _ in 0..16 {
            if !cx.update_entity(&session, |s, cx| s.pump(cx)) {
                break;
            }
        }
    };
    settle(&mut cx);
    let (pos, id) = cx
        .update(|cx| {
            let state = session.read(cx).state();
            state
                .entries()
                .iter()
                .enumerate()
                .rev()
                .find(|(_, e)| e.row.kind == SegmentKind::Encounter && e.row.name == fight)
                .map(|(pos, e)| (pos, e.id))
        })
        .expect("the fight is in the log");
    cx.update_entity(&session, |s, cx| s.act(|st| st.goto_list_pos(pos), cx));
    settle(&mut cx);
    let row = cx
        .update(|cx| {
            session
                .read(cx)
                .state()
                .rows()
                .into_iter()
                .find(|r| r.label.starts_with(&owner))
        })
        .expect("the owner fought");
    let player = Player {
        name: row.label.clone(),
        spec_id: row.spec.map(|s| s.id()),
        guid: row.key.clone(),
    };
    for (name, w, h) in SIZES {
        let player = player.clone();
        let session = session.clone();
        shoot(
            &mut cx,
            dir,
            name,
            (w, h),
            Some(player.clone()),
            move |cx, _, v| {
                cx.update_entity(v, |v, cx| {
                    v.ask_loadout(&session, SegmentRef::Id(id), player.guid.clone(), cx)
                });
                for _ in 0..16 {
                    if !cx.update_entity(&session, |s, cx| s.pump(cx)) {
                        break;
                    }
                }
                let logged = cx.update(|cx| v.read(cx).ui.logged);
                assert!(logged, "the owner's loadout was adopted");
            },
        );
    }
}

/// Against the REAL per-machine dataset: every spec opens on its empty
/// tree, draws its class and spec panes, and a press takes one of its
/// roots. Ignored like the other per-machine gates.
#[gpui_kit::test]
#[ignore = "needs the per-machine talents.json (tools/gen-talent-trees.sh)"]
fn real_dataset_draws_every_spec(cx: &mut TestAppContext) {
    let ds = logic::load_dataset().expect("no talents.json on this machine");
    let specs: Vec<u32> = match ds.get("trees") {
        Some(wowdps_proto::json::Json::Arr(trees)) => trees
            .iter()
            .filter_map(|t| match t.get("specs") {
                Some(wowdps_proto::json::Json::Arr(s)) => Some(s.clone()),
                _ => None,
            })
            .flatten()
            .filter_map(|s| s.get("specId").and_then(wowdps_proto::json::Json::as_u64))
            .map(|id| id as u32)
            .collect(),
        _ => Vec::new(),
    };
    assert!(specs.len() > 30, "every class's specs: {}", specs.len());
    for spec_id in specs {
        let (window, viewer) = testkit::open(cx, size(px(1440.), px(900.)), move |window, cx| {
            cx.new(|cx| {
                TalentViewer::open(
                    Some(Player {
                        name: format!("Spec{spec_id}"),
                        spec_id: Some(spec_id),
                        guid: String::new(),
                    }),
                    window,
                    cx,
                )
            })
        });
        let root = viewer.read_with(cx, |v, _| {
            assert!(v.ui.error.is_none(), "spec {spec_id}: {:?}", v.ui.error);
            let b = v.ui.build.as_ref().expect("a layout");
            b.class_pane
                .nodes
                .iter()
                .find(|n| n.available && !n.choice)
                .map(|n| n.id)
        });
        cx.update_window(window, |_, window, cx| {
            window.render_frame(cx);
            for pane in [0, 2] {
                let b = window.find(format!("talent-pane-{pane}")).bounds();
                assert!(
                    b.size.width > px(40.) && b.size.height > px(40.),
                    "spec {spec_id}: pane {pane} is {b:?}"
                );
            }
        })
        .expect("the window is open");
        if let Some(root) = root {
            viewer.update(cx, |v, cx| v.apply(Msg::NodeClick(root), cx));
            viewer.read_with(cx, |v, _| {
                assert!(
                    v.ui.sels.contains_key(&root),
                    "spec {spec_id}: a root takes"
                );
            });
        }
    }
}
