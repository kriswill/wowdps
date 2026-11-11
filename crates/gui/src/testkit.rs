//! What the GUI's tests open windows with (spec §9): the daemon's in-process
//! mock as a `Link`, and the two contexts.
//!
//! - **Interaction** runs under `#[gpui_kit::test]`'s `TestAppContext`:
//!   deterministic, with Kit's `TestWindowExt` clicking through GPUI's own
//!   hit testing — but its text system is a stub (every glyph 0.6 em, fonts
//!   ignored).
//! - **Anything whose answer depends on text metrics** (heights, widths,
//!   truncation, the chrome budget) runs in `headless()`: a
//!   `HeadlessAppContext` over the Linux platform's real cosmic-text system,
//!   reached through Kit's `platform::current_platform(true)` (spike S5's
//!   text route, with no direct dependency on `gpui-pre-wgpu`).

use std::sync::Arc;

use gpui_kit::{
    AnyWindowHandle, App, Bounds, Entity, HeadlessAppContext, Pixels, Point, Render, Size,
    TestAppContext, Window, WindowBounds, WindowOptions,
};
use wowdps_daemon::mock::MockDaemon;
use wowdps_proto::{ClientMsg, DaemonMsg, Reconnect};

use crate::session::Link;

/// The daemon's mock as a link: what a send answers is waiting for the
/// next poll, as the real socket's replies would be.
pub struct MockLink {
    mock: MockDaemon,
    inbox: Vec<DaemonMsg>,
}

impl MockLink {
    pub fn new(mock: MockDaemon) -> Self {
        Self {
            mock,
            inbox: Vec::new(),
        }
    }

    /// The committed `sample.txt`: two encounters and trash, three players.
    pub fn fixture() -> Self {
        Self::new(MockDaemon::fixture())
    }

    /// A committed fixture by name (`tree.txt`, `taken.txt` …).
    pub fn at(name: &str) -> Self {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../core/fixtures")
            .join(name);
        Self::new(MockDaemon::fixture_at(&path))
    }
}

impl Link for MockLink {
    fn send(&mut self, msg: &ClientMsg) {
        let replies = self.mock.handle(msg.clone());
        self.inbox.extend(replies);
    }

    fn poll(&mut self) -> Vec<DaemonMsg> {
        std::mem::take(&mut self.inbox)
    }

    fn reconnect(&mut self) -> Reconnect {
        Reconnect::Connected
    }
}

/// v49: a daemon of two editions, as one following a source per edition
/// is: Retail's `sample.txt` is followed, the store holds both its fights
/// and Forever's `forever.txt`'s, and — `with_forever_source` — Forever's
/// log is followed too. A Watch is answered by the edition's source (a pin
/// to an edition no source carries by the empty answer, as the daemon
/// gives it); following, by the live edition's. Every history question
/// goes to the one store. What the window sent is kept for a test to read.
pub struct EditionsLink {
    retail: MockDaemon,
    forever: Option<MockDaemon>,
    /// The live edition: what a following session is answered by.
    pub live: wowdps_model::Edition,
    edition: wowdps_proto::EditionRef,
    inbox: Vec<DaemonMsg>,
    pub sent: std::rc::Rc<std::cell::RefCell<Vec<ClientMsg>>>,
}

impl EditionsLink {
    fn fixture(name: &str) -> std::path::PathBuf {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../core/fixtures")
            .join(name)
    }

    /// Retail followed, both editions' fights stored.
    pub fn new() -> Self {
        Self::owned(&[])
    }

    /// As `new`, the store naming `characters` the account's (the config's
    /// `history_characters`), so its cards have owners in both editions.
    pub fn owned(characters: &[&str]) -> Self {
        let names: Vec<String> = characters.iter().map(|c| c.to_string()).collect();
        let retail = MockDaemon::fixture()
            .with_characters(&names)
            .with_history()
            .with_history_of(&Self::fixture("forever.txt"));
        Self {
            retail,
            forever: None,
            live: wowdps_model::Edition::Retail,
            edition: wowdps_proto::EditionRef::Follow,
            inbox: Vec::new(),
            sent: std::rc::Rc::default(),
        }
    }

    /// Forever's log followed as well.
    pub fn with_forever_source(mut self) -> Self {
        self.forever = Some(MockDaemon::fixture_at(&Self::fixture("forever.txt")));
        self
    }

    /// The mock that answers the session's edition.
    fn source(&mut self) -> &mut MockDaemon {
        use wowdps_model::Edition;
        let edition = match self.edition {
            wowdps_proto::EditionRef::Follow => self.live,
            wowdps_proto::EditionRef::Pin(e) => e.counts_as(),
        };
        match (&mut self.forever, edition) {
            (Some(f), Edition::Forever) => f,
            _ => &mut self.retail,
        }
    }
}

impl Default for EditionsLink {
    fn default() -> Self {
        Self::new()
    }
}

impl Link for EditionsLink {
    fn send(&mut self, msg: &ClientMsg) {
        self.sent.borrow_mut().push(msg.clone());
        let replies = match msg {
            ClientMsg::Watch { edition, .. } => {
                self.edition = *edition;
                self.source().handle(msg.clone())
            }
            ClientMsg::GetLoadout { .. } => self.source().handle(msg.clone()),
            ClientMsg::GetStatus { .. } => {
                let live = self.live;
                let forever = self.forever.is_some();
                let mut replies = self.retail.handle(msg.clone());
                for r in &mut replies {
                    if let DaemonMsg::Status { editions, .. } = r {
                        for e in editions.iter_mut() {
                            e.live = live == wowdps_model::Edition::Retail;
                        }
                        if forever {
                            editions.push(wowdps_proto::EditionSource {
                                edition: Some(wowdps_model::Edition::Forever),
                                source: Some("forever.txt".to_string()),
                                log: Some("forever.txt".to_string()),
                                live: live == wowdps_model::Edition::Forever,
                                running: false,
                            });
                        }
                    }
                }
                replies
            }
            _ => self.retail.handle(msg.clone()),
        };
        self.inbox.extend(replies);
    }

    fn poll(&mut self) -> Vec<DaemonMsg> {
        std::mem::take(&mut self.inbox)
    }

    fn reconnect(&mut self) -> Reconnect {
        Reconnect::Connected
    }
}

/// A link that answers nothing: a state built by hand
/// (`wowdps_gui_logic::raid`) stays exactly as it was built.
pub struct NullLink;

impl Link for NullLink {
    fn send(&mut self, _: &ClientMsg) {}

    fn poll(&mut self) -> Vec<DaemonMsg> {
        Vec::new()
    }

    fn reconnect(&mut self) -> Reconnect {
        Reconnect::Connected
    }
}

fn options(size: Size<Pixels>) -> WindowOptions {
    WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(Bounds {
            origin: Point::default(),
            size,
        })),
        focus: false,
        show: false,
        ..Default::default()
    }
}

/// A window of `size` in the deterministic test app, through the same
/// `gpui_kit::open_window` (Kit's Root around the view) the window uses.
pub fn open<V: Render>(
    cx: &mut TestAppContext,
    size: Size<Pixels>,
    build: impl FnOnce(&mut Window, &mut App) -> Entity<V>,
) -> (AnyWindowHandle, Entity<V>) {
    cx.update(gpui_kit::init);
    cx.update(|cx| gpui_kit::open_window(options(size), cx, build))
        .expect("a test window opens")
}

/// The theme the shot tests draw in: `WOWDPS_SHOTS_THEME` names a built-in
/// (`onyx`, `frost`, …), else `navy`. The render guard never reads it.
pub fn shots_theme() -> &'static wowdps_gui_logic::theme::Def {
    std::env::var("WOWDPS_SHOTS_THEME")
        .ok()
        .and_then(|name| wowdps_gui_logic::theme::builtin(&name))
        .unwrap_or(&wowdps_gui_logic::theme::NAVY)
}

/// An app with the platform's real text system and the headless renderer.
pub fn headless() -> HeadlessAppContext {
    let mut cx = HeadlessAppContext::with_platform(
        gpui_kit::platform::current_platform(true).text_system(),
        Arc::new(gpui_kit::assets::Assets),
        gpui_kit::platform::current_headless_renderer,
    );
    cx.update(gpui_kit::init);
    cx
}

/// A window of `size` in a `headless()` app, through `gpui_kit::open_window`.
pub fn open_headless<V: Render>(
    cx: &mut HeadlessAppContext,
    size: Size<Pixels>,
    build: impl FnOnce(&mut Window, &mut App) -> Entity<V>,
) -> (AnyWindowHandle, Entity<V>) {
    cx.update(|cx| gpui_kit::open_window(options(size), cx, build))
        .expect("a headless window opens")
}
