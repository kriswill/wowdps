//! `--overlay`. In step 1.1 it is an empty, keyboard-less layer surface on
//! the overlay layer, tab-sized at the right edge: proof that stock
//! gpui-pre opens one. Spikes S1–S4 settle the rest — click-through, the
//! edge strip, the output — before phase 2 builds the overlay on it.

use gpui_kit::App;

#[cfg(target_os = "linux")]
pub fn open(cx: &mut App) -> Result<(), String> {
    use gpui_kit::component::ActiveTheme;
    use gpui_kit::layer_shell::{Anchor, KeyboardInteractivity, Layer, LayerShellOptions};
    use gpui_kit::prelude::*;
    use gpui_kit::{
        Bounds, Context, Window, WindowBackgroundAppearance, WindowBounds, WindowKind,
        WindowOptions, div, point, px, size,
    };

    struct Tab;

    impl Render for Tab {
        fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            let theme = cx.theme();
            div()
                .size_full()
                .rounded_l_md()
                .bg(theme.background)
                .border_1()
                .border_color(theme.border)
        }
    }

    let options = WindowOptions {
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
pub fn open(_: &mut App) -> Result<(), String> {
    Err("the overlay is a wlr-layer-shell surface, which only Linux has".to_string())
}
