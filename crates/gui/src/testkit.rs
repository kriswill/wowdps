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
/// is: Retail's `sample.txt` is followed, the store holds its fights and —
/// unless built without — Forever's `forever.txt`'s, and with
/// `with_forever_log` a Forever log is followed too. A Watch is answered by
/// the edition's source (a pin to an edition no source carries by the
/// empty answer, as the daemon gives it); following, by the live edition's,
/// and a following session is moved (sent the new live source's list)
/// when a test moves `live`. `playing` is the game the watcher sees
/// running. What the window sent is kept for a test to read.
pub struct EditionsLink {
    retail: MockDaemon,
    forever: Option<MockDaemon>,
    /// The live edition: what a following session is answered by.
    pub live: std::rc::Rc<std::cell::Cell<wowdps_model::Edition>>,
    /// The edition whose game runs, as `Status` says.
    pub playing: std::rc::Rc<std::cell::Cell<Option<wowdps_model::Edition>>>,
    /// The live edition a following session was last answered by.
    answered: wowdps_model::Edition,
    edition: wowdps_proto::EditionRef,
    inbox: Vec<DaemonMsg>,
    pub sent: std::rc::Rc<std::cell::RefCell<Vec<ClientMsg>>>,
}

impl EditionsLink {
    pub fn fixture(name: &str) -> std::path::PathBuf {
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
        Self::stored(characters, true)
    }

    /// Retail followed and stored, and Forever's fights stored with
    /// `forever`.
    pub fn stored(characters: &[&str], forever: bool) -> Self {
        let names: Vec<String> = characters.iter().map(|c| c.to_string()).collect();
        let mut retail = MockDaemon::fixture().with_characters(&names).with_history();
        if forever {
            retail = retail.with_history_of(&Self::fixture("forever.txt"));
        }
        Self {
            retail,
            forever: None,
            live: std::rc::Rc::new(std::cell::Cell::new(wowdps_model::Edition::Retail)),
            playing: std::rc::Rc::default(),
            answered: wowdps_model::Edition::Retail,
            edition: wowdps_proto::EditionRef::Follow,
            inbox: Vec::new(),
            sent: std::rc::Rc::default(),
        }
    }

    /// Retail's log cut mid-pull (`MockDaemon::fixture_live`): its last
    /// fight still open and the game running, so the pill says "Live".
    /// `characters` owned, Forever's fights stored, as [`Self::owned`].
    pub fn with_retail_live(mut self, characters: &[&str]) -> Self {
        let names: Vec<String> = characters.iter().map(|c| c.to_string()).collect();
        self.retail = MockDaemon::fixture_live()
            .with_characters(&names)
            .with_history()
            .with_history_of(&Self::fixture("forever.txt"));
        self
    }

    /// Forever's `forever.txt` followed as well.
    pub fn with_forever_source(self) -> Self {
        self.with_forever_log(&Self::fixture("forever.txt"))
    }

    /// A Forever log at `path` followed as well.
    pub fn with_forever_log(mut self, path: &std::path::Path) -> Self {
        self.forever = Some(MockDaemon::fixture_at(path));
        self
    }

    /// The edition answering the session now.
    fn answering(&self) -> wowdps_model::Edition {
        match self.edition {
            wowdps_proto::EditionRef::Follow => self.live.get(),
            wowdps_proto::EditionRef::Pin(e) => e.counts_as(),
        }
    }

    /// The mock that answers the session's edition.
    fn source(&mut self) -> &mut MockDaemon {
        let edition = self.answering();
        match (&mut self.forever, edition) {
            (Some(f), wowdps_model::Edition::Forever) => f,
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
        use wowdps_model::Edition;
        self.sent.borrow_mut().push(msg.clone());
        let replies = match msg {
            ClientMsg::Watch { edition, .. } => {
                self.edition = *edition;
                self.answered = self.live.get();
                self.source().handle(msg.clone())
            }
            ClientMsg::GetLoadout { .. } => self.source().handle(msg.clone()),
            ClientMsg::GetStatus { .. } => {
                let live = self.live.get();
                let playing = self.playing.get();
                let forever = self.forever.is_some();
                let mut replies = self.retail.handle(msg.clone());
                for r in &mut replies {
                    if let DaemonMsg::Status { editions, .. } = r {
                        for e in editions.iter_mut() {
                            e.live = live == Edition::Retail;
                            e.running = playing == Some(Edition::Retail);
                        }
                        if forever {
                            editions.push(wowdps_proto::EditionSource {
                                edition: Some(Edition::Forever),
                                source: Some("forever.txt".to_string()),
                                log: Some("forever.txt".to_string()),
                                live: live == Edition::Forever,
                                running: playing == Some(Edition::Forever),
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
        // The live edition moved under a following session: the daemon
        // sends it the new live source's list, the frame that moves it.
        if self.edition == wowdps_proto::EditionRef::Follow && self.live.get() != self.answered {
            self.answered = self.live.get();
            let list = self
                .source()
                .handle(ClientMsg::watch(wowdps_proto::Cursor::List));
            self.inbox.extend(list);
        }
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
