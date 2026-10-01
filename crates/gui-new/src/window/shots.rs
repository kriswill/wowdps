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

/// What the shots run over: a log and the fight to land on, or the fixture.
struct Input {
    log: Option<PathBuf>,
    fight: String,
    owner: String,
}

impl Input {
    fn from_env() -> Self {
        Input {
            log: std::env::var_os("WOWDPS_SHOTS_LOG").map(PathBuf::from),
            fight: std::env::var("WOWDPS_SHOTS_FIGHT")
                .unwrap_or_else(|_| "The Coiled Altar".into()),
            owner: std::env::var("WOWDPS_SHOTS_OWNER").unwrap_or_else(|_| "Tranqlock".into()),
        }
    }

    fn link(&self) -> MockLink {
        match &self.log {
            Some(log) => MockLink::new(wowdps_daemon::mock::MockDaemon::fixture_at(log)),
            None => MockLink::fixture(),
        }
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
fn app() -> HeadlessAppContext {
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
    vec![
        ("damage", |_, _| {}),
        ("home", |cx, s| {
            cx.update_entity(&s.gui, |g, cx| {
                g.place = super::Place::Home;
                cx.notify();
            })
        }),
    ]
}

#[test]
#[ignore = "needs a wgpu adapter"]
fn window_shots() {
    let dir = std::env::var_os("WOWDPS_SHOTS_DIR").map(PathBuf::from);
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
