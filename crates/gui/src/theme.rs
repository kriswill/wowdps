//! The active theme (spec §6.1). gui-logic's `theme::Def` is the one
//! definition; `apply` maps it onto two targets:
//!
//! - GPUI Kit's `Theme`, slot by slot, so every Kit component wears it;
//! - our `Look`, a global holding the definition and the chrome accent, for
//!   every surface Kit has no slot for (inks, gold-dim, the overlay).
//!
//! No surface draws a literal colour: each reads `Look` (or Kit's theme),
//! so `apply` with another definition repaints everything.
//!
//! Kit's slot names are shadcn's: `primary` is the brand colour (a
//! primary button, a checked box, the selected tab), `accent` is the
//! hover wash behind a menu or list item. So the chrome (the game's gold,
//! or the owner's class) goes to `primary` and `ring`, and `accent` is the
//! prototype's raise.

use gpui_kit::component::{Theme, ThemeMode};
use gpui_kit::{App, Global, Hsla, Rgba, px};
use wowdps_gui_logic::theme::{self as gl, Accent, Def};

/// A gui-logic colour as GPUI's.
pub fn hsla(c: gl::Color) -> Hsla {
    Rgba {
        r: c.r,
        g: c.g,
        b: c.b,
        a: c.a,
    }
    .into()
}

/// The active definition and chrome, for every surface that draws itself.
#[derive(Clone, Copy, Debug)]
pub struct Look {
    pub def: &'static Def,
    /// The chrome: the theme's own accent, or the owner's class.
    pub accent: Accent,
}

impl Global for Look {}

impl Look {
    /// The active look; `gold` with its own chrome before any `apply` (a
    /// test that opens a bare window).
    pub fn global(cx: &App) -> Look {
        cx.try_global::<Look>().copied().unwrap_or(Look {
            def: &gl::GOLD,
            accent: own_accent(&gl::GOLD),
        })
    }

    /// The window's colour for a token, as GPUI's (the window draws
    /// through its `W`; the step-1.2 meter test still reads this).
    #[cfg(test)]
    pub fn w(&self, pick: impl FnOnce(&gl::WindowTokens) -> gl::Color) -> Hsla {
        hsla(pick(&self.def.window))
    }
}

/// The chrome a definition wears by itself: its own accent token.
pub fn own_accent(def: &Def) -> Accent {
    Accent {
        base: def.window.gold,
        ink: def.window.gold_ink,
    }
}

/// Make `def` the active theme, with `accent` as the chrome (a class
/// chrome's; `None` is the theme's own), and repaint every window.
pub fn apply(def: &'static Def, accent: Option<Accent>, cx: &mut App) {
    let accent = accent.unwrap_or_else(|| own_accent(def));
    let mode = if def.dark {
        ThemeMode::Dark
    } else {
        ThemeMode::Light
    };
    // A mode switch reloads Kit's registered theme over the colours, so it
    // comes first and the edits after it.
    if Theme::global(cx).mode != mode {
        Theme::change(mode, None, cx);
    }
    Theme::update(cx, |theme| map(def, accent, theme));
    cx.set_global(Look { def, accent });
    cx.refresh_windows();
}

fn map(def: &Def, accent: Accent, theme: &mut Theme) {
    let w = &def.window;
    let h = hsla;
    let hover = w.hover.over(w.ground);
    theme.font_family = def.faces.ui.into();
    // The tabular digits are baked into the face, so it is the mono face too.
    theme.mono_font_family = def.faces.ui.into();
    theme.font_size = px(14.5);
    theme.radius = px(def.radius);
    theme.radius_lg = px(def.radius + 2.0);
    theme.shadow = true;

    let c = &mut theme.colors;
    c.background = h(w.ground);
    c.foreground = h(w.ink);
    c.border = h(w.line);
    c.input = h(w.line);
    c.ring = h(accent.base);
    c.caret = h(w.ink);
    c.selection = h(w.selection);
    c.link = h(accent.base);
    c.link_hover = h(accent.base.lighten(0.15));
    c.link_active = h(accent.base.darken(0.1));
    c.window_border = h(w.edge);
    c.overlay = h(w.scrim);

    c.primary = h(accent.base);
    c.primary_foreground = h(accent.ink);
    c.primary_hover = h(accent.base.lighten(0.08));
    c.primary_active = h(accent.base.darken(0.08));
    c.secondary = h(w.raise);
    c.secondary_foreground = h(w.ink);
    c.secondary_hover = h(w.raise.lighten(0.04));
    c.secondary_active = h(w.raise.darken(0.04));
    c.accent = h(w.raise);
    c.accent_foreground = h(w.ink);
    c.muted = h(w.surface);
    c.muted_foreground = h(w.ink_2);

    // Kit's buttons read slots of their own. The plain one is the
    // prototype's `.btn`: no fill, its words in ink 2, the pointer's wash.
    c.button = h(gl::Color::TRANSPARENT);
    c.button_foreground = h(w.ink_2);
    c.button_hover = h(hover);
    c.button_active = h(w.raise);
    c.button_primary = h(accent.base);
    c.button_primary_foreground = h(accent.ink);
    c.button_primary_hover = h(accent.base.lighten(0.08));
    c.button_primary_active = h(accent.base.darken(0.08));
    c.button_secondary = h(w.raise);
    c.button_secondary_foreground = h(w.ink);
    c.button_secondary_hover = h(w.raise.lighten(0.04));
    c.button_secondary_active = h(w.raise.darken(0.04));
    c.button_danger = h(w.bad);
    c.button_danger_foreground = h(w.ink);
    c.button_danger_hover = h(w.bad.lighten(0.08));
    c.button_danger_active = h(w.bad.darken(0.08));
    c.button_success = h(w.good);
    c.button_success_foreground = h(w.gold_ink);
    c.button_success_hover = h(w.good.lighten(0.08));
    c.button_success_active = h(w.good.darken(0.08));
    c.button_warning = h(w.amber);
    c.button_warning_foreground = h(w.gold_ink);
    c.button_warning_hover = h(w.amber.lighten(0.08));
    c.button_warning_active = h(w.amber.darken(0.08));
    c.button_info = h(accent.base);
    c.button_info_foreground = h(accent.ink);
    c.button_info_hover = h(accent.base.lighten(0.08));
    c.button_info_active = h(accent.base.darken(0.08));

    c.popover = h(w.surface);
    c.popover_foreground = h(w.ink);

    c.list = h(w.ground);
    c.list_even = h(w.ground);
    c.list_head = h(w.surface);
    c.list_hover = h(hover);
    c.list_active = h(w.raise);
    c.list_active_border = h(accent.base);
    c.table = h(w.ground);
    c.table_even = h(w.ground);
    c.table_head = h(w.surface);
    c.table_head_foreground = h(w.gold_dim);
    c.table_hover = h(hover);
    c.table_active = h(w.raise);
    c.table_active_border = h(accent.base);
    c.table_row_border = h(w.line);
    c.table_foot = h(w.surface);
    c.table_foot_foreground = h(w.ink);

    c.tab_bar = h(w.ground);
    c.tab_bar_segmented = h(w.surface);
    c.tab = h(gl::Color::TRANSPARENT);
    c.tab_active = h(w.ground);
    c.tab_foreground = h(w.ink_2);
    c.tab_active_foreground = h(w.ink);

    c.title_bar = h(w.surface);
    c.title_bar_border = h(w.line);
    c.status_bar = h(w.surface);
    c.status_bar_border = h(w.line);
    c.sidebar = h(w.surface);
    c.sidebar_foreground = h(w.ink);
    c.sidebar_border = h(w.line);
    c.sidebar_accent = h(w.raise);
    c.sidebar_accent_foreground = h(w.ink);
    c.sidebar_primary = h(accent.base);
    c.sidebar_primary_foreground = h(accent.ink);

    c.scrollbar = h(gl::Color::TRANSPARENT);
    c.scrollbar_thumb = h(w.thumb);
    c.scrollbar_thumb_hover = h(w.edge);

    c.danger = h(w.bad);
    c.danger_foreground = h(w.ink);
    c.danger_hover = h(w.bad.lighten(0.08));
    c.danger_active = h(w.bad.darken(0.08));
    c.success = h(w.good);
    c.success_foreground = h(w.gold_ink);
    c.success_hover = h(w.good.lighten(0.08));
    c.success_active = h(w.good.darken(0.08));
    c.warning = h(w.amber);
    c.warning_foreground = h(w.gold_ink);
    c.warning_hover = h(w.amber.lighten(0.08));
    c.warning_active = h(w.amber.darken(0.08));
    c.info = h(accent.base);
    c.info_foreground = h(accent.ink);
    c.info_hover = h(accent.base.lighten(0.08));
    c.info_active = h(accent.base.darken(0.08));
}

#[cfg(test)]
mod tests {
    use gpui_kit::TestAppContext;
    use gpui_kit::component::Theme;
    use wowdps_gui_logic::theme::{FROST, GOLD, class_accent};
    use wowdps_model::Class;

    use super::{Look, apply, hsla};

    /// One definition drives both targets, and another replaces both: Kit's
    /// slots and our Look switch together, and a class chrome reaches the
    /// slots that draw the chrome.
    #[gpui_kit::test]
    fn a_definition_drives_kit_and_look_alike(cx: &mut TestAppContext) {
        cx.update(gpui_kit::init);
        cx.update(|cx| apply(&GOLD, None, cx));
        cx.update(|cx| {
            let theme = Theme::global(cx);
            assert!(theme.is_dark());
            assert_eq!(theme.background, hsla(GOLD.window.ground));
            assert_eq!(theme.primary, hsla(GOLD.window.gold));
            assert_eq!(theme.font_family.as_ref(), GOLD.faces.ui);
            assert_eq!(Look::global(cx).def.name, "gold");
        });

        cx.update(|cx| apply(&FROST, None, cx));
        cx.update(|cx| {
            let theme = Theme::global(cx);
            assert_eq!(theme.background, hsla(FROST.window.ground));
            assert_eq!(theme.primary, hsla(FROST.window.gold));
            assert_eq!(Look::global(cx).def.name, "frost");
        });

        let priest = class_accent(Some(Class::Priest));
        cx.update(|cx| apply(&GOLD, Some(priest), cx));
        cx.update(|cx| {
            let theme = Theme::global(cx);
            assert_eq!(theme.primary, hsla(priest.base));
            assert_eq!(theme.ring, hsla(priest.base));
            assert_eq!(theme.primary_foreground, hsla(priest.ink));
            assert_eq!(Look::global(cx).accent, priest);
        });
    }
}
