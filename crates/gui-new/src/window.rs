//! The window. In step 1.1 it shows the daemon's `Status` through a
//! `Session`: proof of the link, Kit's theme and a GPUI window, and nothing
//! a later step keeps (phase 3 builds the real window, in the redesign's
//! order).

use gpui_kit::component::ActiveTheme;
use gpui_kit::prelude::*;
use gpui_kit::{
    App, Bounds, Context, Entity, Subscription, TitlebarOptions, Window, WindowBounds,
    WindowOptions, div, px, size,
};
use wowdps_proto::{DaemonClient, OverlayState};

use crate::meter::meter;
use crate::session::{Linked, Session, Status};

pub fn open(client: DaemonClient, cx: &mut App) -> Result<(), String> {
    let bounds = Bounds::centered(None, size(px(560.), px(320.)), cx);
    let options = WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(bounds)),
        titlebar: Some(TitlebarOptions {
            title: Some("wowdps (gui-new)".into()),
            ..Default::default()
        }),
        app_id: Some("wowdps-gui-new".to_string()),
        ..Default::default()
    };
    gpui_kit::open_window(options, cx, |_, cx| {
        let session = cx.new(|cx| Session::running(client, cx));
        // The newest segment, as the window opens on with no pull chosen.
        session.update(cx, |session, cx| session.act(|state| state.pin_live(), cx));
        cx.new(|cx| StatusView::new(session, cx))
    })
    .map(|_| ())
    .map_err(|e| format!("cannot open the window: {e}"))
}

struct StatusView {
    session: Entity<Session>,
    /// Redraw whenever the session notifies.
    _changes: Subscription,
}

impl StatusView {
    fn new(session: Entity<Session>, cx: &mut Context<Self>) -> Self {
        let changes = cx.observe(&session, |_, _, cx| cx.notify());
        Self {
            session,
            _changes: changes,
        }
    }
}

impl Render for StatusView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let (ground, ink, quiet) = (theme.background, theme.foreground, theme.muted_foreground);
        let session = self.session.read(cx);
        let rows = lines(
            session.linked(),
            session.status(),
            session.state().segment_count(),
        );
        div()
            .size_full()
            .flex()
            .flex_col()
            .gap_1()
            .p_4()
            .bg(ground)
            .text_color(ink)
            .children(rows.into_iter().map(move |(label, value)| {
                div()
                    .flex()
                    .gap_3()
                    .child(div().w_20().text_color(quiet).child(label))
                    .child(value)
            }))
            .child(div().pt_4().child(meter(&self.session, cx)))
    }
}

/// The status as label / value lines, in `wowdps status`'s order.
fn lines(link: &Linked, status: Option<&Status>, segments: usize) -> Vec<(&'static str, String)> {
    let mut out = vec![(
        "daemon",
        match link {
            Linked::Up => "connected".to_string(),
            Linked::Down(why) => why.clone(),
        },
    )];
    let Some(s) = status else {
        out.push(("status", "asking…".to_string()));
        return out;
    };
    out.push((
        "source",
        s.source.clone().unwrap_or_else(|| "(none)".into()),
    ));
    out.push(("clients", s.clients.to_string()));
    out.push((
        "game",
        if s.game_running {
            "running"
        } else {
            "not running"
        }
        .to_string(),
    ));
    out.push(("linger", if s.linger { "yes" } else { "no" }.to_string()));
    out.push((
        "overlay",
        match &s.overlay {
            OverlayState::Absent => "absent".to_string(),
            OverlayState::Visible => "visible".to_string(),
            OverlayState::Hidden => "hidden".to_string(),
            // The child's stderr tail: its last line says why.
            OverlayState::Failed(e) => format!(
                "failed: {}",
                e.lines().rev().find(|l| !l.trim().is_empty()).unwrap_or("")
            ),
        },
    ));
    out.push((
        "history",
        if s.history.enabled {
            format!("{} fights", s.history.fights)
        } else {
            "disabled".to_string()
        },
    ));
    out.push(("segments", segments.to_string()));
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use wowdps_proto::HistoryStatus;

    #[test]
    fn the_lines_say_what_wowdps_status_says() {
        let status = Status {
            game_running: false,
            source: Some("/logs".into()),
            clients: 2,
            linger: true,
            overlay: OverlayState::Failed("spawning x\nno WAYLAND_DISPLAY\n".into()),
            history: HistoryStatus {
                enabled: true,
                fights: 886,
                ..HistoryStatus::default()
            },
        };
        let got = lines(&Linked::Up, Some(&status), 4);
        let want: Vec<(&str, String)> = [
            ("daemon", "connected"),
            ("source", "/logs"),
            ("clients", "2"),
            ("game", "not running"),
            ("linger", "yes"),
            ("overlay", "failed: no WAYLAND_DISPLAY"),
            ("history", "886 fights"),
            ("segments", "4"),
        ]
        .into_iter()
        .map(|(k, v)| (k, v.to_string()))
        .collect();
        assert_eq!(got, want);
    }

    #[test]
    fn before_an_answer_only_the_link_is_known() {
        let got = lines(&Linked::Down("starting the daemon".into()), None, 0);
        assert_eq!(
            got,
            vec![
                ("daemon", "starting the daemon".to_string()),
                ("status", "asking…".to_string()),
            ]
        );
    }
}
