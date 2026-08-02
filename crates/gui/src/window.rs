//! The regular-window frontend.
//!
//! The runtime shape mirrors `wowdps-tui`: a reader thread tails the log and
//! a 100 ms tick drains it with a bounded budget, so a large replay can never
//! starve input or redraws. All state lives in [`wowdps_core::app::App`];
//! this module only translates iced events into `Action`s and draws.

use std::sync::mpsc::{Receiver, TryRecvError};
use std::time::{Duration, Instant};

use iced::{Subscription, Task, Theme, keyboard, time, window};

use wowdps_core::app::{self, Action, App};
use wowdps_core::index;
use wowdps_core::tail::{self, SourceSpec, TailEvent};

use crate::config::Config;
use crate::keys;
use crate::view;

/// Redraw/drain cadence. Live durations tick at this rate.
pub(crate) const TICK: Duration = Duration::from_millis(100);

/// Longest a single tick will spend swallowing tailed lines before it lets
/// the frame render; replaying a big log must not freeze the window.
pub(crate) const DRAIN_BUDGET: Duration = Duration::from_millis(25);

const ZOOM_STEP: f32 = 0.1;
const ZOOM_RANGE: std::ops::RangeInclusive<f32> = 0.5..=3.0;

pub fn run(spec: SourceSpec, cfg: Config) -> iced::Result {
    iced::application(move || Gui::new(spec.clone(), cfg.clone()), update, view::view)
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
}

pub(crate) struct Gui {
    pub(crate) app: App,
    lines: Receiver<TailEvent>,
    /// When the last combat lines arrived, wall-clock. WoW buffers its log
    /// writes (sometimes for a long while), so the meter shows how far behind
    /// the file is instead of silently looking frozen.
    last_lines_at: Option<Instant>,
    cfg: Config,
}

impl Gui {
    fn new(spec: SourceSpec, cfg: Config) -> Self {
        Self {
            app: App::new(),
            lines: tail::spawn(spec),
            last_lines_at: None,
            cfg,
        }
    }

    /// Seconds since combat lines last arrived, once it stops looking live.
    pub(crate) fn stale_secs(&self) -> Option<u64> {
        stale_secs(self.last_lines_at)
    }
}

/// Shared with the overlay: 5s of silence is when "live" starts needing an
/// asterisk, thanks to the game's buffered log writes.
pub(crate) fn stale_secs(last_lines_at: Option<Instant>) -> Option<u64> {
    let secs = last_lines_at?.elapsed().as_secs();
    (secs >= 5).then_some(secs)
}

/// Drain the reader thread for at most [`DRAIN_BUDGET`]. Returns the arrival
/// instant to store when combat lines came in. Shared with the overlay.
pub(crate) fn drain_tail(
    app: &mut App,
    lines: &Receiver<TailEvent>,
    last_lines_at: &mut Option<Instant>,
) {
    let deadline = Instant::now() + DRAIN_BUDGET;
    loop {
        match lines.try_recv() {
            Ok(event) => {
                if matches!(&event, TailEvent::Lines(l) if !l.is_empty()) {
                    *last_lines_at = Some(Instant::now());
                }
                app.on_tail(event);
            }
            Err(TryRecvError::Empty) => break,
            Err(TryRecvError::Disconnected) => {
                app.status = Some("log reader stopped".to_string());
                break;
            }
        }
        if Instant::now() >= deadline {
            break;
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) enum Message {
    /// Drain the reader thread and let live durations advance.
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
    match state.app.source.as_deref() {
        Some(name) => format!("wowdps — {name}"),
        None => "wowdps".to_string(),
    }
}

fn update(state: &mut Gui, message: Message) -> Task<Message> {
    match message {
        Message::Tick => drain_tail(&mut state.app, &state.lines, &mut state.last_lines_at),
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
                    state.app.apply(action);
                }
            }
        }
        Message::ListRow(row) => {
            state.app.set_list_selection(row);
            state.app.apply(Action::Open);
        }
        Message::MeterRow(row) => {
            state.app.row_sel = row;
            state.app.apply(Action::Open);
        }
    }
    service_loads(&mut state.app);

    if state.app.quit {
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

/// Lazily parse the indexed segment the user just navigated to, exactly like
/// the TUI's between-frames servicing. Synchronous: a boss pull is a few MB
/// of slice, well under a frame's worth of patience. Shared with the overlay.
pub(crate) fn service_loads(app: &mut App) {
    while let Some((pos, meta)) = app.load_request() {
        let Some(path) = app.source_path.clone() else {
            app.load_failed("no log file to load from".to_string());
            break;
        };
        match index::load_segment(&path, &meta) {
            Ok(lines) => {
                app.install_loaded(pos, app::meter_from_lines(lines.iter().map(String::as_str)));
            }
            Err(e) => {
                app.load_failed(format!("{}: {e}", path.display()));
                break;
            }
        }
    }
}
