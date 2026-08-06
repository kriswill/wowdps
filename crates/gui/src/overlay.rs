//! The wlr-layer-shell overlay: a slim tab pinned to a configured screen
//! edge, on the compositor's `overlay` layer so it stays visible above the
//! fullscreen game on every workspace of its output.
//!
//! Under Hyprland (and unless `follow_game = false`), the surface instead
//! tracks the game: a [`crate::hypr`] thread watches which workspace the
//! game window is on, and whenever that workspace is not on screen the
//! overlay hides. Layer-shell has no unmap, so "hidden" means shrunk to a
//! 1×1 transparent, click-through pixel — see [`apply_visibility`].
//!
//! Clicking the tab expands a narrow live meter; clicking the panel header
//! collapses it again. Dragging either slides the surface along its edge and
//! persists the position to the config file on release. Under Hyprland the
//! tab can be dragged around the whole screen perimeter — it reorients onto
//! whichever monitor edge is nearest (horizontal along top/bottom, vertical
//! on the sides), clamped to the monitor, and the panel then opens into the
//! screen from wherever the tab sits. The surface never takes keyboard
//! focus (`KeyboardInteractivity::None`), so the game keeps every
//! keystroke; interaction is mouse-only.

use std::sync::mpsc::Receiver;
use std::time::Instant;

use iced::widget::{Space, column, container, mouse_area, row, scrollable, text};
use iced::{Color, Element, Event, Length, Subscription, Task, Theme, event, mouse, time};
use iced_layershell::actions::ActionCallback;
use iced_layershell::reexport::{Anchor, KeyboardInteractivity, Layer};
use iced_layershell::settings::{LayerShellSettings, StartMode};
use iced_layershell::to_layer_message;

use wowdps_model::fmt::{duration, view_name};
use wowdps_model::{Action, ListRow, Screen, SegmentId, SegmentKind, View};
use wowdps_proto::{
    ClientKind, ClientMsg, ClientState, Cursor, DaemonClient, DaemonMsg, SegmentRef,
};

use crate::config::{Config, Edge};
use crate::hypr;
use crate::timeline;
use crate::view::{
    DIM, GREEN, OVERLAY_DRILL_COLS, RED, YELLOW, overlay_drill_row, overlay_row, recap_row,
};
use crate::window::{TICK, stale_secs};

/// Tab dimensions: thin across the edge, long along it.
const TAB_THICKNESS: u32 = 26;
const TAB_LENGTH: u32 = 96;
/// A press that travels less than this many pixels is a click, not a drag.
const DRAG_THRESHOLD: f32 = 5.0;

pub fn run(cfg: Config) -> Result<(), String> {
    // Replace any running overlay before touching the daemon: even a wedged
    // or version-skewed incumbent gets evicted, and never later than the
    // moment a second surface could appear.
    crate::single::claim_overlay(|| {
        eprintln!("wowdps-gui: replaced by a newer overlay, exiting");
        std::process::exit(0);
    });
    let first = std::sync::Mutex::new(Some(crate::window::connect_as(
        wowdps_proto::ClientKind::Overlay,
    )?));
    let start_mode = match cfg.monitor.clone() {
        Some(name) => StartMode::TargetScreen(name),
        None => StartMode::Active,
    };
    let layer_settings = LayerShellSettings {
        anchor: anchor_for(cfg.edge),
        // The only layer that stacks above fullscreen windows.
        layer: Layer::Overlay,
        exclusive_zone: -1,
        size: Some(if start_expanded() {
            (cfg.width, cfg.height)
        } else {
            tab_size(cfg.edge, cfg.zoom)
        }),
        margin: margin_for(cfg.edge, cfg.offset),
        keyboard_interactivity: KeyboardInteractivity::None,
        start_mode,
        events_transparent: false,
    };

    iced_layershell::application(
        move || {
            let client = first
                .lock()
                .expect("client handoff poisoned")
                .take()
                .unwrap_or_else(|| {
                    crate::window::connect_as(wowdps_proto::ClientKind::Overlay)
                        .expect("daemon vanished during startup")
                });
            Overlay::new(client, cfg.clone())
        },
        || String::from("wowdps"),
        update,
        view,
    )
    // NOTE: no `.scale_factor()` here — iced_layershell 0.19 scales the
    // layout by it but not the pointer coordinates, so every hit-test misses
    // once it isn't 1.0. The overlay zooms manually via `cfg.zoom` instead.
    .style(style)
    .theme(theme)
    .subscription(subscription)
    .layer_settings(layer_settings)
    .run()
    .map_err(|e| e.to_string())
}

#[derive(Clone, Copy)]
enum Drag {
    /// Hyprland: the drag follows the compositor-global cursor, clamped to
    /// the monitor grabbed on. Tab drags roam the whole perimeter,
    /// reorienting to the nearest edge; `grab`/`base` are re-seeded on each
    /// edge flip so the slide math stays local to the current edge.
    Global {
        /// Global cursor at the press (or the last edge flip).
        grab: (f32, f32),
        /// `shown_offset` at that same moment.
        base: i32,
        /// Logical rect of the monitor the drag started on.
        mon: (i32, i32, i32, i32),
        moved: bool,
    },
    /// Fallback without Hyprland: surface-local chase along the current
    /// edge — each event's distance from the grab point is how far the
    /// surface still has to move; no reorientation, no clamping.
    Local { grab: f32, moved: bool },
}

impl Drag {
    fn moved(&self) -> bool {
        match self {
            Drag::Global { moved, .. } | Drag::Local { moved, .. } => *moved,
        }
    }
}

struct Overlay {
    app: ClientState,
    client: DaemonClient,
    last_snapshot_at: Option<Instant>,
    cfg: Config,
    expanded: bool,
    /// Offset the surface actually sits at right now. Usually `cfg.offset`
    /// (the tab's durable anchor), but the expanded panel may borrow a
    /// shifted value to fit on screen, and drags move this first — only a
    /// settled drag writes it back to `cfg.offset`.
    shown_offset: i32,
    /// Last observed cursor position along the edge axis, surface-local.
    cursor: f32,
    drag: Option<Drag>,
    /// Workspace-visibility transitions from the Hyprland tracker; `None`
    /// when not under Hyprland or `follow_game` is off.
    hypr: Option<Receiver<bool>>,
    /// Hyprland's IPC socket directory, for global-cursor drag queries.
    /// Independent of `follow_game`.
    hypr_dir: Option<std::path::PathBuf>,
    /// Whether the game's workspace is on screen (always true untracked).
    game_visible: bool,
    /// The daemon supervisor's wish (`SetVisible`); composed with
    /// `game_visible` — either saying "hide" hides.
    daemon_visible: bool,
    /// Ticks until the debug auto-toggle fires; 0 = disabled.
    autotoggle: u32,
    /// Debug aid: drill into the top row once data arrives, for
    /// screenshotting the drilldown on outputs nothing can click.
    autodrill: bool,
    /// Process start, for debug-trace timestamps.
    started: Instant,
    /// Footer Σ toggle: show the instance's Σ overall under the current
    /// fight's rows.
    split: bool,
    /// Second daemon connection for the split view, watching the instance's
    /// Σ overall. `Window`-kind on purpose: a second `Overlay`-kind session
    /// would confuse the daemon's overlay supervisor.
    aux: Option<DaemonClient>,
    /// What the aux connection currently watches: (Σ overall id, view).
    aux_watch: Option<(SegmentId, View)>,
    aux_rows: Vec<wowdps_model::Row>,
    aux_info: Option<wowdps_model::SegmentInfo>,
}

impl Overlay {
    fn new(mut client: DaemonClient, cfg: Config) -> Self {
        let hypr = cfg
            .follow_game
            .then(|| hypr::spawn(cfg.game_match.clone()))
            .flatten();
        let shown_offset = cfg.offset;
        let split = cfg.overlay_split;
        let mut app = ClientState::new();
        // Debug aid: WOWDPS_OVERLAY_AUTOVIEW=deaths (etc.) starts on that
        // view, for screenshotting view-specific panes headlessly.
        if let Some(view) = start_view() {
            app.view = view;
        }
        client.send(&app.initial_request());
        Self {
            app,
            client,
            last_snapshot_at: None,
            cfg,
            expanded: start_expanded(),
            shown_offset,
            cursor: 0.0,
            drag: None,
            hypr,
            hypr_dir: hypr::socket_dir(),
            game_visible: true,
            daemon_visible: true,
            autotoggle: if std::env::var_os("WOWDPS_OVERLAY_AUTOTOGGLE").is_some() {
                20
            } else {
                0
            },
            autodrill: std::env::var_os("WOWDPS_OVERLAY_AUTODRILL").is_some(),
            started: Instant::now(),
            split,
            aux: None,
            aux_watch: None,
            aux_rows: Vec::new(),
            aux_info: None,
        }
    }
}

/// Flip between the tab and the panel. `AnchorSizeChange` (not the bare
/// `SizeChange`) is the resize path upstream exercises in its own examples.
fn toggle(state: &mut Overlay) -> Task<Message> {
    state.expanded = !state.expanded;
    let size = current_size(state);
    if debug() {
        eprintln!(
            "overlay: [{:>8.1}ms] toggle -> expanded={} size={size:?}",
            state.started.elapsed().as_secs_f64() * 1000.0,
            state.expanded
        );
    }
    // The tab's anchor (`cfg.offset`) is the durable position: the panel
    // only borrows it, shifted just enough to fit on screen, and collapsing
    // returns to it untouched.
    state.shown_offset = state.cfg.offset;
    if state.expanded
        && let Some(max) = max_offset(state, size)
    {
        state.shown_offset = state.cfg.offset.min(max);
    }
    Task::batch([
        Task::done(Message::AnchorSizeChange(anchor_for(state.cfg.edge), size)),
        Task::done(Message::MarginChange(margin_for(
            state.cfg.edge,
            state.shown_offset,
        ))),
    ])
}

/// Surface size for the current expanded/collapsed state.
fn current_size(state: &Overlay) -> (u32, u32) {
    if state.expanded {
        (state.cfg.width, state.cfg.height)
    } else {
        tab_size(state.cfg.edge, state.cfg.zoom)
    }
}

/// Debug aid: begin expanded instead of as a tab, for headless screenshots
/// and layout work where nothing can click the grip.
fn start_expanded() -> bool {
    std::env::var_os("WOWDPS_OVERLAY_START_EXPANDED").is_some()
}

/// Debug aid: trace input on stderr (`WOWDPS_OVERLAY_DEBUG=1`).
fn debug() -> bool {
    std::env::var_os("WOWDPS_OVERLAY_DEBUG").is_some()
}

/// Debug aid: the view named by `WOWDPS_OVERLAY_AUTOVIEW`, if any.
fn start_view() -> Option<View> {
    match std::env::var("WOWDPS_OVERLAY_AUTOVIEW").ok()?.as_str() {
        "damage" => Some(View::Damage),
        "healing" => Some(View::Healing),
        "interrupts" => Some(View::Interrupts),
        "cc" => Some(View::CrowdControl),
        "dispels" => Some(View::Dispels),
        "deaths" => Some(View::Deaths),
        _ => None,
    }
}

#[to_layer_message]
#[derive(Debug, Clone)]
enum Message {
    Tick,
    Ice(Event),
    /// Press on the tab (collapsed) or the panel header (expanded): the
    /// start of a click-toggle or a drag, told apart on release.
    GripPressed,
    GripReleased,
    CycleView,
    /// Bottom-center arrows: step to the previous / next *block* — a whole
    /// instance visit or a stray fight — never through a visit's members.
    PrevBlock,
    NextBlock,
    /// Timeline strip or chip scrubber: jump to this combined-list position.
    TimelineGoto(usize),
    /// Footer: return to following the live fight.
    GoLive,
    /// Footer Σ: split the rows into current fight + instance overall.
    ToggleSplit,
    /// A meter row was clicked: drill into that player's spells.
    RowClicked(usize),
    /// Wheel over the header (or the collapsed tab): scale the whole UI by
    /// this many notches — keyboard modifiers never reach the overlay
    /// (`KeyboardInteractivity::None`; the game keeps every keystroke), so
    /// the zoom gesture is placement-scoped instead of Ctrl-scoped.
    Zoom(f32),
}

/// Wheel notches from a scroll event: one line = one zoom step; touchpad
/// pixels are normalized to roughly a line per 40px.
fn notches(delta: mouse::ScrollDelta) -> f32 {
    match delta {
        mouse::ScrollDelta::Lines { y, .. } => y,
        mouse::ScrollDelta::Pixels { y, .. } => y / 40.0,
    }
}

fn theme(_state: &Overlay) -> Theme {
    Theme::TokyoNight
}

/// The surface itself is transparent; panels draw their own backgrounds.
fn style(_state: &Overlay, theme: &Theme) -> iced::theme::Style {
    iced::theme::Style {
        background_color: Color::TRANSPARENT,
        text_color: theme.palette().text,
    }
}

fn subscription(_state: &Overlay) -> Subscription<Message> {
    Subscription::batch([
        time::every(TICK).map(|_| Message::Tick),
        event::listen().map(Message::Ice),
    ])
}

fn update(state: &mut Overlay, message: Message) -> Task<Message> {
    match message {
        Message::Tick => {
            let wishes = drain_overlay(state);
            let mut tasks = Vec::new();
            // Honor the daemon supervisor's visibility wish (last one wins).
            if let Some(visible) = wishes.last().copied()
                && visible != state.daemon_visible
            {
                state.daemon_visible = visible;
                if debug() {
                    eprintln!(
                        "overlay: [{:>8.1}ms] daemon SetVisible({visible})",
                        state.started.elapsed().as_secs_f64() * 1000.0
                    );
                }
                tasks.push(apply_visibility(state));
            }
            // The overlay has no list screen: pin to the newest segment as
            // soon as one exists.
            if state.app.screen == Screen::List && state.app.segment_count() > 0 {
                state.app.set_list_selection(usize::MAX);
                for req in state.app.apply(Action::Open) {
                    state.client.send(&req);
                }
            }
            // Follow the game's workspace: only the latest transition counts.
            if let Some(rx) = &state.hypr {
                let mut latest = None;
                while let Ok(visible) = rx.try_recv() {
                    latest = Some(visible);
                }
                if let Some(visible) = latest.filter(|&v| v != state.game_visible) {
                    state.game_visible = visible;
                    if debug() {
                        eprintln!(
                            "overlay: [{:>8.1}ms] game workspace visible={visible}",
                            state.started.elapsed().as_secs_f64() * 1000.0
                        );
                    }
                    tasks.push(apply_visibility(state));
                }
            }
            // Debug aid: WOWDPS_OVERLAY_AUTODRILL opens the top row's
            // drilldown as soon as there is one to open.
            if state.autodrill && state.app.drill.is_none() && !state.app.rows().is_empty() {
                state.autodrill = false;
                state.app.row_sel = 0;
                for req in state.app.apply(Action::Open) {
                    state.client.send(&req);
                }
            }
            // Debug aid: WOWDPS_OVERLAY_AUTOTOGGLE flips the panel once after
            // ~2s, so resizing can be verified on outputs nothing can click.
            if state.autotoggle > 0 {
                state.autotoggle -= 1;
                if state.autotoggle == 0 {
                    tasks.push(toggle(state));
                }
            }
            sync_aux(state);
            Task::batch(tasks)
        }
        Message::Ice(Event::Mouse(mouse::Event::CursorMoved { position })) => {
            state.cursor = if state.cfg.edge.is_vertical() {
                position.y
            } else {
                position.x
            };
            drag_motion(state)
        }
        Message::Ice(Event::Mouse(
            m @ (mouse::Event::ButtonPressed(_) | mouse::Event::CursorEntered),
        )) => {
            if debug() {
                eprintln!("overlay: mouse {m:?}");
            }
            // Right-click backs out of an open drilldown to the rank list.
            // Never past it: with no drill open, Back would land on the list
            // screen the overlay doesn't have (the tick would re-pin it).
            if matches!(m, mouse::Event::ButtonPressed(mouse::Button::Right))
                && state.app.drill.is_some()
            {
                for req in state.app.apply(Action::Back) {
                    state.client.send(&req);
                }
            }
            Task::none()
        }
        Message::Ice(Event::Mouse(m @ mouse::Event::ButtonReleased(_))) => {
            if debug() {
                eprintln!("overlay: mouse {m:?}");
            }
            // A release the widgets never saw: the surface lagged the
            // pointer off the grip mid-drag. Settle now or the drag keeps
            // following a button that is no longer down. (When the grip
            // does see a release, GripReleased has already taken the drag
            // by the time this raw event arrives.)
            settle_drag(state);
            Task::none()
        }
        Message::Ice(Event::Mouse(mouse::Event::CursorLeft)) => {
            // Fast drags can outrun the surface; a pointer that left mid-drag
            // will not deliver a release, so settle up now.
            settle_drag(state);
            Task::none()
        }
        Message::Ice(_) => Task::none(),
        Message::GripPressed => {
            if debug() {
                eprintln!(
                    "overlay: [{:>8.1}ms] grip pressed at {}",
                    state.started.elapsed().as_secs_f64() * 1000.0,
                    state.cursor
                );
            }
            state.drag = Some(match global_grab(state) {
                Some((grab, mon)) => Drag::Global {
                    grab,
                    base: state.shown_offset,
                    mon,
                    moved: false,
                },
                None => Drag::Local {
                    grab: state.cursor,
                    moved: false,
                },
            });
            Task::none()
        }
        Message::GripReleased => match state.drag.as_ref() {
            Some(d) if d.moved() => {
                settle_drag(state);
                Task::none()
            }
            Some(_) => {
                state.drag = None;
                toggle(state)
            }
            // The raw-release fallback already settled this drag.
            None => Task::none(),
        },
        Message::CycleView => {
            let next = match state.app.view {
                View::Damage => View::Healing,
                View::Healing => View::Interrupts,
                View::Interrupts => View::CrowdControl,
                View::CrowdControl => View::Dispels,
                View::Dispels => View::Deaths,
                View::Deaths => View::Damage,
            };
            for req in state.app.apply(Action::SetView(next)) {
                state.client.send(&req);
            }
            Task::none()
        }
        Message::PrevBlock => {
            nav_block(state, -1);
            Task::none()
        }
        Message::NextBlock => {
            nav_block(state, 1);
            Task::none()
        }
        Message::TimelineGoto(pos) => {
            let reqs = state.app.goto_list_pos(pos);
            if debug() {
                eprintln!("overlay: timeline goto {pos} ({} reqs)", reqs.len());
            }
            send_all(state, reqs);
            Task::none()
        }
        Message::GoLive => {
            let reqs = state.app.pin_live();
            send_all(state, reqs);
            Task::none()
        }
        Message::ToggleSplit => {
            state.split = !state.split;
            state.cfg.overlay_split = state.split;
            state.cfg.save();
            sync_aux(state);
            Task::none()
        }
        Message::RowClicked(i) => {
            state.app.row_sel = i;
            for req in state.app.apply(Action::Open) {
                state.client.send(&req);
            }
            Task::none()
        }
        // UI scaling: 5% per wheel notch, surface and content together (the
        // panel's width/height scale with the zoom so proportions hold), and
        // the result persists like a settled drag does.
        Message::Zoom(n) => {
            let old = state.cfg.zoom;
            let new = (old + 0.05 * n).clamp(0.6, 2.5);
            if (new - old).abs() < 0.001 {
                return Task::none();
            }
            let ratio = new / old;
            state.cfg.zoom = new;
            state.cfg.width = ((state.cfg.width as f32 * ratio).round() as u32).max(160);
            state.cfg.height = ((state.cfg.height as f32 * ratio).round() as u32).max(180);
            state.cfg.save();
            if debug() {
                eprintln!(
                    "overlay: [{:>8.1}ms] zoom {old:.2} -> {new:.2} ({}x{})",
                    state.started.elapsed().as_secs_f64() * 1000.0,
                    state.cfg.width,
                    state.cfg.height
                );
            }
            // Same re-fit as toggle(): the grown panel may need to slide to
            // stay fully on the monitor.
            let size = current_size(state);
            state.shown_offset = state.cfg.offset;
            if state.expanded
                && let Some(max) = max_offset(state, size)
            {
                state.shown_offset = state.cfg.offset.min(max);
            }
            Task::batch([
                Task::done(Message::AnchorSizeChange(anchor_for(state.cfg.edge), size)),
                Task::done(Message::MarginChange(margin_for(
                    state.cfg.edge,
                    state.shown_offset,
                ))),
            ])
        }
        // Layer-shell control messages generated by `to_layer_message` are
        // consumed by the runtime, never delivered back to us.
        _ => Task::none(),
    }
}

/// The overlay's drain: like `window::drain_client`, but `SetVisible`
/// commands from the daemon's supervisor are intercepted (they are a display
/// concern, not client state) and returned in order.
fn drain_overlay(state: &mut Overlay) -> Vec<bool> {
    let mut wishes = Vec::new();
    for msg in state.client.poll() {
        match msg {
            DaemonMsg::SetVisible(v) => wishes.push(v),
            msg => {
                if matches!(
                    msg,
                    DaemonMsg::Snapshot { .. } | DaemonMsg::SegmentList { .. }
                ) {
                    state.last_snapshot_at = Some(Instant::now());
                }
                let opened = matches!(msg, DaemonMsg::SegmentOpened { .. });
                for req in state.app.on_msg(msg) {
                    state.client.send(&req);
                }
                // A new pull always brings the meter home to Live: scrubbing
                // history is between-pulls inspection, and the overlay is a
                // live meter first. The one deliberate parking spot that
                // stays put is the live visit's Σ overall — that *is* a live
                // meter of its own.
                if opened && !state.app.following_live() {
                    let parked_on_live_overall = watched_pos(&state.app)
                        .and_then(|p| state.app.entries().get(p))
                        .is_some_and(|e| e.row.kind == SegmentKind::Overall && e.row.live);
                    if !parked_on_live_overall {
                        let reqs = state.app.pin_live();
                        if debug() && !reqs.is_empty() {
                            eprintln!("overlay: new pull — snapping back to live");
                        }
                        for req in reqs {
                            state.client.send(&req);
                        }
                    }
                }
            }
        }
    }
    if state.client.is_dead() {
        state.app.status = Some("daemon gone — reconnecting…".to_string());
        if state.client.reconnect_if_dead() {
            state.app.status = None;
            state.client.send(&state.app.initial_request());
        }
    }
    wishes
}

/// Hide or restore the surface to match `game_visible`. Layer-shell has no
/// unmap, so hiding means a 1×1 surface that renders nothing (see [`view`])
/// plus an emptied input region so the leftover pixel is click-through.
/// Restoring re-asserts the real size and an input region larger than any
/// surface — the compositor clips it, so it means "the whole surface" no
/// matter how the overlay is later resized or toggled. The runtime clears
/// the current surface extent from the region before each callback, which
/// makes both transitions order-independent within the batch.
fn apply_visibility(state: &Overlay) -> Task<Message> {
    let anchor = anchor_for(state.cfg.edge);
    if state.game_visible && state.daemon_visible {
        let size = if state.expanded {
            (state.cfg.width, state.cfg.height)
        } else {
            tab_size(state.cfg.edge, state.cfg.zoom)
        };
        Task::batch([
            Task::done(Message::AnchorSizeChange(anchor, size)),
            Task::done(Message::SetInputRegion(ActionCallback::new(|region| {
                region.add(0, 0, 1 << 24, 1 << 24);
            }))),
        ])
    } else {
        Task::batch([
            Task::done(Message::SetInputRegion(ActionCallback::new(|_| {}))),
            Task::done(Message::AnchorSizeChange(anchor, (1, 1))),
        ])
    }
}

/// End any in-flight drag. A drag that actually moved is a deliberate
/// placement: it becomes the new durable anchor (edge included — perimeter
/// drags may have reoriented it).
fn settle_drag(state: &mut Overlay) {
    if state.drag.take().is_some_and(|d| d.moved()) {
        if debug() {
            eprintln!(
                "overlay: [{:>8.1}ms] drag settled at offset={}",
                state.started.elapsed().as_secs_f64() * 1000.0,
                state.shown_offset
            );
        }
        state.cfg.offset = state.shown_offset;
        state.cfg.save();
    }
}

/// Global pointer plus the rect of the monitor it is on, when Hyprland is
/// there to ask. `None` selects the surface-local fallback drag.
fn global_grab(state: &Overlay) -> Option<((f32, f32), (i32, i32, i32, i32))> {
    let dir = state.hypr_dir.as_deref()?;
    let (x, y) = hypr::cursor_pos(dir)?;
    let mon = hypr::monitor_at(dir, (x, y))?;
    Some(((x as f32, y as f32), mon))
}

/// The largest offset that keeps a surface of `size` fully on the monitor
/// under the cursor. `None` when Hyprland cannot be asked.
fn max_offset(state: &Overlay, size: (u32, u32)) -> Option<i32> {
    let dir = state.hypr_dir.as_deref()?;
    let mon = hypr::monitor_at(dir, hypr::cursor_pos(dir)?)?;
    let (_, len, span) = drag_axis(state.cfg.edge, mon, size);
    Some((len - span).max(0))
}

/// Monitor origin, monitor length, and surface span along `edge`'s
/// drag axis.
fn drag_axis(edge: Edge, mon: (i32, i32, i32, i32), size: (u32, u32)) -> (i32, i32, i32) {
    let (mx, my, mw, mh) = mon;
    if edge.is_vertical() {
        (my, mh, size.1 as i32)
    } else {
        (mx, mw, size.0 as i32)
    }
}

/// The monitor edge nearest the pointer, when it is close enough to
/// capture the tab and beats the current edge by enough to be worth
/// flipping to. The near-edge gate keeps mid-screen drags from flailing
/// between two far-but-equidistant edges (dead center, every edge ties);
/// the hysteresis keeps corners from flickering.
fn nearest_edge(current: Edge, p: (f32, f32), mon: (i32, i32, i32, i32)) -> Option<Edge> {
    const NEAR: f32 = 150.0;
    const HYSTERESIS: f32 = 24.0;
    let (mx, my, mw, mh) = mon;
    let distances = [
        (Edge::Left, p.0 - mx as f32),
        (Edge::Right, (mx + mw - 1) as f32 - p.0),
        (Edge::Top, p.1 - my as f32),
        (Edge::Bottom, (my + mh - 1) as f32 - p.1),
    ];
    let to_current = distances.iter().find(|(e, _)| *e == current)?.1;
    let (best, to_best) = distances.into_iter().min_by(|a, b| a.1.total_cmp(&b.1))?;
    (best != current && to_best < NEAR && to_best + HYSTERESIS < to_current).then_some(best)
}

/// Advance an in-flight drag for a new pointer sample. The drag is taken
/// out of `state` for the duration so the two can be borrowed freely.
fn drag_motion(state: &mut Overlay) -> Task<Message> {
    let Some(mut drag) = state.drag.take() else {
        return Task::none();
    };
    let task = advance_drag(state, &mut drag);
    state.drag = Some(drag);
    task
}

fn advance_drag(state: &mut Overlay, drag: &mut Drag) -> Task<Message> {
    let (grab, base, mon, moved) = match drag {
        Drag::Local { grab, moved } => {
            // Surface-local chase: the event's distance from the grab point
            // is how far the surface still has to move. Once it catches up,
            // the local position returns to the grab point.
            let delta = state.cursor - *grab;
            if delta.abs() > DRAG_THRESHOLD {
                *moved = true;
            }
            if *moved && delta as i32 != 0 {
                state.shown_offset = (state.shown_offset + delta as i32).max(0);
                return Task::done(Message::MarginChange(margin_for(
                    state.cfg.edge,
                    state.shown_offset,
                )));
            }
            return Task::none();
        }
        Drag::Global {
            grab,
            base,
            mon,
            moved,
        } => (grab, base, mon, moved),
    };
    let Some(dir) = state.hypr_dir.as_deref() else {
        return Task::none();
    };
    let Some((gx, gy)) = hypr::cursor_pos(dir) else {
        return Task::none();
    };
    // The pointer sample is clamped to the grabbed monitor, so neither the
    // reorientation nor the slide can ever leave the display.
    let (mx, my, mw, mh) = *mon;
    let p = (
        (gx.clamp(mx, mx + mw - 1)) as f32,
        (gy.clamp(my, my + mh - 1)) as f32,
    );
    if !*moved {
        let (dx, dy) = (p.0 - grab.0, p.1 - grab.1);
        if (dx * dx + dy * dy).sqrt() > DRAG_THRESHOLD {
            *moved = true;
        }
    }
    if !*moved {
        return Task::none();
    }
    // Tab drags roam the whole perimeter: crossing toward another edge
    // reorients the tab onto it, centered under the cursor, and re-seeds
    // the grab so sliding continues in the new axis.
    if !state.expanded
        && let Some(edge) = nearest_edge(state.cfg.edge, p, *mon)
    {
        state.cfg.edge = edge;
        let size = tab_size(edge, state.cfg.zoom);
        let (origin, len, span) = drag_axis(edge, *mon, size);
        let along = if edge.is_vertical() { p.1 } else { p.0 } - origin as f32;
        let offset = (along as i32 - span / 2).clamp(0, (len - span).max(0));
        state.shown_offset = offset;
        *grab = p;
        *base = offset;
        if debug() {
            eprintln!(
                "overlay: [{:>8.1}ms] drag reoriented to {edge:?} at offset={offset}",
                state.started.elapsed().as_secs_f64() * 1000.0
            );
        }
        return Task::batch([
            Task::done(Message::AnchorSizeChange(anchor_for(edge), size)),
            Task::done(Message::MarginChange(margin_for(edge, offset))),
        ]);
    }
    // Slide along the current edge, clamped to the monitor.
    let edge = state.cfg.edge;
    let (_, len, span) = drag_axis(edge, *mon, current_size(state));
    let delta = if edge.is_vertical() {
        p.1 - grab.1
    } else {
        p.0 - grab.0
    };
    let offset = (*base + delta as i32).clamp(0, (len - span).max(0));
    if offset == state.shown_offset {
        return Task::none();
    }
    state.shown_offset = offset;
    Task::done(Message::MarginChange(margin_for(edge, offset)))
}

// ---- instance navigation ----------------------------------------------------

/// Rows the split view's Σ section asks for.
const AUX_TOP_N: u32 = 8;

/// The watched segment's position in the entries table, clamped to it.
fn watched_pos(app: &ClientState) -> Option<usize> {
    let len = app.entries().len();
    (len > 0).then(|| app.segment_index().min(len - 1))
}

fn send_all(state: &mut Overlay, reqs: Vec<ClientMsg>) {
    for req in reqs {
        state.client.send(&req);
    }
}

/// ◀ ▶: step whole blocks. Landing on the newest block while it is still
/// accumulating re-pins Live (the meter comes home); any other block lands
/// on its anchor — the Σ summary for instances, the segment itself for
/// stray fights.
fn nav_block(state: &mut Overlay, delta: isize) {
    let target = {
        let entries = state.app.entries();
        let blocks = timeline::blocks(entries);
        let pos = watched_pos(&state.app);
        let cur = pos.and_then(|p| timeline::block_of(&blocks, p));
        match cur.and_then(|c| c.checked_add_signed(delta)) {
            Some(t) if t < blocks.len() => {
                let live_last = t + 1 == blocks.len() && timeline::is_live(&blocks[t], entries);
                blocks[t].anchor().map(|a| (a, live_last))
            }
            _ => None,
        }
    };
    let Some((anchor, live_last)) = target else {
        return;
    };
    let reqs = if live_last {
        state.app.pin_live()
    } else {
        state.app.goto_list_pos(anchor)
    };
    if debug() {
        eprintln!(
            "overlay: nav block {delta:+} -> anchor {anchor} live={live_last} ({} reqs)",
            reqs.len()
        );
    }
    send_all(state, reqs);
}

/// Keep the split view's second connection watching the current block's Σ
/// overall in the current view — or idle when split is off, the watched
/// block has no Σ, or the Σ itself is being watched (nothing to duplicate).
fn sync_aux(state: &mut Overlay) {
    let want = if state.split && state.expanded {
        let entries = state.app.entries();
        let blocks = timeline::blocks(entries);
        watched_pos(&state.app)
            .and_then(|pos| timeline::block_of(&blocks, pos).map(|bi| (pos, bi)))
            .and_then(|(pos, bi)| {
                let block = &blocks[bi];
                block
                    .overall
                    .filter(|&o| block.is_instance() && o != pos)
                    .and_then(|o| entries.get(o))
                    .map(|e| (e.id, state.app.view))
            })
    } else {
        None
    };
    let Some((id, view)) = want else {
        if state.aux_watch.take().is_some()
            && let Some(c) = state.aux.as_mut()
        {
            c.send(&ClientMsg::Watch(Cursor::List));
        }
        state.aux_rows.clear();
        state.aux_info = None;
        if let Some(c) = state.aux.as_mut() {
            c.poll();
        }
        return;
    };
    if state.aux.is_none() {
        // Never race the main client's reconnect: with the daemon gone, an
        // aux connect per tick would respawn daemons in a loop.
        if state.client.is_dead() {
            return;
        }
        state.aux = match crate::window::connect_as(ClientKind::Window) {
            Ok(c) => Some(c),
            Err(e) => {
                eprintln!("wowdps-gui: split view unavailable: {e}");
                state.split = false;
                return;
            }
        };
    }
    let Some(client) = state.aux.as_mut() else {
        return;
    };
    if client.is_dead() && client.reconnect_if_dead() {
        state.aux_watch = None;
    }
    if state.aux_watch != Some((id, view)) {
        client.send(&ClientMsg::Watch(Cursor::Segment {
            segment: SegmentRef::Id(id),
            view,
            top_n: Some(AUX_TOP_N),
            drill: None,
        }));
        state.aux_watch = Some((id, view));
        state.aux_rows.clear();
        state.aux_info = None;
    }
    for msg in client.poll() {
        if let DaemonMsg::Snapshot {
            segment: SegmentRef::Id(sid),
            view: v,
            info,
            rows,
            ..
        } = msg
            && state.aux_watch == Some((sid, v))
        {
            state.aux_info = Some(info);
            state.aux_rows = rows;
        }
    }
}

/// Header badge for an instance visit's Σ row: its outcome once known (R10
/// wording), else LIVE while the visit is in progress. A keyed visit's
/// badge carries the tier and overtime detail ("TIMED +2", "OVER +0:26",
/// live pace "LIVE +3"), judged at `clock_ms` — the clock shown beside it.
fn overall_tag(row: &ListRow, clock_ms: i64) -> (String, Color) {
    // A known outcome beats "still inside": a timed key is TIMED even while
    // the party finishes trash before zoning out.
    match (row.success, row.pars_ms) {
        (success @ Some(timed), Some(pars)) => (
            wowdps_model::fmt::key_tag(clock_ms, pars, success),
            if timed { GREEN } else { RED },
        ),
        (Some(true), None) => ("TIMED".into(), GREEN),
        (Some(false), None) => ("OVER".into(), RED),
        (None, pars) if row.live => (
            match pars {
                Some(p) => format!("LIVE {}", wowdps_model::fmt::key_tag(clock_ms, p, None)),
                None => "LIVE".into(),
            },
            YELLOW,
        ),
        (None, _) => (String::new(), DIM),
    }
}

/// The instance clock for the header, best source first: the snapshot when
/// the Σ itself is watched, the aux connection's snapshot when split has
/// one, else the Σ list row's clock at the last broadcast advanced by
/// however much the watched live member has grown since.
fn instance_elapsed(state: &Overlay, block: &timeline::Block, overall: usize) -> i64 {
    let app = &state.app;
    let entries = app.entries();
    let pos = watched_pos(app);
    if pos == Some(overall) {
        return app.duration_ms();
    }
    if let (Some(info), Some((id, _))) = (state.aux_info.as_ref(), state.aux_watch)
        && entries.get(overall).is_some_and(|e| e.id == id)
    {
        return info.duration_ms;
    }
    let base = entries.get(overall).map_or(0, |e| e.row.duration_ms);
    // A resolved key's clock is frozen at the official time — combat after
    // the END (looting heals, a leftover pack) must not advance it.
    if entries
        .get(overall)
        .is_some_and(|e| e.row.success.is_some())
    {
        return base;
    }
    let grown = pos
        .filter(|&p| block.contains(p))
        .and_then(|p| entries.get(p))
        .filter(|e| e.row.live)
        .map_or(0, |e| (app.duration_ms() - e.row.duration_ms).max(0));
    base + grown
}

// ---- geometry ---------------------------------------------------------------

/// Anchor to the corner the offset measures from: the top of side edges, the
/// left of horizontal ones.
fn anchor_for(edge: Edge) -> Anchor {
    match edge {
        Edge::Left => Anchor::Left | Anchor::Top,
        Edge::Right => Anchor::Right | Anchor::Top,
        Edge::Top => Anchor::Top | Anchor::Left,
        Edge::Bottom => Anchor::Bottom | Anchor::Left,
    }
}

/// (top, right, bottom, left) — `layershellev::set_margin` order.
fn margin_for(edge: Edge, offset: i32) -> (i32, i32, i32, i32) {
    if edge.is_vertical() {
        (offset, 0, 0, 0)
    } else {
        (0, 0, 0, offset)
    }
}

/// Tab surface size, scaled with the zoom so its glyphs never outgrow it.
fn tab_size(edge: Edge, zoom: f32) -> (u32, u32) {
    let thickness = (TAB_THICKNESS as f32 * zoom).round() as u32;
    let length = (TAB_LENGTH as f32 * zoom).round() as u32;
    if edge.is_vertical() {
        (thickness, length)
    } else {
        (length, thickness)
    }
}

// ---- rendering --------------------------------------------------------------

fn view(state: &Overlay) -> Element<'_, Message> {
    if !(state.game_visible && state.daemon_visible) {
        // Hidden: the surface is 1×1 and click-through; draw nothing so not
        // even a panel-background pixel shows.
        Space::new().into()
    } else if state.expanded {
        panel(state)
    } else {
        tab(state)
    }
}

/// The collapsed tab: a slim grip with a combat indicator.
fn tab(state: &Overlay) -> Element<'static, Message> {
    let z = state.cfg.zoom;
    let live = state.app.is_live();
    let dot = text("●")
        .size(10.0 * z)
        .color(if live { YELLOW } else { DIM });
    let letter = |c: &'static str| text(c).size(11.0 * z);
    let label: Element<'static, Message> = if state.cfg.edge.is_vertical() {
        column![dot, letter("d"), letter("p"), letter("s")]
            .spacing(2)
            .align_x(iced::Alignment::Center)
            .into()
    } else {
        row![dot, letter("dps")]
            .spacing(4)
            .align_y(iced::Alignment::Center)
            .into()
    };
    mouse_area(
        container(label)
            .center(Length::Fill)
            .style(|_: &Theme| panel_style(0.85)),
    )
    .on_press(Message::GripPressed)
    .on_release(Message::GripReleased)
    .on_scroll(|d| Message::Zoom(notches(d)))
    .into()
}

/// The expanded panel: header grip, instance timeline, live meter rows,
/// view switcher.
///
/// Inside an instance visit the frame anchors on the *instance*: the header
/// wears the visit's name/outcome/clock, a Σ–①─②─③–⚑ strip maps its bosses
/// and trash gaps, and the chip line under it names the member actually
/// being watched. Scrubbing members never changes the frame — only the chip
/// and the rows.
fn panel(state: &Overlay) -> Element<'_, Message> {
    let app = &state.app;
    let z = state.cfg.zoom;
    let entries = app.entries();
    let blocks = timeline::blocks(entries);
    let pos = watched_pos(app);
    let cur_block = pos.and_then(|p| timeline::block_of(&blocks, p));
    let instance = cur_block.filter(|&bi| blocks[bi].is_instance());

    let (head_name, (head_tag, head_tag_color), head_dur) = match instance {
        Some(bi) => {
            let o = blocks[bi].overall.expect("is_instance checked");
            let row = &entries[o].row;
            let elapsed = instance_elapsed(state, &blocks[bi], o);
            (row.name.clone(), overall_tag(row, elapsed), elapsed)
        }
        None => {
            let (tag, color) = crate::view::header_tag(app);
            (
                app.segment_name()
                    .unwrap_or_else(|| "waiting for combat…".to_string()),
                (tag.to_string(), color),
                app.duration_ms(),
            )
        }
    };

    let header = mouse_area(
        container(
            row![
                text(head_name).size(13.0 * z),
                text(head_tag).size(10.0 * z).color(head_tag_color),
                Space::new().width(Length::Fill),
                text(duration(head_dur)).size(12.0 * z),
            ]
            .spacing(6)
            .align_y(iced::Alignment::Center),
        )
        .padding([4, 8])
        .width(Length::Fill),
    )
    .on_press(Message::GripPressed)
    .on_release(Message::GripReleased)
    .on_scroll(|d| Message::Zoom(notches(d)));

    let show_split = state.split
        && app.drill.is_none()
        && instance.is_some_and(|bi| blocks[bi].overall != pos)
        && !state.aux_rows.is_empty();

    let mut list = column![].spacing(2);
    if let Some(drill) = app.drill.as_ref() {
        // Drilled into one player: their spells, with hit count and crit
        // rate. A caption line shares the drill rows' column widths so the
        // numbers sit under their headings.
        let who = drill
            .label
            .split('-')
            .next()
            .unwrap_or(&drill.label)
            .to_string();
        let caption = |s: &'static str, width: f32| {
            text(s)
                .size(9.0 * z)
                .color(DIM)
                .font(iced::Font::MONOSPACE)
                .width(Length::Fixed(width * z))
                .align_x(iced::Alignment::End)
        };
        // Deaths drill into the recap timeline (R9), other views by spell.
        // Count views can't crit and total == count: one column says it all.
        let recap = app.view == View::Deaths;
        let count_only = matches!(
            app.view,
            View::Interrupts | View::CrowdControl | View::Dispels
        );
        let (w_hits, w_crit, w_total) = OVERLAY_DRILL_COLS;
        let mut captions = row![
            text(who).size(11.0 * z).color(YELLOW),
            Space::new().width(Length::Fill),
        ]
        .spacing(4)
        .padding([0, 8])
        .align_y(iced::Alignment::Center);
        if recap {
            captions = captions
                .push(caption("amount", 52.0))
                .push(caption("hp", 40.0));
        } else if count_only {
            captions = captions.push(caption("count", w_total));
        } else {
            captions = captions
                .push(caption("hits", w_hits))
                .push(caption("crit", w_crit))
                .push(caption("total", w_total));
        }
        list = list.push(captions);
        let (by_spell, _) = app.breakdown();
        if by_spell.is_empty() {
            list = list.push(text("no data yet").size(12.0 * z).color(DIM));
        }
        let max = by_spell.iter().map(|r| r.amount).max().unwrap_or(1);
        for r in &by_spell {
            list = list.push(if recap {
                recap_row(r, max, 20.0 * z, z, true)
            } else {
                overlay_drill_row(r, max, 20.0 * z, z, count_only)
            });
        }
    } else {
        let rows = app.rows();
        if rows.is_empty() {
            list = list.push(text("no data yet").size(12.0 * z).color(DIM));
        }
        let max = rows.first().map_or(1, |r| r.amount);
        for (i, r) in rows.iter().enumerate() {
            list = list.push(
                mouse_area(overlay_row(r, max, 20.0 * z, z)).on_press(Message::RowClicked(i)),
            );
        }
        // Σ split: the visit's overall appended under the current fight's
        // rows, fed by the aux connection watching the Σ row's id.
        if show_split {
            let odur = state.aux_info.as_ref().map_or(0, |i| i.duration_ms);
            list = list.push(
                container(
                    row![
                        text("Σ overall").size(10.0 * z).color(YELLOW),
                        Space::new().width(Length::Fill),
                        text(duration(odur))
                            .size(10.0 * z)
                            .color(DIM)
                            .font(iced::Font::MONOSPACE),
                    ]
                    .align_y(iced::Alignment::Center),
                )
                .padding([3, 8]),
            );
            let omax = state.aux_rows.first().map_or(1, |r| r.amount);
            for r in &state.aux_rows {
                list = list.push(overlay_row(r, omax, 20.0 * z, z));
            }
        }
    }

    // Left: view switcher (+ Σ split toggle inside an instance). Center:
    // block navigation — whole visits, never a visit's members. Right:
    // live-return, hints, warnings. The side clusters take equal fill so
    // the arrows sit dead center.
    let mut left = row![
        mouse_area(text(view_name(app.view)).size(11.0 * z).color(DIM))
            .on_press(Message::CycleView),
    ]
    .spacing(8);
    if instance.is_some() {
        left = left.push(
            mouse_area(
                text("Σ")
                    .size(11.0 * z)
                    .color(if state.split { YELLOW } else { DIM }),
            )
            .on_press(Message::ToggleSplit),
        );
    }

    let bcount = blocks.len();
    let bpos = cur_block.unwrap_or(bcount.saturating_sub(1));
    // Generous padding: the glyph alone is a hopeless mid-fight click target.
    let arrow = |glyph: &'static str, enabled: bool, msg: Message| {
        let t = text(glyph)
            .size(11.0 * z)
            .color(if enabled { Color::WHITE } else { DIM });
        let area = mouse_area(container(t).padding([4, 12]));
        if enabled { area.on_press(msg) } else { area }
    };
    let nav = row![
        arrow("◀", bpos > 0, Message::PrevBlock),
        text(format!("{}/{}", bpos + 1, bcount.max(1)))
            .size(10.0 * z)
            .color(DIM)
            .font(iced::Font::MONOSPACE),
        arrow("▶", bpos + 1 < bcount, Message::NextBlock),
    ]
    .align_y(iced::Alignment::Center);

    let mut right = row![].spacing(8);
    if !app.following_live() && app.segment_count() > 0 {
        right = right.push(
            mouse_area(text("⦿ live").size(10.0 * z).color(YELLOW)).on_press(Message::GoLive),
        );
    }
    if app.drill.is_some() {
        right = right.push(text("right-click: back").size(10.0 * z).color(DIM));
    }
    if let (true, Some(secs)) = (app.is_live(), stale_secs(state.last_snapshot_at)) {
        right = right.push(
            text(format!("no events for {secs}s"))
                .size(10.0 * z)
                .color(YELLOW),
        );
    }

    let status = row![
        container(left).width(Length::FillPortion(1)),
        nav,
        container(right)
            .width(Length::FillPortion(1))
            .align_x(iced::Alignment::End),
    ]
    .align_y(iced::Alignment::Center);

    let mut content = column![header].spacing(4);
    if let Some(bi) = instance {
        let items = timeline::items(&blocks[bi], entries);
        content = content
            .push(container(timeline::strip(&items, pos, z, Message::TimelineGoto)).padding([0, 8]))
            .push(chip(state, &blocks[bi], pos, z));
    }
    content = content
        .push(scrollable(list).height(Length::Fill))
        .push(status);

    container(content.padding(6).height(Length::Fill))
        .style(|_: &Theme| panel_style(0.92))
        .into()
}

/// The selection chip under the strip: ‹ › scrubbers over the visit's Σ +
/// members, the watched member's identity, and its own clock (the header
/// keeps the instance's).
fn chip(
    state: &Overlay,
    block: &timeline::Block,
    pos: Option<usize>,
    z: f32,
) -> Element<'static, Message> {
    let app = &state.app;
    let entries = app.entries();
    let prev = pos.and_then(|p| timeline::scrub(block, p, -1));
    let next = pos.and_then(|p| timeline::scrub(block, p, 1));
    // Real buttons, not bare glyphs: a padded, z-scaled hit box each, so
    // mid-fight clicks land — and so neither area ever reaches the label.
    let mini = |glyph: &'static str, target: Option<usize>| {
        let t = text(glyph)
            .size(13.0 * z)
            .color(if target.is_some() { Color::WHITE } else { DIM });
        let area = mouse_area(
            container(t)
                .center(Length::Shrink)
                .padding([3.0 * z, 8.0 * z]),
        );
        match target {
            Some(p) => area.on_press(Message::TimelineGoto(p)),
            None => area,
        }
    };
    let sel = pos.and_then(|p| entries.get(p)).map(|e| &e.row);
    let (sel_name, sel_color) = match sel {
        Some(r) if r.kind == SegmentKind::Overall => ("Σ overall".to_string(), YELLOW),
        Some(r) if r.kind == SegmentKind::Encounter => (r.name.clone(), Color::WHITE),
        Some(r) if !r.name.is_empty() => (r.name.clone(), DIM),
        Some(_) => ("trash".to_string(), DIM),
        None => (String::new(), DIM),
    };
    // Outcome only (KILL/WIPE, a resolved key's TIMED/OVER): while live the
    // header and the footer's ⦿ already say so — a LIVE badge here reads
    // like part of the name.
    let (tag, tag_color) = if app.is_live() {
        ("", DIM)
    } else {
        crate::view::header_tag(app)
    };
    row![
        mini("‹", prev),
        mini("›", next),
        Space::new().width(Length::Fixed(4.0 * z)),
        text(sel_name).size(11.0 * z).color(sel_color),
        text(tag).size(9.0 * z).color(tag_color),
        Space::new().width(Length::Fill),
        text(duration(app.duration_ms()))
            .size(11.0 * z)
            .color(DIM)
            .font(iced::Font::MONOSPACE),
    ]
    .spacing(6.0 * z)
    .padding([0, 8])
    .align_y(iced::Alignment::Center)
    .into()
}

fn panel_style(alpha: f32) -> iced::widget::container::Style {
    iced::widget::container::Style {
        background: Some(
            Color {
                a: alpha,
                ..Color::from_rgb8(0x16, 0x16, 0x1e)
            }
            .into(),
        ),
        border: iced::Border {
            color: Color::from_rgba(1.0, 1.0, 1.0, 0.15),
            width: 1.0,
            radius: 6.into(),
        },
        ..iced::widget::container::Style::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn geometry_follows_the_configured_edge() {
        assert_eq!(anchor_for(Edge::Right), Anchor::Right | Anchor::Top);
        assert_eq!(
            margin_for(Edge::Right, 250),
            (250, 0, 0, 0),
            "side edges offset from the top"
        );
        assert_eq!(tab_size(Edge::Right, 1.0), (TAB_THICKNESS, TAB_LENGTH));
        assert_eq!(
            tab_size(Edge::Right, 2.0),
            (TAB_THICKNESS * 2, TAB_LENGTH * 2)
        );

        assert_eq!(anchor_for(Edge::Bottom), Anchor::Bottom | Anchor::Left);
        assert_eq!(
            margin_for(Edge::Bottom, 250),
            (0, 0, 0, 250),
            "horizontal edges offset from the left"
        );
        assert_eq!(tab_size(Edge::Bottom, 1.0), (TAB_LENGTH, TAB_THICKNESS));
    }

    #[test]
    fn reorientation_needs_a_near_edge_and_a_clear_winner() {
        let mon = (0, 0, 3440, 1440);
        assert_eq!(
            nearest_edge(Edge::Right, (1720.0, 720.0), mon),
            None,
            "dead center: top/bottom are nearest but too far to capture"
        );
        assert_eq!(
            nearest_edge(Edge::Right, (1720.0, 100.0), mon),
            Some(Edge::Top),
            "near the top, far from the right: flip"
        );
        assert_eq!(
            nearest_edge(Edge::Top, (1720.0, 1339.0), mon),
            Some(Edge::Bottom)
        );
        assert_eq!(
            nearest_edge(Edge::Right, (3400.0, 1400.0), mon),
            None,
            "corner: bottom is equally near but not by the hysteresis margin"
        );
        assert_eq!(
            nearest_edge(Edge::Right, (3300.0, 1430.0), mon),
            Some(Edge::Bottom),
            "clearly past the corner diagonal: flip"
        );
    }
}
