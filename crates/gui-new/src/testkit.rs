//! What gui-new's tests open windows with (spec §9): the daemon's in-process
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
