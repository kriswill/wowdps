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
use wowdps_model::Edition;
use wowdps_proto::{
    ClientKind, ClientMsg, ClientState, DaemonClient, DaemonMsg, EditionRef, Reconnect, SourceArg,
};

const USAGE: &str = "\
wowdps - a terminal damage meter for World of Warcraft combat logs

Usage:
  wowdps                 TUI client (starts the daemon if none is running)
  wowdps --file <path>   ...with the daemon replaying/following one log file
  wowdps --logs <dir>    ...with the daemon following the newest log in <dir>
  wowdps --edition <e>   ...showing one edition's logs, retail or forever,
                         when the daemon follows them (an install's are both
                         followed; default: the live one)
  wowdps gui             start the daemon if needed, launch wowdps-gui, exit
  wowdps daemon          run the daemon in the foreground (systemd target)
          [--linger]     ...and never idle-exit
          [--file|--logs] override the config's logs_dir
  wowdps status          report the running daemon's state: each edition's
                         source, the log it follows and which is live
  wowdps stop            shut the daemon down
  wowdps addon           report the wowdps addon in the game's AddOns folder
  wowdps addon install   write it there (the daemon keeps it current after
                         that; guilds reach the history store on logout)
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
        Cmd::Addon { install } => do_addon(install),
        Cmd::Gui { source } => launch_gui(source),
        Cmd::Tui { source, edition } => run_tui(source, edition),
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
            history,
            editions,
            ..
        }) => {
            println!("wowdps daemon: running");
            for line in source_lines(source.as_deref(), &editions) {
                println!("{line}");
            }
            println!("  clients: {clients}");
            println!("  game:    {}", game_line(game_running, &editions));
            println!("  linger:  {}", if linger { "yes" } else { "no" });
            println!("  overlay: {overlay:?}");
            let mut h = if history.enabled {
                format!("{} fights", history.fights)
            } else {
                "disabled".to_string()
            };
            if history.importing > 0 {
                h.push_str(&format!(", importing {}", history.importing));
            }
            if history.dropped > 0 {
                h.push_str(&format!(", {} dropped", history.dropped));
            }
            if history.owner_inferred {
                h.push_str(", owner inferred");
            }
            if let Some(e) = &history.error {
                h.push_str(&format!(" ({e})"));
            }
            println!("  history: {h}");
            if history.enabled {
                let mut a = match &history.addon {
                    Some(v) => format!("installed, version {v}"),
                    None => "not installed (wowdps addon install)".to_string(),
                };
                a.push_str(&format!("; {} affiliations", history.affiliations));
                if let Some(t) = history.affiliations_utc_ms {
                    a.push_str(&format!(", newest seen {}", utc_date(t)));
                }
                println!("  addon:   {a}");
            }
            0
        }
        _ => {
            println!("wowdps: daemon did not answer");
            1
        }
    }
}

/// v49: `wowdps status`'s lines for what the daemon follows, a pair per
/// edition's source: the folder or file followed, then its edition, whether
/// it is the live one (what a following client is shown) and the log it
/// follows now —
///
/// ```text
///   source:  dir:/…/_retail_/Logs
///   edition: Retail (live), WoWCombatLog-….txt
/// ```
///
/// The first pair is the daemon's first source, as `Status::source` names
/// it: a daemon of one source prints that pair alone, as it always has. An
/// edition its log has not named yet reads "not named yet"; no log reads
/// "no log yet".
fn source_lines(first: Option<&str>, editions: &[wowdps_proto::EditionSource]) -> Vec<String> {
    let source = |s: Option<&str>| format!("  source:  {}", s.unwrap_or("(none)"));
    let mut out = vec![source(first)];
    for (i, e) in editions.iter().enumerate() {
        if i > 0 {
            out.push(source(e.source.as_deref()));
        }
        let name = e.edition.map_or("not named yet", |e| e.label());
        let live = if e.live { " (live)" } else { "" };
        let log = e.log.as_deref().unwrap_or("no log yet");
        out.push(format!("  edition: {name}{live}, {log}"));
    }
    out
}

/// `wowdps status`'s game line: `running` or `not running`, and — with
/// more than one edition's source listed — the editions whose game runs:
/// `running (Forever)`.
fn game_line(game_running: bool, editions: &[wowdps_proto::EditionSource]) -> String {
    if !game_running {
        return "not running".to_string();
    }
    let named: Vec<&str> = editions
        .iter()
        .filter(|e| e.running)
        .map(|e| e.edition.map_or("not named yet", |e| e.label()))
        .collect();
    if editions.len() < 2 || named.is_empty() {
        return "running".to_string();
    }
    format!("running ({})", named.join(", "))
}

/// `YYYY-MM-DD` of a UTC epoch in milliseconds, hand-rolled (no chrono).
fn utc_date(ms: i64) -> String {
    let days = ms.div_euclid(86_400_000);
    // Howard Hinnant's civil-from-days.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!("{y:04}-{m:02}-{d:02}")
}

// ---- addon ------------------------------------------------------------------

/// `wowdps addon [status | install]`: the game's product directory comes
/// from the configured (or discovered) logs directory — the same place the
/// daemon tails — so the addon always lands beside the logs being read.
fn do_addon(install: bool) -> i32 {
    use wowdps_daemon::addon::{self, AddonState};
    let cfg = Config::load();
    let Some(logs) = cfg
        .logs_dir
        .clone()
        .or_else(wowdps_core::cli::default_logs_dir)
    else {
        eprintln!(
            "wowdps: no logs_dir configured and no WoW install found — set logs_dir in \
             ~/.config/wowdps/config.toml or WOWDPS_WOW_DIR"
        );
        return 1;
    };
    let Some(product) = addon::product_dir(&logs) else {
        eprintln!(
            "wowdps: {} is not a WoW install's Logs directory, so there is no AddOns \
             folder to write to",
            logs.display()
        );
        return 1;
    };
    println!("install: {}", product.display());
    println!("addon:   {}", addon::addon_dir(&product).display());
    match addon::interface_version(&product) {
        Some(i) => println!("game:    interface {i}"),
        None => println!("game:    version unknown (.build.info unreadable)"),
    }
    if install {
        match addon::install(&product) {
            Ok(v) => println!("state:   installed version {v}"),
            Err(e) => {
                eprintln!("wowdps: installing the addon failed: {e}");
                return 1;
            }
        }
    } else {
        match addon::inspect(&product) {
            AddonState::Missing => println!("state:   not installed (wowdps addon install)"),
            AddonState::Current(v) => println!("state:   installed, version {v}, current"),
            AddonState::Stale(v) => println!(
                "state:   installed, version {v}, out of date (the daemon rewrites it on \
                 start; or wowdps addon install)"
            ),
        }
    }
    let files = addon::saved_variables(&product);
    if files.is_empty() {
        println!("saved:   no account has written WOWDPS_DATA yet (it lands on logout)");
    }
    for (account, path) in files {
        let seen = std::fs::metadata(&path)
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map_or_else(|| "?".to_string(), |d| utc_date(d.as_millis() as i64));
        let records = std::fs::read_to_string(&path)
            .ok()
            .and_then(|t| wowdps_proto::history::addon_table(&t).ok())
            .map_or_else(
                || "unreadable".to_string(),
                |data| {
                    let players = data.as_ref().map_or(0, |d| {
                        wowdps_proto::history::Affiliation::from_addon_table(d, &account).len()
                    });
                    let creatures = data.as_ref().map_or(0, |d| {
                        wowdps_proto::creatures::Creature::from_addon_table(d).len()
                    });
                    format!("{players} players, {creatures} creatures")
                },
            );
        println!("saved:   {account}: {records}, written {seen}");
    }
    0
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

fn run_tui(source: Option<SourceSpec>, edition: Option<Edition>) -> i32 {
    let client = match connect(source_arg(&source)) {
        Ok(client) => client,
        Err(e) => {
            eprintln!("wowdps: {e}");
            return 1;
        }
    };
    if let Err(e) = run(client, tui_state(edition)) {
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

/// The TUI's state before its first request: following the live edition,
/// or — `--edition` — pinned to one, whichever runs. The pin is kept for
/// the session, reconnects included (every Watch names it); the header
/// names the edition each answer carries.
fn tui_state(edition: Option<Edition>) -> ClientState {
    let mut state = ClientState::new();
    if let Some(e) = edition {
        // Nothing is sent yet: `initial_request` declares the pin.
        let _ = state.set_edition(EditionRef::Pin(e));
    }
    state
}

fn run(mut client: DaemonClient, mut state: ClientState) -> io::Result<()> {
    install_panic_hook();

    let _guard = TerminalGuard::new()?;
    let mut terminal = Terminal::new(CrosstermBackend::new(io::stdout()))?;
    terminal.clear()?;

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
            // Non-blocking, spawn-throttled: the frame loop keeps drawing.
            match client.try_reconnect() {
                Reconnect::Connected => {
                    state.status = None;
                    client.send(&state.initial_request());
                }
                Reconnect::Spawned => {
                    state.status = Some("daemon gone — starting one…".to_string());
                }
                Reconnect::Waiting => {
                    state.status = Some("daemon gone — reconnecting…".to_string());
                }
                Reconnect::Failed(e) => state.status = Some(format!("daemon gone — {e}")),
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

#[cfg(test)]
mod tests {
    use super::{game_line, source_lines, tui_state};
    use wowdps_model::Edition;
    use wowdps_proto::{ClientMsg, Cursor, EditionRef, EditionSource};

    fn retail() -> EditionSource {
        EditionSource {
            edition: Some(Edition::Retail),
            source: Some("dir:/wow/_retail_/Logs".to_string()),
            log: Some("WoWCombatLog-110126_200000.txt".to_string()),
            live: true,
            running: false,
        }
    }

    /// A daemon of one source prints the source and its edition's line,
    /// as `wowdps status` always has.
    #[test]
    fn status_of_one_source_reads_as_it_did() {
        let one = [retail()];
        assert_eq!(
            source_lines(Some("dir:/wow/_retail_/Logs"), &one),
            [
                "  source:  dir:/wow/_retail_/Logs",
                "  edition: Retail (live), WoWCombatLog-110126_200000.txt",
            ]
        );
        assert_eq!(game_line(false, &one), "not running");
        let running = [EditionSource {
            running: true,
            ..retail()
        }];
        assert_eq!(game_line(true, &running), "running");
        // A daemon listing no source still names its own.
        assert_eq!(source_lines(None, &[]), ["  source:  (none)"]);
    }

    /// v49: with a source per edition, each is listed with its edition,
    /// its log and which is live, and the game line names whose game runs.
    /// An edition its log has not named yet, or a source with no log yet,
    /// says so.
    #[test]
    fn status_lists_each_editions_source_its_log_and_the_live_one() {
        let several = [
            EditionSource {
                live: false,
                ..retail()
            },
            EditionSource {
                edition: Some(Edition::Forever),
                source: Some("dir:/wow/_forever_/Logs".to_string()),
                log: Some("WoWCombatLog-110126_210000.txt".to_string()),
                live: true,
                running: true,
            },
            EditionSource::default(),
        ];
        assert_eq!(
            source_lines(Some("dir:/wow/_retail_/Logs"), &several),
            [
                "  source:  dir:/wow/_retail_/Logs",
                "  edition: Retail, WoWCombatLog-110126_200000.txt",
                "  source:  dir:/wow/_forever_/Logs",
                "  edition: Forever (live), WoWCombatLog-110126_210000.txt",
                "  source:  (none)",
                "  edition: not named yet, no log yet",
            ]
        );
        assert_eq!(game_line(true, &several), "running (Forever)");
        assert_eq!(game_line(false, &several), "not running");
        // The watcher saw a game it could not place in a source.
        let unplaced = [retail(), EditionSource::default()];
        assert_eq!(game_line(true, &unplaced), "running");
    }

    /// `--edition` pins the session before its first request, so the first
    /// Watch already looks in that edition; without it the TUI follows.
    #[test]
    fn the_edition_flag_pins_the_first_watch() {
        let watch_edition = |msg: ClientMsg| match msg {
            ClientMsg::Watch {
                cursor: Cursor::List,
                edition,
            } => edition,
            other => panic!("not a list Watch: {other:?}"),
        };
        let pinned = tui_state(Some(Edition::Forever));
        assert_eq!(pinned.edition_ref(), EditionRef::Pin(Edition::Forever));
        assert_eq!(pinned.edition(), Some(Edition::Forever));
        assert_eq!(
            watch_edition(pinned.initial_request()),
            EditionRef::Pin(Edition::Forever)
        );
        let following = tui_state(None);
        assert_eq!(following.edition(), None);
        assert_eq!(
            watch_edition(following.initial_request()),
            EditionRef::Follow
        );
    }
}
