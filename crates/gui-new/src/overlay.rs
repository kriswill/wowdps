//! `--overlay`. In step 1.1 it is an empty, keyboard-less layer surface on
//! the overlay layer, tab-sized at the right edge: proof that stock
//! gpui-pre opens one. Spikes S1–S4 settle the rest — click-through, the
//! edge strip, the output — before phase 2 builds the overlay on it.

use gpui_kit::App;

#[cfg_attr(
    not(test),
    expect(dead_code, reason = "the surface uses it from step 2.2")
)]
mod strip;

/// Open the overlay once its output is known. Wayland names its outputs
/// only after the event loop turns, so a named output is waited for —
/// briefly: a second, then the compositor chooses. A surface that cannot
/// open goes to `failed`.
pub fn open(cx: &mut App, output: Option<String>, failed: impl FnOnce(String, &mut App) + 'static) {
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
            if let Err(e) = surface(cx, output.as_deref()) {
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
fn surface(cx: &mut App, output: Option<&str>) -> Result<(), String> {
    use gpui_kit::layer_shell::{Anchor, KeyboardInteractivity, Layer, LayerShellOptions};
    use gpui_kit::prelude::*;
    use gpui_kit::{
        Bounds, Context, Window, WindowBackgroundAppearance, WindowBounds, WindowKind,
        WindowOptions, div, point, px, size,
    };

    struct Tab;

    impl Render for Tab {
        fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            let look = crate::theme::Look::global(cx);
            div()
                .size_full()
                .rounded_l_md()
                .bg(look.o(|t| t.panel))
                .border_1()
                .border_color(look.o(|t| t.dim.alpha(0.4)))
        }
    }

    // The named output, found by the UUID GPUI derives from its name
    // (spike S4); none named, or none found, leaves it to the compositor.
    let display_id = output.and_then(|name| display_named(cx, name));
    if let (Some(name), None) = (output, display_id) {
        eprintln!("wowdps-gui-new: no output named {name}; the compositor chooses");
    }
    let options = WindowOptions {
        display_id,
        titlebar: None,
        window_bounds: Some(WindowBounds::Windowed(Bounds {
            origin: point(px(0.), px(0.)),
            size: size(px(28.), px(96.)),
        })),
        app_id: Some("wowdps-gui-new".to_string()),
        window_background: WindowBackgroundAppearance::Transparent,
        focus: false,
        kind: WindowKind::LayerShell(LayerShellOptions {
            namespace: "wowdps-gui-new".to_string(),
            // The only layer that stacks above a fullscreen game.
            layer: Layer::Overlay,
            anchor: Anchor::RIGHT,
            keyboard_interactivity: KeyboardInteractivity::None,
            ..Default::default()
        }),
        ..Default::default()
    };
    // Straight through GPUI, not `gpui_kit::open_window`: Kit's Root paints
    // the theme's ground under the content and, on a client-decorated
    // surface (a layer surface always is), a 20 px shadow border that it
    // also claims as the client inset, so a 28 × 96 tab mapped as 68 × 136.
    cx.open_window(options, |_, cx| cx.new(|_| Tab))
        .map(|_| ())
        .map_err(|e| format!("cannot open the overlay: {e}"))
}

#[cfg(not(target_os = "linux"))]
fn surface(_: &mut App, _: Option<&str>) -> Result<(), String> {
    Err("the overlay is a wlr-layer-shell surface, which only Linux has".to_string())
}
