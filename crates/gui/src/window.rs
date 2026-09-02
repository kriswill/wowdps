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
    /// v19: the in-flight `GetLoadout`'s req_id, matched against `Loadout`
    /// replies. A reply for anything else (a closed viewer, a superseded
    /// open) is dropped.
    pending_loadout: Option<u32>,
    next_req_id: u32,
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
            pending_loadout: None,
            next_req_id: 1,
        }
    }

    /// Seconds since data last arrived, once it stops looking live.
    pub(crate) fn stale_secs(&self) -> Option<u64> {
        stale_secs(self.last_snapshot_at)
    }
}

#[cfg(test)]
impl Gui {
    /// A window over an already-driven `ClientState` and any client (tests
    /// hand in a socketpair whose peer plays daemon, or stays silent).
    pub(crate) fn for_test(client: DaemonClient, state: ClientState, cfg: Config) -> Self {
        let mut gui = Self::new(client, cfg);
        gui.state = state;
        gui
    }

    pub(crate) fn set_last_snapshot_at(&mut self, at: Option<Instant>) {
        self.last_snapshot_at = at;
    }

    pub(crate) fn pending_loadout(&self) -> Option<u32> {
        self.pending_loadout
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
/// v19: `Loadout` replies are one-shots the window consumes itself (the
/// shared state machine treats them as no-ops), so they come back to the
/// caller instead of going through `on_msg`.
pub(crate) fn drain_client(
    state: &mut ClientState,
    client: &mut DaemonClient,
    last_snapshot_at: &mut Option<Instant>,
) -> Vec<DaemonMsg> {
    let mut intercepted = Vec::new();
    for msg in client.poll() {
        if matches!(
            msg,
            DaemonMsg::Snapshot { .. } | DaemonMsg::SegmentList { .. }
        ) {
            *last_snapshot_at = Some(Instant::now());
        }
        if matches!(msg, DaemonMsg::Loadout { .. }) {
            intercepted.push(msg);
            continue;
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
    intercepted
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
        Message::Tick => {
            let intercepted = drain_client(
                &mut state.state,
                &mut state.client,
                &mut state.last_snapshot_at,
            );
            // v19: the answered loadout lands in the open talent viewer. A
            // `None` loadout leaves whatever the viewer opened with (stored
            // simc paste or the empty tree) — the silent fallback.
            for msg in intercepted {
                if let DaemonMsg::Loadout {
                    req_id, loadout, ..
                } = msg
                    && state.pending_loadout == Some(req_id)
                {
                    state.pending_loadout = None;
                    if let (Some(ui), Some(l)) = (state.talents.as_mut(), loadout) {
                        ui.adopt_logged(&l);
                    }
                }
            }
        }
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
                        // A parked reply must not land in a viewer opened
                        // later for someone else.
                        state.pending_loadout = None;
                    } else if modified_key == keyboard::Key::Named(keyboard::key::Named::Tab) {
                        ui.on_msg(talents::Msg::ToggleTab);
                    }
                } else if modified_key == keyboard::Key::Character("t".into())
                    && !modifiers.control()
                {
                    // Open on the selected meter row's player when there is
                    // one: a stored simc paste (or the spec's empty tree)
                    // shows instantly, and the daemon is asked for the
                    // logged COMBATANT_INFO build, which wins when it lands.
                    let row = state.state.rows().get(state.state.row_sel).cloned();
                    let player = row
                        .as_ref()
                        .map(|r| (r.label.clone(), r.spec.map(|s| s.id())));
                    state.talents = Some(talents::TalentsUi::open(player));
                    // Any older request now answers a viewer that no longer
                    // exists; only the request made HERE may adopt.
                    state.pending_loadout = None;
                    if let Some(r) = row {
                        let req_id = state.next_req_id;
                        state.next_req_id = state.next_req_id.wrapping_add(1);
                        state.pending_loadout = Some(req_id);
                        requests.push(wowdps_proto::ClientMsg::GetLoadout {
                            req_id,
                            segment: state.state.watched_segment(),
                            guid: r.key,
                        });
                    }
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
            talents::Msg::Close => {
                state.talents = None;
                state.pending_loadout = None;
            }
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

/// Test scaffolding shared by the window's render tests (`view.rs`,
/// `compare.rs`, `gauge.rs`): a `DaemonClient` over a socketpair, a bridge
/// that lets the daemon's in-process mock answer that socket, and headless
/// rendering through `iced_test` (tiny-skia, no display).
#[cfg(test)]
pub(crate) mod testkit {
    use std::io::Write;
    use std::os::unix::net::UnixStream;
    use std::sync::Once;

    use iced::Element;
    use wowdps_daemon::mock::{MockDaemon, pump};
    use wowdps_model::Action;
    use wowdps_proto::{
        ClientKind, ClientMsg, ClientState, DaemonClient, DaemonMsg, PROTO_VERSION,
    };

    use super::{Gui, Message, update};
    use crate::config::Config;

    /// A client whose handshake a thread on the peer end answered; the peer
    /// comes back so a test can play daemon (or drop it to play a crash).
    pub(crate) fn fake_client() -> (DaemonClient, UnixStream) {
        let (ours, theirs) = UnixStream::pair().unwrap();
        let mut peer = theirs.try_clone().unwrap();
        let ack = std::thread::spawn(move || {
            let (tag, body) = wowdps_proto::wire::read_frame(&mut peer).unwrap();
            let hello = ClientMsg::decode(tag, &body).unwrap();
            assert!(matches!(hello, ClientMsg::Hello { .. }), "{hello:?}");
            peer.write_all(
                &DaemonMsg::HelloAck {
                    proto: PROTO_VERSION,
                    version: "test".to_string(),
                }
                .encode(),
            )
            .unwrap();
        });
        let client = DaemonClient::over(ours, ClientKind::Window).unwrap();
        ack.join().unwrap();
        (client, theirs)
    }

    /// A window over a pre-driven state. The socket peer is kept alive but
    /// silent, so the client neither answers nor looks dead.
    pub(crate) fn gui_over(state: ClientState) -> (Gui, UnixStream) {
        let (client, peer) = fake_client();
        (Gui::for_test(client, state, Config::default()), peer)
    }

    /// Keep every config write these tests trigger out of the real
    /// `~/.config`: a process-wide scratch `XDG_CONFIG_HOME`, set once.
    pub(crate) fn isolate_config() {
        static ONCE: Once = Once::new();
        ONCE.call_once(|| {
            let dir = std::env::temp_dir().join(format!("wowdps-gui-tests-{}", std::process::id()));
            // SAFETY: set before any test thread reads it through
            // `Config::path`; every caller funnels through this `Once`.
            unsafe { std::env::set_var("XDG_CONFIG_HOME", dir) };
        });
    }

    // ---- ClientState builders over the mock daemon ---------------------

    pub(crate) fn apply(state: &mut ClientState, mock: &mut MockDaemon, action: Action) {
        let reqs = state.apply(action);
        pump(state, mock, reqs);
    }

    /// Indexed startup over the whole fixture: the list screen.
    pub(crate) fn indexed() -> (ClientState, MockDaemon) {
        let mut mock = MockDaemon::fixture();
        let mut state = ClientState::new();
        let first = state.initial_request();
        pump(&mut state, &mut mock, vec![first]);
        (state, mock)
    }

    /// The meter on the newest segment: the final wipe.
    pub(crate) fn wipe() -> (ClientState, MockDaemon) {
        let (mut state, mut mock) = indexed();
        apply(&mut state, &mut mock, Action::Open);
        (state, mock)
    }

    /// Mid-fight arrival: the live meter.
    pub(crate) fn live() -> (ClientState, MockDaemon) {
        let mut mock = MockDaemon::fixture_live();
        let mut state = ClientState::new();
        let first = state.initial_request();
        pump(&mut state, &mut mock, vec![first]);
        (state, mock)
    }

    /// The boss kill — the fixture's richest segment.
    pub(crate) fn kill() -> (ClientState, MockDaemon) {
        let (mut state, mut mock) = wipe();
        apply(&mut state, &mut mock, Action::OlderSegment);
        apply(&mut state, &mut mock, Action::OlderSegment);
        assert_eq!(state.segment_name().as_deref(), Some("The Ashen Warden"));
        (state, mock)
    }

    /// The kill's top row drilled open (by spell / by target + timeline).
    pub(crate) fn drilled() -> (ClientState, MockDaemon) {
        let (mut state, mut mock) = kill();
        apply(&mut state, &mut mock, Action::Open);
        assert!(state.drill.is_some());
        (state, mock)
    }

    /// One level deeper: the drilled player's top ability.
    pub(crate) fn spell_drilled() -> (ClientState, MockDaemon) {
        let (mut state, mut mock) = drilled();
        apply(&mut state, &mut mock, Action::Open);
        assert!(state.drill_spell().is_some());
        (state, mock)
    }

    /// The kill's top two players compared.
    pub(crate) fn compared() -> (ClientState, MockDaemon) {
        let (mut state, mut mock) = kill();
        apply(&mut state, &mut mock, Action::PickCompare);
        apply(&mut state, &mut mock, Action::Down);
        apply(&mut state, &mut mock, Action::PickCompare);
        assert_eq!(state.screen, wowdps_model::Screen::Compare);
        assert!(state.compare_sides().is_some(), "the mock answers inline");
        (state, mock)
    }

    // ---- the socket bridge -----------------------------------------------

    /// A window whose socket the mock daemon answers: requests the window
    /// writes are read off the peer, handled, and the replies written back,
    /// so `update`/`drain_client` run over the real client plumbing.
    pub(crate) struct Bridge {
        pub gui: Gui,
        pub mock: MockDaemon,
        peer: UnixStream,
    }

    impl Bridge {
        pub(crate) fn new(mock: MockDaemon) -> Self {
            let (client, peer) = fake_client();
            peer.set_read_timeout(Some(std::time::Duration::from_millis(10)))
                .unwrap();
            let gui = Gui::for_test(client, ClientState::new(), Config::default());
            let mut b = Self { gui, mock, peer };
            b.settle();
            b
        }

        /// Every request the window has written so far.
        pub(crate) fn requests(&mut self) -> Vec<ClientMsg> {
            let mut out = Vec::new();
            while let Ok((tag, body)) = wowdps_proto::wire::read_frame(&mut self.peer) {
                out.push(ClientMsg::decode(tag, &body).unwrap());
            }
            out
        }

        /// Push one daemon message at the window (its reader thread picks
        /// it up; the next `settle` drains it).
        pub(crate) fn push(&mut self, msg: &DaemonMsg) {
            self.peer.write_all(&msg.encode()).unwrap();
        }

        /// Serve requests and tick until the exchange has been quiet for a
        /// while — the client's reader thread delivers asynchronously.
        pub(crate) fn settle(&mut self) {
            let mut quiet = 0;
            while quiet < 8 {
                let reqs = self.requests();
                if reqs.is_empty() {
                    quiet += 1;
                } else {
                    quiet = 0;
                    let mut replies = Vec::new();
                    for req in reqs {
                        replies.extend(self.mock.handle(req));
                    }
                    for reply in replies {
                        self.push(&reply);
                    }
                }
                std::thread::sleep(std::time::Duration::from_millis(5));
                let _ = update(&mut self.gui, Message::Tick);
            }
        }

        pub(crate) fn send(&mut self, msg: Message) {
            let _ = update(&mut self.gui, msg);
            self.settle();
        }
    }

    // ---- headless rendering ----------------------------------------------

    /// iced_test tries wgpu first; a bare runner has no adapter and the dev
    /// box would spin a GPU device per test. Pin the software renderer.
    fn force_tiny_skia() {
        static ONCE: Once = Once::new();
        ONCE.call_once(|| {
            // SAFETY: set once, before the first simulator is built.
            unsafe { std::env::set_var("ICED_TEST_BACKEND", "tiny-skia") };
        });
    }

    pub(crate) fn simulator<'a, M: 'a>(el: Element<'a, M>) -> iced_test::Simulator<'a, M> {
        force_tiny_skia();
        iced_test::Simulator::with_size(
            iced::Settings::default(),
            iced::Size::new(640.0, 480.0),
            el,
        )
    }

    /// Lay out and draw an element through the software renderer, so every
    /// style closure and canvas program in it actually runs.
    pub(crate) fn render<'a, M: 'a>(el: Element<'a, M>) -> iced_test::simulator::Snapshot {
        let mut ui = simulator(el);
        ui.snapshot(&iced::Theme::TokyoNight).unwrap()
    }

    /// A software renderer for calling canvas `Program::draw` directly.
    pub(crate) fn renderer() -> iced::Renderer {
        use iced_test::core::renderer::Headless;
        iced_test::futures::futures::executor::block_on(iced::Renderer::new(
            iced::Font::DEFAULT,
            iced::Pixels(14.0),
            Some("tiny-skia"),
        ))
        .unwrap()
    }

    /// A key press as the window sees it.
    pub(crate) fn key(k: iced::keyboard::Key, modifiers: iced::keyboard::Modifiers) -> Message {
        Message::Key(iced::keyboard::Event::KeyPressed {
            key: k.clone(),
            modified_key: k,
            physical_key: iced::keyboard::key::Physical::Unidentified(
                iced::keyboard::key::NativeCode::Unidentified,
            ),
            location: iced::keyboard::Location::Standard,
            modifiers,
            repeat: false,
            text: None,
        })
    }

    pub(crate) fn chr(c: &str) -> Message {
        key(
            iced::keyboard::Key::Character(c.into()),
            iced::keyboard::Modifiers::default(),
        )
    }

    pub(crate) fn named(n: iced::keyboard::key::Named) -> Message {
        key(
            iced::keyboard::Key::Named(n),
            iced::keyboard::Modifiers::default(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::testkit::{Bridge, chr, fake_client, gui_over, isolate_config, key, named};
    use super::*;
    use iced::keyboard::key::Named;
    use iced::keyboard::{Key, Modifiers};
    use wowdps_daemon::mock::MockDaemon;
    use wowdps_model::{Pane, Screen, View};
    use wowdps_proto::{ClientMsg, Cursor};

    #[test]
    fn staleness_starts_at_five_seconds() {
        assert_eq!(stale_secs(None), None);
        assert_eq!(stale_secs(Some(Instant::now())), None);
        let old = Instant::now() - Duration::from_secs(7);
        assert_eq!(stale_secs(Some(old)), Some(7));
        let (mut gui, _peer) = gui_over(ClientState::new());
        assert_eq!(gui.stale_secs(), None);
        gui.set_last_snapshot_at(Some(old));
        assert_eq!(gui.stale_secs(), Some(7));
    }

    #[test]
    fn title_names_the_source_and_style_uses_the_window_alpha() {
        let (mut gui, _peer) = gui_over(ClientState::new());
        assert_eq!(title(&gui), "wowdps");
        gui.state.source = Some("WoWCombatLog-1.txt".to_string());
        assert_eq!(title(&gui), "wowdps — WoWCombatLog-1.txt");
        gui.cfg.window_alpha = 0.5;
        let t = theme(&gui);
        let s = style(&gui, &t);
        assert_eq!(s.background_color.a, 0.5);
        assert_eq!(s.text_color, t.palette().text);
        gui.cfg.window_alpha = 7.0;
        assert_eq!(style(&gui, &t).background_color.a, 1.0);
        // Just building the subscription: it batches the tick and the keys.
        let _ = subscription(&gui);
    }

    #[test]
    fn a_new_window_declares_the_list_cursor() {
        let (client, mut peer) = fake_client();
        let _gui = Gui::for_test(client, ClientState::new(), Config::default());
        let (tag, body) = wowdps_proto::wire::read_frame(&mut peer).unwrap();
        assert_eq!(
            ClientMsg::decode(tag, &body).unwrap(),
            ClientMsg::Watch(Cursor::List)
        );
    }

    #[test]
    fn ticks_drain_the_daemon_into_the_state() {
        let b = Bridge::new(MockDaemon::fixture());
        assert_eq!(b.gui.state.screen, Screen::List);
        assert!(
            b.gui.state.segment_count() >= 3,
            "the fixture's list arrived"
        );
        assert!(b.gui.state.source.is_some());
        assert_eq!(b.gui.stale_secs(), None, "data just arrived");
    }

    #[test]
    fn arriving_mid_fight_lands_on_the_live_meter() {
        // The list's `active` verdict makes the state answer with a new
        // Watch, which the drain forwards to the daemon.
        let b = Bridge::new(MockDaemon::fixture_live());
        assert_eq!(b.gui.state.screen, Screen::Meter);
        assert!(b.gui.state.is_live());
        assert!(!b.gui.state.rows().is_empty());
    }

    #[test]
    fn pointer_rows_open_meter_drill_and_ability() {
        let mut b = Bridge::new(MockDaemon::fixture());
        b.send(Message::ListRow(0));
        assert_eq!(b.gui.state.screen, Screen::Meter);
        assert_eq!(b.gui.state.segment_index(), 0);
        assert!(!b.gui.state.rows().is_empty());

        b.send(Message::MeterRow(1));
        assert_eq!(b.gui.state.row_sel, 1);
        let drill = b.gui.state.drill.clone().unwrap();
        assert_eq!(drill.label, b.gui.state.rows()[1].label);

        b.send(Message::SpellRow(0));
        let drill = b.gui.state.drill.clone().unwrap();
        assert_eq!(drill.pane, Pane::Spell);
        assert_eq!(drill.spell_sel, 0);
        assert!(drill.spell.is_some(), "Open descended into the ability");

        b.send(Message::DrillRange(Some((1_000, 5_000))));
        assert_eq!(b.gui.state.drill_range(), Some((1_000, 5_000)));
        b.send(Message::DrillRange(None));
        assert_eq!(b.gui.state.drill_range(), None);
    }

    #[test]
    fn class_icons_pick_the_comparison_and_right_click_clears_it() {
        let mut b = Bridge::new(MockDaemon::fixture());
        b.send(Message::ListRow(0));
        b.send(Message::CompareRow(0));
        assert_eq!(b.gui.state.compare_picks().len(), 1);
        assert_eq!(b.gui.state.screen, Screen::Meter);
        b.send(Message::CompareRow(1));
        assert_eq!(b.gui.state.screen, Screen::Compare);
        let (a, bb) = b.gui.state.compare_sides().expect("both sides answered");
        let spell = a.spells.first().cloned().expect("the side has spells");
        assert_ne!(a.guid, bb.guid);

        b.send(Message::CompareHover(Some("Potion".to_string())));
        assert_eq!(b.gui.compare_hover.as_deref(), Some("Potion"));
        b.send(Message::GraphProbe(Some(12.5)));
        assert_eq!(b.gui.graph_probe, Some(12.5));

        b.send(Message::CompareRange(Some((0, 10_000))));
        assert_eq!(b.gui.state.compare_shown_range(), Some((0, 10_000)));
        b.send(Message::CompareRange(None));
        assert_eq!(b.gui.state.compare_shown_range(), None);

        b.send(Message::CompareSpell((
            spell.key.clone(),
            spell.label.clone(),
        )));
        assert_eq!(
            b.gui.state.compare_spell().map(|(k, _)| k.as_str()),
            Some(spell.key.as_str())
        );

        // Right-click backs out one level: the ability first, then the pair.
        b.send(Message::ClearCompare);
        assert_eq!(b.gui.state.screen, Screen::Compare);
        assert!(b.gui.state.compare_spell().is_none());
        b.send(Message::ClearCompare);
        assert_eq!(b.gui.state.screen, Screen::Meter);
        assert!(b.gui.state.compare_picks().is_empty());
    }

    #[test]
    fn keys_reach_the_shared_keymap() {
        let mut b = Bridge::new(MockDaemon::fixture());
        b.send(named(Named::Enter));
        assert_eq!(b.gui.state.screen, Screen::Meter);
        b.send(chr("j"));
        assert_eq!(b.gui.state.row_sel, 1);
        b.send(chr("h"));
        assert_eq!(b.gui.state.view, View::Healing);
        b.send(named(Named::Escape));
        assert_eq!(b.gui.state.screen, Screen::List);
        // Unknown keys are ignored.
        b.send(chr("z"));
        assert_eq!(b.gui.state.screen, Screen::List);
        b.send(chr("q"));
        assert!(b.gui.state.quit);
    }

    #[test]
    fn zoom_chords_step_and_clamp() {
        isolate_config();
        let (mut gui, _peer) = gui_over(ClientState::new());
        let base = gui.cfg.zoom;
        let _ = update(&mut gui, key(Key::Character("=".into()), Modifiers::CTRL));
        assert!((gui.cfg.zoom - (base + ZOOM_STEP)).abs() < 1e-6);
        let _ = update(&mut gui, key(Key::Character("0".into()), Modifiers::CTRL));
        assert_eq!(gui.cfg.zoom, Config::default().zoom);
        for _ in 0..40 {
            let _ = update(&mut gui, key(Key::Character("-".into()), Modifiers::CTRL));
        }
        assert_eq!(gui.cfg.zoom, *ZOOM_RANGE.start());
        for _ in 0..40 {
            let _ = update(&mut gui, key(Key::Character("+".into()), Modifiers::CTRL));
        }
        assert_eq!(gui.cfg.zoom, *ZOOM_RANGE.end());
    }

    #[test]
    fn options_panel_toggles_and_saves_ranks() {
        isolate_config();
        let (mut gui, _peer) = gui_over(ClientState::new());
        assert!(!gui.options_open);
        let _ = update(&mut gui, Message::ToggleOptions);
        assert!(gui.options_open);
        let _ = update(&mut gui, Message::Noop);
        assert!(gui.options_open);
        let _ = update(&mut gui, Message::CloseOptions);
        assert!(!gui.options_open);
        let _ = update(&mut gui, Message::SetShowRanks(false));
        assert!(!gui.cfg.show_ranks);
        let _ = update(&mut gui, Message::SetShowRanks(true));
        assert!(gui.cfg.show_ranks);
    }

    #[test]
    fn t_opens_the_talent_viewer_on_the_selected_player_and_asks_for_the_loadout() {
        let mut b = Bridge::new(MockDaemon::fixture());
        b.send(named(Named::Enter));
        let top = b.gui.state.rows()[0].clone();
        b.send(chr("t"));
        let ui = b.gui.talents.as_ref().expect("viewer open");
        assert_eq!(ui.player.as_deref(), Some(top.label.as_str()));
        // The mock answered the GetLoadout inline; the reply was consumed.
        assert_eq!(b.gui.pending_loadout(), None);

        // The meter keymap is swallowed while the viewer is up.
        b.send(chr("q"));
        assert!(!b.gui.state.quit);
        b.send(named(Named::Tab));
        assert!(b.gui.talents.is_some());
        b.send(named(Named::Escape));
        assert!(b.gui.talents.is_none());
        assert_eq!(
            b.gui.state.screen,
            Screen::Meter,
            "Esc closed the viewer, not the meter"
        );
    }

    #[test]
    fn t_without_a_row_opens_an_empty_viewer() {
        let (mut gui, _peer) = gui_over(ClientState::new());
        let _ = update(&mut gui, chr("t"));
        assert!(gui.talents.is_some());
        assert_eq!(gui.pending_loadout(), None, "nothing to ask about");
        let _ = update(&mut gui, Message::Talents(talents::Msg::Close));
        assert!(gui.talents.is_none());
        // Ctrl-t is not the viewer.
        let _ = update(&mut gui, key(Key::Character("t".into()), Modifiers::CTRL));
        assert!(gui.talents.is_none());
    }

    #[test]
    fn talent_messages_route_to_the_open_viewer() {
        let (mut gui, _peer) = gui_over(ClientState::new());
        // Nobody to route to: all no-ops.
        let _ = update(&mut gui, Message::Talents(talents::Msg::PasteClipboard));
        let _ = update(&mut gui, Message::Talents(talents::Msg::CopyString));
        let _ = update(&mut gui, Message::Talents(talents::Msg::ToggleTab));
        assert!(gui.talents.is_none());

        let _ = update(&mut gui, chr("t"));
        let _ = update(
            &mut gui,
            Message::Talents(talents::Msg::Input("abc".to_string())),
        );
        assert_eq!(gui.talents.as_ref().unwrap().input, "abc");
        let _ = update(&mut gui, Message::Talents(talents::Msg::PasteClipboard));
        let _ = update(&mut gui, Message::Talents(talents::Msg::CopyString));
        let _ = update(&mut gui, Message::Talents(talents::Msg::ToggleTab));
        assert!(gui.talents.is_some());
    }

    #[test]
    fn a_stale_loadout_reply_is_dropped() {
        let mut b = Bridge::new(MockDaemon::fixture());
        b.send(named(Named::Enter));
        let guid = b.gui.state.rows()[0].key.clone();
        b.push(&DaemonMsg::Loadout {
            req_id: 999,
            guid,
            loadout: Some(wowdps_model::Loadout::default()),
        });
        b.settle();
        assert!(b.gui.talents.is_none(), "no viewer, nothing adopted");
        assert_eq!(b.gui.pending_loadout(), None);
    }

    #[test]
    fn a_vanished_daemon_is_reported_in_the_footer() {
        let (client, peer) = fake_client();
        let mut gui = Gui::for_test(client, ClientState::new(), Config::default());
        drop(peer);
        // The reader thread notices the hangup; give it a moment.
        for _ in 0..50 {
            let _ = update(&mut gui, Message::Tick);
            if gui.state.status.is_some() {
                break;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        assert_eq!(
            gui.state.status.as_deref(),
            Some("daemon gone — reconnecting…"),
            "no daemon binary to respawn, so the notice sticks"
        );
    }
}
