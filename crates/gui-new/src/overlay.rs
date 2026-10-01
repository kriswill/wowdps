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
mod scrollbar;
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

/// Which output the overlay is born on — a layer surface never changes
/// output. `WOWDPS_OVERLAY_OUTPUT` (by hand) or a configured `monitor`
/// wins; else, under Hyprland with `follow_game`, the monitor showing the
/// game's workspace, so a daemon-spawned overlay never follows the user's
/// focus onto the terminal's screen. The daemon spawns the overlay on the
/// game PROCESS (`WOWDPS_OVERLAY_GAME_STARTING`) and Proton maps the window
/// seconds later, so that wait is up to `GAME_WINDOW_WAIT`; a hand launch
/// asks once. `None`: the compositor chooses. Called before the app starts:
/// the wait never blocks a frame.
pub fn choose_output(cfg: &Config) -> Option<String> {
    use wowdps_gui_logic::hypr;
    if let Ok(name) = std::env::var("WOWDPS_OVERLAY_OUTPUT") {
        return Some(name);
    }
    cfg.monitor.clone().or_else(|| {
        if !cfg.follow_game {
            return None;
        }
        let dir = hypr::socket_dir()?;
        if std::env::var_os("WOWDPS_OVERLAY_GAME_STARTING").is_some() {
            let deadline = std::time::Instant::now() + GAME_WINDOW_WAIT;
            hypr::game_monitor_wait(&dir, &cfg.game_match, deadline)
        } else {
            hypr::game_monitor(&dir, &cfg.game_match)
        }
    })
}

/// How long a daemon-spawned overlay waits for the game window to map
/// before it picks an output.
const GAME_WINDOW_WAIT: std::time::Duration = std::time::Duration::from_secs(30);

#[cfg(target_os = "linux")]
fn surface(
    cx: &mut App,
    output: Option<&str>,
    client: DaemonClient,
    cfg: Config,
) -> Result<(), String> {
    use std::cell::Cell;
    use std::rc::Rc;

    use gpui_kit::AppContext as _;

    let display_id = output.and_then(|name| display_named(cx, name));
    if let (Some(name), None) = (output, display_id) {
        eprintln!("wowdps-gui-new: no output named {name}; the compositor chooses");
    }
    // WOWDPS_OVERLAY_AUTOVIEW: start on that view, before the first Watch.
    let mut state = wowdps_proto::ClientState::new();
    if let Some(view) = panel::start_view() {
        state.view = view;
    }
    let session = cx.new(|cx| crate::session::Session::running_with(Box::new(client), state, cx));
    let overlay = cx.new(|cx| panel::Overlay::new(session, cfg, Rc::new(split_link), cx));
    let current = Rc::new(Cell::new(Some(open_on(cx, output, display_id, &overlay)?)));
    // A tab dropped by another edge: the surface is recreated there — the
    // new one opened before the old one goes, so the app never stands
    // without a window.
    let held = Rc::clone(&current);
    let output = output.map(str::to_string);
    cx.subscribe(&overlay, move |overlay, _: &panel::Reanchor, cx| {
        overlay.update(cx, |o, _| o.fresh_surface());
        match open_on(cx, output.as_deref(), display_id, &overlay) {
            Ok(new) => {
                if let Some(old) = held.replace(Some(new)) {
                    let _ = old.update(cx, |_, window, _| window.remove_window());
                }
            }
            Err(e) => eprintln!("wowdps-gui-new: the overlay cannot move edge: {e}"),
        }
    })
    .detach();
    // The surface gone — its output unplugged, the compositor closing it:
    // the process ends, and the daemon's supervisor takes it from there.
    cx.on_window_closed(|cx, _| {
        if cx.windows().is_empty() {
            cx.quit();
        }
    })
    .detach();
    Ok(())
}

/// A layer surface for `overlay` on its edge: anchored to the edge and both
/// of its neighbours, so it spans the edge's length (the compositor's
/// configure says how long), as thick as the overlay's content across it.
#[cfg(target_os = "linux")]
fn open_on(
    cx: &mut App,
    output: Option<&str>,
    display_id: Option<gpui_kit::DisplayId>,
    overlay: &gpui_kit::Entity<panel::Overlay>,
) -> Result<gpui_kit::AnyWindowHandle, String> {
    use gpui_kit::layer_shell::{Anchor, KeyboardInteractivity, Layer, LayerShellOptions};
    use gpui_kit::{
        Bounds, WindowBackgroundAppearance, WindowBounds, WindowKind, WindowOptions, point, px,
        size,
    };
    use wowdps_gui_logic::config::Edge;

    let (edge, across) = {
        let o = overlay.read(cx);
        let (w, h) = wowdps_gui_logic::surface::surface_size(&o.cfg, o.expanded, false);
        (o.cfg.edge, (w as f32, h as f32))
    };
    // The edge's full length. Not zero, which layer-shell reads as "stretch":
    // GPUI hands every size to the surface's viewport too, and a zero there
    // is a protocol error that ends the connection.
    let length = edge_length(cx, output, display_id, edge);
    let (anchor, (w, h)) = match edge {
        Edge::Left => (
            Anchor::LEFT | Anchor::TOP | Anchor::BOTTOM,
            (across.0, length),
        ),
        Edge::Right => (
            Anchor::RIGHT | Anchor::TOP | Anchor::BOTTOM,
            (across.0, length),
        ),
        Edge::Top => (
            Anchor::TOP | Anchor::LEFT | Anchor::RIGHT,
            (length, across.1),
        ),
        Edge::Bottom => (
            Anchor::BOTTOM | Anchor::LEFT | Anchor::RIGHT,
            (length, across.1),
        ),
    };
    let options = WindowOptions {
        display_id,
        titlebar: None,
        window_bounds: Some(WindowBounds::Windowed(Bounds {
            origin: point(px(0.), px(0.)),
            size: size(px(w), px(h)),
        })),
        window_background: WindowBackgroundAppearance::Transparent,
        focus: false,
        kind: WindowKind::LayerShell(LayerShellOptions {
            namespace: "wowdps".to_string(),
            // The only layer that stacks above a fullscreen game.
            layer: Layer::Overlay,
            anchor,
            // Over every other surface's exclusive zone (a bar), as iced's.
            exclusive_zone: Some(px(-1.)),
            keyboard_interactivity: KeyboardInteractivity::None,
            ..Default::default()
        }),
        ..Default::default()
    };
    // Straight through GPUI, not `gpui_kit::open_window`: Kit's Root paints
    // the theme's ground under the content and, on a client-decorated
    // surface (a layer surface always is), a 20 px shadow border that it
    // also claims as the client inset (spike S1).
    let root = overlay.clone();
    cx.open_window(options, move |_, _| root)
        .map(Into::into)
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

/// How long `edge` of the overlay's output is, in logical pixels: from
/// Hyprland when it names the output — it accounts for rotation and
/// fractional scale, which GPUI's display bounds do not (spec §7.2) — else
/// from those bounds. The compositor's configure has the last word.
#[cfg(target_os = "linux")]
fn edge_length(
    cx: &App,
    output: Option<&str>,
    display_id: Option<gpui_kit::DisplayId>,
    edge: wowdps_gui_logic::config::Edge,
) -> f32 {
    use wowdps_gui_logic::hypr;
    let vertical = edge.is_vertical();
    let from_hypr = output.and_then(|name| {
        let (_, _, w, h) = hypr::monitor_named(&hypr::socket_dir()?, name)?;
        Some(if vertical { h } else { w } as f32)
    });
    let from_gpui = || {
        let display = display_id
            .and_then(|id| cx.find_display(id))
            .or_else(|| cx.primary_display())?;
        let size = display.bounds().size;
        Some(f32::from(if vertical { size.height } else { size.width }))
    };
    from_hypr
        .or_else(from_gpui)
        .filter(|l| *l > 0.0)
        .unwrap_or(if vertical { 1080.0 } else { 1920.0 })
}
