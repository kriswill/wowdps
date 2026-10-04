//! A minimal meter (step 1.2): the watched segment's rows, each one a click
//! target keyed by the player's guid. It is the template every later list
//! copied — ids by identity, observed with `test_support`, selection read
//! back through `aria_selected`. The window's meter (`window/table.rs`)
//! and the overlay's rows key their rows by [`row_id`]; the minimal meter
//! itself stands now only for the step-1.2 harness tests.

use gpui_kit::{ElementId, SharedString};
#[cfg(test)]
use {
    crate::images,
    crate::session::Session,
    crate::theme::Look,
    gpui_kit::prelude::*,
    gpui_kit::{AnyElement, App, Entity, Pixels, TestSupportExt as _, div, img, px, rgb},
    wowdps_model::{Class, Spec},
};

/// A meter row's id: `"row"` named by the row's key (a player's guid), so
/// the id follows the player when the rows resort.
pub fn row_id(key: &str) -> ElementId {
    ElementId::from((
        ElementId::Name("row".into()),
        SharedString::from(key.to_string()),
    ))
}

/// A player's badge: their spec's icon, else their class crest, else a disc
/// in the class colour (the art caches are per-machine and may be absent).
#[cfg(test)]
pub fn badge(class: Option<Class>, spec: Option<Spec>, side: Pixels) -> AnyElement {
    let art = spec
        .and_then(|s| images::spec_icon(s.id()))
        .or_else(|| class.and_then(images::class_icon));
    match art {
        Some(tile) => img(tile).size(side).flex_none().into_any_element(),
        None => {
            let (r, g, b) = class.map_or((0x80, 0x80, 0x80), Class::rgb);
            div()
                .size(side)
                .flex_none()
                .rounded_full()
                .bg(rgb(u32::from(r) << 16 | u32::from(g) << 8 | u32::from(b)))
                .into_any_element()
        }
    }
}

#[cfg(test)]
pub fn meter(session: &Entity<Session>, cx: &App) -> impl IntoElement {
    let look = Look::global(cx);
    let (lit, quiet) = (look.w(|t| t.raise), look.w(|t| t.ink_2));
    let edge = crate::theme::hsla(look.accent.base);
    let state = session.read(cx).state();
    let selected = state.row_sel;
    let rows = state.rows();
    div()
        .id("meter")
        .flex()
        .flex_col()
        .children(rows.into_iter().enumerate().map(|(i, row)| {
            let session = session.clone();
            div()
                .id(row_id(&row.key))
                .test_support()
                .aria_selected(i == selected)
                .aria_label(row.label.clone())
                .flex()
                .justify_between()
                .gap_4()
                .px_2()
                .py_1()
                .border_l_2()
                .border_color(gpui_kit::transparent_black())
                .when(i == selected, |row| row.bg(lit).border_color(edge))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .child(badge(row.class, row.spec, px(18.)))
                        .child(div().id("label").test_support().child(row.label)),
                )
                .child(div().text_color(quiet).child(row.amount.to_string()))
                .on_click(move |_, _, cx| {
                    session.update(cx, |session, cx| {
                        session.act(|state| state.select_row(i), cx)
                    })
                })
        }))
}

/// The view a meter test opens: the meter over one session on the fixture,
/// redrawn whenever the session notifies, as every surface will be.
#[cfg(test)]
pub(crate) struct Probe {
    pub(crate) session: gpui_kit::Entity<Session>,
    _changes: gpui_kit::Subscription,
}

#[cfg(test)]
impl Probe {
    pub(crate) fn new(cx: &mut gpui_kit::Context<Self>) -> Self {
        use gpui_kit::AppContext as _;
        let session = cx.new(|_| Session::new(crate::testkit::MockLink::fixture()));
        let changes = cx.observe(&session, |_, _, cx| cx.notify());
        Self {
            session,
            _changes: changes,
        }
    }
}

#[cfg(test)]
impl Render for Probe {
    fn render(
        &mut self,
        _: &mut gpui_kit::Window,
        cx: &mut gpui_kit::Context<Self>,
    ) -> impl IntoElement {
        meter(&self.session, cx)
    }
}

#[cfg(test)]
mod tests {
    use gpui_kit::test::TestWindowExt as _;
    use gpui_kit::{AppContext as _, TestAppContext, TextRun, font, px, size};

    use super::{Probe, row_id};
    use crate::testkit;

    /// The template every interaction test copies: open the view over the
    /// mock, put a pull on it, click a row BY ITS PLAYER'S GUID, and check
    /// the selection both where it is drawn (`aria_selected`) and where it
    /// lives (the `ClientState`).
    #[gpui_kit::test]
    fn clicking_a_row_selects_its_player(cx: &mut TestAppContext) {
        let (window, probe) =
            testkit::open(cx, size(px(420.), px(320.)), |_, cx| cx.new(Probe::new));
        let session = probe.read_with(cx, |probe, _| probe.session.clone());
        session.update(cx, |session, cx| {
            session.act(|state| state.pin_live(), cx);
            session.pump(cx);
        });
        let keys: Vec<String> = session.read_with(cx, |session, _| {
            session.state().rows().into_iter().map(|r| r.key).collect()
        });
        assert!(
            keys.len() >= 2,
            "the fixture's live pull has rows: {keys:?}"
        );

        cx.update_window(window, |_, window, cx| {
            window.render_frame(cx);
            assert_eq!(window.find(row_id(&keys[0])).selected(), Some(true));
            assert_eq!(window.find(row_id(&keys[1])).selected(), Some(false));

            window.click(row_id(&keys[1]), cx);
            session.update(cx, |session, cx| session.pump(cx));
            window.render_frame(cx);
            assert_eq!(window.find(row_id(&keys[1])).selected(), Some(true));
            assert_eq!(window.find(row_id(&keys[0])).selected(), Some(false));
        })
        .unwrap();
        session.read_with(cx, |session, _| assert_eq!(session.state().row_sel, 1));
    }

    /// The real-text twin: the same view with the platform's text system,
    /// where a label is as wide as its glyphs shaped in the theme's font —
    /// and a row is one line of that text tall, plus its padding.
    #[test]
    fn a_row_lays_out_real_text() {
        let mut cx = testkit::headless();
        let (window, probe) = testkit::open_headless(&mut cx, size(px(420.), px(320.)), |_, cx| {
            cx.new(Probe::new)
        });
        let session = cx.update(|cx| probe.read(cx).session.clone());
        cx.update(|cx| {
            session.update(cx, |session, cx| {
                session.act(|state| state.pin_live(), cx);
                session.pump(cx);
            })
        });
        let row = cx.update(|cx| session.read(cx).state().rows().remove(0));

        cx.update_window(window, |_, window, cx| {
            window.render_frame(cx);
            let label = window.within(row_id(&row.key)).find("label").bounds();
            let theme = gpui_kit::component::Theme::global(cx);
            let run = TextRun {
                len: row.label.len(),
                font: font(theme.font_family.clone()),
                color: theme.foreground,
                background_color: None,
                underline: None,
                strikethrough: None,
            };
            let shaped = window.text_system().shape_line(
                row.label.clone().into(),
                theme.font_size,
                &[run],
                None,
            );
            assert!(
                (label.size.width - shaped.width).abs() < px(1.),
                "{:?} laid out {:?} wide, shaped {:?}",
                row.label,
                label.size.width,
                shaped.width
            );
            // The stub text system would make every glyph 0.6 em wide.
            let stub = theme.font_size * (0.6 * row.label.chars().count() as f32);
            assert!(
                (label.size.width - stub).abs() > px(1.),
                "real metrics, not the stub's"
            );
            let row_box = window.find(row_id(&row.key)).bounds();
            assert!(
                row_box.size.height > label.size.height,
                "padding around one line"
            );
            assert!(
                row_box.size.height < label.size.height * 2.,
                "one line, not wrapped"
            );
        })
        .unwrap();
    }
}

#[cfg(test)]
mod render_probe {
    use gpui_kit::{AppContext as _, px, size};

    use super::Probe;
    use crate::testkit;

    /// Spike S5: the meter rendered to pixels with no window system — the
    /// headless renderer (wgpu: a GPU, or lavapipe) under the real text
    /// system. Two captures of one frame must agree byte for byte.
    #[test]
    #[ignore = "needs a wgpu adapter; run by hand: cargo test -p wowdps-gui render_probe -- --ignored"]
    fn the_meter_renders_to_pixels() {
        let mut cx = testkit::headless();
        cx.update(|cx| crate::theme::apply(&wowdps_gui_logic::theme::NAVY, None, cx));
        let (window, probe) = testkit::open_headless(&mut cx, size(px(420.), px(240.)), |_, cx| {
            cx.new(Probe::new)
        });
        let session = cx.update(|cx| probe.read(cx).session.clone());
        cx.update(|cx| {
            session.update(cx, |session, cx| {
                session.act(|state| state.pin_live(), cx);
                session.pump(cx);
            })
        });
        let first = cx.capture_screenshot(window).expect("a headless renderer");
        let second = cx.capture_screenshot(window).expect("a headless renderer");
        assert_eq!(first.dimensions(), second.dimensions());
        assert!(first == second, "one frame, two captures, same bytes");
        let distinct: std::collections::HashSet<[u8; 4]> = first.pixels().map(|p| p.0).collect();
        assert!(
            distinct.len() > 8,
            "text and a highlight, not a blank: {} colours",
            distinct.len()
        );
        if let Some(dir) = std::env::var_os("WOWDPS_SHOTS_DIR") {
            let path = std::path::Path::new(&dir).join("s5-meter.png");
            first.save(&path).expect("png written");
            eprintln!(
                "render_probe: {} ({}x{})",
                path.display(),
                first.width(),
                first.height()
            );
        }
    }
}
