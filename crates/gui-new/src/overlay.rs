//! `--overlay`: the meter as a layer surface over the game (spec §7). The
//! view is `panel::Overlay`; this module opens its surface.
//!
//! Step 2.1 places the surface as the iced overlay does — anchored to the
//! edge and the corner the offset counts from, the offset a margin — and
//! sizes it for the state (`gui_logic::surface`). Step 2.2 moves it onto
//! the edge strip (`strip.rs`), where a drag moves the content and never
//! the surface.

use gpui_kit::App;
use wowdps_gui_logic::config::Config;
use wowdps_proto::DaemonClient;

mod drill;
mod graph;
mod instance;
mod ov;
pub(crate) mod panel;
mod rows;
#[cfg_attr(
    not(test),
    expect(dead_code, reason = "the surface uses it from step 2.2")
)]
mod strip;

/// Open the overlay once its output is known. Wayland names its outputs
/// only after the event loop turns, so a named output is waited for —
/// briefly: a second, then the compositor chooses. A surface that cannot
/// open goes to `failed`.
pub fn open(
    cx: &mut App,
    output: Option<String>,
    client: DaemonClient,
    cfg: Config,
    failed: impl FnOnce(String, &mut App) + 'static,
) {
    cx.spawn(async move |cx| {
        if let Some(name) = output.as_deref() {
            for _ in 0..40 {
                if cx.update(|cx| display_named(cx, name).is_some()) {
                    break;
                }
                cx.background_executor()
                    .timer(std::time::Duration::from_millis(25))
                    .await;
            }
        }
        cx.update(|cx| {
            if let Err(e) = surface(cx, output.as_deref(), client, cfg) {
                failed(e, cx);
            }
        });
    })
    .detach();
}

/// The display GPUI keys by the UUID of `name` (spike S4).
fn display_named(cx: &App, name: &str) -> Option<gpui_kit::DisplayId> {
    let want = wowdps_gui_logic::output::output_uuid(name);
    cx.displays()
        .into_iter()
        .find(|d| d.uuid().is_ok_and(|uuid| *uuid.as_bytes() == want))
        .map(|d| d.id())
}

#[cfg(target_os = "linux")]
fn surface(
    cx: &mut App,
    output: Option<&str>,
    client: DaemonClient,
    cfg: Config,
) -> Result<(), String> {
    use gpui_kit::AppContext as _;
    use gpui_kit::layer_shell::{Anchor, KeyboardInteractivity, Layer, LayerShellOptions};
    use gpui_kit::{
        Bounds, WindowBackgroundAppearance, WindowBounds, WindowKind, WindowOptions, point, px,
        size,
    };
    use wowdps_gui_logic::config::Edge;

    let display_id = output.and_then(|name| display_named(cx, name));
    if let (Some(name), None) = (output, display_id) {
        eprintln!("wowdps-gui-new: no output named {name}; the compositor chooses");
    }
    let expanded = std::env::var_os("WOWDPS_OVERLAY_START_EXPANDED").is_some();
    let (w, h) = wowdps_gui_logic::surface::surface_size(&cfg, expanded, false);
    // Anchored to the corner the offset counts from: the top of a side
    // edge, the left of a horizontal one.
    let offset = px(cfg.offset.max(0) as f32);
    let (anchor, margin) = match cfg.edge {
        Edge::Left => (Anchor::LEFT | Anchor::TOP, (offset, px(0.), px(0.), px(0.))),
        Edge::Right => (
            Anchor::RIGHT | Anchor::TOP,
            (offset, px(0.), px(0.), px(0.)),
        ),
        Edge::Top => (Anchor::TOP | Anchor::LEFT, (px(0.), px(0.), px(0.), offset)),
        Edge::Bottom => (
            Anchor::BOTTOM | Anchor::LEFT,
            (px(0.), px(0.), px(0.), offset),
        ),
    };
    let options = WindowOptions {
        display_id,
        titlebar: None,
        window_bounds: Some(WindowBounds::Windowed(Bounds {
            origin: point(px(0.), px(0.)),
            size: size(px(w as f32), px(h as f32)),
        })),
        window_background: WindowBackgroundAppearance::Transparent,
        focus: false,
        kind: WindowKind::LayerShell(LayerShellOptions {
            namespace: "wowdps".to_string(),
            // The only layer that stacks above a fullscreen game.
            layer: Layer::Overlay,
            anchor,
            margin: Some(margin),
            keyboard_interactivity: KeyboardInteractivity::None,
            ..Default::default()
        }),
        ..Default::default()
    };
    // Straight through GPUI, not `gpui_kit::open_window`: Kit's Root paints
    // the theme's ground under the content and, on a client-decorated
    // surface (a layer surface always is), a 20 px shadow border that it
    // also claims as the client inset (spike S1).
    cx.open_window(options, |_, cx| {
        let session = cx.new(|cx| crate::session::Session::running(client, cx));
        cx.new(|cx| panel::Overlay::new(session, cfg, std::rc::Rc::new(split_link), cx))
    })
    .map(|_| ())
    .map_err(|e| format!("cannot open the overlay: {e}"))
}

/// The Σ split's own daemon connection: a `Window` kind, so the daemon's
/// `SetVisible` — the overlay's alone — never reaches it, holding only the
/// top rows.
#[cfg(target_os = "linux")]
fn split_link(cx: &mut App) -> Result<gpui_kit::Entity<crate::session::Session>, String> {
    use gpui_kit::AppContext as _;
    use wowdps_proto::{ClientKind, ClientState};
    let client = DaemonClient::connect(
        &wowdps_gui_logic::sibling::daemon_bin(),
        None,
        ClientKind::Window,
    )
    .map_err(|e| format!("cannot reach the wowdps daemon: {e}"))?;
    let state = ClientState::with_top_n(Some(panel::AUX_TOP_N));
    Ok(cx.new(|cx| crate::session::Session::running_with(Box::new(client), state, cx)))
}

#[cfg(not(target_os = "linux"))]
fn surface(_: &mut App, _: Option<&str>, _: DaemonClient, _: Config) -> Result<(), String> {
    Err("the overlay is a wlr-layer-shell surface, which only Linux has".to_string())
}
