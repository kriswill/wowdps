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

use iced::widget::{Space, checkbox, column, container, mouse_area, row, scrollable, stack, text};
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
/// One revolution of the staleness radar's hand.
const RADAR_PERIOD_SECS: f32 = 2.5;

/// Runs the overlay. The single-instance claim happens in `main`, before
/// this is called — the incumbent must be evicted before the daemon is
/// touched, and never later than the moment a second surface could appear.
pub fn run(cfg: Config) -> Result<(), String> {
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
            let handoff = first.lock().ok().and_then(|mut slot| slot.take());
            let client = handoff.unwrap_or_else(|| {
                crate::window::reconnect_forever(wowdps_proto::ClientKind::Overlay)
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
    /// R12/v12: the comparison marker label under the cursor, if any.
    compare_hover: Option<String>,
    /// The graph curve value under the cursor, for the legend's readout.
    graph_probe: Option<f64>,
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
    autodrill: u8,
    /// R12 debug aid: pick the top two rows once data arrives, for
    /// screenshotting the comparison the same way.
    autocompare: bool,
    autoseg: Option<usize>,
    /// Process start, for debug-trace timestamps.
    started: Instant,
    /// Footer Σ toggle: show the instance's Σ overall under the current
    /// fight's rows.
    split: bool,
    /// The footer ⚙ options card is open.
    options_open: bool,
    /// Fractional wheel notches over the timeline strip, carried until they
    /// add up to a whole scrub step (touchpads scroll in slivers).
    strip_acc: f32,
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
            compare_hover: None,
            graph_probe: None,
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
            autodrill: match std::env::var("WOWDPS_OVERLAY_AUTODRILL").ok().as_deref() {
                None => 0,
                Some("2") => 2,
                Some(_) => 1,
            },
            autocompare: std::env::var_os("WOWDPS_OVERLAY_AUTOCOMPARE").is_some(),
            autoseg: std::env::var("WOWDPS_OVERLAY_AUTOSEG")
                .ok()
                .and_then(|v| v.parse().ok()),
            started: Instant::now(),
            split,
            options_open: false,
            strip_acc: 0.0,
            aux: None,
            aux_watch: None,
            aux_rows: Vec::new(),
            aux_info: None,
        }
    }
}

#[cfg(test)]
impl Overlay {
    /// Test seam: an overlay over a caller-built state and connection —
    /// collapsed, untracked (no Hyprland, no debug aids, no env lookups).
    fn for_test(app: ClientState, client: DaemonClient, cfg: Config) -> Self {
        let shown_offset = cfg.offset;
        Self {
            app,
            compare_hover: None,
            graph_probe: None,
            client,
            last_snapshot_at: None,
            cfg,
            expanded: false,
            shown_offset,
            cursor: 0.0,
            drag: None,
            hypr: None,
            hypr_dir: None,
            game_visible: true,
            daemon_visible: true,
            autotoggle: 0,
            autodrill: 0,
            autocompare: false,
            autoseg: None,
            started: Instant::now(),
            split: false,
            options_open: false,
            strip_acc: 0.0,
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

/// R12: a comparison is two spell tables and two graphs; the meter panel's
/// width is one column of names. These are floors, not fixed sizes — a user
/// who has already dragged the panel bigger keeps their size.
const COMPARE_MIN: (u32, u32) = (620, 460);

/// Surface size for the current expanded/collapsed state.
fn current_size(state: &Overlay) -> (u32, u32) {
    if !state.expanded {
        return tab_size(state.cfg.edge, state.cfg.zoom);
    }
    let (w, h) = (state.cfg.width, state.cfg.height);
    if state.app.screen == Screen::Compare {
        let z = state.cfg.zoom;
        return (
            w.max((COMPARE_MIN.0 as f32 * z) as u32),
            h.max((COMPARE_MIN.1 as f32 * z) as u32),
        );
    }
    (w, h)
}

/// Re-anchor the surface at whatever `current_size` now says — the resize
/// half of `toggle`, for changes that alter the size without collapsing.
fn resize(state: &mut Overlay) -> Task<Message> {
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

use crate::view::scroll_clear;

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
    /// R12: a row's class icon was clicked — pick that player for the
    /// comparison, or unpick them.
    CompareRow(usize),
    /// R12: footer graph toggle — rolling DPS vs cumulative damage.
    ToggleGraph,
    /// R12: right-click on the body — drop the picked pair (or a lone
    /// half-pick) and return to the meter.
    ClearCompare,
    /// R12/v12: a drag on a comparison graph selected a time window (ms from
    /// segment start) — or a right-click on the graph asked for the whole
    /// fight back.
    CompareRange(Option<(u32, u32)>),
    /// R12/v12: the cursor entered (or left) a marker icon on a comparison
    /// graph; both graphs highlight every use of that item.
    CompareHover(Option<String>),
    /// v14: a drag on the drilldown's graph selected a zoom window (or a
    /// right-click asked for the whole fight back). Client-side only.
    DrillRange(Option<(u32, u32)>),
    /// The curve value under the cursor on any graph, for the legend's
    /// "dps: ###" readout. None when the pointer leaves.
    GraphProbe(Option<f64>),
    /// v16: a by-spell drill row was clicked — descend into that ability.
    SpellRow(usize),
    /// v18: a comparison spell row was clicked — drill BOTH sides into that
    /// ability (by-spell key, label).
    CompareSpell((String, String)),
    /// Wheel over the header (or the collapsed tab): scale the whole UI by
    /// this many notches — keyboard modifiers never reach the overlay
    /// (`KeyboardInteractivity::None`; the game keeps every keystroke), so
    /// the zoom gesture is placement-scoped instead of Ctrl-scoped.
    Zoom(f32),
    /// Radar frame clock: a no-op update whose only effect is a redraw.
    Animate,
    /// Footer trash can: ask the daemon to drop closed out-of-instance
    /// Trash from the list (R11).
    DiscardTrash,
    /// Wheel over the timeline strip: scrub through the visit's members —
    /// up toward older, down toward newer (notches, may accumulate).
    StripScroll(f32),
    /// Footer ⚙: open/close the overlay's options card.
    ToggleOptions,
    /// The pointer left the options card: dismiss it.
    CloseOptions,
    /// Options card: number meter rows by sort position.
    SetShowRanks(bool),
    /// Swallow presses on the options card's body so they don't fall
    /// through to the rows underneath.
    Noop,
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

fn subscription(state: &Overlay) -> Subscription<Message> {
    // The staleness radar animates at ~30fps, but only while it is actually
    // on screen — the rest of the time the 100ms Tick is the only clock.
    let radar_on = state.expanded
        && state.game_visible
        && state.daemon_visible
        && state.app.is_live()
        && stale_secs(state.last_snapshot_at).is_some();
    let animate = if radar_on {
        time::every(std::time::Duration::from_millis(33)).map(|_| Message::Animate)
    } else {
        Subscription::none()
    };
    Subscription::batch([
        animate,
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
            // Debug aid: WOWDPS_OVERLAY_AUTOSEG=<pos> parks the frame on
            // that combined-list position once the list arrives, so any
            // segment can be screenshotted without a pointer. The other
            // AUTO* aids hold their fire until it lands, then act on the
            // parked segment's rows.
            if let Some(pos) = state.autoseg
                && !state.app.entries().is_empty()
            {
                state.autoseg = None;
                let reqs = state.app.goto_list_pos(pos);
                send_all(state, reqs);
            }
            // Debug aid: WOWDPS_OVERLAY_AUTODRILL opens the top row's
            // drilldown as soon as there is one to open; `=2` descends once
            // more into the top ability when the by-spell rows arrive.
            if state.autoseg.is_none() && state.autodrill > 0 {
                let ready = if state.app.drill.is_none() {
                    state.app.row_sel = 0;
                    !state.app.rows().is_empty()
                } else if state.app.drill_spell().is_none() {
                    !state.app.breakdown().0.is_empty()
                } else {
                    state.autodrill = 0;
                    false
                };
                if ready {
                    state.autodrill -= 1;
                    for req in state.app.apply(Action::Open) {
                        state.client.send(&req);
                    }
                }
            }
            // R12 debug aid: WOWDPS_OVERLAY_AUTOCOMPARE picks the top two
            // players as soon as there are two, so the comparison can be
            // screenshotted on outputs nothing can click.
            if state.autoseg.is_none() && state.autocompare && state.app.rows().len() >= 2 {
                state.autocompare = false;
                // AUTOCOMPARE=1 picks two (the comparison screen);
                // AUTOCOMPARE=half picks one (the badged-but-waiting meter).
                let n = if std::env::var("WOWDPS_OVERLAY_AUTOCOMPARE").as_deref() == Ok("half") {
                    1
                } else {
                    2
                };
                let picks: Vec<(String, String)> = state
                    .app
                    .rows()
                    .iter()
                    .take(n)
                    .map(|r| (r.key.clone(), r.label.clone()))
                    .collect();
                for (key, label) in picks {
                    for req in state.app.toggle_compare(&key, &label) {
                        state.client.send(&req);
                    }
                }
                tasks.push(resize(state));
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
        // Wheel over the strip: whole notches become scrub steps (up = older,
        // down = newer), fractions carry over. Stepping recomputes the block
        // from the position just landed on, so fast spins stay in order.
        Message::StripScroll(n) => {
            state.strip_acc += n;
            let whole = state.strip_acc.trunc();
            state.strip_acc -= whole;
            let mut steps = whole as i32;
            let mut reqs = Vec::new();
            while steps != 0 {
                let delta: isize = if steps > 0 { -1 } else { 1 };
                let target = watched_pos(&state.app).and_then(|p| {
                    let entries = state.app.entries();
                    let blocks = timeline::blocks(entries);
                    let bi = timeline::block_of(&blocks, p)?;
                    timeline::scrub(blocks.get(bi)?, p, delta)
                });
                let Some(p) = target else { break };
                reqs.extend(state.app.goto_list_pos(p));
                steps -= steps.signum();
            }
            send_all(state, reqs);
            Task::none()
        }
        Message::ToggleOptions => {
            state.options_open = !state.options_open;
            Task::none()
        }
        Message::CloseOptions => {
            state.options_open = false;
            Task::none()
        }
        Message::SetShowRanks(on) => {
            state.cfg.show_ranks = on;
            state.cfg.save();
            Task::none()
        }
        Message::Noop => Task::none(),
        // R12: picking the second player opens the comparison, which needs a
        // bigger surface than the meter tab — resize the way `toggle` does.
        Message::CompareRow(i) => {
            state.app.row_sel = i;
            for req in state.app.apply(Action::PickCompare) {
                state.client.send(&req);
            }
            resize(state)
        }
        Message::ToggleGraph => {
            state.app.toggle_graph();
            Task::none()
        }
        // R12: leaving the comparison shrinks the surface back to the
        // meter's size, the inverse of the grow on the second pick.
        Message::ClearCompare => {
            for req in state.app.clear_compare() {
                state.client.send(&req);
            }
            resize(state)
        }
        Message::CompareRange(range) => {
            for req in state.app.set_compare_range(range) {
                state.client.send(&req);
            }
            Task::none()
        }
        Message::CompareHover(label) => {
            state.compare_hover = label;
            Task::none()
        }
        Message::DrillRange(range) => {
            state.app.set_drill_range(range);
            Task::none()
        }
        Message::GraphProbe(v) => {
            state.graph_probe = v;
            Task::none()
        }
        // v16: select the clicked spell row, then Open descends into it.
        Message::SpellRow(i) => {
            if let Some(d) = state.app.drill.as_mut() {
                d.spell_sel = i;
                d.pane = wowdps_model::Pane::Spell;
            }
            for req in state.app.apply(Action::Open) {
                state.client.send(&req);
            }
            Task::none()
        }
        Message::CompareSpell((key, label)) => {
            for req in state.app.drill_compare_spell(&key, &label) {
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
        Message::Animate => Task::none(),
        Message::DiscardTrash => {
            state.client.send(&ClientMsg::DiscardTrash);
            Task::none()
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
fn global_grab(state: &Overlay) -> Option<((f32, f32), hypr::MonitorRect)> {
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
fn drag_axis(edge: Edge, mon: hypr::MonitorRect, size: (u32, u32)) -> (i32, i32, i32) {
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
            Some(t) => blocks.get(t).and_then(|b| {
                let live_last = t + 1 == blocks.len() && timeline::is_live(b, entries);
                b.anchor().map(|a| (a, live_last))
            }),
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
                let block = blocks.get(bi)?;
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
            spell: None,
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
    let instance = cur_block
        .and_then(|bi| blocks.get(bi))
        .filter(|b| b.is_instance());

    // `is_instance` guarantees the Σ index; a missing entry for it can only
    // mean the list moved under us, and then the plain header is right.
    let instance_head = instance.and_then(|b| {
        let o = b.overall?;
        let row = &entries.get(o)?.row;
        let elapsed = instance_elapsed(state, b, o);
        Some((row.name.clone(), overall_tag(row, elapsed), elapsed))
    });

    let (head_name, (head_tag, head_tag_color), head_dur) = match instance_head {
        Some(head) => head,
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
        && instance.is_some_and(|b| b.overall != pos)
        && !state.aux_rows.is_empty();

    let mut list = column![].spacing(2);
    if let (Some(drill), Some((_, spell_label))) = (app.drill.as_ref(), app.drill_spell().cloned())
    {
        // v16: the ability drill — breadcrumb and stat strip; the graph
        // below carries the spell's own curve over the player's ghost.
        let spell_row = app.drill_spell_row();
        list = list.push(
            container(crate::view::spell_breadcrumb::<Message>(
                &drill.label,
                &spell_label,
                spell_row.as_ref(),
                z,
            ))
            .padding([2, 8]),
        );
        match &spell_row {
            Some(r) => {
                list = list.push(
                    container(crate::view::spell_stats::<Message>(r, app.view, z)).padding([4, 8]),
                )
            }
            None => list = list.push(text("no data yet").size(12.0 * z).color(DIM)),
        }
        // v17: who the ability landed on.
        let targets = app.spell_target_rows();
        if !targets.is_empty() {
            list = list.push(
                container(
                    row![
                        text("targets").size(10.0 * z).color(DIM),
                        Space::new().width(Length::Fill),
                        text("hits · total · %")
                            .size(9.0 * z)
                            .color(DIM)
                            .font(iced::Font::MONOSPACE),
                    ]
                    .align_y(iced::Alignment::Center),
                )
                .padding([2, 8]),
            );
            list = list.push(crate::view::spell_target_list::<Message>(
                &targets,
                20.0 * z,
                z,
            ));
        }
    } else if let Some(drill) = app.drill.as_ref() {
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
        for (i, r) in by_spell.iter().enumerate() {
            list = list.push(if recap {
                recap_row(r, max, 20.0 * z, z, true)
            } else {
                // v16: a spell row descends into its ability drill.
                mouse_area(overlay_drill_row(r, max, 20.0 * z, z, count_only))
                    .on_press(Message::SpellRow(i))
                    .into()
            });
        }
    } else {
        let rows = app.rows();
        if rows.is_empty() {
            list = list.push(text("no data yet").size(12.0 * z).color(DIM));
        }
        // R13: the enemy team sorts after the friendly one, so the biggest
        // bar is no longer necessarily the first row.
        let max = rows.iter().map(|r| r.amount).max().unwrap_or(1);
        let split = crate::view::enemy_split(&rows);
        for (i, r) in rows.iter().enumerate() {
            // R13: mark where the enemy team's block starts.
            if split == Some(i) {
                list = list.push(crate::view::team_divider(9.0 * z));
            }
            // R12: the class icon picks for comparison, the bar still
            // drills. Two hit areas, two questions.
            list = list.push(
                row![
                    mouse_area(crate::compare::class_icon(
                        r.class,
                        r.spec,
                        app.compare_slot(&r.key),
                        14.0 * z
                    ))
                    .on_press(Message::CompareRow(i)),
                    mouse_area(overlay_row(
                        r,
                        max,
                        20.0 * z,
                        z,
                        state.cfg.show_ranks.then_some(i + 1),
                    ))
                    .on_press(Message::RowClicked(i)),
                ]
                .spacing(4.0 * z)
                .align_y(iced::Alignment::Center),
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
            for (i, r) in state.aux_rows.iter().enumerate() {
                list = list.push(overlay_row(
                    r,
                    omax,
                    20.0 * z,
                    z,
                    state.cfg.show_ranks.then_some(i + 1),
                ));
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
        // R11: throw away closed out-of-instance trash; keys/raids and the
        // live segment survive.
        mouse_area(crate::gauge::trash(DIM, 11.0 * z)).on_press(Message::DiscardTrash),
    ]
    .spacing(8)
    .align_y(iced::Alignment::Center);
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
    // R12: while comparing, the graph mode replaces nothing — it is simply
    // the one control the comparison needs that the meter does not. v14: the
    // drilldown's own graph earns the same toggle.
    if app.screen == Screen::Compare || app.drill_timeline().is_some() {
        // The toggle words the curve it would show: "hps" when the drilled
        // view is Healing (the comparison is always damage).
        let label = match (app.graph_mode(), app.view) {
            (wowdps_model::GraphMode::Dps, View::Healing) if app.screen != Screen::Compare => "hps",
            (m, _) => m.label(),
        };
        left = left.push(
            mouse_area(text(label).size(11.0 * z).color(YELLOW)).on_press(Message::ToggleGraph),
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

    // Vector indicators (crate::gauge): drawn, not font glyphs, so they are
    // crisp at any zoom and sit on the row's centerline instead of some
    // fallback symbol font's baseline.
    let glyphed = |glyph: Element<'static, Message>, label: String, color: Color| {
        row![glyph, text(label).size(10.0 * z).color(color)]
            .spacing(3.0 * z)
            .align_y(iced::Alignment::Center)
    };
    let mut right = row![].spacing(8).align_y(iced::Alignment::Center);
    if !app.following_live() && app.segment_count() > 0 {
        right = right.push(
            mouse_area(glyphed(
                crate::gauge::dot(YELLOW, 7.0 * z),
                "live".to_string(),
                YELLOW,
            ))
            .on_press(Message::GoLive),
        );
    }
    // The game flushes the log in bursts: a quiet meter is usually an
    // unflushed buffer, so show how stale we are — a radar whose hand
    // sweeps while we wait, its trail fading behind it (the Animate
    // subscription runs only while this is on screen).
    if let (true, Some(secs)) = (app.is_live(), stale_secs(state.last_snapshot_at)) {
        let angle =
            (state.started.elapsed().as_secs_f32() / RADAR_PERIOD_SECS) * std::f32::consts::TAU;
        right = right.push(glyphed(
            crate::gauge::radar(angle, 13.0 * z, GREEN),
            format!("{secs}s"),
            DIM,
        ));
    }
    right = right.push(
        mouse_area(
            text("⚙")
                .size(12.0 * z)
                .color(if state.options_open { YELLOW } else { DIM }),
        )
        .on_press(Message::ToggleOptions),
    );

    let status = row![
        container(left).width(Length::FillPortion(1)),
        nav,
        container(right)
            .width(Length::FillPortion(1))
            .align_x(iced::Alignment::End),
    ]
    .align_y(iced::Alignment::Center);

    let mut content = column![header].spacing(4);
    if let Some(b) = instance {
        // Progression nights: interstitial wipe runs collapse into ×N chips
        // (the watched, live and newest attempts always stay visible); the
        // chip scrubbers below still step every hidden attempt.
        let items = timeline::collapse(timeline::items(b, entries), entries, pos);
        content = content
            .push(
                container(
                    // The wheel scrubs the visit's members — the fanned
                    // badges compress space, the wheel gives it back.
                    mouse_area(timeline::strip(
                        &items,
                        pos,
                        z,
                        // The panel's content padding (6+6) plus the strip
                        // container's own (8+8): what the strip may fill.
                        (state.cfg.width as f32 - 28.0).max(40.0),
                        Message::TimelineGoto,
                    ))
                    .on_scroll(|d| Message::StripScroll(notches(d))),
                )
                .padding([0, 8]),
            )
            .push(chip(state, b, pos, z));
    }
    // R12: the comparison replaces the rows outright — at panel width there
    // is no room to show both, and the meter is one click away.
    let body: Element<'_, Message> = if app.screen == Screen::Compare {
        // R12/v12: graph gestures — drag-select a window, hover a marker,
        // right-click zoom-out (captured by the canvas, so it never falls
        // through to the clear-compare area below).
        let ctl = crate::compare::GraphCtl {
            on_range: std::rc::Rc::new(Message::CompareRange),
            on_hover: std::rc::Rc::new(Message::CompareHover),
            hover: state.compare_hover.clone(),
            on_probe: std::rc::Rc::new(Message::GraphProbe),
            probe: state.graph_probe,
            on_spell: std::rc::Rc::new(Message::CompareSpell),
        };
        crate::compare::compare_body(app, z, 90.0 * z, false, ctl)
    } else if let Some(t) = app
        .drill_timeline()
        .filter(|t| !t.buckets.is_empty())
        .cloned()
    {
        // v14: the drilled player's timeline under their spell rows — the
        // comparison's graph for one side. The zoom is client-side (the
        // timeline arrives whole); right-click on the graph resets it, and
        // the canvas captures that press, so it never backs out of the drill.
        let class = app
            .drill
            .as_ref()
            .and_then(|d| app.rows().into_iter().find(|r| r.key == d.key))
            .and_then(|r| r.class);
        let ctl = crate::compare::GraphCtl {
            on_range: std::rc::Rc::new(Message::DrillRange),
            on_hover: std::rc::Rc::new(Message::CompareHover),
            hover: state.compare_hover.clone(),
            on_probe: std::rc::Rc::new(Message::GraphProbe),
            probe: state.graph_probe,
            on_spell: std::rc::Rc::new(Message::CompareSpell),
        };
        let rate = if app.view == View::Healing {
            "hps"
        } else {
            "dps"
        };
        // v16: the ability drill focuses its own curve, in its school color,
        // over the player's ghosted line. Same height as the player drill's
        // graph — a consistent chart leaves the room to the targets list.
        let spell_row = app.drill_spell_row();
        let focus_color = spell_row
            .as_ref()
            .and_then(|r| crate::view::school_color(r.school))
            .unwrap_or(YELLOW);
        let focus = app.spell_timeline().map(|ft| (ft, focus_color));
        column![
            scrollable(scroll_clear(list)).height(Length::Fill),
            crate::compare::drill_graph(app, &t, class, z, 64.0 * z, rate, false, focus, ctl),
        ]
        .spacing(4)
        .into()
    } else {
        scrollable(scroll_clear(list)).height(Length::Fill).into()
    };
    // R12: right-click anywhere on the body clears the comparison (or a lone
    // half-pick) and returns to the meter — the overlay has no keyboard
    // (`KeyboardInteractivity::None`), so this is its only way back. The
    // inner row areas only claim left presses, so the right press reaches us.
    content = content
        .push(mouse_area(body).on_right_press(Message::ClearCompare))
        .push(status);

    let root =
        container(content.padding(6).height(Length::Fill)).style(|_: &Theme| panel_style(0.92));
    if state.options_open {
        stack![root, options_card(&state.cfg, z)].into()
    } else {
        root.into()
    }
}

/// The overlay's own options card — laid out for a narrow panel glanced at
/// mid-fight, so it grows overlay-specific toggles independently of the
/// window's ⚙ panel. Anchored bottom-right, above the footer's ⚙; presses
/// on it never reach the rows beneath, and the pointer leaving dismisses it.
fn options_card(cfg: &Config, z: f32) -> Element<'static, Message> {
    let card = container(
        column![
            text("options").size(9.0 * z).color(DIM),
            checkbox(cfg.show_ranks)
                .label("row ranks")
                .on_toggle(Message::SetShowRanks)
                .size(12.0 * z)
                .text_size(11.0 * z),
        ]
        .spacing(6.0 * z),
    )
    .padding(8.0 * z)
    .style(|_: &Theme| container::Style {
        background: Some(Color::from_rgba(0.09, 0.10, 0.14, 0.97).into()),
        border: iced::Border {
            color: Color::from_rgba(1.0, 1.0, 1.0, 0.25),
            width: 1.0,
            radius: 4.into(),
        },
        ..container::Style::default()
    });
    container(
        mouse_area(card)
            .on_press(Message::Noop)
            .on_right_press(Message::Noop)
            .on_exit(Message::CloseOptions),
    )
    .width(Length::Fill)
    .height(Length::Fill)
    .align_x(iced::Alignment::End)
    .align_y(iced::Alignment::End)
    .padding([26.0 * z, 8.0 * z])
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

    use std::io::Write;
    use std::os::unix::net::UnixStream;
    use std::sync::mpsc;
    use std::time::Duration;

    use wowdps_daemon::mock::{MockDaemon, pump};
    use wowdps_model::{ListRow, Row, SegmentInfo};
    use wowdps_proto::{ListEntry, PROTO_VERSION, wire};

    use crate::hypr::fake::{FakeHypr, test_env};

    // ---- harness ------------------------------------------------------------

    /// The far end of the socketpair the overlay believes is the daemon:
    /// reads what the overlay sent, writes what the daemon would push.
    struct Peer(UnixStream);

    impl Peer {
        /// Every request the overlay has written so far.
        fn sent(&mut self) -> Vec<ClientMsg> {
            self.0
                .set_read_timeout(Some(Duration::from_millis(50)))
                .unwrap();
            let mut out = Vec::new();
            while let Ok((tag, body)) = wire::read_frame(&mut self.0) {
                out.push(ClientMsg::decode(tag, &body).unwrap());
            }
            out
        }

        /// The cursors of every `Watch` the overlay sent.
        fn watches(&mut self) -> Vec<Cursor> {
            self.sent()
                .into_iter()
                .filter_map(|m| match m {
                    ClientMsg::Watch(c) => Some(c),
                    _ => None,
                })
                .collect()
        }

        fn push(&mut self, msg: &DaemonMsg) {
            self.0.write_all(&msg.encode()).unwrap();
        }
    }

    /// A client over a socketpair whose peer answers the handshake and then
    /// stays connected (a dropped peer reads as "daemon gone").
    fn paired(kind: ClientKind) -> (DaemonClient, Peer) {
        let (ours, theirs) = UnixStream::pair().unwrap();
        let answer = std::thread::spawn(move || {
            let mut s = theirs;
            let (tag, _) = wire::read_frame(&mut s).unwrap();
            assert_eq!(tag, 0x01, "the client opens with Hello");
            s.write_all(
                &DaemonMsg::HelloAck {
                    proto: PROTO_VERSION,
                    version: "test".into(),
                }
                .encode(),
            )
            .unwrap();
            s
        });
        let client = DaemonClient::over(ours, kind).unwrap();
        (client, Peer(answer.join().unwrap()))
    }

    fn cfg() -> Config {
        test_env();
        Config {
            follow_game: false,
            ..Config::default()
        }
    }

    fn rig(app: ClientState) -> (Overlay, Peer) {
        let (client, peer) = paired(ClientKind::Overlay);
        (Overlay::for_test(app, client, cfg()), peer)
    }

    /// Hand what the overlay just sent to the mock daemon and feed its
    /// answers straight back into the state — the socket round-trip,
    /// synchronously, over the real engine and fixture.
    fn roundtrip(ov: &mut Overlay, peer: &mut Peer, mock: &mut MockDaemon) {
        let reqs = peer.sent();
        pump(&mut ov.app, mock, reqs);
    }

    /// Tick until `done` holds — pushes from the peer land on a reader
    /// thread, so the first tick after a push may not see them yet.
    fn tick_until(ov: &mut Overlay, mut done: impl FnMut(&Overlay) -> bool) {
        for _ in 0..400 {
            drop(update(ov, Message::Tick));
            if done(ov) {
                return;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        panic!("the expected effect never landed");
    }

    // ---- fixture states (mirroring the TUI's) --------------------------------

    fn apply(state: &mut ClientState, mock: &mut MockDaemon, action: Action) {
        let reqs = state.apply(action);
        pump(state, mock, reqs);
    }

    /// Indexed startup over the whole fixture: the list screen.
    fn indexed() -> (ClientState, MockDaemon) {
        let mut mock = MockDaemon::fixture();
        let mut state = ClientState::new();
        let first = state.initial_request();
        pump(&mut state, &mut mock, vec![first]);
        (state, mock)
    }

    /// The meter on the newest segment: the final wipe.
    fn newest() -> (ClientState, MockDaemon) {
        let (mut state, mut mock) = indexed();
        apply(&mut state, &mut mock, Action::Open);
        (state, mock)
    }

    /// The fixture's boss kill, with the richest data.
    fn kill() -> (ClientState, MockDaemon) {
        let (mut state, mut mock) = newest();
        apply(&mut state, &mut mock, Action::OlderSegment);
        apply(&mut state, &mut mock, Action::OlderSegment);
        assert_eq!(state.segment_name().as_deref(), Some("The Ashen Warden"));
        (state, mock)
    }

    /// Mid-fight arrival: the live meter.
    fn live() -> (ClientState, MockDaemon) {
        let mut mock = MockDaemon::fixture_live();
        let mut state = ClientState::new();
        let first = state.initial_request();
        pump(&mut state, &mut mock, vec![first]);
        (state, mock)
    }

    // ---- synthetic lists (instance visits) ----------------------------------

    fn entry(id: u64, kind: SegmentKind, instance: Option<u32>, live: bool) -> ListEntry {
        ListEntry {
            id: SegmentId(id),
            row: ListRow {
                kind,
                name: match kind {
                    SegmentKind::Encounter => format!("Boss {id}"),
                    SegmentKind::Overall => format!("Visit {id}"),
                    _ => String::new(),
                },
                start_ms: id as i64 * 100_000,
                success: None,
                duration_ms: 10_000 * (id as i64 + 1),
                live,
                instance,
                pars_ms: None,
                arena: false,
                encounter: None,
            },
        }
    }

    /// City trash, a finished visit (Σ + boss + trash), then a live visit
    /// (Σ + one live boss): three blocks.
    fn visits() -> Vec<ListEntry> {
        vec![
            entry(0, SegmentKind::Trash, None, false),
            entry(1, SegmentKind::Overall, Some(0), false),
            entry(2, SegmentKind::Encounter, Some(0), false),
            entry(3, SegmentKind::Trash, Some(0), false),
            entry(4, SegmentKind::Overall, Some(1), true),
            entry(5, SegmentKind::Encounter, Some(1), true),
        ]
    }

    /// A state that has received `entries` as its first list.
    fn listed(entries: Vec<ListEntry>, active: bool) -> ClientState {
        let mut state = ClientState::new();
        let reqs = state.on_msg(DaemonMsg::SegmentList {
            seq: 1,
            entries,
            source: None,
            active,
            log_id: None,
        });
        if active {
            assert_eq!(reqs.len(), 1, "an active log jumps to the live meter");
        }
        state
    }

    fn info(kind: SegmentKind, duration_ms: i64, live: bool) -> SegmentInfo {
        SegmentInfo {
            kind,
            name: "x".into(),
            start_ms: 0,
            duration_ms,
            success: None,
            live,
            instance: Some(1),
            pars_ms: None,
            arena: false,
            encounter: None,
        }
    }

    /// A meter snapshot for the cursor the state currently watches.
    fn snapshot_for(
        state: &ClientState,
        id: SegmentId,
        info: SegmentInfo,
        rows: Vec<Row>,
    ) -> DaemonMsg {
        DaemonMsg::Snapshot {
            seq: 2,
            segment: state.watched_segment(),
            id: Some(id),
            view: state.view,
            info,
            rows,
            total_rows: 0,
            breakdown: None,
            segment_count: state.entries().len() as u32,
            source: None,
            status: None,
        }
    }

    fn cursor_ids(watches: &[Cursor]) -> Vec<SegmentRef> {
        watches
            .iter()
            .filter_map(|c| match c {
                Cursor::Segment { segment, .. } => Some(*segment),
                _ => None,
            })
            .collect()
    }

    /// The pieces of an in-flight global drag.
    fn global(drag: Option<Drag>) -> ((f32, f32), i32, hypr::MonitorRect, bool) {
        match drag {
            Some(Drag::Global {
                grab,
                base,
                mon,
                moved,
            }) => (grab, base, mon, moved),
            other => panic!("not a global drag: {}", other.is_some()),
        }
    }

    fn moved_to(ov: &mut Overlay, x: f32, y: f32) -> Task<Message> {
        update(
            ov,
            Message::Ice(Event::Mouse(mouse::Event::CursorMoved {
                position: iced::Point::new(x, y),
            })),
        )
    }

    /// The fixture, as the overlay's timeline sees it: one raid visit whose
    /// Σ row (still live — nobody zoned out) leads four members.
    #[test]
    fn the_fixture_is_one_live_raid_visit() {
        let (state, _mock) = newest();
        let blocks = timeline::blocks(state.entries());
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].overall, Some(0));
        assert_eq!(blocks[0].members, vec![1, 2, 3, 4]);
        let sigma = &state.entries()[0].row;
        assert_eq!(sigma.kind, SegmentKind::Overall);
        assert!(sigma.live);
        assert_eq!(watched_pos(&state), Some(4), "newest: the final wipe");
        assert_eq!(watched_pos(&ClientState::new()), None);
    }

    // ---- construction -------------------------------------------------------

    #[test]
    fn a_fresh_overlay_asks_for_the_list_and_reads_no_debug_aids() {
        let (client, mut peer) = paired(ClientKind::Overlay);
        let ov = Overlay::new(client, cfg());
        assert!(!ov.expanded);
        assert_eq!(
            (ov.autotoggle, ov.autodrill, ov.autocompare, ov.autoseg),
            (0, 0, false, None)
        );
        assert_eq!(ov.app.view, View::Damage);
        assert!(ov.hypr.is_none(), "follow_game is off");
        assert_eq!(peer.watches(), vec![Cursor::List]);
        assert!(ov.game_visible && ov.daemon_visible);
        assert!(!ov.split);

        // The debug aids, read once at construction. Only the env-locked
        // tests call `new`, and the variables are cleared again before this
        // returns.
        // SAFETY: test-only, and no other test in this binary reads them.
        let _g = ENV_LOCK.lock().unwrap();
        unsafe {
            std::env::set_var("WOWDPS_OVERLAY_START_EXPANDED", "1");
            std::env::set_var("WOWDPS_OVERLAY_AUTOTOGGLE", "1");
            std::env::set_var("WOWDPS_OVERLAY_AUTODRILL", "2");
            std::env::set_var("WOWDPS_OVERLAY_AUTOCOMPARE", "1");
            std::env::set_var("WOWDPS_OVERLAY_AUTOSEG", "3");
            std::env::set_var("WOWDPS_OVERLAY_AUTOVIEW", "deaths");
        }
        let (client, _peer) = paired(ClientKind::Overlay);
        let ov = Overlay::new(client, cfg());
        let aids = (
            ov.expanded,
            ov.autotoggle,
            ov.autodrill,
            ov.autocompare,
            ov.autoseg,
            ov.app.view,
        );
        unsafe {
            std::env::set_var("WOWDPS_OVERLAY_AUTODRILL", "1");
            std::env::set_var("WOWDPS_OVERLAY_AUTOVIEW", "cc");
        }
        let (client, _peer) = paired(ClientKind::Overlay);
        let ov2 = Overlay::new(client, cfg());
        let aids2 = (ov2.autodrill, ov2.app.view);
        for name in [
            "WOWDPS_OVERLAY_START_EXPANDED",
            "WOWDPS_OVERLAY_AUTOTOGGLE",
            "WOWDPS_OVERLAY_AUTODRILL",
            "WOWDPS_OVERLAY_AUTOCOMPARE",
            "WOWDPS_OVERLAY_AUTOSEG",
            "WOWDPS_OVERLAY_AUTOVIEW",
        ] {
            unsafe { std::env::remove_var(name) };
        }
        assert_eq!(aids, (true, 20, 2, true, Some(3), View::Deaths));
        assert_eq!(aids2, (1, View::CrowdControl));
        assert!(!start_expanded());
    }

    #[test]
    fn autoview_names_every_view() {
        // `start_view` reads the environment; the mapping itself is what a
        // typo in the docs table would break.
        for (name, view) in [
            ("damage", View::Damage),
            ("healing", View::Healing),
            ("interrupts", View::Interrupts),
            ("cc", View::CrowdControl),
            ("dispels", View::Dispels),
            ("deaths", View::Deaths),
        ] {
            let (client, _peer) = paired(ClientKind::Overlay);
            // SAFETY: see above — serialized by the env lock below.
            let _g = ENV_LOCK.lock().unwrap();
            unsafe { std::env::set_var("WOWDPS_OVERLAY_AUTOVIEW", name) };
            let ov = Overlay::new(client, cfg());
            unsafe { std::env::remove_var("WOWDPS_OVERLAY_AUTOVIEW") };
            assert_eq!(ov.app.view, view, "{name}");
        }
        assert!(start_view().is_none());
    }

    static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    /// `WOWDPS_OVERLAY_DEBUG=1` traces every input path on stderr; walk them
    /// all under it (and the two remaining env-read branches) so the trace
    /// formatting itself is exercised. Env-locked: `debug()` is read live.
    #[test]
    fn debug_tracing_walks_every_traced_path() {
        let _g = ENV_LOCK.lock().unwrap();
        // SAFETY: test-only; the other env readers hold the same lock.
        unsafe {
            std::env::set_var("WOWDPS_OVERLAY_DEBUG", "1");
            std::env::set_var("WOWDPS_OVERLAY_AUTOCOMPARE", "half");
            std::env::set_var("WOWDPS_OVERLAY_AUTOVIEW", "bogus");
        }
        assert!(debug());
        let (client, _peer0) = paired(ClientKind::Overlay);
        let fresh = Overlay::new(client, cfg());
        assert_eq!(
            fresh.app.view,
            View::Damage,
            "an unknown AUTOVIEW is ignored"
        );
        assert!(fresh.autocompare);

        let hypr = FakeHypr::start();
        let (state, mut mock) = kill();
        let (mut ov, mut peer) = rig(state);
        ov.hypr_dir = Some(hypr.dir.clone());
        // The half pick: badged, still the meter.
        ov.autocompare = true;
        drop(update(&mut ov, Message::Tick));
        assert_eq!(ov.app.compare_picks().len(), 1);
        assert_eq!(ov.app.screen, Screen::Meter);
        drop(update(&mut ov, Message::ClearCompare));
        // Visibility flips, both sources.
        peer.push(&DaemonMsg::SetVisible(false));
        tick_until(&mut ov, |o| !o.daemon_visible);
        let (tx, rx) = mpsc::channel();
        ov.hypr = Some(rx);
        tx.send(false).unwrap();
        drop(update(&mut ov, Message::Tick));
        assert!(!ov.game_visible);
        // Pointer: press, raw release, a reorienting drag, its settle.
        drop(update(
            &mut ov,
            Message::Ice(Event::Mouse(mouse::Event::ButtonPressed(
                mouse::Button::Left,
            ))),
        ));
        drop(update(
            &mut ov,
            Message::Ice(Event::Mouse(mouse::Event::ButtonReleased(
                mouse::Button::Left,
            ))),
        ));
        hypr.set_cursor(3400, 300);
        drop(update(&mut ov, Message::GripPressed));
        hypr.set_cursor(3400, 1439);
        drop(moved_to(&mut ov, 0.0, 0.0));
        assert_eq!(ov.cfg.edge, Edge::Bottom);
        drop(update(&mut ov, Message::GripReleased));
        assert_eq!(ov.cfg.offset, 3320);
        // Toggle, zoom, navigation.
        drop(toggle(&mut ov));
        drop(update(&mut ov, Message::Zoom(1.0)));
        drop(update(&mut ov, Message::TimelineGoto(1)));
        roundtrip(&mut ov, &mut peer, &mut mock);
        nav_block(&mut ov, 0);
        // A new pull snapping back to live.
        peer.push(&DaemonMsg::SegmentOpened { id: SegmentId(11) });
        tick_until(&mut ov, |o| o.app.following_live());
        unsafe {
            std::env::remove_var("WOWDPS_OVERLAY_DEBUG");
            std::env::remove_var("WOWDPS_OVERLAY_AUTOCOMPARE");
            std::env::remove_var("WOWDPS_OVERLAY_AUTOVIEW");
        }
        assert!(!debug());
    }

    // ---- the tick -----------------------------------------------------------

    #[test]
    fn the_tick_pins_the_list_screen_to_the_newest_segment() {
        let (state, mut mock) = indexed();
        assert_eq!(state.screen, Screen::List);
        let (mut ov, mut peer) = rig(state);
        drop(update(&mut ov, Message::Tick));
        assert_eq!(ov.app.screen, Screen::Meter);
        assert!(ov.app.following_live());
        roundtrip(&mut ov, &mut peer, &mut mock);
        assert_eq!(ov.app.segment_name().as_deref(), Some("Verkath the Hollow"));
        assert_eq!(ov.app.rows().len(), 3);
        // Quiet ticks send nothing.
        drop(update(&mut ov, Message::Tick));
        assert!(peer.sent().is_empty());
    }

    #[test]
    fn visibility_composes_the_supervisor_wish_with_the_game_workspace() {
        let (state, _mock) = newest();
        let (mut ov, mut peer) = rig(state);
        ov.expanded = true;
        peer.push(&DaemonMsg::SetVisible(false));
        tick_until(&mut ov, |o| !o.daemon_visible);
        drop(view(&ov));
        drop(subscription(&ov));

        let (tx, rx) = mpsc::channel();
        ov.hypr = Some(rx);
        tx.send(false).unwrap();
        tx.send(true).unwrap();
        tx.send(false).unwrap();
        drop(update(&mut ov, Message::Tick));
        assert!(!ov.game_visible, "only the latest transition counts");

        peer.push(&DaemonMsg::SetVisible(true));
        tick_until(&mut ov, |o| o.daemon_visible);
        assert!(!ov.game_visible, "the game's workspace still hides it");
        drop(view(&ov));

        tx.send(true).unwrap();
        drop(update(&mut ov, Message::Tick));
        assert!(ov.game_visible);
        drop(update(&mut ov, Message::Tick));
        assert!(ov.game_visible, "no transition, no change");
        drop(apply_visibility(&ov));
        ov.expanded = false;
        drop(apply_visibility(&ov));
    }

    #[test]
    fn snapshots_feed_the_staleness_clock_and_notices_do_not() {
        let (state, _mock) = newest();
        let (mut ov, mut peer) = rig(state);
        peer.push(&DaemonMsg::Fatal("boom".into()));
        tick_until(&mut ov, |o| o.app.status.as_deref() == Some("boom"));
        assert!(ov.last_snapshot_at.is_none());
        let snap = snapshot_for(
            &ov.app,
            SegmentId(3),
            info(SegmentKind::Encounter, 45_000, false),
            vec![],
        );
        peer.push(&snap);
        tick_until(&mut ov, |o| o.last_snapshot_at.is_some());
        assert!(stale_secs(ov.last_snapshot_at).is_none(), "fresh");
        ov.last_snapshot_at = Instant::now().checked_sub(Duration::from_secs(7));
        assert_eq!(stale_secs(ov.last_snapshot_at), Some(7));
    }

    #[test]
    fn a_new_pull_snaps_a_scrubbed_meter_back_to_live() {
        let (state, mut mock) = kill();
        let (mut ov, mut peer) = rig(state);
        assert!(!ov.app.following_live());
        peer.push(&DaemonMsg::SegmentOpened { id: SegmentId(9) });
        tick_until(&mut ov, |o| o.app.following_live());
        assert_eq!(cursor_ids(&peer.watches()), vec![SegmentRef::Live]);
        roundtrip(&mut ov, &mut peer, &mut mock);
    }

    #[test]
    fn a_new_pull_leaves_the_live_visits_overall_parked() {
        let (state, mut mock) = newest();
        let (mut ov, mut peer) = rig(state);
        drop(update(&mut ov, Message::TimelineGoto(0)));
        roundtrip(&mut ov, &mut peer, &mut mock);
        assert_eq!(watched_pos(&ov.app), Some(0));
        assert!(!ov.app.following_live());
        peer.push(&DaemonMsg::SegmentOpened { id: SegmentId(10) });
        tick_until(&mut ov, |o| o.app.entries().len() == 6);
        assert!(
            !ov.app.following_live(),
            "the live Σ is a live meter of its own"
        );
        assert!(peer.watches().is_empty());
    }

    #[test]
    fn a_vanished_daemon_is_reported_without_a_respawn() {
        let (state, _mock) = newest();
        let (mut ov, peer) = rig(state);
        drop(peer);
        tick_until(&mut ov, |o| o.app.status.is_some());
        assert_eq!(
            ov.app.status.as_deref(),
            Some("daemon gone — reconnecting…")
        );
        assert!(ov.client.is_dead());
        // Split wants an aux connection but never opens one while the main
        // client is dead — that would respawn daemons in a loop.
        ov.split = true;
        ov.expanded = true;
        drop(update(&mut ov, Message::Tick));
        assert!(ov.aux.is_none());
        assert!(ov.split);
    }

    #[test]
    fn autoseg_parks_the_frame_and_autodrill_descends_twice() {
        let (state, mut mock) = newest();
        let (mut ov, mut peer) = rig(state);
        ov.autoseg = Some(2);
        ov.autodrill = 2;
        drop(update(&mut ov, Message::Tick));
        assert_eq!(ov.autoseg, None);
        assert_eq!(ov.app.watched_segment(), SegmentRef::Id(SegmentId(1)));
        assert_eq!(ov.autodrill, 2, "holds fire until the parked rows arrive");
        for _ in 0..4 {
            roundtrip(&mut ov, &mut peer, &mut mock);
            drop(update(&mut ov, Message::Tick));
        }
        roundtrip(&mut ov, &mut peer, &mut mock);
        assert_eq!(ov.app.segment_name().as_deref(), Some("The Ashen Warden"));
        assert!(ov.app.drill.is_some(), "drilled into the top row");
        assert!(ov.app.drill_spell().is_some(), "then into its top ability");
        assert_eq!(ov.autodrill, 0);
        // Armed again with nothing left to descend into: disarms itself.
        ov.autodrill = 1;
        drop(update(&mut ov, Message::Tick));
        assert_eq!(ov.autodrill, 0);
        assert!(peer.sent().is_empty());
    }

    #[test]
    fn autocompare_picks_the_top_two_and_grows_the_surface() {
        // The tick reads WOWDPS_OVERLAY_AUTOCOMPARE for the `half` variant.
        let _g = ENV_LOCK.lock().unwrap();
        let (state, mut mock) = kill();
        let (mut ov, mut peer) = rig(state);
        ov.expanded = true;
        ov.autocompare = true;
        assert_eq!(current_size(&ov), (410, 460));
        drop(update(&mut ov, Message::Tick));
        assert!(!ov.autocompare);
        assert_eq!(ov.app.screen, Screen::Compare);
        assert_eq!(ov.app.compare_picks().len(), 2);
        assert_eq!(current_size(&ov), (775, 575), "COMPARE_MIN × zoom 1.25");
        roundtrip(&mut ov, &mut peer, &mut mock);
        assert!(ov.app.compare_sides().is_some());
        drop(view(&ov));
    }

    #[test]
    fn autotoggle_fires_once_when_its_countdown_ends() {
        let (state, _mock) = newest();
        let (mut ov, _peer) = rig(state);
        ov.autotoggle = 2;
        drop(update(&mut ov, Message::Tick));
        assert!(!ov.expanded);
        drop(update(&mut ov, Message::Tick));
        assert!(ov.expanded);
        drop(update(&mut ov, Message::Tick));
        assert!(ov.expanded, "one shot");
    }

    // ---- pointer ------------------------------------------------------------

    #[test]
    fn cursor_motion_tracks_the_edge_axis() {
        let (mut ov, _peer) = rig(ClientState::new());
        drop(moved_to(&mut ov, 7.0, 42.0));
        assert_eq!(ov.cursor, 42.0, "side edges: y");
        ov.cfg.edge = Edge::Bottom;
        drop(moved_to(&mut ov, 7.0, 42.0));
        assert_eq!(ov.cursor, 7.0, "horizontal edges: x");
        // Raw events the widgets never claimed.
        drop(update(
            &mut ov,
            Message::Ice(Event::Mouse(mouse::Event::CursorEntered)),
        ));
        drop(update(
            &mut ov,
            Message::Ice(Event::Keyboard(iced::keyboard::Event::ModifiersChanged(
                iced::keyboard::Modifiers::empty(),
            ))),
        ));
    }

    #[test]
    fn right_click_backs_out_of_a_drill_but_never_past_it() {
        let (state, mut mock) = kill();
        let (mut ov, mut peer) = rig(state);
        drop(update(&mut ov, Message::RowClicked(0)));
        roundtrip(&mut ov, &mut peer, &mut mock);
        assert!(ov.app.drill.is_some());
        let right = Message::Ice(Event::Mouse(mouse::Event::ButtonPressed(
            mouse::Button::Right,
        )));
        drop(update(&mut ov, right.clone()));
        assert!(ov.app.drill.is_none());
        assert_eq!(peer.watches().len(), 1, "re-watch without the drill");
        drop(update(&mut ov, right));
        assert_eq!(
            ov.app.screen,
            Screen::Meter,
            "no drill: nothing to back out of"
        );
        assert!(peer.sent().is_empty());
    }

    #[test]
    fn a_click_without_motion_toggles_the_panel() {
        let (state, _mock) = newest();
        let (mut ov, _peer) = rig(state);
        drop(update(&mut ov, Message::GripPressed));
        assert!(matches!(ov.drag, Some(Drag::Local { moved: false, .. })));
        drop(update(&mut ov, Message::GripReleased));
        assert!(ov.expanded);
        assert!(ov.drag.is_none());
        assert_eq!(ov.shown_offset, 300);
        drop(update(&mut ov, Message::GripPressed));
        drop(update(&mut ov, Message::GripReleased));
        assert!(!ov.expanded);
        drop(update(&mut ov, Message::GripReleased));
        assert!(!ov.expanded, "a release with no press is nothing");
    }

    #[test]
    fn a_local_drag_slides_along_the_edge_and_settles_into_the_config() {
        let (mut ov, _peer) = rig(ClientState::new());
        drop(moved_to(&mut ov, 0.0, 50.0));
        drop(update(&mut ov, Message::GripPressed));
        drop(moved_to(&mut ov, 0.0, 52.0));
        assert_eq!(ov.shown_offset, 300, "under the drag threshold");
        assert!(!ov.drag.unwrap().moved());
        drop(moved_to(&mut ov, 0.0, 90.0));
        assert_eq!(ov.shown_offset, 340);
        drop(moved_to(&mut ov, 0.0, 50.4));
        assert_eq!(ov.shown_offset, 340, "a sub-pixel delta moves nothing");
        drop(moved_to(&mut ov, 0.0, -1000.0));
        assert_eq!(ov.shown_offset, 0, "never off the top");
        drop(update(&mut ov, Message::GripReleased));
        assert!(ov.drag.is_none());
        assert!(!ov.expanded, "a drag is not a click");
        assert_eq!(ov.cfg.offset, 0);
        assert!(
            Config::path().starts_with(test_env()),
            "saves land in the scratch config dir: {}",
            Config::path().display()
        );
        assert!(Config::path().exists());
    }

    #[test]
    fn a_raw_release_or_a_pointer_leaving_settles_the_drag() {
        let (mut ov, _peer) = rig(ClientState::new());
        drop(moved_to(&mut ov, 0.0, 10.0));
        drop(update(&mut ov, Message::GripPressed));
        drop(moved_to(&mut ov, 0.0, 60.0));
        drop(update(
            &mut ov,
            Message::Ice(Event::Mouse(mouse::Event::ButtonReleased(
                mouse::Button::Left,
            ))),
        ));
        assert!(ov.drag.is_none());
        assert_eq!(ov.cfg.offset, 350);
        drop(update(&mut ov, Message::GripReleased));
        assert!(
            !ov.expanded,
            "the widget's release finds the drag already settled"
        );

        // The grab is wherever the pointer was last seen (60).
        drop(update(&mut ov, Message::GripPressed));
        drop(moved_to(&mut ov, 0.0, 30.0));
        drop(update(
            &mut ov,
            Message::Ice(Event::Mouse(mouse::Event::CursorLeft)),
        ));
        assert!(ov.drag.is_none());
        assert_eq!(ov.cfg.offset, 320);
        assert_eq!(ov.cfg.offset, ov.shown_offset);
    }

    #[test]
    fn a_global_tab_drag_reorients_onto_the_nearest_edge() {
        let hypr = FakeHypr::start();
        let (mut ov, _peer) = rig(ClientState::new());
        ov.hypr_dir = Some(hypr.dir.clone());
        hypr.set_cursor(3400, 300);
        drop(update(&mut ov, Message::GripPressed));
        assert_eq!(
            global(ov.drag),
            ((3400.0, 300.0), 300, (0, 0, 3440, 1440), false)
        );
        hypr.set_cursor(3400, 303);
        drop(moved_to(&mut ov, 0.0, 0.0));
        assert_eq!(ov.shown_offset, 300, "under the threshold");
        hypr.set_cursor(3400, 400);
        drop(moved_to(&mut ov, 0.0, 0.0));
        assert_eq!(ov.shown_offset, 400);
        assert_eq!(ov.cfg.edge, Edge::Right);
        // Off the bottom of the monitor: clamped to it, and the bottom edge
        // captures the tab — centered under the cursor, clamped to fit.
        hypr.set_cursor(3400, 5000);
        drop(moved_to(&mut ov, 0.0, 0.0));
        assert_eq!(ov.cfg.edge, Edge::Bottom);
        assert_eq!(ov.shown_offset, 3320, "3400 - 120/2, clamped to 3440 - 120");
        assert_eq!(
            global(ov.drag),
            ((3400.0, 1439.0), 3320, (0, 0, 3440, 1440), true),
            "re-seeded on the new edge"
        );
        hypr.set_cursor(3000, 1439);
        drop(moved_to(&mut ov, 0.0, 0.0));
        assert_eq!(ov.shown_offset, 2920, "sliding along the new axis");
        drop(moved_to(&mut ov, 0.0, 0.0));
        assert_eq!(ov.shown_offset, 2920, "no motion, no task");
        drop(update(&mut ov, Message::GripReleased));
        assert_eq!((ov.cfg.edge, ov.cfg.offset), (Edge::Bottom, 2920));
        assert!(ov.drag.is_none());
    }

    #[test]
    fn a_global_panel_drag_slides_without_reorienting_and_fails_soft() {
        let hypr = FakeHypr::start();
        let (mut ov, _peer) = rig(ClientState::new());
        ov.hypr_dir = Some(hypr.dir.clone());
        ov.expanded = true;
        hypr.set_cursor(3400, 300);
        drop(update(&mut ov, Message::GripPressed));
        hypr.set_cursor(3400, 2000);
        drop(moved_to(&mut ov, 0.0, 0.0));
        assert_eq!(ov.cfg.edge, Edge::Right, "panels never reorient");
        assert_eq!(ov.shown_offset, 980, "1440 - 460: fully on the monitor");
        // Hyprland stops answering mid-drag: the sample is skipped.
        ov.hypr_dir = Some(hypr.dir.join("gone"));
        hypr.set_cursor(3400, 100);
        drop(moved_to(&mut ov, 0.0, 0.0));
        assert_eq!(ov.shown_offset, 980);
        ov.hypr_dir = None;
        drop(moved_to(&mut ov, 0.0, 0.0));
        assert_eq!(ov.shown_offset, 980);
        drop(update(&mut ov, Message::GripReleased));
        assert_eq!(ov.cfg.offset, 980);
        // Off every monitor at the press: no rect, so a local drag instead.
        ov.hypr_dir = Some(hypr.dir.clone());
        hypr.set_cursor(-50, -50);
        drop(update(&mut ov, Message::GripPressed));
        assert!(matches!(ov.drag, Some(Drag::Local { .. })));
    }

    #[test]
    fn expanding_near_the_bottom_borrows_a_shifted_offset() {
        let hypr = FakeHypr::start();
        let (mut ov, _peer) = rig(ClientState::new());
        ov.cfg.offset = 1400;
        ov.shown_offset = 1400;
        drop(toggle(&mut ov));
        assert!(ov.expanded);
        assert_eq!(
            ov.shown_offset, 1400,
            "no Hyprland: nothing to clamp against"
        );
        drop(toggle(&mut ov));
        ov.hypr_dir = Some(hypr.dir.clone());
        assert_eq!(max_offset(&ov, (410, 460)), Some(980));
        drop(toggle(&mut ov));
        assert_eq!(ov.shown_offset, 980, "borrowed, shifted to fit");
        assert_eq!(ov.cfg.offset, 1400, "the tab's anchor is untouched");
        drop(resize(&mut ov));
        assert_eq!(ov.shown_offset, 980);
        drop(toggle(&mut ov));
        assert_eq!(ov.shown_offset, 1400, "collapsing returns to it");
        hypr.set_cursor(-1, -1);
        assert_eq!(max_offset(&ov, (410, 460)), None);
    }

    // ---- navigation ---------------------------------------------------------

    #[test]
    fn cycle_view_walks_every_view() {
        let (state, mut mock) = kill();
        let (mut ov, mut peer) = rig(state);
        let mut seen = Vec::new();
        for _ in 0..6 {
            drop(update(&mut ov, Message::CycleView));
            roundtrip(&mut ov, &mut peer, &mut mock);
            seen.push(ov.app.view);
        }
        assert_eq!(
            seen,
            [
                View::Healing,
                View::Interrupts,
                View::CrowdControl,
                View::Dispels,
                View::Deaths,
                View::Damage
            ]
        );
    }

    #[test]
    fn block_arrows_step_whole_visits_and_come_home_to_live() {
        let (mut ov, mut peer) = rig(listed(visits(), true));
        assert!(ov.app.following_live());
        assert_eq!(watched_pos(&ov.app), Some(5));
        drop(update(&mut ov, Message::PrevBlock));
        assert_eq!(
            cursor_ids(&peer.watches()),
            vec![SegmentRef::Id(SegmentId(1))],
            "the finished visit's Σ"
        );
        drop(update(&mut ov, Message::PrevBlock));
        assert_eq!(
            cursor_ids(&peer.watches()),
            vec![SegmentRef::Id(SegmentId(0))],
            "the city fight"
        );
        drop(update(&mut ov, Message::PrevBlock));
        assert!(peer.sent().is_empty(), "nothing older");
        drop(update(&mut ov, Message::NextBlock));
        assert_eq!(
            cursor_ids(&peer.watches()),
            vec![SegmentRef::Id(SegmentId(1))]
        );
        drop(update(&mut ov, Message::NextBlock));
        assert_eq!(
            cursor_ids(&peer.watches()),
            vec![SegmentRef::Live],
            "the live visit re-pins Live"
        );
        drop(update(&mut ov, Message::NextBlock));
        assert!(peer.sent().is_empty(), "nothing newer");
        // One block only: the fixture's arrows go nowhere.
        let (state, _mock) = newest();
        let (mut ov, mut peer) = rig(state);
        drop(update(&mut ov, Message::PrevBlock));
        drop(update(&mut ov, Message::NextBlock));
        assert!(peer.sent().is_empty());
        let (mut ov, mut peer) = rig(ClientState::new());
        nav_block(&mut ov, 1);
        assert!(peer.sent().is_empty(), "no entries at all");
    }

    #[test]
    fn timeline_goto_and_go_live_send_exactly_one_watch() {
        let (mut ov, mut peer) = rig(listed(visits(), true));
        drop(update(&mut ov, Message::TimelineGoto(2)));
        assert_eq!(
            cursor_ids(&peer.watches()),
            vec![SegmentRef::Id(SegmentId(2))]
        );
        assert!(!ov.app.following_live());
        drop(update(&mut ov, Message::GoLive));
        assert_eq!(cursor_ids(&peer.watches()), vec![SegmentRef::Live]);
        drop(update(&mut ov, Message::GoLive));
        assert!(peer.sent().is_empty(), "already live");
    }

    #[test]
    fn strip_scroll_scrubs_members_in_whole_notches() {
        let (state, _mock) = newest();
        let (mut ov, mut peer) = rig(state);
        drop(update(&mut ov, Message::StripScroll(0.5)));
        assert!(peer.sent().is_empty(), "half a notch carries over");
        drop(update(&mut ov, Message::StripScroll(0.5)));
        assert_eq!(
            cursor_ids(&peer.watches()),
            vec![SegmentRef::Id(SegmentId(2))],
            "one older"
        );
        assert_eq!(ov.strip_acc, 0.0);
        drop(update(&mut ov, Message::StripScroll(-2.0)));
        assert_eq!(
            cursor_ids(&peer.watches()),
            vec![SegmentRef::Live],
            "newer to the end, then the second step has nowhere to go"
        );
        drop(update(&mut ov, Message::StripScroll(10.0)));
        assert_eq!(
            cursor_ids(&peer.watches()),
            vec![
                SegmentRef::Id(SegmentId(2)),
                SegmentRef::Id(SegmentId(1)),
                SegmentRef::Id(SegmentId(0)),
                SegmentRef::Id(SegmentId(4)),
            ],
            "older through the members to the Σ, then stops"
        );
        let (mut ov, mut peer) = rig(ClientState::new());
        drop(update(&mut ov, Message::StripScroll(3.0)));
        assert!(peer.sent().is_empty());
    }

    // ---- split view ---------------------------------------------------------

    #[test]
    fn the_split_view_watches_the_visits_overall_on_the_aux_connection() {
        let (state, mut mock) = kill();
        let (mut ov, mut peer) = rig(state);
        ov.expanded = true;
        let (aux, mut aux_peer) = paired(ClientKind::Window);
        ov.aux = Some(aux);
        drop(update(&mut ov, Message::ToggleSplit));
        assert!(ov.split && ov.cfg.overlay_split);
        assert_eq!(ov.aux_watch, Some((SegmentId(4), View::Damage)));
        assert_eq!(
            aux_peer.watches(),
            vec![Cursor::Segment {
                segment: SegmentRef::Id(SegmentId(4)),
                view: View::Damage,
                top_n: Some(AUX_TOP_N),
                drill: None,
                spell: None,
            }]
        );
        let rows = ov.app.rows();
        let snap = |sid: u64, view: View, rows: Vec<Row>| DaemonMsg::Snapshot {
            seq: 1,
            segment: SegmentRef::Id(SegmentId(sid)),
            id: Some(SegmentId(sid)),
            view,
            info: info(SegmentKind::Overall, 134_000, true),
            rows,
            total_rows: 3,
            breakdown: None,
            segment_count: 5,
            source: None,
            status: None,
        };
        aux_peer.push(&snap(3, View::Damage, rows.clone()));
        aux_peer.push(&snap(4, View::Healing, rows.clone()));
        aux_peer.push(&snap(4, View::Damage, rows.clone()));
        tick_until(&mut ov, |o| !o.aux_rows.is_empty());
        assert_eq!(ov.aux_rows.len(), 3, "only the watched Σ + view counts");
        assert_eq!(ov.aux_info.as_ref().map(|i| i.duration_ms), Some(134_000));
        let block = &timeline::blocks(ov.app.entries())[0];
        assert_eq!(
            instance_elapsed(&ov, block, 0),
            134_000,
            "the aux snapshot's clock"
        );
        drop(view(&ov));

        // A view change re-watches in the new view and drops the old rows.
        drop(update(&mut ov, Message::CycleView));
        roundtrip(&mut ov, &mut peer, &mut mock);
        drop(update(&mut ov, Message::Tick));
        assert_eq!(ov.aux_watch, Some((SegmentId(4), View::Healing)));
        assert!(ov.aux_rows.is_empty());
        assert_eq!(aux_peer.watches().len(), 1);

        // Watching the Σ itself: nothing to duplicate, the aux idles.
        drop(update(&mut ov, Message::TimelineGoto(0)));
        roundtrip(&mut ov, &mut peer, &mut mock);
        drop(update(&mut ov, Message::Tick));
        assert_eq!(ov.aux_watch, None);
        assert_eq!(aux_peer.watches(), vec![Cursor::List]);
        drop(update(&mut ov, Message::Tick));
        assert!(aux_peer.sent().is_empty(), "idle stays idle");

        // Back on a member, then the aux daemon connection dies: no respawn
        // (no daemon binary to spawn), no panic, the split just goes dark.
        drop(update(&mut ov, Message::TimelineGoto(2)));
        roundtrip(&mut ov, &mut peer, &mut mock);
        drop(update(&mut ov, Message::Tick));
        assert!(ov.aux_watch.is_some());
        drop(aux_peer);
        for _ in 0..100 {
            drop(update(&mut ov, Message::Tick));
            if ov.aux.as_mut().is_some_and(|c| c.is_dead()) {
                break;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(ov.aux.as_mut().unwrap().is_dead());
        drop(update(&mut ov, Message::Tick));

        drop(update(&mut ov, Message::ToggleSplit));
        assert!(!ov.split && !ov.cfg.overlay_split);
        assert_eq!(ov.aux_watch, None);
        // Collapsed, split wants nothing either.
        ov.split = true;
        ov.expanded = false;
        sync_aux(&mut ov);
        assert_eq!(ov.aux_watch, None);
    }

    #[test]
    fn the_instance_clock_grows_with_the_live_member_and_freezes_when_resolved() {
        let mut state = listed(visits(), true);
        let snap = DaemonMsg::Snapshot {
            seq: 2,
            segment: SegmentRef::Live,
            id: Some(SegmentId(5)),
            view: View::Damage,
            info: info(SegmentKind::Encounter, 75_000, true),
            rows: vec![],
            total_rows: 0,
            breakdown: None,
            segment_count: 6,
            source: None,
            status: None,
        };
        assert!(state.on_msg(snap).is_empty());
        assert_eq!(state.duration_ms(), 75_000);
        let (ov, _peer) = rig(state);
        let blocks = timeline::blocks(ov.app.entries());
        assert_eq!(blocks.len(), 3);
        let live_visit = &blocks[2];
        assert_eq!(
            instance_elapsed(&ov, live_visit, 4),
            50_000 + (75_000 - 60_000),
            "the Σ's last broadcast clock plus the live member's growth"
        );
        let done = &blocks[1];
        assert_eq!(
            instance_elapsed(&ov, done, 1),
            20_000,
            "another block: its Σ's own clock"
        );

        let mut resolved = visits();
        resolved[4].row.success = Some(false);
        let (ov, _peer) = rig(listed(resolved, true));
        assert_eq!(
            instance_elapsed(&ov, &timeline::blocks(ov.app.entries())[2], 4),
            50_000,
            "frozen at the official time"
        );
    }

    // ---- the rest of the message surface ------------------------------------

    #[test]
    fn rows_drill_spells_descend_and_the_comparison_grows_and_clears() {
        let (state, mut mock) = kill();
        let (mut ov, mut peer) = rig(state);
        ov.expanded = true;
        drop(update(&mut ov, Message::RowClicked(1)));
        assert!(ov.app.drill.is_some());
        // The drill before its rows arrive: "no data yet".
        drop(view(&ov));
        roundtrip(&mut ov, &mut peer, &mut mock);
        assert_eq!(ov.app.row_sel, 1);
        drop(view(&ov));
        drop(update(&mut ov, Message::DrillRange(Some((1_000, 5_000)))));
        assert_eq!(ov.app.drill_range(), Some((1_000, 5_000)));
        drop(view(&ov));
        drop(update(&mut ov, Message::DrillRange(None)));
        drop(update(&mut ov, Message::SpellRow(0)));
        assert!(ov.app.drill_spell().is_some());
        // The ability drill before its row arrives.
        drop(view(&ov));
        roundtrip(&mut ov, &mut peer, &mut mock);
        assert!(ov.app.drill_spell().is_some());
        assert_eq!(
            ov.app.drill.as_ref().map(|d| d.pane),
            Some(wowdps_model::Pane::Spell)
        );
        drop(view(&ov));

        drop(update(&mut ov, Message::ClearCompare));
        assert_eq!(ov.app.screen, Screen::Meter, "nothing picked: stays put");
        drop(update(&mut ov, Message::CompareRow(0)));
        assert_eq!(ov.app.compare_picks().len(), 1);
        assert_eq!(
            current_size(&ov),
            (410, 460),
            "half a pick is still the meter"
        );
        drop(view(&ov));
        drop(update(&mut ov, Message::CompareRow(2)));
        assert_eq!(ov.app.screen, Screen::Compare);
        assert_eq!(current_size(&ov), (775, 575));
        roundtrip(&mut ov, &mut peer, &mut mock);
        assert!(ov.app.compare_sides().is_some());
        drop(view(&ov));
        drop(update(&mut ov, Message::ToggleGraph));
        assert_eq!(ov.app.graph_mode(), wowdps_model::GraphMode::Total);
        drop(update(
            &mut ov,
            Message::CompareHover(Some("Potion".into())),
        ));
        drop(update(&mut ov, Message::GraphProbe(Some(1234.5))));
        assert_eq!(ov.compare_hover.as_deref(), Some("Potion"));
        assert_eq!(ov.graph_probe, Some(1234.5));
        drop(view(&ov));
        drop(update(&mut ov, Message::CompareRange(Some((0, 10_000)))));
        roundtrip(&mut ov, &mut peer, &mut mock);
        assert_eq!(ov.app.compare_shown_range(), Some((0, 10_000)));
        drop(view(&ov));
        let key = ov
            .app
            .compare_sides()
            .and_then(|(a, _)| a.spells.first().map(|r| (r.key.clone(), r.label.clone())));
        let key = key.expect("the kill has spells");
        drop(update(&mut ov, Message::CompareSpell(key.clone())));
        roundtrip(&mut ov, &mut peer, &mut mock);
        assert_eq!(ov.app.compare_spell(), Some(&key));
        drop(view(&ov));
        drop(update(&mut ov, Message::ClearCompare));
        assert_eq!(
            ov.app.screen,
            Screen::Compare,
            "the spell drill clears first"
        );
        assert_eq!(ov.app.compare_spell(), None);
        drop(update(&mut ov, Message::ClearCompare));
        assert_eq!(ov.app.screen, Screen::Meter);
        assert_eq!(current_size(&ov), (410, 460));
    }

    #[test]
    fn zoom_scales_the_panel_with_the_ui_and_clamps() {
        let (mut ov, _peer) = rig(ClientState::new());
        drop(update(&mut ov, Message::Zoom(1.0)));
        assert!((ov.cfg.zoom - 1.30).abs() < 1e-6);
        assert_eq!((ov.cfg.width, ov.cfg.height), (426, 478));
        assert_eq!(tab_size(ov.cfg.edge, ov.cfg.zoom), (34, 125));
        drop(update(&mut ov, Message::Zoom(100.0)));
        assert_eq!(ov.cfg.zoom, 2.5);
        drop(update(&mut ov, Message::Zoom(1.0)));
        assert_eq!(ov.cfg.zoom, 2.5, "at the ceiling: no-op");
        drop(update(&mut ov, Message::Zoom(-100.0)));
        assert_eq!(ov.cfg.zoom, 0.6);
        assert!(ov.cfg.width >= 160 && ov.cfg.height >= 180, "floors");
        ov.expanded = true;
        drop(update(&mut ov, Message::Zoom(1.0)));
        assert_eq!(ov.shown_offset, ov.cfg.offset);
    }

    #[test]
    fn options_and_the_small_messages() {
        let (state, _mock) = newest();
        let (mut ov, mut peer) = rig(state);
        drop(update(&mut ov, Message::ToggleOptions));
        assert!(ov.options_open);
        ov.expanded = true;
        drop(view(&ov));
        drop(update(&mut ov, Message::SetShowRanks(false)));
        assert!(!ov.cfg.show_ranks);
        drop(view(&ov));
        drop(update(&mut ov, Message::Noop));
        drop(update(&mut ov, Message::CloseOptions));
        assert!(!ov.options_open);
        drop(update(&mut ov, Message::ToggleOptions));
        drop(update(&mut ov, Message::ToggleOptions));
        assert!(!ov.options_open);
        drop(update(&mut ov, Message::Animate));
        drop(update(&mut ov, Message::DiscardTrash));
        assert!(matches!(peer.sent().as_slice(), [ClientMsg::DiscardTrash]));
        // Layer-shell control messages never come back to us.
        drop(update(
            &mut ov,
            Message::AnchorSizeChange(anchor_for(Edge::Left), (1, 1)),
        ));
        drop(update(&mut ov, Message::MarginChange((0, 0, 0, 0))));
    }

    // ---- rendering ----------------------------------------------------------

    #[test]
    fn the_tab_renders_on_both_axes_and_hides_to_nothing() {
        let (state, _mock) = live();
        let (mut ov, _peer) = rig(state);
        assert!(ov.app.is_live());
        drop(view(&ov));
        ov.cfg.edge = Edge::Top;
        drop(view(&ov));
        ov.game_visible = false;
        drop(view(&ov));
        drop(subscription(&ov));
    }

    #[test]
    fn the_panel_renders_while_waiting_for_combat() {
        let (mut ov, _peer) = rig(ClientState::new());
        ov.expanded = true;
        drop(view(&ov));
        drop(subscription(&ov));
        let (state, _mock) = indexed();
        let (mut ov, _peer) = rig(state);
        ov.expanded = true;
        drop(view(&ov));
    }

    #[test]
    fn the_panel_renders_the_meter_inside_a_visit() {
        let (state, mut mock) = live();
        let (mut ov, mut peer) = rig(state);
        ov.expanded = true;
        ov.last_snapshot_at = Instant::now().checked_sub(Duration::from_secs(9));
        drop(view(&ov));
        drop(subscription(&ov));
        // Scrubbed off live: the footer offers the way home.
        drop(update(&mut ov, Message::TimelineGoto(1)));
        roundtrip(&mut ov, &mut peer, &mut mock);
        assert!(!ov.app.following_live());
        drop(view(&ov));
        // Parked on the Σ itself.
        drop(update(&mut ov, Message::TimelineGoto(0)));
        roundtrip(&mut ov, &mut peer, &mut mock);
        assert_eq!(ov.app.segment_kind(), Some(SegmentKind::Overall));
        drop(view(&ov));
        // Split rows under the current fight.
        drop(update(&mut ov, Message::TimelineGoto(2)));
        roundtrip(&mut ov, &mut peer, &mut mock);
        ov.split = true;
        ov.aux_rows = ov.app.rows();
        ov.aux_info = Some(info(SegmentKind::Overall, 134_000, true));
        ov.aux_watch = Some((SegmentId(4), View::Damage));
        drop(view(&ov));
        ov.cfg.show_ranks = false;
        ov.options_open = true;
        drop(view(&ov));
    }

    #[test]
    fn the_panel_renders_every_drill_shape() {
        let (state, mut mock) = kill();
        let (mut ov, mut peer) = rig(state);
        ov.expanded = true;
        for view_ in [View::Damage, View::Healing, View::Interrupts, View::Deaths] {
            let reqs = ov.app.apply(Action::SetView(view_));
            pump(&mut ov.app, &mut mock, reqs);
            drop(view(&ov));
            drop(update(&mut ov, Message::RowClicked(0)));
            roundtrip(&mut ov, &mut peer, &mut mock);
            drop(view(&ov));
            if ov.app.drill.is_some() {
                drop(update(&mut ov, Message::SpellRow(0)));
                roundtrip(&mut ov, &mut peer, &mut mock);
                drop(view(&ov));
            }
            while ov.app.drill.is_some() {
                let reqs = ov.app.apply(Action::Back);
                pump(&mut ov.app, &mut mock, reqs);
                drop(view(&ov));
            }
            assert_eq!(ov.app.screen, Screen::Meter);
        }
    }

    #[test]
    fn the_chip_names_every_selection_kind() {
        let mut entries = visits();
        entries[3].row.name = "Hallway pack".into();
        let (ov, _peer) = rig(listed(entries, true));
        let blocks = timeline::blocks(ov.app.entries());
        let done = &blocks[1];
        for pos in [Some(1), Some(2), Some(3), None] {
            drop(chip(&ov, done, pos, 1.0));
        }
        let (ov, _peer) = rig(listed(visits(), true));
        let blocks = timeline::blocks(ov.app.entries());
        drop(chip(&ov, &blocks[1], Some(3), 1.0));
        drop(options_card(&ov.cfg, 1.5));
    }

    // ---- pure geometry ------------------------------------------------------

    #[test]
    fn wheel_notches_normalize_lines_and_pixels() {
        assert_eq!(notches(mouse::ScrollDelta::Lines { x: 0.0, y: 2.0 }), 2.0);
        assert_eq!(
            notches(mouse::ScrollDelta::Pixels { x: 0.0, y: -80.0 }),
            -2.0
        );
    }

    #[test]
    fn the_drag_axis_follows_the_edge() {
        let mon = (10, 20, 3440, 1440);
        assert_eq!(drag_axis(Edge::Left, mon, (26, 96)), (20, 1440, 96));
        assert_eq!(drag_axis(Edge::Top, mon, (96, 26)), (10, 3440, 96));
    }

    #[test]
    fn overall_tags_word_the_visit_outcome() {
        let mut row = entry(1, SegmentKind::Overall, Some(0), false).row;
        assert_eq!(
            overall_tag(&row, 0),
            (String::new(), DIM),
            "unresolved, not live"
        );
        row.live = true;
        assert_eq!(overall_tag(&row, 0), ("LIVE".into(), YELLOW));
        row.pars_ms = Some((1_800_000, 1_440_000, 1_080_000));
        let (tag, color) = overall_tag(&row, 600_000);
        assert!(tag.starts_with("LIVE "), "{tag}");
        assert_eq!(color, YELLOW);
        row.success = Some(true);
        let (tag, color) = overall_tag(&row, 1_000_000);
        assert!(tag.starts_with("TIMED"), "{tag}");
        assert_eq!(color, GREEN);
        row.success = Some(false);
        let (tag, color) = overall_tag(&row, 2_000_000);
        assert!(tag.starts_with("OVER"), "{tag}");
        assert_eq!(color, RED);
        row.pars_ms = None;
        assert_eq!(overall_tag(&row, 0), ("OVER".into(), RED));
        row.success = Some(true);
        assert_eq!(overall_tag(&row, 0), ("TIMED".into(), GREEN));
    }

    #[test]
    fn styles_and_theme_are_fixed() {
        let (ov, _peer) = rig(ClientState::new());
        assert!(matches!(theme(&ov), Theme::TokyoNight));
        let s = style(&ov, &Theme::TokyoNight);
        assert_eq!(s.background_color, Color::TRANSPARENT);
        let p = panel_style(0.5);
        assert!(matches!(p.background, Some(iced::Background::Color(c)) if c.a == 0.5));
        assert!(
            start_view().is_none(),
            "no AUTOVIEW in the test environment"
        );
    }

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
        assert_eq!(anchor_for(Edge::Left), Anchor::Left | Anchor::Top);
        assert_eq!(anchor_for(Edge::Top), Anchor::Top | Anchor::Left);
        assert_eq!(margin_for(Edge::Left, 7), (7, 0, 0, 0));
        assert_eq!(margin_for(Edge::Top, 7), (0, 0, 0, 7));
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
