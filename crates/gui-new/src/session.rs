//! One daemon link and its `ClientState`, as an entity (spec §6): every
//! surface that watches the daemon owns a `Session`, exactly as each iced
//! `ClientState` has its connection today.
//!
//! All the work is `pump`: reconnect if the link died (one attempt, never a
//! wait), drain it, apply each message to the `ClientState`, send what that
//! asks for, and notify when anything arrived. The app runs it every `TICK`;
//! step 1.2 makes the link a trait so tests drive a `MockDaemon` through
//! the same `pump` without a timer.

use std::time::Duration;

use gpui_kit::{Context, Task};
use wowdps_proto::{
    ClientMsg, ClientState, DaemonClient, DaemonMsg, HistoryStatus, OverlayState, Reconnect,
};

/// The drain cadence: the daemon pushes changes at 10 Hz, the iced GUI
/// drains at the same rate.
pub const TICK: Duration = Duration::from_millis(100);

/// The daemon never broadcasts `Status`; it is asked for once a second.
const STATUS_EVERY: u32 = 10;

/// The daemon's answer to `GetStatus`, kept whole for whoever draws it.
#[derive(Clone, Debug, PartialEq)]
pub struct Status {
    pub game_running: bool,
    pub source: Option<String>,
    pub clients: u32,
    pub linger: bool,
    pub overlay: OverlayState,
    pub history: HistoryStatus,
}

/// How the link stands, in words a surface can show.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Link {
    Up,
    /// The daemon went away and a new one is starting or awaited.
    Down(String),
}

pub struct Session {
    client: DaemonClient,
    state: ClientState,
    status: Option<Status>,
    link: Link,
    ticks: u32,
    /// The `TICK` loop; dropping the session cancels it.
    _pump: Task<()>,
}

impl Session {
    pub fn new(mut client: DaemonClient, cx: &mut Context<Self>) -> Self {
        let state = ClientState::new();
        client.send(&state.initial_request());
        client.send(&ClientMsg::GetStatus { req_id: 0 });
        let pump = cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(TICK).await;
                if this.update(cx, |session, cx| session.pump(cx)).is_err() {
                    break;
                }
            }
        });
        Self {
            client,
            state,
            status: None,
            link: Link::Up,
            ticks: 0,
            _pump: pump,
        }
    }

    pub fn state(&self) -> &ClientState {
        &self.state
    }

    pub fn status(&self) -> Option<&Status> {
        self.status.as_ref()
    }

    pub fn link(&self) -> &Link {
        &self.link
    }

    /// Reconnect, drain, apply, send; notify when anything changed.
    pub fn pump(&mut self, cx: &mut Context<Self>) {
        let link = match self.client.try_reconnect() {
            Reconnect::Connected => Link::Up,
            Reconnect::Spawned | Reconnect::Waiting => Link::Down("starting the daemon".into()),
            Reconnect::Failed(e) => Link::Down(e),
        };
        let mut changed = link != self.link;
        self.link = link;

        for msg in self.client.poll() {
            changed = true;
            if let DaemonMsg::Status {
                game_running,
                source,
                clients,
                linger,
                overlay,
                history,
                ..
            } = &msg
            {
                self.status = Some(Status {
                    game_running: *game_running,
                    source: source.clone(),
                    clients: *clients,
                    linger: *linger,
                    overlay: overlay.clone(),
                    history: history.clone(),
                });
            }
            for request in self.state.on_msg(msg) {
                self.client.send(&request);
            }
        }

        self.ticks = self.ticks.wrapping_add(1);
        if self.link == Link::Up && self.ticks.is_multiple_of(STATUS_EVERY) {
            self.client.send(&ClientMsg::GetStatus { req_id: 0 });
        }
        if changed {
            cx.notify();
        }
    }
}
