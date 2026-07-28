mod app;
mod model;
mod parser;
mod stub;
mod tail;
mod ui;

use std::io::{self, Write};
use std::path::PathBuf;
use std::sync::mpsc::TryRecvError;
use std::time::Duration;

use crossterm::event::{self, Event, KeyEventKind};
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use crossterm::{ExecutableCommand, cursor};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;

use app::App;
use tail::SourceSpec;

/// Verified location of the retail WoW logs under this machine's Proton prefix.
const DEFAULT_LOGS_DIR: &str = "/home/k/.local/share/Steam/steamapps/compatdata/3082075026/pfx/drive_c/Program Files (x86)/World of Warcraft/_retail_/Logs";

const USAGE: &str = "\
wowdps - a terminal damage meter for World of Warcraft combat logs

Usage:
  wowdps                 follow the newest WoWCombatLog*.txt in the default logs dir
  wowdps --file <path>   replay a specific log file, then follow it
  wowdps --logs <dir>    follow the newest WoWCombatLog*.txt in <dir>
  wowdps --help          show this message

Keys:
  d h i c x K   damage / healing / interrupts / crowd control / dispels / deaths
  [ ]           previous / next encounter segment
  j k or arrows move the selection        enter  drill into the selected player
  tab           swap drilldown pane       esc    leave the drilldown
  q             quit";

/// How long to wait for a key before redrawing anyway (live durations tick).
const TICK: Duration = Duration::from_millis(200);

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

/// `Ok(None)` means "help was requested, don't start". Hand-rolled so the only
/// dependencies stay ratatui + crossterm.
fn parse_args<I: IntoIterator<Item = String>>(args: I) -> Result<Option<SourceSpec>, String> {
    let mut file: Option<PathBuf> = None;
    let mut logs: Option<PathBuf> = None;
    let mut args = args.into_iter();

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "-h" | "--help" => return Ok(None),
            // Paths are taken verbatim: the default logs dir contains spaces.
            "-f" | "--file" => {
                file = Some(PathBuf::from(args.next().ok_or("--file needs a path")?));
            }
            "-l" | "--logs" => {
                logs = Some(PathBuf::from(
                    args.next().ok_or("--logs needs a directory")?,
                ));
            }
            other => return Err(format!("unknown argument {other:?}")),
        }
    }

    match (file, logs) {
        (Some(_), Some(_)) => Err("--file and --logs are mutually exclusive".to_string()),
        (Some(f), None) => Ok(Some(SourceSpec::File(f))),
        (None, Some(d)) => Ok(Some(SourceSpec::Dir(d))),
        (None, None) => Ok(Some(SourceSpec::Dir(PathBuf::from(DEFAULT_LOGS_DIR)))),
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

        // Drain everything the reader thread has ready; never block on it.
        loop {
            match lines.try_recv() {
                Ok(event) => app.on_tail(event),
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    app.status = Some("log reader stopped".to_string());
                    break;
                }
            }
        }

        if event::poll(TICK)?
            && let Event::Key(key) = event::read()?
            && key.kind == KeyEventKind::Press
            && let Some(action) = app::action_for(key)
        {
            app.apply(action);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(args: &[&str]) -> Result<Option<SourceSpec>, String> {
        parse_args(args.iter().map(|s| s.to_string()))
    }

    #[test]
    fn no_arguments_follows_the_default_logs_directory() {
        assert_eq!(
            parse(&[]),
            Ok(Some(SourceSpec::Dir(PathBuf::from(DEFAULT_LOGS_DIR))))
        );
    }

    #[test]
    fn file_and_logs_select_their_sources() {
        assert_eq!(
            parse(&["--file", "/tmp/a.txt"]),
            Ok(Some(SourceSpec::File(PathBuf::from("/tmp/a.txt"))))
        );
        assert_eq!(
            parse(&["--logs", "/tmp/logs"]),
            Ok(Some(SourceSpec::Dir(PathBuf::from("/tmp/logs"))))
        );
        assert_eq!(
            parse(&["-f", "/tmp/a.txt"]),
            Ok(Some(SourceSpec::File(PathBuf::from("/tmp/a.txt"))))
        );
    }

    #[test]
    fn paths_with_spaces_survive_intact() {
        let path = "/home/k/Program Files (x86)/World of Warcraft/_retail_/Logs";
        assert_eq!(
            parse(&["--logs", path]),
            Ok(Some(SourceSpec::Dir(PathBuf::from(path))))
        );
    }

    #[test]
    fn the_default_logs_directory_is_the_proton_wow_path() {
        assert!(DEFAULT_LOGS_DIR.ends_with("/World of Warcraft/_retail_/Logs"));
    }

    #[test]
    fn help_asks_for_no_source() {
        assert_eq!(parse(&["--help"]), Ok(None));
        assert_eq!(parse(&["-h"]), Ok(None));
    }

    #[test]
    fn conflicting_and_incomplete_arguments_are_rejected() {
        assert!(parse(&["--file", "a", "--logs", "b"]).is_err());
        assert!(parse(&["--file"]).is_err());
        assert!(parse(&["--logs"]).is_err());
        assert!(parse(&["--nonsense"]).is_err());
        assert!(parse(&["stray"]).is_err());
    }

    #[test]
    fn the_last_source_flag_wins_over_an_earlier_one_of_the_same_kind() {
        assert_eq!(
            parse(&["--file", "a", "--file", "b"]),
            Ok(Some(SourceSpec::File(PathBuf::from("b"))))
        );
    }
}
