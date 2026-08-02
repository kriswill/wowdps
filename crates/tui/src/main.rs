mod keys;
mod ui;

use std::io::{self, Write};
use std::sync::mpsc::TryRecvError;
use std::time::{Duration, Instant};

use crossterm::event::{self, Event, KeyEventKind};
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use crossterm::{ExecutableCommand, cursor};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;

use wowdps_core::app::{self, App};
use wowdps_core::cli::parse_args;
use wowdps_core::index;
use wowdps_core::tail::{self, SourceSpec};

const USAGE: &str = "\
wowdps - a terminal damage meter for World of Warcraft combat logs

Usage:
  wowdps                 follow the newest WoWCombatLog*.txt in the default logs dir
  wowdps --file <path>   replay a specific log file, then follow it
  wowdps --logs <dir>    follow the newest WoWCombatLog*.txt in <dir>
  wowdps --help          show this message

Keys:
  j k or arrows move the selection        enter  open segment / drill into a player
  d h i c x K   damage / healing / interrupts / crowd control / dispels / deaths
  [ ]           previous / next encounter segment
  tab           swap drilldown pane       esc    back (drilldown, then segment list)
  q             quit

Starts on a list of every encounter in the log (indexed, not replayed); pick one
to load it, or arrive mid-fight and the live meter opens itself.";

/// How long to wait for a key before redrawing anyway (live durations tick).
const TICK: Duration = Duration::from_millis(200);

/// Longest a single frame will spend swallowing tailed lines before it must
/// redraw and look at the keyboard again.
const DRAIN_BUDGET: Duration = Duration::from_millis(25);

fn main() {
    match parse_args(std::env::args().skip(1)) {
        Ok(Some(spec)) => {
            if let Err(e) = run(spec) {
                eprintln!("wowdps: {e}");
                std::process::exit(1);
            }
        }
        Ok(None) => println!("{USAGE}"),
        Err(e) => {
            eprintln!("wowdps: {e}\n\n{USAGE}");
            std::process::exit(2);
        }
    }
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

fn run(spec: SourceSpec) -> io::Result<()> {
    let lines = tail::spawn(spec);
    install_panic_hook();

    let _guard = TerminalGuard::new()?;
    let mut terminal = Terminal::new(CrosstermBackend::new(io::stdout()))?;
    terminal.clear()?;

    let mut app = App::new();
    while !app.quit {
        terminal.draw(|frame| ui::draw(frame, &app))?;

        // Take what the reader thread has ready, but only for a slice of a
        // frame. Replaying a large log produces lines faster than we consume
        // them, and an unbounded drain here would starve the redraw and the
        // keyboard until the whole file had been read.
        let deadline = Instant::now() + DRAIN_BUDGET;
        loop {
            match lines.try_recv() {
                Ok(event) => app.on_tail(event),
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    app.status = Some("log reader stopped".to_string());
                    break;
                }
            }
            if Instant::now() >= deadline {
                break;
            }
        }

        if event::poll(TICK)?
            && let Event::Key(key) = event::read()?
            && key.kind == KeyEventKind::Press
            && let Some(action) = keys::action_for(key)
        {
            app.apply(action);
            service_loads(&mut terminal, &mut app)?;
        }
    }
    Ok(())
}

/// Lazily parse the indexed segment the user just navigated to. Synchronous:
/// a boss pull is a few MB of slice, well under a redraw's worth of patience —
/// but a "loading" frame goes up first so the wait is never a mystery.
fn service_loads(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    app: &mut App,
) -> io::Result<()> {
    while let Some((pos, meta)) = app.load_request() {
        let Some(path) = app.source_path.clone() else {
            app.load_failed("no log file to load from".to_string());
            break;
        };
        app.status = Some(format!("loading {}…", meta.name));
        terminal.draw(|frame| ui::draw(frame, app))?;
        match index::load_segment(&path, &meta) {
            Ok(lines) => {
                app.status = None;
                app.install_loaded(pos, app::meter_from_lines(lines.iter().map(String::as_str)));
            }
            Err(e) => {
                app.load_failed(format!("{}: {e}", path.display()));
                break;
            }
        }
    }
    Ok(())
}

