//! One daemon link and its `ClientState`, as an entity (spec §6): every
//! surface that watches the daemon owns a `Session`, exactly as each iced
//! `ClientState` has its connection today.
//!
//! All the work is `pump`: reconnect if the link died (one attempt, never a
//! wait), drain it, apply each message to the `ClientState`, send what that
//! asks for, and notify when anything arrived. The app runs it every `TICK`
//! (`Session::running`); tests build a `Session::new` over the daemon's mock
//! and call `pump` themselves, so no timer keeps the executor from parking.

use std::time::Duration;

use gpui_kit::{Context, Task};
use wowdps_proto::{
    ClientMsg, ClientState, DaemonClient, DaemonMsg, HistoryStatus, OverlayState, Reconnect,
};

/// The drain cadence: the daemon pushes changes at 10 Hz, the iced GUI
/// drains at the same rate.
pub const TICK: Duration = Duration::from_millis(100);

/// The daemon never broadcasts `Status`; a running session asks for it
/// every this many ticks (once a second).
const STATUS_EVERY: u32 = 10;

/// A connection to the daemon, as a `Session` uses one: the real
/// `DaemonClient`, or the daemon's in-process mock under test.
pub trait Link: 'static {
    fn send(&mut self, msg: &ClientMsg);
    /// Everything that arrived since the last poll, never blocking.
    fn poll(&mut self) -> Vec<DaemonMsg>;
    /// One reconnect attempt when the link died; never a wait.
    fn reconnect(&mut self) -> Reconnect;
}

impl Link for DaemonClient {
    fn send(&mut self, msg: &ClientMsg) {
        DaemonClient::send(self, msg);
    }

    fn poll(&mut self) -> Vec<DaemonMsg> {
        DaemonClient::poll(self)
    }

    fn reconnect(&mut self) -> Reconnect {
        self.try_reconnect()
    }
}

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

/// What a session says that its `ClientState` does not own.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SessionEvent {
    /// The daemon's overlay supervisor wishes the overlay shown or hidden.
    SetVisible(bool),
    /// A new segment opened: a live meter comes home to Live.
    SegmentOpened,
}

/// How the link stands, in words a surface can show.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Linked {
    Up,
    /// The daemon went away and a new one is starting or awaited.
    Down(String),
}

pub struct Session {
    link: Box<dyn Link>,
    state: ClientState,
    status: Option<Status>,
    linked: Linked,
    ticks: u32,
    /// When the last snapshot or segment list arrived: a live meter that
    /// hears nothing for a while says how long (the overlay's radar).
    last_snapshot: Option<std::time::Instant>,
    /// The `TICK` loop of a running session; dropping the session cancels it.
    _pump: Option<Task<()>>,
}

impl gpui_kit::EventEmitter<SessionEvent> for Session {}

impl Session {
    /// A session over `link` that pumps only when told to: what tests hold.
    /// It declares the state's first Watch and asks for the status.
    #[cfg(test)]
    pub fn new(link: impl Link) -> Self {
        Self::with_state(Box::new(link), ClientState::new())
    }

    /// `new` over a state prepared by the caller (the Σ split's, which
    /// asks for a top-N), and a link already boxed.
    pub fn with_state(link: Box<dyn Link>, state: ClientState) -> Self {
        let mut session = Self {
            link,
            state,
            status: None,
            linked: Linked::Up,
            ticks: 0,
            last_snapshot: None,
            _pump: None,
        };
        let first = session.state.initial_request();
        session.send(vec![first, ClientMsg::GetStatus { req_id: 0 }]);
        session
    }

    /// The app's session: `new`, pumped every `TICK` on the foreground,
    /// asking for the status once a second.
    pub fn running(link: impl Link, cx: &mut Context<Self>) -> Self {
        Self::running_with(Box::new(link), ClientState::new(), cx)
    }

    /// `running` over a prepared state and a boxed link.
    pub fn running_with(link: Box<dyn Link>, state: ClientState, cx: &mut Context<Self>) -> Self {
        let mut session = Self::with_state(link, state);
        session._pump = Some(cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(TICK).await;
                let alive = this.update(cx, |session, cx| {
                    session.ticks = session.ticks.wrapping_add(1);
                    if session.linked == Linked::Up && session.ticks.is_multiple_of(STATUS_EVERY) {
                        session.send(vec![ClientMsg::GetStatus { req_id: 0 }]);
                    }
                    session.pump(cx);
                });
                if alive.is_err() {
                    break;
                }
            }
        }));
        session
    }

    pub fn state(&self) -> &ClientState {
        &self.state
    }

    pub fn status(&self) -> Option<&Status> {
        self.status.as_ref()
    }

    pub fn last_snapshot(&self) -> Option<std::time::Instant> {
        self.last_snapshot
    }

    pub fn linked(&self) -> &Linked {
        &self.linked
    }

    /// Change the state and send what the change asks for: every UI action
    /// on a session goes through here (`act(|s| s.select_row(1))`).
    pub fn act(
        &mut self,
        f: impl FnOnce(&mut ClientState) -> Vec<ClientMsg>,
        cx: &mut Context<Self>,
    ) {
        let requests = f(&mut self.state);
        self.send(requests);
        cx.notify();
    }

    fn send(&mut self, requests: Vec<ClientMsg>) {
        for request in requests {
            self.link.send(&request);
        }
    }

    /// Reconnect, drain, apply, send; notify when anything changed. Returns
    /// whether anything did.
    pub fn pump(&mut self, cx: &mut Context<Self>) -> bool {
        let linked = match self.link.reconnect() {
            Reconnect::Connected => Linked::Up,
            Reconnect::Spawned | Reconnect::Waiting => Linked::Down("starting the daemon".into()),
            Reconnect::Failed(e) => Linked::Down(e),
        };
        let mut changed = linked != self.linked;
        self.linked = linked;

        // Replies can ask for more (a SegmentList re-watching, a snapshot's
        // follow-up), and a mock answers inline, so drain until quiet.
        for _ in 0..8 {
            let arrived = self.link.poll();
            if arrived.is_empty() {
                break;
            }
            changed = true;
            for msg in arrived {
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
                if matches!(
                    msg,
                    DaemonMsg::Snapshot { .. } | DaemonMsg::SegmentList { .. }
                ) {
                    self.last_snapshot = Some(std::time::Instant::now());
                }
                let opened = matches!(msg, DaemonMsg::SegmentOpened { .. });
                if let DaemonMsg::SetVisible(visible) = &msg {
                    cx.emit(SessionEvent::SetVisible(*visible));
                }
                let requests = self.state.on_msg(msg);
                self.send(requests);
                if opened {
                    cx.emit(SessionEvent::SegmentOpened);
                }
            }
        }
        if changed {
            cx.notify();
        }
        changed
    }
}
