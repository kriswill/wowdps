//! The window's design shots (the iced window's `window::shots`): its
//! states rendered headless at the prototype's three sizes, each reached by
//! the gestures a user makes, for review beside the reference renders
//! (`~/.local/share/wowdps/design-shots/reference/`) and the iced shots.
//!
//! - Over the committed fixture by default.
//! - With `$WOWDPS_SHOTS_LOG` (a combat log the daemon's mock reads, the
//!   iced shots' `coiled-altar-night.txt`), on the newest pull of
//!   `$WOWDPS_SHOTS_FIGHT` (default "The Coiled Altar"), the owner
//!   `$WOWDPS_SHOTS_OWNER` (default "Tranqlock") named in the config's
//!   `history_characters` so the "you" marks find them.
//! - `$WOWDPS_SHOTS_ONLY` narrows to states whose name contains it.
//!
//! Zoom 1, reduced motion: a shot is the parity pixels, never mid-glide.
//!
//! `WOWDPS_SHOTS_DIR=/tmp/s cargo test -p wowdps-gui-new window_shots -- --ignored --nocapture`

use std::path::PathBuf;

use gpui_kit::test::TestWindowExt as _;
use gpui_kit::{AnyWindowHandle, AppContext as _, Entity, HeadlessAppContext, px, size};
use wowdps_gui_logic::config::Config;
use wowdps_model::SegmentKind;

use super::Gui;
use crate::session::Session;
use crate::testkit::{self, MockLink};

/// The prototype's three frames.
pub(crate) const SIZES: [(&str, f32, f32); 3] = [
    ("wide", 1440., 900.),
    ("tile", 960., 880.),
    ("narrow", 460., 860.),
];

/// A pose applied to a settled window before it is photographed.
pub(crate) type Pose = fn(&mut HeadlessAppContext, &Shot);

/// One window being photographed.
pub(crate) struct Shot {
    pub window: AnyWindowHandle,
    pub gui: Entity<Gui>,
    pub session: Entity<Session>,
}

impl Shot {
    pub(crate) fn settle(&self, cx: &mut HeadlessAppContext) {
        for _ in 0..16 {
            if !cx.update_entity(&self.session, |s, cx| s.pump(cx)) {
                break;
            }
        }
    }
}

/// What the shots run over: a log and the fight to land on, or the fixture,
/// and a history store read through under the log's own stored fights.
struct Input {
    log: Option<PathBuf>,
    fight: String,
    owner: String,
    store: Option<PathBuf>,
}

impl Input {
    fn from_env() -> Self {
        Input {
            log: std::env::var_os("WOWDPS_SHOTS_LOG").map(PathBuf::from),
            fight: std::env::var("WOWDPS_SHOTS_FIGHT")
                .unwrap_or_else(|_| "The Coiled Altar".into()),
            owner: std::env::var("WOWDPS_SHOTS_OWNER").unwrap_or_else(|_| "Tranqlock".into()),
            store: std::env::var_os("WOWDPS_SHOTS_HISTORY").map(PathBuf::from),
        }
    }

    /// The daemon's mock over the input, every fight of the log stored as
    /// if it closed live — over a real log stamped with the owner, whom the
    /// daemon reads from the same config — and `$WOWDPS_SHOTS_HISTORY` (a
    /// store's `v1` directory) read through READ-ONLY, as the iced shots
    /// run: the log's own cards alone are one night, and Home and the rail
    /// are about weeks.
    fn link(&self) -> MockLink {
        use wowdps_daemon::mock::MockDaemon;
        let mut mock = match &self.log {
            Some(log) => MockDaemon::fixture_at(log).with_characters(&[self.owner.clone()]),
            None => MockDaemon::fixture(),
        };
        if let Some(store) = &self.store {
            mock = mock.with_store_dir(store);
        }
        MockLink::new(mock.with_history())
    }

    /// The shots' config, written to the test's own file and read back as
    /// the window would: zoom 1, no Home at launch, and over a real log the
    /// owner named and realms hidden, as the iced shots run.
    fn config(&self, path: &std::path::Path) -> Config {
        let mut text = String::from("zoom = 1.0\nhome_on_start = false\n");
        if self.log.is_some() {
            text.push_str(&format!(
                "hide_realms = true\nhistory_characters = [\"{}\"]\n",
                self.owner
            ));
        }
        std::fs::write(path, text).expect("the shots' config is written");
        Config::load()
    }
}

/// A headless app with the window's fonts, the gold theme, motion reduced.
pub(crate) fn app() -> HeadlessAppContext {
    let mut cx = testkit::headless();
    cx.update(|cx| {
        cx.set_reduce_motion(true);
        let faces = wowdps_gui_logic::fonts::FONTS
            .iter()
            .map(|b| std::borrow::Cow::Borrowed(*b))
            .collect();
        let _ = cx.text_system().add_fonts(faces);
        crate::keys::bind(cx);
        crate::theme::apply(&wowdps_gui_logic::theme::GOLD, None, cx);
    });
    cx
}

/// Open a window `at`, land it on the input's fight, and settle.
fn open(cx: &mut HeadlessAppContext, input: &Input, at: (f32, f32)) -> Shot {
    let path = super::tests::own_config();
    let cfg = input.config(&path);
    let link = input.link();
    let mut held = None;
    let (window, gui) = testkit::open_headless(cx, size(px(at.0), px(at.1)), |window, cx| {
        let session = cx.new(|_| Session::new(link));
        held = Some(session.clone());
        let gui = cx.new(|cx| Gui::new(session, cfg, window, cx));
        let focus = gui.read(cx).focus().clone();
        window.focus(&focus, cx);
        gui
    });
    let session = held.expect("the session was built");
    let shot = Shot {
        window,
        gui,
        session,
    };
    shot.settle(cx);
    // "Tonight" is the log's newest night, pinned: the rail's headings never
    // read the wall clock (the iced shots' `Scene::tonight`).
    let tonight = cx.update(|cx| {
        shot.session
            .read(cx)
            .state()
            .entries()
            .iter()
            .map(|e| e.row.start_ms)
            .max()
            .map(wowdps_gui_logic::rail::night_of)
    });
    cx.update_entity(&shot.gui, |g, _| g.hist.tonight_pin = tonight);
    shot.settle(cx);
    if input.log.is_some() {
        let fight = input.fight.clone();
        let pos = cx.update(|cx| {
            shot.session
                .read(cx)
                .state()
                .entries()
                .iter()
                .enumerate()
                .rev()
                .find(|(_, e)| e.row.kind == SegmentKind::Encounter && e.row.name == fight)
                .map(|(pos, _)| pos)
        });
        if let Some(pos) = pos {
            cx.update_entity(&shot.session, |s, cx| s.act(|st| st.goto_list_pos(pos), cx));
            shot.settle(cx);
        }
    }
    shot
}

/// Every state, by name, with its pose.
fn states() -> Vec<(&'static str, Pose)> {
    /// The fight in `v`, as its tab shows it, the owner's row selected the
    /// way the keys get there (the iced shots' `in_view`).
    fn view(cx: &mut HeadlessAppContext, s: &Shot, v: wowdps_model::View) {
        cx.update_entity(&s.gui, |g, cx| g.pick_view(v, cx));
        s.settle(cx);
        let rows = cx.update(|cx| s.gui.read(cx).fight(cx).rows().len());
        for _ in 0..rows * 2 {
            let (sel, owner) = cx.update(|cx| {
                let gui = s.gui.read(cx);
                let state = gui.fight(cx);
                let rows = state.rows();
                (state.row_sel, gui.owner_in(&rows, state.view))
            });
            let Some(owner) = owner.filter(|&o| o != sel) else {
                break;
            };
            let step = if sel < owner {
                wowdps_model::Action::Down
            } else {
                wowdps_model::Action::Up
            };
            let _ = cx.update_window(s.window, |_, window, cx| {
                window.dispatch_action(Box::new(crate::keys::Do(step)), cx);
                window.render_frame(cx);
            });
            s.settle(cx);
        }
    }
    vec![
        ("damage", |cx, s| view(cx, s, wowdps_model::View::Damage)),
        ("healing", |cx, s| view(cx, s, wowdps_model::View::Healing)),
        ("taken", |cx, s| view(cx, s, wowdps_model::View::Taken)),
        ("deaths", |cx, s| view(cx, s, wowdps_model::View::Deaths)),
        ("interrupts", |cx, s| {
            view(cx, s, wowdps_model::View::Interrupts)
        }),
        ("enemies", |cx, s| {
            view(cx, s, wowdps_model::View::EnemyTaken)
        }),
        ("home", |cx, s| {
            cx.update_entity(&s.gui, |g, cx| g.open_home(cx));
            s.settle(cx);
        }),
        // The pull rail as a drawer (tile and narrow) over the fight; beside
        // it at the wide frame, as every wide shot has it.
        ("rail-open", |cx, s| {
            view(cx, s, wowdps_model::View::Damage);
            cx.update_entity(&s.gui, |g, cx| g.open_drawer(cx));
        }),
        // The rail with its trash hidden, open over the fight.
        ("hide-trash", |cx, s| {
            view(cx, s, wowdps_model::View::Damage);
            cx.update_entity(&s.gui, |g, cx| {
                g.toggle_trash(cx);
                g.open_drawer(cx);
            });
        }),
        // A pull of an earlier night on the stage — a kill from the deepest
        // of the first four earlier nights, far enough down that the drawer
        // must scroll to it — and the rail open over it at its row.
        ("rail-earlier", |cx, s| {
            let pull = cx.update(|cx| {
                let rail = s.gui.read(cx).rail(cx);
                let first = rail.earlier()?;
                (first..first + 4).rev().find_map(|i| {
                    let lines: Vec<_> = rail
                        .nights
                        .get(i)?
                        .visits
                        .iter()
                        .flat_map(|v| v.lines.iter())
                        .collect();
                    lines
                        .iter()
                        .find(|l| l.mark == wowdps_gui_logic::rail::Mark::Good)
                        .or(lines.first())
                        .map(|l| l.pull.clone())
                })
            });
            if let Some(pull) = pull {
                stored(cx, s, pull);
            }
            cx.update_entity(&s.gui, |g, cx| g.open_drawer(cx));
        }),
        // A pull from an earlier night, opened on the stage from the store:
        // the rail's newest stored kill, drawn by the renderers a pull of
        // the log is, the owner's row selected.
        ("stored", |cx, s| {
            let pull = cx.update(|cx| {
                s.gui.read(cx).rail(cx).lines().find_map(|l| {
                    (matches!(l.pull, wowdps_gui_logic::rail::Pull::Stored(_))
                        && l.mark == wowdps_gui_logic::rail::Mark::Good
                        && l.key.is_none()
                        && !l.trash)
                        .then(|| l.pull.clone())
                })
            });
            if let Some(pull) = pull {
                stored(cx, s, pull);
            }
        }),
        // The command palette (Ctrl K) over the meter, nothing typed yet:
        // the recent pulls, the pull's players, the views and the screens.
        ("palette", |cx, s| {
            view(cx, s, wowdps_model::View::Damage);
            let _ = cx.update_window(s.window, |_, window, cx| {
                window.dispatch_action(Box::new(crate::keys::Go(crate::keys::Gesture::Jump)), cx);
                window.render_frame(cx);
            });
        }),
    ]
}

/// Open `pull` on the stage, let the store answer, and select the owner's
/// row on it as the keys get there.
fn stored(cx: &mut HeadlessAppContext, s: &Shot, pull: wowdps_gui_logic::rail::Pull) {
    cx.update_entity(&s.gui, |g, cx| g.go_pull(pull, cx));
    s.settle(cx);
    let view = cx.update(|cx| s.gui.read(cx).fight(cx).view);
    let rows = cx.update(|cx| s.gui.read(cx).fight(cx).rows().len());
    for _ in 0..rows * 2 {
        let (sel, owner) = cx.update(|cx| {
            let gui = s.gui.read(cx);
            let state = gui.fight(cx);
            let rows = state.rows();
            (state.row_sel, gui.owner_in(&rows, view))
        });
        let Some(owner) = owner.filter(|&o| o != sel) else {
            break;
        };
        let step = if sel < owner {
            wowdps_model::Action::Down
        } else {
            wowdps_model::Action::Up
        };
        let _ = cx.update_window(s.window, |_, window, cx| {
            window.dispatch_action(Box::new(crate::keys::Do(step)), cx);
            window.render_frame(cx);
        });
        s.settle(cx);
    }
}

#[test]
#[ignore = "needs a wgpu adapter"]
fn window_shots() {
    let dir = std::env::var_os("WOWDPS_SHOTS_DIR").map(PathBuf::from);
    if let Some(dir) = &dir {
        std::fs::create_dir_all(dir).expect("the shots' directory");
    }
    let only = std::env::var("WOWDPS_SHOTS_ONLY").ok();
    let input = Input::from_env();
    for (name, pose) in states() {
        if only.as_deref().is_some_and(|o| !name.contains(o)) {
            continue;
        }
        for (frame, w, h) in SIZES {
            let mut cx = app();
            let shot = open(&mut cx, &input, (w, h));
            let _ = cx.update_window(shot.window, |_, window, cx| window.render_frame(cx));
            pose(&mut cx, &shot);
            shot.settle(&mut cx);
            cx.run_until_parked();
            let _ = cx.update_window(shot.window, |_, window, cx| window.render_frame(cx));
            let image = cx
                .capture_screenshot(shot.window)
                .expect("a headless renderer");
            if let Some(dir) = &dir {
                let path = dir.join(format!("{frame}-{name}.png"));
                image.save(&path).expect("png written");
                eprintln!("window_shots: {}", path.display());
            }
        }
    }
}
