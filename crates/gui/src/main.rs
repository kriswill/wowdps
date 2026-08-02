//! iced frontend: the same core state machine as the TUI, drawn either in a
//! regular window (default) or as a wlr-layer-shell overlay tab (`--overlay`)
//! that pins to a screen edge above the game.

mod config;
mod hypr;
mod keys;
mod overlay;
mod view;
mod window;

use wowdps_core::cli::parse_args;

const USAGE: &str = "\
wowdps-gui - a damage meter window for World of Warcraft combat logs

Usage:
  wowdps-gui                 follow the newest WoWCombatLog*.txt in the default logs dir
  wowdps-gui --file <path>   replay a specific log file, then follow it
  wowdps-gui --logs <dir>    follow the newest WoWCombatLog*.txt in <dir>
  wowdps-gui --overlay       pin an edge tab over the game (wlr-layer-shell);
                             click it to expand the meter, drag it along the edge
                             (on Hyprland: around the whole screen perimeter).
                             Under Hyprland it follows the game: it hides whenever
                             WoW's workspace is off screen (follow_game = false
                             in the config disables this; game_match sets the
                             window class/title substring to look for)
  wowdps-gui --help          show this message

Window keys are the TUI's: j/k move, enter opens, esc backs out, [ ] switch
segment, d h i c x K pick the view, tab swaps drilldown panes, q quits.
Ctrl+= / Ctrl+- / Ctrl+0 zoom. Rows and the segment list respond to the mouse.

Configuration lives in ~/.config/wowdps/config.toml (edge, offset, panel size,
zoom, monitor, follow_game, game_match) and is updated when you drag the tab
or zoom.";

fn main() {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    let overlay = args.iter().any(|a| a == "--overlay");
    args.retain(|a| a != "--overlay");

    let spec = match parse_args(args) {
        Ok(Some(spec)) => spec,
        Ok(None) => {
            println!("{USAGE}");
            return;
        }
        Err(e) => {
            eprintln!("wowdps-gui: {e}\n\n{USAGE}");
            std::process::exit(2);
        }
    };

    let cfg = config::Config::load();
    let result = if overlay {
        overlay::run(spec, cfg).map_err(|e| e.to_string())
    } else {
        window::run(spec, cfg).map_err(|e| e.to_string())
    };
    if let Err(e) = result {
        eprintln!("wowdps-gui: {e}");
        std::process::exit(1);
    }
}
