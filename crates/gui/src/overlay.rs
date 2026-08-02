//! The wlr-layer-shell overlay: a slim tab pinned to a configured screen
//! edge, on the compositor's `overlay` layer so it stays visible above the
//! fullscreen game on every workspace of its output.
//!
//! Clicking the tab expands a narrow live meter; clicking the panel header
//! collapses it again. Dragging either slides the surface along its edge and
//! persists the position to the config file on release. The surface never
//! takes keyboard focus (`KeyboardInteractivity::None`), so the game keeps
//! every keystroke; interaction is mouse-only.

use std::sync::mpsc::Receiver;
use std::time::Instant;

use iced::widget::{Space, column, container, mouse_area, row, scrollable, text};
use iced::{Color, Element, Event, Length, Subscription, Task, Theme, event, mouse, time};
use iced_layershell::reexport::{Anchor, KeyboardInteractivity, Layer};
use iced_layershell::settings::{LayerShellSettings, StartMode};
use iced_layershell::to_layer_message;

use wowdps_core::app::{Action, App, Screen};
use wowdps_core::fmt::{duration, view_name};
use wowdps_core::model::View;
use wowdps_core::tail::{self, SourceSpec, TailEvent};

use crate::config::{Config, Edge};
use crate::view::{DIM, GREEN, RED, YELLOW, bar_row};
use crate::window::{TICK, drain_tail, service_loads, stale_secs};

/// Tab dimensions: thin across the edge, long along it.
const TAB_THICKNESS: u32 = 26;
const TAB_LENGTH: u32 = 96;
/// A press that travels less than this many pixels is a click, not a drag.
const DRAG_THRESHOLD: f32 = 5.0;

pub fn run(spec: SourceSpec, cfg: Config) -> Result<(), iced_layershell::Error> {
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
        margin: margin_for(&cfg),
        keyboard_interactivity: KeyboardInteractivity::None,
        start_mode,
        events_transparent: false,
    };

    iced_layershell::application(
        move || Overlay::new(spec.clone(), cfg.clone()),
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
}

struct Drag {
    /// Cursor position along the edge axis when the press landed.
    grab: f32,
    moved: bool,
}

struct Overlay {
    app: App,
    lines: Receiver<TailEvent>,
    last_lines_at: Option<Instant>,
    cfg: Config,
    expanded: bool,
    /// Last observed cursor position along the edge axis, surface-local.
    cursor: f32,
    drag: Option<Drag>,
    /// Ticks until the debug auto-toggle fires; 0 = disabled.
    autotoggle: u32,
    /// Process start, for debug-trace timestamps.
    started: Instant,
}

impl Overlay {
    fn new(spec: SourceSpec, cfg: Config) -> Self {
        Self {
            app: App::new(),
            lines: tail::spawn(spec),
            last_lines_at: None,
            cfg,
            expanded: start_expanded(),
            cursor: 0.0,
            drag: None,
            autotoggle: if std::env::var_os("WOWDPS_OVERLAY_AUTOTOGGLE").is_some() {
                20
            } else {
                0
            },
            started: Instant::now(),
        }
    }
}

/// Flip between the tab and the panel. `AnchorSizeChange` (not the bare
/// `SizeChange`) is the resize path upstream exercises in its own examples.
fn toggle(state: &mut Overlay) -> Task<Message> {
    state.expanded = !state.expanded;
    let size = if state.expanded {
        (state.cfg.width, state.cfg.height)
    } else {
        tab_size(state.cfg.edge, state.cfg.zoom)
    };
    if debug() {
        eprintln!(
            "overlay: [{:>8.1}ms] toggle -> expanded={} size={size:?}",
            state.started.elapsed().as_secs_f64() * 1000.0,
            state.expanded
        );
    }
    Task::done(Message::AnchorSizeChange(anchor_for(state.cfg.edge), size))
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
            drain_tail(&mut state.app, &state.lines, &mut state.last_lines_at);
            // The overlay has no list screen: pin to the newest segment as
            // soon as one exists (lazy-loading it if it is indexed history).
            if state.app.screen == Screen::List && state.app.segment_count() > 0 {
                state.app.set_list_selection(usize::MAX);
                state.app.apply(Action::Open);
                service_loads(&mut state.app);
            }
            // Debug aid: WOWDPS_OVERLAY_AUTOTOGGLE flips the panel once after
            // ~2s, so resizing can be verified on outputs nothing can click.
            if state.autotoggle > 0 {
                state.autotoggle -= 1;
                if state.autotoggle == 0 {
                    return toggle(state);
                }
            }
            Task::none()
        }
        Message::Ice(Event::Mouse(mouse::Event::CursorMoved { position })) => {
            let along = if state.cfg.edge.is_vertical() {
                position.y
            } else {
                position.x
            };
            state.cursor = along;
            let Some(drag) = state.drag.as_mut() else {
                return Task::none();
            };
            // The surface chases the pointer: each event's distance from the
            // grab point is how far the surface still has to move. Once it
            // catches up, the local position returns to the grab point.
            let delta = along - drag.grab;
            if delta.abs() > DRAG_THRESHOLD {
                drag.moved = true;
            }
            if drag.moved && delta as i32 != 0 {
                state.cfg.offset = (state.cfg.offset + delta as i32).max(0);
                return Task::done(Message::MarginChange(margin_for(&state.cfg)));
            }
            Task::none()
        }
        Message::Ice(Event::Mouse(m @ (mouse::Event::ButtonPressed(_) | mouse::Event::ButtonReleased(_) | mouse::Event::CursorEntered))) => {
            if debug() {
                eprintln!("overlay: mouse {m:?}");
            }
            Task::none()
        }
        Message::Ice(Event::Mouse(mouse::Event::CursorLeft)) => {
            // Fast drags can outrun the surface; a pointer that left mid-drag
            // will not deliver a release, so settle up now.
            if state.drag.take().is_some_and(|d| d.moved) {
                state.cfg.save();
            }
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
            state.drag = Some(Drag {
                grab: state.cursor,
                moved: false,
            });
            Task::none()
        }
        Message::GripReleased => match state.drag.take() {
            Some(d) if d.moved => {
                state.cfg.save();
                Task::none()
            }
            _ => toggle(state),
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
            state.app.apply(Action::SetView(next));
            Task::none()
        }
        // Layer-shell control messages generated by `to_layer_message` are
        // consumed by the runtime, never delivered back to us.
        _ => Task::none(),
    }
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
fn margin_for(cfg: &Config) -> (i32, i32, i32, i32) {
    if cfg.edge.is_vertical() {
        (cfg.offset, 0, 0, 0)
    } else {
        (0, 0, 0, cfg.offset)
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
    if state.expanded {
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
    .into()
}

/// The expanded panel: header grip, live meter rows, view switcher.
fn panel(state: &Overlay) -> Element<'_, Message> {
    let app = &state.app;
    let name = app
        .segment_name()
        .unwrap_or_else(|| "waiting for combat…".to_string());
    let (tag, tag_color) = if app.is_live() {
        ("LIVE", YELLOW)
    } else {
        match app.segment_success() {
            Some(true) => ("KILL", GREEN),
            Some(false) => ("WIPE", RED),
            None => ("", DIM),
        }
    };

    let z = state.cfg.zoom;
    let header = mouse_area(
        container(
            row![
                text(name).size(13.0 * z),
                text(tag).size(10.0 * z).color(tag_color),
                Space::new().width(Length::Fill),
                text(duration(app.duration_ms())).size(12.0 * z),
            ]
            .spacing(6)
            .align_y(iced::Alignment::Center),
        )
        .padding([4, 8])
        .width(Length::Fill),
    )
    .on_press(Message::GripPressed)
    .on_release(Message::GripReleased);

    let rows = app.rows();
    let mut list = column![].spacing(2);
    if rows.is_empty() {
        list = list.push(text("no data yet").size(12.0 * z).color(DIM));
    }
    for r in &rows {
        list = list.push(bar_row(r, false, 20.0 * z, false, z));
    }

    let mut status = row![
        mouse_area(text(view_name(app.view)).size(11.0 * z).color(DIM))
            .on_press(Message::CycleView),
    ]
    .spacing(8);
    if let (true, Some(secs)) = (app.is_live(), stale_secs(state.last_lines_at)) {
        status = status.push(
            text(format!("no events for {secs}s"))
                .size(10.0 * z)
                .color(YELLOW),
        );
    }

    container(
        column![header, scrollable(list).height(Length::Fill), status]
            .spacing(4)
            .padding(6)
            .height(Length::Fill),
    )
    .style(|_: &Theme| panel_style(0.92))
    .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn geometry_follows_the_configured_edge() {
        let mut cfg = Config {
            edge: Edge::Right,
            offset: 250,
            ..Config::default()
        };
        assert_eq!(anchor_for(Edge::Right), Anchor::Right | Anchor::Top);
        assert_eq!(margin_for(&cfg), (250, 0, 0, 0), "side edges offset from the top");
        assert_eq!(tab_size(Edge::Right, 1.0), (TAB_THICKNESS, TAB_LENGTH));
        assert_eq!(tab_size(Edge::Right, 2.0), (TAB_THICKNESS * 2, TAB_LENGTH * 2));

        cfg.edge = Edge::Bottom;
        assert_eq!(anchor_for(Edge::Bottom), Anchor::Bottom | Anchor::Left);
        assert_eq!(margin_for(&cfg), (0, 0, 0, 250), "horizontal edges offset from the left");
        assert_eq!(tab_size(Edge::Bottom, 1.0), (TAB_LENGTH, TAB_THICKNESS));
    }
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
