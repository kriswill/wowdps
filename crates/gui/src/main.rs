//! iced frontend: a pure rendering client of the wowdps daemon, drawn either
//! in a regular window (default) or as a wlr-layer-shell overlay tab
//! (`--overlay`) that pins to a screen edge above the game.
//!
//! This binary depends on `wowdps-model` and `wowdps-proto` only: it cannot
//! open a combat log or parse a line even by accident. What to tail is the
//! daemon's decision (config `logs_dir`, or `wowdps --daemon --file …`).

mod config;
mod hypr;
mod keys;
mod overlay;
mod single;
mod timeline;
mod view;
mod window;

use std::path::PathBuf;

const USAGE: &str = "\
wowdps-gui - a damage meter window for World of Warcraft combat logs

Usage:
  wowdps-gui             meter window (starts the wowdps daemon if needed)
  wowdps-gui --overlay   pin an edge tab over the game (wlr-layer-shell);
                         click it to expand the meter, drag it along the edge
                         (on Hyprland: around the whole screen perimeter).
                         Under Hyprland it follows the game: it hides whenever
                         WoW's workspace is off screen (follow_game = false
                         in the config disables this; game_match sets the
                         window class/title substring to look for)
  wowdps-gui --help      show this message

The GUI is a client: the wowdps daemon owns the log. To meter a specific file
or directory, point the daemon at it (`wowdps --daemon --file <path>`, or
`logs_dir` in the config) — the GUI takes no source flags.

Window keys are the TUI's: j/k move, enter opens, esc backs out, [ ] switch
segment, d h i c x K pick the view, tab swaps drilldown panes, q quits.
Ctrl+= / Ctrl+- / Ctrl+0 zoom. Rows and the segment list respond to the mouse.

Configuration lives in ~/.config/wowdps/config.toml (edge, offset, panel size,
zoom, monitor, follow_game, game_match) and is updated when you drag the tab
or zoom.";

fn main() {
    let mut overlay = false;
    for arg in std::env::args().skip(1) {
        match arg.as_str() {
            "--overlay" => overlay = true,
            "-h" | "--help" => {
                println!("{USAGE}");
                return;
            }
            other => {
                eprintln!("wowdps-gui: unknown argument {other:?}\n\n{USAGE}");
                std::process::exit(2);
            }
        }
    }

    let cfg = config::Config::load();
    let result = if overlay {
        overlay::run(cfg).map_err(|e| e.to_string())
    } else {
        window::run(cfg).map_err(|e| e.to_string())
    };
    if let Err(e) = result {
        eprintln!("wowdps-gui: {e}");
        std::process::exit(1);
    }
}

/// The daemon binary to spawn when none is running: the sibling `wowdps`
/// from the same build, else whatever PATH resolves.
pub(crate) fn daemon_bin() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.join("wowdps")))
        .filter(|p| p.exists())
        .unwrap_or_else(|| PathBuf::from("wowdps"))
}
