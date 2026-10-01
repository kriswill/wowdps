//! The wowdps GUI on Zed's GPUI through GPUI Kit (`docs/spec-gui-new.md`):
//! a pure rendering client of the wowdps daemon, drawn either as the meter
//! window (default) or as a wlr-layer-shell overlay tab (`--overlay`) that
//! pins to a screen edge above the game. It replaced the iced GUI at the
//! cutover (`docs/plan-gui-new.md` phase 5).
//!
//! It depends on `wowdps-model`, `wowdps-proto` and the framework-free
//! `wowdps-gui-logic` only, never the engine, so it cannot open a combat
//! log or parse a line even by accident. What to tail is the daemon's
//! decision (config `logs_dir`, or `wowdps daemon --file …`); the daemon's
//! overlay supervisor spawns this binary (config `gui_binary`, default
//! `wowdps-gui`) with `--overlay` when the game starts.

mod ease;
#[cfg(test)]
mod guard;
mod images;
mod keys;
mod meter;
mod overlay;
#[cfg(test)]
mod probes;
mod scrollbar;
mod session;
mod talents;
#[cfg(test)]
mod testkit;
mod theme;
mod window;

use std::cell::RefCell;
use std::process::ExitCode;
use std::rc::Rc;

use wowdps_gui_logic::config;
use wowdps_gui_logic::sibling::daemon_bin;
use wowdps_gui_logic::theme::Chrome;
use wowdps_proto::{ClientKind, DaemonClient};

const USAGE: &str = "\
wowdps-gui - a damage meter window for World of Warcraft combat logs

Usage:
  wowdps-gui             meter window (starts the wowdps daemon if needed)
  wowdps-gui --overlay   pin an edge tab over the game (wlr-layer-shell);
                         click it to expand the meter, drag it along the edge
                         (on Hyprland: onto another edge). Under Hyprland it
                         is born on the game's monitor and follows the game:
                         it hides whenever WoW's workspace is off screen
                         (follow_game = false in the config disables this;
                         game_match sets the window class/title substring to
                         look for)
  wowdps-gui --help      show this message

The GUI is a client: the wowdps daemon owns the log. To meter a specific file
or directory, point the daemon at it (`wowdps daemon --file <path>`, or
`logs_dir` in the config) — the GUI takes no source flags.

Window keys are the TUI's: j/k move (the inspector follows), enter inspects,
esc backs out, [ ] step pulls, d h T K i c x E pick the view, tab swaps the
inspector's lists, v compares, q quits. Ctrl+K jumps to any pull, player,
view or screen; ? lists every key. Ctrl+= / Ctrl+- / Ctrl+0 zoom. Rows, the
pull rail and the ribbon respond to the mouse.

Configuration lives in ~/.config/wowdps/config.toml (zoom for both; edge,
offset, panel size, monitor, follow_game, game_match for the overlay;
theme = \"gold\" or \"frost\", chrome = \"gold\" or \"class\", density,
home_on_start for the window) and is updated when you drag the tab or zoom.";

fn main() -> ExitCode {
    let mut overlay = false;
    for arg in std::env::args().skip(1) {
        match arg.as_str() {
            "--overlay" => overlay = true,
            "-h" | "--help" => {
                println!("{USAGE}");
                return ExitCode::SUCCESS;
            }
            other => {
                eprintln!("wowdps-gui: unknown argument {other:?}\n\n{USAGE}");
                return ExitCode::from(2);
            }
        }
    }

    let cfg = config::Config::load();
    // The overlay's output, chosen before the app starts: under Hyprland it
    // may wait for the game window to map, a wait no frame should take.
    let output = overlay.then(|| overlay::choose_output(&cfg)).flatten();
    // A class chrome wears the class of the character played last, as the
    // config remembers it; none known yet is the neutral accent.
    let chrome = (cfg.chrome() == Chrome::Class)
        .then(|| wowdps_gui_logic::theme::class_accent(cfg.character_class()));
    if overlay {
        // Replace any running overlay, of either GUI, before touching the
        // daemon: the takeover socket is unversioned and shared, so two
        // surfaces never stand at once.
        wowdps_gui_logic::single::claim_overlay(|| {
            eprintln!("wowdps-gui: replaced by a newer overlay, exiting");
            std::process::exit(0);
        });
    }
    // The link is made before the app starts, as the iced GUI's is:
    // `connect` may spawn a daemon and wait for it, a wait the UI thread
    // must never take once frames are drawing.
    let kind = if overlay {
        ClientKind::Overlay
    } else {
        ClientKind::Window
    };
    let client = match DaemonClient::connect(&daemon_bin(), None, kind) {
        Ok(client) => client,
        Err(e) => {
            eprintln!("wowdps-gui: cannot reach the daemon: {e}");
            return ExitCode::FAILURE;
        }
    };

    // `run` returns when the app quits; a surface that never opened is the
    // one failure it has to report.
    let failure = Rc::new(RefCell::new(None::<String>));
    let failed = Rc::clone(&failure);
    gpui_kit::application().run(move |cx| {
        gpui_kit::init(cx);
        keys::bind(cx);
        fonts(cx);
        theme::apply(cfg.theme(), chrome, cx);
        let fail = move |e: String, cx: &mut gpui_kit::App| {
            *failed.borrow_mut() = Some(e);
            cx.quit();
        };
        if overlay {
            overlay::open(cx, output, client, cfg, fail);
        } else if let Err(e) = window::open(client, cx) {
            fail(e, cx);
        }
    });
    match failure.take() {
        Some(e) => {
            eprintln!("wowdps-gui: {e}");
            ExitCode::FAILURE
        }
        None => ExitCode::SUCCESS,
    }
}

/// The bundled faces, registered before the first window opens. A face that
/// fails to load is drawn in GPUI's default instead, so it is said, not
/// fatal.
fn fonts(cx: &mut gpui_kit::App) {
    let faces = wowdps_gui_logic::fonts::FONTS
        .iter()
        .map(|bytes| std::borrow::Cow::Borrowed(*bytes))
        .collect();
    if let Err(e) = cx.text_system().add_fonts(faces) {
        eprintln!("wowdps-gui: the bundled fonts did not load: {e}");
    }
}
