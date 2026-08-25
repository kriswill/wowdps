//! `wowdps`: the daemon, the launcher, and the TUI client — one binary.
//!
//! The TUI is a pure rendering client: it never opens the log, never parses a
//! line. All state that matters lives in the daemon; this file connects,
//! declares a cursor, and turns snapshots into frames.

mod keys;
mod ui;

use std::io::{self, Write as _};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use crossterm::event::{self, Event, KeyEventKind};
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use crossterm::{ExecutableCommand, cursor};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;

use wowdps_core::cli::{Cmd, SourceSpec, parse_args};
use wowdps_daemon::{DaemonOptions, config::Config, spec_display};
use wowdps_proto::{ClientKind, ClientMsg, ClientState, DaemonClient, DaemonMsg, SourceArg};

const USAGE: &str = "\
wowdps - a terminal damage meter for World of Warcraft combat logs

Usage:
  wowdps                 TUI client (starts the daemon if none is running)
  wowdps --file <path>   ...with the daemon replaying/following one log file
  wowdps --logs <dir>    ...with the daemon following the newest log in <dir>
  wowdps gui             start the daemon if needed, launch wowdps-gui, exit
  wowdps daemon          run the daemon in the foreground (systemd target)
          [--linger]     ...and never idle-exit
          [--file|--logs] override the config's logs_dir
  wowdps status          report the running daemon's state
  wowdps stop            shut the daemon down
  wowdps help            show this message
  wowdps <cmd> [args..]  run wowdps-<cmd> from beside this binary or $PATH
                         (e.g. `wowdps extract ...` runs wowdps-extract)

Keys:
  j k or arrows move the selection        enter  open segment / drill into a player
  d h i c x K   damage / healing / interrupts / crowd control / dispels / deaths
  [ ]           previous / next encounter segment
  tab           swap drilldown pane       esc    back (drilldown, then segment list)
  q             quit

Starts on a list of every encounter in the log; pick one to load it, or arrive
mid-fight and the live meter opens itself. The daemon keeps running (and can
auto-manage the overlay) after the TUI exits.";

/// How long to wait for a key before redrawing anyway (live durations tick).
const TICK: Duration = Duration::from_millis(200);

fn main() {
    let cmd = match parse_args(std::env::args().skip(1)) {
        Ok(Some(cmd)) => cmd,
        Ok(None) => {
            println!("{USAGE}");
            return;
        }
        Err(e) => {
            eprintln!("wowdps: {e}\n\n{USAGE}");
            std::process::exit(2);
        }
    };

    let code = match cmd {
        Cmd::Daemon { source, linger } => run_daemon(source, linger),
        Cmd::Stop => do_stop(),
        Cmd::Status => do_status(),
        Cmd::Gui { source } => launch_gui(source),
        Cmd::Tui { source } => run_tui(source),
        Cmd::External { name, args } => run_external(&name, args),
    };
    std::process::exit(code);
}

/// Git-style external dispatch: `wowdps foo ...` execs `wowdps-foo ...`,
/// preferring a sibling of this binary (same build) over $PATH.
fn run_external(name: &str, args: Vec<String>) -> i32 {
    use std::os::unix::process::CommandExt as _;
    let bin = format!("wowdps-{name}");
    let e = std::process::Command::new(find_bin(&bin)).args(args).exec();
    if e.kind() == io::ErrorKind::NotFound {
        eprintln!("wowdps: '{name}' is not a wowdps command (no {bin} found)\n\n{USAGE}");
    } else {
        eprintln!("wowdps: running {bin} failed: {e}");
    }
    2
}

/// A sibling of the running binary when one exists there, else the bare name
/// for $PATH lookup.
fn find_bin(name: &str) -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.join(name)))
        .filter(|p| p.exists())
        .unwrap_or_else(|| PathBuf::from(name))
}

fn source_arg(source: &Option<SourceSpec>) -> Option<SourceArg> {
    match source {
        Some(SourceSpec::File(p)) => Some(SourceArg::File(p.clone())),
        Some(SourceSpec::Dir(d)) => Some(SourceArg::Logs(d.clone())),
        None => None,
    }
}

// ---- daemon mode ------------------------------------------------------------

/// Foreground daemon (what systemd runs and what `ensure_daemon` spawns
/// detached). Stdio may be null, so anything worth knowing goes to the log
/// file too.
fn run_daemon(source: Option<SourceSpec>, linger: bool) -> i32 {
    let cfg = Config::load();
    let opts = match DaemonOptions::production(&cfg, source, linger) {
        Ok(opts) => opts,
        Err(e) => {
            eprintln!("wowdps: daemon setup failed: {e}");
            daemon_log(&format!("setup failed: {e}"));
            return 1;
        }
    };
    daemon_log(&format!("starting on {}", spec_display(&opts.source)));
    match wowdps_daemon::run(opts) {
        Ok(()) => {
            daemon_log("clean exit");
            0
        }
        Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
            // Another daemon already owns the socket: not an error for a
            // detached self-spawn race, but say so when run by hand.
            eprintln!("wowdps: a daemon is already running");
            0
        }
        Err(e) => {
            eprintln!("wowdps: daemon failed: {e}");
            daemon_log(&format!("failed: {e}"));
            1
        }
    }
}

/// Append one line to `$XDG_STATE_HOME/wowdps/daemon.log` — the only trace a
/// null-stdio daemon leaves.
fn daemon_log(msg: &str) {
    let base = std::env::var_os("XDG_STATE_HOME")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/state")));
    let Some(dir) = base.map(|b| b.join("wowdps")) else {
        return;
    };
    if std::fs::create_dir_all(&dir).is_err() {
        return;
    }
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(dir.join("daemon.log"))
    {
        let _ = writeln!(f, "[{ts}] {msg}");
    }
}

// ---- stop / status ----------------------------------------------------------

/// `--stop`: exit 0 even if nothing was running — the goal state is "no
/// daemon", and that is the state.
fn do_stop() -> i32 {
    match UnixStream::connect(wowdps_proto::socket_path()) {
        Ok(mut stream) => {
            let _ = stream.write_all(&ClientMsg::Shutdown.encode());
            println!("wowdps: daemon asked to stop");
        }
        Err(_) => println!("wowdps: no daemon running"),
    }
    0
}

fn do_status() -> i32 {
    let Ok(stream) = UnixStream::connect(wowdps_proto::socket_path()) else {
        println!("wowdps: no daemon running");
        return 1;
    };
    let Ok(mut client) = DaemonClient::over(stream, ClientKind::Mcp) else {
        println!("wowdps: daemon socket exists but the handshake failed");
        return 1;
    };
    client.send(&ClientMsg::GetStatus { req_id: 1 });
    match wait_status(&mut client) {
        Some(DaemonMsg::Status {
            game_running,
            source,
            clients,
            linger,
            overlay,
            ..
        }) => {
            println!("wowdps daemon: running");
            println!("  source:  {}", source.as_deref().unwrap_or("(none)"));
            println!("  clients: {clients}");
            println!(
                "  game:    {}",
                if game_running {
                    "running"
                } else {
                    "not running"
                }
            );
            println!("  linger:  {}", if linger { "yes" } else { "no" });
            println!("  overlay: {overlay:?}");
            0
        }
        _ => {
            println!("wowdps: daemon did not answer");
            1
        }
    }
}

fn wait_status(client: &mut DaemonClient) -> Option<DaemonMsg> {
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        for msg in client.poll() {
            if matches!(msg, DaemonMsg::Status { .. }) {
                return Some(msg);
            }
        }
        if client.is_dead() {
            return None;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    None
}

// ---- gui launcher -----------------------------------------------------------

fn launch_gui(source: Option<SourceSpec>) -> i32 {
    if let Err(e) = connect(source_arg(&source)) {
        eprintln!("wowdps: {e}");
        return 1;
    }
    // Prefer the sibling binary (same build), fall back to PATH.
    let bin = find_bin("wowdps-gui");
    match std::process::Command::new(&bin)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
    {
        Ok(_) => 0,
        Err(e) => {
            eprintln!("wowdps: launching {} failed: {e}", bin.display());
            1
        }
    }
}

// ---- tui client -------------------------------------------------------------

/// Connect to the daemon — verifying source agreement with one that is
/// already running, spawning one otherwise.
fn connect(source: Option<SourceArg>) -> Result<DaemonClient, String> {
    let sock = wowdps_proto::socket_path();
    match UnixStream::connect(&sock) {
        Ok(stream) => {
            let mut client = DaemonClient::over(stream, ClientKind::Tui)
                .map_err(|e| format!("handshake with running daemon failed: {e}"))?;
            if let Some(arg) = &source {
                let want = match arg {
                    SourceArg::File(p) => spec_display(&SourceSpec::File(p.clone())),
                    SourceArg::Logs(d) => spec_display(&SourceSpec::Dir(d.clone())),
                };
                client.send(&ClientMsg::GetStatus { req_id: 0 });
                let got = match wait_status(&mut client) {
                    Some(DaemonMsg::Status { source, .. }) => source,
                    _ => return Err("running daemon did not answer a status query".to_string()),
                };
                if got.as_deref() != Some(want.as_str()) {
                    return Err(format!(
                        "a daemon is already running against {}, not {want};\n\
                         run `wowdps stop` first if you want to switch",
                        got.as_deref().unwrap_or("(unknown)"),
                    ));
                }
            }
            Ok(client)
        }
        Err(_) => {
            let exe = std::env::current_exe()
                .map_err(|e| format!("cannot find my own binary to spawn the daemon: {e}"))?;
            DaemonClient::connect(&exe, source, ClientKind::Tui)
                .map_err(|e| format!("starting the daemon failed: {e}"))
        }
    }
}

fn run_tui(source: Option<SourceSpec>) -> i32 {
    let client = match connect(source_arg(&source)) {
        Ok(client) => client,
        Err(e) => {
            eprintln!("wowdps: {e}");
            return 1;
        }
    };
    if let Err(e) = run(client) {
        eprintln!("wowdps: {e}");
        return 1;
    }
    0
}

/// Puts the terminal into raw mode + alternate screen and — crucially — takes
/// it back out on every exit path, including `?` and panics.
struct TerminalGuard;

impl TerminalGuard {
    fn new() -> io::Result<Self> {
        enable_raw_mode()?;
        io::stdout().execute(EnterAlternateScreen)?;
        io::stdout().execute(cursor::Hide)?;
        Ok(Self)
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        restore_terminal();
    }
}

fn restore_terminal() {
    let mut out = io::stdout();
    let _ = out.execute(cursor::Show);
    let _ = out.execute(LeaveAlternateScreen);
    let _ = disable_raw_mode();
    let _ = out.flush();
}

/// A panic must never leave the user staring at a raw-mode alternate screen.
fn install_panic_hook() {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        restore_terminal();
        previous(info);
    }));
}

fn run(mut client: DaemonClient) -> io::Result<()> {
    install_panic_hook();

    let _guard = TerminalGuard::new()?;
    let mut terminal = Terminal::new(CrosstermBackend::new(io::stdout()))?;
    terminal.clear()?;

    let mut state = ClientState::new();
    client.send(&state.initial_request());

    while !state.quit {
        terminal.draw(|frame| ui::draw(frame, &state))?;

        // Everything the daemon pushed since the last frame; stale snapshots
        // were already coalesced away in the client library.
        for msg in client.poll() {
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

        if event::poll(TICK)?
            && let Event::Key(key) = event::read()?
            && key.kind == KeyEventKind::Press
            && let Some(action) = keys::action_for(key)
        {
            for req in state.apply(action) {
                client.send(&req);
            }
        }
    }
    Ok(())
}
