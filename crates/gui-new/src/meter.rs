//! A minimal meter (step 1.2): the watched segment's rows, each one a click
//! target keyed by the player's guid. It is the template every later list
//! copies — ids by identity, observed with `test_support`, selection read
//! back through `aria_selected` — and phase 2 replaces its look with the
//! overlay's.

use gpui_kit::component::ActiveTheme;
use gpui_kit::prelude::*;
use gpui_kit::{App, ElementId, Entity, SharedString, TestSupportExt as _, div};

use crate::session::Session;

/// A meter row's id: `"row"` named by the row's key (a player's guid), so
/// the id follows the player when the rows resort.
pub fn row_id(key: &str) -> ElementId {
    ElementId::from((
        ElementId::Name("row".into()),
        SharedString::from(key.to_string()),
    ))
}

pub fn meter(session: &Entity<Session>, cx: &App) -> impl IntoElement {
    let theme = cx.theme();
    let (lit, quiet) = (theme.list_active, theme.muted_foreground);
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
                .when(i == selected, |row| row.bg(lit))
                .child(div().id("label").test_support().child(row.label))
                .child(div().text_color(quiet).child(row.amount.to_string()))
                .on_click(move |_, _, cx| {
                    session.update(cx, |session, cx| {
                        session.act(|state| state.select_row(i), cx)
                    })
                })
        }))
}

#[cfg(test)]
mod tests {
    use gpui_kit::test::TestWindowExt as _;
    use gpui_kit::{
        AppContext as _, Context, Entity, IntoElement, Render, Subscription, TestAppContext,
        TextRun, Window, font, px, size,
    };

    use super::{meter, row_id};
    use crate::session::Session;
    use crate::testkit::{self, MockLink};

    /// The view a meter test opens: the meter over one session, redrawn
    /// whenever the session notifies, as every surface will be.
    struct Probe {
        session: Entity<Session>,
        _changes: Subscription,
    }

    impl Probe {
        fn new(cx: &mut Context<Self>) -> Self {
            let session = cx.new(|_| Session::new(MockLink::fixture()));
            let changes = cx.observe(&session, |_, _, cx| cx.notify());
            Self {
                session,
                _changes: changes,
            }
        }
    }

    impl Render for Probe {
        fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            meter(&self.session, cx)
        }
    }

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
