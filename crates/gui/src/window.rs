//! The regular-window frontend.
//!
//! The runtime shape mirrors `wowdps-tui`: a 100 ms tick drains the daemon
//! client's inbox (stale snapshots were already coalesced away) and feeds the
//! shared [`ClientState`]; this module only translates iced events into
//! `Action`s and draws.

use std::time::{Duration, Instant};

use iced::{Subscription, Task, Theme, keyboard, time, window};

use wowdps_model::Action;
use wowdps_proto::{ClientKind, ClientState, DaemonClient, DaemonMsg};

use crate::config::Config;
use crate::keys;
use crate::view;

/// Redraw/drain cadence. Live durations tick at this rate.
pub(crate) const TICK: Duration = Duration::from_millis(100);

const ZOOM_STEP: f32 = 0.1;
const ZOOM_RANGE: std::ops::RangeInclusive<f32> = 0.5..=3.0;

pub fn run(cfg: Config) -> Result<(), String> {
    // Connect before iced takes over, so a missing daemon is a clean CLI
    // error, not a blank window. The factory is `Fn` but runs once; the
    // fallback reconnect covers the theoretical second call.
    let first = std::sync::Mutex::new(Some(connect()?));
    iced::application(
        move || {
            let client = first
                .lock()
                .expect("client handoff poisoned")
                .take()
                .unwrap_or_else(|| connect().expect("daemon vanished during startup"));
            Gui::new(client, cfg.clone())
        },
        update,
        view::view,
    )
    .title(title)
    .subscription(subscription)
    .theme(theme)
    .scale_factor(|state| state.cfg.zoom)
    .window(window::Settings {
        size: iced::Size::new(460.0, 640.0),
        min_size: Some(iced::Size::new(320.0, 240.0)),
        ..window::Settings::default()
    })
    .run()
    .map_err(|e| e.to_string())
}

/// Connect, spawning the daemon if none is running. There is no embedded
/// fallback: no daemon, no meter. The kind matters: `Overlay` is how the
/// daemon's supervisor recognizes the session it manages (`SetVisible`,
/// never-spawn-a-second-overlay, failure clearing).
pub(crate) fn connect_as(kind: ClientKind) -> Result<DaemonClient, String> {
    DaemonClient::connect(&crate::daemon_bin(), None, kind)
        .map_err(|e| format!("cannot reach the wowdps daemon: {e}"))
}

pub(crate) fn connect() -> Result<DaemonClient, String> {
    connect_as(ClientKind::Window)
}

pub(crate) struct Gui {
    pub(crate) state: ClientState,
    client: DaemonClient,
    /// When the last snapshot arrived, wall-clock. WoW buffers its log
    /// writes (sometimes for a long while), so the meter shows how far
    /// behind the file is instead of silently looking frozen.
    last_snapshot_at: Option<Instant>,
    cfg: Config,
}

impl Gui {
    fn new(mut client: DaemonClient, cfg: Config) -> Self {
        let state = ClientState::new();
        client.send(&state.initial_request());
        Self {
            state,
            client,
            last_snapshot_at: None,
            cfg,
        }
    }

    /// Seconds since data last arrived, once it stops looking live.
    pub(crate) fn stale_secs(&self) -> Option<u64> {
        stale_secs(self.last_snapshot_at)
    }
}

/// Shared with the overlay: 5s of silence is when "live" starts needing an
/// asterisk, thanks to the game's buffered log writes.
pub(crate) fn stale_secs(last_at: Option<Instant>) -> Option<u64> {
    let secs = last_at?.elapsed().as_secs();
    (secs >= 5).then_some(secs)
}

/// Drain the daemon client into the state; snapshots refresh the staleness
/// clock. Reconnects (and re-declares the cursor) if the daemon went away.
/// Shared with the overlay.
pub(crate) fn drain_client(
    state: &mut ClientState,
    client: &mut DaemonClient,
    last_snapshot_at: &mut Option<Instant>,
) {
    for msg in client.poll() {
        if matches!(
            msg,
            DaemonMsg::Snapshot { .. } | DaemonMsg::SegmentList { .. }
        ) {
            *last_snapshot_at = Some(Instant::now());
        }
        for req in state.on_msg(msg) {
            client.send(&req);
        }
    }
    if client.is_dead() {
        state.status = Some("daemon gone — reconnecting…".to_string());
        if client.reconnect_if_dead() {
            state.status = None;
            client.send(&state.initial_request());
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) enum Message {
    /// Drain the daemon client and let live durations advance.
    Tick,
    Key(keyboard::Event),
    /// A segment-list row was clicked: select and open it.
    ListRow(usize),
    /// A meter row was clicked: select it and drill in.
    MeterRow(usize),
}

fn theme(_state: &Gui) -> Theme {
    Theme::TokyoNight
}

fn title(state: &Gui) -> String {
    match state.state.source.as_deref() {
        Some(name) => format!("wowdps — {name}"),
        None => "wowdps".to_string(),
    }
}

fn update(state: &mut Gui, message: Message) -> Task<Message> {
    let mut requests = Vec::new();
    match message {
        Message::Tick => drain_client(
            &mut state.state,
            &mut state.client,
            &mut state.last_snapshot_at,
        ),
        Message::Key(event) => {
            if let keyboard::Event::KeyPressed {
                modified_key,
                modifiers,
                ..
            } = event
            {
                if let Some(zoom) = keys::zoom_for(&modified_key, modifiers) {
                    state.cfg.zoom = match zoom {
                        keys::Zoom::In => state.cfg.zoom + ZOOM_STEP,
                        keys::Zoom::Out => state.cfg.zoom - ZOOM_STEP,
                        keys::Zoom::Reset => Config::default().zoom,
                    }
                    .clamp(*ZOOM_RANGE.start(), *ZOOM_RANGE.end());
                    state.cfg.save();
                } else if let Some(action) = keys::action_for(&modified_key, modifiers) {
                    requests.extend(state.state.apply(action));
                }
            }
        }
        Message::ListRow(row) => {
            state.state.set_list_selection(row);
            requests.extend(state.state.apply(Action::Open));
        }
        Message::MeterRow(row) => {
            state.state.row_sel = row;
            requests.extend(state.state.apply(Action::Open));
        }
    }
    for req in requests {
        state.client.send(&req);
    }

    if state.state.quit {
        iced::exit()
    } else {
        Task::none()
    }
}

fn subscription(_state: &Gui) -> Subscription<Message> {
    Subscription::batch([
        time::every(TICK).map(|_| Message::Tick),
        keyboard::listen().map(Message::Key),
    ])
}
