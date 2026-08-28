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
use crate::talents;
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
            let handoff = first.lock().ok().and_then(|mut slot| slot.take());
            let client = handoff.unwrap_or_else(|| reconnect_forever(ClientKind::Window));
            Gui::new(client, cfg.clone())
        },
        update,
        view::view,
    )
    .title(title)
    .subscription(subscription)
    .theme(theme)
    .style(style)
    .scale_factor(|state| state.cfg.zoom)
    .window(window::Settings {
        size: iced::Size::new(460.0, 640.0),
        min_size: Some(iced::Size::new(320.0, 240.0)),
        // The compositor must see the surface as alpha-capable, or the
        // translucent background composites against black instead of the
        // desktop (see `style`).
        transparent: true,
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

/// Fallback for the state factory's theoretical second call: it must yield a
/// client, and a rendering client without one has nothing to show. Retrying
/// beats aborting — the overlay in particular is supervised, and a crash
/// mid-raid is visible. Unreachable on the normal path (the first connection
/// is handed off).
pub(crate) fn reconnect_forever(kind: ClientKind) -> DaemonClient {
    loop {
        match connect_as(kind) {
            Ok(c) => return c,
            Err(e) => {
                eprintln!("wowdps-gui: {e}; retrying");
                std::thread::sleep(Duration::from_millis(500));
            }
        }
    }
}

pub(crate) struct Gui {
    pub(crate) state: ClientState,
    /// R12/v12: the comparison marker label under the cursor, if any.
    pub(crate) compare_hover: Option<String>,
    /// The graph curve value under the cursor, for the legend's readout.
    pub(crate) graph_probe: Option<f64>,
    client: DaemonClient,
    /// When the last snapshot arrived, wall-clock. WoW buffers its log
    /// writes (sometimes for a long while), so the meter shows how far
    /// behind the file is instead of silently looking frozen.
    last_snapshot_at: Option<Instant>,
    pub(crate) cfg: Config,
    /// The ⚙ options panel is open.
    pub(crate) options_open: bool,
    /// The talent viewer, when open — a window-local screen over the
    /// shared `ClientState` machine, which never learns about it.
    pub(crate) talents: Option<talents::TalentsUi>,
}

impl Gui {
    fn new(mut client: DaemonClient, cfg: Config) -> Self {
        let state = ClientState::new();
        client.send(&state.initial_request());
        Self {
            state,
            compare_hover: None,
            graph_probe: None,
            client,
            last_snapshot_at: None,
            cfg,
            options_open: false,
            talents: None,
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
    /// R12: a meter row's class icon was clicked — pick that player for the
    /// comparison (or unpick them).
    CompareRow(usize),
    /// R12: right-click — drop the picked pair (or a lone half-pick) and
    /// return to the meter. Pointer parity with `Esc`.
    ClearCompare,
    /// R12/v12: a drag on a comparison graph selected a time window (ms from
    /// segment start) — or a right-click asked for the whole fight back.
    CompareRange(Option<(u32, u32)>),
    /// R12/v12: the cursor entered (or left) a marker icon on a comparison
    /// graph; both graphs highlight every use of that item.
    CompareHover(Option<String>),
    /// v14: a drag on the drilldown's graph selected a zoom window (or a
    /// right-click asked for the whole fight back). Client-side only — the
    /// drill timeline is always whole, so nothing round-trips.
    DrillRange(Option<(u32, u32)>),
    /// The curve value under the cursor on any graph — the legend words it
    /// as "dps: 674.5k" while hovering. None when the pointer leaves.
    GraphProbe(Option<f64>),
    /// v16: a by-spell drill row was clicked — descend into that ability.
    SpellRow(usize),
    /// v18: a comparison spell row was clicked — drill BOTH sides into that
    /// ability (by-spell key, label).
    CompareSpell((String, String)),
    /// The header's ⚙ was clicked: open/close the options panel.
    ToggleOptions,
    /// The pointer left the options panel: dismiss it.
    CloseOptions,
    /// Options panel: number meter rows by sort position.
    SetShowRanks(bool),
    /// The talent viewer's own messages (`t` opens it; `talents.rs`).
    Talents(talents::Msg),
    /// Swallow clicks on the options panel's body so they don't fall
    /// through to the meter rows underneath.
    Noop,
}

fn theme(_state: &Gui) -> Theme {
    Theme::TokyoNight
}

/// The translucent look the overlay panel has, for the whole window: the
/// theme's own background at `window_alpha` (the surface is created
/// `transparent: true`, so the remainder shows the desktop through).
fn style(state: &Gui, theme: &Theme) -> iced::theme::Style {
    let palette = theme.palette();
    iced::theme::Style {
        background_color: iced::Color {
            a: state.cfg.window_alpha.clamp(0.0, 1.0),
            ..palette.background
        },
        text_color: palette.text,
    }
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
                } else if let Some(ui) = state.talents.as_mut() {
                    // The viewer swallows the meter keymap: its text input
                    // must be typable without "q" quitting or "d" switching
                    // views. Esc closes it; Tab flips talents/inventory.
                    if modified_key == keyboard::Key::Named(keyboard::key::Named::Escape) {
                        state.talents = None;
                    } else if modified_key == keyboard::Key::Named(keyboard::key::Named::Tab) {
                        ui.on_msg(talents::Msg::ToggleTab);
                    }
                } else if modified_key == keyboard::Key::Character("t".into())
                    && !modifiers.control()
                {
                    // Open on the selected meter row's player when there is
                    // one; a stored simc paste for them wins, else their
                    // spec id draws the empty tree.
                    let player = state
                        .state
                        .rows()
                        .get(state.state.row_sel)
                        .map(|r| (r.label.clone(), r.spec.map(|s| s.id())));
                    state.talents = Some(talents::TalentsUi::open(player));
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
        // R12: pick by class icon. Selecting the row first keeps the keyboard
        // and the pointer on the same player.
        Message::CompareRow(row) => {
            state.state.row_sel = row;
            requests.extend(state.state.apply(Action::PickCompare));
        }
        Message::ClearCompare => {
            requests.extend(state.state.clear_compare());
        }
        Message::CompareRange(range) => {
            requests.extend(state.state.set_compare_range(range));
        }
        Message::CompareHover(label) => state.compare_hover = label,
        Message::DrillRange(range) => state.state.set_drill_range(range),
        Message::GraphProbe(v) => state.graph_probe = v,
        // v16: select the clicked spell row, then Open descends into it.
        Message::SpellRow(i) => {
            if let Some(d) = state.state.drill.as_mut() {
                d.spell_sel = i;
                d.pane = wowdps_model::Pane::Spell;
            }
            requests.extend(state.state.apply(Action::Open));
        }
        Message::CompareSpell((key, label)) => {
            requests.extend(state.state.drill_compare_spell(&key, &label));
        }
        Message::ToggleOptions => state.options_open = !state.options_open,
        Message::CloseOptions => state.options_open = false,
        Message::SetShowRanks(on) => {
            state.cfg.show_ranks = on;
            state.cfg.save();
        }
        Message::Talents(msg) => match msg {
            talents::Msg::Close => state.talents = None,
            // The clipboard read is a Task; its contents come back as
            // another Talents message.
            talents::Msg::PasteClipboard if state.talents.is_some() => {
                return iced::clipboard::read()
                    .map(|c| Message::Talents(talents::Msg::Clipboard(c)));
            }
            // Encode the current (possibly edited) build to the clipboard.
            talents::Msg::CopyString => {
                if let Some(s) = state
                    .talents
                    .as_ref()
                    .and_then(talents::TalentsUi::encode_current)
                {
                    return iced::clipboard::write(s);
                }
            }
            msg => {
                if let Some(ui) = state.talents.as_mut() {
                    ui.on_msg(msg);
                }
            }
        },
        Message::Noop => {}
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
