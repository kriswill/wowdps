//! gui-new: the window and the overlay rebuilt on Zed's GPUI through GPUI
//! Kit (`docs/spec-gui-new.md`), beside the iced `wowdps-gui` until a
//! measured cutover.
//!
//! A pure client like the iced GUI: `wowdps-model`, `wowdps-proto` and
//! `wowdps-gui-logic`, never the engine, so it cannot open a combat log.
//! This is phase 1's skeleton (`docs/plan-gui-new.md` step 1.1): the window
//! shows the daemon's `Status`, and `--overlay` opens an empty layer
//! surface. The daemon keeps spawning `wowdps-gui` as the overlay unless
//! config `gui_binary` names this binary.

mod images;
mod meter;
mod overlay;
#[cfg(test)]
mod probes;
mod session;
#[cfg(test)]
mod testkit;
mod window;

use std::cell::RefCell;
use std::process::ExitCode;
use std::rc::Rc;

use wowdps_gui_logic::sibling::daemon_bin;
use wowdps_proto::{ClientKind, DaemonClient};

const USAGE: &str = "\
wowdps-gui-new - the wowdps GUI on GPUI Kit (in development)

Usage:
  wowdps-gui-new             a window showing the daemon's status (starts
                             the wowdps daemon if needed)
  wowdps-gui-new --overlay   an empty layer surface over the screen
                             (wlr-layer-shell; close it with its process)
  wowdps-gui-new --help      show this message

The GUI you use is still wowdps-gui; this one grows beside it.";

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
                eprintln!("wowdps-gui-new: unknown argument {other:?}\n\n{USAGE}");
                return ExitCode::from(2);
            }
        }
    }

    // The window's link is made before the app starts, as the iced window's
    // is: `connect` may spawn a daemon and wait for it, a wait the UI
    // thread must never take once frames are drawing. The empty overlay
    // has no link: a session of the Overlay kind would tell the daemon's
    // supervisor that an overlay is up.
    let client = if overlay {
        None
    } else {
        match DaemonClient::connect(&daemon_bin(), None, ClientKind::Window) {
            Ok(client) => Some(client),
            Err(e) => {
                eprintln!("wowdps-gui-new: cannot reach the daemon: {e}");
                return ExitCode::FAILURE;
            }
        }
    };

    // `run` returns when the app quits; a surface that never opened is the
    // one failure it has to report.
    let failure = Rc::new(RefCell::new(None::<String>));
    let failed = Rc::clone(&failure);
    gpui_kit::application().run(move |cx| {
        gpui_kit::init(cx);
        let opened = match client {
            Some(client) => window::open(client, cx),
            None => overlay::open(cx),
        };
        if let Err(e) = opened {
            *failed.borrow_mut() = Some(e.to_string());
            cx.quit();
        }
    });
    match failure.take() {
        Some(e) => {
            eprintln!("wowdps-gui-new: {e}");
            ExitCode::FAILURE
        }
        None => ExitCode::SUCCESS,
    }
}
